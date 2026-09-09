// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Canonical module builders used by the MIC3 v0x04 mirror vector generator.

#[cfg(feature = "std-surface")]
use std::collections::BTreeMap;

use libmind::ir::{IRModule, Instr, ValueId};
use libmind::types::{
    CanonicalModuleTypes, FieldDraft, FunctionDeclaration, FunctionIdentity, FunctionKind,
    FunctionSemanticTypes, FunctionSignature, ScalarType, SchemaDraft, SchemaIdentity,
    SchemaRegistryBuilder, SemanticType, TypeExpr,
};

use super::mirror_oracle::read_uleb;

fn i64_type() -> SemanticType {
    SemanticType::Scalar(ScalarType::I64)
}

// ---------------------------------------------------------------------------
// Modules
// ---------------------------------------------------------------------------

pub(super) fn module_value_only() -> IRModule {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(0), i64_type())
        .expect("module type");
    let mut module = IRModule::new();
    let value = module.fresh();
    module.instrs.push(Instr::ConstI64(value, 42));
    module.canonical_types = Some(Box::new(bundle));
    module
}

fn semantic(identity: FunctionIdentity, values: &[usize]) -> Box<FunctionSemanticTypes> {
    let mut semantic = FunctionSemanticTypes::new(identity);
    for value in values {
        semantic
            .set_value_type(ValueId(*value), i64_type())
            .expect("unique function type");
    }
    Box::new(semantic)
}

/// A module whose string table is nonempty and non-trivially ordered: two
/// owners, a record schema with two fields, and three function declarations.
pub(super) fn module_scoped() -> IRModule {
    let mut schemas = SchemaRegistryBuilder::default();
    for owner in ["ownerA", "ownerB"] {
        schemas
            .add_schema(SchemaDraft::new(
                SchemaIdentity::new(owner, "Pair"),
                vec![
                    FieldDraft::new("zeta", TypeExpr::Scalar(ScalarType::I64)),
                    FieldDraft::new("alpha", TypeExpr::Scalar(ScalarType::Bool)),
                ],
            ))
            .expect("schema");
    }
    let mut bundle = CanonicalModuleTypes::new(schemas.finish().expect("registry"));
    let main = FunctionIdentity::new("entry", "main");
    let local_step = FunctionIdentity::new("ownerA", "step");
    let external_step = FunctionIdentity::new("ownerB", "step");
    for (identity, kind) in [
        (main.clone(), FunctionKind::Local),
        (local_step.clone(), FunctionKind::Local),
        (external_step.clone(), FunctionKind::External),
    ] {
        bundle
            .add_declaration(FunctionDeclaration::new(
                identity,
                kind,
                FunctionSignature::new(vec![i64_type()], Some(i64_type())),
            ))
            .expect("declaration");
    }
    bundle
        .set_module_value_type(ValueId(0), i64_type())
        .expect("module %0");

    let mut module = IRModule::new();
    let module_zero = module.fresh();
    module.instrs.push(Instr::ConstI64(module_zero, 9));
    module.instrs.push(Instr::Output(module_zero));
    module.instrs.push(Instr::FnDef {
        name: "step".to_string(),
        params: vec![("x".to_string(), ValueId(0))],
        ret_id: Some(ValueId(0)),
        body: vec![Instr::Return {
            value: Some(ValueId(0)),
        }],
        reap_threshold: None,
        semantic_types: Some(semantic(local_step, &[0])),
        #[cfg(feature = "std-surface")]
        value_types: BTreeMap::new(),
    });
    module.instrs.push(Instr::FnDef {
        name: "main".to_string(),
        params: vec![("x".to_string(), ValueId(0))],
        ret_id: Some(ValueId(1)),
        body: vec![
            Instr::Call {
                dst: ValueId(1),
                name: "step".to_string(),
                args: vec![ValueId(0)],
                resolved_callee: Some(Box::new(external_step)),
            },
            Instr::Return {
                value: Some(ValueId(1)),
            },
        ],
        reap_threshold: None,
        semantic_types: Some(semantic(main, &[0, 1])),
        #[cfg(feature = "std-surface")]
        value_types: BTreeMap::new(),
    });
    module.canonical_types = Some(Box::new(bundle));
    module
}

/// Does any ULEB field in `prefix` occupy more than one byte?
///
/// A corpus whose every length/count fits in seven bits cannot distinguish a
/// correct minimal-ULEB encoder from one that shifts by the wrong amount: a
/// planted `>> 6` mutation survived the first version of this corpus for exactly
/// that reason. The multi-byte vector below exists to kill that mutant, and this
/// predicate is what stops the coverage from being lost again silently.
pub(super) fn prefix_has_multibyte_uleb(prefix: &[u8]) -> bool {
    let mut pos = 5usize;
    let mut multi = false;
    let mut field = |bytes: &[u8], pos: &mut usize| -> u64 {
        let start = *pos;
        let value = read_uleb(bytes, pos).expect("prefix ULEB");
        if *pos - start > 1 {
            multi = true;
        }
        value
    };
    let _surface = field(prefix, &mut pos);
    let count = field(prefix, &mut pos) as usize;
    for _ in 0..count {
        let length = field(prefix, &mut pos) as usize;
        pos += length;
    }
    multi
}

/// A module carrying `count` exports. Exports are the only strings this module
/// contributes, so their string-table indices are 0..count, i.e. CONSECUTIVE
/// and, once `count` exceeds 128, reaching a MULTI-BYTE ULEB index.
///
/// Both properties are load-bearing. Every other positive in the corpus has at
/// most one export sitting at index 0, which leaves an emitter that always
/// writes 0, and an order rule that demands a gap between successive indices,
/// indistinguishable from the correct code.
/// A module carrying a ConstF64, whose payload is eight raw little-endian bytes
/// rather than a ULEB.
///
/// Added after a mutation that copied SEVEN of those eight bytes survived the
/// entire corpus: no fixture contained a ConstF64 at all, so the only
/// fixed-width payload in the instruction grammar was never re-emitted once.
/// The mirror does not interpret these bytes and performs no floating-point
/// arithmetic; it relays them, so the property under test is purely the width.
pub(super) fn module_with_const_f64() -> IRModule {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(0), SemanticType::Scalar(ScalarType::F64))
        .expect("module type");
    let mut module = IRModule::new();
    let value = module.fresh();
    module.instrs.push(Instr::ConstF64(value, 1.5));
    module.canonical_types = Some(Box::new(bundle));
    module
}

pub(super) fn module_with_export_count(count: usize) -> IRModule {
    let mut module = module_value_only();
    for index in 0..count {
        module.exports.insert(format!("e{index:04}"));
    }
    module
}

/// A module whose `next_id` needs a MULTI-BYTE ULEB. `next_id` is the first
/// field in the body that is not bounded by the remaining byte count, so a
/// single-byte-only emitter survives every other positive.
pub(super) fn module_with_wide_next_id() -> IRModule {
    let mut module = module_value_only();
    module.next_id = 300;
    module
}

pub(super) fn module_with_export_name_len(length: usize) -> IRModule {
    let mut module = module_value_only();
    module.exports.insert("x".repeat(length));
    module
}
