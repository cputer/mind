// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Structural gate: a module that is already over the size ceiling may not grow.
//!
//! # Why this file exists
//!
//! The house rule — "200-400 lines typical, 800 max; do not grow an over-limit
//! file, add a new focused module" — was carried only as PROSE. Prose does not
//! fail a build, and three separate landings in one wave added logic to
//! `src/build/mod.rs`, `src/bin/mindc.rs` and `src/project/mod.rs`, each of
//! which was already well past the ceiling. Nothing noticed, because nothing
//! could.
//!
//! So the ceiling is a test. Two directions, both mechanical:
//!
//! * **No module exceeds its budget.** A file not in the table below must be at
//!   or under [`CEILING`]. A file in the table is a pre-existing over-limit
//!   module and must be at or under its PINNED line count — so the only way to
//!   add code to one is to raise its number deliberately, in the diff, where a
//!   reviewer sees it.
//! * **No budget outlives its file.** Every pinned entry must still name a file
//!   that is over the ceiling. An entry whose file was split (or deleted) is a
//!   stale second list, and a stale list is how a gate quietly stops guarding.
//!
//! The budget is a MAXIMUM, not an equality: shrinking an over-limit module is
//! the point of the rule and must never turn the gate red. Re-pinning a budget
//! DOWN after an extraction is what converts a one-time cleanup into a ratchet,
//! and is expected in the same commit as the extraction.
//!
//! This gate deliberately says nothing about whether a file SHOULD be split —
//! only that the over-limit set may not get worse by accident.

use std::path::{Path, PathBuf};

/// The house ceiling. A module at or under this needs no entry below.
const CEILING: usize = 800;

/// Pre-existing over-limit modules, pinned at the line count they may not
/// exceed. Sorted by path; one entry per file.
///
/// Adding a row is admitting a NEW over-limit module and should be rejected in
/// review in favour of a focused module. Raising a row is admitting growth of
/// one that already exists, and needs the same justification the prose rule
/// always asked for — the difference is that now it cannot happen silently.
const LEGACY_BUDGETS: &[(&str, usize)] = &[
    ("src/ast/mod.rs", 1158),
    ("src/bin/mind-ai.rs", 1101),
    ("src/bin/mindc.rs", 3343),
    ("src/build/mod.rs", 1062),
    ("src/check/mod.rs", 945),
    ("src/deps/mod.rs", 1060),
    ("src/doc/mod.rs", 888),
    ("src/eval/abi_gate.rs", 1025),
    ("src/eval/autodiff.rs", 1643),
    ("src/eval/closures.rs", 950),
    ("src/eval/lower.rs", 12689),
    ("src/eval/mlir_build.rs", 1002),
    ("src/eval/mlir_export.rs", 1447),
    ("src/eval/mod.rs", 3917),
    ("src/eval/stdlib/tensor.rs", 1097),
    ("src/fmt/printer.rs", 2225),
    ("src/ir/compact/parse.rs", 967),
    ("src/ir/compact/v2/binary.rs", 960),
    ("src/ir/compact/v2/evidence.rs", 1052),
    ("src/ir/compact/v2/map_tests.rs", 917),
    ("src/ir/compact/v3/emit.rs", 1399),
    ("src/ir/compact/v3/evidence.rs", 4005),
    ("src/ir/compact/v3/mod.rs", 2218),
    ("src/ir/compact/v3/parse.rs", 1354),
    ("src/ir/evidence.rs", 1216),
    ("src/ir/fp_mode.rs", 1217),
    ("src/ir/mod.rs", 1519),
    ("src/ir/verify.rs", 1849),
    ("src/mlir/lowering.rs", 12230),
    ("src/opt/collapse.rs", 1228),
    ("src/opt/native_opt.rs", 1137),
    ("src/opt/scev.rs", 869),
    ("src/parser/expand_bimap.rs", 1836),
    ("src/parser/mod.rs", 6073),
    ("src/project/mod.rs", 3712),
    ("src/test/mod.rs", 840),
    ("src/type_checker/mod.rs", 6623),
    ("src/type_checker/resolve.rs", 1316),
];

/// Repo-relative path of `p`, with `/` separators on every host.
fn rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root)
        .expect("src path under manifest dir")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `src/**/*.rs` as `(repo-relative path, line count)`, sorted.
