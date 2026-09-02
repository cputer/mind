//! Shared self-skip probe for criterion bench targets.
//!
//! Why this exists
//! ---------------
//! The CI-equivalent sweep
//!
//! ```text
//! cargo bench --no-default-features -- --output-format bencher --measurement-time 8
//! ```
//!
//! could not complete. Bench fixtures are fixed inputs, but whether the compiler
//! accepts one depends on the feature set the bench was built with, and several
//! targets turned a refusal into `.expect("compilation failed")`. A panic in one
//! bench target aborts `cargo bench` with exit 101, so a single feature-gated
//! fixture left EVERY later target unmeasured — the sweep reported nothing at all
//! rather than reporting what it could measure.
//!
//! A refusal is information, not a crash. `accepts` probes a fixture once before
//! timing it, and on refusal prints a line naming the target, the fixture and the
//! reason, then returns `false` so the caller skips just that fixture.
//!
//! Two shapes of refusal are handled, because this compiler uses both:
//!   * a returned `Err` — the type checker rejecting a call it has no rule for;
//!   * a PANIC — the deliberate fail-closed convention for "no lowering exists
//!     for this construct, refusing to emit a placeholder that would be a silent
//!     miscompile". That one cannot be observed with a `match`, so it is unwound.
//!
//! This cannot mask a slow or wrong measurement: an accepted fixture is timed
//! exactly as before, and a skipped one is announced on stdout with its reason.
#![allow(dead_code)]

use std::fmt::Debug;
use std::panic::{self, UnwindSafe};

/// `true` if this build accepts `probe`, else `false` plus a printed reason.
pub fn accepts<T, E: Debug>(
    target: &str,
    fixture: &str,
    probe: impl FnOnce() -> Result<T, E> + UnwindSafe,
) -> bool {
    // The default hook is silenced only for the duration of the probe, so the
    // skip line below is the report rather than a stray backtrace; it is
    // restored immediately afterwards.
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let outcome = panic::catch_unwind(probe);
    panic::set_hook(prev);

    let reason = match outcome {
        Ok(Ok(_)) => return true,
        Ok(Err(e)) => format!("{e:?}"),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("<non-string panic payload>");
            format!("refused: {msg}")
        }
    };
    println!("{target}: {fixture} not usable in this feature build ({reason}); skipping");
    false
}
