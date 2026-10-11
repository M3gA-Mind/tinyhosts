//! Verify the digest gate used by the archive verification example.
#![cfg(all(feature = "module", unix))]

use tinybus::broker::Broker;
use tinybus::module::ModuleHost;

#[test]
fn archive_library_is_rejected_before_loading_when_its_manifest_digest_differs()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let library_name = if cfg!(target_os = "macos") {
        "libtinyhosts.dylib"
    } else {
        "libtinyhosts.so"
    };
    let library = directory.path().join(library_name);
    std::fs::write(&library, b"original library bytes")?;
    let digest = tinybus::module::sha256_file(&library)?;
    std::fs::write(
        directory.path().join("modules.toml"),
        format!("\"{library_name}\" = \"{digest}\"\n"),
    )?;
    std::fs::write(&library, b"modified library bytes")?;

    // This is the same entrypoint used by examples/verify_module.rs. The
    // fixture is deliberately not a library: reaching the native loader would
    // produce a different error, proving the allowlist gate ran too late.
    let host = ModuleHost::new(Broker::new());
    let error = host.load_file(&library).unwrap_err();
    assert!(
        matches!(
            &error,
            tinybus::Error::ModuleRefused { reason, .. }
                if reason == "artifact hash does not match the module allowlist"
        ),
        "unexpected refusal: {error}"
    );
    Ok(())
}
