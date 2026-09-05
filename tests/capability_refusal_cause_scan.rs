// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Source-discipline gate: every HOST-capability refusal in `src/` mints its
//! cause code.
//!
//! `tests/fail_closed_capability_skip.rs` proves what the classifier does with
//! a given wire. It cannot prove the compiler still PUTS a cause on that wire —
//! and an uncoded refusal is graded a compiler regression, so a genuine
//! capability gap hard-fails every host that has the gap. This file scans the
//! compiler's own sources for that shape.

use std::path::{Path, PathBuf};

// --- the cause-code anti-drift scan ----------------------------------------
//
// The classifier reads a CODE, which is only as good as the compiler's
// discipline in stamping one. A capability refusal added WITHOUT a code does
// not widen the fail-open hole (uncoded fails closed) but re-creates the
// original defect from the other side: a genuine capability gap hard-failing
// every host that lacks the capability.
//
// The FIRST version of this scan was keyed on ONE prose fragment
// (`requires building with the 'mlir`) and was therefore structurally unable to
// see any other kind of host-capability refusal. It watched the E5003 family
// while `find_runtime_lib`'s "MIND runtime not found for backend" refusal —
// an equally unambiguous host fact, and the DEFAULT state of a public checkout,
// which ships without the separately licensed runtime — went uncoded and
// reddened two test binaries on every such host.
//
// So the scan is keyed on the CLASS of refusal, one case per registered
// host-capability cause, and the table is pinned to `CAPABILITY_CODES` in BOTH
// directions: a new cause with no class fails, and a class naming no cause
// fails. Two independent scopes are derived rather than hand-copied — the file
// set (walk `src/`) and the set of classes (the cause registry) — so neither
// can fall behind the code it checks.

use libmind::diagnostics::capability::{
    CAPABILITY_CODES, NATIVE_TOOLCHAIN_ABSENT, NO_NATIVE_BACKEND, RUNTIME_LIBRARY_ABSENT,
    TARGET_BACKEND_UNAVAILABLE,
};

/// One recognised CLASS of host-capability refusal: the compiler telling the
/// user that something the HOST was supposed to provide is missing.
struct RefusalClass {
    /// The cause every refusal of this class must carry.
    code: &'static str,
    /// What the host is missing, for the failure message.
    what: &'static str,
    /// The phrases the compiler uses to say it. Several per class is normal —
    /// the same fact is worded differently on the CLI route and the project
    /// route, which is exactly why the classifier may not read wording.
    wordings: &'static [&'static str],
}

/// Every class, one per registered host-capability cause.
const REFUSAL_CLASSES: &[RefusalClass] = &[
    RefusalClass {
        code: TARGET_BACKEND_UNAVAILABLE,
        what: "a backend for the requested target",
        wordings: &["no backend available for target"],
    },
    RefusalClass {
        code: RUNTIME_LIBRARY_ABSENT,
        what: "the backend's runtime library (the separately licensed mind-runtime)",
        wordings: &["MIND runtime not found for backend"],
    },
    RefusalClass {
        code: NO_NATIVE_BACKEND,
        what: "a native backend in this binary (built without `mlir-build`)",
        wordings: &[
            "requires building with the 'mlir",
            "requires the 'mlir-build' feature",
        ],
    },
    RefusalClass {
        code: NATIVE_TOOLCHAIN_ABSENT,
        what: "the native toolchain (`mlir-opt` / `clang`) on PATH",
        wordings: &["tool not found"],
    },
];

/// The vocabulary by which the compiler CONSTRUCTS a refusal.
///
/// This is what separates a refusal from an ordinary string that merely
/// mentions a missing capability — notably the C and shell launcher templates
/// in `project::mod`, whose "MIND runtime not found" text is printed by the
/// EMITTED ARTIFACT at execution time, on a different stream at a different
/// time, and is not a `mindc` diagnostic at all.
///
/// Drift here is fail-OPEN (a refusal built some new way is not scanned), so it
/// is held shut from the other side by
/// `every_refusal_class_is_still_emitted`: a class that stops being visible as
/// a refusal fails the build.
const REFUSAL_CONSTRUCTORS: &[&str] = &[
    "anyhow!",
    "CodedRefusal::new(",
    "Diagnostic::error(",
    "eprintln!(",
    "#[error(",
    "BuildError::",
    ".tag(",
];

