// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Body-section differential vectors: module `next_id` and the export list.
//!
//! Each body below is well formed through the whole prefix -- header, string
//! table, an empty schema section and an empty function section -- so the only
//! thing separating it from a body the decoder would keep reading is the defect
//! named. Without that, a refusal could be owned by an earlier section and
//! these would prove nothing about the export walk.

use libmind::ir::compact::v3::emit_mic3_checked;

use super::mirror_modules::{
    module_with_const_f64, module_with_export_count, module_with_wide_next_id,
};
use super::mirror_oracle::{rederive_prefix, synthetic_head, write_uleb};
use super::{Vector, code};

/// Header + sorted table + empty schema section + empty function section.
fn body_through_functions(strings: &[&str]) -> Vec<u8> {
    let mut out = synthetic_head(strings);
    write_uleb(&mut out, 0); // schema count
    write_uleb(&mut out, 0); // function count
    out
}

/// Positives that make the new EMITTER falsifiable.
///
/// Measured on the inherited corpus: every positive had next_id in {0,1}, at
/// most one export, and that export always at string index 0. Three mutants
/// therefore survived the whole 53-vector corpus -- a single-byte-only next_id
/// store, an export emitter hardcoding index 0, and an order rule demanding a
/// gap between successive indices. These fixtures kill all three.
fn append_emitter_positives(vectors: &mut Vec<Vector>) {
    // 130 exports: indices 0..129, so successive indices are CONSECUTIVE and
    // the last ones need a multi-byte ULEB.
    let wide = emit_mic3_checked(&module_with_export_count(130)).expect("wide export body");
    let (_, consumed, _) = rederive_prefix(&wide).expect("wide export prefix");
    vectors.push(Vector {
        name: "pos_prefix_consecutive_exports",
        bytes: wide[..consumed].to_vec(),
        expect: code::OK_EXACT,
        note: "130 exports: consecutive indices and multi-byte index ULEBs",
    });
    vectors.push(Vector {
        name: "pos_full_body_consecutive_exports",
        bytes: wide,
        expect: code::OK_EXACT,
        note: "same body entire: prefix verified, remainder refused",
    });

    let f64_body = emit_mic3_checked(&module_with_const_f64()).expect("const f64 body");
    let (_, consumed_f, _) = rederive_prefix(&f64_body).expect("const f64 prefix");
    assert_eq!(
        consumed_f,
        f64_body.len(),
        "const f64 body decodes entirely"
    );
    vectors.push(Vector {
        name: "pos_full_body_const_f64",
        bytes: f64_body,
        expect: code::OK_EXACT,
        note: "ConstF64: the only fixed-width eight-byte payload in the grammar",
    });

    let wide_id = emit_mic3_checked(&module_with_wide_next_id()).expect("wide next_id body");
    let (_, consumed_id, _) = rederive_prefix(&wide_id).expect("wide next_id prefix");
    vectors.push(Vector {
        name: "pos_prefix_wide_next_id",
        bytes: wide_id[..consumed_id].to_vec(),
        expect: code::OK_EXACT,
        note: "next_id 300, a multi-byte ULEB the emitter must widen",
    });
}

/// A COMPLETE minimal body: empty string table, no schemas, no functions,
/// next_id 1, no exports, one ConstI64, the four reserved zeros, and one module
/// semantic row. Byte-for-byte the shape the encoder emits for a value-only
/// module, which is why these negatives can perturb one field and leave the
/// rest of a genuinely acceptable body intact.
fn complete_minimal_body() -> Vec<u8> {
    let mut out = synthetic_head(&[]);
    write_uleb(&mut out, 0); // schema count
    write_uleb(&mut out, 0); // function count
    write_uleb(&mut out, 1); // next_id
    write_uleb(&mut out, 0); // export count
    write_uleb(&mut out, 1); // instruction count
    out.extend_from_slice(&[0x01, 0x00, 0x54]); // ConstI64 %0, zigzag(42)
    for _ in 0..4 {
        write_uleb(&mut out, 0); // reserved compatibility counts
    }
    write_uleb(&mut out, 1); // module semantic rows
    write_uleb(&mut out, 0); // %0
    out.extend_from_slice(&[0x00, 0x01]); // Scalar(I64)
    out
}

