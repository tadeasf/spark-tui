# Contributing

## Development Setup

### Prerequisites

- Rust 1.85+ (edition 2024)
- A Databricks workspace for testing (optional for code changes, required for integration testing)

### Clone and build

```bash
git clone https://github.com/tadeasf/spark-tui.git
cd spark-tui
cargo build
```

### Run locally

```bash
cargo run -- --host adb-123.azuredatabricks.net --token dapi... --cluster-id 0123-...
```

## Project Structure

```
src/
├── main.rs          Entry point
├── config.rs        CLI args + config resolution
├── fetch/           HTTP client, API types, polling
├── analyze/         Suspect detection and classification
├── tui/             App state, rendering, widgets
└── util/            Formatting and time utilities
```

See [Architecture](./architecture.md) for a detailed breakdown.

## Testing

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run a specific test module
cargo test config::tests
cargo test analyze::skew::tests
cargo test analyze::suspects::tests
```

The test suite covers:

- Config parsing (`~/.databrickscfg` format, profile detection, URL normalization)
- Skew detection (uniform tasks, warning-level skew, critical skew)
- Suspect detection (slow stages, spill, bottleneck classification)
- SQL linking (job-to-SQL mapping)
- Formatting utilities (duration, bytes, truncation, stage name cleaning)
- Time parsing (RFC3339, naive, GMT suffix formats)

## Code Style

- **Edition 2024** — use current Rust idioms
- **Error handling** — use `thiserror` for error types, `Result` for fallible operations
- **Formatting** — run `cargo fmt` before committing
- **Linting** — run `cargo clippy` and address warnings

```bash
cargo fmt --check
cargo clippy -- -D warnings
```

## Dependencies

| Crate                            | Purpose                                    |
| -------------------------------- | ------------------------------------------ |
| `clap`                           | CLI argument parsing with env var fallback |
| `tokio`                          | Async runtime                              |
| `reqwest`                        | HTTP client (with rustls-tls)              |
| `serde` / `serde_json`           | JSON deserialization                       |
| `thiserror`                      | Error type derivation                      |
| `ratatui`                        | Terminal UI framework                      |
| `crossterm`                      | Terminal backend                           |
| `tracing` / `tracing-subscriber` | Structured logging                         |
| `chrono`                         | Timestamp parsing                          |

## Conventions

- Keep analysis logic in `analyze/`, not in the TUI layer
- Keep API types in `fetch/types.rs`, not scattered across modules
- Format functions go in `util/format.rs`
- Each suspect detector is a pure function: `(data) -> Vec<Suspect>`
- The poller is the only place where API calls and analysis are composed together
