// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Canonical directory identity and bounded explicit-file project discovery.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// The one directory identity used at a build-transaction boundary.
pub(crate) fn canonical_dir(dir: &Path) -> Result<Option<PathBuf>> {
    let absolute = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        let Ok(cwd) = std::env::current_dir() else {
            return Ok(None);
        };
        cwd.join(dir)
    };
    Ok(Some(canonicalize_dir_with_missing_fallback(
        absolute,
        |path| path.canonicalize(),
    )?))
}

/// Preserve missing-parent discovery while propagating all other identity errors.
pub(crate) fn canonicalize_dir_with_missing_fallback<F>(
    absolute: PathBuf,
    canonicalize: F,
) -> Result<PathBuf>
where
    F: FnOnce(&Path) -> std::io::Result<PathBuf>,
{
    match canonicalize(&absolute) {
        Ok(canonical) => Ok(canonical),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(absolute),
        Err(error) => Err(anyhow::Error::new(error).context(format!(
            "cannot canonicalize directory {}",
            absolute.display()
        ))),
    }
}

/// Resolve the governing project root for an explicit source within a bounded
/// repository or a co-located scratch manifest.
pub fn find_project_root_for_file(entry_dir: &Path) -> Option<PathBuf> {
    find_project_root_for_file_checked(entry_dir).ok().flatten()
}

/// Fallible companion used by build and project-scope callers.
pub(crate) fn find_project_root_for_file_checked(entry_dir: &Path) -> Result<Option<PathBuf>> {
    let Some(entry_dir) = canonical_dir(entry_dir)? else {
        return Ok(None);
    };

    let mut git_root: Option<PathBuf> = None;
    let mut probe = entry_dir.clone();
    loop {
        if probe.join(".git").exists() {
            git_root = Some(probe.clone());
            break;
        }
        if !probe.pop() {
            break;
        }
    }

    match git_root {
        Some(root) => {
            let mut current = entry_dir.clone();
            loop {
                if current.join("Mind.toml").exists() {
                    return Ok(Some(current));
                }
                if current == root {
                    return Ok(None);
                }
                if !current.pop() {
                    return Ok(None);
                }
            }
        }
        None => {
            if entry_dir.join("Mind.toml").exists() {
                Ok(Some(entry_dir))
            } else {
                Ok(None)
            }
        }
    }
}