/// The vocabulary by which a refusal carries its CAUSE: a rendered code slot
/// (`error[build][{}]:`), a literal `E50xx`, or a reference to the registry
/// that owns the codes.
const CAUSE_MARKERS: &[&str] = &["[{}]:", "[E50", "FallbackReason::"];

/// Does `line` put a cause code on the wire, or name the registry that does?
fn line_carries_a_code(line: &str) -> bool {
    CAUSE_MARKERS.iter().any(|m| line.contains(m))
        || CAPABILITY_CODES.iter().any(|c| {
            // The constant's NAME, e.g. `NATIVE_TOOLCHAIN_ABSENT`, which is how
            // a `#[error(...)]` attribute interpolates it.
            line.contains(code_constant_name(c))
        })
}

/// The Rust constant name that owns `code`. Exhaustive over `CAPABILITY_CODES`
/// by the bijection test below, so a new cause cannot be added without naming
/// it here.
fn code_constant_name(code: &str) -> &'static str {
    match code {
        c if c == TARGET_BACKEND_UNAVAILABLE => "TARGET_BACKEND_UNAVAILABLE",
        c if c == RUNTIME_LIBRARY_ABSENT => "RUNTIME_LIBRARY_ABSENT",
        c if c == NO_NATIVE_BACKEND => "NO_NATIVE_BACKEND",
        c if c == NATIVE_TOOLCHAIN_ABSENT => "NATIVE_TOOLCHAIN_ABSENT",
        other => panic!("no constant name registered for capability code {other}"),
    }
}

/// The lines of the refusal EXPRESSION enclosing `i`: walk out to the nearest
/// statement or block boundary, capped, so a code belonging to a DIFFERENT
/// refusal a few lines away cannot vouch for this one.
fn enclosing_expression<'a>(lines: &[&'a str], i: usize) -> Vec<&'a str> {
    /// A line that ends a statement or a block: the expression cannot span it.
    fn is_boundary(line: &str) -> bool {
        let t = line.trim_end();
        t.is_empty() || t.ends_with(';') || t.ends_with('}') || t.ends_with('{')
    }
    const SPAN: usize = 8;
    let mut lo = i;
    while lo > 0 && i - lo < SPAN && !is_boundary(lines[lo - 1]) {
        lo -= 1;
    }
    let mut hi = i;
    while hi + 1 < lines.len() && hi - i < SPAN && !is_boundary(lines[hi]) {
        hi += 1;
    }
    lines[lo..=hi].to_vec()
}

/// Is the expression enclosing `i` one the COMPILER refuses with (as opposed to
/// a template for text the emitted artifact prints at execution)?
fn is_a_compiler_refusal(lines: &[&str], i: usize) -> bool {
    enclosing_expression(lines, i)
        .iter()
        .any(|l| REFUSAL_CONSTRUCTORS.iter().any(|c| l.contains(c)))
}

/// `src/diagnostics/capability.rs` holds this rule's NEGATIVE controls — the
/// uncoded prose its unit tests assert is not a capability gap. A scanner that
/// flagged its own specimen jar could not keep them.
const CAPABILITY_SELF: &str = "capability.rs";

fn src_sources() -> Vec<(PathBuf, String)> {
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
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter(|p| p.file_name().and_then(|s| s.to_str()) != Some(CAPABILITY_SELF))
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read src source");
            (p, text)
        })
        .collect()
}

/// Every in-scope refusal site of `class`, as `(file:line, carries_a_code)`.
fn refusal_sites(class: &RefusalClass) -> Vec<(String, bool)> {
    let mut sites = Vec::new();
    for (path, text) in src_sources() {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//")
                || !class.wordings.iter().any(|w| line.contains(w))
                || !is_a_compiler_refusal(&lines, i)
            {
                continue;
            }
            let coded = enclosing_expression(&lines, i)
                .iter()
                .any(|l| line_carries_a_code(l));
            sites.push((format!("{}:{}", path.display(), i + 1), coded));
        }
    }
    sites
}

