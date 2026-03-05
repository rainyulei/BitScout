//! End-to-end test: verify rg output format assumptions
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
#[ignore] // run with: cargo test -- --ignored
fn test_rg_output_format_assumptions() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("test.rs"),
        "fn hello_world() {}\nfn goodbye() {}",
    )
    .unwrap();

    // Verify real rg is available and produces expected output
    let output = Command::new("rg")
        .args(&["hello", dir.path().to_str().unwrap()])
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            assert!(
                stdout.contains("hello_world"),
                "rg output should contain match"
            );
            assert!(o.status.success(), "rg should exit with 0");
        }
        Err(_) => {
            eprintln!("rg not installed, skipping test");
        }
    }
}

#[test]
#[ignore]
fn test_rg_json_output_format() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "fn login() {}\nfn logout() {}").unwrap();

    let output = Command::new("rg")
        .args(&["--json", "login", dir.path().to_str().unwrap()])
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // Verify JSON lines format
            for line in stdout.lines() {
                let parsed: serde_json::Value =
                    serde_json::from_str(line).expect("each line should be valid JSON");
                assert!(
                    parsed.get("type").is_some(),
                    "each JSON line should have 'type' field"
                );
            }
        }
        Err(_) => {
            eprintln!("rg not installed, skipping test");
        }
    }
}
