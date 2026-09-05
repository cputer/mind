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
//! # The backlog is drained
//!
//! Every skip-and-return site in `tests/**/*.rs` now routes through this
//! module. `tests/fail_open_skip_site_ratchet.rs` scans the tree and fails on any
//! site that does not, so the count this module governs can never silently
//! grow back.
//!
//! # Two honest classes of absence, ONE decision function
//!
//! `MIND_BENCH_REQUIRE=1` says "use a real backend", not "install everything".
//! Conflating those two claims is what would make the fail-closed guarantee
//! unusable: an opt-in corpus directory or a VNNI rung nobody asked for is not
//! a toolchain gap, and hard-failing on it would red a tier for a non-defect.
//! So the caller names WHICH absence it met ([`Absent`]) and the single
//! decision function [`skipped_because`] applies the rule. There is still
//! exactly one place the skip predicate is written.
//!
//! deferred: `skipped()` prints the `SDLC-GATE <target> ran=0 fail=0` marker
//! that `scripts/exec_semantics_gate.sh`'s SKIP-MARKER CONSUMER already reads,
//! but cargo captures the stdout of a PASSING test, so the marker only reaches
//! the tier log under `--nocapture`. The fail-closed guarantee therefore rests
//! on the `MIND_BENCH_REQUIRE=1` panic, not on the marker. Upgrade path: have
//! the tier runner pass `--nocapture`, or promote the marker to a harness-level
//! summary.

use std::process::Output;

/// The variable `scripts/exec_semantics_gate.sh` exports for the `exec` tier.
///
/// Spelled ONCE, here. Every reader below names this constant, and so does the
/// gate that varies it in a child process
/// (`tests/fail_closed_capability_skip.rs`) — a second hand-typed spelling is
/// exactly the drift that would leave the enforcement path untested while its
/// test looked green.
#[allow(dead_code)]
pub const REQUIRE_VAR: &str = "MIND_BENCH_REQUIRE";

/// True when the run demands a real backend and forbids every capability skip.
///
/// `scripts/exec_semantics_gate.sh` sets `MIND_BENCH_REQUIRE=1` for the `exec`
/// tier precisely so that tier cannot pass vacuously.
///
/// The value must be exactly `1`. `.is_ok()` here would make `=0` and an empty
/// value enforce — the mirror of the defect [`bless_mode`] documents, where
/// `.is_ok()` let a value that reads as "off" switch a mode ON.
#[allow(dead_code)]
pub fn enforce_real_backend() -> bool {
    std::env::var(REQUIRE_VAR)
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
        Outcome::Failed(s) => {
            // Both causes must fail, and they need DIFFERENT instructions: one
            // says "fix the compiler", the other says "install the toolchain or
            // drop MIND_BENCH_REQUIRE". Printing the regression sentence for a
            // host that simply has no backend sends the reader hunting a bug
            // that is not there.
            let why = if enforce && is_capability_gap(&s) {
                "MIND_BENCH_REQUIRE=1 forbids a toolchain skip, and this host \
                 lacks the native backend this gate needs. Install the \
                 toolchain or run without MIND_BENCH_REQUIRE — a skip here \
                 asserts NOTHING."
            } else {
                "this is a compiler regression, not a capability gap, and must \
                 never grade as a pass."
            };
            panic!(
                "{target}: mindc compile FAILED (exit {}) — {why}\nstderr:\n{}",
                out.status.code().unwrap_or(-1),
                if s.trim().is_empty() {
                    "<empty: the call site captured no stderr>"
                } else {
                    s.trim_end()
                }
            )
        }
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

#[allow(dead_code)]
/// Is this a BLESS run — the one mode in which every identity gate asserts
/// NOTHING and merely prints the hash it computed?
///
/// Three defects lived in the `bless_mode()` this
/// replaced, copied at 13 sites:
///
/// * `.is_ok()` is true for ANY value. `MIND_BENCH_BLESS=0` and
///   `MIND_BENCH_BLESS=` both disabled all 26 canaries while the suite still
///   printed `26 passed` — a value that reads as "off" turning the wedge's
///   merge gate into a no-op.
/// * Nothing forbade BLESS and `MIND_BENCH_REQUIRE` being set together, so a
///   stray job-scope export could void a tier that had explicitly demanded a
///   real, asserting run. That combination is now a hard failure: the two flags
///   make contradictory claims and the fail-closed one wins.
/// * A bless log was indistinguishable from a green gate. It now opens with one
///   unmissable banner, printed once per process.
pub fn bless_mode() -> bool {
    let on = matches!(std::env::var("MIND_BENCH_BLESS").as_deref(), Ok("1"));
    if !on {
        return false;
    }
    assert!(
        std::env::var_os(REQUIRE_VAR).is_none(),
        "MIND_BENCH_BLESS=1 and MIND_BENCH_REQUIRE are both set. BLESS mode \
         asserts NOTHING — it prints computed hashes — so a run that demanded a \
         real, asserting backend cannot also be a bless run. Unset one."
    );
    static BANNER: std::sync::Once = std::sync::Once::new();
    BANNER.call_once(|| {
        println!(
            "GATE MODE: BLESS — every cross-substrate identity gate below \
             asserts NOTHING and only prints its computed hash. This log is NOT \
             evidence of a green gate."
        );
    });
    true
}

/// WHICH absence a gate met. The class, not the call site, decides whether
/// `MIND_BENCH_REQUIRE=1` forbids the skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Absent {
    /// The COMPILER's own capability: the `mindc` binary, its native backend,
    /// or the MLIR/clang tools it shells out to. The exec tier's
    /// `MIND_BENCH_REQUIRE` contract is written about exactly this, so it FAILS
    /// CLOSED — a tier that demands a real backend may not report a green run
    /// it never performed.
    Toolchain,
    /// An input the gate never demanded: an opt-in corpus directory, opt-in
    /// silicon, an optional local dev artifact the product does not ship.
    /// Reported as `ran=0` so it is counted, never a hard failure — measured
    /// case: the LLVM/MLIR C-API archives the binding smokes probe for are not
    /// installed by the `mlir-20-tools` package CI pins, so fail-closing on
    /// them would red a tier for a package that was never required.
    OptionalInput,
}

