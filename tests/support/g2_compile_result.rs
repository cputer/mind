// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Explicit compiler refusal, successful output and runtime failure stay distinct.

pub(super) enum MindCall {
    /// `mindc_compile` returned a handle and we decoded its buffer.
    Ok(Vec<u8>),
    /// Explicit compiler refusal (next_id == -1, empty output).
    Refused,
    /// The worker terminated abnormally (panic / internal assertion / stack
    /// overflow the runtime turned into an unwind). NOT an unsupported construct.
    Crashed(String),
}

pub(super) fn classify_compiler_output(next_id: i64, bytes: Vec<u8>) -> MindCall {
    if next_id == -1 && bytes.is_empty() {
        return MindCall::Refused;
    }
    if next_id < 0 {
        return MindCall::Crashed("invalid compiler refusal record".to_string());
    }
    MindCall::Ok(bytes)
}

#[test]
fn explicit_refusal_is_distinct_from_empty_success_and_invalid_status() {
    assert!(matches!(
        classify_compiler_output(-1, vec![]),
        MindCall::Refused
    ));
    assert!(matches!(classify_compiler_output(0, vec![]), MindCall::Ok(bytes) if bytes.is_empty()));
    assert!(matches!(
        classify_compiler_output(1, b"module {}".to_vec()),
        MindCall::Ok(_)
    ));
    assert!(matches!(
        classify_compiler_output(-1, vec![1]),
        MindCall::Crashed(_)
    ));
    assert!(matches!(
        classify_compiler_output(-2, vec![]),
        MindCall::Crashed(_)
    ));
}
