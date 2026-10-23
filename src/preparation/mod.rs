//! Bounded collection of a host-authorized source directory.
//!
//! Authorization and external-effect approval precede this call. The returned
//! bundle is the prepared snapshot: pass its exact bytes to Launch/Deploy rather
//! than collecting the directory again. This operation never connects a provider
//! or looks up credentials. Directory-handle traversal prevents symlink escapes.
use std::io::Read;
use std::path::{Component, Path};

use base64::Engine as _;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use tinyhosts_bus::inputs::BundleFile;
use tinyhosts_bus::preparation::{
    AuthorizedDirectory, MAX_PREPARATION_BYTES, MAX_PREPARATION_DEPTH, MAX_PREPARATION_ENTRIES,
    MAX_PREPARATION_FILES, MAX_PREPARATION_JSON_BYTES, MAX_PREPARATION_PATH_BYTES, PreparedBundle,
};

use crate::{Error, Result, SiteFile};

/// Collect the bounded source snapshot inside an already-authorized directory.
///
/// # Errors
/// Refuses invalid scopes, symlinks, credential roots, unreadable files, empty
/// bundles and byte/file/depth budgets. No provider request is performed.
pub fn prepare_bundle(input: &AuthorizedDirectory) -> Result<PreparedBundle> {
    validate(input)?;
    let workspace = Path::new(&input.workspace);
    let canonical = workspace
        .canonicalize()
        .map_err(|error| read_error(&input.workspace, &error))?;
    if canonical != workspace || credential_path(&canonical.to_string_lossy()) {
        return Err(invalid(
            "workspace must be canonical and outside credential directories",
        ));
    }
    // Anchor at the volume root and open every component without following links,
    // including the workspace itself: a concurrent root alias cannot change scope.
    let volume = workspace
        .ancestors()
        .last()
        .ok_or_else(|| invalid("missing volume root"))?;
    let mut source = Dir::open_ambient_dir(volume, cap_std::ambient_authority())
        .map_err(|error| read_error(&input.workspace, &error))?;
    for component in workspace.components() {
        if let Component::Normal(name) = component {
            source = source
                .open_dir_nofollow(name)
                .map_err(|error| read_error(&input.workspace, &error))?;
        }
    }
    for component in Path::new(&input.path.replace('\\', "/")).components() {
        if let Component::Normal(name) = component {
            source = source
                .open_dir_nofollow(name)
                .map_err(|error| read_error(&input.path, &error))?;
        }
    }
    let mut prepared = PreparedBundle {
        contract_version: tinyhosts_bus::WIRE_CONTRACT_VERSION,
        bundle: Vec::new(),
        file_count: 0,
        total_bytes: 0,
        skipped_entries: 0,
        scanned_entries: 0,
    };
    let mut wire_bytes = 512; // Reserve envelope/facts space, including integer widths.
    collect(&source, "", 0, input, &mut prepared, &mut wire_bytes)?;
    if prepared.bundle.is_empty() {
        return Err(Error::EmptyBundle);
    }
    Ok(prepared)
}

fn invalid(reason: &str) -> Error {
    Error::PreparationPath {
        reason: reason.into(),
    }
}
fn limit(name: &str) -> Error {
    Error::PreparationLimit { limit: name.into() }
}
fn read_error(path: &str, error: &impl std::fmt::Display) -> Error {
    Error::ReadBundle {
        path: path.into(),
        reason: error.to_string(),
    }
}

fn validate(input: &AuthorizedDirectory) -> Result<()> {
    if input.workspace.contains('\0') || input.path.contains('\0') {
        return Err(invalid("null byte in directory scope"));
    }
    if !Path::new(&input.workspace).is_absolute() {
        return Err(invalid("workspace must be absolute"));
    }
    let normalized = input.path.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.split('/').any(|part| part == "..")
    {
        return Err(invalid("source must be relative without traversal"));
    }
    if input
        .workspace
        .replace('\\', "/")
        .split('/')
        .chain(normalized.split('/'))
        .any(credential_entry)
    {
        return Err(invalid("credential directory cannot be prepared"));
    }
    if credential_path(&format!("{}/{}", input.workspace, normalized)) {
        return Err(invalid("credential directory cannot be prepared"));
    }
    if input.max_bytes > MAX_PREPARATION_BYTES || input.max_files > MAX_PREPARATION_FILES {
        return Err(limit("maximum preparation budget"));
    }
    Ok(())
}

