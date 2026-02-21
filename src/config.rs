use std::collections::HashMap;
use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "spark-tui",
    about = "Terminal UI for Spark performance analysis"
)]
pub struct CliArgs {
    /// Databricks workspace host (e.g. adb-123.azuredatabricks.net)
    #[arg(long, env = "DATABRICKS_HOST")]
    pub host: Option<String>,

    /// Databricks personal access token
    #[arg(long, env = "DATABRICKS_TOKEN")]
    pub token: Option<String>,

    /// Databricks cluster ID
    #[arg(long, env = "DATABRICKS_CLUSTER_ID")]
    pub cluster_id: Option<String>,

    /// Databricks config profile name (from ~/.databrickscfg)
    #[arg(long, short, env = "DATABRICKS_CONFIG_PROFILE")]
    pub profile: Option<String>,

    /// Poll interval in seconds
    #[arg(long, default_value = "10", env = "SPARK_TUI_POLL_INTERVAL")]
    pub poll_interval: u64,
}

/// Resolved configuration with all required fields present.
#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub token: String,
    pub cluster_id: String,
    pub poll_interval: u64,
}

impl Config {
    pub fn base_url(&self) -> String {
        let host = self.host.trim_end_matches('/');
        // Strip "https://" if present — we add it back
        let host = host
            .strip_prefix("https://")
            .or_else(|| host.strip_prefix("http://"))
            .unwrap_or(host);
        format!(
            "https://{}/driver-proxy-api/o/0/{}/40001/api/v1",
            host, self.cluster_id
        )
    }
}

/// Resolve config from CLI args > env vars (handled by clap) > ~/.databrickscfg.
pub fn resolve_config() -> Result<Config, String> {
    let args = CliArgs::parse();

    // If all three are provided via CLI/env, we're done
    if let (Some(host), Some(token), Some(cluster_id)) = (&args.host, &args.token, &args.cluster_id)
    {
        return Ok(Config {
            host: host.clone(),
            token: token.clone(),
            cluster_id: cluster_id.clone(),
            poll_interval: args.poll_interval,
        });
    }

    // Try to fill gaps from ~/.databrickscfg
    let cfg_path = databrickscfg_path();
    let profiles = match cfg_path.as_ref().and_then(|p| parse_databrickscfg(p).ok()) {
        Some(profiles) => profiles,
        None => {
            return Err(missing_fields_error(&args));
        }
    };

    // Pick profile: explicit --profile flag, or auto-detect one with host+token+cluster_id
    let profile_name = args.profile.as_deref();
    let profile = match profile_name {
        Some(name) => profiles.get(name).ok_or_else(|| {
            let available: Vec<&str> = profiles.keys().map(|s| s.as_str()).collect();
            format!(
                "Profile '{}' not found in ~/.databrickscfg. Available: {}",
                name,
                available.join(", ")
            )
        })?,
        None => {
            // Auto-detect: find first profile with host + token + cluster_id
            match find_complete_profile(&profiles) {
                Some(p) => p,
                None => return Err(missing_fields_error(&args)),
            }
        }
    };

    let host = args
        .host
        .or_else(|| profile.get("host").cloned())
        .ok_or("Missing 'host'. Set --host, DATABRICKS_HOST, or configure ~/.databrickscfg")?;

    let token = args
        .token
        .or_else(|| profile.get("token").cloned())
        .ok_or("Missing 'token'. Set --token, DATABRICKS_TOKEN, or configure ~/.databrickscfg")?;

    let cluster_id = args
        .cluster_id
        .or_else(|| profile.get("cluster_id").cloned())
        .ok_or(
            "Missing 'cluster_id'. Set --cluster-id, DATABRICKS_CLUSTER_ID, or configure ~/.databrickscfg",
        )?;

    Ok(Config {
        host,
        token,
        cluster_id,
        poll_interval: args.poll_interval,
    })
}

fn missing_fields_error(args: &CliArgs) -> String {
    let mut missing = Vec::new();
    if args.host.is_none() {
        missing.push("host");
    }
    if args.token.is_none() {
        missing.push("token");
    }
    if args.cluster_id.is_none() {
        missing.push("cluster_id");
    }
    format!(
        "Missing required config: {}. Provide via CLI flags, env vars, or ~/.databrickscfg",
        missing.join(", ")
    )
}

