// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Part of the MIND project (Machine Intelligence Native Design).

use std::fs;
use std::process::Command;

use tempfile::tempdir;

/// Shared fail-closed capability gate (`common::gate`): a skip must panic
/// under `MIND_BENCH_REQUIRE=1` and otherwise report `ran=0`.
mod common;

#[test]
fn stdout_emit_default_lowering() {
    let binary = crate::common::mind_bin();
    if !binary.exists() {
        crate::common::gate::skipped(
            "mlir_file_and_lower",
            &format!("Skipping: mind binary not found at {:?}", binary),
        );
        return;
    }

    let out = Command::new(&binary)
        .args(["eval", "1+2", "--emit-mlir", "--mlir-lower", "none"])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stdout run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("module"));
    assert!(s.contains("arith.constant"));
}

#[test]
fn file_emit_with_preset() {
    let binary = crate::common::mind_bin();
    if !binary.exists() {
        crate::common::gate::skipped(
            "mlir_file_and_lower",
            &format!("Skipping: mind binary not found at {:?}", binary),
        );
        return;
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("out.mlir");

    let status = Command::new(&binary)
        .args([
            "eval",
            "let x: Tensor[f32,(2,3)] = 0; x+1",
            "--emit-mlir-file",
            path.to_str().unwrap(),
            "--mlir-lower",
            "arith-linalg",
        ])
        .status()
        .expect("run");
    assert!(status.success());

    let txt = fs::read_to_string(&path).expect("read");
    assert!(txt.contains("tensor.empty") || txt.contains("linalg.fill"));
    assert!(txt.contains("arith.constant"));
}
