# Quick Start

## Prerequisites

- **Rust toolchain** — version 1.85 or later (spark-tui uses edition 2024)
- **Databricks workspace** — with a running cluster and an active Spark application
- **Personal access token** — generated from Databricks Settings > Developer > Access Tokens

## Installation

```bash
git clone https://github.com/tadeasf/spark-tui.git
cd spark-tui
cargo install --path .
```

Or run directly without installing:

```bash
cargo run -- --host adb-123.azuredatabricks.net --token dapi... --cluster-id 0123-...
```

## Configuration

You need three pieces of information:

| Field                 | Example                              |
| --------------------- | ------------------------------------ |
| Workspace host        | `adb-1234567890.azuredatabricks.net` |
| Personal access token | `dapi0123456789abcdef...`            |
| Cluster ID            | `0123-456789-abcdef`                 |

Provide them via any of these methods (highest priority first):

### Option 1: CLI flags

```bash
spark-tui \
  --host adb-123.azuredatabricks.net \
  --token dapi0123456789abcdef \
  --cluster-id 0123-456789-abcdef
```

### Option 2: Environment variables

```bash
export DATABRICKS_HOST=adb-123.azuredatabricks.net
export DATABRICKS_TOKEN=dapi0123456789abcdef
export DATABRICKS_CLUSTER_ID=0123-456789-abcdef
spark-tui
```

### Option 3: `~/.databrickscfg` file

Create or edit `~/.databrickscfg`:

```ini
[my-workspace]
host = adb-123.azuredatabricks.net
token = dapi0123456789abcdef
cluster_id = 0123-456789-abcdef
```

Then run with a specific profile:

```bash
spark-tui --profile my-workspace
```

Or let spark-tui auto-detect the first complete profile:

```bash
spark-tui
```

## First Run

When spark-tui starts, it will:

1. Resolve configuration (CLI > env > databrickscfg)
2. Connect to the Spark REST API via the driver proxy
3. Discover the active Spark application
4. Fetch jobs, stages, and SQL executions
5. Display the Jobs tab with results ranked by duration

You should see a table of Spark jobs. Use `j`/`k` or arrow keys to navigate, `Enter` to drill into a job, and `Tab` to switch to the Suspects tab.

If something goes wrong, check the [Troubleshooting](./troubleshooting.md) guide.

## Next Steps

- [Configuration](./configuration.md) — full reference for all options
- [Navigation](./navigation.md) — keybindings and view modes
- [Understanding Analysis](./analysis-guide.md) — interpreting suspects
