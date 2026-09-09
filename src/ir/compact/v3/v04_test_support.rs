// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Shared canonical modules for v04 evidence and decoder tests.

use super::*;

pub(super) fn i64_type() -> SemanticType {
    SemanticType::Scalar(ScalarType::I64)
}

pub(super) fn decode_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex"), 16).unwrap())
        .collect()
}

pub(super) fn declaration(identity: FunctionIdentity, kind: FunctionKind) -> FunctionDeclaration {
    FunctionDeclaration::new(
        identity,
        kind,
        FunctionSignature::new(vec![i64_type()], Some(i64_type())),
    )
}

pub(super) fn semantic(identity: FunctionIdentity, values: &[usize]) -> Box<FunctionSemanticTypes> {
    let mut semantic = FunctionSemanticTypes::new(identity);
    for value in values {
        semantic
            .set_value_type(ValueId(*value), i64_type())
            .expect("unique function type");
    }
    Box::new(semantic)
}

pub(super) fn scoped_module(omit_step_value: bool) -> IRModule {
    let mut schemas = SchemaRegistryBuilder::default();
    for owner in ["ownerA", "ownerB"] {
        schemas
            .add_schema(SchemaDraft::new(
                SchemaIdentity::new(owner, "Pair"),
                vec![
                    FieldDraft::new("z", TypeExpr::Scalar(ScalarType::I64)),
                    FieldDraft::new("a", TypeExpr::Scalar(ScalarType::Bool)),
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
            .add_declaration(declaration(identity, kind))
            .expect("declaration");
    }
    bundle
        .set_module_value_type(ValueId(0), i64_type())
        .expect("module %0");

    let mut module = IRModule::new();
    let module_zero = module.fresh();
    assert_eq!(module_zero, ValueId(0));
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
        semantic_types: Some(semantic(
            local_step,
            if omit_step_value { &[] } else { &[0] },
        )),
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
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
        value_types: std::collections::BTreeMap::new(),
    });
    module.canonical_types = Some(Box::new(bundle));
    module
}

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

pub(super) fn module_with_export_name_len(length: usize) -> IRModule {
    let mut module = module_value_only();
    module.exports.insert("x".repeat(length));
    module
}

pub(super) fn largest_body_at_most(limit: usize) -> (IRModule, Vec<u8>) {
    let mut low = 0usize;
    let mut high = MAX_MIC3_INPUT - 1;
    while low < high {
        let candidate = low + (high - low).div_ceil(2);
        let module = module_with_export_name_len(candidate);
        if let Ok(bytes) = emit_mic3_checked(&module) {
            if bytes.len() <= limit {
                low = candidate;
                continue;
            }
        }
        high = candidate - 1;
    }
    let module = module_with_export_name_len(low);
    let bytes = emit_mic3_checked(&module).expect("largest admitted body");
    assert!(bytes.len() <= limit);
    (module, bytes)
}

pub(super) fn descriptor_only_module(reverse: bool) -> IRModule {
    let left = SchemaDraft::new(
        SchemaIdentity::new("ownerA", "Node"),
        vec![
            FieldDraft::new(
                "z",
                TypeExpr::DynamicArray {
                    element: Box::new(TypeExpr::RecordRef(SchemaIdentity::new("ownerB", "Node"))),
                },
            ),
            FieldDraft::new("a", TypeExpr::Scalar(ScalarType::Bool)),
        ],
    );
    let right = SchemaDraft::new(
        SchemaIdentity::new("ownerB", "Node"),
        vec![FieldDraft::new(
            "back",
            TypeExpr::FixedArray {
                extent: 2,
                element: Box::new(TypeExpr::RecordRef(SchemaIdentity::new("ownerA", "Node"))),
            },
        )],
    );
    let mut registry = SchemaRegistryBuilder::default();
    let drafts = if reverse {
        [right, left]
    } else {
        [left, right]
    };
    for draft in drafts {
        registry.add_schema(draft).expect("schema draft");
    }
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(CanonicalModuleTypes::new(
        registry.finish().expect("cyclic registry"),
    )));
    module
}
