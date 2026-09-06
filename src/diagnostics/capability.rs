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
//! # Wire shape — the code is POSITIONAL, not free text
//!
//! A coded refusal carries its code as the bracketed token `[E5003]`, in one of
//! exactly two header slots: the diagnostic prefix (`error[build][E5003]: …`,
//! the `DiagnosticEmitter` / `BuildError::render` shape) or the head of the
//! message (`error[build]: [E5003] …`, the [`FallbackReason::tag`] shape used
//! by the `anyhow`-carried refusals of `run_project`). `header_tokens` reads
//! those slots and NOTHING else, so both renderings classify identically.
//!
//! Reading the token from anywhere on the buffer was itself a fail-open: the
//! compiler echoes user text, so a program whose own source indexes with
//! `arr[E5003]` came back as an unknown-identifier diagnostic quoting that
//! name, plus a source-snippet line repeating it — and a whole-buffer scan
//! graded that failing compile a capability skip. A program could BUY a pass
//! from every gate by naming a code. Message bodies, `-->` locations, `|`
//! snippets, `= note:` labels and continuation lines are DATA and contribute
//! nothing.
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
//! So the classifier reads EVERY error header on the wire and merges them
//! fail-closed. A gap requires that every error header carry a cause code and
//! that every one of those codes be a capability cause. Two omissions are
//! therefore safe by construction:
//!
//! * a cause added without being registered as a capability is still inside the
//!   reserved [`CAUSE_CODE_PREFIX`] namespace, reads as unknown, and vetoes the
//!   skip;
//! * a refusal with NO cause code — every ordinary diagnostic, since none is
//!   minted inside the namespace — vetoes it too. Merging only the tokens it
//!   recognised made a real failure INVISIBLE when it shared the wire with a
//!   capability refusal; an undiagnosed refusal is not a capability gap.
//!
//! An ordinary diagnostic still cannot FORGE a verdict (`E0308` is not a cause
//! token), but it does veto one — because it is a failure of this build.
//!
//! # The reservation is MECHANICAL, in both directions
//!
//! That argument holds only while the namespace is ACTUALLY reserved, and it
//! was not: two ordinary diagnostics were minted inside it —
//! `pipeline::CompileError::BackendUnavailable` took `E5001`, and
//! `InvalidManifestExport` took the code after it — and the
//! inclusion-direction test (every cause code is inside the namespace) could
//! not see either. (The second code is not spelled out here on purpose: the
//! scan below reads prose as well as literals, so naming a code the registry
//! does not own would re-open the very hole this paragraph describes.)
//!
//! The consequence was this module's own failure mode, inverted: `mindc
//! x.mind --target gpu` refused with `error[backend][E5001]`, an unregistered
//! token INSIDE the scanned namespace, so a genuine host-capability gap read
//! as an unknown cause, vetoed its own skip, and graded as a compiler
//! regression on every host without that backend.
//!
//! Both occupants are resolved at the root rather than tolerated. The
//! backend-unavailable refusal IS a host-capability cause and is registered as
//! one ([`FallbackReason::TargetBackendUnavailable`]); the manifest-export
//! error is an ordinary user error and was renumbered out of the namespace,
//! into the `E6xxx` manifest range. The exclusion direction is then enforced
//! mechanically by `the_reserved_namespace_holds_only_registered_causes`,
//! which reads every `E50<digits>` literal in the crate's own sources and
//! fails the build unless it is the [`FallbackReason::code`] of a registered
//! cause — so a non-cause diagnostic can no longer be born inside the
//! namespace and veto a legitimate skip, and a new cause cannot be added
//! without registering it.
//!
//! That scan sees codes, so it cannot see a refusal that has NO code at all —
//! which fails closed, and therefore hard-fails every host that genuinely has
//! the gap. The complementary direction (every host-capability refusal in
//! `src/` MINTS a cause) is enforced by
//! `tests/capability_refusal_cause_scan.rs`, one case per registered
//! capability cause, keyed on [`CAPABILITY_CODES`] in both directions.

