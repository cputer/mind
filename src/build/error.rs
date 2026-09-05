// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! The build orchestrator's typed error, and the ONE place a refusal is turned
//! into wire text.
//!
//! Split out of `build::mod` because the wire shape of a refusal is a separate
//! concern from driving a build: the cause code, the bracketed token and the
//! `error[build][E5003]: …` header are read back by
//! [`crate::diagnostics::capability`]'s classifier, so they are load-bearing
//! output format, not an implementation detail of the orchestrator. Keeping
//! them in a leaf module means the rendering and its tests sit together and
//! neither grows the orchestrator.

use crate::diagnostics::capability::{self, FallbackReason};
use crate::project::BuildResult;

/// Typed errors from the build orchestrator.
///
/// `Invalid` maps to exit code 2 (bad usage / bad manifest).
/// `Failed`  maps to exit code 1 (compile / link error).
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("{0}")]
    Invalid(String),
    /// A build refusal. `code` is the stable diagnostic code of the CAUSE (see
    /// [`crate::diagnostics::capability`]) when the cause is
    /// machine-classifiable, `None` for an undiagnosed failure — which is
    /// exactly what "not a capability gap" means to a consumer, so an
    /// uncoded refusal fails closed by construction.
    #[error("{msg}")]
    Failed {
        code: Option<&'static str>,
        msg: String,
    },
}

impl BuildError {
    /// An undiagnosed build failure (no stable cause code).
    pub fn failed(msg: impl Into<String>) -> Self {
        BuildError::Failed {
            code: None,
            msg: msg.into(),
        }
    }

    /// A refusal whose CAUSE has a stable diagnostic code.
    pub fn refused(code: &'static str, msg: impl Into<String>) -> Self {
        BuildError::Failed {
            code: Some(code),
            msg: msg.into(),
        }
    }

    /// The stable cause code, when this refusal has one.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            BuildError::Invalid(_) => None,
            BuildError::Failed { code, .. } => *code,
        }
    }

    /// The bracketed code token (`"[E5003]"`) or `""` when the refusal has no
    /// cause code.
    ///
    /// The shape itself belongs to [`capability::code_token`], which is also
    /// what the classifier's reader is defined against — so this cannot drift
    /// from the token the harness matches.
    pub fn code_tag(&self) -> String {
        match self.code() {
            Some(code) => capability::code_token(code),
            None => String::new(),
        }
    }

    /// Render exactly as the CLI prints it: `error[build][E5003]: <msg>` when
    /// the cause is coded, `error[build]: <msg>` otherwise. Owning the prefix
    /// here is what keeps the code on the wire — a `{err}` at the print site
    /// would drop it.
    pub fn render(&self) -> String {
        format!("error[build]{}: {self}", self.code_tag())
    }

    /// Suggested process exit code per RFC 0008 §6.
    pub fn exit_code(&self) -> i32 {
        match self {
            BuildError::Invalid(_) => 2,
            BuildError::Failed { .. } => 1,
        }
    }
}

/// Fail closed on a build whose modules did not natively compile.
///
/// ISSUE #244 — `mindc build` was not fail-closed. `compile_single_source`
/// treats EVERY diagnostic except E2002 as non-fatal: it warns, embeds the
/// source for the runtime JIT, and returns `Ok(false)`. It reports that through
/// `entry_native_compiled` / `fallback_sources`, whose own doc-comments say
/// `run_project` fails loud on them -- and it does. `build` never looked, so the
/// flags were computed and dropped, and the command exited 0.
///
/// Measured: `fn broken( -> {` (which `mindc check` rejects with E1001) produced
/// a 41 KB ELF and exit 0. A `[WARN]` was printed, so it was not silent -- but
/// any CI step that reads the exit code passed on source that does not compile,
/// which is the whole hazard. The emitted artifact is a launcher deferring to
/// mind-runtime, and when the syntax exceeds that runtime's parser scope it
/// prints a notice and exits 0 too -- a false green all the way down.
///
/// The same two checks `run_project` already performs, in the same order, and
/// they live beside [`BuildError`] because deciding to refuse and wording the
/// refusal are one concern: the wording carries the CAUSE code computed by the
/// compile step (`BuildResult::fallback_reason`), and a consumer that cannot
/// read that code can only guess from prose — which the test harness did, and
/// got wrong, hard-failing every host built without `mlir-build`.
///
/// A host with no native backend at all is a capability gap (`E5003`/`E5004`);
/// a module that did not compile is a real failure (`E5005`). An absent reason
/// is read as the real failure, never as the capability gap.
pub fn refuse_if_fell_back(result: &BuildResult) -> Result<(), BuildError> {
    let cause = result
        .fallback_reason
        .unwrap_or(FallbackReason::SourceNotNativelyCompilable)
        .code();
    if !result.entry_native_compiled {
        return Err(BuildError::refused(
            cause,
            "entry module was not natively compiled (embedded as a runtime-JIT fallback \
             -- see the [WARN] above); refusing to report a successful build for an \
             artifact that is a launcher deferring to the installed mind-runtime, which \
             may exit 0 without executing your program",
        ));
    }
    if !result.fallback_sources.is_empty() {
        return Err(BuildError::refused(
            cause,
            format!(
                "module(s) not natively compiled (embedded as a runtime-JIT fallback -- see \
             the [WARN] above): {}. The natively-compiled entry can call into their \
             launcher-stub symbols and reach the installed mind-runtime at execution, \
             which may exit 0 without executing that code; refusing to report success \
             rather than emit a false green",
                result.fallback_sources.join(", ")
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::capability::{NO_NATIVE_BACKEND, is_capability_gap};

    /// The rendered refusal must put its cause in a slot the classifier reads.
    /// This is the whole point of owning the header here: a print site holding
    /// only `{err}` would drop the code, and an uncoded refusal is graded a
    /// compiler regression on every host that has the capability gap.
    #[test]
    fn a_coded_refusal_renders_into_the_classifier_s_slot() {
        let err = BuildError::refused(NO_NATIVE_BACKEND, "no native backend in this binary");
        assert_eq!(err.code(), Some(NO_NATIVE_BACKEND));
        assert_eq!(err.code_tag(), capability::code_token(NO_NATIVE_BACKEND));
        assert_eq!(
            err.render(),
            format!("error[build][{NO_NATIVE_BACKEND}]: no native backend in this binary")
        );
        assert!(is_capability_gap(&err.render()));
    }

    /// An UNCODED failure must render without a token and must NOT read as a
    /// capability gap — "not machine-classifiable" is exactly what a consumer
    /// needs to hear, and inventing a token here would fail open.
    #[test]
    fn an_uncoded_failure_renders_no_token_and_is_not_a_gap() {
        let err = BuildError::failed("cannot move artifact");
        assert_eq!(err.code(), None);
        assert_eq!(err.code_tag(), "");
        assert_eq!(err.render(), "error[build]: cannot move artifact");
        assert!(!is_capability_gap(&err.render()));
    }

    /// RFC 0008 §6: bad usage is 2, a failed build is 1.
    #[test]
    fn exit_codes_separate_bad_usage_from_a_failed_build() {
        assert_eq!(BuildError::Invalid("bad manifest".into()).exit_code(), 2);
        assert_eq!(BuildError::failed("link error").exit_code(), 1);
    }
}
