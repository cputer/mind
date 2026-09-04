use libmind::conformance::{ConformanceOptions, ConformanceProfile, NO_GPU_CASES, run_conformance};

#[test]
fn cpu_conformance_profile_passes() {
    let report = run_conformance(ConformanceOptions {
        profile: ConformanceProfile::CpuBaseline,
    })
    .expect("CPU conformance suite should pass");
    // Exit status alone cannot distinguish "everything passed" from "nothing
    // ran": pin the count so an empty grid fails this gate.
    assert!(
        report.cpu_ran >= 1,
        "CPU profile must execute at least one case, ran={}",
        report.cpu_ran
    );
    assert_eq!(report.gpu_ran, 0, "CPU profile must not run GPU cases");
}

/// The GPU profile is built without the `mlir-gpu` feature in every default and
/// CI feature set, so its case list is empty. An empty list must FAIL — the
/// previous behaviour printed "conformance passed for profile: CpuAndGpu" and
/// exited 0 while verifying nothing about a GPU.
#[cfg(not(feature = "mlir-gpu"))]
#[test]
fn gpu_profile_fails_closed_when_no_gpu_cases_are_compiled_in() {
    let err = run_conformance(ConformanceOptions {
        profile: ConformanceProfile::CpuAndGpu,
    })
    .expect_err("GPU profile with zero compiled-in cases must not report a pass");
    assert!(
        err.0.iter().any(|f| f == NO_GPU_CASES),
        "failure must name the empty GPU case list, got: {:?}",
        err.0
    );
}

#[cfg(feature = "mlir-gpu")]
#[test]
fn gpu_profile_runs_when_enabled() {
    let report = run_conformance(ConformanceOptions {
        profile: ConformanceProfile::CpuAndGpu,
    })
    .expect("GPU conformance profile should be executed");
    assert!(
        report.gpu_ran >= 1,
        "GPU profile must execute at least one case, ran={}",
        report.gpu_ran
    );
}
