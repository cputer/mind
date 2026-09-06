// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Positive controls for [`super`]: the probe vocabulary and the block scanner.
//!
//! A detector that matches nothing is the `ran=0` defect the prohibition exists
//! to forbid, in its active form — the announced-skip scan reported a clean tree
//! while 17 silent sites were live. Every specimen here is a WHOLE PROGRAM
//! FRAGMENT handed to [`super::skip_block_sites`], never a line fed to a
//! predicate, because the shapes that escaped previous detectors are all legal
//! Rust a line-pair predicate cannot even be asked about.
//!
//! This file is scanned by the prohibition it backs rather than exempted from
//! it: the specimens are string literals, and the scanner reads masked CODE.

/// One whole-program specimen per vocabulary entry, keyed BY the entry.
///
/// The pairing is what makes [`the_vocabulary_and_its_controls_agree`]
/// possible: a scan scope and its own reference detection written as two
/// hand-copied lists is the drift this repo keeps paying for, so the
/// vocabulary is READ OUT of the thing being proven rather than restated.
const PROBE_SPECIMENS: &[(&str, &str)] = &[
    (".exists(", "if !bin.exists() {\n    return;\n}"),
    (
        ".try_exists(",
        "if !p.try_exists().unwrap_or(false) {\n    return;\n}",
    ),
    (".is_file(", "if !p.is_file() {\n    return;\n}"),
    (".is_dir(", "if !p.is_dir() {\n    return;\n}"),
    (
        "metadata(",
        "if std::fs::metadata(p).is_err() {\n    return;\n}",
    ),
    (
        "which(",
        "if which::which(\"mlir-opt\").is_err() {\n    return;\n}",
    ),
    (
        "var_os(",
        "if std::env::var_os(V).is_none() {\n    return;\n}",
    ),
    ("var(", "if std::env::var(V).is_err() {\n    return;\n}"),
    (".success(", "if !out.status.success() {\n    return;\n}"),
    (
        ".output(",
        "if Command::new(\"mlir-opt\").output().is_err() {\n    return;\n}",
    ),
    (
        ".status(",
        "if Command::new(\"mlir-opt\").status().is_err() {\n    return;\n}",
    ),
    (
        "read_to_string(",
        "let s = match std::fs::read_to_string(p) {\n    Ok(s) => s,\n\
         Err(_) => return,\n};",
    ),
    (
        "fs::read(",
        "let b = match std::fs::read(p) {\n    Ok(b) => b,\n    Err(_) => return,\n};",
    ),
];

/// Every vocabulary entry is EXERCISED, and every specimen names a live
/// entry.
///
/// The gate this file backs was widened once already because its claim was
/// wider than its detector. A spelling added to the vocabulary with no
/// control is the same defect one level down — it would read as covered
/// while nothing ever proved the scanner sees it — and a control left
/// behind for a deleted spelling is a proof about code that is gone. Both
/// directions are asserted, so the two can never drift apart.
#[test]
fn the_vocabulary_and_its_controls_agree() {
    for needle in super::AVAILABILITY_PROBES.iter().chain(super::INPUT_READS) {
        let (_, src) = PROBE_SPECIMENS
            .iter()
            .find(|(n, _)| n == needle)
            .unwrap_or_else(|| {
                panic!(
                    "vocabulary entry `{needle}` has no positive control in \
                     PROBE_SPECIMENS; an unexercised spelling reads as covered \
                     while nothing proves the scanner can see it"
                )
            });
        assert!(
            !super::skip_block_sites("specimen.rs", src).is_empty(),
            "vocabulary entry `{needle}` did not fire on its own control:\n{src}"
        );
    }
    for (needle, _) in PROBE_SPECIMENS {
        assert!(
            super::AVAILABILITY_PROBES.contains(needle) || super::INPUT_READS.contains(needle),
            "PROBE_SPECIMENS holds a control for `{needle}`, which is in \
             neither vocabulary — it proves nothing about the live rule"
        );
    }
}

