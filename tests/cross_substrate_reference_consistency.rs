// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! WSP-12 — the cross-substrate equality assertion the identity suite never ran.
//!
//! `cross_substrate_identity.rs` compares each host's computed hash against the
//! entry for its OWN `host_substrate()`: `avx2` on x86_64, `neon` on aarch64. The
//! other substrate's line is never read on that host, so the two legs never meet.
//! Identity across substrates was held by CONVENTION — one hash copied into two
//! keys — while 25 manifests asserted "avx2 == neon by construction" in prose.
//! Deleting or corrupting the `neon` entry left every CI job green.
//!
//! Chained with the per-arch jobs, every link is now executed rather than assumed:
//!
//! ```text
//!   x86 job  : computed_avx2 == file.avx2     (cross_substrate_identity, on x86)
//!   ARM job  : computed_neon == file.neon     (same code, on aarch64)
//!   this test: file.avx2     == file.neon
//!   therefore: computed_avx2 == computed_neon
//! ```
//!
//! **What this does NOT prove:** that ARM hardware produces that hash. Only the
//! aarch64 runner shows that, and this check is worthless if that job stops
//! running. Both are listed in `.github/required-ci-jobs.tsv`; that is the
//! assumption the chain rests on, stated so a green fixture check is never
//! mistaken for a green machine.
//!
//! Runtime coverage is proved separately by case receipts. Of the 25 manifest
//! cases, 23 execute native code, `bimap-phf` measures compiler construction,
//! and the VNNI case may defer only when AVX-512-VNNI is unavailable. This file
//! still checks committed-reference equality; it does not infer execution from
//! manifest prose.
//!
//! Two holes an adversarial review found in the first version of this test, both
//! now closed and both of the same species — *the expectation was under the same
//! commit's control as the thing it checked*:
//!
//!   * narrowing a manifest to `substrates = ["avx2"]` removed that workload from
//!     the comparison and still passed. The declared set is now required to EQUAL
//!     the substrate set the CI matrix actually runs, read out of
//!     `.github/workflows/ci.yml`, so dropping a substrate needs an explicit,
//!     reviewed edit to the workflow rather than one word in a fixture.
//!   * a DUPLICATE `neon = ...` line with a different value still passed, because
//!     a first-match parse never sees the second. Duplicate keys are now an error;
//!     a harvest that appends rather than replaces can no longer be silently
//!     ignored.
//!
//! Deliberately in its own file with NO feature gate. `cross_substrate_identity.rs`
//! is `#![cfg(all(feature = "mlir-build", ...))]` and self-skips without the MLIR
//! toolchain — a fixture check living there would skip in exactly the situations
//! that produced the false green. This reads committed text only: no toolchain, no
//! compiler, no dlopen, so it runs on every job including the bare
//! `--no-default-features` one.
//!
//! The reference reader below is INDEPENDENT of `reference_hash()` in the gated
//! file on purpose: a parser bug shared with the code under test could hide the
//! very disagreement this exists to find.

use std::path::{Path, PathBuf};

mod common;

fn workload_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cross_substrate_identity")
}

/// Every value for `key` in a `<key> = "<value>"` file, in file order.
///
/// Returns ALL matches, not the first. A first-match reader cannot see a
/// duplicate key, and a duplicate is how an append-instead-of-replace harvest
/// hides a second, different hash behind a correct-looking one.
fn scalar_entries(path: &Path, key: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                out.push(v.trim().trim_matches('"').to_string());
            }
        }
    }
    out
}

fn scalar_entry(path: &Path, key: &str) -> Option<String> {
    scalar_entries(path, key).into_iter().next()
}

