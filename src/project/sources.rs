// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0

//! Project source-set resolution — the single owner of "which `.mind` files
//! are translation units of this build, and in what ORDER".
//!
//! Source ORDER is artifact-visible: it fixes the per-file compile sequence,
//! the object list handed to the linker, and (under `cross-module-imports`)
//! the fold order of the whole-project enum/const registries. A source set
//! that depends on `readdir` order therefore makes the emitted bytes depend on
//! filesystem layout — the cross-substrate byte-identity claim would hold only
//! by luck on two machines that happened to hash directory entries the same
//! way. [`collect_sources`] closes that by sorting the walked set on full path
//! bytes before it is ever returned.
//!
//! Two orders exist, and only one is sorted:
//!  - the DECLARED order of `[targets.*].sources` — the author's contract,
//!    preserved verbatim (see `resolve_declared_sources`);
//!  - the WALKED order of the entry-parent directory tree — no author intent
//!    exists, so it is canonicalised by sorting.
//!
//! [`resolve_sources`] is the one place that chooses between them; both the
//! legacy `build_project` compile path and the `run_build` cache key derive
//! their source set from it, so a build can never be keyed on one source set
//! and compiled from another.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

/// Collect all .mind source files from a project
pub fn collect_sources(project_root: &Path, entry: &str) -> Result<Vec<PathBuf>> {
    let entry_path = project_root.join(entry);
    if !entry_path.exists() {
        return Err(anyhow!("Entry file not found: {}", entry_path.display()));
    }

    let src_dir = entry_path.parent().unwrap_or(project_root);
    let mut sources = Vec::new();

    fn collect_recursive(dir: &Path, sources: &mut Vec<PathBuf>) -> Result<()> {
        if dir.is_dir() {
            // Skip a directory we cannot read (e.g. a root-owned `/tmp/systemd-private-*`
            // sibling of a source compiled from `/tmp`) instead of failing the whole
            // build with EACCES — an unreadable sibling dir holds no MIND sources we
            // could import, so silently excluding it keeps `mindc` usable from any cwd.
            let rd = match fs::read_dir(dir) {
                Ok(rd) => rd,
                Err(e) => {
                    eprintln!("mindc: skipping unreadable dir {}: {e}", dir.display());
                    return Ok(());
                }
            };
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Prune non-source subtrees: the build output dir (`target/`),
                    // version control (`.git/`) and any hidden `.dir` never hold
                    // importable MIND modules. Descending them only inflates the
                    // walk — the `.git` of a repo alone can be tens of thousands of
                    // objects. Defense-in-depth alongside the bounded root: even a
                    // legitimately large project dir stays cheap to collect.
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if name == "target" || name.starts_with('.') {
                        continue;
                    }
                    collect_recursive(&path, sources)?;
                } else if path.extension().map(|e| e == "mind").unwrap_or(false) {
                    sources.push(path);
                }
            }
        }
        Ok(())
    }

    collect_recursive(src_dir, &mut sources)?;

    // Canonicalise the walked order. `fs::read_dir` yields entries in whatever
    // order the filesystem stores them — creation order on tmpfs, name-HASH
    // order on ext4/xfs (seeded per filesystem), arbitrary elsewhere — so the
    // pre-sort order of this set was a property of the DISK, not of the
    // project. Every downstream consumer is order-sensitive (compile sequence,
    // link object order, the whole-project enum/const registry fold), so the
    // same sources on two machines could emit different bytes.
    //
    // Sort on the FULL path as raw `OsStr` bytes, not on `Path`'s
    // component-wise `Ord` and not on any locale collation: byte order is the
    // one total order that is identical on every host and every locale, which
    // is exactly the property the cross-substrate byte-identity claim needs.
    // The traversal order above is therefore irrelevant by construction — this
    // is the single point that fixes it.
    sources.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    Ok(sources)
}

/// Resolve a target's explicitly declared `sources = [...]` list against the
/// project root, preserving DECLARED order (no sorting — the manifest order is
/// the author's contract). Every declared path must exist: a missing declared
/// source fails the build loudly, because a silently dropped module is exactly
/// the class of bug an explicit source list exists to prevent. The manifest
/// entry is appended when the list omits it, so the entry is always a
/// translation unit of the build (the compile loop keys `is_entry` off it).
fn resolve_declared_sources(
    project_root: &Path,
    declared: &[String],
    entry: &str,
) -> Result<Vec<PathBuf>> {
    let mut sources: Vec<PathBuf> = Vec::with_capacity(declared.len() + 1);
    for decl in declared {
        // Contract: project-root-relative, inside the root, no duplicates.
        // An absolute or `..`-escaping path would fall outside the
        // project-root-relative keying downstream (module keys and object
        // names would silently derive from machine-dependent absolute paths),
        // and a duplicate would compile twice into ONE object name — both are
        // author errors worth failing loudly at the boundary.
        let decl_path = Path::new(decl);
        if decl_path.is_absolute()
            || decl_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(anyhow!(
                "declared source \"{}\" in Mind.toml [targets.*].sources must be a \
                 project-root-relative path without \"..\" components",
                decl
            ));
        }
        let path = project_root.join(decl);
        if !path.is_file() {
            return Err(anyhow!(
                "declared source not found: {} (listed as \"{}\" in Mind.toml [targets.*].sources)",
                path.display(),
                decl
            ));
        }
        if sources.contains(&path) {
            return Err(anyhow!(
                "declared source \"{}\" appears more than once in Mind.toml [targets.*].sources",
                decl
            ));
        }
        sources.push(path);
    }
    let entry_path = project_root.join(entry);
    if !entry_path.is_file() {
        return Err(anyhow!("Entry file not found: {}", entry_path.display()));
    }
    let entry_canonical = entry_path
        .canonicalize()
        .unwrap_or_else(|_| entry_path.clone());
    let entry_listed = sources
        .iter()
        .any(|s| s.canonicalize().unwrap_or_else(|_| s.clone()) == entry_canonical);
    if !entry_listed {
        sources.push(entry_path);
    }
    Ok(sources)
}

/// Resolve the source set for ONE build: the ordered list of `.mind` files that
/// are translation units of it, plus whether that list came from an explicit
/// `[targets.*].sources` declaration (which changes cross-module keying to
/// project-root-relative paths — see `compile_sources`).
///
/// This is the ONLY selector. `build_project` compiles what it returns and
/// `run_build` fingerprints what it returns into the incremental cache key, so
/// the set a build is keyed on and the set it is compiled from cannot drift
/// apart — the drift that let an edit to a sibling module hit a cache entry
/// keyed on the entry alone and hand back a stale binary at exit 0.
///
/// - `single_file`: an explicit `mindc build <file>` with no governing project
///   manifest — the "project" is exactly the named entry, never its directory.
/// - `declared`: the target block's `sources = [...]` list, when present.
/// - otherwise: the entry-parent directory walk, sorted (see
///   [`collect_sources`]).
pub fn resolve_sources(
    project_root: &Path,
    entry: &str,
    declared: Option<&[String]>,
    single_file: bool,
) -> Result<(Vec<PathBuf>, bool)> {
    if single_file {
        let entry_path = project_root.join(entry);
        if !entry_path.exists() {
            return Err(anyhow!("Entry file not found: {}", entry_path.display()));
        }
        return Ok((vec![entry_path], false));
    }
    match declared {
        Some(list) => Ok((resolve_declared_sources(project_root, list, entry)?, true)),
        None => Ok((collect_sources(project_root, entry)?, false)),
    }
}