/// An input read's PAYLOAD must not travel to a later head as if it were an
/// availability answer.
///
/// The specimen is `fmt_stdlib_stability::check_or_skip` reduced to its
/// shape. Tainting `read_to_string` propagated `src` through a call into
/// `formatted` and flagged `if formatted == src { return; }` — the branch
/// taken when the file under test is STABLE, i.e. the success path of a
/// test that asserted plenty. Both returns below are legitimate and neither
/// may be reported.
#[test]
fn an_input_reads_payload_does_not_taint_a_later_head() {
    let src = "let src = std::fs::read_to_string(&path)\n\
         .unwrap_or_else(|e| panic!(\"cannot read: {e}\"));\n\
         let formatted = match format_source(&src, &cfg) {\n\
         Ok(f) => f,\n\
         Err(e) => {\n\
         if skip_reason(stem).is_some() {\n\
         return;\n\
         }\n\
         panic!(\"format failed: {e}\");\n\
         }\n\
         };\n\
         if formatted == src {\n\
         return;\n\
         }";
    assert!(
        super::skip_block_sites("specimen.rs", src).is_empty(),
        "the stable-file success path was reported as a fail-open skip: {:?}",
        super::skip_block_sites("specimen.rs", src)
    );
}

#[test]
fn the_scanner_can_see_the_probe_block_shape() {
    // Positive control for the SECOND detector. A detector that matches nothing
    // is the `ran=0` defect this file exists to forbid, in its active form: the
    // announced-skip scan reported a clean tree while 17 silent sites were live.
    //
    // Every specimen below is a WHOLE PROGRAM FRAGMENT handed to the scanner,
    // not a line fed to a predicate. That is the point of this arm: the shapes
    // EV1-EV5 further down are all legal Rust that a line-pair predicate cannot
    // even be asked about.
    fn hits(src: &str) -> Vec<String> {
        super::skip_block_sites("specimen.rs", src)
    }
    assert_eq!(
        hits("if !bin.exists() {\n    return;\n}"),
        vec!["specimen.rs:1 (probe block)".to_string()]
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
        // The FAILED-COMPILE shape. Absent from the vocabulary until three
        // sites in mindc_cache_phase_f.rs were measured grading `ok` on a build
        // that had just printed `error[build][E5003]`.
        "    if !s1.success() {",
        "        if !out.status.success() {",
        // POLARITY-FREE. Every draft that demanded a literal `if !` was evaded
        // by writing the same decision the other way up.
        "    if s1.success() {",
        "    if bin.exists() {",
        // ORDINARY spellings of the same question. The vocabulary was an
        // allow-list of four calls, so each of these was a fail-open skip
        // the prohibition could not see; measured one per fresh test file
        // against the built scanner, every one came back clean.
        "    if !p.is_file() {",
        "    if !p.is_dir() {",
        "    if std::fs::metadata(p).is_err() {",
        "    if !p.try_exists().unwrap_or(false) {",
        "    if std::env::var(\"MIND_TRACKING_CORPUS_DIR\").is_err() {",
        "    if Command::new(\"mlir-opt\").arg(\"--version\").output().is_err() {",
        "    if Command::new(\"mlir-opt\").status().is_err() {",
    ] {
        let src = format!("{probe}\n        return;\n    }}");
        assert!(!hits(&src).is_empty(), "missed probe: {probe}");
    }

    // The OTHER branch is the same skip written the other way up. Every draft
    // that scrutinised only the first block was evaded by moving the early-out.
    assert_eq!(
        hits("if bin.exists() {\n    run();\n} else {\n    return;\n}"),
        vec!["specimen.rs:1 (probe block)".to_string()]
    );

    // A `return` that hands back a real verdict is not an early-out, and a
    // block with no early-out at all is not a skip.
    assert!(hits("if !bin.exists() {\n    return Some(bin);\n}").is_empty());
    assert!(hits("if !bin.exists() {\n    panic!(\"no mindc\");\n}").is_empty());
    // A probe with no control-flow block is a plain assertion path.
    assert!(hits("assert!(bin.exists(), \"build mindc first\");").is_empty());

    // A routed probe is not a finding — inside the block, or in the head.
    assert!(
        hits("if !bin.exists() {\n    gate::skipped(\"t\", \"no mindc\");\n    return;\n}")
            .is_empty()
    );
    assert!(hits("let Some(b) = gate::compiled(\"t\", &o) else {\n    return;\n};").is_empty());

    // The bare env-var NAME must no longer exempt anything, and neither may a
    // comment that merely NAMES the helper: prose is not routing.
    assert_eq!(
        hits("// NOTE: does not honour MIND_BENCH_REQUIRE\nif !bin.exists() {\n    return;\n}"),
        vec!["specimen.rs:2 (probe block)".to_string()]
    );
    assert_eq!(
        hits("// gate::skipped panics under enforcement\nif !bin.exists() {\n    return;\n}"),
        vec!["specimen.rs:2 (probe block)".to_string()]
    );
}

