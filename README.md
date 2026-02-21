# spark-tui

A terminal UI for Apache Spark performance analysis via the Databricks driver proxy.

[![CI](https://img.shields.io/github/actions/workflow/status/tadeasf/spark-tui/ci.yml?label=CI)](https://github.com/tadeasf/spark-tui/actions)
[![Docs](https://img.shields.io/github/actions/workflow/status/tadeasf/spark-tui/docs.yml?label=docs)](https://tadeasf.github.io/spark-tui/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Features

- **Live job dashboard** — ranked by duration, auto-refreshing via background poller
- **16 suspect detectors** — flags slow stages, data skew, disk spill, partition count issues, broadcast join opportunities, Python UDF usage, cache opportunities, and more
- **Bottleneck classification** — identifies Large Scan, Wide Shuffle, and Data Explosion patterns
- **SQL correlation** — links jobs and stages back to SQL executions with plan hints
- **Critical path analysis** — annotates the longest-running stage per job with a "CP" marker
- **Estimated savings** — each suspect includes an estimated time savings to help prioritize fixes
- **Drill-down navigation** — job list → job detail (stages + bar chart) → SQL plan view
- **Help overlay** — press `h` for context-sensitive keybinding reference and PySpark recommendations
- **Smooth scrolling** — `tui-scrollview` integration for fluid scrolling in detail views
- **Color-coded severity** — warning (yellow) and critical (red) indicators at a glance
- **PySpark recommendations** — each suspect includes concrete PySpark tuning suggestions
- **Cross-platform releases** — CI/CD builds for Linux, macOS (x86 + ARM), and Windows
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

| Key                 | Action                                                       |
| ------------------- | ------------------------------------------------------------ |
| `Tab` / `Shift+Tab` | Switch between Jobs and Suspects tabs                        |
| `j` / `↓`           | Move selection down                                          |
| `k` / `↑`           | Move selection up                                            |
| `g` / `Home`        | Jump to top                                                  |
| `G` / `End`         | Jump to bottom                                               |
| `Enter`             | Drill into job detail / stage detail                         |
| `s`                 | Open SQL plan view (from job detail)                         |
| `h`                 | Toggle help overlay (keybinding reference / SQL recommendations) |
| `Esc`               | Go back one level                                            |
| `q`                 | Quit                                                         |

## Screenshots
<img width="1225" height="864" alt="image" src="https://github.com/user-attachments/assets/28c0c7a1-722d-47f6-a240-63b0e5ab7798" />
<img width="1223" height="859" alt="image" src="https://github.com/user-attachments/assets/0bb3d8f3-c1f0-4f90-b026-f6ff55b6e93e" />
<img width="1221" height="862" alt="image" src="https://github.com/user-attachments/assets/8e41dd1d-92ab-43b9-ab6f-660a8aaf2d6c" />
<img width="1224" height="858" alt="image" src="https://github.com/user-attachments/assets/cdf5cecf-bd5a-403e-a276-30bd0bc31701" />
<img width="1221" height="857" alt="image" src="https://github.com/user-attachments/assets/04158c89-aca9-4001-8bf2-142b642d0483" />
<img width="806" height="623" alt="image" src="https://github.com/user-attachments/assets/c2e68520-493e-46ca-ba55-d38f83f075d2" />

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
