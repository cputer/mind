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
//!
//! # Three ways this prohibition was text-satisfiable, and what closed them
//!
//! Measured against the tip by mutating the tree under the built scanner:
//!
//! 1. `ROUTED_MARKERS` accepted the bare strings `MIND_BENCH_REQUIRE` and
//!    `enforce_real_backend`, so a *comment* reading
//!    `// NOTE: this gate does not honour MIND_BENCH_REQUIRE` two lines above a
//!    bare `println!("skipping"); return;` bought the site a pass. Only
//!    `gate::`-qualified call syntax counts now. The 17 sites that had been
//!    riding the bare-name form (8 in `cross_substrate_identity`, 9 in
//!    `phase_g_keystone_bootstrap`) hand-rolled the predicate as
//!    `var_os("MIND_BENCH_REQUIRE").is_some()`, which made
//!    `MIND_BENCH_REQUIRE=0` ENFORCE; they now call `gate::skipped`.
//! 2. The scan was keyed on a print macro, so a *silent* probe-and-return
//!    (`if !bin.exists() { return; }`) was structurally invisible — a separate
//!    set of 17 sites was live and unseen. [`silent_probe_sites`] is the second
//!    detector.
//! 3. The banned two-substring capability test was spelled with one variable
//!    name (`stderr.`), so the same predicate under any other name was
//!    invisible — eight live sites kept it. The matcher is variable-agnostic.

use std::path::{Path, PathBuf};

