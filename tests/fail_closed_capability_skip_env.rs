// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! The ENVIRONMENT half of the fail-CLOSED capability-skip contract.
//!
//! # The defect this gate defends against
//!
//! `tests/fail_closed_capability_skip.rs` proves the DECISION
//! (`gate::classify`, `gate::skipped_because`) with the enforcement flag
//! supplied explicitly — `compiled_with(.., enforce)`, `skipped_with(..,
//! enforce)` — which is the right shape for a pure contract and is why that
//! file needs no process-global state.
//!
//! It leaves the flag's READER unpinned. Measured on the tip: mutating
//! `gate::enforce_real_backend` to `false` — deleting the entire
//! `MIND_BENCH_REQUIRE=1` guarantee the `exec` tier of
//! `scripts/exec_semantics_gate.sh` rests on — left that suite at
//! `22 passed; 0 failed`, and mutating `gate::is_fail_closed` to `true` (which
//! hard-fails the opt-in-input class the tier deliberately tolerates) left it
//! at 22 passed plus a green ratchet. The env-reading wrappers
//! `gate::skipped`, `gate::skipped_optional` and `gate::compiled` had no test
//! at all; the only evidence for them was a hand-run of the tier.
//!
//! # Why a child PROCESS
//!
//! `MIND_BENCH_REQUIRE` is process-global. `std::env::set_var` is `unsafe` in
//! edition 2024 precisely because it races every other test in the same
//! binary, so the variable cannot be honestly varied from inside a
//! multi-threaded harness. Each test below therefore spawns THIS test binary,
//! selecting exactly one test and naming the child's job in [`ROLE_VAR`].
//!
//! Every child's `MIND_BENCH_REQUIRE` is set or REMOVED explicitly, never
//! inherited, so these tests read the same under
//! `scripts/exec_semantics_gate.sh exec` — which exports `MIND_BENCH_REQUIRE=1`
//! for the whole run — as under a bare `cargo test`.
//!
//! # Why a separate FILE
//!
//! Not portability: everything here runs on all four rows of `ci.yml`'s
//! `build_test` matrix (no shell stub, no unix-only spawn), unlike
//! `tests/fail_closed_capability_skip_stub_exec.rs`. It is the same reason
//! `tests/capability_refusal_cause_scan.rs` was split off — a different
//! subject with its own machinery, and folding it in put the sibling at 810
//! lines, past this repo's 800-line ceiling.

mod common;

use common::gate;
use std::process::{Command, Output};

/// Verbatim stderr shape of the `mlir-opt`/`clang`-absent refusal
/// (`MlirBuildError::ToolMissing`, `src/eval/mlir_build.rs`), built from the
/// compiler's OWN cause-code constant.
///
/// A second hand-copied fixture is what this family already refuses
/// (`fail_closed_capability_skip_stub_exec.rs` builds its stderr the same way):
/// a fixture that spells `E5004` itself keeps asserting about a wire shape the
/// compiler may no longer emit.
fn cap_tool_stderr() -> String {
    format!(
        "error[build]: [{}] tool not found: mlir-opt\n",
        libmind::diagnostics::capability::NATIVE_TOOLCHAIN_ABSENT
    )
}

/// Names the child's job on the wire. Set only by [`run_self`]; its absence is
/// how a test knows it is the parent.
const ROLE_VAR: &str = "MIND_GATE_ENV_CHILD_ROLE";

/// The target name every child gate call reports under.
const CHILD_TARGET: &str = "env-child";

/// The filter the [`Role::EmitCapabilityStderr`] grandchild is selected with.
/// Any test in this file would do — the ROLE decides what runs — but the name
/// must EXIST, and [`run_self`] proves it did.
const COMPILE_SITE_TEST: &str = "the_environment_alone_forbids_a_compile_site_skip";

/// What a child does. Parsed at the process boundary: an unknown value is a
/// harness defect and says so, rather than quietly running nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// `gate::skipped` — the toolchain class, read from the environment.
    ToolchainSkip,
    /// `gate::compiled` over a real spawned `Output` whose stderr carries the
    /// toolchain-absent cause.
    CompileSite,
    /// `gate::skipped_optional` — the opt-in-input class, read from the
    /// environment.
    OptionalSkip,
    /// `gate::skipped_visibly` — the toolchain class whose `ran=0` marker must
    /// reach the tier log under libtest's DEFAULT stdout capture.
    VisibleToolchainSkip,
    /// Not a gate call: print the capability stderr and exit 1, so
    /// [`Role::CompileSite`] classifies an `Output` an actual spawn produced
    /// rather than one manufactured by hand.
    EmitCapabilityStderr,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::ToolchainSkip => "toolchain-skip",
            Role::CompileSite => "compile-site",
            Role::OptionalSkip => "optional-skip",
            Role::VisibleToolchainSkip => "visible-toolchain-skip",
            Role::EmitCapabilityStderr => "emit-capability-stderr",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "toolchain-skip" => Role::ToolchainSkip,
            "compile-site" => Role::CompileSite,
            "optional-skip" => Role::OptionalSkip,
            "visible-toolchain-skip" => Role::VisibleToolchainSkip,
            "emit-capability-stderr" => Role::EmitCapabilityStderr,
            other => panic!("{ROLE_VAR}={other:?} names no child role"),
        }
    }
}

