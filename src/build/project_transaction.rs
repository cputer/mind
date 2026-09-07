// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Preparation shared by manifest-backed and standalone build transactions.

use super::{BuildError, build_synthetic_manifest};
use crate::project::{ProjectManifest, load_manifest};
use std::path::{Path, PathBuf};

pub(super) use crate::project::build_lock::ManifestEdit;
use crate::project::build_lock::ProjectBuildLock;

/// What the project-transaction boundary hands the rest of a build.
pub(super) struct BuildTransaction {
    /// Canonical project root this build compiles under.
    pub(super) root: PathBuf,
    /// The manifest governing it — loaded from disk, or synthesised.
    pub(super) manifest: ProjectManifest,
    /// Held for the whole build; dropped when the caller's binding is.
    pub(super) lock: ProjectBuildLock,
    /// The "project" is exactly the named entry, never its directory.
    pub(super) single_file: bool,
}

/// Transaction identity of an explicit `mindc build <file>` source: the
/// canonical directory holding it, plus the entry re-spelled inside that
/// directory.
///
/// Only the directory is canonicalised. The leaf keeps the spelling the
/// operator asked for, because a `.mind` entry may legitimately be a symlink to
/// a source elsewhere in the project and resolving it would retarget the build
/// at a file the requested directory's manifest does not govern.
///
/// Sharing [`crate::project::canonical_dir`] with `find_project_root_for_file`
/// is the point: root and entry directory are then two results of one function,
/// so `root == entry_dir` tests directory identity. A lexically derived entry
/// directory compares unequal to the root whenever a component is a symlink,
/// which defeats the vanished-temporary-manifest fallback and mis-classifies a
/// co-located bare manifest as a whole-directory project.
pub(super) fn canonical_explicit_entry(requested: &Path, cwd: &Path) -> (PathBuf, PathBuf) {
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        cwd.join(requested)
    };
    let lexical_dir = absolute
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cwd.to_path_buf());
    let entry_dir = crate::project::canonical_dir(&lexical_dir).unwrap_or(lexical_dir);
    let entry_path = match absolute.file_name() {
        Some(name) => entry_dir.join(name),
        None => absolute,
    };
    (entry_dir, entry_path)
}

/// Open the build transaction for an explicit `mindc build <file>`.
///
/// `probed_root` is `find_project_root_for_file(entry_dir)` evaluated before any
/// lock is held. It is a parameter rather than a call inside this function
/// because the gap between probe and lock is the window a concurrent standalone
/// build lives in: that build holds this same lock while its temporary
/// `Mind.toml` exists and removes the file before releasing. Passing the probe
/// in lets a test interpose an explicit barrier in that window and reproduce the
/// disappearance against the real lock, with no timing-dependent sleep.
///
/// `entry_dir` must be the canonical directory of `entry_path`
/// (`canonical_explicit_entry`) — the same function `probed_root` was derived
/// through, so the `root == entry_dir` tests below compare directory identity.
pub(super) fn open_explicit(
    probed_root: Option<PathBuf>,
    entry_dir: &Path,
    entry_path: &Path,
) -> Result<BuildTransaction, BuildError> {
    match probed_root {
        Some(root) => {
            let lock = lock_project(&root)?;
            // Re-audit the manifest after the lock, not before: the `Mind.toml`
            // the probe saw may have been a concurrent standalone build's
            // temporary manifest, which that build removes while still holding
            // this lock. A root that is the entry's own directory and now has no
            // manifest is that case, and the build proceeds on a synthesised
            // single-file manifest. Any other root is a genuine project whose
            // manifest disappearing is a real error, so it falls through to
            // `load_manifest` and fails loudly. Both arms depend on `root` and
            // `entry_dir` being one identity; a lexical `entry_dir` made the
            // co-location test false for every standalone build under a
            // symlinked parent.
            let manifest = if root == *entry_dir && !root.join("Mind.toml").exists() {
                single_file_manifest(entry_path)?
            } else {
                load_manifest(&root)
                    .map_err(|e| BuildError::Invalid(format!("manifest error: {e}")))?
            };
            // A named file sitting directly next to a *bare* manifest (one
            // declaring no `[targets.*].sources` list) is a single-file build:
            // that manifest is either a real one-file project or a leftover
            // synthetic `Mind.toml` from a prior `mindc build <file>` in a
            // scratch directory. Otherwise the next build of a trivial file
            // there adopts the leftover as a "project" and compiles every
            // unrelated sibling as a translation unit. A genuine multi-file
            // project keeps the whole-directory walk: its entry lives in a
            // subtree (root != the file's own dir) or it declares sources.
            let declares_sources = manifest.targets.values().any(|t| t.sources.is_some());
            let single_file = !declares_sources && root == *entry_dir;
            Ok(BuildTransaction {
                root,
                manifest,
                lock,
                single_file,
            })
        }
        None => {
            // The project lock also protects standalone builds. The manifest
            // is synthesised, so the selected scope is exactly the
            // named entry: a `Mind.toml` appearing in this directory while we
            // waited cannot widen this build's source set. Later stages still
            // read the root — the input snapshot, the cache root, the manifest
            // write — but as the root of that synthesised single-file project.
            let lock = lock_project(entry_dir)?;
            // No governing manifest in-bounds — synthesise a single-file
            // manifest rooted at the entry file's OWN directory (never cwd nor
            // a distant ancestor), so source collection stays scoped to it
            // rather than to whatever tree happens to sit above.
            let manifest = single_file_manifest(entry_path)?;
            Ok(BuildTransaction {
                root: entry_dir.to_path_buf(),
                manifest,
                lock,
                single_file: true,
            })
        }
    }
}

fn single_file_manifest(entry: &Path) -> Result<ProjectManifest, BuildError> {
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

pub(super) fn lock_project(root: &Path) -> Result<ProjectBuildLock, BuildError> {
    ProjectBuildLock::acquire(root)
        .map_err(|e| BuildError::failed(format!("cannot lock project build: {e}")))
}
