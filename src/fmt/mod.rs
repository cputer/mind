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

//! `mindc fmt` — Phase 2A canonical formatter.
//!
//! Entry point: [`format_source`].  Parses the source through the trivia-aware
//! front-end, then walks the AST with `printer::print_module` to emit a
//! canonicalised string.
//!
//! Phase 2A rules (no soft line-wrap):
//! - Indent `cfg.indent_width` spaces per nesting; never tabs.
//! - Single blank line between top-level items; no leading/trailing blanks.
//! - Whitespace normalisation inside expressions.
//! - Comment re-attachment from the trivia stream.
//! - String-literal contents passed through bytewise.

pub mod cli;
mod printer;

use crate::parser::{ParseError, parse_with_trivia};
use crate::project::MindcraftFormatConfig;

/// Error type for formatting operations.
#[derive(Debug, Clone)]
pub enum FmtError {
    /// The source could not be parsed.
    ParseError(Vec<ParseError>),
}

impl std::fmt::Display for FmtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FmtError::ParseError(errs) => {
                write!(f, "parse error(s):")?;
                for e in errs {
                    write!(f, " {e}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for FmtError {}

/// Format a MIND source string according to `cfg`, returning the canonical
/// representation.
///
/// The output is idempotent: `format_source(format_source(s, c), c) ==
/// format_source(s, c)`.
///
/// # Errors
///
/// Returns [`FmtError::ParseError`] when `src` cannot be parsed.
pub fn format_source(src: &str, cfg: &MindcraftFormatConfig) -> Result<String, FmtError> {
    let (module, trivia) = parse_with_trivia(src).map_err(FmtError::ParseError)?;
    Ok(printer::print_module(&module, &trivia, cfg, src))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_source_remains_idempotent() {
        let cfg = MindcraftFormatConfig::default();
        let source = "fn add(a: i64, b: i64) -> i64 { return a + b; }\n";
        let once = format_source(source, &cfg).expect("valid source formats");
        let twice = format_source(&once, &cfg).expect("formatted source re-formats");
        assert_eq!(once, twice);
    }

    #[test]
    fn macro_shaped_source_is_rejected_before_reconstruction() {
        let cfg = MindcraftFormatConfig::default();
        let source = "fn broken() { let content = format!(\"{}\", 1); }\n";
        let error = format_source(source, &cfg).expect_err("unsupported macro must fail closed");
        assert!(
            error.to_string().contains("macro invocation `format!`"),
            "diagnostic must name the offending macro, got: {error}"
        );
    }

    #[test]
    fn macro_text_in_string_and_comment_is_ignored() {
        let cfg = MindcraftFormatConfig::default();
        let source = "fn text() { let s = \"format!(x)\"; } // format!(x)\n";
        assert!(format_source(source, &cfg).is_ok());
    }

    /// Assert every case in `cases` formats, reporting ALL offenders rather than
    /// aborting on the first — a one-case panic hides how wide a regression is.
    fn expect_all_accepted(label: &str, cases: &[&str]) {
        let cfg = MindcraftFormatConfig::default();
        let rejected: Vec<String> = cases
            .iter()
            .filter_map(|src| {
                format_source(src, &cfg)
                    .err()
                    .map(|e| format!("{src:?}: {e}"))
            })
            .collect();
        assert!(
            rejected.is_empty(),
            "{label}: {}/{} valid sources rejected:\n  {}",
            rejected.len(),
            cases.len(),
            rejected.join("\n  ")
        );
    }

    /// Assert every case in `cases` is refused, reporting ALL that slipped through.
    fn expect_all_rejected(label: &str, cases: &[&str]) {
        let cfg = MindcraftFormatConfig::default();
        let accepted: Vec<String> = cases
            .iter()
            .filter(|src| format_source(src, &cfg).is_ok())
            .map(|src| format!("{src:?}"))
            .collect();
        assert!(
            accepted.is_empty(),
            "{label}: {}/{} unsupported sources accepted:\n  {}",
            accepted.len(),
            cases.len(),
            accepted.join("\n  ")
        );
    }

    /// A keyword followed by a parenthesised unary `!` is ordinary MIND: `!`
    /// binds at atom precedence, so negating a compound expression REQUIRES the
    /// `!( … )` form. `return`/`if` are keywords, not callees, and the formatter
    /// must accept every one of these in EVERY feature profile — they are part
    /// of the frozen low-level grammar that `--no-default-features` builds.
    #[test]
    fn parenthesised_unary_not_after_keywords_formats() {
        expect_all_accepted(
            "unary not after keyword",
            &[
                "fn f(a: bool, b: bool) -> bool { return !(a); }\n",
                "fn f(a: bool, b: bool) -> bool { return !(a && b); }\n",
                "fn f(a: bool, b: bool) -> i64 { if !(a && b) { return 1; } return 0; }\n",
                "fn f(a: bool, b: bool) -> i64 { if !(a) { return 1; } else { return 0; } }\n",
            ],
        );
    }

    /// `while` is a `std-surface` statement (`parse_while` is cfg-gated); the
    /// bare grammar reads `while` as a plain identifier. The fixture is shared
    /// by the std-surface acceptance test and the bare rejection pin below so
    /// the two profiles are provably testing the same source.
    const WHILE_UNARY_NOT_FIXTURE: &str =
        "fn f(a: bool, b: bool) -> i64 { while !(a && b) { return 1; } return 0; }\n";

    /// The `while` leg of the keyword rule, run only where the compiler
    /// actually parses `while`. Under `std-surface` the adjacency guard must
    /// not mistake `while !(…)` for a macro any more than `return !(…)`.
    #[cfg(feature = "std-surface")]
    #[test]
    fn parenthesised_unary_not_after_while_formats() {
        expect_all_accepted(
            "unary not after while",
            &[
                WHILE_UNARY_NOT_FIXTURE,
                "fn f(a: bool) -> i64 { while !(a) { return 1; } return 0; }\n",
            ],
        );
    }

    /// Bare-build boundary pin: a `std-surface` program must not be silently
    /// reconstructed by a formatter built without that feature. This fixture
    /// fails to parse bare (`while` is an identifier there, and the shape does
    /// not survive), so the formatter fails closed rather than rewriting it. If
    /// `while` is ever admitted to the bare grammar, this test goes red on
    /// purpose: move the fixture to the accepted set above, do not delete it.
    #[cfg(not(feature = "std-surface"))]
    #[test]
    fn while_fixture_is_rejected_without_std_surface() {
        expect_all_rejected(
            "std-surface while without std-surface",
            &[WHILE_UNARY_NOT_FIXTURE],
        );
    }

    /// The same shape in non-keyword operand position — after `=`, after a
    /// binary operator, and inside a call argument list.
    #[test]
    fn parenthesised_unary_not_in_operand_position_formats() {
        expect_all_accepted(
            "unary not in operand position",
            &[
                "fn f(a: bool, b: bool) -> bool { let c = !(a && b); return c; }\n",
                "fn f(a: bool, b: bool) -> bool { return a || !(b); }\n",
                "fn g(x: bool) -> bool { return x; }\nfn f(a: bool) -> bool { return g(!(a)); }\n",
            ],
        );
    }

    /// `!=` must never be read as a macro bang, spaced or unspaced.
    #[test]
    fn not_equal_operator_is_not_a_macro() {
        expect_all_accepted(
            "not-equal operator",
            &[
                "fn f(a: i64, b: i64) -> bool { return a != b; }\n",
                "fn f(a: i64, b: i64) -> bool { return a!=b; }\n",
                "fn f(a: i64, b: i64) -> bool { return (a) != (b); }\n",
            ],
        );
    }

    /// Every macro delimiter, not just `(`. `vec![1, 2]` recovered into
    /// `let c = vec;` + `![1, 2]`, the same silent restructuring as the paren
    /// form, so both fail closed — as does a dotted callee.
    #[test]
    fn every_macro_delimiter_is_rejected() {
        expect_all_rejected(
            "macro delimiters",
            &[
                "fn broken() { let c = format!(\"{}\", 1); }\n",
                "fn broken() { let c = vec![1, 2]; }\n",
                "fn broken() { let c = format!{\"{}\", 1}; }\n",
                "fn broken() { let c = std.fmt.format!(\"{}\", 1); }\n",
            ],
        );
    }

    /// `assert` and `print` are STATEMENT KEYWORDS, so `assert!{ 1 == 1 }` is
    /// `assert` applied to the unary-not of a block — grammar, not a macro. The
    /// adjacency rule must not touch them, exactly as it must not touch
    /// `return !(…)`. A source that meant Rust's `assert!` therefore still
    /// formats; catching that needs the separator change noted in
    /// `separated_bang_is_a_statement_not_a_macro`, not a formatter guard.
    #[test]
    fn statement_keyword_followed_by_bang_is_not_a_macro() {
        expect_all_accepted(
            "statement keyword before bang",
            &[
                "fn f(a: bool) { assert!(a); }\n",
                "fn f(a: bool) { assert!{ a }; }\n",
            ],
        );
    }

    /// The rule is ADJACENCY: `name!` is a macro, `name !expr` is two statements.
    ///
    /// MIND statements need no separator (`let a = 1 let b = 2` parses), so a
    /// bang divided from the preceding name by whitespace or a comment really is
    /// a fresh unary-not statement, and printing it as one is faithful to the
    /// grammar rather than a formatter rewrite. That permissiveness is a real
    /// wart — it is what lets a mistyped macro survive at all — but narrowing it
    /// is a grammar change affecting every `.mind` source, not a formatter fix.
    /// This test pins the boundary so the distinction stays visible.
    #[test]
    fn separated_bang_is_a_statement_not_a_macro() {
        expect_all_accepted(
            "bang separated from name",
            &[
                "fn f(a: bool) -> bool { let c = a; a !(a); return c; }\n",
                "fn f(a: bool) -> bool { let c = a; a // note\n !(a); return c; }\n",
            ],
        );
    }
}
