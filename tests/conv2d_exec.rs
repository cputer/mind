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

#[cfg(all(feature = "cpu-exec", feature = "cpu-conv"))]
#[cfg(all(feature = "cpu-exec", feature = "cpu-conv"))]
use std::process::Command;

/// Shared fail-closed capability gate (`common::gate`): a skip must panic
/// under `MIND_BENCH_REQUIRE=1` and otherwise report `ran=0`.
mod common;

/// Get the path to the mind binary from the cargo target directory
#[cfg(all(feature = "cpu-exec", feature = "cpu-conv"))]
#[cfg(all(feature = "cpu-exec", feature = "cpu-conv"))]
#[test]
fn conv2d_valid_runs() {
    let binary = crate::common::mind_bin();
    if !binary.exists() {
        crate::common::gate::skipped(
            "conv2d_exec",
            &format!("Skipping: mind binary not found at {:?}", binary),
        );
        return;
    }

    let src = r#"
        let x: Tensor[f32,(1,3,3,1)] = 1;
        let w: Tensor[f32,(2,2,1,1)] = 1;
        tensor.conv2d(x, w, stride_h=1, stride_w=1, padding="valid")
    "#;

    let output = Command::new(&binary)
        .args(["eval", "--exec", src])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // In open-core build, conv2d stubs return Unsupported. With proprietary
    // runtime, the operation executes and produces output shape (1,2,2,1).
    let has_expected_shape = stdout.contains("(1,2,2,1)");
    let has_unsupported_error =
        stderr.contains("proprietary MIND runtime") || stdout.contains("proprietary MIND runtime");

    assert!(
        has_expected_shape || has_unsupported_error,
        "expected either shape (1,2,2,1) or runtime stub error. stdout: {stdout}, stderr: {stderr}"
    );
}
