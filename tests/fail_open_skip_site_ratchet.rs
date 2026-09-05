// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! The fail-OPEN skip-and-return prohibition. The ratchet's floor is now ZERO.
//!
//! # What this gate forbids
//!
//! A wide class of integration-test sites carried the shape
//!
//! ```text
//! if !something_available() { println!("... skipping"); return; }
//! ```
//!
//! without ever consulting `MIND_BENCH_REQUIRE`, so `MIND_BENCH_REQUIRE=1`
//! could not turn them into a hard failure and a tier could pass vacuously.
//! Worse, cargo captures the stdout of a PASSING test, so the announcement was
//! not merely tolerated, it was *unobservable*: the tier gate could not tell
//! "asserted" from "executed".
//!
//! The fail-CLOSED helper every such site must route through is `common::gate`,
//! whose contract is proven in `tests/fail_closed_capability_skip.rs`.
//!
//! # Why there is no backlog table any more
//!
//! The first version of this gate pinned the CARDINALITY (`n == 256`). That
//! does not forbid what it says it forbids: routing one site while adding an
//! unrouted one elsewhere leaves the total unchanged and the gate stays green.
//! The second version froze a per-FILE table, which closed that hole but
//! introduced another: a hand-copied list of 136 file names sitting beside a
//! scanner that derives its scope from disk. Two scopes that must agree, with
//! nothing asserting they do, is the drift this repo has paid for before.
//!
//! Both are gone. The backlog was drained — every site routes through
//! `common::gate` — so the gate is now a flat prohibition with no list to keep
//! in step: ANY skip-and-return in `tests/**/*.rs` that does not consult the
//! shared helper is red, in any file, new or old.
//!
//! # Scope is read out of the thing being checked
//!
//! The scanned set is `tests/**/*.rs` walked from disk (minus this scanner's
//! own source, whose positive controls must quote the shape it forbids), and
//! the routed-detection is the same `ROUTED_MARKERS` list applied to every one
//! of them. Scope and detection cannot drift apart because neither is written
//! down twice.

use std::path::{Path, PathBuf};

/// Text that proves a skip decision consulted the shared fail-closed helper (or
/// the environment variable it reads). One list, used for every file — the scan
/// scope and the routed-detection live in the same place by construction.
const ROUTED_MARKERS: &[&str] = &[
    "gate::compiled",
    "gate::skipped",
    "gate::classify",
    "enforce_real_backend",
    "MIND_BENCH_REQUIRE",
];

/// The last line index of the print macro starting at `lines[i]`, or `None` if
/// `lines[i]` does not open one outside a comment.
///
/// A single-line match is NOT enough: the announcement this scanner exists to
/// find is routinely wrapped across lines, e.g.
///
/// ```text
/// println!(
///     "target: rebuilt .so is not an ELF - \
///      toolchain unavailable, skipping test"
/// );
/// ```
///
/// where the word "skipping" sits three lines below the macro name. Measured
/// while building this scanner: two such sites read as clean under a
/// line-anchored match. A scanner blind to the wrapped form is the same
/// false-green it is meant to catch, so the span is followed to its `);`.
fn print_macro_span(lines: &[&str], i: usize) -> Option<usize> {
    let t = lines[i].trim_start();
    if t.starts_with("//") {
        return None; // prose quoting the bad shape is not the bad shape
    }
    if !t.contains("println!(") && !t.contains("eprintln!(") {
        return None;
    }
    let last = (i + MAX_MACRO_SPAN).min(lines.len() - 1);
    for (n, line) in lines.iter().enumerate().take(last + 1).skip(i) {
        if line.contains(");") {
            return Some(n);
        }
    }
    Some(last)
}

/// A wrapped print macro is followed at most this many lines to its `);`.
const MAX_MACRO_SPAN: usize = 5;

/// Does the print macro spanning `lines[i..=end]` announce a skip?
///
/// "defer" counts too. A deferral is a legitimate, honestly-named outcome (the
/// VNNI rung is deferred by design), but it is still a gate that did not run —
/// if the word were outside this vocabulary, renaming a skip to a deferral
/// would be a one-word escape from this prohibition.
fn is_skip_announcement(lines: &[&str], i: usize, end: usize) -> bool {
    let text = lines[i..=end].join("\n").to_ascii_lowercase();
    text.contains("skip") || text.contains("defer")
}

