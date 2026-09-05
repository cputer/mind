// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0

//! Incremental-cache KEY construction (issue #96 + the sibling-module staleness
//! class).
//!
//! A cache key is a claim: "a fresh compile of these inputs would reproduce the
//! stored artifact byte for byte". Every input that can change the artifact
//! must therefore be inside the key, or the cache serves a stale binary at exit
//! 0 with no diagnostic — the worst failure shape this compiler has, because
//! nothing distinguishes it from a correct build.
//!
//! The inputs are:
//!  - the ENTRY source bytes (the key's trailer);
//!  - EVERY other source that participates in the compilation, fingerprinted
//!    with its project-relative path AND its position in the resolved source
//!    order (see [`source_set_dep_entries`]) — order is artifact-visible, so a
//!    reorder must invalidate;
//!  - target / optimize / edition / emit kind;
//!  - the `mindc` binary's own identity and the external toolchain's
//!    (clang / mlir-opt / mlir-translate).
//!
//! Every path here FAILS CLOSED: an input that cannot be fingerprinted yields
//! `None`, which callers must treat as "do not read and do not write the
//! cache" — never as a stable sentinel key, which would reintroduce exactly
//! the staleness the fingerprint exists to prevent.

use std::path::{Path, PathBuf};

use crate::project::{BuildTarget, EmitKind, OptimizeLevel};

use super::cache::{self, module_cache_key};

/// Emit-kind discriminator entries folded into the module cache key. A
/// `cdylib` shared object, a `binary` PIE, and a relocatable `object` are
/// distinct artifacts from identical source/target/optimize inputs and must
/// never share a slot. `cdylib` contributes NO entry (historical value);
/// `binary` / `object` get a labelled `emit=` entry.
fn emit_discriminator(emit: EmitKind) -> Vec<String> {
    match emit {
        EmitKind::Cdylib => Vec::new(),
        other => vec![format!("emit={}", other.as_str())],
    }
}

/// Toolchain-identity dep entries (clang / mlir-opt / mlir-translate). Each is
/// `toolchain=<name>|<resolved-path>|<size>|<mtime-ns>|<--version banner>` so a
/// toolchain swap — even one reporting an identical `--version` — invalidates
/// the cache (scan-finding S4, sibling of the runtime-obj cache in
/// `mlir_build::clang_identity_string`). Computed once per process. When the
/// `mlir-build` feature is off (no real backend) this is empty.
pub fn toolchain_dep_entries() -> Vec<String> {
    #[cfg(feature = "mlir-build")]
    {
        use std::sync::OnceLock;
        static ENTRIES: OnceLock<Vec<String>> = OnceLock::new();
        ENTRIES
            .get_or_init(|| match crate::eval::mlir_build::resolve_tools() {
                Ok(tools) => vec![
                    format!(
                        "toolchain=clang|{}",
                        crate::eval::mlir_build::tool_identity_string(&tools.clang)
                    ),
                    format!(
                        "toolchain=mlir-opt|{}",
                        crate::eval::mlir_build::tool_identity_string(&tools.mlir_opt)
                    ),
                    format!(
                        "toolchain=mlir-translate|{}",
                        crate::eval::mlir_build::tool_identity_string(&tools.mlir_translate)
                    ),
                ],
                // Tools unresolvable => the full compile would fail anyway and
                // no cache is written, so an empty toolchain set is safe here.
                Err(_) => Vec::new(),
            })
            .clone()
    }
    #[cfg(not(feature = "mlir-build"))]
    {
        Vec::new()
    }
}

/// Full dep-hash entry set for a module cache key: emit-kind discriminator plus
/// toolchain identity. `module_cache_key` sorts these, so order is irrelevant.
pub fn cache_dep_entries(emit: EmitKind) -> Vec<String> {
    let mut deps = emit_discriminator(emit);
    deps.extend(toolchain_dep_entries());
    deps
}

