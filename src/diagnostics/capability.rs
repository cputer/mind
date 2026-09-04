// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Stable diagnostic codes for CAPABILITY refusals, and the one classifier that
//! separates "this host cannot do it" from "this compiler is broken".
//!
//! # Why this module exists
//!
//! Two refusals look identical from the outside — a non-zero exit and a
//! diagnostic on stderr — but mean opposite things:
//!
//! * **Capability gap.** The binary was built without a native backend, or the
//!   backend's tools are not installed. Nothing is wrong with the compiler or
//!   the program; a gate that demands a real backend must fail, every other
//!   caller may skip.
//! * **Real failure.** The program did not compile, or the compiler regressed.
//!   This must NEVER grade as a skip.
//!
//! Before this module the distinction was carried only by the *prose* of the
//! diagnostic, matched with substring tests copied into the test harness
//! (`tests/common/gate.rs`). That is drift by construction: re-wording a
//! diagnostic silently re-opened or re-closed the hole, and a refusal whose
//! wording was never copied — the `mindc build` project route's
//! "entry module was not natively compiled …" — was mis-graded as a compiler
//! regression and hard-failed on every host without `mlir-build`.
//!
//! The fix is a stable CODE per refusal cause, owned here, emitted by the
//! compiler, and matched here. A re-worded diagnostic keeps its code; a new
//! refusal without a code fails CLOSED (it is not a capability gap).
//!
//! # Wire shape
//!
//! A coded refusal carries its code as the bracketed token `[E5003]`, either in
//! the diagnostic prefix (`error[build][E5003]: …`, the `DiagnosticEmitter`
//! shape) or inline at the head of the message (`error[build]: [E5003] …`, for
//! the `anyhow`-carried refusals of `run_project`). [`is_capability_gap`]
//! matches the token, so both renderings classify identically.

/// The binary carries no native backend at all: it was built without the
/// `mlir-build` feature. A host-capability fact, never a defect.
pub const NO_NATIVE_BACKEND: &str = "E5003";

/// The native backend is compiled in, but its toolchain (`mlir-opt` / `clang`)
/// is absent from `PATH`. A host-capability fact, never a defect.
pub const NATIVE_TOOLCHAIN_ABSENT: &str = "E5004";

/// The source itself could not be lowered natively (a parse / type / lowering
/// diagnostic was raised and the module was embedded as a runtime-JIT
/// fallback). A REAL failure — deliberately NOT a capability code.
pub const SOURCE_NOT_NATIVELY_COMPILABLE: &str = "E5005";

/// Why a module was embedded as a runtime-JIT fallback instead of lowered to a
/// native object.
///
/// Produced at the single site that knows the answer
/// (`project::compile_single_source`) and threaded to the refusal sites, so the
/// decision is made ONCE. Deriving it a second time at the refusal (by
/// re-probing `PATH` or re-reading the feature flags) would be the same
/// hand-copied-knowledge shape this module exists to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// No `mlir-build` feature in this binary — see [`NO_NATIVE_BACKEND`].
    NoNativeBackend,
    /// `mlir-opt` / `clang` absent — see [`NATIVE_TOOLCHAIN_ABSENT`].
    NativeToolchainAbsent,
    /// The module did not compile — see [`SOURCE_NOT_NATIVELY_COMPILABLE`].
    SourceNotNativelyCompilable,
}

