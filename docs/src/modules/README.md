# Module Reference

This section provides a reference for each module in the spark-tui codebase.

## Modules

| Module | Path | Description |
|--------|------|-------------|
| [Config](./config.md) | `src/config.rs` | CLI argument parsing, config resolution, `~/.databrickscfg` support |
| [Fetch](./fetch.md) | `src/fetch/` | HTTP client, Spark API types, endpoint methods, background poller |
| [Analyze](./analyze.md) | `src/analyze/` | Suspect detection (slow stages, skew, spill), bottleneck classification, SQL linking |
| [TUI](./tui.md) | `src/tui/` | App state machine, tab rendering, widgets, theme |
| [Utilities](./util.md) | `src/util/` | Formatting (duration, bytes) and time parsing helpers |