/// Compute the full module cache key for a compile of `source_bytes` (the ENTRY
/// module) with `sources` — the complete, ordered set of files participating in
/// the compilation as resolved by `project::sources::resolve_sources` — produced
/// by the `mindc` binary at `mindc_exe`. Returns `None` (fail-closed) when that
/// binary's identity cannot be probed or a participating source cannot be read.
///
/// This is the single source of truth for the key shared by the build path
/// (which passes `std::env::current_exe()`) and the integration tests (which
/// pass `CARGO_BIN_EXE_mindc`): both MUST derive byte-identical keys or a
/// subprocess-populated cache would spuriously miss on an in-process probe
/// (issue #96 lockstep hazard). A `None` return MUST be treated as a cache MISS
/// — the cache is neither read nor written under any sentinel key.
pub fn compile_cache_key(
    source_bytes: &[u8],
    flags: CacheKeyFlags,
    mindc_exe: &Path,
    project_root: &Path,
    sources: &[PathBuf],
) -> Option<String> {
    let identity = cache::compiler_identity_string(mindc_exe)?;
    let compiler_version = format!("{}+{}", env!("CARGO_PKG_VERSION"), identity);
    let mut deps = cache_dep_entries(flags.emit);
    deps.extend(source_set_dep_entries(project_root, sources)?);
    Some(module_cache_key(
        source_bytes,
        flags.target,
        flags.optimize,
        &deps,
        &compiler_version,
        flags.edition,
    ))
}

/// The build knobs that select an artifact identity: two compiles that differ in
/// ANY of them produce different bytes and must never share a cache slot.
///
/// Grouped into one value rather than four positional parameters so a caller
/// cannot silently transpose two of them, and deliberately WITHOUT a `Default`:
/// every field is load-bearing, so each call site must state it (a defaulted
/// `emit` would let a `cdylib` and a `binary` collide on one key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheKeyFlags {
    /// Backend class the artifact is built for.
    pub target: BuildTarget,
    /// Optimization level — debug and release objects differ in codegen.
    pub optimize: OptimizeLevel,
    /// Artifact kind (`cdylib` / `binary` / `object`).
    pub emit: EmitKind,
    /// Language edition — different editions have different semantics.
    pub edition: u32,
}

/// Fingerprint EVERY source file participating in the compilation — the entry
/// and each sibling module — as one dep entry per file:
/// `src<NNNN>=<project-relative path>|<sha256 of contents>`.
///
/// Why each part is load-bearing:
///  - the CONTENT hash is the fix for the defect: the key used to cover the
///    entry source alone, so editing a sibling module (whose functions,
///    `pub const`s and enum tags the entry compiles against) left the key
///    unchanged, the probe HIT, and `mindc build` copied out a binary built
///    from the OLD sibling — exit 0, no diagnostic, wrong program;
///  - the ORDINAL `<NNNN>` makes source ORDER part of the key. `module_cache_key`
///    sorts dep entries, so without a position prefix a reordered source list
///    (a reordered `[targets.*].sources`) would hash identically while emitting
///    different bytes;
///  - the path is PROJECT-RELATIVE with `/` separators so the key is the same
///    on two machines that checked the project out at different absolute paths
///    (an absolute path would defeat any shared or relocated cache).
///
/// Returns `None` — fail closed, caller must neither read nor write the cache —
/// when any participating source cannot be read. A key that silently omitted an
/// unreadable file would claim reproducibility it cannot have.
pub fn source_set_dep_entries(project_root: &Path, sources: &[PathBuf]) -> Option<Vec<String>> {
    let mut entries = Vec::with_capacity(sources.len());
    for (idx, src) in sources.iter().enumerate() {
        let bytes = std::fs::read(src).ok()?;
        let rel = src
            .strip_prefix(project_root)
            .unwrap_or(src)
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(format!(
            "src{:04}={}|{}",
            idx,
            rel,
            cache::sha256_hex(&bytes)
        ));
    }
    Some(entries)
}
