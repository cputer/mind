// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");

use crate::build::{BuildOpts, run_build};
use crate::project::{BuildTarget, EmitKind, OptimizeLevel, source_snapshot};
use std::fs;

const SOURCE_ONE: &str = "pub fn entry() -> i64 { return 1; }\nfn main() -> i32 { return 0; }\n";
const SOURCE_TWO: &str = "pub fn entry() -> i64 { return 2; }\nfn main() -> i32 { return 0; }\n";
const BINARY_ONE: &str = "fn main() -> i32 { return 1; }\n";
const BINARY_TWO: &str = "fn main() -> i32 { return 2; }\n";

unsafe fn entry_value(path: &std::path::Path) -> i64 {
    let library = unsafe { libloading::Library::new(path) }.expect("load emitted cdylib");
    let entry: libloading::Symbol<'_, unsafe extern "C" fn() -> i64> =
        unsafe { library.get(b"entry") }.expect("resolve entry export");
    unsafe { entry() }
}

#[test]
fn snapshot_key_preserves_the_public_cache_key_contract() {
    let tmp = tempfile::tempdir().expect("temp project");
    let source = tmp.path().join("main.mind");
    fs::write(&source, SOURCE_ONE).expect("write source");
    let sources = vec![source.clone()];
    let snapshot = source_snapshot::SourceSnapshot::capture(&sources).expect("capture source");
    let flags = super::CacheKeyFlags {
        target: BuildTarget::Cpu,
        optimize: OptimizeLevel::Debug,
        emit: EmitKind::Cdylib,
        edition: 2024,
    };
    let compiler = std::env::current_exe().expect("current compiler identity");
    assert_eq!(
        super::compile_cache_key_from_snapshot(&snapshot, &source, flags, &compiler, tmp.path(),),
        super::compile_cache_key(
            SOURCE_ONE.as_bytes(),
            flags,
            &compiler,
            tmp.path(),
            &sources,
        )
    );
}

#[test]
fn source_edit_after_key_never_publishes_artifact_under_stale_key() {
    let tmp = tempfile::tempdir().expect("temp project");
    let root = tmp.path();
    let source = root.join("main.mind");
    fs::write(&source, SOURCE_ONE).expect("write initial source");
    fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"snapshot_cache\"\nversion = \"0.1.0\"\n\n\
         [build]\nentry = \"main.mind\"\nemit = \"cdylib\"\n\n\
         [targets.cpu]\nbackend = \"cpu\"\nsources = [\"main.mind\"]\n\n\
         [exports]\nc_abi = [\"entry\"]\n",
    )
    .expect("write manifest");

    let first = root.join("first.so");
    let first_opts = BuildOpts {
        paths: vec![source.clone()],
        target: Some("cpu".to_string()),
        emit: Some(EmitKind::Cdylib),
        out: Some(first.clone()),
        ..BuildOpts::default()
    };
    let edited_source = source.clone();
    source_snapshot::install_test_hook(move || {
        fs::write(edited_source, SOURCE_TWO).expect("edit source after key selection")
    });
    let first_build = run_build(&first_opts).expect("first build");
    assert_eq!(first_build.cache_stats.misses, 1);
    let first_value = unsafe { entry_value(&first) };

    fs::write(&source, SOURCE_ONE).expect("restore original source");
    let second = root.join("second.so");
    let second_build = run_build(&BuildOpts {
        out: Some(second.clone()),
        ..first_opts
    })
    .expect("second build");
    assert_eq!(second_build.cache_stats.hits, 1);
    let second_value = unsafe { entry_value(&second) };
    assert_eq!((first_value, second_value), (1, 1));
    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}

#[test]
fn executable_also_compiles_and_caches_the_captured_source() {
    let tmp = tempfile::tempdir().expect("temp project");
    let root = tmp.path();
    let source = root.join("main.mind");
    fs::write(&source, BINARY_ONE).expect("write initial source");
    fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"snapshot_exe\"\nversion = \"0.1.0\"\n\n\
         [build]\nentry = \"main.mind\"\nemit = \"binary\"\n\n\
         [targets.cpu]\nbackend = \"cpu\"\nsources = [\"main.mind\"]\n",
    )
    .expect("write manifest");

    let first = root.join("first-bin");
    let first_opts = BuildOpts {
        paths: vec![source.clone()],
        target: Some("cpu".to_string()),
        emit: Some(EmitKind::Binary),
        out: Some(first.clone()),
        ..BuildOpts::default()
    };
    let edited_source = source.clone();
    source_snapshot::install_test_hook(move || {
        fs::write(edited_source, BINARY_TWO).expect("edit source after key selection")
    });
    let first_build = run_build(&first_opts).expect("first executable build");
    assert_eq!(first_build.cache_stats.misses, 1);
    let first_status = std::process::Command::new(&first)
        .status()
        .expect("run first executable");

    fs::write(&source, BINARY_ONE).expect("restore original source");
    let second = root.join("second-bin");
    let second_build = run_build(&BuildOpts {
        out: Some(second.clone()),
        ..first_opts
    })
    .expect("second executable build");
    assert_eq!(second_build.cache_stats.hits, 1);
    let second_status = std::process::Command::new(&second)
        .status()
        .expect("run second executable");
    assert_eq!(
        (first_status.code(), second_status.code()),
        (Some(1), Some(1))
    );
    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}
