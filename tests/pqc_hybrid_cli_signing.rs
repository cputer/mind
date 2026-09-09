//! End-to-end CLI controls for post-quantum hybrid artifact signing.
//!
//! The library controls prove the encoder and the non-degradable combiner. These
//! prove the thing an operator actually needs: that the SHIPPED BINARY can sign
//! an artifact, that the shipped verifier accepts it, and that every way of
//! weakening it fails closed.
//!
//! Seeds here are fixed TEST values embedded in source. No production key is
//! read, generated, or required, and none of these controls touches key custody.

#![cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]

#[cfg(unix)]
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

const MINDC: &str = env!("CARGO_BIN_EXE_mindc");

/// Repo-relative path, for committed fixtures.
fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Deterministic, obviously-non-production test seeds.
const TEST_MLDSA87_SEED: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const TEST_SLHDSA_SEED: &str = concat!(
    "222222222222222222222222222222222222222222222222",
    "222222222222222222222222222222222222222222222222",
    "222222222222222222222222222222222222222222222222",
    "222222222222222222222222222222222222222222222222",
);

/// Every environment variable that can supply a signing key or a trust anchor.
///
/// A test that inherits `MIND_EVIDENCE_VERIFY_PUBKEYS` from the surrounding
/// shell would silently change what "trusted" means, and one that inherits a
/// key seed could sign when it meant to emit unsigned. Both would pass locally
/// and mean nothing, so every invocation starts from a cleared environment.
const SIGNING_ENV: &[&str] = &[
    "MIND_EVIDENCE_MLDSA87_KEY",
    "MIND_EVIDENCE_SLHDSA_KEY",
    "MIND_EVIDENCE_MLDSA_KEY",
    "MIND_EVIDENCE_ED25519_KEY",
    "MIND_EVIDENCE_VERIFY_PUBKEYS",
];

trait ClearSigningEnv {
    fn envs_cleared_of_signing_state(&mut self) -> &mut Self;
}

impl ClearSigningEnv for Command {
    fn envs_cleared_of_signing_state(&mut self) -> &mut Self {
        for key in SIGNING_ENV {
            self.env_remove(key);
        }
        self
    }
}

struct Case {
    dir: tempfile::TempDir,
}

