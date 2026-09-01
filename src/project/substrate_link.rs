// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0

//! Native cross-module substrate linking for shared-library emission.
//!
//! A `.mind` source that `import`s a self-contained `std` substrate module
//! (`std.arena`, `std.io_canon`, `std.iouring`, `std.reactor`, `std.ring`,
//! `std.sha256`, `std.time`) calls that module's functions by their bare
//! symbol name. Emitting the consumer alone produces a shared object whose
//! imported symbols are UNDEFINED, so `dlopen` fails at load time:
//!
//! ```text
//! libio_canon.so: undefined symbol: sha256
//! ```
//!
//! The fix is to compile each imported substrate module to its own object and
//! link it into the `.so`. This module owns that walk so BOTH shared-library
//! emitters share one implementation:
//!
//!  * `mindc build --emit=cdylib` (manifest path, [`crate::project`]), and
//!  * `mindc <file> --emit-shared <out.so>` (flat single-file path, `mindc.rs`).
//!
//! The flat path previously had no walk at all, which is why compiling
//! `std/io_canon.mind` — the canonical-I/O module the evidence anchor hashes
//! through — produced an un-`dlopen`-able artifact. The defect was in the
//! LINKER wiring, never in `std/sha256.mind`, which is a complete pure-MIND
//! FIPS-180-4 implementation.
//!
//! Determinism: the closure is a [`BTreeSet`] of `&'static str` module names,
//! so object order in the link command is the module-name sort order, never a
//! hash order. An EMPTY closure yields an empty object vector, which keeps the
//! link byte-identical to the historical single-entry path (the Phase G
//! keystone imports no substrate module).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::eval::mlir_build::{self, BuildTools};
use crate::runtime::types::BackendTarget;

/// Substrate modules whose `.o` is compiled and linked when imported.
///
/// Membership rule: the module must be SELF-CONTAINED at the object level —
/// it may import other members of this list (the closure walk below follows
/// those edges) but nothing outside it, so its object never drags in an
/// unresolved third-party dependency.
pub const SUBSTRATE_MODULES: &[&str] = &[
    "std.arena",
    "std.io_canon",
    "std.iouring",
    "std.reactor",
    "std.ring",
    "std.sha256",
    "std.time",
];

/// Substrate modules directly imported by `src`.
///
/// Keyed on the RESOLVED import path (`path.join(".")`), never on a bare
/// leading segment: only an import naming a member of [`SUBSTRATE_MODULES`]
/// admits that module's object. An unparseable source contributes nothing.
pub fn scan_substrate_imports(src: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    let Ok(ast) = crate::parser::parse(src) else {
        return found;
    };
    for item in &ast.items {
        if let crate::ast::Node::Import { path, .. } = item {
            let key = path.join(".");
            for &name in SUBSTRATE_MODULES {
                if key == name {
                    found.push(name);
                }
            }
        }
    }
    found
}

/// Transitive closure of the substrate imports reachable from `seed_texts`.
///
/// BFS over the import graph: a substrate module imported by a seed pulls in
/// the substrate modules IT imports (e.g. `std.io_canon` imports
/// `std.sha256`, so a consumer that names only `io_canon` still gets
/// `sha256.o`). Without the transitive step the substrate object itself links
/// with an undefined symbol — the same defect one level down.
///
/// The seeds are source TEXTS, so a caller may seed from one entry file (the
/// flat `--emit-shared` path) or from every project source (the manifest
/// cdylib path) without this function knowing the difference.
pub fn substrate_closure<'a, I>(seed_texts: I) -> BTreeSet<&'static str>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut imported: BTreeSet<&'static str> = BTreeSet::new();
    let mut worklist: Vec<&'static str> = Vec::new();
    for text in seed_texts {
        worklist.extend(scan_substrate_imports(text));
    }
    while let Some(modname) = worklist.pop() {
        if imported.insert(modname) {
            if let Some((_, src)) = crate::project::stdlib::STDLIB_MIND_SOURCES
                .iter()
                .find(|(n, _)| *n == modname)
            {
                for dep in scan_substrate_imports(src) {
                    if !imported.contains(dep) {
                        worklist.push(dep);
                    }
                }
            }
        }
    }
    imported
}