/// Returns the path to ~/.databrickscfg if it exists.
fn databrickscfg_path() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let path = PathBuf::from(home).join(".databrickscfg");
    if path.exists() { Some(path) } else { None }
}

type Profile = HashMap<String, String>;

/// Parse ~/.databrickscfg (INI format) into a map of profile_name -> key/value pairs.
fn parse_databrickscfg(path: &PathBuf) -> Result<HashMap<String, Profile>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;

    let mut profiles: HashMap<String, Profile> = HashMap::new();
    let mut current_section: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        // Section header: [profile-name]
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].trim().to_string();
            current_section = Some(name.clone());
            profiles.entry(name).or_default();
            continue;
        }

        // Key = value
        #[allow(clippy::collapsible_if)]
        if let Some(ref section) = current_section {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                profiles.get_mut(section).unwrap().insert(key, value);
            }
        }
    }

    Ok(profiles)
}

/// Find the first profile that has host, token, and cluster_id set.
fn find_complete_profile(profiles: &HashMap<String, Profile>) -> Option<&Profile> {
    // Try DEFAULT first
    #[allow(clippy::collapsible_if)]
    if let Some(p) = profiles.get("DEFAULT") {
        if p.contains_key("host") && p.contains_key("token") && p.contains_key("cluster_id") {
            return Some(p);
        }
    }
    // Then try any other profile
    for (name, p) in profiles {
        if name == "DEFAULT" {
            continue;
        }
        if p.contains_key("host") && p.contains_key("token") && p.contains_key("cluster_id") {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_databrickscfg() {
        let mut tmp = tempfile();
        writeln!(
            tmp.1,
            r#"
; comment
[DEFAULT]

[my-profile]
host       = https://adb-123.6.azuredatabricks.net
cluster_id = 0530-080901-abc123
token      = dapi1234567890

[other]
host=https://adb-999.11.azuredatabricks.net/
auth_type=azure-cli
"#
        )
        .unwrap();
        tmp.1.flush().unwrap();

        let profiles = parse_databrickscfg(&tmp.0).unwrap();
        assert!(profiles.contains_key("DEFAULT"));
        assert!(profiles.contains_key("my-profile"));
        assert!(profiles.contains_key("other"));

        let p = &profiles["my-profile"];
        assert_eq!(p["host"], "https://adb-123.6.azuredatabricks.net");
        assert_eq!(p["cluster_id"], "0530-080901-abc123");
        assert_eq!(p["token"], "dapi1234567890");

        let o = &profiles["other"];
        assert_eq!(o["auth_type"], "azure-cli");
    }

    #[test]
    fn test_find_complete_profile() {
        let mut profiles = HashMap::new();

        // Incomplete profile (no token)
        let mut incomplete = HashMap::new();
        incomplete.insert("host".to_string(), "h".to_string());
        incomplete.insert("cluster_id".to_string(), "c".to_string());
        profiles.insert("incomplete".to_string(), incomplete);

        // Complete profile
        let mut complete = HashMap::new();
        complete.insert("host".to_string(), "h".to_string());
        complete.insert("token".to_string(), "t".to_string());
        complete.insert("cluster_id".to_string(), "c".to_string());
        profiles.insert("complete".to_string(), complete);

        let found = find_complete_profile(&profiles).unwrap();
        assert_eq!(found["token"], "t");
    }

    #[test]
    fn test_base_url_strips_scheme_and_trailing_slash() {
        let config = Config {
            host: "https://adb-123.azuredatabricks.net/".to_string(),
            token: "tok".to_string(),
            cluster_id: "abc".to_string(),
            poll_interval: 10,
        };
        assert_eq!(
            config.base_url(),
            "https://adb-123.azuredatabricks.net/driver-proxy-api/o/0/abc/40001/api/v1"
        );
    }

    /// Helper to create a temporary file and return (path, file).
    fn tempfile() -> (PathBuf, std::fs::File) {
        let path = std::env::temp_dir().join(format!("spark-tui-test-{}", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        (path, file)
    }
}
