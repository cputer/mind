// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Rejection parity for governed bimap and NERVE declarations.

use super::{MindCall, Outcome, call_in_subprocess, oracle_so_path, repo_root};
use std::{fs, path::Path, process::Output};

pub(super) fn check_rejection(
    rust_result: &Output,
    fixture: &Path,
    bin: &Path,
    src_bytes: &[u8],
) -> Option<Outcome> {
    let diagnostic = String::from_utf8_lossy(&rust_result.stderr);
    let rejection_fixture =
        fixture.starts_with(repo_root().join("tests/fixtures/selfhost_policy/reject"));
    if rejection_fixture {
        let expected = fs::read_to_string(fixture.with_extension("diagnostic"))
            .expect("policy rejection fixture must pin its expected diagnostic");
        let expected = expected.trim();
        assert!(!expected.is_empty(), "empty expected rejection diagnostic");
        if rust_result.status.success() || !diagnostic.contains(&format!("[{expected}]")) {
            return Some(Outcome::Diverge {
                diff_preview: format!(
                    "negative fixture requires Rust {expected}; got {}: {diagnostic}",
                    rust_result.status
                ),
            });
        }
    }
    if !rust_result.status.success() {
        // Policy refusals are part of the compiler contract. A matching positive
        // corpus cannot reveal a self-host that silently accepts these inputs.
        if rejection_fixture
            || diagnostic.contains("error[bimap][E")
            || diagnostic.contains("error[type-check][E_NERVE_")
        {
            let so = oracle_so_path(bin).expect("parent must resolve the oracle before fixtures");
            return Some(classify_policy_rejection(call_in_subprocess(
                src_bytes, &so,
            )));
        }
    }
    None
}

/// The Rust compiler has rejected a governed construct; the self-host must
/// reject it too. A crash remains a defect even on deliberately invalid input.
fn classify_policy_rejection(result: MindCall) -> Outcome {
    match result {
        MindCall::Refused => Outcome::RejectMatch,
        MindCall::Ok(bytes) => Outcome::Diverge {
            diff_preview: format!(
                "Rust rejected policy violation; self-host emitted {} bytes",
                bytes.len()
            ),
        },
        MindCall::Crashed(reason) => Outcome::MindCrash { reason },
    }
}

#[test]
fn policy_rejection_gate_distinguishes_refusal_acceptance_and_crash() {
    assert_eq!(
        classify_policy_rejection(MindCall::Refused),
        Outcome::RejectMatch
    );
    assert!(matches!(
        classify_policy_rejection(MindCall::Ok(Vec::new())),
        Outcome::Diverge { .. }
    ));
    assert!(matches!(
        classify_policy_rejection(MindCall::Ok(b"module {}".to_vec())),
        Outcome::Diverge { .. }
    ));
    assert!(matches!(
        classify_policy_rejection(MindCall::Crashed("SIGSEGV".to_string())),
        Outcome::MindCrash { .. }
    ));
}