/// Body-tail negatives. Each perturbs exactly one field of a complete body.
fn append_body_tail_negatives(vectors: &mut Vec<Vector>) {
    // The instruction list is where an unsupported construct must be refused by
    // NAME rather than skipped: an unknown opcode makes the remaining stream
    // unparseable, so continuing would be invention.
    let mut unknown_op = complete_minimal_body();
    unknown_op[12] = 0x03; // OP_CONST_TENSOR: outside the core scalar subset
    vectors.push(Vector {
        name: "neg_unknown_opcode",
        bytes: unknown_op,
        expect: code::UNKNOWN_OPCODE,
        note: "tensor opcode is outside the core scalar subset",
    });

    let mut bad_binop = synthetic_head(&[]);
    write_uleb(&mut bad_binop, 0);
    write_uleb(&mut bad_binop, 0);
    write_uleb(&mut bad_binop, 3);
    write_uleb(&mut bad_binop, 0);
    write_uleb(&mut bad_binop, 1);
    // BinOp %2 = %0 <tag 0x7f> %1 -- a tag no build defines
    bad_binop.extend_from_slice(&[0x04, 0x02, 0x7f, 0x00, 0x01]);
    vectors.push(Vector {
        name: "neg_bad_binop_tag",
        bytes: bad_binop,
        expect: code::BAD_BINOP,
        note: "binary operator tag outside the core set",
    });

    // A reserved compatibility count that is not zero. Small, so it is refused
    // as populated rather than as a bound breach.
    let mut reserved = complete_minimal_body();
    reserved[15] = 1;
    vectors.push(Vector {
        name: "neg_reserved_count_nonzero",
        bytes: reserved,
        expect: code::RESERVED_NONZERO,
        note: "a reserved compatibility count is populated",
    });

    // Two module semantic rows in descending ValueId order.
    let mut rows = synthetic_head(&[]);
    write_uleb(&mut rows, 0);
    write_uleb(&mut rows, 0);
    write_uleb(&mut rows, 2);
    write_uleb(&mut rows, 0);
    write_uleb(&mut rows, 0); // no instructions
    for _ in 0..4 {
        write_uleb(&mut rows, 0);
    }
    write_uleb(&mut rows, 2);
    write_uleb(&mut rows, 1);
    rows.extend_from_slice(&[0x00, 0x01]);
    write_uleb(&mut rows, 0);
    rows.extend_from_slice(&[0x00, 0x01]);
    vectors.push(Vector {
        name: "neg_unsorted_value_rows",
        bytes: rows,
        expect: code::VALUE_ROW_ORDER,
        note: "module semantic rows in descending ValueId order",
    });

    // Trailing content after a COMPLETE body. Named pos_ because the reference's
    // BODY parser accepts it and reports the boundary -- rejecting trailing
    // content is the whole-artifact rule, which the mirror implements. This is
    // the only vector whose reference consumed length is deliberately shorter
    // than the vector, and the generator exempts it for that reason.
    let mut trailing = complete_minimal_body();
    trailing.push(0x00);
    vectors.push(Vector {
        name: "pos_trailing_after_body",
        bytes: trailing,
        expect: code::REMAINDER_REFUSED,
        note: "complete body followed by one trailing byte",
    });
}

pub fn append(vectors: &mut Vec<Vector>) {
    append_emitter_positives(vectors);
    append_body_tail_negatives(vectors);
    let table = ["alpha", "beta"];

    // Descending export references. The reference keeps exports in a set, so a
    // set round-trip would absorb this silently; the wire ordering rule is what
    // makes the encoding canonical.
    let mut descending = body_through_functions(&table);
    write_uleb(&mut descending, 0); // next_id
    write_uleb(&mut descending, 2); // export count
    write_uleb(&mut descending, 1);
    write_uleb(&mut descending, 0);
    vectors.push(Vector {
        name: "neg_unsorted_exports",
        bytes: descending,
        expect: code::EXPORT_ORDER,
        note: "export references in descending order",
    });

    // A repeated index is the same violation and the case a set would hide
    // most completely: decoding it twice yields one member either way.
    let mut duplicate = body_through_functions(&table);
    write_uleb(&mut duplicate, 0);
    write_uleb(&mut duplicate, 2);
    write_uleb(&mut duplicate, 1);
    write_uleb(&mut duplicate, 1);
    vectors.push(Vector {
        name: "neg_duplicate_exports",
        bytes: duplicate,
        expect: code::EXPORT_ORDER,
        note: "the same export reference twice",
    });

    let mut dangling = body_through_functions(&table);
    write_uleb(&mut dangling, 0);
    write_uleb(&mut dangling, 1);
    write_uleb(&mut dangling, 5);
    vectors.push(Vector {
        name: "neg_export_out_of_bounds",
        bytes: dangling,
        expect: code::INDEX_OUT_OF_BOUNDS,
        note: "export names string 5 of 2",
    });

    let mut short = body_through_functions(&table);
    write_uleb(&mut short, 0);
    write_uleb(&mut short, 2);
    write_uleb(&mut short, 0);
    vectors.push(Vector {
        name: "neg_truncated_exports",
        bytes: short,
        expect: code::TRUNCATED,
        note: "declares two exports and supplies one",
    });

    // The body stops exactly where next_id must begin. This pins that the
    // mirror now REQUIRES next_id rather than treating the end of the function
    // section as a complete body.
    vectors.push(Vector {
        name: "neg_truncated_before_next_id",
        bytes: body_through_functions(&table),
        expect: code::TRUNCATED,
        note: "body ends where next_id must begin",
    });
}
