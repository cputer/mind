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

mod common;
use common::{mindc_bin, require_mindc};

use std::process::Command;

// Get the path to the mindc binary from the cargo target directory
// mindc_bin() provided by tests/common (CARGO_BIN_EXE_mindc — staleness-free)

#[test]
fn mindc_emits_ir() {
    let binary = require_mindc();

    let output = Command::new(&binary)
        .args(["tests/fixtures/simple.mind", "--emit-ir"])
        .output()
        .expect("run mindc");

    assert!(
        output.status.success(),
        "mindc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.to_lowercase().contains("output"), "{stdout}");
}

#[test]
fn mindc_accepts_cpu_target_flag() {
    let binary = require_mindc();

    let output = Command::new(&binary)
        .args(["tests/fixtures/simple.mind", "--emit-ir", "--target", "cpu"])
        .output()
        .expect("run mindc with cpu target");

    assert!(
        output.status.success(),
        "mindc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.to_lowercase().contains("output"), "{stdout}");
}

#[cfg(feature = "autodiff")]
#[test]
fn mindc_emits_grad_ir() {
    let binary = require_mindc();

    let output = Command::new(&binary)
        .args([
            "tests/fixtures/autodiff.mind",
            "--func",
            "main",
            "--autodiff",
            "--emit-grad-ir",
        ])
        .output()
        .expect("run mindc autodiff");

    assert!(
        output.status.success(),
        "mindc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.to_lowercase().contains("output"), "{stdout}");
}

#[test]
fn mindc_verify_only_mode() {
    let binary = require_mindc();

    let status = Command::new(&binary)
        .args(["tests/fixtures/simple.mind", "--verify-only"])
        .status()
        .expect("run mindc verify");

    assert!(status.success());
}

#[test]
fn mindc_reports_prefixed_errors() {
    let binary = require_mindc();

    let output = Command::new(&binary)
        .args(["tests/fixtures/invalid.mind"])
        .output()
        .expect("run mindc error path");

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error[parse]")
            || stderr.contains("error[type-check]")
            || stderr.contains("error[ir-verify]"),
        "stderr should include standardized prefix: {stderr}"
    );
}

#[test]
fn mindc_reports_unavailable_gpu_backend() {
    let binary = require_mindc();

    let output = Command::new(&binary)
        .args(["tests/fixtures/simple.mind", "--target", "gpu"])
        .output()
        .expect("run mindc gpu target");

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(stderr.contains("error[backend]"));
    assert!(stderr.contains("no backend available"));
}

#[test]
fn mindc_prints_json_diagnostics_with_flag() {
    let binary = require_mindc();

    let output = Command::new(&binary)
        .args(["tests/fixtures/invalid.mind", "--diagnostic-format", "json"])
        .output()
        .expect("run mindc json diagnostics");

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Find the first line that looks like JSON (starts with '{')
    let json_line = stderr
        .lines()
        .find(|line| line.trim().starts_with('{'))
        .expect("should have json diagnostic line");
    let value: serde_json::Value = serde_json::from_str(json_line).expect("json diagnostic");
    assert!(value["phase"].is_string());
    assert_eq!(value["severity"], "error");
}

#[test]
fn mindc_reports_shape_errors_with_codes() {
    let binary = require_mindc();

    let output = Command::new(&binary)
        .args([
            "tests/fixtures/invalid_broadcast.mind",
            "--diagnostic-format",
            "json",
        ])
        .output()
        .expect("run mindc shape error");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Find the first line that looks like JSON (starts with '{')
    let json_line = stderr
        .lines()
        .find(|line| line.trim().starts_with('{'))
        .expect("should have json diagnostic line");
    let value: serde_json::Value = serde_json::from_str(json_line).expect("json diagnostic");
    assert_eq!(value["code"], "E2101");
    assert_eq!(value["phase"], "type-check");
}

#[test]
fn mindc_color_env_overridden_by_flag() {
    let binary = require_mindc();

    let output = Command::new(&binary)
        .env("MINDC_COLOR", "always")
        // Ensure no terminal is detected
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .args(["tests/fixtures/invalid.mind", "--color", "never"])
        .output()
        .expect("run mindc color flag");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Find lines that look like mindc output (not cargo warnings)
    let mindc_output: String = stderr
        .lines()
        .filter(|line| line.contains("error[") || line.contains("warning["))
        .collect::<Vec<_>>()
        .join("\n");
    // If no mindc-specific output found, check the full stderr
    let to_check = if mindc_output.is_empty() {
        stderr.to_string()
    } else {
        mindc_output
    };
    assert!(
        !to_check.contains("\u{1b}["),
        "mindc output should be uncolored when flag forces never: {to_check}"
    );
}

#[test]
fn mindc_runs_conformance_suite() {
    // No early return on a missing binary: `mindc_bin()` is CARGO_BIN_EXE_mindc,
    // which cargo builds for this test target, so an absent binary is a broken
    // gate, not a reason to report a silent pass.
    let binary = mindc_bin();
    assert!(
        binary.exists(),
        "mindc binary missing at {binary:?}; the conformance gate cannot run"
    );

    let output = Command::new(&binary)
        .args(["conformance", "--profile", "cpu"])
        .output()
        .expect("run mindc conformance");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The suite must report how many cases it executed; a pass with ran=0 is a
    // vacuous attestation.
    assert!(
        stdout.contains("ran="),
        "conformance must report its case count: {stdout}"
    );
    assert!(
        !stdout.contains("ran=0"),
        "conformance reported a pass having run nothing: {stdout}"
    );
}

/// `--profile gpu` on a binary built without the `mlir-gpu` feature has zero GPU
/// cases to run, and must exit non-zero naming the empty list instead of
/// printing "conformance passed for profile: CpuAndGpu".
#[test]
fn mindc_conformance_gpu_profile_fails_closed() {
    let binary = mindc_bin();
    assert!(
        binary.exists(),
        "mindc binary missing at {binary:?}; the conformance gate cannot run"
    );

    let output = Command::new(&binary)
        .args(["conformance", "--profile", "gpu"])
        .output()
        .expect("run mindc conformance --profile gpu");

    assert!(
        !output.status.success(),
        "gpu profile with no compiled-in GPU cases must exit non-zero, stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("no gpu cases compiled in"),
        "gpu failure must name the empty case list, stderr: {stderr}"
    );
}
