// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! The single fail-CLOSED capability gate for the integration-test harness.
//!
//! # Why this module exists
//!
//! Two failure modes were measured across `tests/*.rs`:
//!
//! 1. **Fail-open compile skips (24 sites, 18 files).** The shape
//!    `if !status.success() { println!("... build failed -> skipped"); return; }`
//!    has no panic and no `MIND_BENCH_REQUIRE` consultation, so on a runner
//!    that installs and PATH-verifies the MLIR toolchain the branch can only
//!    fire on a REAL compiler regression — and that regression then graded as
//!    a PASS. A determinism gate, the std-surface runtime-value gates and the
//!    build/cache gates were all fail-open in exactly this way.
//!
//! 2. **Invisible capability probes.** A `println!("skipping")` inside a
//!    passing test is swallowed by cargo's output capture, so a skip is not
//!    merely tolerated, it is *unobservable* — the tier gate cannot tell
//!    "asserted" from "executed".
//!
//! The fix is one decision function, not 24 hand-copied conditionals. A skip
//! is legal ONLY when the stderr carries a genuine capability signature AND
//! the run does not demand a real backend; every other failure panics with
//! the stderr quoted.
//!
//! # What is deliberately preserved
//!
//! A host genuinely lacking `mlir-build` (or the `mlir-opt`/`clang` binaries)
//! still skips. That behaviour is load-bearing for the non-exec tiers and is
//! asserted by `tests/fail_closed_capability_skip.rs`.
//!
//! deferred: 256 further skip-and-return sites still open-code their own
//! `println!("... skipping"); return;`, so `MIND_BENCH_REQUIRE=1` cannot make
//! them hard-fail. They are not hand-edited here — this module is the
//! deliverable, and the backlog is held under a MECHANICAL two-sided ratchet
//! (the set-valued backlog ratchet in
//! `tests/fail_open_skip_site_ratchet.rs`) that forbids a NEW one and demands
//! the frozen per-file entry be lowered whenever a file drains. Upgrade path:
//! route each remaining site through `compiled()` / `skipped()` as its file is
//! next touched, until the backlog is empty.
//!
//! deferred: `skipped()` prints the `SDLC-GATE <target> ran=0 fail=0` marker
//! that `scripts/exec_semantics_gate.sh`'s SKIP-MARKER CONSUMER already reads,
//! but cargo captures the stdout of a PASSING test, so the marker only reaches
//! the tier log under `--nocapture`. The fail-closed guarantee therefore rests
//! on the `MIND_BENCH_REQUIRE=1` panic, not on the marker. Upgrade path: have
//! the tier runner pass `--nocapture`, or promote the marker to a harness-level
//! summary.

use std::process::Output;

/// True when the run demands a real backend and forbids every capability skip.
///
/// `scripts/exec_semantics_gate.sh` sets `MIND_BENCH_REQUIRE=1` for the `exec`
/// tier precisely so that tier cannot pass vacuously.
#[allow(dead_code)]
pub fn enforce_real_backend() -> bool {
    std::env::var("MIND_BENCH_REQUIRE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// The verdict for one compile step.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Outcome {
    /// The compile succeeded; the test must go on to assert.
    Compiled,
    /// The host genuinely lacks the capability and the run tolerates it.
    CapabilitySkip,
    /// Anything else. Carries the stderr so the panic can quote it.
    Failed(String),
}

/// Does `stderr` carry a genuine capability signature?
///
/// Delegates to the compiler's own classifier
/// (`libmind::diagnostics::capability`), which matches the stable diagnostic
/// CODE of the refusal cause (`[E5003]` no native backend in this binary,
/// `[E5004]` no `mlir-opt`/`clang` on PATH) — never its prose.
///
/// This used to be three hand-copied substring tests. They were wrong in both
/// directions: they could not see the `mindc build` project route's refusal
/// (`entry module was not natively compiled …`), so a host without
/// `mlir-build` graded a genuine capability gap as a compiler regression and
/// hard-failed; and any re-worded diagnostic could silently widen or close the
/// hole. The code is the contract; prose is not.
///
/// A bare "error" is NOT a capability gap: an undiagnosed failure fails closed,
/// and so does a stderr that carries a real cause code ALONGSIDE a capability
/// one (a workspace build prints one refusal per member and keeps going).
#[allow(dead_code)]
pub fn is_capability_gap(stderr: &str) -> bool {
    libmind::diagnostics::capability::is_capability_gap(stderr)
}

/// The pure decision core: no environment, no I/O, so the contract is directly
/// testable without mutating process-global state.
#[allow(dead_code)]
pub fn classify(ok: bool, stderr: &str, enforce: bool) -> Outcome {
    if ok {
        return Outcome::Compiled;
    }
    if !enforce && is_capability_gap(stderr) {
        return Outcome::CapabilitySkip;
    }
    Outcome::Failed(stderr.to_string())
}

/// Call-site wrapper with the enforcement flag supplied explicitly (tests).
///
/// Returns `true` when the step compiled, `false` ONLY for a tolerated
/// capability gap (the caller then `return`s), and panics on every other
/// failure with the target name and the captured stderr.
#[allow(dead_code)]
pub fn compiled_with(target: &str, out: &Output, enforce: bool) -> bool {
    let stderr = String::from_utf8_lossy(&out.stderr);
    match classify(out.status.success(), &stderr, enforce) {
        Outcome::Compiled => true,
        Outcome::CapabilitySkip => {
            skip_marker(target, "mlir-build capability unavailable");
            false
        }
        Outcome::Failed(s) => panic!(
            "{target}: mindc compile FAILED (exit {}) — this is a compiler \
             regression, not a capability gap, and must never grade as a \
             pass.\nstderr:\n{}",
            out.status.code().unwrap_or(-1),
            if s.trim().is_empty() {
                "<empty: the call site captured no stderr>"
            } else {
                s.trim_end()
            }
        ),
    }
}

/// Call-site wrapper reading `MIND_BENCH_REQUIRE` from the environment.
///
/// Replaces the fail-open `if !status.success() { println!("skipping"); return }`
/// shape. Use as: `if !gate::compiled("io_canon", &out) { return; }`.
#[allow(dead_code)]
pub fn compiled(target: &str, out: &Output) -> bool {
    compiled_with(target, out, enforce_real_backend())
}

/// A capability PROBE skip (`which::which(...)`, `mlir_available()`), with the
/// enforcement flag supplied explicitly (tests).
///
/// Panics under enforcement; otherwise emits the `ran=0` marker so the skip is
/// a countable event rather than an invisible pass.
#[allow(dead_code)]
pub fn skipped_with(target: &str, reason: &str, enforce: bool) {
    if enforce {
        panic!(
            "{target}: MIND_BENCH_REQUIRE=1 forbids a capability skip, but this \
             gate tried to skip: {reason}. Install the toolchain or run without \
             MIND_BENCH_REQUIRE — a skip here asserts NOTHING."
        );
    }
    skip_marker(target, reason);
}

/// A capability PROBE skip reading `MIND_BENCH_REQUIRE` from the environment.
#[allow(dead_code)]
pub fn skipped(target: &str, reason: &str) {
    skipped_with(target, reason, enforce_real_backend());
}

/// Emit the marker `scripts/exec_semantics_gate.sh` already consumes: a gate
/// that did not run reports `ran=0`, which is fatal unless the target is named
/// in `ENV_TOLERATED_<tier>`.
fn skip_marker(target: &str, reason: &str) {
    println!("SDLC-GATE {target} ran=0 fail=0  (capability skip: {reason})");
}
