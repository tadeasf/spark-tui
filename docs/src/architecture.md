# Architecture

spark-tui follows a modular architecture with clear separation between configuration, data fetching, analysis, and rendering.

## Module Map

```
src/
├── main.rs              Entry point: config → client → poller → app loop
├── config.rs            CLI args, env vars, ~/.databrickscfg parsing
├── fetch/
│   ├── client.rs        SparkHttpClient + FetchError
│   ├── spark.rs         Endpoint methods (get_jobs, get_stages, etc.)
│   ├── types.rs         Spark API response types (serde)
│   ├── databricks.rs    DatabricksClient (cluster info, DBFS, sparkui, history server)
│   ├── poller.rs        Background polling loop + historical fallback chain
│   └── eventlog/        Event log parsing (DBFS download, gzip, SparkEvent serde)
├── analyze/
│   ├── types.rs         Suspect, Severity, SuspectCategory, BottleneckPattern
│   ├── skew.rs          Data skew detection (CV + max/median)
│   ├── suspects.rs      SuspectContext, 10 detectors, bottleneck classification
│   └── sql_linker.rs    Job ↔ SQL ↔ Stage mapping
├── tui/
│   ├── app.rs           App state, event loop, key handling, rendering
│   ├── theme.rs         Color/style functions
│   ├── tabs/
│   │   ├── jobs.rs      Jobs table + job detail + SQL detail + stage detail views
│   │   └── suspects.rs  Suspects table view
│   └── widgets/
│       ├── bar_chart.rs Duration bar chart for stage comparison
│       ├── help.rs      Help overlay (keybinding reference + SQL recommendations)
│       ├── status_line.rs Status bar (cluster ID, app ID, last update time)
│       └── summary_bar.rs Health summary bar (job/IO counts, top issues)
└── util/
    ├── format.rs        format_duration_ms, format_bytes, truncate, clean_stage_name
    └── time.rs          Spark timestamp parsing, duration_between
```

## Data Flow

```
┌──────────┐     ┌──────────────┐     ┌──────────────┐     ┌───────────┐
│  Config  │────▶│ SparkHttp    │────▶│   Poller     │────▶│  Analysis │
│ resolve  │     │  Client      │     │ (poll_once)  │     │  Engine   │
└──────────┘     └──────────────┘     └──────┬───────┘     └─────┬─────┘
                                             │                   │
                                    DataPayload              Suspects
                                    + stage_sql_hints        (via SuspectContext)
                                    + critical_stages
                                             │                   │
                                             ▼                   ▼
                                     ┌───────────────────────────────┐
                                     │          App (TUI)            │
                                     │  event loop ← mpsc channel   │
                                     └───────────────────────────────┘
```

### Step by step:

1. **Config resolution** (`config.rs`) — parses CLI args, env vars, and `~/.databrickscfg` to produce a `Config` struct with host, token, cluster_id, and poll_interval

2. **HTTP client** (`fetch/client.rs`) — `SparkHttpClient` wraps `reqwest::Client` with the base URL and token. `FetchError` maps HTTP status codes to user-friendly messages

3. **Endpoint methods** (`fetch/spark.rs`) — `discover_app_id`, `get_jobs`, `get_stages`, `get_sql_executions`, `get_task_list`, `get_executors` — each calls the Spark REST API and deserializes the response

4. **Background poller** (`fetch/poller.rs`) — `run_poller` runs in a tokio task, calling `poll_once` on each interval. When the cluster becomes unreachable (503 or terminated), the poller automatically falls back to historical data via a 4-strategy chain: Spark UI REST API (with warm-up retry), Spark History Server proxy, DBFS event logs, and default DBFS path scanning. `poll_once`:
   - Fetches jobs, stages, SQL executions, and executors concurrently via 4-way `tokio::join!`
   - Aggregates active executors into `ClusterResources` (total memory, cores, executor count)
   - Builds cross-reference maps (job↔SQL, stage↔job)
   - Creates a `SuspectContext` with cross-reference maps
   - Runs 10 stage-level detectors via function pointer table, plus skew detection on task data
   - Fetches task lists for up to ~15 stages (selected by multiple heuristics)
   - Computes `stage_sql_hints` (SQL plan hints per stage) and `critical_stages` (longest wall-clock stage per job)
   - Computes `HealthSummary` for the summary bar
   - Sends a `DataPayload` (including `cluster_resources`, `stage_sql_hints`, `critical_stages`) through an mpsc channel

