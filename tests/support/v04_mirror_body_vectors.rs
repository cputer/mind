// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Body-section differential vectors: module `next_id` and the export list.
//!
//! Each body below is well formed through the whole prefix -- header, string
//! table, an empty schema section and an empty function section -- so the only
//! thing separating it from a body the decoder would keep reading is the defect
//! named. Without that, a refusal could be owned by an earlier section and
//! these would prove nothing about the export walk.

use super::mirror_oracle::{synthetic_head, write_uleb};
use super::{Vector, code};

/// Header + sorted table + empty schema section + empty function section.
fn body_through_functions(strings: &[&str]) -> Vec<u8> {
    let mut out = synthetic_head(strings);
    write_uleb(&mut out, 0); // schema count
    write_uleb(&mut out, 0); // function count
    out
}

pub fn append(vectors: &mut Vec<Vector>) {
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
