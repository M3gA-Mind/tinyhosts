//! Exercise release shell snippets against local workspace fixtures.

use std::process::Command;

const RELEASE: &str = include_str!("../.github/workflows/release.yml");

#[test]
fn release_package_selection_is_independent_of_workspace_metadata_order()
-> Result<(), Box<dyn std::error::Error>> {
    let assignments = RELEASE
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("crate_name=\"") || line.starts_with("current_version=\""))
        .collect::<Vec<_>>()
        .join("\n");
    let script = format!(
        "set -euo pipefail\nmetadata=\"$RELEASE_TEST_METADATA\"\n{assignments}\nprintf '%s %s' \"$crate_name\" \"$current_version\""
    );
    let output = Command::new("bash").args(["-c", &script])
        .env("RELEASE_TEST_METADATA", r#"{"packages":[{"name":"tinyhosts-bus","version":"0.2.0"},{"name":"tinyhosts","version":"0.2.3"}]}"#)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout)?, "tinyhosts 0.2.3");
    Ok(())
}

#[test]
fn release_minor_and_major_bumps_keep_the_local_contract_dependency_compatible()
-> Result<(), Box<dyn std::error::Error>> {
    let version_commands = RELEASE
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("perl -0pi"))
        .collect::<Vec<_>>()
        .join("\n");
    for next in ["0.3.0", "1.0.0"] {
        let fixture = tempfile::tempdir()?;
        std::fs::create_dir_all(fixture.path().join("crates/tinyhosts-bus/src"))?;
        std::fs::create_dir(fixture.path().join("src"))?;
        std::fs::write(fixture.path().join("src/lib.rs"), "")?;
        std::fs::write(fixture.path().join("crates/tinyhosts-bus/src/lib.rs"), "")?;
        std::fs::write(
            fixture.path().join("Cargo.toml"),
            "[package]\nname = \"tinyhosts\"\nversion = \"0.2.3\"\nedition = \"2024\"\n[workspace]\nmembers = [\"crates/tinyhosts-bus\"]\n[dependencies]\ntinyhosts-bus = { path = \"crates/tinyhosts-bus\", version = \"0.2\" }\n",
        )?;
        std::fs::write(
            fixture.path().join("crates/tinyhosts-bus/Cargo.toml"),
            "[package]\nname = \"tinyhosts-bus\"\nversion = \"0.2.3\"\nedition = \"2024\"\n",
        )?;
        let updated = Command::new("bash")
            .args(["-c", &format!("set -euo pipefail\n{version_commands}")])
            .current_dir(fixture.path())
            .env("NEXT_VERSION", next)
            .output()?;
        assert!(
            updated.status.success(),
            "{}",
            String::from_utf8_lossy(&updated.stderr)
        );
        let metadata = Command::new("cargo")
            .args(["metadata", "--format-version", "1", "--offline"])
            .current_dir(fixture.path())
            .output()?;
        assert!(
            metadata.status.success(),
            "{}",
            String::from_utf8_lossy(&metadata.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
        for package in value["packages"].as_array().ok_or("missing packages")? {
            assert_eq!(package["version"], next);
        }
    }
    Ok(())
}
