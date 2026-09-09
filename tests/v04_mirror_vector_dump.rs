// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Development oracle for the pure-MIND MIC3 `0x04` prefix mirror.
//!
//! This test is a VECTOR GENERATOR, not a claim about the mirror. It builds
//! canonical modules, emits real `0x04` artifacts through the reference encoder,
//! independently re-derives the declared prefix (magic, version, required-surface
//! bits, string table) with a reader/writer written here rather than reused from
//! the codec, and asserts the two agree byte for byte. It then mutates the prefix
//! into one artifact per refusal class and records the reference decoder's verdict
//! for each.
//!
//! The committed manifest and fixtures are always checked against this oracle.
//! `MIND_V04_VECTOR_DIR` only selects an additional dump destination for callers
//! that need regenerated files; it cannot turn the corpus check into a no-op.

use std::{collections::BTreeSet, path::PathBuf};

use libmind::ir::compact::v3::{emit_mic3_checked, parse_mic3_prefix};
#[path = "support/v04_mirror_descriptor_vectors.rs"]
mod descriptor_vectors;
#[path = "support/v04_mirror_intrinsic_vectors.rs"]
mod intrinsic_vectors;
#[path = "support/v04_mirror_modules.rs"]
mod mirror_modules;
#[path = "support/v04_mirror_oracle.rs"]
mod mirror_oracle;

use mirror_modules::{
    module_scoped, module_value_only, module_with_export_name_len, prefix_has_multibyte_uleb,
};
use mirror_oracle::{
    read_uleb, rederive_prefix, sha256_hex, strings_of, synthetic_head, write_uleb,
};
const MIRROR_MAGIC: [u8; 4] = *b"MIC3";
const MIRROR_VERSION: u8 = 0x04;

// ---------------------------------------------------------------------------
// Vector emission
// ---------------------------------------------------------------------------

struct Vector {
    name: &'static str,
    bytes: Vec<u8>,
    /// Expected mirror exit code, decided here (by the reference), not by the harness.
    expect: u32,
    note: &'static str,
}

/// The exact prefix length the REFERENCE decoder consumes for `bytes`, or 0 when
/// the reference refuses it.
///
/// This exists because comparing a program's output against
/// `input[..output.len()]` is self-lengthed: a mirror that emitted a SHORTER but
/// correct prefix would satisfy it. The expected length has to come from
/// somewhere the mirror cannot influence, so it is derived here and published in
/// the manifest for the harness to enforce.
fn reference_consumed_len(bytes: &[u8]) -> usize {
    match rederive_prefix(bytes) {
        Ok((_, prefix_len, _)) => prefix_len,
        Err(_) => 0,
    }
}

fn manifest_for(vectors: &[Vector]) -> String {
    let mut manifest = String::from("# name\texpected_exit\tbytes\tsha256\tconsumed\tnote\n");
    for vector in vectors {
        manifest.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            vector.name,
            vector.expect,
            vector.bytes.len(),
            sha256_hex(&vector.bytes),
            reference_consumed_len(&vector.bytes),
            vector.note
        ));
    }
    manifest
}

/// Check the tracked corpus against the same bytes, verdicts, and consumed
/// lengths that were derived above.  Comparing the complete manifest catches
/// changed or missing rows; comparing every shipped file catches a fixture
/// whose manifest was edited to agree with it.  Recipe rows are intentionally
/// represented only by their oracle digest and size.
fn validate_committed_corpus(vectors: &[Vector], expected_manifest: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("examples/mind_mirror_v04/testdata");
    let actual_manifest =
        std::fs::read_to_string(dir.join("MANIFEST.tsv")).expect("committed v04 vector manifest");
    assert_eq!(
        actual_manifest, expected_manifest,
        "committed v04 manifest differs from the reference oracle"
    );
    let expected_files: BTreeSet<String> = vectors
        .iter()
        .filter(|vector| !vector.note.starts_with("recipe="))
        .map(|vector| format!("{}.mic3", vector.name))
        .collect();
    let actual_files: BTreeSet<String> = std::fs::read_dir(&dir)
        .expect("committed v04 vector directory")
        .map(|entry| {
            entry
                .expect("committed v04 vector directory entry")
                .file_name()
        })
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.ends_with(".mic3"))
        .collect();
    assert_eq!(
        actual_files, expected_files,
        "committed v04 fixture set differs from the reference oracle"
    );
    for vector in vectors {
        if vector.note.starts_with("recipe=") {
            continue;
        }
        let path = dir.join(format!("{}.mic3", vector.name));
        let actual = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("committed v04 fixture {}: {error}", path.display()));
        assert_eq!(
            actual, vector.bytes,
            "committed v04 fixture {} differs from the reference oracle",
            vector.name
        );
    }
}

