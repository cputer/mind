// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Host guard for the runtime-JIT executable entry wrapper.

use crate::diagnostics::capability::FallbackReason;
use crate::diagnostics::refusal::CodedRefusal;

/// Refuse before generating the Unix-only entry wrapper on Windows.
///
/// `reason` is the cause already established by source/native compilation. It
/// must cross this boundary unchanged: a source failure remains a real failure,
/// while missing native support or tools remains a host-capability refusal.
pub(super) fn require_supported(
    is_entry: bool,
    reason: FallbackReason,
) -> Result<(), CodedRefusal> {
    require_supported_on_host(is_entry, reason, cfg!(target_os = "windows"))
}

fn require_supported_on_host(
    is_entry: bool,
    reason: FallbackReason,
    windows_host: bool,
) -> Result<(), CodedRefusal> {
    if is_entry && windows_host {
        return Err(CodedRefusal::new(
            reason,
            "runtime-JIT executable entry wrappers require Unix dynamic-loader APIs and are \
             unavailable on Windows; refusing before C generation",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_entry_refusal_preserves_the_established_cause() {
        for reason in [
            FallbackReason::NoNativeBackend,
            FallbackReason::NativeToolchainAbsent,
            FallbackReason::SourceNotNativelyCompilable,
        ] {
            let refusal = require_supported_on_host(true, reason, true).unwrap_err();
            assert_eq!(refusal.reason(), reason);
        }
    }

    #[test]
    fn real_source_failure_stays_non_capability() {
        let refusal =
            require_supported_on_host(true, FallbackReason::SourceNotNativelyCompilable, true)
                .unwrap_err();
        assert!(!refusal.reason().is_capability());
    }

    #[test]
    fn supported_entry_and_non_entry_paths_continue() {
        assert!(require_supported_on_host(true, FallbackReason::NoNativeBackend, false).is_ok());
        assert!(require_supported_on_host(false, FallbackReason::NoNativeBackend, true).is_ok());
    }
}
