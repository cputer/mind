// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Cross-process coverage for project build transactions.

mod common;

use common::require_mindc;
use libmind::build::cache::{CacheProbe, cache_root, probe};
use libmind::build::{CacheKeyFlags, compile_cache_key};
use libmind::project::{BuildTarget, EmitKind, OptimizeLevel};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;

const SIMPLE_MIND: &str = "fn main() -> i64 { 42 }\n";
const MODIFIED_MIND: &str = "fn main() -> i64 { 99 }\n";

fn make_project(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"concurrent_project\"\nversion = \"0.1.0\"\n\n\
         [build]\nentry = \"src/main.mind\"\n",
    )
    .unwrap();
    fs::write(root.join("src/main.mind"), SIMPLE_MIND).unwrap();
}

#[test]
fn same_project_builds_serialize_without_manifest_or_cache_corruption() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    make_project(root);
    let manifest_before = fs::read(root.join("Mind.toml")).unwrap();
    let mindc = require_mindc();

    let spawn = || {
        let binary = mindc.clone();
        let project = root.to_path_buf();
        thread::spawn(move || {
            Command::new(binary)
                .arg("build")
                .current_dir(project)
                .output()
                .expect("spawn project build")
        })
    };
    let first = spawn();
    let second = spawn();
    let first = first.join().expect("first build panicked");
    let second = second.join().expect("second build panicked");

    assert_eq!(fs::read(root.join("Mind.toml")).unwrap(), manifest_before);
    assert!(root.join(".mind-build.lock").is_file());
    assert!(!root.join("target/.mind-build.lock").exists());
    let capable_first = common::gate::compiled("mindc_project_lock", &first);
    let capable_second = common::gate::compiled("mindc_project_lock", &second);
    assert_eq!(capable_first, capable_second);
    if !capable_first {
        return;
    }

    let key = compile_cache_key(
        SIMPLE_MIND.as_bytes(),
        CacheKeyFlags {
            target: BuildTarget::Cpu,
            optimize: OptimizeLevel::Debug,
            emit: EmitKind::Binary,
            edition: 2024,
        },
        &mindc,
        root,
        &[root.join("src/main.mind")],
    )
    .expect("compiler identity");
    let cache = cache_root(root, BuildTarget::Cpu, OptimizeLevel::Debug);
    let meta = libmind::build::cache::meta_path(&cache, &key);
    let parsed: serde_json::Value = serde_json::from_slice(&fs::read(meta).unwrap()).unwrap();
    assert!(parsed.is_object());
    assert!(matches!(probe(&cache, &key), CacheProbe::Hit { .. }));
}

#[test]
fn standalone_builds_leave_no_temporary_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let source_a = root.join("alpha.mind");
    let source_b = root.join("beta.mind");
    fs::write(&source_a, SIMPLE_MIND).unwrap();
    fs::write(&source_b, MODIFIED_MIND).unwrap();
    let mindc = require_mindc();

    let spawn = |source: &Path, output: &Path| {
        let binary = mindc.clone();
        let project = root.to_path_buf();
        let source = source.to_path_buf();
        let output = output.to_path_buf();
        thread::spawn(move || {
            Command::new(binary)
                .args(["build", "--emit=cdylib", "--out"])
                .arg(output)
                .arg(source)
                .current_dir(project)
                .output()
                .expect("spawn standalone build")
        })
    };
    let first = spawn(&source_a, &root.join("alpha.so"));
    let second = spawn(&source_b, &root.join("beta.so"));
    let first = first.join().expect("alpha build panicked");
    let second = second.join().expect("beta build panicked");

    let capable_first = common::gate::compiled("mindc_project_lock", &first);
    let capable_second = common::gate::compiled("mindc_project_lock", &second);
    assert_eq!(capable_first, capable_second);
    assert!(source_a.is_file());
    assert!(source_b.is_file());
    assert!(!root.join("Mind.toml").exists());
}
