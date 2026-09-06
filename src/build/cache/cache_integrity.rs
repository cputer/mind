// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");

//! Versioned cache-sidecar encoding and fail-closed integrity checks.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{ObjectMeta, sha256_hex};

const SIDECAR_SCHEMA_VERSION: u32 = 1;

/// Inputs committed by a cache key, represented without duplicating source bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheInputInventory {
    /// Cache-key format discriminator.
    pub key_format: String,
    /// Package version plus compiler executable identity.
    pub compiler_version: String,
    /// Source-language edition.
    pub edition: u32,
    /// Exact target spelling included in the key preimage.
    pub target: String,
    /// Exact optimization spelling included in the key preimage.
    pub optimize: String,
    /// Sorted dependency, source-set, emit, and toolchain entries.
    pub dependencies: Vec<String>,
    /// Digest of the entry bytes included at the end of the key preimage.
    pub source_sha256: String,
}

/// The key and exact inventory generated together from one key preimage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKeyMaterial {
    pub(crate) key: String,
    pub(crate) input_inventory: CacheInputInventory,
}

impl CacheKeyMaterial {
    /// Content-addressed key derived from this material.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Exact inputs the key commits.
    pub fn input_inventory(&self) -> &CacheInputInventory {
        &self.input_inventory
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredObjectMeta {
    schema_version: u32,
    metadata: ObjectMeta,
    key_inventory_sha256: String,
    artifact_size: u64,
    artifact_sha256: String,
}

pub(crate) fn is_cache_key(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_input_inventory(meta: &ObjectMeta) -> bool {
    let inputs = &meta.input_inventory;
    inputs.key_format == "mindc-cache-v2"
        && inputs.compiler_version == meta.compiler_version
        && inputs.target == meta.target
        && inputs.optimize == meta.optimize
        && inputs.dependencies == meta.dep_hashes
        && inputs
            .dependencies
            .windows(2)
            .all(|pair| pair[0] <= pair[1])
        && is_cache_key(&inputs.source_sha256)
}

pub(crate) fn encode_sidecar(
    material: &CacheKeyMaterial,
    object_bytes: &[u8],
    meta: &ObjectMeta,
) -> Result<Vec<u8>> {
    if !is_cache_key(&material.key)
        || meta.cache_key != material.key
        || meta.input_inventory != material.input_inventory
        || !valid_input_inventory(meta)
    {
        anyhow::bail!("refuse cache publication with an invalid key or input inventory");
    }
    let inventory_bytes =
        serde_json::to_vec(&meta.input_inventory).context("serialise cache input inventory")?;
    serde_json::to_vec_pretty(&StoredObjectMeta {
        schema_version: SIDECAR_SCHEMA_VERSION,
        metadata: meta.clone(),
        key_inventory_sha256: key_inventory_sha256(&material.key, &inventory_bytes),
        artifact_size: object_bytes.len() as u64,
        artifact_sha256: sha256_hex(object_bytes),
    })
    .context("serialise meta")
}

pub(crate) fn parse_sidecar(
    meta_bytes: &[u8],
    material: &CacheKeyMaterial,
) -> Option<StoredObjectMeta> {
    let stored: StoredObjectMeta = serde_json::from_slice(meta_bytes).ok()?;
    let inventory_bytes = serde_json::to_vec(&stored.metadata.input_inventory).ok()?;
    (stored.schema_version == SIDECAR_SCHEMA_VERSION
        && stored.metadata.cache_key == material.key
        && stored.metadata.input_inventory == material.input_inventory
        && valid_input_inventory(&stored.metadata)
        && stored.key_inventory_sha256 == key_inventory_sha256(&material.key, &inventory_bytes)
        && is_cache_key(&stored.artifact_sha256))
    .then_some(stored)
}

fn key_inventory_sha256(key: &str, inventory_bytes: &[u8]) -> String {
    let mut binding = Vec::with_capacity(key.len() + 1 + inventory_bytes.len());
    binding.extend_from_slice(key.as_bytes());
    binding.push(0);
    binding.extend_from_slice(inventory_bytes);
    sha256_hex(&binding)
}

pub(crate) fn artifact_matches(stored: &StoredObjectMeta, object_bytes: &[u8]) -> bool {
    stored.artifact_size == object_bytes.len() as u64
        && stored.artifact_sha256 == sha256_hex(object_bytes)
}
