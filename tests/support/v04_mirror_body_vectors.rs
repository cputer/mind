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
        note: "same complete body: canonical bytes compared through the body end",
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

/// Complete bodies with exactly the requested module ConstI64 destinations.
/// An empty instruction list still carries one semantic row, which gives the
/// reference an authority-bearing, valid zero-instruction body.
fn complete_const_body(ids: &[u64]) -> Vec<u8> {
    let mut out = synthetic_head(&[]);
    write_uleb(&mut out, 0); // schema count
    write_uleb(&mut out, 0); // function count
    let next_id = ids.iter().copied().max().map_or(1, |id| id + 1);
    write_uleb(&mut out, next_id);
    write_uleb(&mut out, 0); // exports
    write_uleb(&mut out, ids.len() as u64);
    for id in ids {
        out.push(0x01); // ConstI64
        write_uleb(&mut out, *id);
        write_uleb(&mut out, 84); // zigzag(42)
    }
    for _ in 0..4 {
        write_uleb(&mut out, 0); // reserved compatibility counts
    }
    let rows = if ids.is_empty() { &[0][..] } else { ids };
    write_uleb(&mut out, rows.len() as u64);
    for id in rows {
        write_uleb(&mut out, *id);
        out.extend_from_slice(&[0x00, 0x01]); // Scalar(I64)
    }
    out
}

/// A zero-instruction body whose nonempty schema registry supplies canonical
/// authority without inventing a ValueId producer.
fn complete_zero_instruction_body() -> Vec<u8> {
    let mut out = synthetic_head(&["S", "m"]);
    write_uleb(&mut out, 1); // one schema header
    write_uleb(&mut out, 1); // owner -> "m"
    write_uleb(&mut out, 0); // name -> "S"
    write_uleb(&mut out, 0); // no fields
    write_uleb(&mut out, 0); // no function declarations
    write_uleb(&mut out, 0); // next_id
    write_uleb(&mut out, 0); // exports
    write_uleb(&mut out, 0); // zero instructions
    for _ in 0..4 {
        write_uleb(&mut out, 0); // reserved compatibility counts
    }
    write_uleb(&mut out, 0); // no module semantic rows
    out
}

/// Boundary and multi-byte ValueId positives. The two-instruction fixture is
/// deliberate: the reference charges 320 for each instruction, so this case
/// exercises the exact 640 logical charge rather than a single instruction.
fn append_instruction_boundary_vectors(vectors: &mut Vec<Vector>) {
    vectors.push(Vector {
        name: "pos_full_body_zero_instructions",
        bytes: complete_zero_instruction_body(),
        expect: code::OK_EXACT,
        note: "zero instructions with one authority-bearing module row",
    });
    vectors.push(Vector {
        name: "pos_full_body_one_instruction",
        bytes: complete_const_body(&[0]),
        expect: code::OK_EXACT,
        note: "one ConstI64 instruction: one 320-byte logical charge",
    });
    vectors.push(Vector {
        name: "pos_full_body_two_instructions",
        bytes: complete_const_body(&[0, 1]),
        expect: code::OK_EXACT,
        note: "two ConstI64 instructions: two 320-byte charges, exactly 640",
    });
    vectors.push(Vector {
        name: "pos_full_body_multibyte_value_id",
        bytes: complete_const_body(&[300]),
        expect: code::OK_EXACT,
        note: "ConstI64 destination and semantic row ValueId 300 use multi-byte ULEBs",
    });
}

/// A declared instruction count must be consumed as instructions. These
/// vectors stop at the count, so a decoder that silently treats a missing body
/// as an empty list would report a false positive.
fn append_instruction_count_negative(vectors: &mut Vec<Vector>) {
    let mut truncated = synthetic_head(&[]);
    write_uleb(&mut truncated, 0); // schema count
    write_uleb(&mut truncated, 0); // function count
    write_uleb(&mut truncated, 0); // next_id
    write_uleb(&mut truncated, 0); // exports
    write_uleb(&mut truncated, 2); // two instructions, no bytes follow
    vectors.push(Vector {
        name: "neg_instruction_count_exceeds_remaining",
        bytes: truncated,
        expect: code::TRUNCATED,
        note: "instruction count 2 exceeds the zero remaining instruction bytes",
    });
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

/// One FnDef instruction, optionally wrapping `inner` as its whole body.
fn fndef(name: usize, identity: usize, inner: Option<&[u8]>) -> Vec<u8> {
    let mut out = vec![0x15];
    write_uleb(&mut out, name as u64);
    write_uleb(&mut out, 0); // parameter count
    out.push(0); // ret_id absent
    out.push(0); // reap threshold absent
    match inner {
        None => write_uleb(&mut out, 0),
        Some(body) => {
            write_uleb(&mut out, 1);
            out.extend_from_slice(body);
        }
    }
    write_uleb(&mut out, 0); // reserved legacy ArrayType table
    write_uleb(&mut out, identity as u64);
    write_uleb(&mut out, 0); // scoped semantic rows
    out
}

/// A complete body whose single module instruction is a chain of `depth` nested
/// FnDefs. The outermost is read at depth 0, so the innermost is read at
/// `depth - 1`.
fn nested_fndef_body(depth: usize) -> Vec<u8> {
    let names: Vec<String> = (0..depth).map(|index| format!("f{index:03}")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut chain = fndef(depth - 1, depth - 1, None);
    for index in (0..depth - 1).rev() {
        chain = fndef(index, index, Some(&chain));
    }
    let mut table = name_refs;
    table.push("m");
    let mut out = synthetic_head(&table);
    write_uleb(&mut out, 0); // schema count
    write_uleb(&mut out, depth as u64); // function declarations
    for index in 0..depth {
        write_uleb(&mut out, depth as u64); // owner -> "m"
        write_uleb(&mut out, index as u64); // name -> "fNNN"
        out.push(0); // kind: local
        write_uleb(&mut out, 0); // signature parameters
        out.push(0); // no return type
    }
    write_uleb(&mut out, 0); // next_id
    write_uleb(&mut out, 0); // exports
    write_uleb(&mut out, 1); // one module instruction
    out.extend_from_slice(&chain);
    for _ in 0..4 {
        write_uleb(&mut out, 0);
    }
    write_uleb(&mut out, 0); // module semantic rows
    out
}

/// The nesting limit, at the threshold and one over.
///
/// The reference checks `depth >= 256` on ENTRY to each instruction, so a FnDef
/// read at depth 255 is admitted and only the first nested read at 256 refuses.
/// A mirror that refused the 255 case would be wrong in the safe-looking
/// direction, which is exactly why the accepted side is pinned too.
fn append_nesting_vectors(vectors: &mut Vec<Vector>) {
    vectors.push(Vector {
        name: "pos_full_body_nesting_at_limit",
        bytes: nested_fndef_body(256),
        expect: code::OK_EXACT,
        note: "256 nested FnDefs: innermost read at depth 255, the last admitted",
    });
    vectors.push(Vector {
        name: "neg_nesting_one_over_limit",
        bytes: nested_fndef_body(257),
        expect: code::INSTR_DEPTH,
        note: "257 nested FnDefs: innermost read at depth 256",
    });
}

pub fn append(vectors: &mut Vec<Vector>) {
    append_nesting_vectors(vectors);
    append_instruction_boundary_vectors(vectors);
    append_instruction_count_negative(vectors);
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
