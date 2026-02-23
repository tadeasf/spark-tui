# TUI Module

**Path:** `src/tui/`

Contains the terminal UI: app state machine, event loop, tab rendering, widgets, and theme.

## Files

| File | Purpose |
|------|---------|
| `app/` | `App` struct, event loop, key handling, rendering dispatch (`state.rs`, `input.rs`, `render.rs`) |
| `theme.rs` | Color and style functions |
| `highlight.rs` | SQL/plan syntax highlighting |
| `tabs/jobs_list.rs` | Jobs table view |
| `tabs/job_detail.rs` | Stage breakdown for a selected job |
| `tabs/sql_detail.rs` | SQL execution plan view (scrollable) |
| `tabs/stage_detail.rs` | Detailed stage metrics view |
| `tabs/suspects.rs` | Suspects table view |
| `widgets/help.rs` | Help overlay (keybinding reference + SQL recommendations) |
| `widgets/status_line.rs` | Status bar with cluster info and last update time |
| `widgets/summary_bar.rs` | Health summary bar (top issues, job/IO counts) |

## `app/` — App State

### `Tab`

```rust
pub enum Tab {
    Jobs,
    Suspects,
}
```

Methods: `next()`, `prev()`, `index()`, `from_index()`, `titles()`.

### `ViewMode`

```rust
pub enum ViewMode {
    List,         // Tab-level table view
    JobDetail,    // Stage breakdown for a selected job
    SqlDetail,    // SQL execution plan (scrollable)
    StageDetail,  // Detailed metrics for a selected stage
}
```

### `App`

```rust
pub struct App {
    pub active_tab: Tab,
    pub view_mode: ViewMode,
    pub data: Option<Arc<DataPayload>>,
    pub error_msg: Option<String>,
    pub cluster_id: String,
    pub should_quit: bool,
    pub job_table_state: TableState,
    pub suspect_table_state: TableState,
    pub detail_table_state: TableState,
    pub sql_scroll_state: ScrollViewState,
    pub stage_detail_scroll_state: ScrollViewState,
    show_help: bool,
    return_tab: Option<Tab>,
    client: Arc<SparkHttpClient>,
    tx: mpsc::UnboundedSender<Action>,
    pending_task_fetches: HashSet<i64>,
}
```

**Notable changes from v1:**
- `data` changed from `Option<DataPayload>` to `Option<Arc<DataPayload>>` for cheaper cloning
- `sql_scroll: u16` and `stage_detail_scroll: u16` replaced with `sql_scroll_state: ScrollViewState` and `stage_detail_scroll_state: ScrollViewState` (from `tui-scrollview`)
- `show_help: bool` — toggles the help overlay
- `return_tab: Option<Tab>` — tracks which tab to return to on Esc from JobDetail (e.g., Suspects tab when entering via a suspect)

**Key methods:**

| Method | Description |
|--------|-------------|
| `new(cluster_id, client, tx)` | Creates a new App with initial state |
| `run(&mut self, terminal, rx)` | Main event loop — receives Actions from the channel, handles keys, re-renders |
| `handle_key(key_event)` | Processes keyboard input based on current ViewMode |
| `handle_action(action)` | Processes DataUpdate, FetchError, Key, Mouse, Resize actions |
| `handle_escape()` | Goes back one level; respects `return_tab` |
| `handle_enter()` | Drills into the selected item |
| `handle_navigation_down()` | Moves selection/scroll down based on ViewMode |
| `handle_navigation_up()` | Moves selection/scroll up based on ViewMode |
| `handle_navigation_home()` | Jumps to top |
| `handle_navigation_end()` | Jumps to bottom |
| `open_sql_detail()` | Opens SQL detail for the current job |
| `enter_suspect_job()` | Navigates from Suspects tab to a suspect's job detail, setting `return_tab` |
| `enter_stage_detail()` | Navigates to stage detail from job detail |
| `trigger_task_fetch(stage_id)` | Triggers on-demand task fetch for a stage not already in `stage_tasks` |
| `render(frame)` | Renders the current state to the terminal |
| `render_tab_bar(frame, area)` | Renders the tab header |
| `render_content(frame, area)` | Renders the main content area for the current ViewMode |
| `render_status_bar(frame, area)` | Renders the bottom status bar with context-sensitive hint strings |

## `theme.rs` — Styles

Pure functions that return `ratatui::style::Style`:

