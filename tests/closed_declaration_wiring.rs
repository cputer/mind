// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! A workflow declaration that a hole is CLOSED must rest on a gate that
//! EXISTS and still RUNS.
//!
//! # The defect this pins
//!
//! `.github/workflows/ci.yml` carries a `# CLOSED:` note asserting that the
//! capability-skip hole is enforced by `tests/fail_open_skip_site_ratchet.rs`
//! over `tests/common/gate.rs`. That assertion was raised by TYPING: nothing
//! stopped either file being deleted or renamed, and nothing stopped the gate
//! being dropped from the per-harness floors in
//! `scripts/exec_semantics_gate.sh`, while the note went on reading as
//! enforcement. A declaration whose subject can vanish under it is prose.
//!
//! Measured before this gate existed: the note ALSO claimed more than the
//! scanner could see ("every skip-and-return site"), and four tests in
//! `mindc_cache_phase_f` graded `ok` on a build that had just printed
//! `error[build][E5003]`. Correcting the sentence without binding it to the
//! code would have re-typed the same class of claim.
//!
//! # Scope is read out of the thing being checked
//!
//! The tiers are DERIVED by scanning `scripts/exec_semantics_gate.sh` for every
//! `CRITICAL_<tier>=(` array it declares, never hand-copied here. A second
//! spelling of "there are three tiers" is the drift this repo keeps paying for:
//! a lint whose scan scope and its own reference detection are two hand-typed
//! lists that quietly stop agreeing.

use std::path::PathBuf;

/// The files the `# CLOSED` declaration names as its enforcement.
const DECLARED_ENFORCEMENT: &[&str] = &[
    "tests/fail_open_skip_site_ratchet.rs",
    // The shape vocabulary and the block scanner that decide what the ratchet
    // can SEE. The declaration's "to the width of its detector" clause is a
    // claim about this file; a note that stopped naming it could keep reading
    // as enforced while the detector it describes was deleted.
    "tests/skip_shape_scan/mod.rs",
    "tests/common/gate.rs",
    // This gate itself: a note that stopped naming what holds it could be
    // deleted with nothing left pointing at the check.
    "tests/closed_declaration_wiring.rs",
];

/// The gate target whose per-harness floor makes the declaration true.
const DECLARED_GATE: &str = "fail_open_skip_site_ratchet";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The `# CLOSED` declaration block of `ci.yml`, from its marker to the end of
/// the leading comment header.
fn closed_declaration(ci: &str) -> &str {
    let start = ci
        .find("# CLOSED")
        .expect("ci.yml must carry the CLOSED declaration this gate backs");
    let rest = &ci[start..];
    // The header ends at the first non-comment, non-blank line.
    let end = rest
        .lines()
        .scan(0usize, |off, l| {
            let here = *off;
            *off += l.len() + 1;
            Some((here, l))
        })
        .find(|(_, l)| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .map(|(off, _)| off)
        .unwrap_or(rest.len());
    &rest[..end]
}

/// Every `CRITICAL_<tier>=(...)` array in `sh`, as `(header line, rows)`.
///
/// A pure function over text so the scan itself has a positive control: a
/// detector that can only be exercised by editing the real script is a detector
/// nobody ever proves can fail.
fn critical_arrays(sh: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut lines = sh.lines();
    while let Some(line) = lines.next() {
        if !(line.starts_with("CRITICAL_") && line.ends_with("=(")) {
            continue;
        }
        let rows: Vec<String> = lines
            .by_ref()
            .take_while(|l| !l.starts_with(')'))
            .map(|l| l.to_string())
            .collect();
        out.push((line.to_string(), rows));
    }
    out
}

#[test]
fn the_closed_declaration_names_files_that_exist() {
    let ci = read(".github/workflows/ci.yml");
    let note = closed_declaration(&ci);
    let root = repo_root();
    for f in DECLARED_ENFORCEMENT {
        assert!(
            note.contains(f),
            "the CLOSED declaration no longer names {f}; a declaration that has \
             stopped pointing at its enforcement is prose"
        );
        assert!(
            root.join(f).exists(),
            "the CLOSED declaration names {f}, which does not exist"
        );
    }
}

#[test]
fn every_critical_tier_names_the_gate_the_declaration_rests_on() {
    let sh = read("scripts/exec_semantics_gate.sh");
    let arrays = critical_arrays(&sh);
    assert!(
        arrays.len() >= 3,
        "found only {} CRITICAL_<tier> arrays in scripts/exec_semantics_gate.sh; \
         the scan that derives the tiers checked nothing",
        arrays.len()
    );
    for (header, rows) in arrays {
        assert!(
            rows.iter().any(|r| r.contains(DECLARED_GATE)),
            "{header} has no `{DECLARED_GATE}` row, so that tier can run without \
             the gate the ci.yml CLOSED declaration rests on while the \
             declaration still reads as enforced"
        );
    }
}

#[test]
fn the_array_scan_can_see_a_tier_that_dropped_the_row() {
    // Positive control. A scan that matched nothing would pass silently — the
    // ran=0 defect, in its active form.
    let sh = "CRITICAL_exec=(\n  \"a 1\"\n  \"fail_open_skip_site_ratchet 8\"\n)\n\
              CRITICAL_pkg=(\n  \"b 1\"\n)\n";
    let arrays = critical_arrays(sh);
    assert_eq!(arrays.len(), 2, "both arrays must be found");
    assert_eq!(arrays[0].0, "CRITICAL_exec=(");
    assert!(arrays[0].1.iter().any(|r| r.contains(DECLARED_GATE)));
    assert!(
        !arrays[1].1.iter().any(|r| r.contains(DECLARED_GATE)),
        "a tier that dropped the row must read as missing it"
    );
}

#[test]
fn the_declaration_block_stops_at_the_end_of_the_header() {
    // The block must not swallow the whole file: a scan that returns everything
    // would report any path mentioned anywhere as "declared".
    let ci = "# CLOSED: a note\n# more note\n\non:\n  push:\n    branches: [ main ]\n";
    let note = closed_declaration(ci);
    assert!(note.contains("more note"));
    assert!(
        !note.contains("branches"),
        "the block must end at the header"
    );
}
