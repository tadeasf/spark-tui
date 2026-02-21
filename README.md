# spark-tui

A terminal UI for Apache Spark performance analysis via the Databricks driver proxy.

[![CI](https://img.shields.io/github/actions/workflow/status/tadeasf/spark-tui/ci.yml?label=CI)](https://github.com/tadeasf/spark-tui/actions)
[![Docs](https://img.shields.io/github/actions/workflow/status/tadeasf/spark-tui/docs.yml?label=docs)](https://tadeasf.github.io/spark-tui/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Features

- **Live job dashboard** — ranked by duration, auto-refreshing via background poller
- **Suspect detection** — flags slow stages, data skew, and disk spill automatically
- **Bottleneck classification** — identifies Large Scan, Wide Shuffle, and Data Explosion patterns
- **SQL correlation** — links jobs and stages back to SQL executions with plan hints
- **Drill-down navigation** — job list → job detail (stages + bar chart) → SQL plan view
- **Color-coded severity** — warning (yellow) and critical (red) indicators at a glance
- **Actionable recommendations** — each suspect includes a concrete tuning suggestion
- **Zero setup** — reads credentials from CLI flags, env vars, or `~/.databrickscfg`

## Quick Start

### Prerequisites

- Rust toolchain (1.85+ for edition 2024)
- A running Databricks cluster with an active Spark application

### Install

```bash
git clone https://github.com/tadeasf/spark-tui.git
cd spark-tui
cargo install --path .
```

### Configure

Provide credentials using any of these methods (highest priority first):

| Method             | Host              | Token              | Cluster ID              |
| ------------------ | ----------------- | ------------------ | ----------------------- |
| CLI flags          | `--host`          | `--token`          | `--cluster-id`          |
| Environment        | `DATABRICKS_HOST` | `DATABRICKS_TOKEN` | `DATABRICKS_CLUSTER_ID` |
| `~/.databrickscfg` | `host`            | `token`            | `cluster_id`            |

### Run

```bash
# With CLI flags
spark-tui --host adb-123.azuredatabricks.net --token dapi... --cluster-id 0123-456789-abcdef

# With environment variables
export DATABRICKS_HOST=adb-123.azuredatabricks.net
export DATABRICKS_TOKEN=dapi...
export DATABRICKS_CLUSTER_ID=0123-456789-abcdef
spark-tui

# With a specific databrickscfg profile
spark-tui --profile my-workspace
```

## Configuration

| Flag              | Env Var                     | Default     | Description                                             |
| ----------------- | --------------------------- | ----------- | ------------------------------------------------------- |
| `--host`          | `DATABRICKS_HOST`           | —           | Workspace hostname (e.g. `adb-123.azuredatabricks.net`) |
| `--token`         | `DATABRICKS_TOKEN`          | —           | Personal access token                                   |
| `--cluster-id`    | `DATABRICKS_CLUSTER_ID`     | —           | Cluster ID                                              |
| `--profile`, `-p` | `DATABRICKS_CONFIG_PROFILE` | auto-detect | Profile name from `~/.databrickscfg`                    |
| `--poll-interval` | `SPARK_TUI_POLL_INTERVAL`   | `10`        | Refresh interval in seconds                             |

## Keybindings

| Key                 | Action                                |
| ------------------- | ------------------------------------- |
| `Tab` / `Shift+Tab` | Switch between Jobs and Suspects tabs |
| `j` / `↓`           | Move selection down                   |
| `k` / `↑`           | Move selection up                     |
| `g` / `Home`        | Jump to top                           |
| `G` / `End`         | Jump to bottom                        |
| `Enter`             | Drill into job detail / stage detail  |
| `s`                 | Open SQL plan view (from job detail)  |
| `Esc`               | Go back one level                     |
| `q`                 | Quit                                  |

## Screenshots
<img width="1080" height="1110" alt="image" src="https://github.com/user-attachments/assets/25845362-4af0-4003-85cb-ccb9157abfea" />
<img width="1080" height="1105" alt="image" src="https://github.com/user-attachments/assets/4cfde96d-b0d4-4f40-b23f-7a21317c83b5" />
<img width="1080" height="1103" alt="image" src="https://github.com/user-attachments/assets/946e6470-bdf0-4a0d-8dcd-17ec39b83cab" />

## Documentation

Full documentation is available at the [GitHub Pages site](https://tadeasf.github.io/spark-tui/).

Build docs locally:

```bash
cargo install mdbook
mdbook serve docs
# Open http://localhost:3000
```

## License

MIT

## Contributing

See [Contributing Guide](https://tadeasf.github.io/spark-tui/contributing.html) for development setup, testing, and code conventions.