/// This scanner's OWN source. It is excluded from the scan because its positive
/// control must quote the very shape it forbids; a scanner that flagged its own
/// specimen jar would be unable to prove it can still see the bad shape.
const SCANNER_SELF: &str = "fail_open_skip_site_ratchet.rs";

/// Every `tests/**/*.rs` source except this scanner, with its path RELATIVE to
/// `tests/` — the identity a failure is reported by, so two same-named files in
/// different directories can never be confused for one entry.
fn test_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).expect("read tests dir") {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter(|p| p.file_name().and_then(|s| s.to_str()) != Some(SCANNER_SELF))
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read test source");
            let rel = p
                .strip_prefix(&root)
                .expect("test source under tests/")
                .to_string_lossy()
                .replace('\\', "/");
            (rel, text)
        })
        .collect()
}

/// Sites that announce a skip, return, and never consult the fail-closed helper,
/// as `<path relative to tests/>:<line>`.
fn open_skip_sites() -> Vec<String> {
    let mut open = Vec::new();
    for (rel, text) in test_sources() {
        let lines: Vec<&str> = text.lines().collect();
        for i in 0..lines.len() {
            let Some(end) = print_macro_span(&lines, i) else {
                continue;
            };
            if !is_skip_announcement(&lines, i, end) {
                continue;
            }
            let tail_end = (end + 4).min(lines.len());
            if !lines[i..tail_end].iter().any(|l| l.contains("return")) {
                continue;
            }
            let head = i.saturating_sub(8);
            let window = lines[head..tail_end].join("\n");
            if ROUTED_MARKERS.iter().any(|m| window.contains(m)) {
                continue;
            }
            open.push(format!("{}:{}", rel, i + 1));
        }
    }
    open
}

#[test]
fn the_scan_scope_is_not_empty() {
    // A prohibition over an empty set is the `ran=0` defect it exists to
    // forbid. This asserts the scanner actually read the tree before the three
    // negative assertions below are allowed to mean anything.
    let sources = test_sources();
    assert!(
        sources.len() > 250,
        "the scan walked only {} tests/**/*.rs sources; the tree holds far more, \
         so the prohibition below would be vacuous",
        sources.len()
    );
    assert!(
        sources.iter().any(|(p, _)| p == "common/gate.rs"),
        "the scan must reach tests/common/, the module every routed site names"
    );
}

#[test]
fn no_test_source_holds_a_fail_open_skip_site() {
    // THE DEFECT THIS PINS: `println!("... skipping"); return;` exits 0, so a
    // missing prerequisite — or a FAILING compile — is reported as a passing
    // test. There is no backlog and no tolerated file: every skip decision goes
    // through common::gate, which panics under MIND_BENCH_REQUIRE=1 and
    // otherwise emits `SDLC-GATE <target> ran=0 fail=0`.
    let open = open_skip_sites();
    assert!(
        open.is_empty(),
        "{} fail-open skip-and-return site(s) do not consult the shared \
         fail-closed helper. A skip that never reads MIND_BENCH_REQUIRE cannot \
         be turned into a hard failure, so the tier can pass vacuously. Route \
         each through common::gate::{{compiled, skipped, skipped_optional}}.\n  {}",
        open.len(),
        open.join("\n  ")
    );
}

/// The exact fail-OPEN string a prior finding named, ASSEMBLED at run time.
///
/// Spelling it as one literal would make this scanner a hit on its own scan and
/// on the shell gate that greps the same text, so the specimen is built from
/// halves. It is still one definition, used by both the assertion and the
/// scanner's positive control.
fn banned_literal() -> String {
    format!("{}{}", "compile failed", "; skipping")
}

