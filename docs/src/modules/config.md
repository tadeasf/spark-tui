# Config Module

**Path:** `src/config.rs`

Handles CLI argument parsing, environment variable fallback, and `~/.databrickscfg` file parsing to produce a resolved `Config` struct.

## Structs

### `CliArgs`

Clap-derived struct for command-line arguments. Each field uses `#[arg(env = "...")]` for automatic env var fallback.

```rust
pub struct CliArgs {
    pub host: Option<String>,          // --host / DATABRICKS_HOST
    pub token: Option<String>,         // --token / DATABRICKS_TOKEN
    pub cluster_id: Option<String>,    // --cluster-id / DATABRICKS_CLUSTER_ID
    pub profile: Option<String>,       // --profile, -p / DATABRICKS_CONFIG_PROFILE
    pub poll_interval: u64,            // --poll-interval / SPARK_TUI_POLL_INTERVAL (default: 10)
}
```

### `Config`

Resolved configuration with all required fields guaranteed present.

```rust
pub struct Config {
    pub host: String,
    pub token: String,
    pub cluster_id: String,
    pub poll_interval: u64,
}
```

**Methods:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `base_url` | `&self -> String` | Constructs the Spark REST API base URL. Strips `https://` prefix and trailing slashes from host |

## Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `resolve_config` | `() -> Result<Config, String>` | Main entry point. Resolves config from CLI > env > databrickscfg |
| `databrickscfg_path` | `() -> Option<PathBuf>` | Returns `~/.databrickscfg` path if the file exists |
| `parse_databrickscfg` | `(path) -> Result<HashMap<String, Profile>>` | Parses INI-format config file into profile sections |
| `find_complete_profile` | `(profiles) -> Option<&Profile>` | Finds the first profile with host, token, and cluster_id |

## Resolution Logic

1. Parse CLI args (clap handles env var fallback)
2. If all three fields are present → return `Config`
3. Otherwise, read `~/.databrickscfg`
4. If `--profile` is set → use that section (error if not found)
5. Otherwise → auto-detect the first complete profile
6. Merge CLI/env values with profile values (CLI/env takes priority)

## Tests

- `test_parse_databrickscfg` — verifies INI parsing
- `test_find_complete_profile` — verifies auto-detection of complete profiles
- `test_base_url_strips_scheme_and_trailing_slash` — verifies URL normalization
