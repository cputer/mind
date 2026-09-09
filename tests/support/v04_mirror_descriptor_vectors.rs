// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Reference-derived semantic descriptor-budget vectors for the pure-MIND
//! MIC3 `0x04` declared-prefix mirror.

use libmind::ir::IRModule;
use libmind::ir::compact::v3::{emit_mic3_checked, parse_mic3_prefix};
use libmind::types::{
    CanonicalModuleTypes, FieldDraft, ScalarType, SchemaDraft, SchemaIdentity,
    SchemaRegistryBuilder, TypeExpr,
};

use super::{Vector, code, rederive_prefix, synthetic_head, write_uleb};

fn fixed_i64(extents: &[u64]) -> Vec<u8> {
    let mut out = Vec::new();
    for extent in extents {
        out.push(2);
        write_uleb(&mut out, *extent);
    }
    out.extend_from_slice(&[0, 1]);
    out
}

fn one_field_schema_prefix(descriptor: &[u8]) -> Vec<u8> {
    let mut out = synthetic_head(&["A", "B", "f"]);
    write_uleb(&mut out, 1);
    write_uleb(&mut out, 0);
    write_uleb(&mut out, 1);
    write_uleb(&mut out, 1);
    write_uleb(&mut out, 2);
    out.extend_from_slice(descriptor);
    write_uleb(&mut out, 0);
    out
}

fn complete_empty_core_tail(mut prefix: Vec<u8>) -> Vec<u8> {
    // next_id, exports, instructions, four fixed legacy registries, and module
    // semantic values. All are empty so the reference reaches canonical-type
    // validation rather than refusing a truncated body first.
    prefix.extend_from_slice(&[0; 8]);
    prefix
}

fn require_reference_budget_refusal(name: &str, bytes: &[u8]) {
    let error = match parse_mic3_prefix(bytes) {
        Err(error) => error,
        Ok(_) => panic!("{name} must be refused by the reference decoder"),
    };
    assert!(
        error
            .to_string()
            .contains("fixed-array element count exceeds"),
        "{name} must reach reference semantic-element validation: {error}"
    );
}

fn module_with_schema(descriptor: TypeExpr) -> IRModule {
    let mut schemas = SchemaRegistryBuilder::default();
    schemas
        .add_schema(SchemaDraft::new(
            SchemaIdentity::new("A", "B"),
            vec![FieldDraft::new("f", descriptor)],
        ))
        .expect("schema descriptor must be accepted");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(CanonicalModuleTypes::new(
        schemas.finish().expect("schema registry must be accepted"),
    )));
    module
}

fn fixed_expr(extents: &[u64]) -> TypeExpr {
    extents
        .iter()
        .rev()
        .fold(TypeExpr::Scalar(ScalarType::I64), |element, extent| {
            TypeExpr::FixedArray {
                element: Box::new(element),
                extent: *extent,
            }
        })
}

