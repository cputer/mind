// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Audit F1 (SECURITY) regression — a signed mic@3 artifact must not be
//! byte-MALLEABLE.
//!
//! Before the fix, four distinct mutations produced four DIFFERENT files (four
//! different SHA-256s) that every one of them reported
//! `verified: signature is valid and signer key is trusted` and exited 0:
//!
//!   1. append an arbitrary byte after the MAP epilogue,
//!   2. pad the artifact out to the `MAX_MIC3_INPUT` ceiling,
//!   3. re-encode the MAP entry-count ULEB non-minimally (`0x09` -> `0x89 0x00`)
//!      *inside* the region the signature's provenance preimage covers,
//!   4. downgrade the wire-version byte inside the accepted read window,
//!   5. flip a body byte the decoder normalises away.
//!
//! Neither integrity layer covered the literal bytes: `trace_hash` anchors the
//! *re-emission* of the parsed IR, and the RFC 0021 signature preimage covers
//! `trace_hash` + scheme tag + DECODED provenance entries. `mindc verify` now
//! runs a canonical-form gate (`mic3_canonical_check`) that re-emits the
//! artifact and byte-compares against the input, so distinct byte streams can
//! never share one verdict.
//!
//! Bare builds exercise canonical unsigned evidence. The explicit PQC CI run
//! exercises the same mutations on a valid, dual-key-pinned hybrid signature.
//! Every rejection assertion is on the EXIT CODE (fail-closed).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

const PROGRAM: &str = "fn add(a: i64, b: i64) -> i64 { a + b }\n";

fn mindc() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mindc"));
    for key in [
        "MIND_EVIDENCE_ED25519_KEY",
        "MIND_EVIDENCE_MLDSA_KEY",
        "MIND_EVIDENCE_MLDSA87_KEY",
        "MIND_EVIDENCE_SLHDSA_KEY",
        "MIND_EVIDENCE_VERIFY_PUBKEYS",
    ] {
        cmd.env_remove(key);
    }
    cmd
}

fn write_src(dir: &Path) -> PathBuf {
    let src = dir.join("t.mind");
    fs::write(&src, PROGRAM).unwrap();
    src
}