/// A backend for the requested target is not available in this build: the
/// target lowers to canonical IR here, but final emission needs the matching
/// `mind-runtime` backend library. A host/build capability fact, never a
/// defect — see `pipeline::CompileError::BackendUnavailable`.
pub const TARGET_BACKEND_UNAVAILABLE: &str = "E5001";

/// The installed MIND runtime library required by a non-CPU backend is absent:
/// neither `MIND_LIB_DIR` nor `~/.mind/lib` holds it. Native CPU executables use
/// the bundled public runtime-support shim; accelerator runtimes ship separately,
/// so their absence is a host-capability fact, never a compiler defect.
pub const RUNTIME_LIBRARY_ABSENT: &str = "E5002";

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

/// Why a native artifact could not be produced: a module embedded as a
/// runtime-JIT fallback, or a target refused outright.
///
/// This enum is also the REGISTRY of the reserved [`CAUSE_CODE_PREFIX`]
/// namespace — every code in that namespace belongs to exactly one variant
/// here, and the source scan in the tests enforces it.
///
/// Produced at the single site that knows the answer
/// (`project::compile_single_source`, `pipeline::compile_source_with_name`) and
/// threaded to the refusal sites, so the decision is made ONCE. Deriving it a
/// second time at the refusal (by re-probing `PATH` or re-reading the feature
/// flags) would be the same hand-copied-knowledge shape this module exists to
/// remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// This build has no backend for the requested target — see
    /// [`TARGET_BACKEND_UNAVAILABLE`].
    TargetBackendUnavailable,
    /// The backend's runtime library is not installed on this host — see
    /// [`RUNTIME_LIBRARY_ABSENT`].
    RuntimeLibraryAbsent,
    /// No `mlir-build` feature in this binary — see [`NO_NATIVE_BACKEND`].
    NoNativeBackend,
    /// `mlir-opt` / `clang` absent — see [`NATIVE_TOOLCHAIN_ABSENT`].
    NativeToolchainAbsent,
    /// The module did not compile — see [`SOURCE_NOT_NATIVELY_COMPILABLE`].
    SourceNotNativelyCompilable,
}

impl FallbackReason {
    /// Every cause, so a registry-wide rule can be stated once instead of
    /// re-listed at each call site.
    ///
    /// Hand-listed, but it cannot drift silently: a variant left out of it is
    /// still a code LITERAL in this file, and the namespace scan reads that
    /// literal, finds no registered owner for it, and fails the build.
    pub const ALL: [FallbackReason; 5] = [
        FallbackReason::TargetBackendUnavailable,
        FallbackReason::RuntimeLibraryAbsent,
        FallbackReason::NoNativeBackend,
        FallbackReason::NativeToolchainAbsent,
        FallbackReason::SourceNotNativelyCompilable,
    ];

