// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Composes the input image consumed by the frozen pure-MIND compiler.
//!
//! Wire layout: `[8B user_lo LE][8B src_len LE][std_blob ++ user_region]`.
//! `user_lo` marks the first user byte; `src_len` covers the complete source image.
//! Standard-library modules retain manifest order and a trailing newline each.
//! User sources retain resolver order with a newline between sources only, keeping
//! a single source byte-identical to the original input layout.
//!
//! Order is semantic: the frozen compiler resolves duplicate bare names by their
//! position. The caller must reject collisions before emitting the image.

/// The std seed blob: every seed module's bytes, each followed by `\n`.
///
/// A newtype, not a bare `Vec<u8>`, because the seam is a correctness boundary
/// rather than a convention. Everything at or after `user_lo` is a prune ROOT to
/// the frozen compiler; everything before it is prunable std. If user text could
/// be passed where a std blob is expected, it would land below the seam and
/// silently stop being a root — changing which bodies the compiler keeps, with no
/// diagnostic. Making the only constructor [`compose_std_blob`] also guarantees
/// the per-module trailing `\n`: a blob assembled by hand without it would glue
/// its last std line to the first user line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdBlob(Vec<u8>);

impl StdBlob {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Compose the std seed blob: each module's bytes followed by `\n`.
pub fn compose_std_blob(modules: &[Vec<u8>]) -> StdBlob {
    let mut blob = Vec::new();
    for m in modules {
        blob.extend_from_slice(m);
        blob.push(b'\n');
    }
    StdBlob(blob)
}

/// The user region: sources in the caller's order, joined by a single `\n`.
///
/// The joiner goes BETWEEN sources only, so a single source is emitted verbatim —
/// that is what keeps the single-file path byte-identical, and a trailing newline
/// there would be exactly the silent one-byte drift the baselines exist to catch.
///
/// An EMPTY source is not special-cased, and the consequence is worth stating
/// because it is a bytes policy: `["a", ""]` composes `a\n` and `["", "a"]`
/// composes `\na`. Each empty element still contributes its joiner. That is
/// harmless to the frozen parser but it is observable, so a caller that could
/// produce empty sources must refuse them BEFORE composing rather than rely on
/// this function to absorb them. The bridge never does: its sources come from a
/// resolved file set, and an unreadable file is a hard refusal upstream.
pub fn compose_user_region(sources: &[Vec<u8>]) -> Vec<u8> {
    let mut region = Vec::new();
    for (i, src) in sources.iter().enumerate() {
        if i > 0 {
            region.push(b'\n');
        }
        region.extend_from_slice(src);
    }
    region
}

/// A composed image plus the two offsets the frozen driver needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeImage {
    pub bytes: Vec<u8>,
    pub user_lo: i64,
    pub src_len: i64,
}

