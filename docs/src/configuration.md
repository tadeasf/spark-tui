# Configuration

spark-tui requires three credentials to connect to a Databricks cluster: **host**, **token**, and **cluster ID**. These can be provided through CLI flags, environment variables, or a `~/.databrickscfg` file.

## Priority Resolution

Configuration is resolved in this order (highest priority first):

1. **CLI flags** — `--host`, `--token`, `--cluster-id`
2. **Environment variables** — `DATABRICKS_HOST`, `DATABRICKS_TOKEN`, `DATABRICKS_CLUSTER_ID`
3. **`~/.databrickscfg`** — INI-format file with profile sections

CLI flags and environment variables are handled by [clap](https://docs.rs/clap) with the `env` feature — each flag falls back to its corresponding env var automatically.

If all three required fields are not satisfied by CLI/env, spark-tui reads `~/.databrickscfg` to fill the gaps. You can mix sources: for example, set `host` and `token` via env vars but `cluster_id` via the config file.

## CLI Reference

| Flag | Short | Env Var | Default | Description |
|------|-------|---------|---------|-------------|
| `--host` | | `DATABRICKS_HOST` | — | Workspace hostname |
| `--token` | | `DATABRICKS_TOKEN` | — | Personal access token |
| `--cluster-id` | | `DATABRICKS_CLUSTER_ID` | — | Cluster ID |
| `--profile` | `-p` | `DATABRICKS_CONFIG_PROFILE` | auto-detect | Profile name from `~/.databrickscfg` |
| `--poll-interval` | | `SPARK_TUI_POLL_INTERVAL` | `10` | Poll interval in seconds |

## `~/.databrickscfg` Format

The file uses INI format with named profile sections:

```ini
[DEFAULT]
host = adb-123.azuredatabricks.net
token = dapi0123456789abcdef

[production]
host = adb-999.azuredatabricks.net
token = dapi_prod_token
cluster_id = 0123-456789-prod

[development]
host = adb-123.azuredatabricks.net
token = dapi_dev_token
cluster_id = 0456-789012-dev
```

### Profile selection

- **Explicit**: `spark-tui --profile production` uses the `[production]` section
- **Auto-detect**: without `--profile`, spark-tui scans all profiles and uses the first one that has all three required fields (`host`, `token`, `cluster_id`)

If the named profile doesn't exist, spark-tui lists available profiles in the error message.

## Base URL Construction

spark-tui constructs the Spark REST API base URL as:

```
https://{host}/driver-proxy-api/o/0/{cluster_id}/40001/api/v1
```

The `host` field is normalized: any `https://` prefix and trailing slashes are stripped before URL construction.

## Poll Interval

The `--poll-interval` flag controls how often spark-tui refreshes data from the Spark API (default: 10 seconds). Lower values give more responsive updates but increase API load.

```bash
# Refresh every 5 seconds
spark-tui --poll-interval 5
```

## Logging

spark-tui writes logs to `/tmp/spark-tui.log` (logs cannot go to stderr as it would corrupt the TUI). Control the log level with the `RUST_LOG` environment variable:

```bash
RUST_LOG=info spark-tui    # Info and above
RUST_LOG=debug spark-tui   # Debug messages
RUST_LOG=trace spark-tui   # Everything
```

Default log level is `warn`.
