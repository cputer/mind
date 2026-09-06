// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg(all(unix, target_os = "linux", feature = "mlir-build"))]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn project(stem: &str, source: &str, backend: &str) -> (PathBuf, PathBuf) {
    let root = common::scratch_dir(stem);
    let user_dir = root.join("isolated-user");
    let empty_lib = root.join("empty-runtime-lib");
    fs::create_dir_all(root.join("src")).expect("create source directory");
    fs::create_dir_all(&user_dir).expect("create isolated user directory");
    fs::create_dir_all(&empty_lib).expect("create empty runtime directory");
    fs::write(root.join("src/main.mind"), source).expect("write source");
    fs::write(
        root.join("Mind.toml"),
        format!(
            "[package]\nname = \"{stem}\"\nversion = \"0.1.0\"\n\n\
             [build]\nentry = \"src/main.mind\"\n\n\
             [targets.cpu]\nbackend = \"{backend}\"\n"
        ),
    )
    .expect("write manifest");
    (root, empty_lib)
}

fn build(root: &Path, empty_lib: &Path) -> Output {
    Command::new(common::mindc_bin())
        .args(["build", "--no-cache"])
        .current_dir(root)
        // These are subprocess-only test inputs. Together they prove the build
        // cannot see an ambient ~/.mind installation or a private link path.
        .env("HOME", root.join("isolated-user"))
        .env("MIND_LIB_DIR", empty_lib)
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LIBRARY_PATH")
        .output()
        .expect("spawn mindc build")
}

#[test]
fn cpu_binary_needs_no_installed_runtime_and_is_deterministic() {
    let (root, empty_lib) = project(
        "public_cpu_native_link",
        "fn main() -> i64 { return 0; }\n",
        "cpu",
    );

    let first = build(&root, &empty_lib);
    assert!(
        first.status.success(),
        "public CPU build failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let artifact = root.join("target/debug/public_cpu_native_link");
    let first_bytes = fs::read(&artifact).expect("read first artifact");
    assert_eq!(
        &first_bytes[..4],
        b"\x7fELF",
        "artifact is not a native ELF"
    );

    let run = Command::new(&artifact)
        .output()
        .expect("run native artifact");
    assert_eq!(run.status.code(), Some(0), "native artifact failed");

    let dynamic = Command::new("readelf")
        .args(["-d"])
        .arg(&artifact)
        .output()
        .expect("run readelf");
    assert!(dynamic.status.success(), "readelf rejected native artifact");
    let dynamic = String::from_utf8_lossy(&dynamic.stdout).to_ascii_lowercase();
    assert!(
        !dynamic.contains("libmind") && !dynamic.contains(".mind/lib"),
        "native ELF retained a private MIND runtime dependency:\n{dynamic}"
    );

    let second = build(&root, &empty_lib);
    assert!(
        second.status.success(),
        "repeat public CPU build failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        first_bytes,
        fs::read(&artifact).expect("read repeated artifact"),
        "two --no-cache builds produced different native ELF bytes"
    );
}

#[test]
fn unresolved_native_symbol_fails_without_writing_a_launcher() {
    let (root, empty_lib) = project(
        "public_cpu_missing_symbol",
        "extern \"C\" { safe fn public_cpu_symbol_that_does_not_exist() -> i64; }\n\
         fn main() -> i64 { return public_cpu_symbol_that_does_not_exist(); }\n",
        "cpu",
    );
    let artifact = root.join("target/debug/public_cpu_missing_symbol");
    fs::create_dir_all(artifact.parent().expect("artifact parent")).expect("create target dir");
    fs::write(&artifact, b"#!/bin/sh\nexit 0\n").expect("seed stale launcher");
    let out = build(&root, &empty_lib);
    assert!(
        !out.status.success(),
        "unresolved native symbol linked successfully"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("public_cpu_symbol_that_does_not_exist"),
        "link failure did not identify the unresolved symbol:\n{stderr}"
    );
    assert!(
        !artifact.exists(),
        "failed native link left a stale artifact that could be mistaken for a binary"
    );
}

#[test]
fn invalid_module_fails_before_any_native_artifact() {
    let (root, empty_lib) = project(
        "public_cpu_invalid_module",
        "fn main( -> i64 { return 0; }\n",
        "cpu",
    );
    let artifact = root.join("target/debug/public_cpu_invalid_module");
    fs::create_dir_all(artifact.parent().expect("artifact parent")).expect("create target dir");
    fs::write(&artifact, b"\x7fELF stale").expect("seed stale executable");
    let out = build(&root, &empty_lib);
    assert!(!out.status.success(), "invalid module built successfully");
    assert!(
        !artifact.exists(),
        "invalid module left a stale runnable artifact"
    );
}

#[test]
fn accelerator_still_requires_its_installed_runtime() {
    let (root, empty_lib) = project(
        "public_cpu_accelerator_control",
        "fn main() -> i64 { return 0; }\n",
        "cuda",
    );
    let out = build(&root, &empty_lib);
    assert!(
        !out.status.success(),
        "CUDA build bypassed its runtime requirement"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("MIND runtime not found for backend 'cuda'"),
        "CUDA build no longer checked its installed runtime requirement:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