fn src_module_sizes() -> Vec<(String, usize)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).expect("read src dir") {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    let root = manifest_root();
    let mut paths = Vec::new();
    walk(&root.join("src"), &mut paths);
    let mut sizes: Vec<(String, usize)> = paths
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read src module");
            (rel(&p, &root), text.lines().count())
        })
        .collect();
    sizes.sort();
    sizes
}

/// The budget `path` may not exceed: its pinned entry, else the ceiling.
fn budget_for(path: &str) -> usize {
    LEGACY_BUDGETS
        .iter()
        .find(|(p, _)| *p == path)
        .map_or(CEILING, |(_, n)| *n)
}

/// The over-budget modules of `sizes`, as reviewer-readable lines.
fn over_budget(sizes: &[(String, usize)]) -> Vec<String> {
    sizes
        .iter()
        .filter(|(p, n)| *n > budget_for(p))
        .map(|(p, n)| format!("{p}: {n} lines (budget {})", budget_for(p)))
        .collect()
}

#[test]
fn no_module_exceeds_its_budget() {
    let sizes = src_module_sizes();
    assert!(
        !sizes.is_empty(),
        "the walk found no `src/**/*.rs` at all, so this gate proves nothing"
    );
    let bad = over_budget(&sizes);
    assert!(
        bad.is_empty(),
        "these modules are over the {CEILING}-line ceiling (or over the line count \
         pinned for an already-over-limit module). Carry the new logic into a \
         focused module instead of growing one that is already too large; if the \
         growth is genuinely unavoidable, raise the number in `LEGACY_BUDGETS` in \
         the same commit so the decision is visible.\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn no_budget_outlives_its_file() {
    // The other direction. A pinned row for a file that was split, renamed or
    // deleted is a stale second list: it guards nothing, and it hides the fact
    // that the over-limit set changed. Re-pin it or drop it.
    let sizes = src_module_sizes();
    for (path, budget) in LEGACY_BUDGETS {
        let actual = sizes.iter().find(|(p, _)| p == path).map(|(_, n)| *n);
        let Some(actual) = actual else {
            panic!(
                "`LEGACY_BUDGETS` pins {path} at {budget} lines, but no such module \
                 exists. Drop the row — a budget for a file that is gone is a stale \
                 list, not a gate."
            );
        };
        assert!(
            actual > CEILING,
            "`LEGACY_BUDGETS` pins {path} at {budget}, but it is now {actual} lines \
             — at or under the {CEILING}-line ceiling. Drop the row so the ceiling \
             itself guards it; leaving the row would license growing it back."
        );
    }
}

#[test]
fn the_budget_table_is_sorted_and_has_no_duplicates() {
    let mut seen: Vec<&str> = Vec::new();
    for (path, _) in LEGACY_BUDGETS {
        assert!(
            !seen.contains(path),
            "{path} is pinned twice; the second row would be dead and the first \
             would silently win"
        );
        seen.push(path);
    }
    let mut sorted = seen.clone();
    sorted.sort_unstable();
    assert_eq!(
        seen, sorted,
        "keep `LEGACY_BUDGETS` sorted by path so a row is found by reading, and \
         two commits adding a row do not collide on the same line"
    );
}

#[test]
fn the_ratchet_can_still_see_growth() {
    // Positive control: the check is only evidence if it fails on the shape it
    // forbids. Both defect shapes, against a synthetic set.
    let pinned = LEGACY_BUDGETS[0];
    let grown = vec![(pinned.0.to_string(), pinned.1 + 1)];
    assert_eq!(
        over_budget(&grown).len(),
        1,
        "an over-limit module growing by one line must be seen"
    );
    let fresh = vec![("src/brand/new.rs".to_string(), CEILING + 1)];
    assert_eq!(
        over_budget(&fresh).len(),
        1,
        "a NEW module over the ceiling must be seen even though it is unpinned"
    );
    // ... and does not cry wolf on the compliant shapes.
    let at_budget = vec![
        (pinned.0.to_string(), pinned.1),
        ("src/brand/new.rs".to_string(), CEILING),
    ];
    assert!(over_budget(&at_budget).is_empty());
    let shrunk = vec![(pinned.0.to_string(), pinned.1 - 1)];
    assert!(
        over_budget(&shrunk).is_empty(),
        "shrinking an over-limit module is the point of the rule and must not \
         turn the gate red"
    );
}