/// The role this process was spawned to play, or `None` in the parent.
fn child_role() -> Option<Role> {
    std::env::var(ROLE_VAR).ok().map(|v| Role::parse(&v))
}

/// The `ran=0` marker `gate`'s `skip_marker` emits for [`CHILD_TARGET`].
fn marker() -> String {
    format!("SDLC-GATE {CHILD_TARGET} ran=0 fail=0")
}

/// Everything a child does. No `enforce` argument appears anywhere below: every
/// call is an environment-READING wrapper, which is the whole subject.
fn run_as_child(role: Role) {
    match role {
        Role::ToolchainSkip => gate::skipped(CHILD_TARGET, "no mlir-opt on PATH"),
        Role::CompileSite => {
            let out = run_self(COMPILE_SITE_TEST, Role::EmitCapabilityStderr, None);
            // Positive control: a verdict about this `Output` means nothing
            // unless the spawn really failed and really carried the cause code.
            assert!(
                !out.status.success(),
                "the capability stub exited 0; there is no failure to classify"
            );
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains(libmind::diagnostics::capability::NATIVE_TOOLCHAIN_ABSENT),
                "the capability stub printed no cause code: {stderr:?}"
            );
            assert!(
                !gate::compiled(CHILD_TARGET, &out),
                "an unenforced run must still tolerate a genuine toolchain gap"
            );
        }
        // OPTIONAL INPUT: the reason stands for an opt-in corpus the product
        // does not ship and CI never installs; the developer who wants the rung
        // supplies it. `MIND_BENCH_REQUIRE` says "use a real backend", not
        // "install everything", so this class must survive enforcement.
        Role::OptionalSkip => gate::skipped_optional(CHILD_TARGET, "opt-in corpus not present"),
        Role::VisibleToolchainSkip => gate::skipped_visibly(CHILD_TARGET, "no mlir-opt on PATH"),
        Role::EmitCapabilityStderr => {
            eprint!("{}", cap_tool_stderr());
            std::process::exit(1);
        }
    }
}

/// Run ONE of this binary's own tests in a child process, with a chosen
/// `MIND_BENCH_REQUIRE` setting.
///
/// `require = None` REMOVES the variable, so a tier that exports it cannot leak
/// into the cases that must observe its absence.
fn run_self(test_name: &str, role: Role, require: Option<&str>) -> Output {
    // `--nocapture` so the child's `ran=0` marker and panic text reach the pipe:
    // libtest swallows the stdout of a PASSING test, which is the very
    // invisibility the marker exists to defeat.
    run_self_inner(test_name, role, require, true)
}

/// As [`run_self`], but the child runs under libtest's DEFAULT stdout capture —
/// the way `scripts/exec_semantics_gate.sh` actually runs the suite.
///
/// Every other spawn here passes `--nocapture`, which is precisely the condition
/// under which the marker cannot be swallowed, so none of them can observe
/// whether it survives a normal run.
fn run_self_captured(test_name: &str, role: Role, require: Option<&str>) -> Output {
    run_self_inner(test_name, role, require, false)
}

fn run_self_inner(test_name: &str, role: Role, require: Option<&str>, nocapture: bool) -> Output {
    let exe = std::env::current_exe().expect("current_exe");
    let mut cmd = Command::new(exe);
    cmd.args([test_name, "--exact", "--test-threads", "1"])
        .env(ROLE_VAR, role.as_str())
        .env_remove(gate::REQUIRE_VAR);
    if nocapture {
        cmd.arg("--nocapture");
    }
    if let Some(v) = require {
        cmd.env(gate::REQUIRE_VAR, v);
    }
    let out = cmd.output().expect("spawn this test binary");
    // The ran=0 discipline, applied to this gate itself: `--exact` on a name
    // that no longer exists selects NOTHING and libtest still exits 0, so a
    // renamed test would turn every assertion below into a vacuous pass.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("running 1 test"),
        "the child selected no test: `{test_name}` matched nothing (renamed?), \
         so this gate would be asserting about a process that ran nothing.\n{stdout}"
    );
    out
}

/// A child's stdout and stderr joined. Which stream carries a panic is
/// libtest's business, not this contract's.
fn child_text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn the_environment_alone_forbids_a_probe_skip() {
    let Some(role) = child_role() else {
        let out = run_self(
            "the_environment_alone_forbids_a_probe_skip",
            Role::ToolchainSkip,
            Some("1"),
        );
        let text = child_text(&out);
        assert!(
            !out.status.success(),
            "MIND_BENCH_REQUIRE=1 in the ENVIRONMENT must forbid a toolchain \
             skip; the child returned 0 and asserted nothing.\n{text}"
        );
        assert!(
            text.contains(CHILD_TARGET) && text.contains("forbids a toolchain skip"),
            "the child died for some reason other than the fail-closed rule:\n{text}"
        );
        assert!(
            !text.contains(&marker()),
            "a forbidden skip must not also emit the ran=0 marker:\n{text}"
        );
        return;
    };
    run_as_child(role);
}