/// Compose the full stdin image from a std blob and an ordered user source list.
pub fn compose_image(std_blob: &StdBlob, sources: &[Vec<u8>]) -> NativeImage {
    let user_region = compose_user_region(sources);
    let mut combined = Vec::with_capacity(std_blob.len() + user_region.len());
    combined.extend_from_slice(std_blob.as_bytes());
    let user_lo = combined.len() as i64;
    combined.extend_from_slice(&user_region);
    let src_len = combined.len() as i64;

    let mut bytes = Vec::with_capacity(16 + combined.len());
    bytes.extend_from_slice(&user_lo.to_le_bytes());
    bytes.extend_from_slice(&src_len.to_le_bytes());
    bytes.extend_from_slice(&combined);
    NativeImage {
        bytes,
        user_lo,
        src_len,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing property: with ONE source the joiner contributes nothing,
    /// so the composed bytes are exactly the pre-existing single-file bytes.
    #[test]
    fn one_source_composes_the_legacy_layout_exactly() {
        let blob = compose_std_blob(&[b"a".to_vec(), b"bb".to_vec()]);
        assert_eq!(blob.as_bytes(), b"a\nbb\n");
        let user = b"fn main() -> i64 { return 7; }".to_vec();

        let img = compose_image(&blob, std::slice::from_ref(&user));

        // Byte-for-byte reconstruction of what the bridge did before this module.
        let mut legacy = blob.as_bytes().to_vec();
        let legacy_user_lo = legacy.len() as i64;
        legacy.extend_from_slice(&user);
        let legacy_src_len = legacy.len() as i64;
        let mut expect = Vec::new();
        expect.extend_from_slice(&legacy_user_lo.to_le_bytes());
        expect.extend_from_slice(&legacy_src_len.to_le_bytes());
        expect.extend_from_slice(&legacy);

        assert_eq!(img.bytes, expect, "single-source bytes must not move");
        assert_eq!(img.user_lo, legacy_user_lo);
        assert_eq!(img.src_len, legacy_src_len);
    }

    /// `src_len` is the WHOLE image length, not the user length.
    #[test]
    fn src_len_covers_std_and_user() {
        let blob = compose_std_blob(&[b"xy".to_vec()]);
        let img = compose_image(&blob, &[b"abc".to_vec()]);
        assert_eq!(img.user_lo, 3, "std blob is 'xy' + newline");
        assert_eq!(img.src_len, 6, "3 std + 3 user, NOT 3");
        assert_eq!(&img.bytes[16..], b"xy\nabc");
    }

    /// Sources are joined by exactly one newline, between only — no trailer.
    #[test]
    fn sources_are_joined_by_a_single_newline_with_no_trailer() {
        let img = compose_image(
            &compose_std_blob(&[]),
            &[b"one".to_vec(), b"two".to_vec(), b"three".to_vec()],
        );
        assert_eq!(&img.bytes[16..], b"one\ntwo\nthree");
        assert_eq!(img.user_lo, 0);
        assert_eq!(img.src_len, 13);
    }

    /// Order is preserved EXACTLY, asserted against expected bytes.
    ///
    /// Asserting merely that two orders DIFFER would be tautological: a function
    /// that reversed its input would satisfy it. The frozen compiler resolves
    /// duplicate bare names last-definition-wins, so which order actually lands in
    /// the image decides which program runs.
    #[test]
    fn order_is_preserved_exactly() {
        let a = b"AAA".to_vec();
        let b = b"BBB".to_vec();
        let empty = compose_std_blob(&[]);
        assert_eq!(
            &compose_image(&empty, &[a.clone(), b.clone()]).bytes[16..],
            b"AAA\nBBB"
        );
        assert_eq!(&compose_image(&empty, &[b, a]).bytes[16..], b"BBB\nAAA");
    }

    /// The empty-source policy is PINNED rather than left to discovery: an empty
    /// element still contributes its joiner, so `["a", ""]` composes `a\n`. That
    /// is observable, so a caller which could produce empty sources must refuse
    /// them before composing.
    #[test]
    fn an_empty_source_still_contributes_its_joiner() {
        let empty = compose_std_blob(&[]);
        assert_eq!(
            &compose_image(&empty, &[b"a".to_vec(), b"".to_vec()]).bytes[16..],
            b"a\n"
        );
        assert_eq!(
            &compose_image(&empty, &[b"".to_vec(), b"a".to_vec()]).bytes[16..],
            b"\na"
        );
    }

    /// An empty source list is a legal composition of zero user bytes: user_lo
    /// and src_len coincide. The bridge refuses earlier, but the function must
    /// not panic or silently produce a trailing joiner.
    #[test]
    fn no_sources_yields_an_empty_user_region() {
        let blob = compose_std_blob(&[b"s".to_vec()]);
        let img = compose_image(&blob, &[]);
        assert_eq!(img.user_lo, 2);
        assert_eq!(img.src_len, 2);
        assert_eq!(&img.bytes[16..], b"s\n");
    }
}
