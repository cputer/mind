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

use super::mirror_modules::{module_with_export_count, module_with_wide_next_id};
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
        expect: code::REMAINDER_REFUSED,
        note: "same body entire: prefix verified, remainder refused",
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

pub fn append(vectors: &mut Vec<Vector>) {
    append_emitter_positives(vectors);
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