/// The two-substring capability test, assembled at run time for the same reason.
///
/// The banned predicate ANDed two floating `stderr.contains(...)` probes for the
/// feature name and the word "requires". It is superstring-satisfiable: ANY
/// compiler failure whose stderr happens to carry both tokens anywhere graded as
/// a capability gap. The classifier that replaced it matches the stable
/// diagnostic CODE, owned by `libmind::diagnostics::capability`. The needle is
/// assembled here rather than spelled out so a repo-wide grep for the bad shape
/// returns call sites only.
fn banned_substring_test() -> String {
    format!(
        "contains(\"{}\") && stderr.contains(\"{}\")",
        "mlir-build", "requires"
    )
}

#[test]
fn the_converted_compile_sites_are_gone() {
    // The exact fail-OPEN string the finding named. Zero is the whole point.
    let hits: Vec<String> = test_sources()
        .into_iter()
        .filter(|(_, t)| t.contains(&banned_literal()))
        .map(|(p, _)| p)
        .collect();
    assert!(hits.is_empty(), "fail-open compile skips remain: {hits:?}");
}

#[test]
fn the_two_substring_capability_test_is_gone() {
    // A re-worded diagnostic must not silently widen or close the skip hole, and
    // a program that merely NAMES the tokens must not be able to buy a pass.
    // The prose is not the contract; the code is.
    let needle = banned_substring_test();
    let hits: Vec<String> = test_sources()
        .into_iter()
        .filter(|(_, t)| t.contains(&needle))
        .map(|(p, _)| p)
        .collect();
    assert!(
        hits.is_empty(),
        "these files re-implement the superstring-satisfiable capability test \
         instead of calling common::gate (which delegates to \
         libmind::diagnostics::capability): {hits:?}"
    );
}

#[test]
fn every_optional_input_skip_names_what_is_optional() {
    // `skipped_optional` is the ONE outcome MIND_BENCH_REQUIRE does not close,
    // so it is the only place a vacuous pass could re-enter. It stays honest by
    // being greppable and self-documenting: each call site must carry a comment
    // in the eight lines above it naming what is optional and who supplies it.
    let mut undocumented = Vec::new();
    for (rel, text) in test_sources() {
        if rel == "common/gate.rs" {
            continue; // the definition, not a call site
        }
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("gate::skipped_optional") || line.trim_start().starts_with("//") {
                continue;
            }
            let head = i.saturating_sub(8);
            let documented = lines[head..i].iter().any(|l| {
                l.trim_start().starts_with("//") && l.to_ascii_lowercase().contains("opt")
            });
            if !documented {
                undocumented.push(format!("{}:{}", rel, i + 1));
            }
        }
    }
    assert!(
        undocumented.is_empty(),
        "these opt-in skips carry no comment naming what is optional and who \
         supplies it; an undocumented one is indistinguishable from a fail-open \
         escape hatch.\n  {}",
        undocumented.join("\n  ")
    );
}

#[test]
fn the_scanner_itself_can_see_the_bad_shape() {
    // Positive control: a scanner that matches nothing would pass silently.
    fn one(l: &str) -> bool {
        let lines = vec![l];
        print_macro_span(&lines, 0).is_some_and(|e| is_skip_announcement(&lines, 0, e))
    }
    let specimen = format!("    println!(\"arena: mindc {}\");", banned_literal());
    assert!(one(&specimen));
    assert!(one(
        r#"        eprintln!("SKIP: MLIR tools not available");"#
    ));
    // Renaming a skip to a deferral must not escape the prohibition.
    assert!(one(r#"        println!("DEFER vnni: rung not run here");"#));
    assert!(!one(
        r#"    //! println!("... build failed -> skipped"); return;"#
    ));
    assert!(!one(r#"    println!("all good");"#));

    // The wrapped form: the word sits three lines below the macro name.
    let wrapped = vec![
        r#"        println!("#,
        r#"            "target: rebuilt .so is not an ELF - \"#,
        r#"             toolchain unavailable, skipping test""#,
        r#"        );"#,
    ];
    let end = print_macro_span(&wrapped, 0).expect("span must be found");
    assert_eq!(end, 3);
    assert!(is_skip_announcement(&wrapped, 0, end));
}