pub(super) fn append(vectors: &mut Vec<Vector>) {
    // The public registry and encoder produce this exact accepted boundary.
    let boundary = module_with_schema(fixed_expr(&[1 << 20, 1 << 20]));
    let full_boundary = emit_mic3_checked(&boundary).expect("2^40 element boundary body");
    let (prefix_boundary, consumed, _) =
        rederive_prefix(&full_boundary).expect("2^40 element boundary prefix");
    assert_eq!(prefix_boundary, full_boundary[..consumed]);
    vectors.push(Vector {
        name: "pos_prefix_element_boundary",
        bytes: prefix_boundary,
        expect: code::OK_EXACT,
        note: "reference-built nested fixed arrays total exactly 2^40 elements",
    });
    vectors.push(Vector {
        name: "pos_full_body_element_boundary",
        bytes: full_boundary,
        expect: code::REMAINDER_REFUSED,
        note: "real encoder body whose schema descriptor totals exactly 2^40 elements",
    });

    // RecordRef is one semantic element; it never recursively expands fields.
    let identity = SchemaIdentity::new("A", "B");
    let record = module_with_schema(TypeExpr::RecordRef(identity));
    let full_record = emit_mic3_checked(&record).expect("self record-reference body");
    let (prefix_record, _, _) = rederive_prefix(&full_record).expect("record-reference prefix");
    vectors.push(Vector {
        name: "pos_prefix_record_ref_cost_one",
        bytes: prefix_record,
        expect: code::OK_EXACT,
        note: "a record reference has semantic cost one without expanding its fields",
    });

    let oversized = fixed_i64(&[u64::from(u32::MAX), u64::from(u32::MAX)]);
    let product = complete_empty_core_tail(one_field_schema_prefix(&oversized));
    require_reference_budget_refusal("neg_schema_descriptor_product", &product);
    vectors.push(Vector {
        name: "neg_schema_descriptor_product",
        bytes: product,
        expect: code::DESCRIPTOR_ELEMENTS,
        note: "nested fixed-array product exceeds 2^40 before i64 multiplication",
    });

    let mut hidden = vec![2];
    write_uleb(&mut hidden, 0);
    hidden.extend_from_slice(&oversized);
    let zero_hidden = complete_empty_core_tail(one_field_schema_prefix(&hidden));
    require_reference_budget_refusal("neg_zero_extent_hides_product", &zero_hidden);
    vectors.push(Vector {
        name: "neg_zero_extent_hides_product",
        bytes: zero_hidden,
        expect: code::DESCRIPTOR_ELEMENTS,
        note: "zero outer extent cannot hide an invalid nested element shape",
    });

    let mut dynamic = vec![3];
    dynamic.extend_from_slice(&fixed_i64(&[1 << 20, 1 << 20]));
    let dynamic_over = complete_empty_core_tail(one_field_schema_prefix(&dynamic));
    require_reference_budget_refusal("neg_dynamic_over_boundary", &dynamic_over);
    vectors.push(Vector {
        name: "neg_dynamic_over_boundary",
        bytes: dynamic_over,
        expect: code::DESCRIPTOR_ELEMENTS,
        note: "dynamic descriptor adds one to a child already at 2^40",
    });

    let half = fixed_i64(&[1 << 19, 1 << 20]);
    let mut schema_total = synthetic_head(&["A", "B", "a", "b", "c"]);
    write_uleb(&mut schema_total, 1);
    write_uleb(&mut schema_total, 0);
    write_uleb(&mut schema_total, 1);
    write_uleb(&mut schema_total, 3);
    for name in [2_u64, 3] {
        write_uleb(&mut schema_total, name);
        schema_total.extend_from_slice(&half);
    }
    write_uleb(&mut schema_total, 4);
    schema_total.extend_from_slice(&[0, 1]);
    write_uleb(&mut schema_total, 0);
    schema_total = complete_empty_core_tail(schema_total);
    require_reference_budget_refusal("neg_schema_cumulative_elements", &schema_total);
    vectors.push(Vector {
        name: "neg_schema_cumulative_elements",
        bytes: schema_total,
        expect: code::DESCRIPTOR_ELEMENTS,
        note: "two 2^39 fields reach the boundary; one scalar exceeds the schema total",
    });

    let mut function_total = synthetic_head(&["entry", "main"]);
    write_uleb(&mut function_total, 0);
    write_uleb(&mut function_total, 1);
    write_uleb(&mut function_total, 0);
    write_uleb(&mut function_total, 1);
    function_total.push(0);
    write_uleb(&mut function_total, 2);
    function_total.extend_from_slice(&half);
    function_total.extend_from_slice(&half);
    function_total.push(1);
    function_total.extend_from_slice(&[0, 1]);
    function_total = complete_empty_core_tail(function_total);
    require_reference_budget_refusal("neg_function_cumulative_elements", &function_total);
    vectors.push(Vector {
        name: "neg_function_cumulative_elements",
        bytes: function_total,
        expect: code::DESCRIPTOR_ELEMENTS,
        note: "two 2^39 parameters reach the boundary; scalar return exceeds it",
    });
}
