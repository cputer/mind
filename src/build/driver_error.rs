// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! One place where a project-driver error becomes a typed [`BuildError`].

use crate::diagnostics::refusal::CodedRefusal;

use super::BuildError;

/// Turn a project-driver `anyhow` error into a typed [`BuildError`], KEEPING
/// its cause code.
///
/// Every driver error used to be flattened with
/// `BuildError::failed(format!("{e}"))`, whose code is `None` — the value that
/// means "undiagnosed", and an undiagnosed refusal fails closed. So a refusal
/// that knew exactly why it refused (the backend's runtime library is not
/// installed on this host) arrived at the consumer with no cause, and a genuine
/// host-capability gap graded as a compiler regression on every host without
/// the separately licensed runtime — the default state of a public checkout.
///
/// The cause travels as a typed payload, so recovering it is a downcast rather
/// than a second, hand-written decoder of the message text.
pub(super) fn classify_driver_error(err: anyhow::Error) -> BuildError {
    match CodedRefusal::of(&err) {
        Some(refusal) => BuildError::refused(refusal.reason().code(), refusal.message()),
        None => BuildError::failed(format!("{err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::capability::{FallbackReason, RUNTIME_LIBRARY_ABSENT};

    #[test]
    fn a_coded_refusal_keeps_its_cause_through_the_orchestrator() {
        // The STRUCTURAL contract, independent of how the message renders:
        // `BuildError::code()` is what a consumer classifies on, and the
        // flattening constructor answers `None` there whatever the prose says.
        let err = anyhow::Error::new(CodedRefusal::new(
            FallbackReason::RuntimeLibraryAbsent,
            "MIND runtime not found for backend 'cpu'.",
        ));
        let build_err = classify_driver_error(err);
        assert_eq!(build_err.code(), Some(RUNTIME_LIBRARY_ABSENT));
        // The code is rendered ONCE, in the header slot -- not also inside the
        // message, which is what a `{e}`-flattened refusal would produce.
        assert_eq!(
            build_err.render(),
            "error[build][E5002]: MIND runtime not found for backend 'cpu'."
        );
    }

    #[test]
    fn an_undiagnosed_driver_error_stays_undiagnosed() {
        // The negative twin: no cause invented for an error that has none, so
        // an ordinary failure can never grade as a capability gap.
        let build_err = classify_driver_error(anyhow::anyhow!("cannot write manifest"));
        assert_eq!(build_err.code(), None);
        assert_eq!(build_err.render(), "error[build]: cannot write manifest");
    }
}
