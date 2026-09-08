use crate::eval::value::Value;
use crate::eval::{ExecMode, eval_module_value_with_env_mode};
use crate::ir::IRModule;
use crate::ir::compact::v3::{emit_mic3_checked, parse_mic3_body};
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
    /// Number of executed cases that exercised the AUTODIFF leg
    /// (`run_autodiff`), across every case list the profile ran.
    ///
    /// Counted separately because a per-run total cannot distinguish "the
    /// autodiff surface was verified" from "one arithmetic case ran and the
    /// autodiff leg was never entered" — and `docs/versioning.md` sells a
    /// passing profile as evidence of autodiff stability. Zero while
    /// [`AUTODIFF_COMPILED_IN`] is true is a failure, not a pass.
    pub autodiff_ran: usize,
    /// Number of executed cases whose VALUE cell actually ran through
    /// [`run_value_oracle`].
    ///
    /// Counted where the check executes, not where it is declared: a case that
    /// pins a value and also pins a compile error never reaches the oracle, so
    /// a declaration-side count would attest a runtime value nobody computed.
    /// Zero is a failure (see [`NO_VALUE_CASES`]).
    ///
    /// What a green value cell attests is [`VALUE_ORACLE_ENGINE`]'s
    /// [`ValueOracleEngine::attests`] — read it before quoting this count.
    pub value_ran: usize,
}

impl ConformanceReport {
    /// The lines a passing run prints, in order.
    ///
    /// The suite owns what its own pass ATTESTS, so the wording lives beside the
    /// facts it quotes ([`AUTODIFF_COMPILED_IN`], [`VALUE_ORACLE_ENGINE`],
    /// [`ValueOracleEngine::attests`]) rather than in the CLI. A print site that
    /// only has the report cannot then paraphrase it, and flipping the value
    /// oracle or building without `autodiff` changes the attestation in exactly
    /// one place.
    ///
    /// Reporting the COUNT the suite executed, not just the exit code, is the
    /// point: a profile that ran zero cases verified nothing, and
    /// [`run_conformance`] fails closed on exactly that — printing the count is
    /// what keeps the attestation checkable by whoever reads the CI log.
    ///
    /// The autodiff and value legs are reported SEPARATELY, each naming what
    /// produced it. A build without the `autodiff` feature says so instead of
    /// printing a `0` a reader could take for "checked, nothing wrong"
    /// (`docs/versioning.md` sells a passing profile as evidence of autodiff
    /// stability, so the line must state whether that leg ran at all), and the
    /// value leg names its engine because a green value cell attests THAT
    /// engine's result — a log that omits it invites the reader to take it for
    /// the compiled artifact's execution.
    pub fn attestation_lines(&self) -> Vec<String> {
        let autodiff = if AUTODIFF_COMPILED_IN {
            format!("autodiff={}", self.autodiff_ran)
        } else {
            "autodiff=n/a: built without the `autodiff` feature, so this \
             run attests nothing about autodiff"
                .to_string()
        };
        let engine = VALUE_ORACLE_ENGINE;
        vec![
            format!(
                "Core v1 conformance passed for profile: {:?} — ran={} (cpu={}, gpu={}, value={} via {}, {autodiff})",
                self.profile,
                self.total_ran(),
                self.cpu_ran,
                self.gpu_ran,
                self.value_ran,
                engine.tag()
            ),
            format!("value oracle: {}", engine.attests()),
        ]
    }

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

/// Failure text for a build that compiled the autodiff leg in but ran no case
/// through it.
pub const NO_AUTODIFF_CASES: &str = "autodiff: no conformance case exercises the \
                                     autodiff leg (0 cases ran); this build \
                                     compiled the `autodiff` feature in, so a \
                                     pass here would attest an autodiff \
                                     stability the suite never checked — \
                                     treating as failure";

/// Failure text for a run whose value leg executed zero cells.
pub const NO_VALUE_CASES: &str = "value: no conformance case executed the value \
                                  oracle (0 cells ran); a pass here would attest \
                                  runtime values the suite never computed — \
                                  treating as failure";

/// The engine whose result a conformance VALUE cell attests.
///
/// One owner for the fact, so the suite, the CLI attestation line and any
/// downstream claim read the same sentence instead of inferring "the compiler
/// is correct" from a green cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueOracleEngine {
    /// The shipped AST evaluator (`eval_module_value_with_env_mode`), executing
    /// the case SOURCE. The compiled IR is checked for canonical-artifact
    /// integrity only (mic@3 emit -> parse -> emit fixed point).
    AstEvaluator,
    /// The compiled artifact's own execution. Not reachable yet — the typed
    /// name of the upgrade documented on [`run_value_oracle`], so that landing
    /// it changes the attestation text in exactly one place.
    CompiledArtifact,
}

