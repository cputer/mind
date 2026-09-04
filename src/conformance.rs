use crate::eval::value::Value;
use crate::eval::{ExecMode, eval_module_value_with_env_mode};
use crate::ir::IRModule;
use crate::ir::compact::v3::{emit_mic3, parse_mic3};
use crate::pipeline::{CompileOptions, compile_source};
use crate::runtime::types::BackendTarget;

#[cfg(any(feature = "mlir-lowering", feature = "mlir-build"))]
use crate::pipeline::{MlirProducts, lower_to_mlir};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConformanceProfile {
    CpuBaseline,
    CpuAndGpu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceOptions {
    pub profile: ConformanceProfile,
}

#[derive(Debug, thiserror::Error)]
#[error("conformance failures: {0:?}")]
pub struct ConformanceFailure(pub Vec<String>);

#[derive(Debug)]
struct ConformanceCase {
    name: &'static str,
    source: &'static str,
    target: BackendTarget,
    func: Option<&'static str>,
    expected_ir: &'static str,
    expected_value: Option<ExpectedValue>,
    #[cfg_attr(not(feature = "mlir-lowering"), allow(dead_code))]
    expected_mlir: Option<&'static str>,
    #[cfg(feature = "autodiff")]
    expected_grad_ir: Option<&'static str>,
    expected_error: Option<&'static str>,
    run_autodiff: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpectedValue {
    Int(i64),
}

/// Per-profile execution counts for one conformance run.
///
/// A gate's exit code answers "did anything that ran fail", never "did the gate
/// run anything" — so the suite reports how many cases it actually executed and
/// the caller prints it. An empty case list for a REQUESTED profile is a
/// failure (see [`run_conformance`]), not a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConformanceReport {
    /// The profile that was requested.
    pub profile: ConformanceProfile,
    /// Number of CPU-baseline cases executed.
    pub cpu_ran: usize,
    /// Number of GPU-profile cases executed (always 0 for `CpuBaseline`).
    pub gpu_ran: usize,
}

impl ConformanceReport {
    /// Total number of cases executed across the profile's case lists.
    pub fn total_ran(&self) -> usize {
        self.cpu_ran + self.gpu_ran
    }
}

/// Failure text for a requested profile whose case list is empty. Kept as a
/// constant so the CLI, the tests and the CI gate all assert the same string
/// instead of three hand-copied spellings of the same policy.
pub const NO_CPU_CASES: &str = "cpu: no CPU cases compiled in (0 cases ran); \
                                nothing was verified — treating as failure";

/// Failure text for a requested GPU profile with no compiled-in GPU cases.
pub const NO_GPU_CASES: &str = "gpu: no GPU cases compiled in (0 cases ran); \
                                the `mlir-gpu` feature was not enabled at build \
                                time, so nothing was verified for this profile — \
                                treating as failure";

/// Run the Core v1 conformance suite for one profile.
///
/// Fails closed on an empty case list: a profile that executes zero cases
/// verified nothing, and printing a pass for it is a false attestation (the
/// docs sell a passing profile as evidence of API/IR/MLIR stability). This
/// mirrors the zero-test policy `mindc test` already applies to a discovery
/// that finds nothing to run.
pub fn run_conformance(opts: ConformanceOptions) -> Result<ConformanceReport, ConformanceFailure> {
    let mut failures = Vec::new();

    let cpu = cpu_cases();
    if cpu.is_empty() {
        failures.push(NO_CPU_CASES.to_string());
    }
    for case in &cpu {
        if let Err(msg) = run_case(case) {
            failures.push(format!("cpu:{} => {msg}", case.name));
        }
    }

    let mut gpu_ran = 0;
    if matches!(opts.profile, ConformanceProfile::CpuAndGpu) {
        let gpu = gpu_cases();
        if gpu.is_empty() {
            failures.push(NO_GPU_CASES.to_string());
        }
        gpu_ran = gpu.len();
        for case in &gpu {
            if let Err(msg) = run_case(case) {
                failures.push(format!("gpu:{} => {msg}", case.name));
            }
        }
    }

    if failures.is_empty() {
        Ok(ConformanceReport {
            profile: opts.profile,
            cpu_ran: cpu.len(),
            gpu_ran,
        })
    } else {
        Err(ConformanceFailure(failures))
    }
}

/// Runtime-value oracle for a conformance case.
///
/// The pinned value is checked by EXECUTING the program with the shipped
/// execution engine — the same [`eval_module_value_with_env_mode`] entry point
/// `mindc test` runs test bodies through — after asserting that the canonical
/// mic@3 artifact emitted for the compiled IR is a byte-exact fixed point under
/// emit -> parse -> emit.
///
/// It replaces the IR *preview* evaluator (`eval::ir_interp::eval_ir`), which
/// models 20 of the 35 `Instr` variants and silently ignores the rest: a case
/// over control flow, a call or an array returned the last *handled* value
/// (`Int(0)` for a `while` program that computes 6), so a conformance cell
/// beyond straight-line arithmetic would have passed on the wrong value.
///
/// deferred: the strongest oracle is executing the emitted NATIVE artifact (or
/// the canonical mic@3 through the same VM the differential fuzzer uses) rather
/// than the interpreter; the artifact round-trip below pins the bytes but not
/// their execution. Upgrade path: run the case through `ExecMode::MlirJitCpu` /
/// the self-host stage1 runner once the conformance job can require that
/// toolchain — owned by the grid-expansion task that turns this suite into the
/// language definition.
pub fn run_value_oracle(source: &str, ir: &IRModule) -> Result<Value, String> {
    // (a) Artifact integrity: the canonical mic@3 bytes for this IR must
    //     survive emit -> parse -> emit unchanged. A codec or lowering
    //     regression that perturbs the artifact fails here instead of being
    //     invisible to a value-only check.
    let bytes = emit_mic3(ir);
    let recovered = parse_mic3(&bytes)
        .map_err(|err| format!("canonical mic@3 artifact does not parse back: {err:?}"))?;
    let reemitted = emit_mic3(&recovered);
    if reemitted != bytes {
        return Err(format!(
            "canonical mic@3 artifact is not a fixed point: {} bytes emitted, \
             {} bytes after emit -> parse -> emit",
            bytes.len(),
            reemitted.len()
        ));
    }

    // (b) Execution: run the program, do not re-read the constant-folded IR.
    let module = crate::parser::parse(source)
        .map_err(|errs| format!("conformance source does not parse: {errs:?}"))?;
    let mut env = std::collections::HashMap::new();
    eval_module_value_with_env_mode(&module, &mut env, None, ExecMode::Preview)
        .map_err(|err| format!("execution failed: {err}"))
}

fn run_case(case: &ConformanceCase) -> Result<(), String> {
    let compile_opts = CompileOptions {
        func: case.func.map(ToOwned::to_owned),
        enable_autodiff: case.run_autodiff,
        target: case.target,
        ..Default::default()
    };

    match compile_source(case.source, &compile_opts) {
        Ok(products) => {
            if let Some(expected) = case.expected_error {
                return Err(format!(
                    "expected failure containing '{expected}' but compilation succeeded"
                ));
            }

            let rendered = normalize(&format!("{}", products.ir));
            if rendered != normalize(case.expected_ir) {
                return Err(format!(
                    "IR mismatch. expected:\n{}\nactual:\n{}",
                    case.expected_ir.trim(),
                    rendered
                ));
            }

            if let Some(value) = &case.expected_value {
                // A case that pins a value for ONE selected function would need
                // the oracle to call that function rather than execute the
                // module; refuse instead of silently checking the wrong thing.
                if let Some(func) = case.func {
                    return Err(format!(
                        "case pins a runtime value but also selects func '{func}'; \
                         the value oracle executes the module, so this case cannot \
                         be checked — split it or drop expected_value"
                    ));
                }
                let evaluated = run_value_oracle(case.source, &products.ir)?;
                match (value, evaluated) {
                    (ExpectedValue::Int(expected), Value::Int(actual)) if *expected == actual => {}
                    (ExpectedValue::Int(expected), got) => {
                        return Err(format!("expected runtime value {expected}, got {got:?}"));
                    }
                }
            }

            #[cfg(feature = "autodiff")]
            if case.run_autodiff {
                let grad = products
                    .grad
                    .as_ref()
                    .ok_or_else(|| "autodiff results missing".to_string())?;

                if let Some(expected) = case.expected_grad_ir {
                    let rendered_grad = normalize(&format!("{}", grad.gradient_module));
                    if rendered_grad != normalize(expected) {
                        return Err(format!(
                            "gradient IR mismatch. expected:\n{}\nactual:\n{}",
                            expected.trim(),
                            rendered_grad
                        ));
                    }
                }
            }

            #[cfg(any(feature = "mlir-lowering", feature = "mlir-build"))]
            if let Some(expected_mlir) = case.expected_mlir {
                #[cfg(feature = "autodiff")]
                let mlir: MlirProducts = lower_to_mlir(&products.ir, products.grad.as_ref())
                    .map_err(|err| format!("MLIR lowering failed: {err}"))?;
                #[cfg(not(feature = "autodiff"))]
                let mlir: MlirProducts = lower_to_mlir(&products.ir)
                    .map_err(|err| format!("MLIR lowering failed: {err}"))?;

                let rendered_mlir = normalize(&mlir.primal_mlir);
                if rendered_mlir != normalize(expected_mlir) {
                    return Err(format!(
                        "MLIR mismatch. expected:\n{}\nactual:\n{}",
                        expected_mlir.trim(),
                        rendered_mlir
                    ));
                }
            }

            Ok(())
        }
        Err(err) => {
            if let Some(expected) = case.expected_error {
                let msg = format!("{err}").to_lowercase();
                if msg.contains(&expected.to_lowercase()) {
                    Ok(())
                } else {
                    Err(format!("expected error containing '{expected}', got {msg}"))
                }
            } else {
                Err(format!("unexpected compile error: {err:?}"))
            }
        }
    }
}

fn normalize(text: &str) -> String {
    text.trim().replace('\r', "")
}

fn cpu_cases() -> Vec<ConformanceCase> {
    #[allow(unused_mut)]
    let mut cases = vec![ConformanceCase {
        name: "simple_arith",
        source: include_str!("../tests/conformance/cpu_baseline/simple_arith.mind"),
        target: BackendTarget::Cpu,
        func: None,
        expected_ir: include_str!("../tests/conformance/cpu_baseline/simple_arith.ir"),
        expected_value: Some(ExpectedValue::Int(7)),
        expected_mlir: Some(include_str!(
            "../tests/conformance/cpu_baseline/simple_arith.mlir"
        )),
        #[cfg(feature = "autodiff")]
        expected_grad_ir: None,
        expected_error: None,
        run_autodiff: false,
    }];

    // The autodiff_pairwise conformance entry was removed 2026-05-20 — its
    // fixture used the obsolete top-level expression syntax `tensor.zeros(f32, ())`
    // which the grammar tightened out in mindc v0.4.x (bare type names are
    // no longer expressions). The autodiff feature is exercised end-to-end
    // by `tests/autodiff.rs` (5 tests) and `tests/autodiff_preview.rs`,
    // both of which use the current `fn`-wrapped syntax. The conformance
    // duplicate was silently broken under `cargo test --features autodiff`
    // until the 2026-05-19 stale-test sweep surfaced it.

    cases
}

fn gpu_cases() -> Vec<ConformanceCase> {
    #[allow(unused_mut)]
    let mut cases = Vec::new();

    #[cfg(feature = "mlir-gpu")]
    {
        cases.push(ConformanceCase {
            name: "backend_unavailable",
            source: include_str!("../tests/conformance/gpu_profile/backend_unavailable.mind"),
            target: BackendTarget::Gpu,
            func: None,
            expected_ir: "",
            expected_value: None,
            expected_mlir: None,
            #[cfg(feature = "autodiff")]
            expected_grad_ir: None,
            expected_error: Some("backend unavailable"),
            run_autodiff: false,
        });
    }

    cases
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A case that pins the WRONG runtime value must be reported as a failure.
    /// Positive control for the value oracle: without it, a green cell proves
    /// only that the case compiled.
    #[test]
    fn wrong_expected_value_is_reported_as_a_failure() {
        let case = ConformanceCase {
            name: "simple_arith_wrong_value",
            source: include_str!("../tests/conformance/cpu_baseline/simple_arith.mind"),
            target: BackendTarget::Cpu,
            func: None,
            expected_ir: include_str!("../tests/conformance/cpu_baseline/simple_arith.ir"),
            expected_value: Some(ExpectedValue::Int(8)),
            expected_mlir: None,
            #[cfg(feature = "autodiff")]
            expected_grad_ir: None,
            expected_error: None,
            run_autodiff: false,
        };
        let err = run_case(&case).expect_err("a wrong pinned value must fail the case");
        assert!(
            err.contains("expected runtime value 8"),
            "failure must name the mismatch, got: {err}"
        );
    }

    /// The oracle must EXECUTE calls. `Instr::FnDef` / `Call` / `Return` are
    /// unmodelled by the IR preview evaluator it replaced, so this program read
    /// as `Int(0)` there.
    #[test]
    fn value_oracle_executes_a_call() {
        let source = "pub fn add3(x: i64) -> i64 { return x + 3 }\nadd3(4)\n";
        let opts = CompileOptions {
            target: BackendTarget::Cpu,
            ..Default::default()
        };
        let products = compile_source(source, &opts).expect("call source compiles");
        let value = run_value_oracle(source, &products.ir).expect("oracle runs the program");
        assert_eq!(
            value,
            Value::Int(7),
            "add3(4) = 7; a skipped call yields Int(0)"
        );
    }

    /// The oracle must EXECUTE control flow. The IR preview evaluator it
    /// replaced returned the last *handled* value — `Int(0)` — for this
    /// program, so a control-flow cell would have passed on the wrong value.
    #[cfg(feature = "std-surface")]
    #[test]
    fn value_oracle_executes_control_flow() {
        let source = "pub fn sum_below(n: i64) -> i64 {\n\
                      let mut s: i64 = 0\n\
                      let mut i: i64 = 0\n\
                      while i < n { s = s + i i = i + 1 }\n\
                      return s\n\
                      }\n\
                      sum_below(4)\n";
        let opts = CompileOptions {
            target: BackendTarget::Cpu,
            ..Default::default()
        };
        let products = compile_source(source, &opts).expect("control-flow source compiles");
        let value = run_value_oracle(source, &products.ir).expect("oracle runs the program");
        assert_eq!(
            value,
            Value::Int(6),
            "0+1+2+3 = 6; a skipped loop yields Int(0)"
        );
    }

    /// A requested profile with an empty case list must fail, never pass.
    #[cfg(not(feature = "mlir-gpu"))]
    #[test]
    fn gpu_profile_with_no_cases_fails_closed() {
        let err = run_conformance(ConformanceOptions {
            profile: ConformanceProfile::CpuAndGpu,
        })
        .expect_err("zero GPU cases must not report a pass");
        assert!(
            err.0.iter().any(|f| f == NO_GPU_CASES),
            "failure list must carry the empty-GPU-case-list policy text, got: {:?}",
            err.0
        );
    }
}