/// Mirror exit-code vocabulary. Kept in one place so the .mind program and this
/// generator cannot drift silently: the harness compares against these numbers.
mod code {
    pub const OK_EXACT: u32 = 0;
    pub const TRUNCATED: u32 = 10;
    pub const BAD_MAGIC: u32 = 11;
    pub const WRONG_VERSION: u32 = 12;
    pub const NON_MINIMAL_ULEB: u32 = 13;
    pub const OVERLONG_ULEB: u32 = 14;
    pub const UNKNOWN_SURFACE: u32 = 15;
    pub const STD_SURFACE_REQUIRED: u32 = 16;
    pub const OVERSIZE: u32 = 17;
    pub const STRING_TABLE_ORDER: u32 = 19;
    pub const REMAINDER_REFUSED: u32 = 20;
    pub const BAD_UTF8: u32 = 21;
    pub const INDEX_OUT_OF_BOUNDS: u32 = 23;
    pub const UNKNOWN_TYPE_TAG: u32 = 25;
    pub const SCHEMA_ORDER: u32 = 26;
    pub const BAD_IDENTITY: u32 = 27;
    pub const DUPLICATE_FIELD: u32 = 28;
    pub const UNKNOWN_FUNCTION_KIND: u32 = 29;
    pub const FUNCTION_ORDER: u32 = 30;
    pub const RESERVED_OWNER: u32 = 31;
    pub const BAD_OPTIONAL_TAG: u32 = 32;
    pub const DESCRIPTOR_ELEMENTS: u32 = 33;
    pub const INTRINSIC_CONTRACT: u32 = 34;
}
fn splice_surface(prefix: &[u8], replacement: &[u8]) -> Vec<u8> {
    // The surface field starts at offset 5; find its end by walking continuations.
    let mut end = 5usize;
    while prefix[end] >= 0x80 {
        end += 1;
    }
    end += 1;
    let mut out = prefix[..5].to_vec();
    out.extend_from_slice(replacement);
    out.extend_from_slice(&prefix[end..]);
    out
}

