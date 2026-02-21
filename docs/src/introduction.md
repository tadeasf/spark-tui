# Introduction

**spark-tui** is a terminal-based performance analysis tool for Apache Spark applications running on Databricks. It connects to the Spark REST API through the Databricks driver proxy and presents live job metrics, stage breakdowns, and automated suspect detection in an interactive TUI.

## Why spark-tui?

Debugging Spark performance problems typically involves clicking through the Spark UI in a browser, manually comparing stage durations, and guessing which stages have data skew or excessive spill. This process is slow and error-prone.

spark-tui automates this analysis:

- **Automatic suspect detection** — identifies slow stages, data skew, and disk spill without manual inspection
- **Bottleneck classification** — categorizes root causes as Large Scan, Wide Shuffle, or Data Explosion
- **Actionable recommendations** — each finding includes a concrete tuning suggestion
- **SQL correlation** — links stages back to the originating SQL query and shows plan hints
- **Live updates** — polls the Spark API on a configurable interval and refreshes the display

## What You See

The interface has two main tabs:

1. **Jobs** — all Spark jobs ranked by duration (slowest first), with drill-down to stage details, duration bar charts, and SQL execution plans
2. **Suspects** — automatically detected performance issues, sorted by severity (critical first), with category labels, I/O summaries, and recommendations

## How It Works

spark-tui connects to the Spark History Server API exposed through Databricks' driver proxy endpoint:

```
https://{host}/driver-proxy-api/o/0/{cluster_id}/40001/api/v1
```

A background poller fetches jobs, stages, SQL executions, and task lists at regular intervals. The analysis engine processes this data to detect anomalies, then the TUI renders the results in real time.

## Next Steps

- [Quick Start](./quickstart.md) — install, configure, and run spark-tui
- [Navigation](./navigation.md) — learn the keybindings
- [Understanding Analysis](./analysis-guide.md) — interpret the suspect findings
