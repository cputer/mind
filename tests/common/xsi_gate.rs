// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Runtime evidence for the cross-substrate identity suite.
//!
//! A passing libtest function is not proof that its workload ran: an allowed
//! capability branch can return and libtest still prints `ok`.  This module
//! binds every manifest case to one atomically published outcome.  Policy is
//! read from the case manifest; absence classification remains owned by
//! [`super::gate`].

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

pub const RECEIPT_DIR_VAR: &str = "MIND_XSI_RECEIPTS_DIR";
pub const VNNI_VERIFY_VAR: &str = "MIND_INTDOT_VNNI_VERIFY";
pub const VNNI_CASE: &str = "gemm-i8-vnni-64x64x64";
pub const COMPILE_ONLY_CASE: &str = "bimap-phf";
const TARGET: &str = "cross_substrate_identity";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    NativeRuntime,
    CompilerConstruction,
}

impl EvidenceKind {
    pub fn token(self) -> &'static str {
        match self {
            Self::NativeRuntime => "native-runtime",
            Self::CompilerConstruction => "compiler-construction",
        }
    }

    fn parse(token: &str) -> Result<Self, String> {
        match token {
            "native-runtime" => Ok(Self::NativeRuntime),
            "compiler-construction" => Ok(Self::CompilerConstruction),
            other => Err(format!("unknown evidence kind `{other}`")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredIsa {
    Avx512Vnni,
}

impl RequiredIsa {
    pub fn token(self) -> &'static str {
        match self {
            Self::Avx512Vnni => "avx512vnni",
        }
    }

    fn parse(token: &str) -> Result<Self, String> {
        match token {
            "avx512vnni" => Ok(Self::Avx512Vnni),
            other => Err(format!("unknown deferred ISA `{other}`")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegPolicy {
    Required,
    DeferredWhenIsaMissing(RequiredIsa),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasePolicy {
    pub id: String,
    pub evidence: EvidenceKind,
    pub legs: LegPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VnniDecision {
    Run,
    DeferUnsupported(RequiredIsa),
}

pub fn workload_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cross_substrate_identity")
}

fn valid_case_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Deserialize)]
struct ManifestPolicy {
    name: String,
    evidence_kind: Option<String>,
    computed_legs: Option<String>,
    required_isa: Option<String>,
}

fn manifest_policy(path: &Path) -> Result<ManifestPolicy, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("read manifest {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("parse manifest {}: {e}", path.display()))
}

pub fn policy_from_manifest(path: &Path) -> Result<CasePolicy, String> {
    let manifest = manifest_policy(path)?;
    let id = manifest.name;
    if !valid_case_id(&id) {
        return Err(format!("{}: invalid case id `{id}`", path.display()));
    }
    let dir_id = path
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("{}: manifest has no UTF-8 case directory", path.display()))?;
    if id != dir_id {
        return Err(format!(
            "{}: manifest name `{id}` does not match directory `{dir_id}`",
            path.display()
        ));
    }

    let evidence = match manifest.evidence_kind.as_deref() {
        Some(value) => EvidenceKind::parse(value)?,
        None => EvidenceKind::NativeRuntime,
    };
    if id == COMPILE_ONLY_CASE && evidence != EvidenceKind::CompilerConstruction {
        return Err(format!(
            "{id}: compile-only case must declare evidence_kind = \"compiler-construction\""
        ));
    }
    if id != COMPILE_ONLY_CASE && evidence == EvidenceKind::CompilerConstruction {
        return Err(format!(
            "{id}: only `{COMPILE_ONLY_CASE}` is a compiler-construction case"
        ));
    }

    let legs = match (
        manifest.computed_legs.as_deref(),
        manifest.required_isa.as_deref(),
    ) {
        (None, None) => LegPolicy::Required,
        (Some("deferred"), Some(isa)) => {
            LegPolicy::DeferredWhenIsaMissing(RequiredIsa::parse(isa)?)
        }
        (Some("deferred"), None) => {
            return Err(format!("{id}: deferred legs require `required_isa`"));
        }
        (Some(other), _) => return Err(format!("{id}: unknown computed_legs `{other}`")),
        (None, Some(_)) => return Err(format!("{id}: required_isa without deferred legs")),
    };

    // One ratchet owns the only accepted runtime deferral.  A manifest edit
    // cannot silently create another optional workload.
    match (&*id, legs) {
        (VNNI_CASE, LegPolicy::DeferredWhenIsaMissing(RequiredIsa::Avx512Vnni)) => {}
        (VNNI_CASE, _) => {
            return Err(format!(
                "{VNNI_CASE}: must declare the AVX-512-VNNI hardware deferral"
            ));
        }
        (_, LegPolicy::Required) => {}
        (_, LegPolicy::DeferredWhenIsaMissing(_)) => {
            return Err(format!("{id}: only `{VNNI_CASE}` may defer a computed leg"));
        }
    }

    Ok(CasePolicy { id, evidence, legs })
}

pub fn load_inventory(root: &Path) -> Result<BTreeMap<String, CasePolicy>, String> {
    let entries = std::fs::read_dir(root)
        .map_err(|e| format!("read workload root {}: {e}", root.display()))?;
    let mut inventory = BTreeMap::new();
    for entry in entries {
        let path = entry
            .map_err(|e| format!("read workload entry: {e}"))?
            .path();
        let manifest = path.join("manifest.toml");
        if !manifest.is_file() {
            continue;
        }
        let policy = policy_from_manifest(&manifest)?;
        let id = policy.id.clone();
        if inventory.insert(id.clone(), policy).is_some() {
            return Err(format!("duplicate workload id `{id}`"));
        }
    }
    if inventory.is_empty() {
        return Err(format!("no workload manifests under {}", root.display()));
    }
    Ok(inventory)
}

pub fn policy_for_id(id: &str) -> Result<CasePolicy, String> {
    if !valid_case_id(id) {
        return Err(format!("invalid case id `{id}`"));
    }
    let path = workload_root().join(id).join("manifest.toml");
    policy_from_manifest(&path)
}

pub fn vnni_decision(
    policy: &CasePolicy,
    host_has_avx512vnni: bool,
    verify: Option<&str>,
) -> Result<VnniDecision, String> {
    if policy.id != VNNI_CASE
        || policy.legs != LegPolicy::DeferredWhenIsaMissing(RequiredIsa::Avx512Vnni)
    {
        return Err("VNNI decision requires the declared VNNI case policy".to_string());
    }
    if !host_has_avx512vnni {
        return Ok(VnniDecision::DeferUnsupported(RequiredIsa::Avx512Vnni));
    }
    match verify {
        Some("1") => Ok(VnniDecision::Run),
        Some(other) => Err(format!(
            "host has AVX-512-VNNI but {VNNI_VERIFY_VAR}={other:?}; expected exactly `1`"
        )),
        None => Err(format!(
            "host has AVX-512-VNNI but {VNNI_VERIFY_VAR}=1 was not supplied"
        )),
    }
}

#[cfg(target_arch = "x86_64")]
pub fn host_has_avx512vnni() -> bool {
    std::arch::is_x86_feature_detected!("avx512vnni")
}

#[cfg(not(target_arch = "x86_64"))]
pub fn host_has_avx512vnni() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiptOutcome {
    Measured,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Receipt {
    case: String,
    outcome: ReceiptOutcome,
    evidence: EvidenceKind,
    substrate: String,
    required_isa: Option<RequiredIsa>,
}

static RECEIPT_NONCE: AtomicU64 = AtomicU64::new(0);

fn publish_receipt(dir: &Path, receipt: &Receipt) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!(
            "receipt directory {} does not exist",
            dir.display()
        ));
    }
    if !valid_case_id(&receipt.case) {
        return Err(format!("invalid receipt case id `{}`", receipt.case));
    }
    let outcome = match receipt.outcome {
        ReceiptOutcome::Measured => "measured",
        ReceiptOutcome::Deferred => "deferred",
    };
    let isa = receipt.required_isa.map_or("none", RequiredIsa::token);
    let body = format!(
        "version=1\ncase={}\noutcome={outcome}\nevidence={}\nsubstrate={}\nrequired_isa={isa}\n",
        receipt.case,
        receipt.evidence.token(),
        receipt.substrate
    );
    let nonce = RECEIPT_NONCE.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(
        ".{}.{}.{}.tmp",
        receipt.case,
        std::process::id(),
        nonce
    ));
    let final_path = dir.join(format!("{}.receipt", receipt.case));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("create receipt temp {}: {e}", tmp.display()))?;
    file.write_all(body.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("write receipt temp {}: {e}", tmp.display()))?;
    drop(file);
    let linked = std::fs::hard_link(&tmp, &final_path)
        .map_err(|e| format!("publish receipt {}: {e}", final_path.display()));
    let _ = std::fs::remove_file(&tmp);
    linked
}