/// Text that proves a skip decision consulted the shared fail-closed helper.
///
/// ONLY `gate::`-qualified call syntax. The bare names `MIND_BENCH_REQUIRE` and
/// `enforce_real_backend` used to appear here, which made the prohibition
/// satisfiable by PROSE: a comment that merely mentions the variable is not
/// evidence that any decision consulted it, and a site can name the variable in
/// the very sentence that says it ignores it. A marker a comment can supply
/// grades text, not routing.
const ROUTED_MARKERS: &[&str] = &[
    "gate::compiled",
    "gate::skipped",
    "gate::classify",
    "gate::is_capability_gap",
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

/// Sites that ANNOUNCE a skip, return, and never consult the fail-closed
/// helper, as `<path relative to tests/>:<line>`.
fn announced_skip_sites(rel: &str, lines: &[&str]) -> Vec<String> {
    let mut open = Vec::new();
    for i in 0..lines.len() {
        let Some(end) = print_macro_span(lines, i) else {
            continue;
        };
        if !is_skip_announcement(lines, i, end) {
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
        open.push(format!("{}:{} (announced skip)", rel, i + 1));
    }
    open
}

/// The identifier run starting at byte 0 of `s`, as a byte length.
///
/// Rust identifiers are ASCII here, so a byte count is exact and lets the
/// caller index straight back into the slice.
fn ident_len(s: &str) -> usize {
    s.bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count()
}

/// Does `t` open a capability PROBE that decides whether the gate can run?
///
/// The three measured shapes: a filesystem existence check, a PATH lookup, an
/// environment read. Each is matched by its METHOD call, never by the name of
/// the receiver — a needle spelled with one variable name is evaded by picking
/// another, which is exactly how the superstring capability test stayed hidden
/// at eight sites.
fn is_capability_probe(t: &str) -> bool {
    let t = t.trim_start();
    if t.starts_with("//") {
        return false; // prose quoting the bad shape is not the bad shape
    }
    let ordered = |open: &str, close: &str| t.find(open).is_some_and(|p| t[p..].contains(close));
    // The RECEIVER is deliberately not pinned to a bare identifier.
    // `if !bin.exists()`, `if !Path::new(p).exists()` and
    // `if !std::path::Path::new(p).exists()` are one decision written three
    // ways, and a detector keyed on `if !<ident>.exists()` sees only the first —
    // measured while building this scanner: the inline-path specimen walked
    // straight past that draft, which is the same variable-name blindness that
    // hid eight superstring capability tests.
    ordered("if !", ".exists()")
        || ordered("which(", ".is_err()")
        || ordered("var_os(", ".is_none()")
}

/// A `return` that hands the caller nothing it can tell apart from success.
fn is_bare_return(line: &str) -> bool {
    matches!(line.trim(), "return;" | "return None;" | "return Ok(());")
}

/// How many lines after the probe a bare `return` still counts as its body.
const PROBE_RETURN_SPAN: usize = 2;

/// Sites where a probe skips WITHOUT announcing anything.
///
/// THE BLIND SPOT THIS CLOSES: [`announced_skip_sites`] is keyed on a print
/// macro, so `if !bin.exists() { return; }` — no print, no panic, exit 0 — was
/// structurally invisible to it. Measured at the tip: 17 live sites, every one
/// of them reported clean.
fn silent_probe_sites(rel: &str, lines: &[&str]) -> Vec<String> {
    let mut open = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !is_capability_probe(line) {
            continue;
        }
        let end = (i + PROBE_RETURN_SPAN + 1).min(lines.len());
        let window = &lines[i..end];
        if !window.iter().any(|l| is_bare_return(l)) {
            continue;
        }
        if window.iter().any(|l| l.contains("gate::")) {
            continue;
        }
        open.push(format!("{}:{} (silent probe)", rel, i + 1));
    }
    open
}

/// Every skip-and-return site that does not consult the fail-closed helper,
/// announced or silent, as `<path relative to tests/>:<line> (<shape>)`.
fn open_skip_sites() -> Vec<String> {
    let mut open = Vec::new();
    for (rel, text) in test_sources() {
        let lines: Vec<&str> = text.lines().collect();
        open.extend(announced_skip_sites(&rel, &lines));
        open.extend(silent_probe_sites(&rel, &lines));
    }
    open
}

#[test]
fn the_scan_scope_is_not_empty() {
    // A prohibition over an empty set is the `ran=0` defect it exists to
    // forbid. This asserts the scanner actually read the tree before the
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

// ---------------------------------------------------------------------------
// ONE OWNER for the fail-closed predicate itself.
// ---------------------------------------------------------------------------

/// The single file allowed to read `MIND_BENCH_REQUIRE` out of the environment.
const PREDICATE_OWNER: &str = "common/gate.rs";

/// The env accessors that READ a variable's value.
///
/// `Command::env` / `Command::env_remove` SET a child's environment and are how
/// the enforcement path is tested, so they are deliberately absent: writing the
/// variable for a child is not a second reader of the rule.
const ENV_READS: &[&str] = &["var_os(", "var("];

/// Does `line` read the fail-closed variable straight from the environment?
///
/// Matched by ACCESSOR plus the variable's identity — its literal name or the
/// `REQUIRE_VAR` constant that spells it — never by the surrounding shape. The
/// site this closes on is an `assert!(var_os(..).is_none(), ..)` inside a
/// `-> bool` helper: it announces no skip and returns no bare `return`, so it
/// was structurally invisible to BOTH detectors above while implementing a
/// competing copy of the very rule they enforce routing to.
fn reads_require_var_from_env(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("//") {
        return false; // prose naming the variable is not a second reader
    }
    let names_it = t.contains("MIND_BENCH_REQUIRE") || t.contains("REQUIRE_VAR");
    names_it && ENV_READS.iter().any(|r| t.contains(r))
}

#[test]
fn the_fail_closed_predicate_has_exactly_one_owner() {
    // THE DEFECT THIS PINS: two readers of one variable drifted into two
    // DIFFERENT rules. `gate::enforce_real_backend` requires the value `1`; the
    // hand-rolled `assert!(var_os("MIND_BENCH_REQUIRE").is_none())` made ANY
    // value enforce, so a value that reads as "off" hard-failed one gate while
    // its neighbour skipped cleanly under the identical environment. Measured
    // before this arm existed, MLIR tools off PATH and MIND_BENCH_REQUIRE=0:
    //
    //   mindfuzz_cross_substrate  -> panicked "MIND_BENCH_REQUIRE is set but
    //                                'mlir-opt' is not on PATH" -> FAILED
    //   cross_substrate_identity  -> SDLC-GATE ... ran=0 fail=0 -> ok, 1 passed
    //
    // One variable, one rule, one reader.
    let mut second_owners = Vec::new();
    for (rel, text) in test_sources() {
        if rel == PREDICATE_OWNER {
            continue;
        }
        for (i, line) in text.lines().enumerate() {
            if reads_require_var_from_env(line) {
                second_owners.push(format!("{}:{}", rel, i + 1));
            }
        }
    }
    assert!(
        second_owners.is_empty(),
        "these sites read MIND_BENCH_REQUIRE from the environment instead of \
         asking common::gate, which owns the rule. A second reader is a second \
         RULE: the two spellings measured here disagreed about the value `0`. \
         Route the decision through common::gate::{{skipped, skipped_optional, \
         compiled, enforce_real_backend}}.\n  {}",
        second_owners.join("\n  ")
    );
}

#[test]
fn the_scanner_can_see_a_second_owner_of_the_predicate() {
    // Positive control for the FIFTH detector. A prohibition that matches
    // nothing is the ran=0 defect this file exists to forbid, in its active
    // form — and this detector's whole reason to exist is that the two above it
    // reported a clean tree while a competing predicate was live.
    let var = "MIND_BENCH_REQUIRE";
    for spelling in [
        format!("            std::env::var_os(\"{var}\").is_none(),"),
        format!("    if std::env::var(\"{var}\").is_ok() {{"),
        format!("    let on = env::var_os(\"{var}\").is_some();"),
        // Reading the OWNER's constant is still a second reader of the rule.
        "    let on = std::env::var_os(gate::REQUIRE_VAR).is_some();".to_string(),
    ] {
        assert!(reads_require_var_from_env(&spelling), "missed: {spelling}");
    }
    // Routing through the owner, SETTING a child's environment, and prose that
    // merely names the variable are all legitimate and must not be flagged.
    for ok in [
        "    if crate::common::gate::enforce_real_backend() {".to_string(),
        "        cmd.env(gate::REQUIRE_VAR, v);".to_string(),
        "            .env_remove(gate::REQUIRE_VAR);".to_string(),
        format!("    // NOTE: this gate does not honour {var}"),
        format!("        \"{var} is set but the artifact is a stub\","),
    ] {
        assert!(!reads_require_var_from_env(&ok), "false positive: {ok}");
    }
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

/// Does `line` re-implement the banned two-substring capability test?
///
/// The banned predicate ANDed two floating `contains(...)` probes for the
/// feature name and the word "requires". It is superstring-satisfiable: ANY
/// compiler failure whose stderr happens to carry both tokens anywhere graded as
/// a capability gap. The classifier that replaced it matches the stable
/// diagnostic CODE, owned by `libmind::diagnostics::capability`.
///
/// VARIABLE-AGNOSTIC BY CONSTRUCTION. The needle used to be one literal spelled
/// with the receiver name `stderr.`, so the identical predicate written over
/// `e.`, `err.` or `es_err.` was invisible — measured, eight live sites kept it
/// while this gate reported the shape gone. Only the two method calls and the
/// `&&` between them are matched; the receivers are read as identifier runs of
/// any name. The tokens are assembled at run time so a repo-wide grep for the
/// bad shape returns call sites only.
fn is_superstring_capability_test(line: &str) -> bool {
    let feature = format!(".contains(\"{}\")", "mlir-build");
    let word = format!(".contains(\"{}\")", "requires");
    let Some(p) = line.find(&feature) else {
        return false;
    };
    // A receiver of ANY name must sit left of the first probe.
    if !line.as_bytes()[..p]
        .last()
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return false;
    }
    let rest = line[p + feature.len()..].trim_start();
    let Some(rest) = rest.strip_prefix("&&") else {
        return false; // an `||` of unrelated tokens is not the banned AND-pair
    };
    let rest = rest.trim_start();
    let n = ident_len(rest);
    n > 0 && rest[n..].starts_with(&word)
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
    let mut hits = Vec::new();
    for (rel, text) in test_sources() {
        for (i, line) in text.lines().enumerate() {
            if is_superstring_capability_test(line) {
                hits.push(format!("{}:{}", rel, i + 1));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "these sites re-implement the superstring-satisfiable capability test \
         instead of calling common::gate::is_capability_gap (which delegates to \
         libmind::diagnostics::capability).\n  {}",
        hits.join("\n  ")
    );
}

/// The literal token every `skipped_optional` call site must carry overhead.
///
/// The check used to accept any comment containing the substring "opt", which
/// `// needs mlir-opt` satisfies — a comment naming a TOOLCHAIN gap bought a
/// pass for the one outcome `MIND_BENCH_REQUIRE` does not close. The token is
/// deliberately shouty and unlikely to appear by accident; all six current call
/// sites already carry it.
const OPTIONAL_INPUT_TOKEN: &str = "OPTIONAL INPUT";

/// Does a comment in the eight lines above `lines[i]` carry the token?
fn names_optional_input(lines: &[&str], i: usize) -> bool {
    let head = i.saturating_sub(8);
    lines[head..i]
        .iter()
        .any(|l| l.trim_start().starts_with("//") && l.contains(OPTIONAL_INPUT_TOKEN))
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
            if !names_optional_input(&lines, i) {
                undocumented.push(format!("{}:{}", rel, i + 1));
            }
        }
    }
    assert!(
        undocumented.is_empty(),
        "these opt-in skips carry no `{OPTIONAL_INPUT_TOKEN}` comment naming \
         what is optional and who supplies it; an undocumented one is \
         indistinguishable from a fail-open escape hatch.\n  {}",
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

#[test]
fn the_scanner_can_see_the_silent_probe_shape() {
    // Positive control for the SECOND detector. A detector that matches nothing
    // is the `ran=0` defect this file exists to forbid, in its active form: the
    // announced-skip scan reported a clean tree while 17 silent sites were live.
    let silent = ["if !bin.exists() {", "        return;", "    }"];
    assert_eq!(
        silent_probe_sites("specimen.rs", &silent),
        vec!["specimen.rs:1 (silent probe)".to_string()]
    );

    // Variable-agnostic: renaming the receiver must not escape the prohibition.
    for probe in [
        "    if !binary.exists() {",
        "    if !artifact_2.exists() {",
        // Inline receivers: these defeated the first draft of the detector.
        "    if !Path::new(\"/nonexistent\").exists() {",
        "    if !std::path::Path::new(p).exists() {",
        "        if which::which(\"mlir-opt\").is_err() {",
        "    if std::env::var_os(\"MIND_TRACKING_CORPUS_DIR\").is_none() {",
    ] {
        assert!(is_capability_probe(probe), "missed probe: {probe}");
    }
    // Prose quoting the shape, and a probe that is not a run/skip decision.
    assert!(!is_capability_probe("    // if !bin.exists() { return; }"));
    assert!(!is_capability_probe("    if bin.exists() {"));

    // Every bare-return spelling, and a return that hands back a real verdict.
    assert!(is_bare_return("        return;"));
    assert!(is_bare_return("            return None;"));
    assert!(is_bare_return("    return Ok(());"));
    assert!(!is_bare_return("        return Some(bin);"));

    // A routed probe is not a finding.
    let routed = [
        "if !bin.exists() {",
        "    gate::skipped(\"t\", \"no mindc\");",
        "    return;",
    ];
    assert!(silent_probe_sites("specimen.rs", &routed).is_empty());

    // The bare env-var NAME must no longer exempt anything: a comment that
    // merely mentions it is prose, not routing.
    let commented = [
        "// NOTE: this gate does not honour MIND_BENCH_REQUIRE",
        "if !bin.exists() {",
        "    return;",
        "}",
    ];
    assert_eq!(
        silent_probe_sites("specimen.rs", &commented),
        vec!["specimen.rs:2 (silent probe)".to_string()]
    );
}

#[test]
fn the_scanner_can_see_the_capability_test_under_any_receiver_name() {
    // Positive control for the THIRD detector. The old needle was one literal
    // spelled `stderr.`; these four receivers are the ones measured live.
    for name in ["stderr", "e", "err", "es_err"] {
        let line = format!(
            "    if {name}.contains(\"{}\") && {name}.contains(\"{}\") {{",
            "mlir-build", "requires"
        );
        assert!(is_superstring_capability_test(&line), "missed: {line}");
    }
    // Mixed receivers, and the negated form, are the same banned predicate.
    assert!(is_superstring_capability_test(&format!(
        "    !(e.contains(\"{}\") && stderr.contains(\"{}\"))",
        "mlir-build", "requires"
    )));
    // An `||` of unrelated tokens is a different, legitimate assertion.
    assert!(!is_superstring_capability_test(&format!(
        "        stderr.contains(\"tool not found\") || stderr.contains(\"{}\"),",
        "mlir-build"
    )));
    // The routed replacement must not read as the banned shape.
    assert!(!is_superstring_capability_test(
        "        if crate::common::gate::is_capability_gap(&e) {"
    ));
}

#[test]
fn the_optional_input_check_rejects_a_merely_opt_shaped_comment() {
    // Positive control for the FOURTH check. `// needs mlir-opt` contains "opt"
    // and used to buy a pass for the one outcome MIND_BENCH_REQUIRE cannot
    // close — while naming a TOOLCHAIN gap, the class that must fail closed.
    let weak = [
        "        // needs mlir-opt",
        "        gate::skipped_optional(",
    ];
    assert!(!names_optional_input(&weak, 1));
    let strong = [
        "        // OPTIONAL INPUT: supplied by the operator via an env var.",
        "        gate::skipped_optional(",
    ];
    assert!(names_optional_input(&strong, 1));
}