impl ValueOracleEngine {
    /// Short machine-greppable tag for the CI log line.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::AstEvaluator => "ast-evaluator",
            Self::CompiledArtifact => "compiled-artifact",
        }
    }

    /// One line stating exactly what a green value cell does and does not prove.
    pub const fn attests(self) -> &'static str {
        match self {
            Self::AstEvaluator => {
                "a green value cell attests the AST evaluator's result and the \
                 canonical mic@3 artifact's emit -> parse -> emit fixed point, \
                 NOT the compiled artifact's execution"
            }
            Self::CompiledArtifact => {
                "a green value cell attests the compiled artifact's own execution"
            }
        }
    }
}

/// The engine this build's value cells actually run on.
pub const VALUE_ORACLE_ENGINE: ValueOracleEngine = ValueOracleEngine::AstEvaluator;

/// Whether the `autodiff` feature was compiled into this binary.
///
/// One owner for the fact: the suite, the CLI attestation line and the tests
/// all read this constant instead of each spelling their own `cfg!` check.
pub const AUTODIFF_COMPILED_IN: bool = cfg!(feature = "autodiff");

/// Run the Core v1 conformance suite for one profile.
///
/// Fails closed on an empty case list: a profile that executes zero cases
/// verified nothing, and printing a pass for it is a false attestation (the
/// docs sell a passing profile as evidence of API/IR/MLIR stability). This
/// mirrors the zero-test policy `mindc test` already applies to a discovery
/// that finds nothing to run.
pub fn run_conformance(opts: ConformanceOptions) -> Result<ConformanceReport, ConformanceFailure> {
    let mut failures = Vec::new();
    let mut counts = LegCounts {
        cpu_ran: 0,
        gpu_requested: matches!(opts.profile, ConformanceProfile::CpuAndGpu),
        gpu_ran: 0,
        value_ran: 0,
        autodiff_ran: 0,
    };

    let cpu = cpu_cases();
    counts.cpu_ran = cpu.len();
    for case in &cpu {
        match run_case(case) {
            Ok(outcome) => counts.record(&outcome),
            Err(msg) => failures.push(format!("cpu:{} => {msg}", case.name)),
        }
    }

    if counts.gpu_requested {
        let gpu = gpu_cases();
        counts.gpu_ran = gpu.len();
        for case in &gpu {
            match run_case(case) {
                Ok(outcome) => counts.record(&outcome),
                Err(msg) => failures.push(format!("gpu:{} => {msg}", case.name)),
            }
        }
    }

    failures.extend(leg_failures(&counts));

    if failures.is_empty() {
        Ok(ConformanceReport {
            profile: opts.profile,
            cpu_ran: counts.cpu_ran,
            gpu_ran: counts.gpu_ran,
            autodiff_ran: counts.autodiff_ran,
            value_ran: counts.value_ran,
        })
    } else {
        Err(ConformanceFailure(failures))
    }
}