fn environment_receipt_dir() -> Option<PathBuf> {
    std::env::var_os(RECEIPT_DIR_VAR).map(PathBuf::from)
}

pub fn record_measured(id: &str) {
    let Some(dir) = environment_receipt_dir() else {
        return;
    };
    assert!(
        !super::gate::bless_requested(),
        "{TARGET}/{id}: BLESS asserts nothing and cannot publish a measured receipt"
    );
    let policy = policy_for_id(id).unwrap_or_else(|e| panic!("{TARGET}/{id}: {e}"));
    let receipt = Receipt {
        case: id.to_string(),
        outcome: ReceiptOutcome::Measured,
        evidence: policy.evidence,
        substrate: host_substrate().to_string(),
        required_isa: None,
    };
    publish_receipt(&dir, &receipt).unwrap_or_else(|e| panic!("{TARGET}/{id}: {e}"));
    super::gate::completed_case(TARGET, id, policy.evidence.token());
}

pub fn record_deferred(id: &str, isa: RequiredIsa) {
    let isa_available = host_has_avx512vnni();
    assert!(
        !isa_available,
        "{TARGET}/{id}: cannot defer for `{}` when the ISA is available",
        isa.token()
    );
    let policy = policy_for_id(id).unwrap_or_else(|e| panic!("{TARGET}/{id}: {e}"));
    assert_eq!(
        policy.legs,
        LegPolicy::DeferredWhenIsaMissing(isa),
        "{TARGET}/{id}: runtime deferral is not declared by this manifest"
    );
    if let Some(dir) = environment_receipt_dir() {
        let receipt = Receipt {
            case: id.to_string(),
            outcome: ReceiptOutcome::Deferred,
            evidence: policy.evidence,
            substrate: host_substrate().to_string(),
            required_isa: Some(isa),
        };
        publish_receipt(&dir, &receipt).unwrap_or_else(|e| panic!("{TARGET}/{id}: {e}"));
    }
}