#[test]
fn the_refusal_class_table_covers_every_capability_cause() {
    // The Article-XII.5 shape: the scan's SCOPE is read out of the registry it
    // guards, in both directions, instead of being hand-copied beside it. The
    // first version of this scan knew one class of four and could not see the
    // other three; nothing failed when a fourth cause was registered.
    for code in CAPABILITY_CODES {
        assert!(
            REFUSAL_CLASSES.iter().any(|c| c.code == code),
            "capability cause {code} has no refusal class, so no scan can tell \
             whether its refusals carry their code. Add a `RefusalClass` naming \
             the wording(s) the compiler uses for it."
        );
        // ... and its constant name is registered (this panics otherwise).
        let _ = code_constant_name(code);
    }
    for class in REFUSAL_CLASSES {
        assert!(
            CAPABILITY_CODES.contains(&class.code),
            "refusal class for {} names {}, which no registered host-capability \
             cause owns",
            class.what,
            class.code
        );
        assert!(
            !class.wordings.is_empty(),
            "class {} has no wording",
            class.code
        );
    }
    assert_eq!(
        REFUSAL_CLASSES.len(),
        CAPABILITY_CODES.len(),
        "one class per capability cause, no duplicates"
    );
}

#[test]
fn every_capability_refusal_carries_its_cause_code() {
    let mut uncoded = Vec::new();
    for class in REFUSAL_CLASSES {
        for (site, coded) in refusal_sites(class) {
            if !coded {
                uncoded.push(format!("{site}  (missing {})", class.code));
            }
        }
    }
    assert!(
        uncoded.is_empty(),
        "these refusals name a missing HOST capability but carry no cause code, \
         so the harness grades a genuine capability gap as a compiler regression \
         and hard-fails every host that lacks it. Emit the cause: \
         `error[<phase>][{{}}]:` with the constant from \
         `diagnostics::capability`, or refuse through \
         `diagnostics::refusal::CodedRefusal` when the error travels as \
         `anyhow`.\n  {}",
        uncoded.join("\n  ")
    );
}

#[test]
fn every_refusal_class_is_still_emitted() {
    // The other side of the ratchet. `every_capability_refusal_carries_its_cause_code`
    // passes vacuously for a class whose wording no longer appears — because it
    // was reworded, or because the refusal moved to a construction shape
    // `REFUSAL_CONSTRUCTORS` does not recognise. Either way the scan has stopped
    // watching that class and must say so rather than report a silent `ran=0`.
    for class in REFUSAL_CLASSES {
        assert!(
            !refusal_sites(class).is_empty(),
            "no refusal in `src/` was recognised for the class that reports a host \
             missing {}. Either the wording changed (update `wordings`), or the \
             refusal is now built in a way `REFUSAL_CONSTRUCTORS` does not list — \
             in which case the scan is blind to the whole class.",
            class.what
        );
    }
}

#[test]
fn the_cause_code_scan_can_still_see_the_bad_shape() {
    // Positive control: the scan is only evidence if it fails on the shape it
    // forbids. Two real pre-fix lines, one per defect generation.
    let bad_feature = r#"        eprintln!("error[build]: --emit-obj requires building with the 'mlir-build' feature");"#;
    let bad_runtime = r#"    Err(anyhow!("MIND runtime not found for backend '{}'.", backend))"#;
    for bad in [bad_feature, bad_runtime] {
        let lines = vec![bad];
        assert!(is_a_compiler_refusal(&lines, 0), "{bad}");
        assert!(!line_carries_a_code(bad), "{bad}");
        assert!(
            REFUSAL_CLASSES
                .iter()
                .any(|c| c.wordings.iter().any(|w| bad.contains(w))),
            "no class recognises {bad}"
        );
    }
    let good = r#"            "error[build][{}]: --emit-obj requires building with the 'mlir-build' feature","#;
    assert!(line_carries_a_code(good));
}

#[test]
fn artifact_text_is_not_a_compiler_refusal() {
    // The scope control for `is_a_compiler_refusal`. The emitted launcher's own
    // message names the same missing runtime and the same licensing surface, but
    // it is printed by the ARTIFACT at execution time — it is not a `mindc`
    // diagnostic and must not be required to carry a compiler cause code.
    let lines = vec![
        r#"        fprintf(stderr, "Error: MIND runtime not found\n");"#,
        r#"        fprintf(stderr, "See https://mindlang.dev/enterprise for licensing.\n");"#,
    ];
    assert!(!is_a_compiler_refusal(&lines, 1));
}
