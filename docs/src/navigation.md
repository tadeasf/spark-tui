# Navigation & Keybindings

spark-tui uses vim-style keybindings for navigation. The interface has three view modes arranged in a drill-down hierarchy.

## View Modes

```
List ──Enter──▶ JobDetail ──Enter──▶ StageDetail
                    │                     │
                    └──s──▶ SqlDetail     │
  ◀──Esc──        ◀──Esc──        ◀──Esc──
```

1. **List** — the top-level view showing either the Jobs or Suspects tab
2. **JobDetail** — stage breakdown and duration bar chart for a selected job
3. **StageDetail** — detailed metrics for a selected stage (I/O, CPU %, peak RAM, task histograms, per-executor breakdown, skew metrics)
4. **SqlDetail** — scrollable SQL execution plan for the selected job's query

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `q` | Quit the application |
| `Esc` | Go back one level (SqlDetail → JobDetail → List → Quit) |
| `h` | Toggle help overlay (keybinding reference in most views; PySpark recommendations in SqlDetail) |

### List Mode (Jobs / Suspects tabs)

| Key | Action |
|-----|--------|
| `Tab` | Switch to next tab |
| `Shift+Tab` | Switch to previous tab |
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `g` / `Home` | Jump to first row |
| `G` / `End` | Jump to last row |
| `Enter` | Drill into the selected job's detail view |

Status bar hint: `q:quit Tab:switch j/k:nav Enter:detail h:help`

### JobDetail Mode

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down in the stage list |
| `k` / `↑` | Move selection up in the stage list |
| `g` / `Home` | Jump to first stage |
| `G` / `End` | Jump to last stage |
| `Enter` | Drill into the selected stage's detail view |
| `s` | Open SQL plan view (if the job has a linked SQL execution) |
| `Esc` | Return to List mode |

Status bar hint: `Esc:back j/k:nav Enter:stage s:sql h:help`

**Note:** When entering JobDetail from the Suspects tab, pressing `Esc` returns to the Suspects tab (not Jobs). This is tracked via the `return_tab` field.

### StageDetail Mode

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `g` / `Home` | Scroll to top |
| `G` / `End` | Scroll to bottom |
| `Esc` | Return to JobDetail mode |

Status bar hint: `Esc:back j/k:scroll g/G:top/bot h:help`

### SqlDetail Mode

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `g` / `Home` | Scroll to top |
| `G` / `End` | Scroll to bottom |
| `h` | Show PySpark recommendations for suspects related to this SQL execution |
| `Esc` | Return to JobDetail mode |

Status bar hint: `Esc:back j/k:scroll g/G:top/bot h:hints`

## Help Overlay

Pressing `h` toggles a help overlay:

- **In List, JobDetail, and StageDetail modes:** Shows a general keybinding reference card listing all available shortcuts for the current view.
- **In SqlDetail mode:** Shows PySpark-specific recommendations based on suspect findings related to the current SQL execution.

Press `h` again or `Esc` to dismiss the overlay.

## Tabs

### Jobs Tab

Displays all Spark jobs in a table, ranked by duration (slowest first). Running jobs (with no completion time) appear at the top. Columns include:

- Job ID
- Status (with color coding)
- Duration
- Task counts
- SQL description (if linked)
- Submission time

### Suspects Tab

Displays automatically detected performance issues, sorted by severity (Critical first), then by estimated savings descending as a tiebreaker. Each row shows:

- Severity indicator (color-coded)
- Category (Slow Stage / Data Skew / Data Size Skew / Record Count Skew / Disk Spill / CPU Bottleneck / I/O Bottleneck / Record Explosion / Task Failures / Memory Pressure / Executor Hotspot / Too Many Partitions / Too Few Partitions / Broadcast Join Opportunity / Python UDF / Cache Opportunity)
- Stage ID and job ID
- Title with key metrics
- Detail summary
- Recommendation

## Color Coding

| Color | Meaning |
|-------|---------|
| Red | Critical severity, failed status |
| Yellow | Warning severity, running status |
| Green | Healthy / succeeded status |
| Gray | Muted / secondary information |
| Cyan | Selected row highlight |
| Magenta | **CP** — Critical Path stage (longest-running stage per job) |

### CPU Utilization (Stage Detail)

| Color | Range | Meaning |
|-------|-------|---------|
| Red | ≥ 95% or < 30% | CPU saturated or severe I/O bound |
| Green | 50%–94% | Healthy utilization |
| Yellow | 30%–49% | Underutilized |

### Peak Memory (Stage Detail)

Color-coded relative to total cluster memory (ratio-based), with absolute fallback when executor data is unavailable. See [Understanding Analysis](analysis-guide.md#color-coding-in-stage-detail) for threshold details.

### Summary Bar

The health summary bar in List view uses colored foreground text (not colored background) to display job/IO counts and top issues: red for critical, yellow for warning, green for healthy.
