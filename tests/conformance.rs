use libmind::conformance::{ConformanceOptions, ConformanceProfile, run_conformance};
// Used only by the fail-closed test below, which is itself gated on the GPU
// feature being ABSENT; under --all-features that test compiles out and an
// ungated import would dangle. Gating the import to match its single use keeps
// the non-GPU case intact instead of removing it.
#[cfg(not(feature = "mlir-gpu"))]
use libmind::conformance::NO_GPU_CASES;

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
    // Same rule per LEG: a grid whose value cells never executed attests no
    // runtime value at all. What a green cell means is the engine's own
    // sentence, printed by the CLI — never "the compiled artifact is correct".
    assert!(
        report.value_ran >= 1,
        "CPU profile must execute at least one value cell, value_ran={}",
        report.value_ran
    );
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

// ── Corpus well-formedness, per documented category ───────────────────────
//
// tests/CONFORMANCE_TESTS.md documents `cargo test --test conformance -- lexical`
// (and `-- type_checker`) as the way to run a category. Until now this target
// held two tests, neither named for a category, so BOTH commands matched zero
// tests, printed `0 passed` and exited 0 — the documented entry point to the
// conformance story was a guaranteed vacuous green. The tests below carry the
// documented names so those filters select something, and each asserts a real,
// checkable property of its corpus rather than a slogan.
//
// deferred: these assert the corpus is PRESENT and WELL-FORMED, not that the
// compiler agrees with each `// Expected:` line. The fixtures are written in a
// pre-1.0 surface syntax (`tensor<f32[2, 3]>`, and one file that says outright
// it is a placeholder for an IR-level test), so executing them today would
// assert the compiler rejects its own documentation. Upgrade path: port each
// fixture to the current surface and drive it through the conformance-cell grid
// (plan of record Phase 19.3), at which point these become execution gates.

use std::path::PathBuf;

/// Fixtures in one documented corpus category, with their text.
fn corpus(category: &str) -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(category);
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("conformance category `{category}` unreadable: {e}"))
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("mind"))
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read fixture");
            (p.file_name().unwrap().to_string_lossy().into_owned(), text)
        })
        .collect();
    out.sort();
    out
}

/// Every fixture must declare what it expects, or the corpus cannot ever be
/// turned into an execution gate — and a category directory must not be empty,
/// which is the `ran=0` shape this whole target exists to refuse.
fn assert_category_is_well_formed(category: &str, min: usize) {
    let files = corpus(category);
    assert!(
        files.len() >= min,
        "conformance category `{category}` holds {} fixture(s), expected >= {min}; \
         an empty category makes the documented `-- {category}` filter vacuous",
        files.len()
    );
    let undeclared: Vec<&str> = files
        .iter()
        .filter(|(_, t)| {
            !t.lines()
                .any(|l| l.trim_start().starts_with("// Expected:"))
        })
        .map(|(n, _)| n.as_str())
        .collect();
    assert!(
        undeclared.is_empty(),
        "these `{category}` fixtures declare no `// Expected:` line, so nothing \
         could ever check them: {undeclared:?}"
    );
}

#[test]
fn lexical_corpus_is_present_and_declares_its_expectations() {
    assert_category_is_well_formed("lexical", 3);
}

#[test]
fn type_checker_corpus_is_present_and_declares_its_expectations() {
    assert_category_is_well_formed("type_checker", 2);
}

#[test]
fn shapes_corpus_is_present_and_declares_its_expectations() {
    assert_category_is_well_formed("shapes", 3);
}

#[test]
fn ir_verification_corpus_is_present_and_declares_its_expectations() {
    assert_category_is_well_formed("ir_verification", 2);
}