/// Compile each module in `closure` to `<obj_dir>/__std_<short>.o` and return
/// the object paths, in module-name sort order.
///
/// Every object has its synthetic `@main` localized (`objcopy
/// --localize-symbol=main`): the MLIR emit gives each module object a `main`,
/// which would otherwise collide with the consumer entry's `main` at link
/// time. The module's public symbols (`canon_*`, `ring_*`, `sha256`, …) stay
/// global so the consumer resolves them.
///
/// Fail-loud: a substrate module that fails to compile, lower or `objcopy`
/// aborts the build with the real diagnostics rather than emitting a `.so`
/// with the symbol still undefined.
pub fn compile_substrate_objects(
    closure: &BTreeSet<&'static str>,
    obj_dir: &Path,
    target: BackendTarget,
    tools: &BuildTools,
) -> Result<Vec<PathBuf>> {
    use crate::pipeline::{CompileOptions, compile_source_with_name, lower_to_mlir};

    let sub_opts = CompileOptions {
        func: None,
        enable_autodiff: false,
        target,
        manifest_exports: Vec::new(),
        ..Default::default()
    };

    let mut objs: Vec<PathBuf> = Vec::new();
    for modname in closure {
        let modname = *modname;
        let Some((_, src)) = crate::project::stdlib::STDLIB_MIND_SOURCES
            .iter()
            .find(|(n, _)| *n == modname)
        else {
            continue;
        };
        let prod = compile_source_with_name(src, Some(modname), &sub_opts).map_err(|e| {
            // Render the real diagnostics (file:line:col + message) instead of
            // the opaque CompileError Display, matching the cdylib build path.
            let diags = e.into_diagnostics(Some(modname));
            let rendered = diags
                .iter()
                .map(|d| crate::diagnostics::render(src, d))
                .collect::<Vec<_>>()
                .join("\n");
            if rendered.trim().is_empty() {
                anyhow!("substrate compile failed for {modname}")
            } else {
                anyhow!("substrate compile failed for {modname}:\n{rendered}")
            }
        })?;
        #[cfg(feature = "autodiff")]
        let sub_mlir = lower_to_mlir(&prod.ir, prod.grad.as_ref())
            .map_err(|e| anyhow!("substrate MLIR lowering for {modname}: {e}"))?;
        #[cfg(not(feature = "autodiff"))]
        let sub_mlir = lower_to_mlir(&prod.ir)
            .map_err(|e| anyhow!("substrate MLIR lowering for {modname}: {e}"))?;
        let short = modname.rsplit('.').next().unwrap_or(modname);
        let obj_path = obj_dir.join(format!("__std_{short}.o"));
        let sub_bo = mlir_build::BuildOptions {
            preset: mlir_build::preset_for_mlir(&sub_mlir.primal_mlir),
            emit_mlir_file: None,
            emit_llvm_file: None,
            emit_obj_file: Some(&obj_path),
            emit_shared: None,
            opt_pipeline: None,
            target_triple: None,
        };
        mlir_build::build_all(&sub_mlir.primal_mlir, tools, &sub_bo)
            .map_err(|e| anyhow!("substrate object build for {modname}: {e}"))?;
        let st = std::process::Command::new("objcopy")
            .arg("--localize-symbol=main")
            .arg(&obj_path)
            .status()
            .map_err(|e| anyhow!("objcopy spawn failed for {modname}: {e}"))?;
        if !st.success() {
            return Err(anyhow!(
                "objcopy --localize-symbol=main failed for {modname}"
            ));
        }
        objs.push(obj_path);
    }
    Ok(objs)
}

/// The substrate objects a SINGLE-FILE shared-library emit must link.
///
/// Seeds the closure from the entry text alone, then removes any module the
/// entry IS: compiling `std/io_canon.mind` itself must link `sha256.o` but
/// must NOT also link an `io_canon.o` that redefines every `canon_*` symbol
/// the entry already emits. Identity is decided by SOURCE TEXT equality
/// against the bundled blob — an exact test, not a filename heuristic — so a
/// user module that merely happens to be named `io_canon.mind` still links
/// the real `std.io_canon` object.
pub fn substrate_objects_for_entry(
    entry_src: &str,
    obj_dir: &Path,
    target: BackendTarget,
    tools: &BuildTools,
) -> Result<Vec<PathBuf>> {
    let mut closure = substrate_closure([entry_src]);
    closure.retain(|modname| {
        !crate::project::stdlib::STDLIB_MIND_SOURCES
            .iter()
            .any(|(n, s)| n == modname && *s == entry_src)
    });
    compile_substrate_objects(&closure, obj_dir, target, tools)
}
