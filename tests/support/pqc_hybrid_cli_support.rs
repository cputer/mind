// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Shared fixture and process helpers for the signing CLI controls.

#[cfg(unix)]
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) const MINDC: &str = env!("CARGO_BIN_EXE_mindc");

/// Repo-relative path, for committed fixtures.
pub(super) fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Deterministic, obviously-non-production test seeds.
pub(super) const TEST_MLDSA87_SEED: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const TEST_SLHDSA_SEED: &str = concat!(
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

pub(super) trait ClearSigningEnv {
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

pub(super) struct Case {
    pub(super) dir: tempfile::TempDir,
}

impl Case {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("p.mind"),
            "fn main() -> i64 {\n    return 7;\n}\n",
        )
        .expect("write source");
        Self { dir }
    }

    pub(super) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Emit an evidence artifact. `seeds` selects which signing keys are offered.
    pub(super) fn emit(&self, out: &str, seeds: &[(&str, &str)]) -> (i32, String) {
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
    pub(super) fn emit_with_os_seed(&self, out: &str, name: &str, seed: &OsStr) -> (i32, String) {
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

    pub(super) fn verify(&self, artifact: &str, extra: &[&str]) -> (i32, String) {
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

    pub(super) fn both_legs(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("MIND_EVIDENCE_MLDSA87_KEY", TEST_MLDSA87_SEED),
            ("MIND_EVIDENCE_SLHDSA_KEY", TEST_SLHDSA_SEED),
        ]
    }

    pub(super) fn exists(&self, name: &str) -> bool {
        Path::new(&self.path(name)).exists()
    }
}

pub(super) fn assert_bad_32_byte_seed(env_name: &str, seed: &str, label: &str, diagnostic: &str) {
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
