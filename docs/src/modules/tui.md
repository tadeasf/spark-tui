# TUI Module

**Path:** `src/tui/`

Contains the terminal UI: app state machine, event loop, tab rendering, widgets, and theme.

## Files

| File | Purpose |
|------|---------|
| `app.rs` | `App` struct, event loop, key handling, rendering dispatch |
| `theme.rs` | Color and style functions |
| `tabs/jobs.rs` | Jobs table, job detail, SQL detail views |
| `tabs/suspects.rs` | Suspects table view |
| `widgets/bar_chart.rs` | Duration bar chart for stage comparison |
| `widgets/status_line.rs` | Status bar with cluster info and last update time |

## `app.rs` — App State

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
    List,       // Tab-level table view
    JobDetail,  // Stage breakdown for a selected job
    SqlDetail,  // SQL execution plan (scrollable)
}
```

### `App`

```rust
pub struct App {
    pub active_tab: Tab,
    pub view_mode: ViewMode,
    pub data: Option<DataPayload>,
    pub error_msg: Option<String>,
    pub cluster_id: String,
    pub should_quit: bool,
    pub job_table_state: TableState,
    pub suspect_table_state: TableState,
    pub detail_table_state: TableState,
    pub sql_scroll: u16,
}
```

**Key methods:**

| Method | Description |
|--------|-------------|
| `new(cluster_id)` | Creates a new App with initial state |
| `run(&mut self, terminal, rx)` | Main event loop — receives Actions from the channel, handles keys, re-renders |
| `handle_key(key_event)` | Processes keyboard input based on current ViewMode |
| `handle_enter()` | Drills into the selected item |
| `handle_action(action)` | Processes DataUpdate, FetchError, Key, Mouse, Resize actions |
| `render(frame)` | Renders the current state to the terminal |

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

Size thresholds for byte styling: `MB = 1_048_576`, `GB = 1_073_741_824`.

## `tabs/jobs.rs` — Jobs Tab

| Function | Description |
|----------|-------------|
| `render_jobs_tab(frame, area, app)` | Renders the jobs table with columns: ID, Status, Duration, Tasks, Failed, SQL, Submitted |
| `render_job_detail(frame, area, app)` | Splits area into stage table (top) and duration bar chart (bottom) |
| `render_sql_detail(frame, area, app)` | Renders scrollable SQL execution plan text |

## `tabs/suspects.rs` — Suspects Tab

| Function | Description |
|----------|-------------|
| `render_suspects_tab(frame, area, app)` | Renders suspects table with columns: Severity, Category, Stage, Job, Title, Detail, Recommendation |

## `widgets/bar_chart.rs`

| Function | Description |
|----------|-------------|
| `render_duration_chart(frame, area, stages)` | Renders a horizontal bar chart comparing stage durations |

## `widgets/status_line.rs`

| Function | Description |
|----------|-------------|
| `render_status_line(frame, area, app)` | Renders the bottom status bar showing cluster ID, app ID, and last update time |
