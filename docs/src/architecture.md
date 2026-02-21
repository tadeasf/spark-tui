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
│   └── poller.rs        Background polling loop + data aggregation
├── analyze/
│   ├── types.rs         Suspect, Severity, SuspectCategory, BottleneckPattern
│   ├── skew.rs          Data skew detection (CV + max/median)
│   ├── suspects.rs      Slow stage + spill detection, bottleneck classification
│   └── sql_linker.rs    Job ↔ SQL ↔ Stage mapping
├── tui/
│   ├── app.rs           App state, event loop, key handling, rendering
│   ├── theme.rs         Color/style functions
│   ├── tabs/
│   │   ├── jobs.rs      Jobs table + job detail + SQL detail views
│   │   └── suspects.rs  Suspects table view
│   └── widgets/
│       ├── bar_chart.rs Duration bar chart for stage comparison
│       └── status_line.rs Status bar (cluster ID, app ID, last update time)
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

3. **Endpoint methods** (`fetch/spark.rs`) — `discover_app_id`, `get_jobs`, `get_stages`, `get_sql_executions`, `get_task_list` — each calls the Spark REST API and deserializes the response

4. **Background poller** (`fetch/poller.rs`) — `run_poller` runs in a tokio task, calling `poll_once` on each interval. `poll_once`:
   - Fetches jobs, stages, and SQL executions concurrently via `tokio::join!`
   - Builds cross-reference maps (job↔SQL, stage↔job)
   - Runs analysis (slow stages, spill, skew detection)
   - Fetches task lists for the top 5 slowest stages
   - Sends a `DataPayload` through an mpsc channel

5. **Analysis** (`analyze/`) — `detect_slow_stages`, `detect_spill`, and `detect_skew` each produce `Vec<Suspect>`. `aggregate_suspects` sorts by severity

6. **App event loop** (`tui/app.rs`) — `App::run` receives `Action` variants from the mpsc channel:
   - `Action::DataUpdate(payload)` — stores the new data
   - `Action::FetchError(err)` — stores the error message
   - `Action::Key(event)` — processes keybindings
   - `Action::Resize(w, h)` — triggers re-render

7. **Rendering** (`tui/tabs/`, `tui/widgets/`) — renders the current view mode (List, JobDetail, SqlDetail) using ratatui widgets

## Async Model

spark-tui uses the **tokio** runtime with three concurrent tasks:

| Task | Channel | Description |
|------|---------|-------------|
| Poller | `tx → rx` | Fetches data and sends `Action::DataUpdate` / `Action::FetchError` |
| Event reader | `tx → rx` | Reads terminal events via `crossterm::event::read` (blocking, wrapped in `spawn_blocking`) |
| App loop | `rx` | Receives all actions and processes them sequentially |

All tasks communicate through a single `mpsc::UnboundedSender<Action>` channel. The app loop owns the receiver and processes actions one at a time, ensuring thread-safe state updates without locks.

## Design Decisions

- **Bounded task fetching**: Task lists (per-task metrics) are only fetched for the top 5 slowest stages to avoid overwhelming the API
- **Concurrent fetches**: Jobs, stages, and SQL executions are fetched in parallel with `tokio::join!` to minimize latency
- **Log file**: Logs go to `/tmp/spark-tui.log` instead of stderr to avoid corrupting the TUI
- **Panic hook**: A custom panic hook restores the terminal before printing the panic message, preventing terminal corruption
- **Edition 2024**: Uses the latest Rust edition for modern language features