| Function | Usage |
|----------|-------|
| `critical()` | Red — critical severity |
| `warning()` | Yellow — warning severity |
| `healthy()` | Green — success/healthy |
| `running()` | Yellow — running status |
| `failed()` | Red — failed status |
| `muted()` | Gray — secondary text |
| `selected()` | Cyan — selected row |
| `tab_active()` | Active tab style |
| `tab_inactive()` | Inactive tab style |
| `status_bar()` | Status bar background |
| `severity_style(severity)` | Maps `Severity` to style |
| `job_status_style(status)` | Maps job status string to style |
| `metric_bytes_style(bytes)` | Color-codes byte counts by size |
| `shuffle_bytes_style(bytes)` | Color-codes shuffle bytes |
| `spill_bytes_style(bytes)` | Color-codes spill bytes |
| `cpu_utilization_style(ratio)` | Color-codes CPU utilization: ≥0.95 red (saturated), ≥0.5 green (healthy), ≥0.3 yellow (underutilized), <0.3 red (I/O bound) |
| `memory_utilization_style(peak, cluster_mem)` | Color-codes peak memory relative to cluster total; falls back to absolute thresholds when cluster data unavailable |

Size thresholds for byte styling: `MB = 1_048_576`, `GB = 1_073_741_824`.

## `tabs/jobs_list.rs` — Jobs List

| Function | Description |
|----------|-------------|
| `format_submission_time(time)` | Formats submission time for display |
| `render_jobs_tab(frame, area, app)` | Renders the jobs table with columns: ID, Status, Duration, Tasks, Failed, SQL, Submitted |

## `tabs/job_detail.rs` — Job Detail

| Function | Description |
|----------|-------------|
| `render_job_detail(frame, area, job, stages, sql_executions, stage_state, critical_stages)` | Renders stage breakdown table for a job. Critical path stages are annotated with "CP" |

## `tabs/sql_detail.rs` — SQL Detail

| Function | Description |
|----------|-------------|
| `render_sql_detail(frame, area, job, scroll_state, suspects)` | Renders scrollable SQL execution plan text with syntax highlighting. Uses `ScrollViewState` from `tui-scrollview` |

## `tabs/stage_detail.rs` — Stage Detail

| Function | Description |
|----------|-------------|
| `render_stage_header(frame, area, stage, total_cluster_memory, sql_hint)` | Renders stage header with I/O metrics, CPU %, and optional SQL plan hint |
| `render_duration_histogram(frame, area, tasks)` | Renders task duration histogram |
| `render_executor_breakdown(frame, area, tasks)` | Renders per-executor breakdown table |
| `render_peak_memory_section(frame, area, tasks, total_cluster_memory)` | Renders peak memory per task section |
| `render_skew_metrics(frame, area, tasks)` | Renders skew metrics (CV, max/median ratio) |
| `render_stage_detail(frame, area, stage, tasks, loading, scroll_state, total_cluster_memory, sql_hint)` | Renders full stage detail using `ScrollViewState`. Composes stage_header, I/O metrics, duration histogram, executor breakdown, peak memory, and skew metrics |

## `tabs/suspects.rs` — Suspects Tab

| Function | Description |
|----------|-------------|
| `render_suspects_tab(frame, area, suspects, state, critical_stages)` | Renders suspects table with columns: Severity, Category, Stage, Job, Title, Detail, Recommendation. Table title: `"Suspects (severity → savings)"` |

## `highlight.rs` — Syntax Highlighting

| Function | Description |
|----------|-------------|
| `highlight_sql(text)` | Applies syntax highlighting to SQL query text for display in the SQL detail view |
| `highlight_spark_plan(text)` | Applies syntax highlighting to Spark physical plan text |

## `widgets/help.rs`

| Function | Description |
|----------|-------------|
| `centered_rect(area, percent_x, percent_y)` | Computes a centered rectangle within an area (private helper) |
| `render_help_overlay(frame, area)` | Renders a general keybinding reference overlay |
| `render_sql_help_overlay(frame, area, job, suspects)` | Renders PySpark-specific recommendations for suspects related to the current SQL execution |

## `widgets/status_line.rs`

| Function | Description |
|----------|-------------|
| `render_status_line(frame, area, app)` | Renders the bottom status bar showing cluster ID, app ID, and last update time |

## `widgets/summary_bar.rs`

| Function | Description |
|----------|-------------|
| `render_summary_bar(frame, area, summary)` | Renders 2-line health summary with colored foreground text (red=critical, yellow=warning, green=healthy) showing job/IO counts and top issues |
