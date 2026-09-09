// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Independent reader/writer and digest helpers for the MIC3 v0x04 mirror oracle.

use super::{MIRROR_MAGIC, MIRROR_VERSION};

type ReDerivedFunction = (usize, usize, u8, Vec<Vec<u8>>, Option<Vec<u8>>);

// ---------------------------------------------------------------------------
// An independent reader/writer for the declared prefix.
//
// Deliberately NOT the codec's own helpers: if this shared the encoder's ULEB
// routine or its string-table walk, an agreement between the two would be an
// agreement of one implementation with itself.
// ---------------------------------------------------------------------------

pub(super) fn read_uleb(bytes: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut value: u64 = 0;
    let mut index = 0usize;
    loop {
        if *pos >= bytes.len() {
            return Err("truncated".to_string());
        }
        let byte = bytes[*pos];
        *pos += 1;
        if index >= 9 {
            return Err("overlong".to_string());
        }
        value += u64::from(byte & 0x7f) * 128u64.pow(index as u32);
        if byte < 0x80 {
            if index != 0 && byte == 0 {
                return Err("non-minimal".to_string());
            }
            return Ok(value);
        }
        index += 1;
    }
}

pub(super) fn write_uleb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let low = (value % 128) as u8;
        value /= 128;
        if value == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// Shared wire-safe identity predicate, re-derived here rather than imported so
/// that the oracle does not inherit a mistake from the code it is checking.
pub(super) fn identity_component_ok(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && !bytes.contains(&b'/')
        && !bytes.contains(&b'\\')
        && !bytes.windows(2).any(|pair| pair == b"..")
}

/// Walk one type descriptor, appending its canonical bytes to `out`.
pub(super) fn rederive_type(
    body: &[u8],
    pos: &mut usize,
    schema_count: usize,
    out: &mut Vec<u8>,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("type descriptor nests past 64".to_string());
    }
    let tag = *body.get(*pos).ok_or("type tag past the body")?;
    *pos += 1;
    out.push(tag);
    match tag {
        0 => {
            let scalar = *body.get(*pos).ok_or("scalar tag past the body")?;
            *pos += 1;
            if scalar > 8 {
                return Err(format!("unknown scalar tag {scalar}"));
            }
            out.push(scalar);
            Ok(())
        }
        1 => {
            let index = read_uleb(body, pos)? as usize;
            if index >= schema_count {
                return Err("record reference names no schema".to_string());
            }
            write_uleb(out, index as u64);
            Ok(())
        }
        2 => {
            let extent = read_uleb(body, pos)?;
            if extent > u64::from(u32::MAX) {
                return Err("fixed-array extent exceeds u32::MAX".to_string());
            }
            write_uleb(out, extent);
            rederive_type(body, pos, schema_count, out, depth + 1)
        }
        3 => rederive_type(body, pos, schema_count, out, depth + 1),
        other => Err(format!("unknown type tag {other}")),
    }
}

/// The offset just past the string table, and the table itself. Used only to
/// graft a hand-built schema section onto a real header, so the negatives below
/// differ from an accepted body in exactly the section under test.
pub(super) fn strings_of(body: &[u8]) -> (usize, Vec<Vec<u8>>) {
    let mut pos = 5_usize;
    let _surface = read_uleb(body, &mut pos).expect("surface");
    let count = read_uleb(body, &mut pos).expect("string count") as usize;
    let mut strings = Vec::new();
    for _ in 0..count {
        let length = read_uleb(body, &mut pos).expect("string length") as usize;
        strings.push(body[pos..pos + length].to_vec());
        pos += length;
    }
    (pos, strings)
}

/// A synthetic header plus a sorted string table, for negatives whose defect
/// must live in a string the real fixtures do not contain.
pub(super) fn synthetic_head(strings: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MIRROR_MAGIC);
    out.push(MIRROR_VERSION);
    write_uleb(&mut out, 0);
    write_uleb(&mut out, strings.len() as u64);
    for text in strings {
        write_uleb(&mut out, text.len() as u64);
        out.extend_from_slice(text.as_bytes());
    }
    out
}

