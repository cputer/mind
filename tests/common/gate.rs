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
//! # The `ran=0` marker reaches the tier log unconditionally
//!
//! A skip is only observable if its marker survives to the log
//! `scripts/exec_semantics_gate.sh` greps. `println!` does not survive:
//! libtest swaps the sink `std::io::_print` writes to and DISCARDS a PASSING
//! test's stdout — and a capability skip passes — so the marker was thrown
//! away in exactly the case it exists for. Measured on this host, one target,
//! one skip:
//!
//! `--test std_mlir_bindings_smoke` under `std-surface,mlir-lowering`, whose
//! `mlir_capi_symbols_present_in_static_libs` takes an optional-input skip
//! here: default capture printed `ok. 4 passed` and ZERO `SDLC-GATE` lines;
//! `-- --show-output` printed the same 4 passed and ONE. So `skipped_optional`
//! graded as an invisible PASS — the tier could not tell "asserted nothing"
//! from "asserted and agreed". The sink is now the PROCESS stdout handle,
//! which libtest does not shim.
//!
//! Making the tier runner pass `--show-output` instead was measured and
//! REJECTED: it splices every passing test's captured stdout into the log the
//! tier counts `^test result:` lines in, and this repo has tests that print a
//! subprocess `mindc test` summary beginning with exactly that prefix. One run
//! each way, same tree: `lowering` 335 harnesses / 1891 executed -> 340 / 1900,
//! `pkg` 334 / 1474 -> 339 / 1483 — +5 phantom harnesses and +9 phantom
//! executed tests each, one of them a phantom FAILING test. The flag corrupts
//! the very counters the gate rests on; the producer-side sink costs them
//! nothing.
//!
//! Every marker carries its [`Absent`] class (`class=toolchain` /
//! `class=optional`), so the tier script judges it against its own
//! `REQUIRE_TOOLCHAIN_<tier>` contract rather than a hand-copied list of target
//! names that drifts the moment a gate is added.

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
    compiled_with_to(&mut default_sink(), target, out, enforce)
}

/// As [`compiled_with`], with the marker SINK supplied explicitly.
///
/// The gate's own tests drive this path with a synthetic target name, and a
/// synthetic `ran=0` printed to the harness stdout is indistinguishable in the
/// tier log from a real gate that did not run — a self-test manufacturing the
/// exact evidence the tier consumer treats as fatal. Injecting the sink lets
/// those tests ASSERT the marker's bytes instead of leaking them.
#[allow(dead_code)]
pub fn compiled_with_to<W: std::io::Write>(
    w: &mut W,
    target: &str,
    out: &Output,
    enforce: bool,
) -> bool {
    let stderr = String::from_utf8_lossy(&out.stderr);
    match classify(out.status.success(), &stderr, enforce) {
        Outcome::Compiled => true,
        Outcome::CapabilitySkip => {
            skip_marker_to(
                w,
                target,
                "mlir-build capability unavailable",
                Absent::Toolchain,
            );
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
    skipped_because_to(&mut default_sink(), target, reason, absent, enforce);
}

/// As [`skipped_because`], with the marker SINK supplied explicitly.
///
/// Same reason as [`compiled_with_to`]: the gate's own tests must be able to
/// exercise the marker without printing a `ran=0` for a target that does not
/// exist into the tier log they are testing.
#[allow(dead_code)]
pub fn skipped_because_to<W: std::io::Write>(
    w: &mut W,
    target: &str,
    reason: &str,
    absent: Absent,
    enforce: bool,
) {
    forbid_skip_if_enforced(target, reason, absent, enforce);
    skip_marker_to(w, target, reason, absent);
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

/// The [`Absent`] class, spelled INTO the marker.
///
/// The tier script has to decide whether a `ran=0` is fatal, and that decision
/// is [`is_fail_closed`] crossed with the tier's own `REQUIRE_TOOLCHAIN_<tier>`
/// knob. Without the class it could only guess — from the reason PROSE, or from
/// a hand-maintained list of target names, which is the drift this repo already
/// pays for elsewhere. The class travels with the marker, so the rule keeps ONE
/// owner and the script keeps no second list.
fn class_token(absent: Absent) -> &'static str {
    match absent {
        Absent::Toolchain => "toolchain",
        Absent::OptionalInput => "optional",
    }
}

/// The marker `scripts/exec_semantics_gate.sh` consumes: a gate that did not run
/// reports `ran=0`. ONE definition of the text, so producer and consumer can
/// never drift into two dialects the consumer's regex reads differently.
fn marker_line(target: &str, reason: &str, absent: Absent) -> String {
    format!(
        "SDLC-GATE {target} ran=0 fail=0 class={}  (capability skip: {reason})",
        class_token(absent)
    )
}

/// Write the marker to `w`.
fn skip_marker_to<W: std::io::Write>(w: &mut W, target: &str, reason: &str, absent: Absent) {
    let _ = writeln!(w, "{}", marker_line(target, reason, absent));
    let _ = w.flush();
}

/// THE default marker sink: the PROCESS stdout handle.
///
/// Never `println!` — see the module header. `std::io::stdout()` is the real
/// handle rather than the per-test sink libtest swaps in, so the marker reaches
/// the log a plain `cargo test` writes, under libtest's default capture.
/// Named once here so the two public wrappers cannot disagree about it, and so
/// there is exactly one line to revert when proving the guarantee:
/// `tests/fail_closed_capability_skip_env.rs::the_skip_marker_survives_libtest_capture`
/// spawns a child WITHOUT `--nocapture` and fails if this becomes `println!`.
fn default_sink() -> std::io::Stdout {
    std::io::stdout()
}