#[test]
fn v04_prefix_vectors() {
    let mut vectors: Vec<Vector> = Vec::new();

    // --- positive control: the reference encoder really produces 0x04 ---
    let full_small = emit_mic3_checked(&module_value_only()).expect("v0x04 body (value only)");
    assert_eq!(full_small[0..4], MIRROR_MAGIC, "magic");
    assert_eq!(full_small[4], MIRROR_VERSION, "version byte");
    let full_scoped = emit_mic3_checked(&module_scoped()).expect("v0x04 body (scoped)");
    assert_eq!(full_scoped[4], MIRROR_VERSION, "version byte");
    assert!(
        parse_mic3_prefix(&full_small).is_ok(),
        "reference decoder accepts its own small body"
    );
    assert!(
        parse_mic3_prefix(&full_scoped).is_ok(),
        "reference decoder accepts its own scoped body"
    );

    // --- independent re-derivation of the declared prefix ---
    let (prefix_small, len_small, strings_small) =
        rederive_prefix(&full_small).expect("small prefix");
    let (prefix_scoped, len_scoped, strings_scoped) =
        rederive_prefix(&full_scoped).expect("scoped prefix");
    assert_eq!(
        prefix_small,
        full_small[..len_small],
        "independently re-derived prefix must equal the encoder's own bytes (small)"
    );
    assert_eq!(
        prefix_scoped,
        full_scoped[..len_scoped],
        "independently re-derived prefix must equal the encoder's own bytes (scoped)"
    );
    // The scoped fixture must actually exercise the string table, or the
    // string-table half of the mirror is never measured by it.
    assert_eq!(strings_small, 0, "value-only module has no strings");
    assert!(
        strings_scoped >= 6,
        "scoped fixture must carry a real string table, got {strings_scoped}"
    );

    // Positives: a prefix-only artifact is fully certifiable (exit 0); the full
    // body is prefix-identical but carries content past the declared subset.
    vectors.push(Vector {
        name: "pos_prefix_empty_strings",
        bytes: prefix_small.clone(),
        expect: code::OK_EXACT,
        note: "header + empty string table, nothing beyond the declared subset",
    });
    vectors.push(Vector {
        name: "pos_prefix_scoped",
        bytes: prefix_scoped.clone(),
        expect: code::OK_EXACT,
        note: "header + 6+ sorted strings, nothing beyond the declared subset",
    });
    vectors.push(Vector {
        name: "pos_full_body_small",
        bytes: full_small.clone(),
        expect: code::REMAINDER_REFUSED,
        note: "real encoder body: prefix verified, remainder explicitly refused",
    });
    vectors.push(Vector {
        name: "pos_full_body_scoped",
        bytes: full_scoped.clone(),
        expect: code::REMAINDER_REFUSED,
        note: "real encoder body: prefix verified, remainder explicitly refused",
    });

    // A real encoder body whose string table carries a 200-byte entry, so the
    // length field is a MULTI-BYTE ULEB. Without this the whole corpus encodes
    // every count and length in one byte and a wrong-shift encoder passes.
    let full_long = emit_mic3_checked(&module_with_export_name_len(200))
        .expect("v0x04 body (long export name)");
    assert_eq!(full_long[4], MIRROR_VERSION, "version byte");
    let (prefix_long, len_long, strings_long) = rederive_prefix(&full_long).expect("long prefix");
    assert_eq!(
        prefix_long,
        full_long[..len_long],
        "independently re-derived prefix must equal the encoder's own bytes (long)"
    );
    assert!(strings_long >= 1, "long fixture must carry a string");
    assert!(
        prefix_has_multibyte_uleb(&prefix_long),
        "the long fixture exists to exercise a multi-byte ULEB; it does not"
    );
    assert!(
        !prefix_has_multibyte_uleb(&prefix_scoped),
        "the scoped fixture is expected to be single-byte throughout, so the \
         long fixture is the only source of multi-byte coverage"
    );
    vectors.push(Vector {
        name: "pos_prefix_long_string",
        bytes: prefix_long.clone(),
        expect: code::OK_EXACT,
        note: "header + one 200-byte string: multi-byte ULEB length field",
    });
    vectors.push(Vector {
        name: "pos_full_body_long_string",
        bytes: full_long.clone(),
        expect: code::REMAINDER_REFUSED,
        note: "real encoder body with a multi-byte ULEB string length",
    });
    descriptor_vectors::append(&mut vectors);
    // --- negatives, each mutated from a positive ---
    let mut bad_magic = prefix_scoped.clone();
    bad_magic[3] = b'4';
    vectors.push(Vector {
        name: "neg_bad_magic",
        bytes: bad_magic,
        expect: code::BAD_MAGIC,
        note: "MIC3 -> MIC4",
    });

    let mut wrong_version = prefix_scoped.clone();
    wrong_version[4] = 0x03;
    vectors.push(Vector {
        name: "neg_wrong_version",
        bytes: wrong_version,
        expect: code::WRONG_VERSION,
        note: "version byte 0x03",
    });

    vectors.push(Vector {
        name: "neg_non_minimal_surface",
        bytes: splice_surface(&prefix_scoped, &[0x80, 0x00]),
        expect: code::NON_MINIMAL_ULEB,
        note: "surface bits encoded as 0x80 0x00 instead of 0x00",
    });

    vectors.push(Vector {
        name: "neg_overlong_surface",
        bytes: splice_surface(&prefix_scoped, &[0x80; 12]),
        expect: code::OVERLONG_ULEB,
        note: "twelve continuation bytes, never terminated",
    });

    vectors.push(Vector {
        name: "neg_unknown_surface_bits",
        bytes: splice_surface(&prefix_scoped, &[0x04]),
        expect: code::UNKNOWN_SURFACE,
        note: "bit 2 is outside KNOWN_SURFACE_BITS",
    });

    vectors.push(Vector {
        name: "neg_std_surface_required",
        bytes: splice_surface(&prefix_scoped, &[0x01]),
        expect: code::STD_SURFACE_REQUIRED,
        note: "REQUIRED_STD_SURFACE set: known bit, refused by the core codec",
    });

    vectors.push(Vector {
        name: "neg_truncated_header",
        bytes: prefix_scoped[..4].to_vec(),
        expect: code::TRUNCATED,
        note: "magic only, no version byte",
    });

    vectors.push(Vector {
        name: "neg_truncated_string_bytes",
        bytes: prefix_scoped[..prefix_scoped.len() - 3].to_vec(),
        expect: code::TRUNCATED,
        note: "last string's bytes cut short",
    });

    // Unsorted / duplicate string table: swap the first two entries' bytes by
    // rebuilding the table in descending order.
    {
        let mut pos = 5usize;
        let surface = read_uleb(&prefix_scoped, &mut pos).expect("surface");
        let count = read_uleb(&prefix_scoped, &mut pos).expect("count") as usize;
        let mut strings = Vec::new();
        for _ in 0..count {
            let length = read_uleb(&prefix_scoped, &mut pos).expect("len") as usize;
            strings.push(prefix_scoped[pos..pos + length].to_vec());
            pos += length;
        }
        assert!(count >= 2, "need two strings to unsort");
        strings.swap(0, 1);
        let mut out = Vec::new();
        out.extend_from_slice(&MIRROR_MAGIC);
        out.push(MIRROR_VERSION);
        write_uleb(&mut out, surface);
        write_uleb(&mut out, count as u64);
        for text in &strings {
            write_uleb(&mut out, text.len() as u64);
            out.extend_from_slice(text);
        }
        vectors.push(Vector {
            name: "neg_unsorted_strings",
            bytes: out,
            expect: code::STRING_TABLE_ORDER,
            note: "first two string-table entries transposed",
        });

        let mut dup = Vec::new();
        dup.extend_from_slice(&MIRROR_MAGIC);
        dup.push(MIRROR_VERSION);
        write_uleb(&mut dup, surface);
        write_uleb(&mut dup, 2);
        for _ in 0..2 {
            write_uleb(&mut dup, strings[0].len() as u64);
            dup.extend_from_slice(&strings[0]);
        }
        vectors.push(Vector {
            name: "neg_duplicate_strings",
            bytes: dup,
            expect: code::STRING_TABLE_ORDER,
            note: "the same entry twice: sorted-and-unique requires strict increase",
        });

        let mut bad_utf8 = Vec::new();
        bad_utf8.extend_from_slice(&MIRROR_MAGIC);
        bad_utf8.push(MIRROR_VERSION);
        write_uleb(&mut bad_utf8, surface);
        write_uleb(&mut bad_utf8, 1);
        write_uleb(&mut bad_utf8, 2);
        bad_utf8.extend_from_slice(&[0xc3, 0x28]);
        vectors.push(Vector {
            name: "neg_bad_utf8_string",
            bytes: bad_utf8,
            expect: code::BAD_UTF8,
            note: "0xc3 0x28 is a truncated two-byte sequence",
        });

        let mut huge_len = Vec::new();
        huge_len.extend_from_slice(&MIRROR_MAGIC);
        huge_len.push(MIRROR_VERSION);
        write_uleb(&mut huge_len, surface);
        write_uleb(&mut huge_len, 1);
        write_uleb(&mut huge_len, 1 << 40);
        huge_len.extend_from_slice(b"ab");
        vectors.push(Vector {
            name: "neg_string_length_overruns",
            bytes: huge_len,
            expect: code::TRUNCATED,
            note: "declared string length 2^40 with two bytes present",
        });

        let mut huge_count = Vec::new();
        huge_count.extend_from_slice(&MIRROR_MAGIC);
        huge_count.push(MIRROR_VERSION);
        write_uleb(&mut huge_count, surface);
        write_uleb(&mut huge_count, 1 << 40);
        vectors.push(Vector {
            name: "neg_string_count_overruns",
            bytes: huge_count,
            expect: code::TRUNCATED,
            note: "declared count 2^40 exceeds the remaining bytes",
        });
    }

    // Oversize: one byte past MAX_MIC3_INPUT (10 MiB).
    {
        let mut oversize = prefix_scoped.clone();
        oversize.resize(10 * 1024 * 1024 + 1, 0);
        vectors.push(Vector {
            name: "neg_oversize_input",
            bytes: oversize,
            expect: code::OVERSIZE,
            // Declared RECIPE rather than a shipped 10 MiB binary. The harness
            // rebuilds these bytes in scratch and verifies the size and digest
            // recorded in the manifest before using them, so the fixture is
            // still pinned without a large permanent blob in the repository.
            note: "recipe=pad:pos_prefix_scoped:10485761 (10 MiB + 1 bytes)",
        });
    }

    // --- schema-section negatives ------------------------------------------
    //
    // Each grafts a hand-built schema section onto a REAL header and string
    // table, so the only difference from an accepted body is the defect named.
    // Without these the schema controls this slice adds are decoration: the
    // positives above exercise the accept path only, and every one of these
    // refusals is owned by a distinct named cause rather than a bare nonzero.
    {
        let (strings_end, table) = strings_of(&prefix_scoped);
        let head = prefix_scoped[..strings_end].to_vec();
        let index_of = |needle: &str| -> u64 {
            table
                .iter()
                .position(|entry| entry == needle.as_bytes())
                .unwrap_or_else(|| panic!("the scoped table must contain {needle}"))
                as u64
        };
        let owner_a = index_of("ownerA");
        let owner_b = index_of("ownerB");
        let pair = index_of("Pair");
        let zeta = index_of("zeta");
        let alpha = index_of("alpha");
        let scalar_pair_fields = |out: &mut Vec<u8>| {
            write_uleb(out, 2);
            write_uleb(out, zeta);
            out.push(0);
            out.push(1);
            write_uleb(out, alpha);
            out.push(0);
            out.push(5);
        };

        let mut descending = head.clone();
        write_uleb(&mut descending, 2);
        write_uleb(&mut descending, owner_b);
        write_uleb(&mut descending, pair);
        write_uleb(&mut descending, owner_a);
        write_uleb(&mut descending, pair);
        scalar_pair_fields(&mut descending);
        scalar_pair_fields(&mut descending);
        vectors.push(Vector {
            name: "neg_unsorted_schemas",
            bytes: descending,
            expect: code::SCHEMA_ORDER,
            note: "schema identities in descending order",
        });

        let mut duplicate = head.clone();
        write_uleb(&mut duplicate, 1);
        write_uleb(&mut duplicate, owner_a);
        write_uleb(&mut duplicate, pair);
        write_uleb(&mut duplicate, 2);
        write_uleb(&mut duplicate, zeta);
        duplicate.push(0);
        duplicate.push(1);
        write_uleb(&mut duplicate, zeta);
        duplicate.push(0);
        duplicate.push(1);
        vectors.push(Vector {
            name: "neg_duplicate_field",
            bytes: duplicate,
            expect: code::DUPLICATE_FIELD,
            note: "one schema declares the same field name twice",
        });

        let mut unknown_type = head.clone();
        write_uleb(&mut unknown_type, 1);
        write_uleb(&mut unknown_type, owner_a);
        write_uleb(&mut unknown_type, pair);
        write_uleb(&mut unknown_type, 1);
        write_uleb(&mut unknown_type, zeta);
        unknown_type.push(4);
        vectors.push(Vector {
            name: "neg_unknown_type_tag",
            bytes: unknown_type,
            expect: code::UNKNOWN_TYPE_TAG,
            note: "type descriptor tag 4 is outside the core codec",
        });

        let mut unknown_scalar = head.clone();
        write_uleb(&mut unknown_scalar, 1);
        write_uleb(&mut unknown_scalar, owner_a);
        write_uleb(&mut unknown_scalar, pair);
        write_uleb(&mut unknown_scalar, 1);
        write_uleb(&mut unknown_scalar, zeta);
        unknown_scalar.push(0);
        unknown_scalar.push(9);
        vectors.push(Vector {
            name: "neg_unknown_scalar_tag",
            bytes: unknown_scalar,
            expect: code::UNKNOWN_TYPE_TAG,
            note: "scalar tag 9 is outside the nine core scalars",
        });

        let mut dangling = head.clone();
        write_uleb(&mut dangling, 1);
        write_uleb(&mut dangling, owner_a);
        write_uleb(&mut dangling, pair);
        write_uleb(&mut dangling, 1);
        write_uleb(&mut dangling, zeta);
        dangling.push(1);
        write_uleb(&mut dangling, 5);
        vectors.push(Vector {
            name: "neg_schema_ref_out_of_bounds",
            bytes: dangling,
            expect: code::INDEX_OUT_OF_BOUNDS,
            note: "record reference names schema 5 of 1",
        });

        let mut short = head.clone();
        write_uleb(&mut short, 2);
        write_uleb(&mut short, owner_a);
        write_uleb(&mut short, pair);
        vectors.push(Vector {
            name: "neg_truncated_schema_headers",
            bytes: short,
            expect: code::TRUNCATED,
            note: "declares two schemas and supplies one header",
        });

        // The defect must live in a STRING, so these carry their own table.
        let mut path_like = synthetic_head(&["A..B", "Pair", "f"]);
        write_uleb(&mut path_like, 1);
        write_uleb(&mut path_like, 0);
        write_uleb(&mut path_like, 1);
        write_uleb(&mut path_like, 1);
        write_uleb(&mut path_like, 2);
        path_like.push(0);
        path_like.push(1);
        vectors.push(Vector {
            name: "neg_path_like_schema_owner",
            bytes: path_like,
            expect: code::BAD_IDENTITY,
            note: "schema owner contains a parent-directory spelling",
        });

        let mut empty_owner = synthetic_head(&["", "Pair", "f"]);
        write_uleb(&mut empty_owner, 1);
        write_uleb(&mut empty_owner, 0);
        write_uleb(&mut empty_owner, 1);
        write_uleb(&mut empty_owner, 1);
        write_uleb(&mut empty_owner, 2);
        empty_owner.push(0);
        empty_owner.push(1);
        vectors.push(Vector {
            name: "neg_empty_schema_owner",
            bytes: empty_owner,
            expect: code::BAD_IDENTITY,
            note: "schema owner is the empty string",
        });
    }

    // --- function-declaration negatives -------------------------------------
    //
    // These carry their own header and an EMPTY schema section, so the function
    // section starts immediately and the defect under test is the only thing
    // separating them from a body the decoder would keep reading.
    {
        let table = ["__mind_intrinsic", "entry", "main", "other"];
        let reserved = 0_u64;
        let entry = 1_u64;
        let main = 2_u64;
        let other = 3_u64;
        let declaration = |out: &mut Vec<u8>, owner: u64, name: u64, kind: u8| {
            write_uleb(out, owner);
            write_uleb(out, name);
            out.push(kind);
            write_uleb(out, 0);
            out.push(0);
        };

        let mut descending = synthetic_head(&table);
        write_uleb(&mut descending, 0);
        write_uleb(&mut descending, 2);
        declaration(&mut descending, other, main, 0);
        declaration(&mut descending, entry, main, 0);
        vectors.push(Vector {
            name: "neg_unsorted_functions",
            bytes: descending,
            expect: code::FUNCTION_ORDER,
            note: "function identities in descending order",
        });

        let mut bad_kind = synthetic_head(&table);
        write_uleb(&mut bad_kind, 0);
        write_uleb(&mut bad_kind, 1);
        write_uleb(&mut bad_kind, entry);
        write_uleb(&mut bad_kind, main);
        bad_kind.push(3);
        vectors.push(Vector {
            name: "neg_unknown_function_kind",
            bytes: bad_kind,
            expect: code::UNKNOWN_FUNCTION_KIND,
            note: "function kind tag 3 is outside local/external/intrinsic",
        });

        let mut fake_intrinsic = synthetic_head(&table);
        write_uleb(&mut fake_intrinsic, 0);
        write_uleb(&mut fake_intrinsic, 1);
        declaration(&mut fake_intrinsic, entry, main, 2);
        vectors.push(Vector {
            name: "neg_intrinsic_without_reserved_owner",
            bytes: fake_intrinsic,
            expect: code::RESERVED_OWNER,
            note: "intrinsic kind under an ordinary owner",
        });

        let mut squatted = synthetic_head(&table);
        write_uleb(&mut squatted, 0);
        write_uleb(&mut squatted, 1);
        declaration(&mut squatted, reserved, main, 0);
        vectors.push(Vector {
            name: "neg_reserved_owner_not_intrinsic",
            bytes: squatted,
            expect: code::RESERVED_OWNER,
            note: "ordinary kind wearing the reserved intrinsic owner",
        });

        let mut bad_tag = synthetic_head(&table);
        write_uleb(&mut bad_tag, 0);
        write_uleb(&mut bad_tag, 1);
        write_uleb(&mut bad_tag, entry);
        write_uleb(&mut bad_tag, main);
        bad_tag.push(0);
        write_uleb(&mut bad_tag, 0);
        bad_tag.push(2);
        vectors.push(Vector {
            name: "neg_bad_return_present_tag",
            bytes: bad_tag,
            expect: code::BAD_OPTIONAL_TAG,
            note: "return-present tag is neither 0 nor 1",
        });

        // Proves the identity predicate is called on the FUNCTION side too, not
        // only from the schema headers.
        let mut path_like = synthetic_head(&["A..B", "main"]);
        write_uleb(&mut path_like, 0);
        write_uleb(&mut path_like, 1);
        declaration(&mut path_like, 0, 1, 0);
        vectors.push(Vector {
            name: "neg_path_like_function_owner",
            bytes: path_like,
            expect: code::BAD_IDENTITY,
            note: "function owner contains a parent-directory spelling",
        });
    }

    intrinsic_vectors::add_intrinsic_vectors(&mut vectors);

    // --- reference-decoder cross-check on every vector -------------------
    //
    // Positive control for the negatives: each malformed vector must be REFUSED
    // by the reference decoder, and each positive must be ACCEPTED. Without this
    // a "negative" could be a well-formed artifact the mirror happened to reject.
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for vector in &vectors {
        let verdict = parse_mic3_prefix(&vector.bytes);
        let should_accept = vector.name.starts_with("pos_");
        if should_accept {
            // The prefix-only vectors are truncated bodies: the reference parses
            // the WHOLE body, so only the full-body vectors can be accepted by it.
            if vector.name.starts_with("pos_full_body") {
                assert!(
                    verdict.is_ok(),
                    "{} must parse under the reference decoder",
                    vector.name
                );
                accepted += 1;
            } else {
                assert!(
                    verdict.is_err(),
                    "{} is a deliberate prefix slice; the whole-body reference \
                     decoder is expected to want more",
                    vector.name
                );
                refused += 1;
            }
        } else {
            assert!(
                verdict.is_err(),
                "{} must be refused by the reference decoder, else it is not a \
                 negative at all",
                vector.name
            );
            refused += 1;
        }
    }
    assert_eq!(accepted, 5, "five full-body positives");
    assert!(refused >= 14, "at least fourteen refusals, got {refused}");

    let manifest = manifest_for(&vectors);
    validate_committed_corpus(&vectors, &manifest);

    // A vector whose note declares a RECIPE is not shipped as a file. The
    // harness rebuilds it in scratch from the recipe and checks it against the
    // size and digest recorded in the manifest, so it stays pinned without a
    // large permanent binary in the repository.
    if let Some(dir) = std::env::var_os("MIND_V04_VECTOR_DIR").map(PathBuf::from) {
        std::fs::create_dir_all(&dir).expect("vector dir");
        for vector in &vectors {
            if !vector.note.starts_with("recipe=") {
                let path = dir.join(format!("{}.mic3", vector.name));
                std::fs::write(&path, &vector.bytes).expect("write vector");
            }
        }
        std::fs::write(dir.join("MANIFEST.tsv"), &manifest).expect("manifest");
        eprintln!("wrote {} vectors to {}", vectors.len(), dir.display());
    } else {
        eprintln!(
            "validated {} committed v04 vectors against the reference oracle",
            vectors.len()
        );
    }
}
