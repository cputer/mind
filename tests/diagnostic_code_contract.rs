// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Core v1 diagnostic assignments that must not drift between compiler paths.

use libmind::diagnostics::capability::TARGET_BACKEND_UNAVAILABLE;
use libmind::eval::materialization::MaterializationRefusal;
use libmind::pipeline::{CompileError, MATERIALIZATION_REFUSAL_CODE};
use libmind::runtime::types::BackendTarget;

#[test]
fn backend_unavailable_and_materialization_have_distinct_catalog_codes() {
    let backend = CompileError::BackendUnavailable {
        target: BackendTarget::Gpu,
    }
    .into_diagnostics(None);
    let materialization = CompileError::Materialization(MaterializationRefusal::CounterOverflow)
        .into_diagnostics(None);

    assert_eq!(TARGET_BACKEND_UNAVAILABLE, "E6002");
    assert_eq!(MATERIALIZATION_REFUSAL_CODE, "E6009");
    assert_eq!(backend.len(), 1);
    assert_eq!(backend[0].phase, "backend");
    assert_eq!(backend[0].code, TARGET_BACKEND_UNAVAILABLE);
    assert_eq!(materialization.len(), 1);
    assert_eq!(materialization[0].phase, "materialization");
    assert_eq!(materialization[0].code, MATERIALIZATION_REFUSAL_CODE);
    assert_ne!(backend[0].code, materialization[0].code);
}
