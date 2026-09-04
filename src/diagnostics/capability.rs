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
//!
//! # One stderr, several causes
//!
//! A verdict is NOT "does a capability code appear somewhere". `mindc build`
//! over a workspace (`run_workspace_build`) prints one refusal per member and
//! keeps going, so a single stderr can carry a host-capability refusal for one
//! member and a real source failure for another. Reading the first kind and
//! ignoring the second graded that run as a tolerated skip — a fail-OPEN
//! decision, and a second, disagreeing implementation of the rule
//! [`FallbackReason::merge`] already applies one layer down.
//!
//! So the classifier reads EVERY cause token on the wire and merges them
//! fail-closed: a gap requires at least one capability cause and NO other
//! cause. The scan is over the reserved [`CAUSE_CODE_PREFIX`] namespace, which
//! makes both kinds of omission safe — a cause added without being registered
//! as a capability reads as unknown and vetoes the skip, while a diagnostic
//! outside the namespace (a type error's `E0308`) can neither forge a verdict
//! nor veto one.

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
/// capability cause is one edit and forgetting one fails closed: an
/// unregistered cause is still IN the reserved namespace, so it is seen, read
/// as unknown, and vetoes the skip.
pub const CAPABILITY_CODES: [&str; 2] = [NO_NATIVE_BACKEND, NATIVE_TOOLCHAIN_ABSENT];

/// The prefix reserved for CAUSE codes — the codes that answer "why was this
/// refused", as opposed to the ordinary diagnostic codes (`E1001`, `E2002`,
/// `E0308`) that answer "what is wrong with the program".
///
/// Scanning the namespace rather than a hand-listed set is what makes an
/// omission safe in BOTH directions (see the module docs), and
/// `every_cause_code_is_in_the_reserved_namespace` keeps the codes above inside
/// it, so a new cause cannot be born outside the scan.
pub const CAUSE_CODE_PREFIX: &str = "E50";

/// Is `token` (the text between one `[` and the next `]`) a reserved cause
/// code: [`CAUSE_CODE_PREFIX`] followed by at least one digit and nothing else?
fn is_cause_code(token: &str) -> bool {
    match token.strip_prefix(CAUSE_CODE_PREFIX) {
        Some(digits) => !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Every reserved cause-code token carried by `stderr`, in wire order.
///
/// `error[build][E5003]: …` yields `["E5003"]`: the `build` token is not in the
/// namespace, and neither is an ordinary diagnostic code.
pub fn cause_codes(stderr: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = stderr;
    while let Some(open) = rest.find('[') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find(']') else { break };
        let token = &rest[..close];
        if is_cause_code(token) {
            found.push(token);
        }
    }
    found
}

/// Does `stderr` report a genuine host-capability gap, and NOTHING else?
///
/// Matched on the CODE token (`[E5003]`), never on diagnostic prose: a
/// re-worded diagnostic keeps its classification, and a refusal that was never
/// given a code is NOT a capability gap — it fails closed.
///
/// The merge across tokens is the same fail-closed rule as
/// [`FallbackReason::merge`]: one real cause anywhere on the wire dominates
/// every capability cause, so a workspace whose first member skipped for a
/// missing backend and whose second member did not compile is a real failure.
pub fn is_capability_gap(stderr: &str) -> bool {
    let codes = cause_codes(stderr);
    !codes.is_empty() && codes.iter().all(|code| CAPABILITY_CODES.contains(code))
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
    fn every_cause_code_is_in_the_reserved_namespace() {
        // `code()` is an exhaustive match, so a new cause must choose a code;
        // this keeps that code inside the namespace the classifier scans.
        for reason in [
            FallbackReason::NoNativeBackend,
            FallbackReason::NativeToolchainAbsent,
            FallbackReason::SourceNotNativelyCompilable,
        ] {
            assert!(
                is_cause_code(reason.code()),
                "cause {reason:?} carries {}, outside the reserved {CAUSE_CODE_PREFIX} namespace",
                reason.code()
            );
        }
    }

    #[test]
    fn only_reserved_tokens_are_read_as_causes() {
        assert_eq!(
            cause_codes("error[build][E5003]: x"),
            vec![NO_NATIVE_BACKEND]
        );
        // An ordinary diagnostic code is not a cause: it can neither forge a
        // verdict nor veto one.
        assert!(cause_codes("error[E0308]: mismatched types").is_empty());
        assert!(cause_codes("[E50] [E5003x] [build] [WARN]").is_empty());
        assert!(cause_codes("no brackets at all").is_empty());
        assert!(cause_codes("unterminated [E5003").is_empty());
    }

    #[test]
    fn a_real_cause_beside_a_capability_cause_fails_closed() {
        // One workspace stderr, two members, opposite causes.
        let mixed = format!(
            "error[workspace][core][{}]: not natively compiled\n\
             error[workspace][tools][{}]: not natively compiled\n",
            NO_NATIVE_BACKEND, SOURCE_NOT_NATIVELY_COMPILABLE
        );
        assert!(!is_capability_gap(&mixed), "{mixed}");
    }

    #[test]
    fn an_unregistered_cause_vetoes_the_skip() {
        // A future cause added without being registered as a capability is
        // still IN the namespace, so it is seen and fails closed.
        let stderr = format!("error[build][{NO_NATIVE_BACKEND}]: a\nerror[build][E5099]: b\n");
        assert!(!is_capability_gap(&stderr), "{stderr}");
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