pub fn host_substrate() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "avx2"
    } else if cfg!(target_arch = "aarch64") {
        "neon"
    } else {
        "unknown"
    }
}

fn parse_receipt(path: &Path) -> Result<Receipt, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("read receipt {}: {e}", path.display()))?;
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{}: malformed line `{line}`", path.display()))?;
        if fields.insert(key, value).is_some() {
            return Err(format!("{}: duplicate field `{key}`", path.display()));
        }
    }
    let exact: BTreeSet<&str> = [
        "version",
        "case",
        "outcome",
        "evidence",
        "substrate",
        "required_isa",
    ]
    .into_iter()
    .collect();
    if fields.keys().copied().collect::<BTreeSet<_>>() != exact {
        return Err(format!("{}: receipt fields are not exact", path.display()));
    }
    if fields["version"] != "1" {
        return Err(format!("{}: unsupported receipt version", path.display()));
    }
    let outcome = match fields["outcome"] {
        "measured" => ReceiptOutcome::Measured,
        "deferred" => ReceiptOutcome::Deferred,
        other => return Err(format!("{}: unknown outcome `{other}`", path.display())),
    };
    let required_isa = match fields["required_isa"] {
        "none" => None,
        token => Some(RequiredIsa::parse(token)?),
    };
    Ok(Receipt {
        case: fields["case"].to_string(),
        outcome,
        evidence: EvidenceKind::parse(fields["evidence"])?,
        substrate: fields["substrate"].to_string(),
        required_isa,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CoverageSummary {
    pub measured_native: usize,
    pub measured_construction: usize,
    pub deferred_native: usize,
}

impl CoverageSummary {
    pub fn total(self) -> usize {
        self.measured_native + self.measured_construction + self.deferred_native
    }
}

/// Validate the receipts from the current asserting run against the manifest
/// inventory and the host's actual optional-ISA capability.
pub fn validate_inventory(
    expected: &BTreeMap<String, CasePolicy>,
    dir: &Path,
    substrate: &str,
) -> Result<CoverageSummary, Vec<String>> {
    validate_inventory_with_vnni(expected, dir, substrate, host_has_avx512vnni())
}

/// Test seam for the production inventory validator's host capability input.
///
/// The real wrapper above always reads the process' feature detector.  Tests
/// use this inner helper to exercise both sides of the VNNI rule without
/// pretending that this host has AVX-512-VNNI hardware.
pub fn validate_inventory_with_vnni(
    expected: &BTreeMap<String, CasePolicy>,
    dir: &Path,
    substrate: &str,
    host_has_vnni: bool,
) -> Result<CoverageSummary, Vec<String>> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| vec![format!("read receipt directory {}: {e}", dir.display())])?;
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    let mut summary = CoverageSummary::default();
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(e) => {
                errors.push(format!("read receipt entry: {e}"));
                continue;
            }
        };
        if path.extension() != Some(OsStr::new("receipt")) {
            errors.push(format!(
                "unexpected file in receipt directory: {}",
                path.display()
            ));
            continue;
        }
        let receipt = match parse_receipt(&path) {
            Ok(receipt) => receipt,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let file_case = path.file_stem().and_then(OsStr::to_str).unwrap_or("");
        if file_case != receipt.case {
            errors.push(format!(
                "{}: filename case `{file_case}` differs from body `{}`",
                path.display(),
                receipt.case
            ));
            continue;
        }
        if !seen.insert(receipt.case.clone()) {
            errors.push(format!("duplicate receipt for `{}`", receipt.case));
            continue;
        }
        let Some(policy) = expected.get(&receipt.case) else {
            errors.push(format!("unexpected receipt for `{}`", receipt.case));
            continue;
        };
        if receipt.substrate != substrate {
            errors.push(format!(
                "{}: substrate `{}` differs from runner `{substrate}`",
                receipt.case, receipt.substrate
            ));
        }
        if receipt.evidence != policy.evidence {
            errors.push(format!(
                "{}: evidence `{}` differs from manifest policy `{}`",
                receipt.case,
                receipt.evidence.token(),
                policy.evidence.token()
            ));
        }
        if receipt.case == VNNI_CASE {
            match (receipt.outcome, host_has_vnni) {
                (ReceiptOutcome::Deferred, true) => errors.push(format!(
                    "{VNNI_CASE}: deferred receipt is invalid because the AVX-512-VNNI \
                     detector reports available"
                )),
                (ReceiptOutcome::Measured, false) => errors.push(format!(
                    "{VNNI_CASE}: measured receipt is invalid because the AVX-512-VNNI \
                     detector reports unavailable"
                )),
                _ => {}
            }
        }
        match (
            receipt.outcome,
            receipt.required_isa,
            policy.legs,
            policy.evidence,
        ) {
            (ReceiptOutcome::Measured, None, _, EvidenceKind::NativeRuntime) => {
                summary.measured_native += 1;
            }
            (ReceiptOutcome::Measured, None, _, EvidenceKind::CompilerConstruction) => {
                summary.measured_construction += 1;
            }
            (
                ReceiptOutcome::Deferred,
                Some(actual),
                LegPolicy::DeferredWhenIsaMissing(expected_isa),
                EvidenceKind::NativeRuntime,
            ) if actual == expected_isa => summary.deferred_native += 1,
            _ => errors.push(format!(
                "{}: outcome/ISA/evidence combination is not permitted by its manifest",
                receipt.case
            )),
        }
    }
    for id in expected.keys() {
        if !seen.contains(id) {
            errors.push(format!("missing receipt for `{id}`"));
        }
    }
    errors.sort();
    if errors.is_empty() {
        Ok(summary)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
pub fn record_measured_to(policy: &CasePolicy, dir: &Path, substrate: &str) -> Result<(), String> {
    publish_receipt(
        dir,
        &Receipt {
            case: policy.id.clone(),
            outcome: ReceiptOutcome::Measured,
            evidence: policy.evidence,
            substrate: substrate.to_string(),
            required_isa: None,
        },
    )
}
