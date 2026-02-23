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
        event_log_path: None,
        sparkui_cookie: None,
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