/// Per-leg execution counts for one run — the input to the fail-closed policy.
///
/// One owner for the "did this leg run anything" rule: empty-CPU, empty-GPU,
/// no-value-cell and no-autodiff-case are four spellings of the same invariant
/// (an exit code answers "did anything that ran fail", never "did anything
/// run"), so they are decided once in [`leg_failures`] and unit-tested there
/// instead of being re-derived inline at each leg.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LegCounts {
    cpu_ran: usize,
    gpu_requested: bool,
    gpu_ran: usize,
    value_ran: usize,
    autodiff_ran: usize,
}

impl LegCounts {
    fn record(&mut self, outcome: &CaseOutcome) {
        if outcome.value_ran {
            self.value_ran += 1;
        }
        if outcome.autodiff_ran {
            self.autodiff_ran += 1;
        }
    }
}

/// Which optional legs a single case actually EXECUTED.
///
/// Recorded where the check runs, not where it is declared: a case can carry
/// `expected_value` and still never reach the oracle (it also pins a compile
/// error), and counting the declaration would attest a value nobody computed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CaseOutcome {
    value_ran: bool,
    autodiff_ran: bool,
}

/// The fail-closed policy over one run's per-leg counts.
fn leg_failures(counts: &LegCounts) -> Vec<String> {
    let mut failures = Vec::new();
    if counts.cpu_ran == 0 {
        failures.push(NO_CPU_CASES.to_string());
    }
    if counts.gpu_requested && counts.gpu_ran == 0 {
        failures.push(NO_GPU_CASES.to_string());
    }
    if counts.value_ran == 0 {
        failures.push(NO_VALUE_CASES.to_string());
    }
    if AUTODIFF_COMPILED_IN && counts.autodiff_ran == 0 {
        failures.push(NO_AUTODIFF_CASES.to_string());
    }
    failures
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
/// SCOPE OF A GREEN VALUE CELL — do not over-read it: it attests the AST
/// evaluator's result and the canonical mic@3 artifact's emit -> parse -> emit
/// fixed point, NOT the execution of the compiled artifact; a lowering or
/// codegen miscompile leaves the artifact a valid fixed point and the
/// evaluator's answer correct, so the cell stays green. The single spelling of
/// that sentence is [`ValueOracleEngine::attests`] for [`VALUE_ORACLE_ENGINE`],
/// which the CLI prints on every run so a CI log cannot be quoted for more than
/// it checked.
///
/// deferred: execute the COMPILED artifact instead of the source.
/// - Owner: docs/roadmap.md "Phase 19.3 — Exhaustive-cell conformance over the
///   codegen flag product" (the task that turns this suite into the language
///   definition); this oracle is its per-cell value leg.
/// - Route (pick one, in preference order): (1) `ExecMode::MlirJitCpu` via
///   `eval_module_value_with_env_mode`, which already exists behind the
///   `mlir-jit` feature; (2) the differential fuzzer's mic@3 VM over the
///   canonical bytes emitted above (`tests/mindfuzz_cross_substrate.rs`), which
///   needs no native toolchain; (3) the self-host stage1 runner, once it
///   accepts an arbitrary conformance case.
/// - CI prerequisite: the `conformance` job in `.github/workflows/ci.yml` today
///   builds `cargo build --release --bin mindc` with default features and
///   installs no LLVM/MLIR toolchain, so route (1) is blocked until that job
///   pins LLVM 20 (`mlir-20-tools clang-20`, PATH `/usr/lib/llvm-20/bin`, as the
///   cross-substrate job does) and builds with `--features mlir-jit`. Route (2)
///   has no such prerequisite and is the cheapest first step.
/// - On landing: flip [`VALUE_ORACLE_ENGINE`] to
///   [`ValueOracleEngine::CompiledArtifact`]; the attestation text follows from
///   that one constant.
pub fn run_value_oracle(source: &str, ir: &IRModule) -> Result<Value, String> {
    // (a) Artifact integrity: the canonical mic@3 bytes for this IR must
    //     survive emit -> parse -> emit unchanged. A codec or lowering
    //     regression that perturbs the artifact fails here instead of being
    //     invisible to a value-only check.
    let bytes = emit_mic3_checked(ir)
        .map_err(|err| format!("canonical mic@3 artifact emission failed: {err}"))?;
    let recovered = parse_mic3_body(&bytes)
        .map_err(|err| format!("canonical mic@3 artifact does not parse back: {err:?}"))?;
    let reemitted = emit_mic3_checked(&recovered)
        .map_err(|err| format!("canonical mic@3 artifact re-emission failed: {err}"))?;
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

fn run_case(case: &ConformanceCase) -> Result<CaseOutcome, String> {
    let mut outcome = CaseOutcome::default();
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
                // Recorded here, where the oracle actually ran.
                outcome.value_ran = true;
            }

            #[cfg(feature = "autodiff")]
            if case.run_autodiff {
                outcome.autodiff_ran = true;
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

            Ok(outcome)
        }
        Err(err) => {
            if let Some(expected) = case.expected_error {
                let msg = format!("{err}").to_lowercase();
                if msg.contains(&expected.to_lowercase()) {
                    // Nothing optional executed: the case pinned a compile error.
                    Ok(CaseOutcome::default())
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

    // The AUTODIFF leg of the profile. `enable_autodiff` requires a function
    // to differentiate and `differentiate_function` differentiates the
    // module's top level, so the case selects `main` and pins the gradient
    // module derived from the canonical primal.
    //
    // Gated on the feature that compiles the differentiator in: without it
    // `compile_source` refuses the request (`CompileError::AutodiffDisabled`),
    // so the cell would pin the refusal, not a gradient. A build without the
    // feature runs zero autodiff cases and the `mindc conformance` attestation
    // line says so rather than implying coverage it does not have.
    //
    // deferred: this cell differentiates a CONSTANT program — it pins that the
    // differentiator runs end to end from source and that its gradient module
    // is byte-stable, NOT that a tensor derivative is correct. No source-level
    // differentiable TENSOR program can be a cell today: a top-level
    // `let x: diff tensor<f32[3]> = [1.0, 2.0, 3.0]` fails type-check with
    // E2001 ("annotation Tensor[f32, (3)] vs inferred Scalar[f64]"), and a
    // fn-wrapped one lowers to `module { %0 = const.i64 0  output %0 }`, whose
    // gradient is the seed alone. Derivative CORRECTNESS is pinned meanwhile
    // by the autodiff API tests (`tests/autodiff.rs`: grad_of_square,
    // grad_of_relu, grad_of_conv2d, matmul_rule_applied, ...). Upgrade path:
    // add tensor-gradient cells here once the source pipeline lowers a
    // differentiable tensor program — owned by the conformance
    // grid-expansion task, docs/roadmap.md "Phase 19.3 — Exhaustive-cell
    // conformance over the codegen flag product". docs/versioning.md carries
    // the same pointer so no reader takes the corpus for more than it is.
    #[cfg(feature = "autodiff")]
    cases.push(ConformanceCase {
        name: "autodiff_seed",
        source: include_str!("../tests/conformance/cpu_baseline/autodiff_seed.mind"),
        target: BackendTarget::Cpu,
        func: Some("main"),
        expected_ir: include_str!("../tests/conformance/cpu_baseline/autodiff_seed.ir"),
        // The value oracle executes the whole module and this case selects a
        // function, so it pins IR (primal + gradient), not a runtime value.
        expected_value: None,
        expected_mlir: None,
        expected_grad_ir: Some(include_str!(
            "../tests/conformance/cpu_baseline/autodiff_seed.grad.ir"
        )),
        expected_error: None,
        run_autodiff: true,
    });

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

    fn sample_report() -> ConformanceReport {
        ConformanceReport {
            profile: ConformanceProfile::CpuBaseline,
            cpu_ran: 7,
            gpu_ran: 0,
            autodiff_ran: 2,
            value_ran: 5,
        }
    }

    /// The attestation is the ONLY thing a CI reader takes away from a green
    /// run, so its shape is pinned here rather than left to whatever the CLI
    /// happened to format. Every count the suite fails closed on must appear.
    #[test]
    fn the_attestation_reports_every_leg_and_its_counts() {
        let lines = sample_report().attestation_lines();
        assert_eq!(lines.len(), 2, "one summary line, one value-oracle line");
        let summary = &lines[0];
        assert!(summary.starts_with("Core v1 conformance passed for profile: CpuBaseline"));
        for cell in ["ran=7", "cpu=7", "gpu=0", "value=5"] {
            assert!(summary.contains(cell), "{summary} omits {cell}");
        }
        assert!(
            summary.contains(VALUE_ORACLE_ENGINE.tag()),
            "the value leg must name the engine that produced it: {summary}"
        );
        assert_eq!(
            lines[1],
            format!("value oracle: {}", VALUE_ORACLE_ENGINE.attests())
        );
    }

    /// A build without the `autodiff` feature must SAY so. Printing `autodiff=0`
    /// there reads as "checked, nothing wrong" for a leg that never ran, and
    /// `docs/versioning.md` sells a passing profile as evidence of autodiff
    /// stability.
    #[test]
    fn the_autodiff_leg_states_whether_it_ran_at_all() {
        let summary = sample_report().attestation_lines()[0].clone();
        if AUTODIFF_COMPILED_IN {
            assert!(summary.contains("autodiff=2"), "{summary}");
        } else {
            assert!(summary.contains("autodiff=n/a"), "{summary}");
            assert!(
                summary.contains("attests nothing about autodiff"),
                "a build without the feature must not leave the reader to infer \
                 what the absence means: {summary}"
            );
        }
    }

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

    /// The suite must actually ENTER the autodiff leg on a build that compiled
    /// it in — `docs/versioning.md` sells a passing profile as evidence of
    /// autodiff stability, and a corpus whose every case leaves `run_autodiff`
    /// false attests that with nothing. Pins the count per leg, the way
    /// `cpu_ran` pins the total.
    #[cfg(feature = "autodiff")]
    #[test]
    fn autodiff_leg_runs_at_least_one_case() {
        let report = run_conformance(ConformanceOptions {
            profile: ConformanceProfile::CpuBaseline,
        })
        .expect("cpu baseline profile passes");
        assert!(
            report.autodiff_ran >= 1,
            "no conformance case exercised the autodiff leg (autodiff_ran={}); \
             a pass here would attest autodiff stability the suite never checked",
            report.autodiff_ran
        );
    }

    /// The VALUE leg must actually execute on a passing profile. The suite's
    /// exit code cannot tell "every value cell matched" from "no case pinned a
    /// value, so the oracle never ran" — and the second reads as a pass while
    /// attesting nothing about runtime values.
    #[test]
    fn value_leg_runs_at_least_one_case() {
        let report = run_conformance(ConformanceOptions {
            profile: ConformanceProfile::CpuBaseline,
        })
        .expect("cpu baseline profile passes");
        assert!(
            report.value_ran >= 1,
            "no conformance case executed the value oracle (value_ran={}); \
             a pass here would attest runtime values the suite never checked",
            report.value_ran
        );
    }

    /// A run whose value leg executed zero cells must fail closed, exactly as
    /// an empty case list does.
    #[test]
    fn zero_value_cells_fails_closed() {
        let failures = leg_failures(&LegCounts {
            cpu_ran: 1,
            gpu_requested: false,
            gpu_ran: 0,
            value_ran: 0,
            autodiff_ran: 1,
        });
        assert!(
            failures.iter().any(|f| f == NO_VALUE_CASES),
            "zero executed value cells must be reported as a failure, got: {failures:?}"
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
