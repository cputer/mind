// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the “License”);
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an “AS IS” BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Part of the MIND project (Machine Intelligence Native Design).

//! A refusal that keeps its registered CAUSE while travelling through an
//! `anyhow` channel.
//!
//! # The hole this closes
//!
//! [`super::capability`] gives every refusal a stable cause code, but a code is
//! only worth what survives the trip to the consumer. The project driver's
//! refusals are `anyhow::Error` values, and `anyhow` erases the concrete type;
//! the build orchestrator then re-wrapped every one of them with
//! `BuildError::failed(format!("{e}"))`, which is the constructor whose code is
//! `None`. So a refusal that knew exactly why it refused — the backend's
//! runtime library is not installed on this host — reached `stderr` with no
//! code at all, the classifier read "undiagnosed", and a genuine capability gap
//! graded as a compiler regression on every host without the (separately
//! licensed) runtime. That is the DEFAULT state of a public checkout.
//!
//! # Why a typed payload rather than parsing the message back
//!
//! The message is already tagged for humans, so the code could in principle be
//! read back off the rendered string. That would put a second, hand-written
//! decoder next to the encoder ([`FallbackReason::tag`]) and give a program's
//! own text a way to mint a capability verdict by starting an error message
//! with a bracketed code. A typed payload recovered with `downcast_ref` has
//! neither property: there is one encoder, the recovery is exact, and only the
//! compiler can construct one.

use super::capability::FallbackReason;

/// A refusal whose cause is known at the site that refused.
///
/// The cause may be a host-capability fact or a real failure — the point is
/// that it is REGISTERED (`FallbackReason`), so the consumer classifies it
/// instead of guessing from prose.
///
/// Constructed at the site that knows the answer and recovered by the
/// orchestrator with [`CodedRefusal::of`], so the cause is decided ONCE
/// and never re-derived by re-probing the host.
///
/// `Display` renders the [`FallbackReason::tag`] shape (`[E5002] <message>`),
/// so a print site that only has `{e}` still puts the code on the wire in a
/// slot the classifier reads.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", reason.tag(message))]
pub struct CodedRefusal {
    reason: FallbackReason,
    message: String,
}

impl CodedRefusal {
    /// Refuse with `reason` as the cause.
    pub fn new(reason: FallbackReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }

    /// The cause, for a consumer that must classify rather than print.
    pub const fn reason(&self) -> FallbackReason {
        self.reason
    }

    /// The message WITHOUT the code tag, for a consumer that renders the code
    /// in its own header slot (`error[build][E5002]: …`) and would otherwise
    /// print it twice.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Recover a refusal from an erased `anyhow` error, anywhere in its cause
    /// chain — so adding `.context(...)` on the way up cannot lose the cause.
    pub fn of(err: &anyhow::Error) -> Option<&Self> {
        err.chain().find_map(|e| e.downcast_ref::<Self>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::capability::{RUNTIME_LIBRARY_ABSENT, is_capability_gap};

    fn sample() -> CodedRefusal {
        CodedRefusal::new(FallbackReason::RuntimeLibraryAbsent, "no runtime for 'cpu'")
    }

    #[test]
    fn display_carries_the_code_and_message_does_not() {
        assert_eq!(
            sample().to_string(),
            format!("[{RUNTIME_LIBRARY_ABSENT}] no runtime for 'cpu'")
        );
        assert_eq!(sample().message(), "no runtime for 'cpu'");
    }

    #[test]
    fn a_printed_refusal_classifies_as_a_capability_gap() {
        // The `{e}` path: whatever prints it, the code lands in code position.
        assert!(is_capability_gap(&format!("error[build]: {}", sample())));
    }

    #[test]
    fn the_cause_survives_erasure_and_added_context() {
        use anyhow::Context as _;
        let err = anyhow::Error::new(sample());
        assert_eq!(
            CodedRefusal::of(&err).map(|r| r.reason()),
            Some(FallbackReason::RuntimeLibraryAbsent)
        );
        let wrapped = Err::<(), _>(err).context("while linking").unwrap_err();
        assert_eq!(
            CodedRefusal::of(&wrapped).map(|r| r.reason()),
            Some(FallbackReason::RuntimeLibraryAbsent),
            "a context layer must not erase the cause"
        );
    }

    #[test]
    fn an_ordinary_error_has_no_cause() {
        let err = anyhow::anyhow!("cannot write manifest");
        assert!(CodedRefusal::of(&err).is_none());
    }
}
