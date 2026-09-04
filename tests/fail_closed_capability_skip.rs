// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Fail-CLOSED contract for the shared capability-skip helper.
//!
//! # The defect this gate defends against
//!
//! 24 integration-test sites carried the shape
//!
//! ```text
//! if !status.success() { println!("... build failed -> skipped"); return; }
//! ```
//!
//! with no panic and no `MIND_BENCH_REQUIRE` consultation. On a tier that
//! installs and PATH-verifies the MLIR toolchain, the only remaining way that
//! branch fires is a real compiler regression — which then graded as a PASS.
//! A determinism gate, eleven std-surface runtime-value gates and nine build
//! and cache gates were all fail-OPEN in exactly this way.
//!
//! Every assertion below is on `common::gate`, the single helper those sites
//! now route through, so the contract is proven once instead of 24 times.
//! The legitimate capability skip — a host genuinely lacking `mlir-build` or
//! the `mlir-opt`/`clang` binaries — is asserted to SURVIVE, because removing
//! it would be a different bug, not a fix.

mod common;

use common::gate::{self, Outcome};
use std::path::{Path, PathBuf};

/// Verbatim stderr of `mindc --emit-shared` built without `mlir-build`
/// (`src/bin/mindc.rs:3286`). This is the ONLY compile failure that may skip.
const CAP_FEATURE: &str =
    "error[build]: --emit-shared requires building with the 'mlir-build' feature\n";

/// Verbatim stderr shape when the feature is on but the toolchain binary is
/// absent from PATH (`BuildError::ToolNotFound`, `src/eval/mlir_build.rs:72`).
const CAP_TOOL: &str = "error: tool not found: mlir-opt\n";

/// A real compiler regression: the class that must NEVER grade as a pass.
const REAL_FAILURE: &str = "error[E0308]: mismatched types in `idiv`\n";

// --- the pure decision core -------------------------------------------------

#[test]
fn success_is_compiled() {
    assert_eq!(gate::classify(true, "", false), Outcome::Compiled);
    assert_eq!(gate::classify(true, "", true), Outcome::Compiled);
}

#[test]
fn genuine_capability_gap_still_skips_when_not_enforcing() {
    // MUST NOT CHANGE: a host genuinely lacking the toolchain keeps skipping.
    assert_eq!(
        gate::classify(false, CAP_FEATURE, false),
        Outcome::CapabilitySkip
    );
    assert_eq!(
        gate::classify(false, CAP_TOOL, false),
        Outcome::CapabilitySkip
    );
}

#[test]
fn real_compiler_failure_is_never_a_skip() {
    // The whole point: today this printed "skipping" and the test PASSED.
    match gate::classify(false, REAL_FAILURE, false) {
        Outcome::Failed(s) => assert!(s.contains("mismatched types")),
        other => panic!("a real compiler failure must fail closed, got {other:?}"),
    }
}

#[test]
fn empty_stderr_failure_is_never_a_skip() {
    // `.status()` sites captured no stderr at all; an empty diagnostic must
    // not be mistaken for a capability gap.
    match gate::classify(false, "", false) {
        Outcome::Failed(_) => {}
        other => panic!("an undiagnosed failure must fail closed, got {other:?}"),
    }
}

#[test]
fn enforcement_forbids_even_a_genuine_capability_skip() {
    // MIND_BENCH_REQUIRE=1 -> the exec tier cannot pass vacuously.
    for stderr in [CAP_FEATURE, CAP_TOOL] {
        match gate::classify(false, stderr, true) {
            Outcome::Failed(_) => {}
            other => panic!("MIND_BENCH_REQUIRE=1 must forbid a skip, got {other:?}"),
        }
    }
}

// --- the call-site wrapper the 24 converted sites use -----------------------

