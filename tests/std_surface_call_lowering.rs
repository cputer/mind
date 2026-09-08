// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! RFC 0005 Phase 0 — generic `Instr::Call` -> MLIR `func.call`.
//!
//! Proves a non-tensor call lowers to `func.call @name(i64..) -> i64`
//! with a matching `func.func private @name(...)` declaration, and
//! that non-i64 call args are a clear error (aggregate ABI is phase
//! 2+). Gated: `cargo test --features std-surface,mlir-lowering
//! --test std_surface_call_lowering`.

#![cfg(all(feature = "std-surface", feature = "mlir-lowering"))]

use libmind::ir::{IRModule, Instr, ValueId};
use libmind::mlir::lower_ir_to_mlir;

fn scalar(m: &mut IRModule, v: i64) -> ValueId {
    let id = m.fresh();
    m.instrs.push(Instr::ConstI64(id, v));
    id
}

#[test]
fn call_lowers_to_func_call_with_private_decl() {
    let mut m = IRModule::new();
    let n = scalar(&mut m, 64);
    let dst = m.fresh();
    m.instrs.push(Instr::Call {
        dst,
        name: "__mind_alloc".to_string(),
        args: vec![n],

        resolved_callee: None,
    });
    m.instrs.push(Instr::Output(dst));

    let text = lower_ir_to_mlir(&m).expect("lowering must succeed").text;

    assert!(
        text.contains("func.call @__mind_alloc(%"),
        "expected a func.call to __mind_alloc; got:\n{text}"
    );
    assert!(
        text.contains(") : (i64) -> i64"),
        "call must use the i64 ABI; got:\n{text}"
    );
    assert!(
        text.contains("func.func private @__mind_alloc(i64) -> i64"),
        "expected a private declaration for the callee; got:\n{text}"
    );
}

#[test]
fn distinct_callees_each_get_one_decl_sorted() {
    let mut m = IRModule::new();
    let a = scalar(&mut m, 1);
    let d1 = m.fresh();
    m.instrs.push(Instr::Call {
        dst: d1,
        name: "__mind_free".to_string(),
        args: vec![a],

        resolved_callee: None,
    });
    let d2 = m.fresh();
    m.instrs.push(Instr::Call {
        dst: d2,
        name: "__mind_alloc".to_string(),
        args: vec![a],

        resolved_callee: None,
    });
    m.instrs.push(Instr::Output(d2));

    let text = lower_ir_to_mlir(&m).expect("lowering").text;
    let alloc = text.find("func.func private @__mind_alloc").unwrap();
    let free = text.find("func.func private @__mind_free").unwrap();
    // BTreeSet ordering -> deterministic, alphabetical.
    assert!(
        alloc < free,
        "extern decls must be emitted in deterministic sorted order"
    );
    assert_eq!(
        text.matches("func.func private @__mind_alloc").count(),
        1,
        "one declaration per distinct callee"
    );
}

#[test]
fn non_i64_call_arg_is_a_clear_error() {
    use libmind::types::{DType, ShapeDim};
    let mut m = IRModule::new();
    let t = m.fresh();
    m.instrs.push(Instr::ConstTensor(
        t,
        DType::F32,
        vec![ShapeDim::Known(2)],
        Some(0.0),
    ));
    let dst = m.fresh();
    m.instrs.push(Instr::Call {
        dst,
        name: "__mind_alloc".to_string(),
        args: vec![t], // tensor arg -> must be rejected

        resolved_callee: None,
    });
    m.instrs.push(Instr::Output(dst));

    let err = lower_ir_to_mlir(&m).expect_err("tensor call arg must error");
    let msg = format!("{err}");
    assert!(
        msg.contains("non-i64") && msg.contains("phase 2"),
        "error must name the i64-ABI limitation and point to phase 2+; got: {msg}"
    );
}

#[test]
fn inline_intrinsics_reject_tensor_operands_in_every_position() {
    use libmind::types::{DType, ShapeDim};
    for (name, arity) in [
        ("__mind_load_i8", 1),
        ("__mind_load_i16", 1),
        ("__mind_load_i32", 1),
        ("__mind_store_i8", 2),
        ("__mind_store_i16", 2),
        ("__mind_store_i32", 2),
        ("__mind_f64_to_bits", 1),
    ] {
        for position in 0..arity {
            let mut m = IRModule::new();
            let tensor = m.fresh();
            m.instrs.push(Instr::ConstTensor(
                tensor,
                DType::F32,
                vec![ShapeDim::Known(2)],
                Some(0.0),
            ));
            let args = (0..arity)
                .map(|i| {
                    if i == position {
                        tensor
                    } else {
                        scalar(&mut m, 1)
                    }
                })
                .collect();
            let dst = m.fresh();
            m.instrs.push(Instr::Call {
                dst,
                name: name.into(),
                args,
                resolved_callee: None,
            });
            m.instrs.push(Instr::Output(dst));
            let err = lower_ir_to_mlir(&m)
                .expect_err("inline scalar intrinsic must reject tensor operands");
            assert!(
                err.to_string().contains(name),
                "{name} argument {position}: {err}"
            );
        }
    }
}
