// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Preparation shared by manifest-backed and standalone build transactions.

use super::{BuildError, build_synthetic_manifest};
use std::path::Path;

pub(super) use crate::project::build_lock::ManifestEdit;

pub(super) fn single_file_manifest(
    entry: &Path,
) -> Result<crate::project::ProjectManifest, BuildError> {
    let stem = entry
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .replace('-', "_");
    let pkg_name = if stem
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
    {
        stem
    } else {
        format!("pkg_{stem}")
    };
    toml::from_str(&build_synthetic_manifest(&pkg_name, "src/main.mind"))
        .map_err(|e| BuildError::Invalid(format!("synthetic manifest: {e}")))
}

pub(super) fn lock_project(
    root: &Path,
) -> Result<crate::project::build_lock::ProjectBuildLock, BuildError> {
    crate::project::build_lock::ProjectBuildLock::acquire(root)
        .map_err(|e| BuildError::failed(format!("cannot lock project build: {e}")))
}