/// Plain mic@3: no evidence chain at all.
fn emit_plain(dir: &Path) -> PathBuf {
    let out = dir.join("plain.mic3");
    let res = mindc()
        .arg(write_src(dir).to_str().unwrap())
        .arg("--emit-mic3")
        .arg(out.to_str().unwrap())
        .output()
        .expect("spawn mindc --emit-mic3");
    assert!(
        res.status.success(),
        "--emit-mic3 failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    out
}

/// Attested (evidence chain, unsigned).
fn emit_attested(dir: &Path) -> PathBuf {
    let out = dir.join("attested.mic3");
    let res = mindc()
        .arg(write_src(dir).to_str().unwrap())
        .arg("--emit-evidence")
        .arg(out.to_str().unwrap())
        // Explicitly UNSET so an ambient key in the developer's environment
        // cannot silently turn this fixture into a signed one.
        .env_remove("MIND_EVIDENCE_ED25519_KEY")
        .env_remove("MIND_EVIDENCE_MLDSA_KEY")
        .output()
        .expect("spawn mindc --emit-evidence");
    assert!(
        res.status.success(),
        "--emit-evidence failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    out
}

/// Canonical evidence under test: unsigned on a bare build, or the supported
/// dual-PQC signature when both crypto features are compiled. CI runs both.
#[cfg(not(all(feature = "evidence-mldsa", feature = "evidence-slhdsa")))]
fn emit_evidence_case(dir: &Path) -> (PathBuf, Option<Vec<String>>) {
    assert!(
        std::env::var_os("MIND_TEST_REQUIRE_PQC").is_none(),
        "this run requires the PQC signing features; unsigned controls cannot substitute"
    );
    (emit_attested(dir), None)
}

#[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
fn emit_evidence_case(dir: &Path) -> (PathBuf, Option<Vec<String>>) {
    let out = dir.join("signed.mic3");
    let res = mindc()
        .arg(write_src(dir))
        .arg("--emit-evidence")
        .arg(&out)
        .env("MIND_EVIDENCE_MLDSA87_KEY", "77".repeat(32))
        .env("MIND_EVIDENCE_SLHDSA_KEY", "22".repeat(96))
        .output()
        .expect("spawn hybrid evidence emitter");
    assert!(
        res.status.success(),
        "hybrid emit failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    let inspected = mindc()
        .arg("verify")
        .arg(&out)
        .output()
        .expect("inspect evidence");
    let stdout = String::from_utf8_lossy(&inspected.stdout);
    let keys = ["signature_mldsa87_pubkey:", "signature_slhdsa_pubkey:"]
        .iter()
        .map(|prefix| {
            stdout
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("missing {prefix} in {stdout}"))
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(keys.iter().all(|key| !key.is_empty()));
    assert_eq!(
        verify(&out, Some(&keys)),
        0,
        "the unmodified supported hybrid must be trusted with both keys"
    );
    (out, Some(keys))
}

/// `mindc verify` on `path`, pinning `pubkey` when supplied. Returns the exit code.
fn verify(path: &Path, pubkey: Option<&[String]>) -> i32 {
    let mut cmd = mindc();
    cmd.arg("verify").arg(path.to_str().unwrap());
    if let Some(pk) = pubkey {
        for key in pk {
            cmd.arg("--signer-pubkey").arg(key);
        }
        cmd.arg("--require-signed");
    }
    let res = cmd.output().expect("spawn mindc verify");
    res.status.code().unwrap_or(-1)
}

/// Offset of the MAP sentinel: the epilogue starts exactly where the canonical
/// body ends, i.e. at `emit_mic3(parse_mic3(bytes)).len()`.
fn map_sentinel_offset(bytes: &[u8]) -> usize {
    libmind::ir::compact::emit_mic3(
        &libmind::ir::compact::parse_mic3(bytes).expect("fixture must parse"),
    )
    .len()
}

fn mutate(src: &Path, name: &str, f: impl FnOnce(&mut Vec<u8>)) -> PathBuf {
    let mut bytes = fs::read(src).unwrap();
    f(&mut bytes);
    let out = src.parent().unwrap().join(name);
    fs::write(&out, &bytes).unwrap();
    out
}

// ─── The finding's headline case, in all three artifact shapes ────────────────

#[test]
fn appended_byte_is_rejected_in_available_evidence_shapes() {
    let dir = tempdir().unwrap();

    let plain = emit_plain(dir.path());
    assert_eq!(
        verify(&plain, None),
        0,
        "pristine plain artifact must verify"
    );
    let plain_x = mutate(&plain, "plain_x.mic3", |b| b.push(b'X'));
    assert_eq!(
        verify(&plain_x, None),
        1,
        "plain artifact || b\"X\" must be REJECTED (exit 1)"
    );

    let attested = emit_attested(dir.path());
    assert_eq!(
        verify(&attested, None),
        0,
        "pristine attested artifact must verify"
    );
    let attested_x = mutate(&attested, "attested_x.mic3", |b| b.push(b'X'));
    assert_eq!(
        verify(&attested_x, None),
        1,
        "attested artifact || b\"X\" must be REJECTED (exit 1)"
    );

    let (signed, pubkey) = emit_evidence_case(dir.path());
    assert_eq!(
        verify(&signed, pubkey.as_deref()),
        0,
        "pristine evidence case must verify"
    );
    let signed_x = mutate(&signed, "signed_x.mic3", |b| b.push(b'X'));
    assert_eq!(
        verify(&signed_x, pubkey.as_deref()),
        1,
        "evidence case || b\"X\" must be REJECTED (exit 1)"
    );
}

// ─── The remaining accepted mutations from the finding ────────────────────────

#[test]
fn padding_to_the_input_ceiling_is_rejected() {
    // The size cap is `len > MAX_MIC3_INPUT`, so padding to EXACTLY the ceiling
    // slipped past it and the trailing bytes were then ignored by the MAP parser.
    let dir = tempdir().unwrap();
    let (signed, pubkey) = emit_evidence_case(dir.path());
    let cap = libmind::ir::compact::MAX_MIC3_INPUT;
    let padded = mutate(&signed, "padded.mic3", |b| b.resize(cap, b'A'));
    assert_eq!(fs::metadata(&padded).unwrap().len() as usize, cap);
    assert_eq!(
        verify(&padded, pubkey.as_deref()),
        1,
        "evidence artifact padded to the input ceiling must be REJECTED (exit 1)"
    );
}

#[test]
fn non_minimal_map_count_uleb_is_rejected() {
    // Re-encode the MAP entry-count varint non-minimally, INSIDE the region the
    // signature's provenance preimage covers. `[0x89, 0x00]` decodes to 9 exactly
    // as `[0x09]` does, so the decoded entries — and therefore the preimage and
    // the signature — are unchanged.
    let dir = tempdir().unwrap();
    let (signed, pubkey) = emit_evidence_case(dir.path());
    let bytes = fs::read(&signed).unwrap();

    // Locate the sentinel the way the library does — the epilogue begins exactly
    // where the canonical body ends. Its absolute offset is artifact-dependent,
    // so never hardcode it.
    let sentinel = map_sentinel_offset(&bytes);
    assert_eq!(bytes[sentinel], b'M', "sentinel byte");
    let count = bytes[sentinel + 1];
    assert!(count < 0x80, "count varint must be single-byte here");

    let mut padded = Vec::with_capacity(bytes.len() + 1);
    padded.extend_from_slice(&bytes[..sentinel + 1]);
    padded.push(count | 0x80);
    padded.push(0x00);
    padded.extend_from_slice(&bytes[sentinel + 2..]);
    let out = dir.path().join("nonminimal.mic3");
    fs::write(&out, &padded).unwrap();

    assert_eq!(
        verify(&out, pubkey.as_deref()),
        1,
        "non-minimal MAP count ULEB must be REJECTED (exit 1)"
    );
}

#[test]
fn wire_version_downgrade_is_rejected() {
    // Not an arbitrary flip: the reader accepts the whole
    // MIC3_MIN_READ_VERSION..=MIC3_VERSION window, so 0x02 -> 0x01 parses, the
    // IR re-emits at the CURRENT version, and both the trace_hash and the
    // signature still check out. Only the literal-byte compare catches it.
    let dir = tempdir().unwrap();
    let (signed, pubkey) = emit_evidence_case(dir.path());
    let version = fs::read(&signed).unwrap()[4];
    assert!(
        version > 1,
        "fixture must ship above the minimum read version"
    );
    let down = mutate(&signed, "downgrade.mic3", |b| b[4] = 0x01);
    assert_eq!(
        verify(&down, pubkey.as_deref()),
        1,
        "wire-version downgrade must be REJECTED (exit 1)"
    );
}

#[test]
fn normalised_away_body_flip_is_rejected() {
    // Malleability was never confined to the MAP epilogue: a body byte the
    // decoder normalises away re-emits identically, so `trace_hash_valid` stays
    // true. Scan the body for any such byte and require every one of them to be
    // rejected — the whole point is that NO byte of the artifact is free.
    let dir = tempdir().unwrap();
    let (signed, pubkey) = emit_evidence_case(dir.path());
    let bytes = fs::read(&signed).unwrap();
    let body_len = map_sentinel_offset(&bytes);

    let mut checked = 0usize;
    for i in 0..body_len {
        let flipped = mutate(&signed, "bodyflip.mic3", |b| b[i] ^= 0x01);
        assert_eq!(
            verify(&flipped, pubkey.as_deref()),
            1,
            "body byte {i} flipped must be REJECTED (exit 1)"
        );
        checked += 1;
    }
    assert!(checked > 0, "body must be non-empty");
}

// ─── Library-level canonical-form unit gates ──────────────────────────────────

#[test]
fn canonical_check_accepts_what_the_emitter_produces() {
    use libmind::ir::compact::mic3_canonical_check;
    let dir = tempdir().unwrap();
    for path in [
        emit_plain(dir.path()),
        emit_attested(dir.path()),
        emit_evidence_case(dir.path()).0,
    ] {
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            mic3_canonical_check(&bytes),
            Ok(()),
            "freshly emitted {} must be canonical",
            path.display()
        );
    }
}

#[test]
fn canonical_check_rejects_every_finding_mutation() {
    use libmind::ir::compact::{Mic3NonCanonical, mic3_canonical_check};
    let dir = tempdir().unwrap();
    let (signed, _pk) = emit_evidence_case(dir.path());
    let bytes = fs::read(&signed).unwrap();

    let mut appended = bytes.clone();
    appended.push(b'X');
    assert!(
        matches!(
            mic3_canonical_check(&appended),
            Err(Mic3NonCanonical::MalformedMap)
        ),
        "append must be non-canonical, got {:?}",
        mic3_canonical_check(&appended)
    );

    let mut downgraded = bytes.clone();
    downgraded[4] = 0x01;
    assert!(
        matches!(
            mic3_canonical_check(&downgraded),
            Err(Mic3NonCanonical::Body { .. })
        ),
        "version downgrade must be non-canonical, got {:?}",
        mic3_canonical_check(&downgraded)
    );
}

#[test]
fn duplicate_map_key_is_rejected() {
    // Load-bearing on its own: two identical keys sort ADJACENTLY, so the
    // canonical re-encode reproduces them byte-for-byte and the byte-compare
    // alone would pass. A duplicate makes "the entry for key K" reader-dependent
    // (first-wins vs last-wins), which is a splitting attack against any consumer
    // that disagrees with the verifier — so it is rejected explicitly.
    use libmind::ir::compact::{Mic3NonCanonical, emit_mic3, mic3_canonical_check, parse_mic3};

    let dir = tempdir().unwrap();
    let plain = fs::read(emit_plain(dir.path())).unwrap();
    let body = emit_mic3(&parse_mic3(&plain).unwrap());

    // Hand-build a MAP epilogue with one key repeated. Canonical in every other
    // respect: sentinel, minimal ULEB count, sorted keys, string values.
    const TAG_STRING: u8 = 0x00;
    let key: &[u8] = b"evidence_chain.substrate";
    let val: &[u8] = b"cpu";
    let mut art = body.clone();
    art.push(b'M');
    art.push(2); // count = 2
    for _ in 0..2 {
        art.push(key.len() as u8);
        art.extend_from_slice(key);
        art.push(TAG_STRING);
        art.push(val.len() as u8);
        art.extend_from_slice(val);
    }

    assert_eq!(
        mic3_canonical_check(&art),
        Err(Mic3NonCanonical::DuplicateKey(
            "evidence_chain.substrate".to_string()
        )),
        "a repeated MAP key must be non-canonical"
    );

    let out = dir.path().join("dupkey.mic3");
    fs::write(&out, &art).unwrap();
    assert_eq!(
        verify(&out, None),
        1,
        "artifact with a duplicate MAP key must be REJECTED (exit 1)"
    );
}

#[test]
fn minimal_uleb_reader_rejects_zero_padding() {
    // Guards the belt-and-braces half directly: `[0x89, 0x00]` and `[0x09]` must
    // not both decode. Non-minimality is what let a length be re-encoded in place.
    use libmind::ir::compact::v2::{uleb128_read, uleb128_read_minimal};
    let padded: &[u8] = &[0x89, 0x00];
    assert_eq!(
        uleb128_read(&mut std::io::Cursor::new(padded)).unwrap(),
        9,
        "the lenient reader is what accepted the padding (documents the gap)"
    );
    assert!(
        uleb128_read_minimal(&mut std::io::Cursor::new(padded)).is_err(),
        "minimal reader must reject a zero-padded encoding"
    );
    // Canonical forms still read.
    assert_eq!(
        uleb128_read_minimal(&mut std::io::Cursor::new(&[0x09u8][..])).unwrap(),
        9
    );
    assert_eq!(
        uleb128_read_minimal(&mut std::io::Cursor::new(&[0x00u8][..])).unwrap(),
        0
    );
    assert_eq!(
        uleb128_read_minimal(&mut std::io::Cursor::new(&[0x80u8, 0x01][..])).unwrap(),
        128
    );
}
