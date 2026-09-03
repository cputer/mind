//! Corpus-size floor for the differential determinism fuzzer.
//!
//! Carried out of `tests/mindfuzz_cross_substrate.rs` (already far past the
//! 800-line ceiling) as its own focused module rather than growing that file
//! further. Included with `mod mindfuzz_corpus_floor;` — a `mod.rs` under
//! `tests/` is compiled INTO the integration-test binary that declares it, not
//! as a harness of its own, so the fuzzer and this floor stay one binary.

/// Number of programs generated per run. Fixed so wall-time is bounded and the
/// verdict is deterministic. Override upward locally with `MINDFUZZ_ITERS` for a
/// heavier soak; CI uses this default. It is also the FLOOR: see `resolve_iters`.
pub const DEFAULT_ITERS: usize = 32;

/// Resolve the program count from a raw `MINDFUZZ_ITERS` value, refusing any
/// value that would SHRINK the corpus.
///
/// `mindfuzz_cross_runner_identity` is a required, self-described RANK-1
/// existential gate, and it compared two files for "non-empty and equal" only.
/// The compared digest is a running SHA-256 over the mic@3 bytes of `iters`
/// programs, so `MINDFUZZ_ITERS=0` made both runners write the SHA-256 of
/// nothing: a 64-hex string, non-empty, identical on both substrates, and the
/// job printed `PASS: avx2 == neon` having compared no programs at all. The env
/// knob is documented as an upward-only soak control, so the floor is the
/// default itself — a request to run fewer is a configuration error, not a
/// smaller run.
pub fn resolve_iters(raw: Option<String>) -> usize {
    let Some(s) = raw else {
        return DEFAULT_ITERS;
    };
    let n: usize = s.trim().parse().unwrap_or_else(|e| {
        panic!("MINDFUZZ_ITERS={s:?} is not a program count ({e}) — refusing to guess")
    });
    assert!(
        n >= DEFAULT_ITERS,
        "MINDFUZZ_ITERS={n} would SHRINK the fuzzed corpus below the floor of \
         {DEFAULT_ITERS}. MINDFUZZ_ITERS raises the soak, it never lowers it: at \
         iters=0 both runners write the SHA-256 of nothing and the cross-runner \
         identity gate compares two empty corpora and prints PASS."
    );
    n
}

#[test]
fn mindfuzz_iters_floor_refuses_a_shrunken_corpus() {
    assert_eq!(
        resolve_iters(None),
        DEFAULT_ITERS,
        "unset must use the default"
    );
    assert_eq!(
        resolve_iters(Some("64".into())),
        64,
        "raising the soak must work"
    );
    assert_eq!(resolve_iters(Some(" 32 ".into())), DEFAULT_ITERS);
    // The refusals below panic on purpose, so their messages are printed. The
    // global panic hook is deliberately NOT silenced: this test shares a process
    // with the fuzzer, and muting the hook here swallowed the fuzzer's own panic
    // message when the two ran concurrently — a test that hides another test's
    // diagnostic is worse than a noisy one.
    println!("mindfuzz_corpus_floor: the 4 panics below are EXPECTED refusals");
    let mut checked = 0usize;
    for bad in ["0", "1", "31", "not-a-number"] {
        let r = std::panic::catch_unwind(|| resolve_iters(Some(bad.to_string())));
        assert!(
            r.is_err(),
            "MINDFUZZ_ITERS={bad} must be REFUSED — a corpus that small (or \
             unparseable) makes the cross-runner digest compare vacuous"
        );
        checked += 1;
    }
    assert_eq!(checked, 4, "every refusal case must have been exercised");
}
