//! Exercise the shared release caller's tag validation.
use std::process::Command;

const RELEASE: &str = include_str!("../.github/workflows/release.yml");

fn resolve(tag: &str) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let body = RELEASE
        .split("        run: |\n")
        .nth(1)
        .ok_or("missing release resolver")?
        .lines()
        .take_while(|line| line.starts_with("          "))
        .map(|line| &line[10..])
        .collect::<Vec<_>>()
        .join("\n");
    let output = tempfile::NamedTempFile::new()?;
    let result = Command::new("bash")
        .args(["-c", &body])
        .env("TAG", tag)
        .env("GITHUB_OUTPUT", output.path())
        .output()?;
    if result.status.success() {
        let values = std::fs::read_to_string(output.path())?;
        assert_eq!(values, format!("tag={tag}\nversion={}\n", &tag[1..]));
    }
    Ok(result)
}

#[test]
fn reviewed_version_tags_resolve_without_modifying_the_workspace()
-> Result<(), Box<dyn std::error::Error>> {
    for tag in ["v0.3.0", "v1.0.0", "v1.0.0-beta.1"] {
        assert!(resolve(tag)?.status.success());
    }
    assert!(!RELEASE.contains("git push"));
    assert!(!RELEASE.contains("perl -0pi"));
    Ok(())
}

#[test]
fn release_rejects_non_version_refs_and_shell_text() -> Result<(), Box<dyn std::error::Error>> {
    for tag in ["", "main", "../escape", "v1.0.0; echo injected", "v1.0"] {
        assert!(!resolve(tag)?.status.success());
    }
    Ok(())
}

#[test]
fn the_caller_selects_the_host_module_and_canonical_release_pipeline() {
    assert!(RELEASE.contains("module-crate: tinyhosts"));
    assert!(RELEASE.contains("tinyhumansai/.github/.github/workflows/module-release.yml@"));
    assert!(!RELEASE.contains("native-bundles:"));
    assert!(!RELEASE.contains("include-distros: false"));
}
