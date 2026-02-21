# Navigation & Keybindings

spark-tui uses vim-style keybindings for navigation. The interface has three view modes arranged in a drill-down hierarchy.

## View Modes

```
List ──Enter──▶ JobDetail ──s──▶ SqlDetail
  ◀──Esc──        ◀──Esc──
```

1. **List** — the top-level view showing either the Jobs or Suspects tab
2. **JobDetail** — stage breakdown and duration bar chart for a selected job
3. **SqlDetail** — scrollable SQL execution plan for the selected job's query

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `q` | Quit the application |
| `Esc` | Go back one level (SqlDetail → JobDetail → List → Quit) |

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

### JobDetail Mode

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down in the stage list |
| `k` / `↑` | Move selection up in the stage list |
| `g` / `Home` | Jump to first stage |
| `G` / `End` | Jump to last stage |
| `s` | Open SQL plan view (if the job has a linked SQL execution) |
| `Esc` | Return to List mode |

### SqlDetail Mode

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `g` / `Home` | Scroll to top |
| `G` / `End` | Scroll to bottom |
| `Esc` | Return to JobDetail mode |

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

Displays automatically detected performance issues, sorted by severity (Critical first, then Warning). Each row shows:

- Severity indicator (color-coded)
- Category (Slow Stage / Data Skew / Disk Spill)
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