/// Does this absence forbid a skip under `MIND_BENCH_REQUIRE=1`?
#[allow(dead_code)]
pub fn is_fail_closed(absent: Absent) -> bool {
    matches!(absent, Absent::Toolchain)
}

/// The fail-closed half of the skip decision, factored out so that EVERY sink
/// below refuses on the same rule and dies with the same sentence.
fn forbid_skip_if_enforced(target: &str, reason: &str, absent: Absent, enforce: bool) {
    if enforce && is_fail_closed(absent) {
        panic!(
            "{target}: MIND_BENCH_REQUIRE=1 forbids a toolchain skip, but this \
             gate tried to skip: {reason}. Install the toolchain or run without \
             MIND_BENCH_REQUIRE — a skip here asserts NOTHING."
        );
    }
}

/// THE skip decision, with the enforcement flag supplied explicitly (tests).
///
/// Panics when the run demands a real backend and the absence is a toolchain
/// gap; otherwise emits the `ran=0` marker so the skip is a countable event
/// rather than an invisible pass.
#[allow(dead_code)]
pub fn skipped_because(target: &str, reason: &str, absent: Absent, enforce: bool) {
    forbid_skip_if_enforced(target, reason, absent, enforce);
    skip_marker(target, reason);
}

/// A toolchain-capability skip, with the enforcement flag supplied explicitly
/// (tests).
#[allow(dead_code)]
pub fn skipped_with(target: &str, reason: &str, enforce: bool) {
    skipped_because(target, reason, Absent::Toolchain, enforce);
}

/// A toolchain-capability PROBE skip (`which::which(...)`, `mlir_available()`,
/// a missing `mindc`) reading `MIND_BENCH_REQUIRE` from the environment.
///
/// This is the default: a gate that cannot reach the compiler's own backend has
/// not run, and a tier that demanded one must hear about it.
#[allow(dead_code)]
pub fn skipped(target: &str, reason: &str) {
    skipped_because(target, reason, Absent::Toolchain, enforce_real_backend());
}

/// A skip for an input the gate never demanded — opt-in corpus, opt-in silicon,
/// an optional local artifact. Counted (`ran=0`), never fail-closed.
///
/// Every call site is greppable (`grep -rn 'gate::skipped_optional' tests/`) and
/// must carry a one-line comment naming what is optional and who supplies it.
#[allow(dead_code)]
pub fn skipped_optional(target: &str, reason: &str) {
    skipped_because(
        target,
        reason,
        Absent::OptionalInput,
        enforce_real_backend(),
    );
}

/// A toolchain-capability skip whose `ran=0` marker must reach the tier log even
/// under libtest's DEFAULT stdout capture.
///
/// Same predicate, same refusal, same marker text as [`skipped`] — only the SINK
/// differs. `println!` goes through `std::io::_print`, whose sink libtest swaps
/// per test: a PASSING test's stdout is buffered and discarded unless the run
/// asks for `--nocapture`, and a capability skip PASSES. Measured, same child,
/// same skip: 0 occurrences of the marker without `--nocapture`, 1 with. Writing
/// to the process stdout handle bypasses that shim.
///
/// deferred: [`skip_marker`] itself still uses `println!`, so the ~135 targets
/// that route a toolchain skip through [`skipped`] stay invisible under capture.
/// That is NOT an oversight to fix here: those targets carry no
/// `required-features`, so they build and skip in the `lowering` and `pkg` tiers
/// of `scripts/exec_semantics_gate.sh` (neither exports `MIND_BENCH_REQUIRE`),
/// and making every one of them visible at once would hand that script ~130
/// fatal `ran=0` markers and red two tiers that are green today. Upgrade path
/// (unchanged, and owned by the tier runner, not by this helper): have the
/// runner pass `--nocapture`, or promote the marker to a harness-level summary —
/// then this function collapses into [`skipped`] and should be deleted.
#[allow(dead_code)]
pub fn skipped_visibly(target: &str, reason: &str) {
    forbid_skip_if_enforced(target, reason, Absent::Toolchain, enforce_real_backend());
    use std::io::Write as _;
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}", marker_line(target, reason));
    let _ = out.flush();
}

/// The marker `scripts/exec_semantics_gate.sh` already consumes: a gate that did
/// not run reports `ran=0`, which is fatal unless the target is named in
/// `ENV_TOLERATED_<tier>`. ONE definition of the text, so the two sinks below can
/// never drift into two dialects the consumer's regex reads differently.
fn marker_line(target: &str, reason: &str) -> String {
    format!("SDLC-GATE {target} ran=0 fail=0  (capability skip: {reason})")
}

fn skip_marker(target: &str, reason: &str) {
    println!("{}", marker_line(target, reason));
}