/// Returns `(prefix_bytes, prefix_len, string_count)` for `body`.
pub(super) fn rederive_prefix(body: &[u8]) -> Result<(Vec<u8>, usize, usize), String> {
    if body.len() < 5 {
        return Err("shorter than a header".to_string());
    }
    if body[0..4] != MIRROR_MAGIC {
        return Err("bad magic".to_string());
    }
    if body[4] != MIRROR_VERSION {
        return Err(format!("version {:#04x}", body[4]));
    }
    let mut pos = 5usize;
    let surface = read_uleb(body, &mut pos)?;
    let count = read_uleb(body, &mut pos)? as usize;
    let mut strings: Vec<Vec<u8>> = Vec::new();
    for _ in 0..count {
        let length = read_uleb(body, &mut pos)? as usize;
        if pos + length > body.len() {
            return Err("string runs past the body".to_string());
        }
        let text = body[pos..pos + length].to_vec();
        pos += length;
        if std::str::from_utf8(&text).is_err() {
            return Err("string table is not UTF-8".to_string());
        }
        if strings
            .last()
            .is_some_and(|previous| previous[..] >= text[..])
        {
            return Err("string table is not strictly sorted".to_string());
        }
        strings.push(text);
    }
    // --- schema identity headers ---
    //
    // Order is checked on the identity BYTES here, deliberately unlike the
    // mirror, which compares string indices. Both are correct given a unique
    // sorted string table, and checking them differently is the point: two
    // implementations that agree by construction would not be evidence.
    let schema_count = read_uleb(body, &mut pos)? as usize;
    let mut headers: Vec<(usize, usize)> = Vec::new();
    for _ in 0..schema_count {
        let owner = read_uleb(body, &mut pos)? as usize;
        let name = read_uleb(body, &mut pos)? as usize;
        let owner_bytes = strings
            .get(owner)
            .ok_or_else(|| "schema owner names no string".to_string())?;
        let name_bytes = strings
            .get(name)
            .ok_or_else(|| "schema name names no string".to_string())?;
        if !identity_component_ok(owner_bytes) || !identity_component_ok(name_bytes) {
            return Err("schema identity component is empty or path-like".to_string());
        }
        if let Some((previous_owner, previous_name)) = headers.last().copied() {
            if (
                strings[previous_owner].as_slice(),
                strings[previous_name].as_slice(),
            ) >= (owner_bytes.as_slice(), name_bytes.as_slice())
            {
                return Err("schema identities are not strictly sorted".to_string());
            }
        }
        headers.push((owner, name));
    }

    // --- schema field lists, in header order ---
    let mut field_lists: Vec<Vec<(usize, Vec<u8>)>> = Vec::new();
    for _ in 0..schema_count {
        let field_count = read_uleb(body, &mut pos)? as usize;
        let mut fields: Vec<(usize, Vec<u8>)> = Vec::new();
        for _ in 0..field_count {
            let name = read_uleb(body, &mut pos)? as usize;
            let name_bytes = strings
                .get(name)
                .ok_or_else(|| "field names no string".to_string())?;
            if name_bytes.is_empty() {
                return Err("field name is empty".to_string());
            }
            if fields.iter().any(|(seen, _)| *seen == name) {
                return Err("schema declares a duplicate field name".to_string());
            }
            let mut descriptor = Vec::new();
            rederive_type(body, &mut pos, schema_count, &mut descriptor, 0)?;
            fields.push((name, descriptor));
        }
        field_lists.push(fields);
    }

    // --- function declarations ---
    let function_count = read_uleb(body, &mut pos)? as usize;
    let mut functions: Vec<ReDerivedFunction> = Vec::new();
    for _ in 0..function_count {
        let owner = read_uleb(body, &mut pos)? as usize;
        let name = read_uleb(body, &mut pos)? as usize;
        let owner_bytes = strings
            .get(owner)
            .ok_or_else(|| "function owner names no string".to_string())?;
        let name_bytes = strings
            .get(name)
            .ok_or_else(|| "function name names no string".to_string())?;
        if !identity_component_ok(owner_bytes) || !identity_component_ok(name_bytes) {
            return Err("function identity component is empty or path-like".to_string());
        }
        if let Some((previous_owner, previous_name, _, _, _)) = functions.last() {
            if (
                strings[*previous_owner].as_slice(),
                strings[*previous_name].as_slice(),
            ) >= (owner_bytes.as_slice(), name_bytes.as_slice())
            {
                return Err("function identities are not strictly sorted".to_string());
            }
        }
        let kind = *body.get(pos).ok_or("function kind past the body")?;
        pos += 1;
        if kind > 2 {
            return Err(format!("unknown function kind {kind}"));
        }
        // Intrinsic kind and the reserved owner imply each other, both ways.
        let reserved = owner_bytes.as_slice() == b"__mind_intrinsic";
        if (kind == 2) != reserved {
            return Err("intrinsic kind and reserved owner must agree".to_string());
        }
        let parameter_count = read_uleb(body, &mut pos)? as usize;
        let mut parameters = Vec::new();
        for _ in 0..parameter_count {
            let mut descriptor = Vec::new();
            rederive_type(body, &mut pos, schema_count, &mut descriptor, 0)?;
            parameters.push(descriptor);
        }
        let present = *body.get(pos).ok_or("return-present tag past the body")?;
        pos += 1;
        let returns = match present {
            0 => None,
            1 => {
                let mut descriptor = Vec::new();
                rederive_type(body, &mut pos, schema_count, &mut descriptor, 0)?;
                Some(descriptor)
            }
            other => return Err(format!("invalid return-present tag {other}")),
        };
        functions.push((owner, name, kind, parameters, returns));
    }

    // --- module next_id and exports ---
    //
    // Export references are string indices that must be STRICTLY increasing.
    // The reference keeps exports in a set, so a repeated or descending index
    // would be absorbed silently on a set round-trip; the ordering rule is what
    // makes the encoding canonical, and it is checked here on the wire values.
    let next_id = read_uleb(body, &mut pos)? as usize;
    let export_count = read_uleb(body, &mut pos)? as usize;
    let mut exports: Vec<usize> = Vec::new();
    for _ in 0..export_count {
        let index = read_uleb(body, &mut pos)? as usize;
        if index >= strings.len() {
            return Err("export names no string".to_string());
        }
        // Order is compared on the export NAME BYTES here, deliberately unlike
        // the mirror, which compares string indices. Both are correct given a
        // unique byte-sorted string table, and checking them differently is the
        // point: a review found this section had converged on the same shortcut
        // in both implementations, so a shared mistake would have cancelled out.
        if let Some(previous) = exports.last() {
            if strings[*previous].as_slice() >= strings[index].as_slice() {
                return Err("export references are not strictly sorted".to_string());
            }
        }
        exports.push(index);
    }
    let prefix_len = pos;

    let mut out = Vec::new();
    out.extend_from_slice(&MIRROR_MAGIC);
    out.push(MIRROR_VERSION);
    write_uleb(&mut out, surface);
    write_uleb(&mut out, strings.len() as u64);
    for text in &strings {
        write_uleb(&mut out, text.len() as u64);
        out.extend_from_slice(text);
    }
    write_uleb(&mut out, schema_count as u64);
    for (owner, name) in &headers {
        write_uleb(&mut out, *owner as u64);
        write_uleb(&mut out, *name as u64);
    }
    for fields in &field_lists {
        write_uleb(&mut out, fields.len() as u64);
        for (name, descriptor) in fields {
            write_uleb(&mut out, *name as u64);
            out.extend_from_slice(descriptor);
        }
    }
    write_uleb(&mut out, function_count as u64);
    for (owner, name, kind, parameters, returns) in &functions {
        write_uleb(&mut out, *owner as u64);
        write_uleb(&mut out, *name as u64);
        out.push(*kind);
        write_uleb(&mut out, parameters.len() as u64);
        for descriptor in parameters {
            out.extend_from_slice(descriptor);
        }
        match returns {
            None => out.push(0),
            Some(descriptor) => {
                out.push(1);
                out.extend_from_slice(descriptor);
            }
        }
    }
    write_uleb(&mut out, next_id as u64);
    write_uleb(&mut out, exports.len() as u64);
    for index in &exports {
        write_uleb(&mut out, *index as u64);
    }
    Ok((out, prefix_len, strings.len()))
}

/// Minimal SHA-256 so the generator needs no extra dependency.
pub(super) fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (index, word) in chunk.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for index in 0..8 {
            h[index] = h[index].wrapping_add(v[index]);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}
