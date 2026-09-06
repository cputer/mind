// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");

use std::path::Path;
use std::sync::OnceLock;

/// Return a stable binary identity for `mindc`: canonical path, size, mtime
/// nanoseconds, and package version. Failure returns `None`, which callers must
/// treat as a cache miss rather than substituting a reusable sentinel.
pub fn compiler_identity_string(exe: &Path) -> Option<String> {
    static SELF_IDENTITY: OnceLock<Option<String>> = OnceLock::new();
    let is_self = std::env::current_exe()
        .ok()
        .map(|current| current == exe)
        .unwrap_or(false);
    if is_self {
        return SELF_IDENTITY
            .get_or_init(|| compute_compiler_identity(exe))
            .clone();
    }
    compute_compiler_identity(exe)
}

fn compute_compiler_identity(exe: &Path) -> Option<String> {
    let resolved = std::fs::canonicalize(exe).ok()?;
    let meta = std::fs::metadata(&resolved).ok()?;
    let mtime_ns = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!(
        "{}|{}|{}|{}",
        resolved.display(),
        meta.len(),
        mtime_ns,
        env!("CARGO_PKG_VERSION")
    ))
}