fn stub_compiler(name: &str, exit_code: i32, stderr: &str) -> PathBuf {
    // Per-PROCESS directory: a fixed shared path made two concurrent runs of
    // this harness write and exec the same file, and the loser saw ETXTBSY —
    // a flake in the very gate that exists to remove false results.
    let dir = std::env::temp_dir().join(format!("mind_fail_closed_stub_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir stub dir");
    let p = dir.join(name);
    std::fs::write(
        &p,
        format!("#!/bin/sh\nprintf '%s' \"{stderr}\" >&2\nexit {exit_code}\n"),
    )
    .expect("write stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    p
}

fn run_stub(stub: &Path) -> std::process::Output {
    std::process::Command::new(stub)
        .output()
        .expect("run stub compiler")
}

#[test]
fn broken_compiler_panics_through_the_call_site_wrapper() {
    let stub = stub_compiler("mindc_broken", 1, "error: mismatched types");
    let out = run_stub(&stub);
    let r = std::panic::catch_unwind(|| gate::compiled_with("stub-target", &out, false));
    let msg = r.expect_err("a broken compiler must PANIC, not skip");
    let text = msg
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| String::from("<non-string panic>"));
    assert!(
        text.contains("stub-target") && text.contains("mismatched types"),
        "panic must name the target and quote stderr, got: {text}"
    );
}

#[test]
fn capability_stub_skips_through_the_call_site_wrapper() {
    let stub = stub_compiler("mindc_no_cap", 1, CAP_FEATURE.trim_end());
    let out = run_stub(&stub);
    assert!(
        !gate::compiled_with("stub-target", &out, false),
        "a genuine capability gap must still skip"
    );
}

#[test]
fn capability_stub_panics_under_enforcement() {
    let stub = stub_compiler("mindc_no_cap_req", 1, CAP_FEATURE.trim_end());
    let out = run_stub(&stub);
    let r = std::panic::catch_unwind(|| gate::compiled_with("stub-target", &out, true));
    assert!(
        r.is_err(),
        "MIND_BENCH_REQUIRE=1 must turn a capability skip into a hard failure"
    );
}

#[test]
fn working_compiler_reports_compiled() {
    let stub = stub_compiler("mindc_ok", 0, "");
    let out = run_stub(&stub);
    assert!(gate::compiled_with("stub-target", &out, false));
    assert!(gate::compiled_with("stub-target", &out, true));
}

// --- the capability-probe marker (which::which / mlir_available sites) ------

#[test]
fn probe_skip_panics_under_enforcement_and_marks_otherwise() {
    let r = std::panic::catch_unwind(|| gate::skipped_with("probe-target", "no mlir-opt", true));
    assert!(
        r.is_err(),
        "MIND_BENCH_REQUIRE=1 must forbid a capability probe skip"
    );
    // Not enforcing: returns normally and emits the `ran=0` marker the tier
    // gate's SKIP-MARKER CONSUMER already treats as fatal-unless-tolerated.
    gate::skipped_with("probe-target", "no mlir-opt", false);
}

// --- the source-shape ratchet ----------------------------------------------
//
// The helper above is worth exactly as many sites as route through it. 24 sites
// of the fail-OPEN compile shape were converted with this slice; a wider class
// of 256 announce a skip and `return` without ever consulting
// `MIND_BENCH_REQUIRE`, so `MIND_BENCH_REQUIRE=1` cannot make them hard-fail.
// Hand-editing 256 sites is not the deliverable — the helper is — so the count
// is put under a MECHANICAL two-sided ratchet instead: it may never rise, and
// when it falls this gate demands the baseline be lowered, so the number can
// never quietly stop meaning what it says.
//
// deferred: drive OPEN_SKIP_SITE_BASELINE to 0 by routing each remaining site
// through `common::gate::{compiled, skipped}` as its file is next touched.
// Upgrade path and owner: this ratchet fails the moment the count regresses, so
// no NEW fail-open site can be added while the backlog drains.

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

/// Measured on this tree at the commit that introduced the ratchet.
/// MAY ONLY EVER SHRINK.
const OPEN_SKIP_SITE_BASELINE: usize = 256;

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
fn is_skip_announcement(lines: &[&str], i: usize, end: usize) -> bool {
    lines[i..=end]
        .join("\n")
        .to_ascii_lowercase()
        .contains("skip")
}

/// This scanner's OWN source. It is excluded from the scan because its positive
/// control must quote the very shape it forbids; a scanner that flagged its own
/// specimen jar would be unable to prove it can still see the bad shape.
const SCANNER_SELF: &str = "fail_closed_capability_skip.rs";

/// Every `tests/**/*.rs` source except this scanner, with its path, read once.
fn test_sources() -> Vec<(PathBuf, String)> {
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
            let s = std::fs::read_to_string(&p).expect("read test source");
            (p, s)
        })
        .collect()
}

/// Sites that announce a skip, return, and never consult the fail-closed helper.
fn open_skip_sites() -> Vec<String> {
    let mut open = Vec::new();
    for (path, text) in test_sources() {
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
            open.push(format!(
                "{}:{}",
                path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                i + 1
            ));
        }
    }
    open
}

#[test]
fn no_new_fail_open_skip_site_may_be_added() {
    let open = open_skip_sites();
    let n = open.len();
    assert!(
        n <= OPEN_SKIP_SITE_BASELINE,
        "{} unrouted skip-and-return sites, baseline {}. A skip that never \
         consults MIND_BENCH_REQUIRE cannot be turned into a hard failure, so \
         the tier can pass vacuously. Route the new site through \
         common::gate::{{compiled, skipped}}.\nnew or unrouted:\n  {}",
        n,
        OPEN_SKIP_SITE_BASELINE,
        open.join("\n  ")
    );
    assert!(
        n >= OPEN_SKIP_SITE_BASELINE,
        "the backlog shrank to {n} (baseline {OPEN_SKIP_SITE_BASELINE}); lower \
         OPEN_SKIP_SITE_BASELINE to {n} so the ratchet keeps its meaning."
    );
}

/// The exact fail-OPEN string the finding named, ASSEMBLED at run time.
///
/// Spelling it as one literal would make this scanner a hit on its own scan and
/// on the shell gate that greps the same text, so the specimen is built from
/// halves. It is still one definition, used by both the assertion and the
/// scanner's positive control.
fn banned_literal() -> String {
    format!("{}{}", "compile failed", "; skipping")
}

#[test]
fn the_converted_compile_sites_are_gone() {
    // The exact fail-OPEN string the finding named. Zero is the whole point.
    let hits: Vec<String> = test_sources()
        .into_iter()
        .filter(|(_, t)| t.contains(&banned_literal()))
        .map(|(p, _)| p.display().to_string())
        .collect();
    assert!(hits.is_empty(), "fail-open compile skips remain: {hits:?}");
}

#[test]
fn the_scanner_itself_can_see_the_bad_shape() {
    // Positive control: a scanner that matches nothing would pass silently.
    fn one_owned(l: &str) -> bool {
        let lines = vec![l];
        print_macro_span(&lines, 0).is_some_and(|e| is_skip_announcement(&lines, 0, e))
    }
    let one = one_owned;
    let specimen = format!("    println!(\"arena: mindc {}\");", banned_literal());
    assert!(one_owned(&specimen));
    assert!(one(
        r#"        eprintln!("SKIP: MLIR tools not available");"#
    ));
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