/// The ordinary Rust spellings measured walking past a previous detector.
///
/// Each was measured live: appended to `tests/if_expr.rs` on the tip and the
/// built scanner re-run, every one reported `test result: ok. 12 passed` — a
/// fail-open skip added to the tree while the prohibition said the tree was
/// clean. They are the reason the decision moved from a two-line text window to
/// the brace-balanced block in `skip_shape_scan`, and they are pinned here so
/// the window can never come back.
#[test]
fn the_measured_escapes_are_closed() {
    let escapes = [
        // EV1 — the early-out sits FOUR lines under the probe; the window was 2.
        "if !Path::new(p).exists() {\n    println!(\"a\");\n    println!(\"b\");\n\
         println!(\"c\");\n    return;\n}",
        // EV2 — a let-else: no `if` at all.
        "let Ok(_t) = which::which(\"mlir-opt\") else { return; };",
        // EV3 — a match arm: no `if`, and the exit is `return,` not `return;`.
        "match std::env::var_os(V) { None => return, Some(_) => {} }",
        // EV4 — the probe is BOUND first, so the scrutinee names no method.
        "let ok = Path::new(p).exists();\nif !ok {\n    return;\n}",
        // EV5 — the same, over a child process's exit status.
        "let ok = out.status.success();\nif !ok {\n    return;\n}",
        // EV6 — a tracked-input read whose failure is swallowed by a match
        // arm. LIVE on `examples/mindc_mind/main.mind` in
        // `tests/stmt_keyword_recognizer.rs` while the vocabulary was an
        // allow-list of four calls, and doubly invisible: `head_of` gave up
        // on any `let` line carrying no `else`, so the match was never even
        // read as a head.
        "let src = match std::fs::read_to_string(p) {\n    Ok(s) => s,\n\
         Err(_) => return,\n};",
        // EV7 — the same decision over raw bytes.
        "let b = match std::fs::read(p) {\n    Ok(b) => b,\n    Err(_) => return,\n};",
        // EV8 — the non-`_os` env read, which `var_os(` never matched.
        "let Ok(v) = std::env::var(V) else {\n    return;\n};",
    ];
    for (n, src) in escapes.iter().enumerate() {
        assert!(
            !super::skip_block_sites("specimen.rs", src).is_empty(),
            "EV{} still escapes the prohibition:\n{src}",
            n + 1
        );
    }
    // Routing any of them clears it — the gate forbids the fail-OPEN skip, not
    // the skip. Proven on the shape furthest from the original window (EV4's
    // bound probe), so this control cannot pass by matching nothing.
    assert!(
        super::skip_block_sites(
            "specimen.rs",
            "let ok = Path::new(p).exists();\nif !ok {\n    gate::skipped(\"t\", \"gone\");\n\
             return;\n}",
        )
        .is_empty()
    );
}