impl Case {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("p.mind"),
            "fn main() -> i64 {\n    return 7;\n}\n",
        )
        .expect("write source");
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Emit an evidence artifact. `seeds` selects which signing keys are offered.
    fn emit(&self, out: &str, seeds: &[(&str, &str)]) -> (i32, String) {
        let mut cmd = Command::new(MINDC);
        cmd.arg(self.path("p.mind"))
            .arg("--emit-evidence")
            .arg(self.path(out))
            .current_dir(self.dir.path())
            .envs_cleared_of_signing_state();
        for (k, v) in seeds {
            cmd.env(k, v);
        }
        let out = cmd.output().expect("spawn mindc");
        (
            out.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    #[cfg(unix)]
    fn emit_with_os_seed(&self, out: &str, name: &str, seed: &OsStr) -> (i32, String) {
        let mut cmd = Command::new(MINDC);
        cmd.arg(self.path("p.mind"))
            .arg("--emit-evidence")
            .arg(self.path(out))
            .current_dir(self.dir.path())
            .envs_cleared_of_signing_state()
            .env(name, seed);
        let output = cmd.output().expect("spawn mindc");
        (
            output.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    fn verify(&self, artifact: &str, extra: &[&str]) -> (i32, String) {
        let mut cmd = Command::new(MINDC);
        cmd.arg("verify")
            .arg(self.path(artifact))
            .args(extra)
            .current_dir(self.dir.path())
            .envs_cleared_of_signing_state();
        let out = cmd.output().expect("spawn mindc verify");
        (
            out.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    fn both_legs(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("MIND_EVIDENCE_MLDSA87_KEY", TEST_MLDSA87_SEED),
            ("MIND_EVIDENCE_SLHDSA_KEY", TEST_SLHDSA_SEED),
        ]
    }

    fn exists(&self, name: &str) -> bool {
        Path::new(&self.path(name)).exists()
    }
}

/// The shipped binary signs, and the shipped verifier accepts.
#[test]
fn the_cli_signs_with_the_pqc_hybrid_and_the_cli_verifies_it() {
    let c = Case::new();
    let (code, err) = c.emit("signed.mic3", &c.both_legs());
    assert_eq!(code, 0, "signing must succeed: {err}");
    assert!(
        err.contains("pqc-hybrid-ml-dsa-87-slh-dsa-256s-signed"),
        "the artifact must be tagged with the hybrid scheme: {err}"
    );

    let (vcode, vout) = c.verify("signed.mic3", &[]);
    assert_eq!(vcode, 0, "the shipped verifier must accept it: {vout}");
    assert!(
        vout.contains("signature is internally consistent"),
        "verify must actually check the signature, not only the trace hash: {vout}"
    );
    assert!(
        vout.contains("pqc-hybrid-ml-dsa-87-slh-dsa-256s"),
        "verify must report the hybrid scheme: {vout}"
    );
}

/// ONE LEG IS NOT A SIGNATURE. Supplying only the lattice seed must refuse at
/// sign time and write nothing — never quietly emit a single-leg artifact.
#[test]
fn a_single_leg_is_refused_at_sign_time_with_no_artifact() {
    let c = Case::new();
    let (code, err) = c.emit(
        "oneleg.mic3",
        &[("MIND_EVIDENCE_MLDSA87_KEY", TEST_MLDSA87_SEED)],
    );
    assert_ne!(code, 0, "one leg alone must refuse: {err}");
    assert!(
        !c.exists("oneleg.mic3"),
        "a refused signing run must write NO artifact"
    );
    assert!(
        err.contains("BOTH"),
        "the refusal must say both legs are required: {err}"
    );
}

/// Unsigned output stays available and is NOT silently treated as signed.
#[test]
fn an_unsigned_artifact_is_emitted_but_rejected_when_a_signature_is_required() {
    let c = Case::new();
    let (code, err) = c.emit("unsigned.mic3", &[]);
    assert_eq!(code, 0, "the unsigned path must still work: {err}");

    let (ok, _) = c.verify("unsigned.mic3", &[]);
    assert_eq!(ok, 0, "an unsigned artifact still verifies its trace hash");

    let (req, out) = c.verify("unsigned.mic3", &["--require-signed"]);
    assert_ne!(
        req, 0,
        "--require-signed must reject an unsigned artifact: {out}"
    );
}

/// Tampering with a signed artifact is detected.
#[test]
fn a_tampered_signed_artifact_is_rejected() {
    let c = Case::new();
    let (code, err) = c.emit("signed.mic3", &c.both_legs());
    assert_eq!(code, 0, "signing must succeed: {err}");

    let mut bytes = std::fs::read(c.path("signed.mic3")).expect("read artifact");
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    std::fs::write(c.path("tampered.mic3"), &bytes).expect("write tampered");

    let (vcode, vout) = c.verify("tampered.mic3", &[]);
    assert_ne!(vcode, 0, "a tampered artifact must be rejected: {vout}");
    assert!(
        vout.contains("error[verify]"),
        "rejection must be a named verify refusal, not an incidental failure: {vout}"
    );
    assert!(
        vout.contains("tampered") || vout.contains("does not verify"),
        "the cause must name tampering or signature failure, so this cannot pass \
         on an unrelated error such as a missing file: {vout}"
    );
}

/// The TRUST ANCHOR. Pinning the signer turns "internally consistent" into
/// "signed by a key I trust", and pinning makes a signature mandatory.
#[test]
fn pinning_the_signer_accepts_the_right_key_and_rejects_everything_else() {
    let c = Case::new();
    let (code, err) = c.emit("signed.mic3", &c.both_legs());
    assert_eq!(code, 0, "signing must succeed: {err}");

    let (_, report) = c.verify("signed.mic3", &[]);
    let field = |name: &str| -> String {
        report
            .lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .map(str::trim)
            .unwrap_or_else(|| panic!("verify must report {name}; got:\n{report}"))
            .to_string()
    };
    let pubkey = field("signature_mldsa87_pubkey:");
    let slh_pubkey = field("signature_slhdsa_pubkey:");
    assert!(!pubkey.is_empty(), "signer public key must not be empty");

    // BOTH legs must be pinned. The combiner is an AND: pinning only the lattice
    // key leaves the hash-based signer untrusted, and the verifier refuses —
    // which is the correct reading of "signed by a key I trust" for a hybrid.
    let (good, gout) = c.verify(
        "signed.mic3",
        &["--signer-pubkey", &pubkey, "--signer-pubkey", &slh_pubkey],
    );
    assert_eq!(good, 0, "both true signer keys must be accepted: {gout}");

    // Pinning ONLY one leg is not enough, and that is a feature, not a gap.
    let (half, hout) = c.verify("signed.mic3", &["--signer-pubkey", &pubkey]);
    assert_ne!(
        half, 0,
        "pinning one leg of a hybrid must NOT be accepted as a trusted signer: {hout}"
    );

    // Wrong key: rejected as untrusted, not merely unverifiable.
    let wrong = "ab".repeat(pubkey.len() / 2);
    let (bad, bout) = c.verify("signed.mic3", &["--signer-pubkey", &wrong]);
    assert_ne!(bad, 0, "an untrusted signer must be rejected: {bout}");
    assert!(
        bout.contains("NOT in the trusted allowlist"),
        "the refusal must name the allowlist, not merely exit nonzero: {bout}"
    );

    // Pinning makes a signature REQUIRED, so an unsigned artifact fails too.
    let (u, _) = c.emit("unsigned.mic3", &[]);
    assert_eq!(u, 0, "unsigned emit must succeed");
    let (pinned_unsigned, pout) = c.verify(
        "unsigned.mic3",
        &["--signer-pubkey", &pubkey, "--signer-pubkey", &slh_pubkey],
    );
    assert_ne!(
        pinned_unsigned, 0,
        "pinning a signer must reject an unsigned artifact: {pout}"
    );
    assert!(
        pout.contains("a signer key is pinned"),
        "the refusal must name the pinned-signer rule: {pout}"
    );
}

/// UNKNOWN-SCHEME DISPATCH. Rewriting `signature.scheme` to a value the
/// verifier does not recognise is rejected through the actual CLI.
///
/// SCOPE, stated because an earlier name overclaimed this: the replacement tag
/// used here is NOT a supported scheme, so this exercises unknown-tag dispatch
/// and proves an unrecognised `alg` can never be silently accepted. It does
/// NOT prove downgrade-to-a-supported-weaker-scheme, and it does not by itself
/// demonstrate preimage binding of the tag.
///
/// The evidence for actual non-degradability is the library control
/// `pqc_hybrid_non_degradable`, which strips either real leg of the signed
/// artifact and requires the result to be Malformed rather than collapsing to
/// a valid single-scheme signature, behind a Valid positive control.
///
/// The mutation is length-preserving so no offset shift can explain the
/// result, and it asserts the tag was present first, so a no-op edit cannot
/// pass.
#[test]
fn rewriting_the_scheme_tag_to_an_unknown_value_is_rejected() {
    let c = Case::new();
    let (code, err) = c.emit("signed.mic3", &c.both_legs());
    assert_eq!(code, 0, "signing must succeed: {err}");

    let bytes = std::fs::read(c.path("signed.mic3")).expect("read artifact");
    let tag = b"pqc-hybrid-ml-dsa-87-slh-dsa-256s";
    let at = bytes
        .windows(tag.len())
        .position(|w| w == tag)
        .expect("positive control: the scheme tag must be present in the artifact");

    // Same length, different value: a structural downgrade attempt that keeps
    // every offset intact so nothing else can explain the rejection.
    let mut mutated = bytes.clone();
    let weaker = b"pqc-hybrid-ml-dsa-87-slh-dsa-000s";
    assert_eq!(weaker.len(), tag.len(), "mutation must preserve length");
    mutated[at..at + tag.len()].copy_from_slice(weaker);
    assert_ne!(
        mutated, bytes,
        "the mutation must actually change the bytes"
    );
    std::fs::write(c.path("scheme_tampered.mic3"), &mutated).expect("write mutated");

    let (vcode, vout) = c.verify("scheme_tampered.mic3", &[]);
    assert_ne!(vcode, 0, "a rewritten scheme tag must be rejected: {vout}");
    assert!(
        vout.contains("error[verify]"),
        "rejection must be a named verify refusal: {vout}"
    );
    assert!(
        !vout.contains("verified: signature is internally consistent"),
        "a downgraded tag must NEVER be reported as a consistent signature: {vout}"
    );
}

/// APPLICATION FIELD MUTATION. An attacker-controlled evidence attribute must
/// not be rewritable under a signature that still verifies.
///
/// The signature preimage covers the evidence entries, so editing an attached
/// application attribute after signing must break verification. Without this,
/// "signed" would only mean the IR was attested while its surrounding claims
/// stayed malleable.
#[test]
fn mutating_a_signed_application_attribute_is_rejected() {
    let c = Case::new();
    let mut cmd = Command::new(MINDC);
    cmd.arg(c.path("p.mind"))
        .arg("--emit-evidence")
        .arg(c.path("attr.mic3"))
        .arg("--evidence-attr")
        // Application keys must be namespaced (contain a dot) and must not use a
        // reserved prefix; an un-namespaced key is refused at the library boundary.
        .arg("app.release=alpha")
        .current_dir(c.dir.path())
        .envs_cleared_of_signing_state()
        .env("MIND_EVIDENCE_MLDSA87_KEY", TEST_MLDSA87_SEED)
        .env("MIND_EVIDENCE_SLHDSA_KEY", TEST_SLHDSA_SEED);
    let out = cmd.output().expect("spawn mindc");
    assert_eq!(
        out.status.code().unwrap_or(-1),
        0,
        "signing with an application attribute must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // POSITIVE, unchanged: the same artifact with the attribute untouched
    // verifies, so a rejection below cannot be blamed on the fixture.
    let (ok, oout) = c.verify("attr.mic3", &[]);
    assert_eq!(ok, 0, "the freshly signed artifact must verify: {oout}");
    assert!(
        oout.contains("trace_hash_valid: yes"),
        "unchanged control: the body must validate: {oout}"
    );
    assert!(
        !oout.contains("signature:        invalid"),
        "unchanged control: the signature must NOT be invalid: {oout}"
    );

    let bytes = std::fs::read(c.path("attr.mic3")).expect("read artifact");
    let at = bytes
        .windows(5)
        .position(|w| w == b"alpha")
        .expect("positive control: the attribute value must be present");
    let mut mutated = bytes.clone();
    mutated[at..at + 5].copy_from_slice(b"final");
    std::fs::write(c.path("attr_tampered.mic3"), &mutated).expect("write mutated");

    let (vcode, vout) = c.verify("attr_tampered.mic3", &[]);
    assert_ne!(
        vcode, 0,
        "rewriting a signed application attribute must be rejected: {vout}"
    );
    // The PRECISE boundary: the body was not touched, so its trace hash still
    // validates, and the failure must be the SIGNATURE. Asserting only a
    // nonzero exit would also pass on a parse error, which would prove nothing
    // about authenticated metadata.
    assert!(
        vout.contains("trace_hash_valid: yes"),
        "the body trace hash must still validate — otherwise this measures body \
         tampering, not signed-metadata tampering: {vout}"
    );
    assert!(
        vout.contains("signature:        invalid") || vout.contains("signature is invalid"),
        "the signature specifically must be reported invalid: {vout}"
    );
}

/// EITHER leg wrong fails, even when the other is correct.
///
/// The combiner is an AND, so trust must be an AND too. This covers both
/// asymmetric cases explicitly rather than assuming one implies the other: a
/// verifier that only ever consulted the first pinned key would pass one of
/// these and fail the other.
#[test]
fn one_correct_key_paired_with_one_wrong_key_is_rejected_either_way() {
    let c = Case::new();
    let (code, err) = c.emit("signed.mic3", &c.both_legs());
    assert_eq!(code, 0, "signing must succeed: {err}");

    let (_, report) = c.verify("signed.mic3", &[]);
    let field = |name: &str| -> String {
        report
            .lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .map(str::trim)
            .unwrap_or_else(|| panic!("verify must report {name}; got:\n{report}"))
            .to_string()
    };
    let good_mldsa = field("signature_mldsa87_pubkey:");
    let good_slhdsa = field("signature_slhdsa_pubkey:");
    let wrong_mldsa = "ab".repeat(good_mldsa.len() / 2);
    let wrong_slhdsa = "cd".repeat(good_slhdsa.len() / 2);

    // Positive control first: the true pair is accepted, so a rejection below
    // cannot be explained by the fixture simply never verifying.
    let (ok, okout) = c.verify(
        "signed.mic3",
        &[
            "--signer-pubkey",
            &good_mldsa,
            "--signer-pubkey",
            &good_slhdsa,
        ],
    );
    assert_eq!(ok, 0, "the true pair must be accepted: {okout}");

    for (label, a, b) in [
        (
            "wrong lattice leg, correct hash-based leg",
            &wrong_mldsa,
            &good_slhdsa,
        ),
        (
            "correct lattice leg, wrong hash-based leg",
            &good_mldsa,
            &wrong_slhdsa,
        ),
    ] {
        let (code, out) = c.verify("signed.mic3", &["--signer-pubkey", a, "--signer-pubkey", b]);
        assert_ne!(code, 0, "{label}: must be rejected: {out}");
        assert!(
            out.contains("NOT in the trusted allowlist"),
            "{label}: refusal must name the allowlist: {out}"
        );
    }
}

/// An allowlist that names two DIFFERENT identities' keys is rejected.
///
/// SCOPE, corrected after review because the earlier name claimed more than the
/// fixture does. The artifact here is UNCHANGED and was signed wholly by
/// identity A; the trust set pins A's lattice key beside B's hash-based key. So
/// what this proves is that an allowlist which does not contain the true
/// complete pair is refused — the same family as the wrong-key controls, with
/// the wrong key sourced from a real second identity rather than invented.
///
/// It does NOT construct a spliced artifact. True pair-splicing would combine
/// A's lattice signature and key with B's hash-based signature and key in ONE
/// message, with both original pairs authorised, and require exact paired
/// identity to reject the recombination. That control belongs to the release
/// profile, where a signed key-pair identity exists to compare against, and is
/// NOT yet implemented. It is named here so the gap is visible rather than
/// implied to be covered.
#[test]
fn an_allowlist_naming_two_different_identities_is_rejected() {
    let c = Case::new();
    let (a_code, a_err) = c.emit("identity_a.mic3", &c.both_legs());
    assert_eq!(a_code, 0, "identity A must sign: {a_err}");

    // A second, different identity: same scheme, different seeds.
    const OTHER_MLDSA87_SEED: &str =
        "7777777777777777777777777777777777777777777777777777777777777777";
    const OTHER_SLHDSA_SEED: &str = concat!(
        "888888888888888888888888888888888888888888888888",
        "888888888888888888888888888888888888888888888888",
        "888888888888888888888888888888888888888888888888",
        "888888888888888888888888888888888888888888888888",
    );
    let (b_code, b_err) = c.emit(
        "identity_b.mic3",
        &[
            ("MIND_EVIDENCE_MLDSA87_KEY", OTHER_MLDSA87_SEED),
            ("MIND_EVIDENCE_SLHDSA_KEY", OTHER_SLHDSA_SEED),
        ],
    );
    assert_eq!(b_code, 0, "identity B must sign: {b_err}");

    let key = |artifact: &str, name: &str| -> String {
        let (_, report) = c.verify(artifact, &[]);
        report
            .lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .map(str::trim)
            .unwrap_or_else(|| panic!("verify must report {name} for {artifact}"))
            .to_string()
    };
    let a_mldsa = key("identity_a.mic3", "signature_mldsa87_pubkey:");
    let b_slhdsa = key("identity_b.mic3", "signature_slhdsa_pubkey:");
    assert_ne!(
        a_mldsa,
        key("identity_b.mic3", "signature_mldsa87_pubkey:"),
        "the two identities must actually differ, or this proves nothing"
    );

    // A's lattice leg beside B's hash-based leg: no single identity signed this.
    let (code, out) = c.verify(
        "identity_a.mic3",
        &["--signer-pubkey", &a_mldsa, "--signer-pubkey", &b_slhdsa],
    );
    assert_ne!(
        code, 0,
        "a pair mixed across identities must be rejected: {out}"
    );
}

/// RETAINED: an un-namespaced application key is refused at the library
/// boundary, before it can reach the MAP or the signature preimage.
///
/// This is kept deliberately. It was the first fixture attempt for the
/// attribute-mutation control and it failed, which is exactly what made the
/// boundary visible: `release=alpha` is rejected because an application key
/// must be namespaced. Keeping it turns an accident into a control, and stops a
/// future change from quietly admitting un-namespaced keys into signed
/// metadata.
#[test]
fn an_un_namespaced_application_key_is_refused_before_signing() {
    let c = Case::new();
    let mut cmd = Command::new(MINDC);
    cmd.arg(c.path("p.mind"))
        .arg("--emit-evidence")
        .arg(c.path("badattr.mic3"))
        .arg("--evidence-attr")
        .arg("release=alpha")
        .current_dir(c.dir.path())
        .envs_cleared_of_signing_state()
        .env("MIND_EVIDENCE_MLDSA87_KEY", TEST_MLDSA87_SEED)
        .env("MIND_EVIDENCE_SLHDSA_KEY", TEST_SLHDSA_SEED);
    let out = cmd.output().expect("spawn mindc");
    let err = String::from_utf8_lossy(&out.stderr).into_owned();

    assert_ne!(
        out.status.code().unwrap_or(-1),
        0,
        "an un-namespaced application key must be refused: {err}"
    );
    assert!(
        !c.exists("badattr.mic3"),
        "a refused emit must write NO artifact"
    );
    assert!(
        err.contains("application evidence key"),
        "the refusal must name the application-key rule: {err}"
    );
}

// ---------------------------------------------------------------------------
// PERMANENT ED25519 RETIREMENT
//
// Ed25519 and the old Ed25519+ML-DSA-65 hybrid are retired from BOTH supported
// signing and trust verification. The controls below pin the four properties
// that make that real rather than declared: a legacy request refuses instead of
// being ignored, a mixed config refuses instead of quietly using the supported
// key, a historical artifact reports RETIRED rather than valid or unsigned, and
// the unsigned path is untouched.
// ---------------------------------------------------------------------------

/// A legacy Ed25519 seed refuses, and refuses EXPLICITLY.
///
/// The failure mode this forbids is silent substitution: accepting the run and
/// signing with some other key, so an operator who believes they configured
/// Ed25519 gets something else and is never told.
#[test]
fn a_retired_ed25519_seed_refuses_instead_of_being_ignored() {
    let c = Case::new();
    let (code, err) = c.emit(
        "ed.mic3",
        &[(
            "MIND_EVIDENCE_ED25519_KEY",
            "9999999999999999999999999999999999999999999999999999999999999999",
        )],
    );
    assert_ne!(code, 0, "a retired seed must refuse: {err}");
    assert!(!c.exists("ed.mic3"), "a refused run must write NO artifact");
    assert!(
        err.contains("retired"),
        "the refusal must name retirement as the cause: {err}"
    );
}

/// A MIXED configuration refuses: legacy seed beside the supported pair.
///
/// This is the case that would otherwise be most tempting to "helpfully"
/// resolve by using the good keys. Doing so would leave a dead legacy config in
/// place, unnoticed, for as long as it kept appearing to work.
#[test]
fn a_mixed_legacy_and_supported_configuration_refuses() {
    let c = Case::new();
    let mut seeds = c.both_legs();
    seeds.push((
        "MIND_EVIDENCE_ED25519_KEY",
        "9999999999999999999999999999999999999999999999999999999999999999",
    ));
    let (code, err) = c.emit("mixed.mic3", &seeds);
    assert_ne!(
        code, 0,
        "a legacy seed must refuse even when supported keys are present: {err}"
    );
    assert!(
        !c.exists("mixed.mic3"),
        "a refused run must write NO artifact, not one signed with the other key"
    );
    assert!(
        err.contains("retired"),
        "the refusal must name retirement: {err}"
    );
}

/// The supported pair still signs. Without this the two controls above would
/// pass on a build that refuses everything.
#[test]
fn the_supported_pair_still_signs_after_the_retirement() {
    let c = Case::new();
    let (code, err) = c.emit("ok.mic3", &c.both_legs());
    assert_eq!(code, 0, "the supported hybrid must still sign: {err}");
    assert!(
        err.contains("pqc-hybrid-ml-dsa-87-slh-dsa-256s-signed"),
        "and must still be tagged with the hybrid scheme: {err}"
    );
    let (v, vout) = c.verify("ok.mic3", &[]);
    assert_eq!(v, 0, "and must still verify: {vout}");
}

/// The unsigned path is UNCHANGED by the retirement.
///
/// Byte identity of unsigned artifacts is the determinism wedge. A retirement
/// that moved those bytes would be a far worse regression than the thing it
/// fixed, so this asserts the exact size the corpus expects.
#[test]
fn the_unsigned_path_is_unchanged_by_the_retirement() {
    let c = Case::new();
    let (code, err) = c.emit("plain.mic3", &[]);
    assert_eq!(code, 0, "the unsigned path must still work: {err}");
    let bytes = std::fs::read(c.path("plain.mic3")).expect("read artifact");
    // BYTE IDENTITY, not size stability. A length check passes on any mutation
    // that preserves length, which is most of them, so it would have been a
    // weaker claim than the comment made. Compared against committed reference
    // bytes instead.
    let reference = std::fs::read(repo_path("tests/fixtures/evidence/unsigned_reference.mic3"))
        .expect("committed unsigned reference must exist");
    assert_eq!(
        bytes, reference,
        "unsigned evidence bytes must be BYTE-IDENTICAL to the committed \
         reference: signing changes are metadata after the hashed body, so any \
         difference here means the retirement disturbed the determinism wedge"
    );
}

/// HISTORICAL REJECTION FIXTURE — genuine pre-retirement bytes.
///
/// These bytes were minted by the compiler BEFORE Ed25519 was retired and are
/// committed under `tests/fixtures/evidence/`. They cannot be regenerated: the
/// production API now refuses to sign under a retired scheme, which is exactly
/// why they must be kept rather than produced on demand.
///
/// The properties asserted are the ones that distinguish a retirement from a
/// deletion: the artifact is REJECTED, the cause is named as retirement rather
/// than a generic failure, it is NOT silently reported as unsigned, and it
/// remains INSPECTABLE — its body still decodes and its trace hash still
/// validates. Inspection is not trust.
#[test]
fn a_historical_ed25519_artifact_is_rejected_but_still_inspectable() {
    let c = Case::new();
    let fixture = repo_path("tests/fixtures/evidence/historical_ed25519_signed.mic3");
    let bytes = std::fs::read(&fixture).expect("committed historical fixture must exist");
    std::fs::write(c.path("historical.mic3"), &bytes).expect("stage fixture");

    let (code, out) = c.verify("historical.mic3", &[]);
    assert_ne!(
        code, 0,
        "a retired scheme must never verify as trusted: {out}"
    );
    assert!(
        out.contains("retired"),
        "the verdict must name retirement, not a generic failure: {out}"
    );
    assert!(
        !out.contains("signature:        absent"),
        "a signed artifact must NOT be demoted to unsigned — that would return \
         success for a retired signature: {out}"
    );
    // Still inspectable: the body decodes and its anchor validates.
    assert!(
        out.contains("trace_hash_valid: yes"),
        "historical bytes must remain inspectable: {out}"
    );
}

/// The old Ed25519 + ML-DSA-65 artifact is also historical input: it must be
/// decoded and identified, but never accepted as trusted or demoted to unsigned.
#[test]
fn a_historical_old_hybrid_is_rejected_but_still_inspectable() {
    let c = Case::new();
    let fixture =
        repo_path("tests/fixtures/evidence/historical_hybrid_ed25519_mldsa65_signed.mic3");
    std::fs::copy(fixture, c.path("historical-hybrid.mic3")).expect("stage fixture");

    let (code, out) = c.verify("historical-hybrid.mic3", &[]);
    assert_ne!(
        code, 0,
        "a retired old hybrid must never verify as trusted: {out}"
    );
    assert!(
        out.contains("retired") && out.contains("hybrid-ed25519-ml-dsa-65"),
        "the old hybrid retirement must be named: {out}"
    );
    assert!(
        !out.contains("signature:        absent"),
        "must remain signed: {out}"
    );
    assert!(
        out.contains("trace_hash_valid: yes"),
        "body must remain inspectable: {out}"
    );
}

fn assert_bad_32_byte_seed(env_name: &str, seed: &str, label: &str, diagnostic: &str) {
    let c = Case::new();
    // Include the other hybrid leg for ML-DSA-87. Otherwise a parser that
    // silently accepts the malformed value still fails for the unrelated
    // "one leg is missing" rule.
    let seeds = if env_name == "MIND_EVIDENCE_MLDSA87_KEY" {
        vec![
            (env_name, seed),
            ("MIND_EVIDENCE_SLHDSA_KEY", TEST_SLHDSA_SEED),
        ]
    } else {
        vec![(env_name, seed)]
    };
    let (code, output) = c.emit(label, &seeds);
    assert_eq!(
        code, 1,
        "malformed 32-byte seed must exit 1 for {env_name}: {output}"
    );
    assert!(
        !c.exists(label),
        "refusal must leave no artifact for {env_name}"
    );
    assert!(
        output.contains(diagnostic),
        "malformed 32-byte seed must report {diagnostic:?} for {env_name}: {output}"
    );
    assert!(
        !output.contains(seed),
        "diagnostics must not echo the configured seed for {env_name}: {output}"
    );
}

/// Both 32-byte readers reject a wrong-length value before signing.
#[test]
fn a_wrong_length_32_byte_seed_refuses_for_both_readers() {
    for (index, env_name) in ["MIND_EVIDENCE_MLDSA87_KEY", "MIND_EVIDENCE_MLDSA_KEY"]
        .into_iter()
        .enumerate()
    {
        assert_bad_32_byte_seed(
            env_name,
            "bad-32-byte-length-sentinel-7f3b",
            &format!("bad-32-length-{index}.mic3"),
            "expected 64 hex chars",
        );
    }
}

/// A 64-byte ASCII value with a non-hex character must be reported as malformed,
/// rather than reaching a byte-offset panic or silently becoming unsigned.
#[test]
fn an_invalid_ascii_32_byte_seed_refuses_for_both_readers() {
    let malformed = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
    for (index, env_name) in ["MIND_EVIDENCE_MLDSA87_KEY", "MIND_EVIDENCE_MLDSA_KEY"]
        .into_iter()
        .enumerate()
    {
        assert_bad_32_byte_seed(
            env_name,
            malformed,
            &format!("bad-32-ascii-{index}.mic3"),
            "seed must be hex digits only",
        );
    }
}

/// A byte-length-64 non-ASCII value must fail before the hex parser slices it.
#[test]
fn a_non_ascii_32_byte_seed_refuses_for_both_readers() {
    let malformed = "é".repeat(32);
    for (index, env_name) in ["MIND_EVIDENCE_MLDSA87_KEY", "MIND_EVIDENCE_MLDSA_KEY"]
        .into_iter()
        .enumerate()
    {
        assert_bad_32_byte_seed(
            env_name,
            &malformed,
            &format!("bad-32-nonascii-{index}.mic3"),
            "seed must be ASCII hex",
        );
    }
}

#[cfg(unix)]
#[test]
fn a_non_utf8_32_byte_seed_refuses_for_both_readers() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    for (index, env_name) in ["MIND_EVIDENCE_MLDSA87_KEY", "MIND_EVIDENCE_MLDSA_KEY"]
        .into_iter()
        .enumerate()
    {
        let c = Case::new();
        let raw = OsString::from_vec(vec![0xff; 64]);
        let label = format!("bad-32-utf8-{index}.mic3");
        let mut cmd = Command::new(MINDC);
        cmd.arg(c.path("p.mind"))
            .arg("--emit-evidence")
            .arg(c.path(&label))
            .current_dir(c.dir.path())
            .envs_cleared_of_signing_state()
            .env(env_name, &raw);
        if env_name == "MIND_EVIDENCE_MLDSA87_KEY" {
            cmd.env("MIND_EVIDENCE_SLHDSA_KEY", TEST_SLHDSA_SEED);
        }
        let output = cmd.output().expect("spawn mindc");
        let code = output.status.code().unwrap_or(-1);
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            code, 1,
            "non-UTF-8 32-byte seed must exit 1 for {env_name}: {output}"
        );
        assert!(!c.exists(&label), "refusal must leave no artifact");
        assert!(
            output.contains("not valid UTF-8") && !output.contains("ff"),
            "diagnostics must identify encoding without echoing bytes: {output}"
        );
    }
}

/// A malformed 96-byte seed is rejected before the output path is created.
#[test]
fn a_wrong_length_slhdsa_seed_refuses_without_an_artifact_or_seed_leak() {
    let c = Case::new();
    let malformed = "SLHDSA_BAD_LENGTH_SENTINEL_7f3b";
    let (code, err) = c.emit(
        "bad-length.mic3",
        &[
            ("MIND_EVIDENCE_MLDSA87_KEY", TEST_MLDSA87_SEED),
            ("MIND_EVIDENCE_SLHDSA_KEY", malformed),
        ],
    );
    assert_ne!(code, 0, "wrong-length SLH seed must refuse: {err}");
    assert!(
        !c.exists("bad-length.mic3"),
        "refusal must leave no artifact"
    );
    assert!(
        err.contains("192") && !err.contains(malformed),
        "the refusal must report length without echoing the configured seed: {err}"
    );
}

/// A correctly sized ASCII value with a non-hex character must be rejected by
/// the 96-byte SLH-DSA reader before any byte-offset decoding.
#[test]
fn an_invalid_ascii_slhdsa_seed_refuses_without_an_artifact_or_seed_leak() {
    let c = Case::new();
    let malformed = format!("SLHDSA_BAD_HEX_SENTINEL_7f3b{}", "z".repeat(164));
    let (code, err) = c.emit(
        "bad-slhdsa-ascii.mic3",
        &[
            ("MIND_EVIDENCE_MLDSA87_KEY", TEST_MLDSA87_SEED),
            ("MIND_EVIDENCE_SLHDSA_KEY", malformed.as_str()),
        ],
    );
    assert_eq!(code, 1, "invalid ASCII SLH seed must exit 1: {err}");
    assert!(!c.exists("bad-slhdsa-ascii.mic3"));
    assert!(
        err.contains("seed must be hex digits only") && !err.contains(&malformed),
        "the refusal must identify invalid hex without echoing the seed: {err}"
    );
}

/// A value with 192 UTF-8 bytes but non-ASCII characters must take the ASCII
/// diagnostic before any byte-offset slicing can panic.
#[test]
fn a_non_ascii_slhdsa_seed_refuses_without_an_artifact_or_seed_leak() {
    let c = Case::new();
    let non_ascii = "é".repeat(96);
    let (code, err) = c.emit(
        "bad-ascii.mic3",
        &[
            ("MIND_EVIDENCE_MLDSA87_KEY", TEST_MLDSA87_SEED),
            ("MIND_EVIDENCE_SLHDSA_KEY", non_ascii.as_str()),
        ],
    );
    assert_ne!(code, 0, "non-ASCII SLH seed must refuse: {err}");
    assert!(
        !c.exists("bad-ascii.mic3"),
        "refusal must leave no artifact"
    );
    assert!(err.contains("ASCII") && !err.contains("é"));
}

#[cfg(unix)]
#[test]
fn a_non_utf8_slhdsa_seed_refuses_without_an_artifact_or_seed_leak() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let c = Case::new();
    let raw = OsString::from_vec(vec![0xff; 192]);
    let (code, err) = c.emit_with_os_seed("bad-utf8.mic3", "MIND_EVIDENCE_SLHDSA_KEY", &raw);
    assert_ne!(code, 0, "non-UTF-8 SLH seed must refuse: {err}");
    assert!(!c.exists("bad-utf8.mic3"), "refusal must leave no artifact");
    assert!(err.contains("not valid UTF-8") && !err.contains("ff"));
}