5. **Analysis** (`analyze/`) — 10 stage-level detectors are dispatched via a function pointer table (`&[DetectorFn]`): `detect_slow_stages`, `detect_spill`, `detect_cpu_efficiency`, `detect_record_explosion`, `detect_task_failures`, `detect_memory_pressure`, `detect_partition_count`, `detect_broadcast_join`, `detect_python_udf`, `detect_cache_opportunity`. Each takes `(&[SparkStage], &SuspectContext)` and returns `Vec<Suspect>`. `detect_skew` runs separately on task data. `aggregate_suspects` sorts by severity then `estimated_savings_ms`

6. **App event loop** (`tui/app.rs`) — `App::run` receives `Action` variants from the mpsc channel:
   - `Action::DataUpdate(payload)` — stores the new data
   - `Action::FetchError(err)` — stores the error message
   - `Action::Key(event)` — processes keybindings
   - `Action::Resize(w, h)` — triggers re-render

7. **Rendering** (`tui/tabs/`, `tui/widgets/`) — renders the current view mode (List, JobDetail, StageDetail, SqlDetail) using ratatui widgets. The summary bar widget displays health metrics in List view

## Async Model

spark-tui uses the **tokio** runtime with three concurrent tasks:

| Task | Channel | Description |
|------|---------|-------------|
| Poller | `tx → rx` | Fetches data and sends `Action::DataUpdate` / `Action::FetchError` |
| Event reader | `tx → rx` | Reads terminal events via `crossterm::event::read` (blocking, wrapped in `spawn_blocking`) |
| App loop | `rx` | Receives all actions and processes them sequentially |

All tasks communicate through a single `mpsc::UnboundedSender<Action>` channel. The app loop owns the receiver and processes actions one at a time, ensuring thread-safe state updates without locks.

## Design Decisions

- **Bounded task fetching**: Task lists (per-task metrics) are fetched for up to ~15 stages selected by multiple heuristics (top-by-runtime, top-by-shuffle, high-parallelism). On-demand task fetching is triggered when entering StageDetail for stages not already analyzed
- **Concurrent fetches**: Jobs, stages, SQL executions, and executors are fetched in parallel with 4-way `tokio::join!` to minimize latency
- **Function pointer dispatch**: Stage-level detectors are stored in a `&[DetectorFn]` array and dispatched via `flat_map`, making it easy to add new detectors
- **SuspectContext**: Replaces ad-hoc parameter passing — all cross-reference maps are bundled in a single struct with helper methods (`job_id`, `resolve_sql`, `resolve_plan_hint_for`, `enrich`)
- **tui-scrollview**: Used for smooth scrolling in StageDetail and SqlDetail views, replacing manual `u16` scroll offsets with `ScrollViewState`
- **Log file**: Logs go to `/tmp/spark-tui.log` instead of stderr to avoid corrupting the TUI
- **Panic hook**: A custom panic hook restores the terminal before printing the panic message, preventing terminal corruption
- **Edition 2024**: Uses the latest Rust edition for modern language features

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing with env var fallback |
| `tokio` | Async runtime (`macros`, `rt-multi-thread`, `time`, `sync` features) |
| `reqwest` | HTTP client (with `rustls-tls`) |
| `serde` / `serde_json` | JSON deserialization |
| `thiserror` | Error type derivation |
| `ratatui` | Terminal UI framework (with `unstable-rendered-line-info` feature) |
| `crossterm` | Terminal backend |
| `tracing` / `tracing-subscriber` | Structured logging |
| `chrono` | Timestamp parsing |
| `syntect` / `syntect-tui` | SQL syntax highlighting |
| `tui-scrollview` | Smooth scrollable views for detail panels |

## CI/CD Workflows

| Workflow | Trigger | Description |
|----------|---------|-------------|
| `ci.yml` | Push / PR | Runs `cargo fmt --check`, `cargo clippy`, `cargo test` |
| `docs.yml` | Push / PR | Builds and deploys mdbook documentation to GitHub Pages |
| `auto-tag.yml` | Push to `master` (Cargo.toml changed) | Creates a `vX.Y.Z` tag when the version in `Cargo.toml` changes |
| `release.yml` | Tag `v*` | Cross-platform release builds (Linux x86_64, macOS x86_64 + aarch64, Windows x86_64) with GitHub Release artifacts |
