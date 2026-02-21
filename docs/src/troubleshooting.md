# Troubleshooting

## Common Errors

spark-tui maps HTTP status codes from the Spark REST API to user-friendly error messages:

| Error | HTTP Status | Meaning | Solution |
|-------|-------------|---------|----------|
| **Unauthorized** | 401 | Token expired or invalid | Regenerate your token at Databricks Settings > Developer > Access Tokens |
| **Forbidden** | 403 | Insufficient permissions | Check that your token has access to the specified cluster |
| **Not Found** | 404 | Spark UI not available | The Spark application may have ended. Start a new Spark session or check the application ID |
| **Service Unavailable** | 503 | Cluster not reachable | Verify the cluster is running in the Databricks workspace |
| **No Applications** | — | No Spark apps on cluster | Ensure a Spark session is active on the cluster (e.g., run a notebook or submit a job) |

## Configuration Errors

### "Missing 'host' / 'token' / 'cluster_id'"

spark-tui couldn't find all three required fields. Check that you've provided them via CLI flags, environment variables, or `~/.databrickscfg`. See [Configuration](./configuration.md) for details.

### "Profile 'xyz' not found in ~/.databrickscfg"

The `--profile` flag specifies a section name that doesn't exist in your `~/.databrickscfg` file. The error message lists available profiles.

### Auto-detection fails

When no `--profile` is specified, spark-tui looks for the first profile in `~/.databrickscfg` that has all three required fields (`host`, `token`, `cluster_id`). If no profile is complete, you'll get a missing fields error.

## Connection Issues

### Timeout or no response

- Verify the cluster is in a **Running** state in Databricks
- Check that `--host` matches your workspace URL (e.g., `adb-1234567890.azuredatabricks.net`)
- Ensure `--cluster-id` is correct (find it in the cluster's URL or configuration page)

### TLS errors

spark-tui uses `rustls` for TLS. If you're behind a corporate proxy with custom CA certificates, you may need to set the `SSL_CERT_FILE` or `SSL_CERT_DIR` environment variables.

## Log File

spark-tui writes logs to `/tmp/spark-tui.log`. To increase verbosity:

```bash
RUST_LOG=debug spark-tui --host ... --token ... --cluster-id ...
```

Available log levels: `error`, `warn` (default), `info`, `debug`, `trace`.

Check the log file for detailed error information:

```bash
tail -f /tmp/spark-tui.log
```

## Terminal Issues

### Display is corrupted after a crash

If spark-tui exits abnormally (e.g., killed by a signal), the terminal may remain in raw mode. Reset it with:

```bash
reset
```

spark-tui installs a panic hook that attempts to restore the terminal on panic, but external signals bypass this.

### Colors look wrong

spark-tui uses 256-color mode via ratatui/crossterm. Ensure your terminal emulator supports 256 colors and that `TERM` is set correctly (e.g., `xterm-256color`).

## SQL Rendering Artifacts

If SQL plan text appears corrupted or causes display glitches, this is likely caused by raw newlines embedded in SQL text. spark-tui sanitizes these via `sanitize_for_span()` in `util/format.rs`, which replaces embedded `\n`, `\r`, and `\t` characters with spaces before passing text to ratatui's `Line`/`Span` types. Ratatui's differential renderer tracks cursor positions per line, so embedded newlines corrupt its state.

If you encounter rendering artifacts, check whether the SQL text contains unusual control characters and file an issue.

## Deserialization Errors

If the log shows deserialization errors, the Spark API may have returned an unexpected response format. This can happen with:

- Very old or very new Databricks Runtime versions
- Custom Spark configurations that alter the REST API response

File an issue with the error message and your Databricks Runtime version.
