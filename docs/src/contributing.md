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
| `tokio`                          | Async runtime (`macros`, `rt-multi-thread`, `time`, `sync` features) |
| `reqwest`                        | HTTP client (with `rustls-tls`, no default features) |
| `serde` / `serde_json`           | JSON deserialization                       |
| `thiserror`                      | Error type derivation                      |
| `ratatui`                        | Terminal UI framework (`unstable-rendered-line-info` feature) |
| `crossterm`                      | Terminal backend                           |
| `tracing` / `tracing-subscriber` | Structured logging (with `env-filter`)     |
| `chrono`                         | Timestamp parsing                          |
| `syntect` / `syntect-tui`        | SQL syntax highlighting                    |
| `tui-scrollview`                 | Smooth scrollable views for detail panels  |

## CI/CD

The project uses four GitHub Actions workflows:

- **ci.yml** — runs `cargo fmt --check`, `cargo clippy`, and `cargo test` on every push and PR
- **docs.yml** — builds and deploys mdbook documentation to GitHub Pages
- **auto-tag.yml** — creates a `vX.Y.Z` git tag when `Cargo.toml` version changes on `master`
- **release.yml** — triggered by `v*` tags; builds cross-platform release binaries (Linux x86_64, macOS x86_64 + aarch64, Windows x86_64) and creates a GitHub Release with artifacts

## Conventions

- Keep analysis logic in `analyze/`, not in the TUI layer
- Keep API types in `fetch/types.rs`, not scattered across modules
- Format functions go in `util/format.rs`
- Each suspect detector is a pure function: `(&[SparkStage], &SuspectContext) -> Vec<Suspect>`
- The poller is the only place where API calls and analysis are composed together