impl FallbackReason {
    /// The stable diagnostic code for this cause. Exhaustive by construction:
    /// a new variant cannot be added without choosing its code.
    pub const fn code(self) -> &'static str {
        match self {
            FallbackReason::NoNativeBackend => NO_NATIVE_BACKEND,
            FallbackReason::NativeToolchainAbsent => NATIVE_TOOLCHAIN_ABSENT,
            FallbackReason::SourceNotNativelyCompilable => SOURCE_NOT_NATIVELY_COMPILABLE,
        }
    }

    /// Is this cause a genuine host-capability gap (as opposed to a real
    /// failure of the program or the compiler)?
    pub const fn is_capability(self) -> bool {
        match self {
            FallbackReason::NoNativeBackend | FallbackReason::NativeToolchainAbsent => true,
            FallbackReason::SourceNotNativelyCompilable => false,
        }
    }

    /// Fail-CLOSED merge for a build with several fallen-back modules: a real
    /// source failure dominates every host-capability cause, so a project whose
    /// entry skipped for a missing toolchain but whose sibling failed to
    /// compile is reported as a real failure, never as a skip.
    pub fn merge(self, other: Self) -> Self {
        if !self.is_capability() || !other.is_capability() {
            FallbackReason::SourceNotNativelyCompilable
        } else if self == other {
            self
        } else {
            // Two different capability causes cannot both hold on one host
            // (the feature is either compiled in or not); prefer the narrower
            // "backend absent" reading rather than inventing a third state.
            FallbackReason::NoNativeBackend
        }
    }

    /// Prefix `message` with this cause's code token, for refusals carried as
    /// plain strings (`anyhow`) rather than structured diagnostics.
    pub fn tag(self, message: impl AsRef<str>) -> String {
        format!("[{}] {}", self.code(), message.as_ref())
    }
}

/// The result of trying to lower ONE source to a native object.
///
/// Returned by `project::compile_single_source` so the CAUSE of a runtime-JIT
/// fallback travels with the fact of it. The previous `bool` erased the cause
/// at the only place it was known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOutcome {
    /// Lowered to a real native object.
    Native,
    /// Embedded as a runtime-JIT fallback, for this reason.
    Fallback(FallbackReason),
}

/// Every code that means "this host lacks the capability".
///
/// The classifier below is defined over exactly this list, so adding a
/// capability cause is one edit and forgetting one fails closed.
pub const CAPABILITY_CODES: [&str; 2] = [NO_NATIVE_BACKEND, NATIVE_TOOLCHAIN_ABSENT];

/// Does `stderr` carry a genuine capability code?
///
/// Matched on the CODE token (`[E5003]`), never on diagnostic prose: a
/// re-worded diagnostic keeps its classification, and a refusal that was never
/// given a code is NOT a capability gap — it fails closed.
pub fn is_capability_gap(stderr: &str) -> bool {
    CAPABILITY_CODES
        .iter()
        .any(|code| stderr.contains(&format!("[{code}]")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct() {
        let all = [
            NO_NATIVE_BACKEND,
            NATIVE_TOOLCHAIN_ABSENT,
            SOURCE_NOT_NATIVELY_COMPILABLE,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "two causes share one code: {a}");
            }
        }
    }

    #[test]
    fn the_real_failure_code_is_never_a_capability_gap() {
        assert!(!CAPABILITY_CODES.contains(&SOURCE_NOT_NATIVELY_COMPILABLE));
        assert!(!is_capability_gap(
            &FallbackReason::SourceNotNativelyCompilable.tag("x")
        ));
    }

    #[test]
    fn capability_causes_classify_as_gaps() {
        for reason in [
            FallbackReason::NoNativeBackend,
            FallbackReason::NativeToolchainAbsent,
        ] {
            assert!(reason.is_capability());
            assert!(is_capability_gap(
                &reason.tag("entry module was not natively compiled")
            ));
            assert!(is_capability_gap(&format!(
                "error[build][{}]: x",
                reason.code()
            )));
        }
    }

    #[test]
    fn prose_alone_is_not_a_gap() {
        // The exact wording that used to be matched by substring.
        assert!(!is_capability_gap(
            "error[build]: --emit-shared requires building with the 'mlir-build' feature\n"
        ));
        assert!(!is_capability_gap("error: tool not found: mlir-opt\n"));
    }

    #[test]
    fn a_real_failure_dominates_the_merge() {
        let cap = FallbackReason::NativeToolchainAbsent;
        let real = FallbackReason::SourceNotNativelyCompilable;
        assert_eq!(cap.merge(real), real);
        assert_eq!(real.merge(cap), real);
        assert_eq!(cap.merge(cap), cap);
    }
}