/// The substrates the cross-substrate CI job actually runs, read out of the
/// workflow's matrix.
///
/// The point of reading this is that the manifests and the fixtures land in the
/// same commit: an expectation stored beside the thing it checks can be narrowed
/// by the same edit that breaks it. The workflow matrix is the one statement of
/// which substrates are really exercised, and changing it is a visible,
/// reviewable act.
fn ci_matrix_substrates() -> Vec<String> {
    let wf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("ci.yml");
    let Ok(text) = std::fs::read_to_string(&wf) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut in_job = false;
    for line in text.lines() {
        if line.starts_with("  cross_substrate_identity:") {
            in_job = true;
            continue;
        }
        // Next job header at the same indent ends the block.
        if in_job
            && line.len() > 2
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
        {
            break;
        }
        if in_job {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim() == "substrate" {
                    let val = v.trim().trim_matches('"').trim().to_string();
                    if !val.is_empty() && !out.contains(&val) {
                        out.push(val);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// Parse `substrates = ["avx2", "neon"]` from a workload manifest.
///
/// The set of substrates that must agree is DECLARED in manifest.toml and read
/// from there — never inferred from the reference file under test. Deriving the
/// expectation from the artifact being checked is how a count silently becomes
/// "however many happen to be present", which can never catch an omission.
fn declared_substrates(manifest: &Path) -> Vec<String> {
    let Some(raw) = scalar_entry(manifest, "substrates") else {
        return Vec::new();
    };
    raw.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Every substrate a workload declares must carry a committed hash, and all of a
/// workload's hashes must be identical (RFC 0015 §3.1).
///
/// Turns red on: deleting a declared substrate's line, changing one hex digit of
/// it, a manifest declaring a substrate the fixture omits, a fixture with hashes
/// but no manifest, or the workload directory going missing.
#[test]
fn committed_reference_hashes_agree_across_substrates() {
    let root = workload_root();

    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read workload root {}: {e}", root.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("manifest.toml").is_file())
        .collect();
    dirs.sort();

    let policies = common::xsi_gate::load_inventory(&root)
        .unwrap_or_else(|e| panic!("invalid runtime evidence policy: {e}"));
    assert_eq!(
        policies.len(),
        dirs.len(),
        "runtime evidence policy does not cover every workload manifest"
    );

    // Independent second traversal. A fixture carrying reference hashes but no
    // manifest is skipped by the loop below, which would read as a pass; the two
    // counts disagreeing is the only way to see that from inside the test.
    let with_hashes = std::fs::read_dir(&root)
        .expect("workload root")
        .filter_map(Result::ok)
        .filter(|e| e.path().join("reference_hashes.toml").is_file())
        .count();

    // What CI really runs. Derived, so a fixture cannot quietly opt out of a
    // substrate that the matrix still exercises.
    let ci_substrates = ci_matrix_substrates();
    assert!(
        ci_substrates.len() >= 2,
        "derived only {:?} from the cross_substrate_identity matrix in \
         .github/workflows/ci.yml — fewer than two substrates means either the \
         matrix shrank or this derivation broke, and in both cases the identity \
         comparison below has nothing to compare",
        ci_substrates
    );

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    let mut pairs_compared = 0usize;

    for dir in &dirs {
        let id = dir
            .file_name()
            .expect("workload dir name")
            .to_string_lossy()
            .to_string();
        let mut declared = declared_substrates(&dir.join("manifest.toml"));
        if declared.is_empty() {
            failures.push(format!(
                "{id}: manifest.toml declares no `substrates` list, so nothing pins which \
                 substrates must agree"
            ));
            continue;
        }
        declared.sort();
        if declared != ci_substrates {
            failures.push(format!(
                "{id}: manifest declares substrates {declared:?} but the CI matrix runs \
                 {ci_substrates:?} — a workload that lists fewer substrates than are \
                 actually exercised drops out of the identity comparison while both arch \
                 jobs stay green. Change the workflow matrix if the coverage really \
                 changed."
            ));
            continue;
        }

        let hashes = dir.join("reference_hashes.toml");
        let mut present: Vec<(String, String)> = Vec::new();
        for s in &declared {
            let found = scalar_entries(&hashes, s);
            if found.len() > 1 {
                failures.push(format!(
                    "{id}: reference_hashes.toml carries {} '{s} = ...' lines ({found:?}); a \
                     first-match read would silently ignore the rest, which is how an \
                     append-instead-of-replace harvest hides a second hash",
                    found.len()
                ));
                continue;
            }
            match found.into_iter().next() {
                Some(h) => present.push((s.clone(), h)),
                None => failures.push(format!(
                    "{id}: manifest declares substrate '{s}' but reference_hashes.toml has no \
                     '{s} = \"...\"' entry — identity cannot be checked for a substrate with \
                     no committed hash"
                )),
            }
        }

        if let Some((s0, h0)) = present.first() {
            for (s, h) in present.iter().skip(1) {
                pairs_compared += 1;
                if h != h0 {
                    failures.push(format!(
                        "{id}: {s0} = {h0}\n       but {s} = {h}\n       RFC 0015 §3.1 requires \
                         every substrate of a workload to share ONE content hash; a divergence \
                         means the substrates are not byte-identical, or one entry was \
                         re-blessed alone"
                    ));
                }
            }
        }
        checked += 1;
    }

    assert!(
        checked > 0,
        "vacuous: no workloads with a manifest.toml under {} — a check that examined nothing \
         has not shown identity, it has shown an empty directory",
        root.display()
    );
    assert!(
        pairs_compared > 0,
        "vacuous: {checked} workloads examined but ZERO substrate pairs compared — every \
         manifest would have to declare a single substrate for that to be legitimate"
    );
    assert_eq!(
        checked, with_hashes,
        "{checked} workloads have a manifest but {with_hashes} carry reference_hashes.toml; a \
         fixture without a manifest is silently skipped by this check"
    );
    assert!(
        failures.is_empty(),
        "cross-substrate reference hashes disagree with the manifests ({} problem(s) across \
         {checked} workloads, {pairs_compared} pair(s) compared):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
