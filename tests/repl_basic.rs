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

use std::io::Write;
use std::process::Command;
use std::process::Stdio;

/// Shared fail-closed capability gate (`common::gate`): a skip must panic
/// under `MIND_BENCH_REQUIRE=1` and otherwise report `ran=0`.
mod common;

#[test]
fn repl_accepts_statements_and_expressions() {
    let binary = crate::common::mind_bin();
    if !binary.exists() {
        crate::common::gate::skipped(
            "repl_basic",
            &format!("Skipping: mind binary not found at {:?}", binary),
        );
        return;
    }

    let mut child = Command::new(&binary)
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn repl");

    let mut stdin = child.stdin.take().expect("stdin");
    // Feed a small session
    writeln!(stdin, "let x = 2;").unwrap();
    writeln!(stdin, "x * 3").unwrap();
    writeln!(stdin, ":quit").unwrap();
    drop(stdin);

    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success());

    let stdout = String::from_utf8_lossy(&out.stdout);
    // Expect to see the result '6' somewhere in the output
    assert!(stdout.contains("6"), "stdout was: {}", stdout);
}