    /// The stable diagnostic code for this cause. Exhaustive by construction:
    /// a new variant cannot be added without choosing its code.
    pub const fn code(self) -> &'static str {
        match self {
            FallbackReason::TargetBackendUnavailable => TARGET_BACKEND_UNAVAILABLE,
            FallbackReason::RuntimeLibraryAbsent => RUNTIME_LIBRARY_ABSENT,
            FallbackReason::NoNativeBackend => NO_NATIVE_BACKEND,
            FallbackReason::NativeToolchainAbsent => NATIVE_TOOLCHAIN_ABSENT,
            FallbackReason::SourceNotNativelyCompilable => SOURCE_NOT_NATIVELY_COMPILABLE,
        }
    }

    /// Is this cause a genuine host-capability gap (as opposed to a real
    /// failure of the program or the compiler)?
    pub const fn is_capability(self) -> bool {
        match self {
            FallbackReason::TargetBackendUnavailable
            | FallbackReason::RuntimeLibraryAbsent
            | FallbackReason::NoNativeBackend
            | FallbackReason::NativeToolchainAbsent => true,
            FallbackReason::SourceNotNativelyCompilable => false,
        }
    }

    /// Fail-CLOSED precedence used by [`FallbackReason::merge`]: the highest
    /// rank on the wire is the verdict.
    ///
    /// Exhaustive by construction, so a new cause cannot be added without
    /// placing itself in the order, and the resulting merge is commutative and
    /// associative — unlike the pairwise special case it replaces.
    const fn precedence(self) -> u8 {
        match self {
            // Narrowest: a statement about ONE requested target.
            FallbackReason::TargetBackendUnavailable => 0,
            // About this host's PATH.
            FallbackReason::NativeToolchainAbsent => 1,
            // About what is INSTALLED on this host: broader than one missing
            // build tool on `PATH`, narrower than "this binary has no native
            // backend at all".
            FallbackReason::RuntimeLibraryAbsent => 2,
            // Broadest capability statement: this whole binary has no native
            // backend, so it wins a tie among capability causes.
            FallbackReason::NoNativeBackend => 3,
            // A real failure dominates every capability cause.
            FallbackReason::SourceNotNativelyCompilable => 4,
        }
    }

    /// Fail-CLOSED merge for a build with several fallen-back modules: a real
    /// source failure dominates every host-capability cause, so a project whose
    /// entry skipped for a missing toolchain but whose sibling failed to
    /// compile is reported as a real failure, never as a skip.
    pub fn merge(self, other: Self) -> Self {
        if self.precedence() >= other.precedence() {
            self
        } else {
            other
        }
    }

    /// Prefix `message` with this cause's code token, for refusals carried as
    /// plain strings (`anyhow`) rather than structured diagnostics.
    ///
    /// The token's shape is [`code_token`]'s, not this function's.
    pub fn tag(self, message: impl AsRef<str>) -> String {
        let mut out = code_token(self.code());
        out.push(' ');
        out.push_str(message.as_ref());
        out
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

/// Render `code` as the bracketed token a refusal carries on the wire
/// (`"E5003"` -> `"[E5003]"`).
///
/// The ONE writer of the shape `header_tokens` reads, so the encoder and the
/// decoder live in the same module and cannot drift: changing the token's
/// punctuation is a change to this function plus its reader, and every producer
/// follows. Both header slots named in the module docs go through it — the
/// message-head shape ([`FallbackReason::tag`]) and the diagnostic-prefix shape
/// (`build::error::BuildError::code_tag`).
pub fn code_token(code: &str) -> String {
    format!("[{code}]")
}

/// Every code that means "this host lacks the capability".
///
/// The classifier below is defined over exactly this list, so adding a
/// capability cause is one edit and forgetting one fails closed: an
/// unregistered cause is still IN the reserved namespace, so it is seen, read
/// as unknown, and vetoes the skip.
pub const CAPABILITY_CODES: [&str; 4] = [
    TARGET_BACKEND_UNAVAILABLE,
    RUNTIME_LIBRARY_ABSENT,
    NO_NATIVE_BACKEND,
    NATIVE_TOOLCHAIN_ABSENT,
];

/// The prefix reserved for CAUSE codes — the codes that answer "why was this
/// refused", as opposed to the ordinary diagnostic codes (`E1001`, `E2002`,
/// `E0308`) that answer "what is wrong with the program".
///
/// Scanning the namespace rather than a hand-listed set is what makes an
/// omission safe in BOTH directions (see the module docs). Both directions are
/// pinned mechanically: `every_cause_code_is_in_the_reserved_namespace` keeps
/// every cause INSIDE the namespace, and
/// `the_reserved_namespace_holds_only_registered_causes` keeps every code
/// inside the namespace owned by a cause.
pub const CAUSE_CODE_PREFIX: &str = "E50";

/// Is `token` (the text between one `[` and the next `]`) a reserved cause
/// code: [`CAUSE_CODE_PREFIX`] followed by at least one digit and nothing else?
fn is_cause_code(token: &str) -> bool {
    match token.strip_prefix(CAUSE_CODE_PREFIX) {
        Some(digits) => !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// The severity word that opens a REFUSAL header. A `warning[...]` line
/// reports something the build tolerated, so it neither carries a refusal
/// cause nor vetoes one.
const ERROR_SEVERITY: &str = "error";

/// The bracketed tokens `line` carries in CODE POSITION, or `None` when `line`
/// is not an error-diagnostic header.
///
/// A cause code is POSITIONAL, never free text. This crate prints exactly two
/// header renderings and the token is read from those slots only:
///
/// * `error[<phase>][<CODE>]: <msg>` — the `DiagnosticEmitter` and
///   `BuildError::render` shape. The leading run of `[..]` groups may be longer
///   than two (`error[workspace][<member>][<CODE>]:`), so the whole run is read.
/// * `error[<phase>]: [<CODE>] <msg>` — the [`FallbackReason::tag`] shape, for
///   refusals carried as plain `anyhow` strings. Exactly ONE group, at the head
///   of the message and followed by a space, is in code position.
///
/// Everything else on the wire is DATA and can never contribute a token: a
/// message body that quotes a user identifier, a `-->` location, a `|` source
/// snippet, a `= note:` label, or any continuation line — all of which either
/// begin with whitespace or fail the header grammar below. Reading them was a
/// fail-OPEN hole: a program could forge its own capability skip by naming a
/// cause code in its own source text, which the compiler then echoed back.
fn header_tokens(line: &str) -> Option<Vec<&str>> {
    // A header starts at column 0; every snippet, label and note line is
    // indented, so leading whitespace alone disqualifies a line.
    let mut rest = line.strip_prefix(ERROR_SEVERITY)?;
    let mut tokens = Vec::new();
    while let Some(after) = rest.strip_prefix('[') {
        let close = after.find(']')?;
        tokens.push(&after[..close]);
        rest = &after[close + 1..];
    }
    // The severity/phase run must be terminated by the message separator, or
    // this is an ordinary line that merely begins with the word "error".
    let message = rest.strip_prefix(':')?.strip_prefix(' ').unwrap_or("");
    if let Some(after) = message.strip_prefix('[') {
        if let Some(close) = after.find(']') {
            // `tag()` renders `[CODE] message`; requiring the space keeps a
            // bracketed word that merely OPENS a message out of code position.
            if after[close + 1..].starts_with(' ') {
                tokens.push(&after[..close]);
            }
        }
    }
    Some(tokens)
}

/// Every reserved cause-code token carried by `stderr`, in wire order.
///
/// `error[build][E5003]: …` yields `["E5003"]`: the `build` token is not in the
/// namespace, and neither is an ordinary diagnostic code. A cause code that is
/// not in a header's code slot — echoed source text, a path, a message body —
/// yields nothing: see `header_tokens`.
pub fn cause_codes(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter_map(header_tokens)
        .flat_map(|tokens| tokens.into_iter().filter(|t| is_cause_code(t)))
        .collect()
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
    let mut saw_capability = false;
    for line in stderr.lines() {
        let Some(tokens) = header_tokens(line) else {
            continue;
        };
        let mut coded = false;
        for token in tokens {
            if !is_cause_code(token) {
                continue;
            }
            coded = true;
            if !CAPABILITY_CODES.contains(&token) {
                // A real cause, or a cause nobody registered: fail closed.
                return false;
            }
            saw_capability = true;
        }
        if !coded {
            // An UNCODED refusal. No ordinary diagnostic is minted inside the
            // cause namespace, so a real failure sharing the wire with a
            // capability refusal carries no cause token at all — reading only
            // cause tokens made it invisible and graded the run a skip. An
            // undiagnosed refusal is not a capability gap.
            return false;
        }
    }
    saw_capability
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A code inside the reserved namespace that no cause owns.
    ///
    /// Assembled rather than written as a literal because a literal here would
    /// (correctly) be reported by the namespace scan below — which is exactly
    /// the property `the_namespace_scan_still_sees_an_intruder` controls for.
    const UNREGISTERED_TOKEN: &str = concat!("E50", "99");

    #[test]
    fn codes_are_distinct() {
        let all: Vec<&str> = FallbackReason::ALL.iter().map(|r| r.code()).collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "two causes share one code: {a}");
            }
        }
    }

    #[test]
    fn the_capability_list_agrees_with_the_registry() {
        // One source of truth: `CAPABILITY_CODES` is what the classifier reads,
        // `is_capability()` is what the compiler reasons with. A cause listed in
        // one and not the other is a silent fail-open (or a silent veto).
        for reason in FallbackReason::ALL {
            assert_eq!(
                CAPABILITY_CODES.contains(&reason.code()),
                reason.is_capability(),
                "{reason:?} ({}) disagrees between CAPABILITY_CODES and is_capability()",
                reason.code()
            );
        }
        assert_eq!(
            CAPABILITY_CODES.len(),
            FallbackReason::ALL
                .iter()
                .filter(|r| r.is_capability())
                .count(),
            "CAPABILITY_CODES carries a code no registered cause owns"
        );
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
        for reason in FallbackReason::ALL
            .into_iter()
            .filter(|r| r.is_capability())
        {
            assert!(reason.is_capability());
            // The `anyhow` rendering: `run_project`'s error is printed as
            // `error: <tag>`, so the code sits at the head of the message.
            assert!(is_capability_gap(&format!(
                "error: {}",
                reason.tag("entry module was not natively compiled")
            )));
            // ... and the `DiagnosticEmitter` rendering, code in the prefix.
            assert!(is_capability_gap(&format!(
                "error[build][{}]: x",
                reason.code()
            )));
            // A BARE tag is not a wire shape: nothing prints a refusal without
            // its `error[...]` header, and a loose token must not decide.
            assert!(!is_capability_gap(&reason.tag("x")));
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
        for reason in FallbackReason::ALL {
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
        // The anyhow rendering: one code slot at the head of the message.
        assert_eq!(
            cause_codes("error[build]: [E5004] tool not found: mlir-opt"),
            vec![NATIVE_TOOLCHAIN_ABSENT]
        );
        // POSITION decides. A code in a message body, in an echoed source
        // snippet, in a `-->` path or on an indented label is user data.
        assert!(cause_codes("error[type-check][E2002]: unknown identifier `E5003`").is_empty());
        assert!(cause_codes("   |     arr[E5003]").is_empty());
        assert!(cause_codes("  --> /tmp/[E5003]/src/main.mind:3:9").is_empty());
        assert!(cause_codes("   = note: see [E5003]").is_empty());
        assert!(cause_codes(" error[build][E5003]: indented, so not a header").is_empty());
        // A warning is not a refusal, so it carries no cause.
        assert!(cause_codes("warning[build][E5003]: x").is_empty());
        // A word that merely starts with "error" is not a header.
        assert!(cause_codes("errors[build][E5003]: x").is_empty());
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
        let stderr = format!(
            "error[build][{NO_NATIVE_BACKEND}]: a\nerror[build][{UNREGISTERED_TOKEN}]: b\n"
        );
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

    #[test]
    fn an_unavailable_target_backend_is_a_capability_gap() {
        // VERBATIM stderr of `mindc x.mind --target gpu` (rendered by
        // `DiagnosticEmitter::render_prefix` as `error[<phase>][<code>]`). This
        // build simply has no backend for that target: a host/build capability
        // fact. Before the code was registered it was an unknown token INSIDE
        // the scanned namespace, so it vetoed its own skip and every converted
        // call site graded it a compiler regression.
        let stderr = "error[backend][E5001]: no backend available for target gpu\n";
        assert!(
            stderr.contains(TARGET_BACKEND_UNAVAILABLE),
            "fixture drifted from the compiler's own code"
        );
        assert!(is_capability_gap(stderr), "{stderr}");
    }

    #[test]
    fn an_ordinary_diagnostic_cannot_forge_a_verdict_but_does_veto_one() {
        // The manifest-export error's twin, renumbered out of the cause
        // namespace (`E6001`): an invalid `Mind.toml` entry is a user error.
        let manifest = "error[manifest][E6001]: invalid Mind.toml [exports] c_abi entry \
                        `bad name`: not a C identifier\n";
        // It is not a refusal CAUSE, so it can never forge a capability verdict.
        assert!(cause_codes(manifest).is_empty(), "{manifest}");
        assert!(!is_capability_gap(manifest), "{manifest}");
        // But it IS a failure of this build, so it vetoes a capability skip
        // that shares the stderr. Ignoring it was the fail-open: no ordinary
        // diagnostic is minted inside the cause namespace, so a classifier that
        // merged only cause tokens could not see a real failure at all.
        let with_gap = format!("{manifest}error[build][{NO_NATIVE_BACKEND}]: no backend\n");
        assert!(!is_capability_gap(&with_gap), "{with_gap}");
    }

    #[test]
    fn a_program_cannot_forge_a_skip_by_naming_a_cause_code() {
        // VERBATIM `mindc build` stderr for
        // `fn main() -> i64 { let arr: [i64; 2] = [1, 2]; arr[E5003] }`.
        // Every `E5003` on this wire is the program's OWN text, echoed back by
        // the compiler. A whole-buffer token scan graded it a capability skip.
        let forged = "error[type-check][E2002]: unknown identifier `E5003`\n  \
                      --> /tmp/.tmp0/src/main.mind:3:9\n   |     arr[E5003]\n   \
                      |         ^^^^^\nerror[build]: /tmp/.tmp0/src/main.mind: \
                      unresolved identifier(s) — refusing to embed a module that \
                      would crash at lowering\n";
        assert!(
            forged.contains(NO_NATIVE_BACKEND),
            "fixture lost its forgery"
        );
        assert!(cause_codes(forged).is_empty(), "{:?}", cause_codes(forged));
        assert!(!is_capability_gap(forged), "{forged}");
    }

    #[test]
    fn an_uncoded_error_header_vetoes_a_capability_skip() {
        // The other half of the merge: a real failure that carries an ORDINARY
        // code shares the wire with a genuine capability refusal.
        let mixed = format!(
            "error[build][{NO_NATIVE_BACKEND}]: entry module was not natively compiled\n\
             error[type-check][E2002]: unknown identifier `helper`\n"
        );
        assert_eq!(cause_codes(&mixed), vec![NO_NATIVE_BACKEND]);
        assert!(!is_capability_gap(&mixed), "{mixed}");
    }

    #[test]
    fn merging_is_order_independent() {
        // The verdict may not depend on which member of a workspace printed
        // first: `merge` is a precedence maximum, not a pairwise special case.
        for a in FallbackReason::ALL {
            for b in FallbackReason::ALL {
                assert_eq!(a.merge(b), b.merge(a), "{a:?} vs {b:?}");
                assert_eq!(
                    a.merge(b).is_capability(),
                    a.is_capability() && b.is_capability(),
                    "{a:?} merged with {b:?} changed the capability verdict"
                );
            }
        }
    }

    // --- the exclusion direction, enforced over the crate's own sources ------
    //
    // `every_cause_code_is_in_the_reserved_namespace` proves only that causes
    // are INSIDE the namespace. Nothing proved the converse, and two ordinary
    // diagnostics had already moved in (the backend-unavailable refusal and the
    // manifest-export error) — one of them a real capability gap that the
    // classifier therefore graded as a regression. The
    // scan below closes that direction: a code literal in the namespace that no
    // cause owns fails the build.

    /// Every `E50<digits>` token in `text`, wherever it appears — a string
    /// literal, a doc comment or ordinary prose. Deliberately not restricted to
    /// quoted literals: a code discussed in a comment is still a code minted in
    /// the namespace as far as a reader (and the next `Diagnostic::error` call)
    /// is concerned.
    fn cause_namespace_tokens(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0;
        while let Some(hit) = text[i..].find(CAUSE_CODE_PREFIX) {
            let start = i + hit;
            let mut end = start + CAUSE_CODE_PREFIX.len();
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start + CAUSE_CODE_PREFIX.len() {
                found.push(text[start..end].to_string());
            }
            i = end.max(start + 1);
        }
        found
    }

    /// The tokens in `text` that no registered cause owns.
    fn unregistered_namespace_tokens(text: &str) -> Vec<String> {
        cause_namespace_tokens(text)
            .into_iter()
            .filter(|t| !FallbackReason::ALL.iter().any(|r| r.code() == t))
            .collect()
    }

    /// Every `.rs` file of this crate, read from the manifest directory.
    ///
    /// The scope is DERIVED (walk `$CARGO_MANIFEST_DIR/src`), not a hand-copied
    /// file list, so a new module or subdirectory cannot fall outside the rule
    /// the way the two occupants did. No file is excluded — including this one:
    /// the negative-control token above is assembled at compile time precisely
    /// so the scanner need not be blinded to its own module.
    fn crate_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read source dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut paths = Vec::new();
        walk(&root, &mut paths);
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let text = std::fs::read_to_string(&path).expect("read source");
                (path.display().to_string(), text)
            })
            .collect()
    }

    #[test]
    fn the_reserved_namespace_holds_only_registered_causes() {
        let mut intruders = Vec::new();
        for (path, text) in crate_sources() {
            for token in unregistered_namespace_tokens(&text) {
                intruders.push(format!("{path}: {token}"));
            }
        }
        assert!(
            intruders.is_empty(),
            "these diagnostic codes sit inside the reserved {CAUSE_CODE_PREFIX}xx cause \
             namespace but no `FallbackReason` owns them, so the classifier reads each one \
             as an unknown refusal cause and VETOES every capability skip on the same \
             stderr. Either register the cause (add a `FallbackReason` variant, and add its \
             code to `CAPABILITY_CODES` if it is a host-capability fact), or renumber the \
             diagnostic out of the namespace.\n  {}",
            intruders.join("\n  ")
        );
    }

    #[test]
    fn the_namespace_scan_still_sees_an_intruder() {
        // Positive control: the scan is evidence only if it fails on the shape
        // it forbids. This is the exact pre-fix emission from `pipeline.rs`.
        let intruder =
            format!("vec![Diagnostic::error(\"backend\", \"{UNREGISTERED_TOKEN}\", msg)]");
        assert_eq!(
            unregistered_namespace_tokens(&intruder),
            vec![UNREGISTERED_TOKEN.to_string()],
            "the scan cannot see a code no cause owns"
        );
        // ... and does not report a registered one.
        assert!(
            unregistered_namespace_tokens(&format!("\"{NO_NATIVE_BACKEND}\"")).is_empty(),
            "the scan reported a registered cause"
        );
        // ... and is not vacuous: it really read the tree, and really found
        // codes there. A scan over an empty set passes without asserting.
        let sources = crate_sources();
        assert!(sources.len() > 1, "ran=0: the source scan read no files");
        let scanned: usize = sources
            .iter()
            .map(|(_, text)| cause_namespace_tokens(text).len())
            .sum();
        assert!(scanned > 0, "ran=0: the scan found no cause code at all");
    }
}