#[test]
fn the_environment_alone_forbids_a_compile_site_skip() {
    let Some(role) = child_role() else {
        let out = run_self(COMPILE_SITE_TEST, Role::CompileSite, Some("1"));
        let text = child_text(&out);
        assert!(
            !out.status.success(),
            "MIND_BENCH_REQUIRE=1 in the ENVIRONMENT must forbid a capability \
             skip at a COMPILE site too; the child returned 0.\n{text}"
        );
        // The two causes need DIFFERENT instructions. Sending a host that
        // merely lacks the toolchain off to hunt a compiler regression is the
        // defect `gate::compiled_with` splits its message for.
        assert!(
            text.contains("lacks the native backend"),
            "the enforced compile site must print the TOOLCHAIN instruction:\n{text}"
        );
        assert!(
            !text.contains("compiler regression"),
            "a toolchain gap must not be reported as a compiler regression:\n{text}"
        );
        return;
    };
    run_as_child(role);
}

#[test]
fn an_absent_flag_leaves_both_capability_skips_legal() {
    let Some(role) = child_role() else {
        // MUST NOT CHANGE: a host genuinely lacking the toolchain keeps
        // skipping — and says so with the countable marker — when nothing in
        // the environment demanded a real backend.
        for role in [Role::ToolchainSkip, Role::CompileSite] {
            let out = run_self(
                "an_absent_flag_leaves_both_capability_skips_legal",
                role,
                None,
            );
            let text = child_text(&out);
            assert!(
                out.status.success(),
                "with MIND_BENCH_REQUIRE unset, {role:?} must skip, not fail.\n{text}"
            );
            assert!(
                text.contains(&marker()),
                "a tolerated skip must still emit the ran=0 marker so it is \
                 counted rather than invisible ({role:?}).\n{text}"
            );
        }
        return;
    };
    run_as_child(role);
}

#[test]
fn only_the_exact_value_one_enforces() {
    let Some(role) = child_role() else {
        // `.is_ok()` here would let a value that READS as off switch
        // enforcement ON — the defect `gate::bless_mode` documents, one
        // predicate over. `0` and the empty value are the two measured shapes.
        for value in ["0", ""] {
            let out = run_self(
                "only_the_exact_value_one_enforces",
                Role::ToolchainSkip,
                Some(value),
            );
            let text = child_text(&out);
            assert!(
                out.status.success(),
                "MIND_BENCH_REQUIRE={value:?} does not demand a real backend, \
                 so it must not forbid the skip.\n{text}"
            );
            assert!(text.contains(&marker()), "no ran=0 marker:\n{text}");
        }
        return;
    };
    run_as_child(role);
}

#[test]
fn an_optional_input_skip_survives_the_enforced_environment() {
    let Some(role) = child_role() else {
        // The other half of the class decision, and the only outcome
        // MIND_BENCH_REQUIRE does NOT close: an input the gate never demanded
        // is reported `ran=0`, never hard-failed. Fail-closing on it would red
        // a tier for a package nobody required.
        let out = run_self(
            "an_optional_input_skip_survives_the_enforced_environment",
            Role::OptionalSkip,
            Some("1"),
        );
        let text = child_text(&out);
        assert!(
            out.status.success(),
            "MIND_BENCH_REQUIRE=1 says 'use a real backend', not 'install \
             everything': an OptionalInput absence must not hard-fail.\n{text}"
        );
        assert!(
            text.contains(&marker()),
            "an optional-input skip must still be COUNTED:\n{text}"
        );
        return;
    };
    run_as_child(role);
}

#[test]
fn the_skip_marker_survives_libtest_capture() {
    let Some(role) = child_role() else {
        // THE DEFECT THIS PINS: the marker was written with `println!`, whose
        // sink libtest swaps per test — a PASSING test's stdout is buffered and
        // thrown away unless the run asks for `--nocapture`. A capability skip
        // PASSES, so the marker was discarded in exactly the case it exists for,
        // while every test in this file spawned its child WITH `--nocapture` and
        // could not see it. Measured on the tip, same child, same role:
        //
        //   without --nocapture -> 0 occurrences of the marker
        //   with    --nocapture -> 1
        //
        // scripts/exec_semantics_gate.sh's SKIP-MARKER CONSUMER greps a plain
        // `cargo test` log. Evidence that exists only under a flag nobody passes
        // is documentation, not a gate: the tier could not tell "asserted
        // nothing" from a green run.
        let out = run_self_captured(
            "the_skip_marker_survives_libtest_capture",
            Role::VisibleToolchainSkip,
            None,
        );
        let text = child_text(&out);
        assert!(
            out.status.success(),
            "the child must SKIP and PASS, so the capture is the only thing \
             under test here:\n{text}"
        );
        assert!(
            text.contains(&marker()),
            "the `{}` marker did not survive libtest's stdout capture, so a \
             gate that asserted NOTHING reads as a green run in the tier log.\n{text}",
            marker()
        );
        return;
    };
    run_as_child(role);
}
