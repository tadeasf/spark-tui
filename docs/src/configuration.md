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
| `--event-log-path` | | `SPARK_TUI_EVENT_LOG_PATH` | — | DBFS path to a Spark event log file |
| `--sparkui-cookie` | | `SPARK_TUI_SPARKUI_COOKIE` | — | `DATAPLANE_DOMAIN_DBAUTH` cookie value |

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

## Historical Mode

When spark-tui detects a **terminated** cluster (HTTP 503 or `INVALID_STATE` response), it automatically attempts to load historical Spark data using a 4-strategy fallback chain:

| Priority | Strategy | Description |
|----------|----------|-------------|
| 0 | **Spark UI REST API** | Probes `https://{host}/sparkui/{cluster}/{driver}/api/v1/`. Requires `spark_context_id` from the cluster info. Also tries the dataplane domain variant (`adb-dp-` prefix). |
| 1 | **Spark History Server** | Probes known Databricks history server proxy URLs (multiple path patterns). |
| 2 | **DBFS event logs** | Reads event logs from the cluster's `cluster_log_conf` delivery path, or from `--event-log-path` if specified. |
| 3 | **Default DBFS paths** | Scans well-known DBFS directories (`dbfs:/cluster-logs/`, `dbfs:/databricks/spark/eventLogs/`, etc.) for event log files. |

The first strategy that succeeds provides the historical data. The status line shows a **HISTORICAL** badge.

### Spark UI warm-up

The Historical Spark UI needs to download and parse event logs from DBFS before serving JSON data. During this warm-up phase, it returns an HTML loading page instead of JSON. spark-tui detects this and retries with backoff (3s, 5s, 10s, 15s, 20s — ~53s total), showing progress messages like "Spark UI loading... retrying (2/5)".

### Getting the `--sparkui-cookie`

On authenticated Databricks workspaces, the Spark UI endpoint requires a cookie instead of a Bearer token:

1. Open the Databricks workspace in your browser
2. Navigate to your cluster's **Spark UI** tab (this warms up the endpoint)
3. Open browser DevTools (F12) → **Application** → **Cookies**
4. Find the `adb-dp-*` domain (e.g., `adb-dp-1234567890.azuredatabricks.net`)
5. Copy the value of the `DATAPLANE_DOMAIN_DBAUTH` cookie

Then pass it to spark-tui:

```bash
spark-tui --sparkui-cookie "eyJ0eXAiOiJKV1Q..." --cluster-id 0123-456789-abcdef
# or
export SPARK_TUI_SPARKUI_COOKIE="eyJ0eXAiOiJKV1Q..."
spark-tui --cluster-id 0123-456789-abcdef
```

### Using `--event-log-path`

If you know the exact DBFS path to a Spark event log file:

```bash
spark-tui --event-log-path "dbfs:/cluster-logs/0123-456789-abcdef/eventlog/events.log.gz"
```

This is useful when the automatic DBFS scanning doesn't find your logs (e.g., custom log delivery paths).

## Logging

spark-tui writes logs to `/tmp/spark-tui.log` (logs cannot go to stderr as it would corrupt the TUI). Control the log level with the `RUST_LOG` environment variable:

```bash
RUST_LOG=info spark-tui    # Info and above
RUST_LOG=debug spark-tui   # Debug messages
RUST_LOG=trace spark-tui   # Everything
```

Default log level is `warn`.