fn credential_entry(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        ".ssh"
            | ".aws"
            | ".gnupg"
            | ".azure"
            | ".kube"
            | "keychains"
            | ".env"
            | ".npmrc"
            | ".netrc"
            | ".pypirc"
            | ".git-credentials"
    ) || name.starts_with(".env.")
}

pub(crate) fn credential_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let parts: Vec<_> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    parts.iter().any(|part| credential_entry(part))
        || parts.windows(2).any(|pair| {
            pair[0] == "microsoft"
                && matches!(pair[1], "protect" | "credentials" | "crypto" | "vault")
        })
}

pub(crate) fn validate_deployment_bundle(bundle: &crate::Bundle) -> Result<()> {
    for file in bundle.files() {
        if credential_path(file.path())
            || file
                .path()
                .split('/')
                .any(|part| part == ".env" || part.starts_with(".env."))
        {
            return Err(Error::CredentialBundlePath {
                path: file.path().into(),
            });
        }
    }
    Ok(())
}

pub(crate) fn excluded(name: &str) -> bool {
    crate::EXCLUDED.contains(&name) || name.starts_with(".env.") || credential_entry(name)
}

fn source_name(name: std::ffi::OsString) -> Result<String> {
    name.into_string()
        .map_err(|_| invalid("non-UTF-8 source name"))
}

fn collect(
    dir: &Dir,
    prefix: &str,
    depth: usize,
    input: &AuthorizedDirectory,
    out: &mut PreparedBundle,
    wire_bytes: &mut usize,
) -> Result<()> {
    if depth > MAX_PREPARATION_DEPTH {
        return Err(limit("directory depth"));
    }
    let entries = dir.entries().map_err(|error| read_error(prefix, &error))?;
    for entry in entries {
        if out.scanned_entries >= MAX_PREPARATION_ENTRIES {
            return Err(limit("entry count"));
        }
        out.scanned_entries += 1;
        let entry = entry.map_err(|error| read_error(prefix, &error))?;
        let name = source_name(entry.file_name())?;
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if path.len() > MAX_PREPARATION_PATH_BYTES {
            return Err(limit("relative path bytes"));
        }
        let kind = entry
            .file_type()
            .map_err(|error| read_error(&path, &error))?;
        if excluded(&name)
            || credential_path(&path)
            || kind.is_symlink()
            || (!kind.is_dir() && !kind.is_file())
        {
            out.skipped_entries = out.skipped_entries.saturating_add(1);
        } else if kind.is_dir() {
            let child = dir
                .open_dir_nofollow(&name)
                .map_err(|error| read_error(&path, &error))?;
            collect(&child, &path, depth + 1, input, out, wire_bytes)?;
        } else {
            collect_file(dir, &name, &path, input, out, wire_bytes)?;
        }
    }
    Ok(())
}

fn collect_file(
    dir: &Dir,
    name: &str,
    path: &str,
    input: &AuthorizedDirectory,
    out: &mut PreparedBundle,
    wire_bytes: &mut usize,
) -> Result<()> {
    if out.file_count >= input.max_files {
        return Err(limit("file count"));
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let file = dir
        .open_with(name, &options)
        .map_err(|error| read_error(path, &error))?;
    if !file
        .metadata()
        .map_err(|error| read_error(path, &error))?
        .is_file()
    {
        return Err(invalid("source changed to a non-regular file"));
    }
    let remaining = input.max_bytes.saturating_sub(out.total_bytes);
    let mut bytes = Vec::new();
    file.take(remaining + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| read_error(path, &error))?;
    if bytes.len() as u64 > remaining {
        return Err(limit("source bytes"));
    }
    let file = SiteFile::new(path, bytes)?;
    // Charge escaped path JSON, base64 expansion, punctuation and field names
    // before allocating the encoded String or growing the returned bundle.
    let path_bytes = serde_json::to_string(file.path())
        .map_err(|error| Error::Envelope {
            reason: error.to_string(),
        })?
        .len();
    let next_bytes = path_bytes + file.len().div_ceil(3) * 4 + 32;
    if next_bytes > MAX_PREPARATION_JSON_BYTES.saturating_sub(*wire_bytes) {
        return Err(limit("serialized snapshot bytes"));
    }
    *wire_bytes += next_bytes;
    out.total_bytes += file.len() as u64;
    out.file_count += 1;
    out.bundle.push(BundleFile {
        path: file.path().into(),
        contents: base64::engine::general_purpose::STANDARD.encode(file.contents()),
    });
    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
