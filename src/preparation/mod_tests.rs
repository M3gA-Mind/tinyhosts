//! Bounded directory preparation and deployment credential exclusions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

fn input(root: &Path) -> AuthorizedDirectory {
    AuthorizedDirectory {
        workspace: root.canonicalize().unwrap().to_string_lossy().into(),
        path: ".".into(),
        max_bytes: MAX_PREPARATION_BYTES,
        max_files: MAX_PREPARATION_FILES,
    }
}

#[test]
fn preparation_and_deployment_exclude_standard_credential_files_case_insensitively() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.html"), "safe").unwrap();
    let names = [
        ".npmrc",
        ".netrc",
        ".pypirc",
        ".git-credentials",
        ".Env.local",
        ".NPMRC",
    ];
    for name in names {
        std::fs::write(root.path().join(name), "secret").unwrap();
    }
    let prepared = prepare_bundle(&input(root.path())).unwrap();
    assert_eq!(prepared.file_count, 1);
    assert_eq!(prepared.bundle[0].path, "index.html");
    for name in names {
        let bundle = crate::Bundle::from_files(vec![
            crate::SiteFile::new(name, b"secret".to_vec()).unwrap(),
        ]);
        assert!(
            matches!(
                validate_deployment_bundle(&bundle),
                Err(Error::CredentialBundlePath { .. })
            ),
            "accepted {name}"
        );
    }
}

#[test]
fn preparation_preserves_source_bytes_exclusions_and_captured_snapshot() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("app")).unwrap();
    std::fs::write(root.path().join("app/index.bin"), [0, 1, 255]).unwrap();
    std::fs::write(root.path().join("package.json"), "{}").unwrap();
    for name in crate::EXCLUDED {
        std::fs::write(root.path().join(name), "secret").unwrap();
    }
    let legacy = crate::Bundle::from_dir(root.path()).unwrap();
    let expected: Vec<BundleFile> =
        serde_json::from_value(serde_json::to_value(legacy).unwrap()).unwrap();
    std::fs::create_dir(root.path().join(".ssh")).unwrap();
    std::fs::write(root.path().join(".ssh/id_rsa"), "credential").unwrap();
    let prepared = prepare_bundle(&input(root.path())).unwrap();
    assert_eq!(prepared.file_count, 2);
    assert_eq!(prepared.total_bytes, 5);
    assert_eq!(prepared.bundle, expected);
    assert!(
        prepared
            .bundle
            .iter()
            .any(|file| file.path == "app/index.bin" && file.contents == "AAH/")
    );
    std::fs::write(root.path().join("package.json"), "changed").unwrap();
    assert_eq!(prepared.bundle, expected);
    assert!(!format!("{prepared:?}").contains("AAH/"));
}

#[test]
fn invalid_scopes_and_bounded_file_reads_are_refused() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("source"), "12345").unwrap();
    for path in [
        "../outside",
        "/outside",
        "C:\\outside",
        "a\\..\\b",
        "\0",
        ".ssh",
        ".aws",
        ".gnupg",
    ] {
        let mut request = input(root.path());
        request.path = path.into();
        assert!(prepare_bundle(&request).is_err(), "accepted {path}");
    }
    let mut request = input(root.path());
    request.workspace = "relative".into();
    assert!(prepare_bundle(&request).is_err());
    request = input(root.path());
    request.max_bytes = 4;
    assert!(matches!(
        prepare_bundle(&request),
        Err(Error::PreparationLimit { .. })
    ));
    request.max_bytes = 5;
    request.max_files = 0;
    assert!(matches!(
        prepare_bundle(&request),
        Err(Error::PreparationLimit { .. })
    ));
    request.max_files = MAX_PREPARATION_FILES + 1;
    assert!(matches!(
        prepare_bundle(&request),
        Err(Error::PreparationLimit { .. })
    ));
    request.max_files = MAX_PREPARATION_FILES;
    request.max_bytes = MAX_PREPARATION_BYTES + 1;
    assert!(matches!(
        prepare_bundle(&request),
        Err(Error::PreparationLimit { .. })
    ));
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(
        prepare_bundle(&input(empty.path())),
        Err(Error::EmptyBundle)
    ));
}

#[cfg(unix)]
#[test]
fn source_directory_symlinks_are_rejected_and_nested_symlinks_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "outside").unwrap();
    std::fs::write(root.path().join("source"), "inside").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret"),
        root.path().join("linked-file"),
    )
    .unwrap();
    let prepared = prepare_bundle(&input(root.path())).unwrap();
    assert_eq!(prepared.file_count, 1);
    assert_eq!(prepared.skipped_entries, 2);
    let mut request = input(root.path());
    request.path = "linked".into();
    assert!(prepare_bundle(&request).is_err());
    request.path = "source".into();
    assert!(prepare_bundle(&request).is_err());
}

#[cfg(unix)]
#[test]
fn workspace_symlink_is_refused_before_reading_credential_contents() {
    let parent = tempfile::tempdir().unwrap();
    let credentials = parent.path().join(".ssh");
    std::fs::create_dir(&credentials).unwrap();
    std::fs::write(credentials.join("id_rsa"), "credential").unwrap();
    let alias = parent.path().join("approved");
    std::os::unix::fs::symlink(&credentials, &alias).unwrap();
    assert!(matches!(
        prepare_bundle(&input(&alias)),
        Err(Error::PreparationPath { .. })
    ));
}

