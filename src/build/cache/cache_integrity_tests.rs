// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");

use super::{
    CacheInputInventory, CacheKeyMaterial, CacheProbe, ObjectMeta, meta_path, object_path, probe,
    sha256_hex, write_object,
};
use serde_json::Value;
use std::fs;

const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OBJECT: &[u8] = b"\x7fELF integrity fixture";

fn material() -> CacheKeyMaterial {
    CacheKeyMaterial {
        key: KEY.to_string(),
        input_inventory: CacheInputInventory {
            key_format: "mindc-cache-v2".to_string(),
            compiler_version: "0.10.2+/mindc|1|2|0.10.2".to_string(),
            edition: 2024,
            target: "Cpu".to_string(),
            optimize: "Debug".to_string(),
            dependencies: vec!["src0000=src/main.mind|fixture".to_string()],
            source_sha256: sha256_hex(b"source fixture"),
        },
    }
}

fn metadata(material: &CacheKeyMaterial) -> ObjectMeta {
    ObjectMeta {
        source_path: "src/main.mind".to_string(),
        cache_key: material.key.clone(),
        target: "Cpu".to_string(),
        optimize: "Debug".to_string(),
        compiler_version: "0.10.2+/mindc|1|2|0.10.2".to_string(),
        compiler_fingerprint: "/mindc|1|2|0.10.2".to_string(),
        dep_hashes: vec!["src0000=src/main.mind|fixture".to_string()],
        input_inventory: material.input_inventory.clone(),
    }
}

fn populated_cache() -> (tempfile::TempDir, CacheKeyMaterial) {
    let tmp = tempfile::tempdir().expect("temp cache");
    let material = material();
    write_object(tmp.path(), &material, OBJECT, &metadata(&material))
        .expect("publish cache object");
    (tmp, material)
}

#[test]
fn sidecar_records_the_actual_artifact_and_key_inputs() {
    let (tmp, material) = populated_cache();
    let value: Value =
        serde_json::from_slice(&fs::read(meta_path(tmp.path(), KEY)).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["metadata"]["cache_key"], KEY);
    assert_eq!(
        value["metadata"]["input_inventory"]["dependencies"][0],
        "src0000=src/main.mind|fixture"
    );
    assert_eq!(value["artifact_size"], OBJECT.len() as u64);
    assert_eq!(value["artifact_sha256"], sha256_hex(OBJECT));
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Hit { object_bytes, .. } if object_bytes == OBJECT
    ));
}

#[test]
fn modified_object_is_a_cache_miss() {
    let (tmp, material) = populated_cache();
    fs::write(object_path(tmp.path(), KEY), b"modified artifact").unwrap();
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Miss { .. }
    ));
}

#[test]
fn truncated_object_is_a_cache_miss() {
    let (tmp, material) = populated_cache();
    fs::write(object_path(tmp.path(), KEY), &OBJECT[..5]).unwrap();
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Miss { .. }
    ));
}

#[test]
fn malformed_or_unknown_metadata_is_a_cache_miss() {
    let (tmp, material) = populated_cache();
    fs::write(meta_path(tmp.path(), KEY), b"{not json").unwrap();
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Miss { .. }
    ));

    write_object(tmp.path(), &material, OBJECT, &metadata(&material)).unwrap();
    let path = meta_path(tmp.path(), KEY);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("future_unverified_field".into(), Value::Bool(true));
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Miss { .. }
    ));
}

#[test]
fn metadata_for_a_different_key_is_a_cache_miss() {
    let (tmp, material) = populated_cache();
    let path = meta_path(tmp.path(), KEY);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["metadata"]["cache_key"] =
        Value::String("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into());
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Miss { .. }
    ));
}

#[test]
fn modified_input_inventory_is_a_cache_miss() {
    let (tmp, material) = populated_cache();
    let path = meta_path(tmp.path(), KEY);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["metadata"]["input_inventory"]["edition"] = Value::from(2025);
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Miss { .. }
    ));
}

#[test]
fn self_consistent_sidecar_for_different_expected_inputs_is_a_miss() {
    let (tmp, expected) = populated_cache();
    let mut different = expected.clone();
    different.input_inventory.edition = 2025;

    assert!(
        write_object(tmp.path(), &expected, OBJECT, &metadata(&different)).is_err(),
        "publication must reject metadata not derived with the supplied material"
    );
    write_object(tmp.path(), &different, OBJECT, &metadata(&different)).unwrap();
    assert!(matches!(
        probe(tmp.path(), &expected),
        CacheProbe::Miss { .. }
    ));
}

#[test]
fn a_hit_cannot_be_changed_between_probe_and_restore() {
    let (tmp, material) = populated_cache();
    let verified = match probe(tmp.path(), &material) {
        CacheProbe::Hit { object_bytes, .. } => object_bytes,
        CacheProbe::Miss { .. } => panic!("fresh entry must hit"),
    };
    fs::write(
        object_path(tmp.path(), KEY),
        b"replacement after validation",
    )
    .unwrap();
    assert_eq!(verified, OBJECT);
}

#[test]
fn mismatched_artifact_size_or_digest_is_a_cache_miss() {
    let (tmp, material) = populated_cache();
    let path = meta_path(tmp.path(), KEY);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["artifact_size"] = Value::from(OBJECT.len() as u64 + 1);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Miss { .. }
    ));

    write_object(tmp.path(), &material, OBJECT, &metadata(&material)).unwrap();
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["artifact_sha256"] = Value::String("0".repeat(64));
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        probe(tmp.path(), &material),
        CacheProbe::Miss { .. }
    ));
}