#[test]
fn long_relative_paths_are_refused_before_collection() {
    let root = tempfile::tempdir().unwrap();
    // Directory handles avoid the host's absolute path limit while constructing
    // a relative path beyond the preparation budget.
    let mut dir = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    for _ in 0..6 {
        let name = "x".repeat(200);
        dir.create_dir(&name).unwrap();
        dir = dir.open_dir(&name).unwrap();
    }
    dir.write("file", "data").unwrap();
    assert!(matches!(
        prepare_bundle(&input(root.path())),
        Err(Error::PreparationLimit { .. })
    ));
}

#[test]
fn serialization_budget_is_charged_before_publishing_encoded_files() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), "data").unwrap();
    let dir = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let mut out = PreparedBundle {
        contract_version: (1, 1),
        bundle: vec![],
        file_count: 0,
        total_bytes: 0,
        skipped_entries: 0,
        scanned_entries: 0,
    };
    let mut charged = MAX_PREPARATION_JSON_BYTES;
    assert!(matches!(
        collect_file(
            &dir,
            "file",
            "file",
            &input(root.path()),
            &mut out,
            &mut charged
        ),
        Err(Error::PreparationLimit { .. })
    ));
    assert_eq!(out.file_count, 0);
    assert_eq!(out.bundle, [] as [tinyhosts_bus::inputs::BundleFile; 0]);
    charged = 512;
    collect_file(
        &dir,
        "file",
        "file",
        &input(root.path()),
        &mut out,
        &mut charged,
    )
    .unwrap();
    assert!(serde_json::to_vec(&out).unwrap().len() < charged);
    out.scanned_entries = MAX_PREPARATION_ENTRIES;
    assert!(matches!(
        collect(&dir, "", 0, &input(root.path()), &mut out, &mut charged),
        Err(Error::PreparationLimit { .. })
    ));
    assert!(matches!(
        collect(
            &dir,
            "",
            MAX_PREPARATION_DEPTH + 1,
            &input(root.path()),
            &mut out,
            &mut charged
        ),
        Err(Error::PreparationLimit { .. })
    ));
    assert!(
        collect_file(
            &dir,
            "missing",
            "missing",
            &input(root.path()),
            &mut out,
            &mut charged
        )
        .is_err()
    );
}

// Linux filesystems accept arbitrary filename bytes; macOS APFS rejects them.
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_names_are_refused_without_lossy_path_rewriting() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(std::ffi::OsString::from_vec(vec![255])),
        "data",
    )
    .unwrap();
    assert!(matches!(
        prepare_bundle(&input(root.path())),
        Err(Error::PreparationPath { .. })
    ));
}

#[test]
fn supplied_bundle_security_floor_covers_environment_and_platform_credentials() {
    for path in [
        ".env",
        "app/.env.production",
        ".AWS/token",
        "Microsoft/Protect/key",
        "Microsoft/Vault/key",
        "Keychains/key",
    ] {
        let bundle =
            crate::Bundle::from_files(vec![SiteFile::new(path, b"credential".to_vec()).unwrap()]);
        assert!(
            validate_deployment_bundle(&bundle).is_err(),
            "accepted {path}"
        );
    }
    let bundle =
        crate::Bundle::from_files(vec![SiteFile::new("app/index", b"safe".to_vec()).unwrap()]);
    validate_deployment_bundle(&bundle).unwrap();
}

#[cfg(unix)]
#[test]
fn source_replaced_by_fifo_is_refused_without_waiting_for_writer() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(root.path().join("file"))
            .status()
            .unwrap()
            .success()
    );
    let request = input(root.path());
    let (send, receive) = std::sync::mpsc::channel();
    let dir = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let worker = std::thread::spawn(move || {
        let mut out = PreparedBundle {
            contract_version: (1, 1),
            bundle: vec![],
            file_count: 0,
            total_bytes: 0,
            skipped_entries: 0,
            scanned_entries: 0,
        };
        let result = collect_file(&dir, "file", "file", &request, &mut out, &mut 512);
        send.send(result).unwrap();
    });
    assert!(matches!(
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        Err(Error::PreparationPath { .. })
    ));
    worker.join().unwrap();
}

#[test]
fn credential_refusal_is_distinct_from_malformed_bundle_paths_without_exposing_paths() {
    for path in ["app/.env.local", ".ssh/id_rsa"] {
        let bundle =
            crate::Bundle::from_files(vec![SiteFile::new(path, b"private".to_vec()).unwrap()]);
        let error = validate_deployment_bundle(&bundle).unwrap_err();
        assert_eq!(error.to_string(), "bundle contains credential material");
        assert!(!format!("{error:?}").contains(path));
    }
}

#[cfg(unix)]
#[test]
fn source_name_rejects_non_utf8_without_rewriting_bytes() {
    use std::os::unix::ffi::OsStringExt;
    assert!(matches!(
        source_name(std::ffi::OsString::from_vec(vec![255])),
        Err(Error::PreparationPath { .. })
    ));
    assert_eq!(source_name("index.html".into()).unwrap(), "index.html");
}
