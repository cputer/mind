// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! ONE owner for the question "is this a fail-OPEN skip-and-return?".
//!
//! # Why this module exists, and why it reads CODE instead of TEXT
//!
//! The prohibition in `tests/fail_open_skip_site_ratchet.rs` used to be decided
//! by a LINE WINDOW: a capability probe had to sit on one line, and the bare
//! early-out had to fall within the next two. That pins a TEXT SHAPE, not a
//! behaviour, and five ORDINARY Rust spellings of the identical fail-open skip
//! walked straight past it while the gate printed `12 passed`. Measured by
//! appending each to `tests/if_expr.rs` and re-running the built scanner:
//!
//! ```text
//! if !Path::new(p).exists() { println!("a"); println!("b");
//!                             println!("c"); return; }   // return 4 lines down
//! let Ok(_t) = which::which("mlir-opt") else { return; }; // let-else
//! match std::env::var_os(V) { None => return, Some(_) => {} } // match arm
//! let ok = Path::new(p).exists(); if !ok { return; }      // probe bound first
//! let ok = out.status.success(); if !ok { return; }       // ditto, exit status
//! ```
//!
//! A prohibition that can be restated in five ways and evaded in all five is a
//! re-introduction ratchet, not a proof. So the decision moved from "do these
//! two lines look like X" to the structure the compiler itself sees:
//!
//!   a control-flow head (`if` / `match` / `let ... else`) whose SCRUTINEE is
//!   derived from a capability probe, whose BLOCK exits early without handing
//!   the caller a verdict, and which never names `common::gate`.
//!
//! Polarity is deliberately NOT part of the rule. `if !ok`, `if x.is_err()`,
//! `let Ok(_) = .. else` and a `None =>` match arm are one decision written
//! four ways, and every draft that demanded a literal `if !` was evaded by
//! picking a different one.
//!
//! # The masker is load-bearing, not a nicety
//!
//! Brace balancing over raw text is meaningless in this tree: test sources
//! embed MIND programs, JSON and MLIR in string literals, all full of braces.
//! [`mask_code`] blanks the CONTENTS of every string, byte-string, raw-string
//! and char literal and drops every comment, keeping line and column numbers
//! intact. Two consequences, both wanted:
//!
//!   * braces balance against the code the compiler sees;
//!   * a file that merely QUOTES the bad shape inside a string, or describes it
//!     in a comment, is not the bad shape — so this module is subject to the
//!     prohibition it defines rather than exempted from it, and a comment can
//!     no longer supply a `gate::` marker for an unrouted site.

use std::collections::HashSet;

/// Text that proves a skip decision consulted the shared fail-closed helper.
///
/// ONLY `gate::`-qualified call syntax. The bare names `MIND_BENCH_REQUIRE` and
/// `enforce_real_backend` used to appear here, which made the prohibition
/// satisfiable by PROSE: a comment that merely mentions the variable is not
/// evidence that any decision consulted it, and a site can name the variable in
/// the very sentence that says it ignores it.
pub const ROUTED_MARKERS: &[&str] = &[
    "gate::compiled",
    "gate::skipped",
    "gate::classify",
    "gate::is_capability_gap",
];

/// The calls whose answer is ABOUT AVAILABILITY: does the prerequisite exist,
/// is it readable, did the child process run.
///
/// Matched by the CALL, never by the name of a receiver or a variable — a
/// needle spelled with one identifier is evaded by picking another, which is
/// how eight superstring capability tests once stayed hidden. `.is_err()` /
/// `.is_none()` are absent on purpose: they are how a probe is TESTED, not what
/// makes it a probe, and alone they match ordinary `Result` handling.
///
/// This was an ALLOW-LIST OF FOUR (`.exists(`, `which(`, `var_os(`, `.success(`)
/// while the header claimed a residual of three named shapes. Measured one probe
/// per fresh test file against the built scanner, every ordinary Rust spelling
/// of the identical question walked past it: `.is_file()`, `.is_dir()`,
/// `metadata()`, `try_exists()`, the non-`_os` `var()`, `.output()` and
/// `.status()`. A vocabulary narrower than the claim reads as coverage of
/// exactly the spellings it cannot see — the defect this gate exists to forbid,
/// committed by the gate itself. Every entry here is held up by a positive
/// control in [`controls`], and [`controls::the_vocabulary_and_its_controls_agree`]
/// makes that a BIJECTION rather than a habit.
const AVAILABILITY_PROBES: &[&str] = &[
    ".exists(",
    ".try_exists(",
    ".is_file(",
    ".is_dir(",
    "metadata(",
    "which(",
    "var_os(",
    "var(",
    ".success(",
    ".output(",
    ".status(",
];

/// The calls that READ A TRACKED INPUT, whose `Ok` carries the PAYLOAD rather
/// than an answer about availability.
///
/// Separate from [`AVAILABILITY_PROBES`] for one measured reason: a probe's
/// answer may TAINT the local it is bound to (that is how `let ok = p.exists();
/// if !ok { return; }` is caught), but a payload must not. Tainting
/// `let src = fs::read_to_string(p).unwrap_or_else(|e| panic!(..));` propagated
/// through `format_source(&src, &cfg)` to `formatted`, and flagged the SUCCESS
/// path of `fmt_stdlib_stability::check_or_skip` — `if formatted == src {
/// return; }`, a test returning because the file it just checked is STABLE. A
/// gate that lands red on a non-defect is not a stronger gate.
///
/// In a HEAD SCRUTINEE both kinds count the same: swallowing a tracked input's
/// read failure and returning is a fail-open skip whatever the `Ok` carried.
const INPUT_READS: &[&str] = &["read_to_string(", "fs::read("];

/// How many lines a control-flow head may span before its block-opening brace.
const MAX_HEAD_SPAN: usize = 6;

/// Does any of `text` (already masked, so comments are gone) call the helper?
pub fn routed_text(text: &str) -> bool {
    ROUTED_MARKERS.iter().any(|m| text.contains(m))
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The byte offset of `w` in `s` at or after `from`, as a WHOLE word.
fn find_word(s: &str, w: &str, from: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut p = from.min(s.len());
    while let Some(off) = s.get(p..)?.find(w) {
        let at = p + off;
        let before_ok = at == 0 || !is_ident_byte(b[at - 1]);
        let after = at + w.len();
        let after_ok = after >= b.len() || !is_ident_byte(b[after]);
        if before_ok && after_ok {
            return Some(at);
        }
        p = at + w.len();
    }
    None
}

/// Lexer state that survives a line boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    Str,
    Raw(usize),
    Comment(usize),
}

/// Every line of `text` with literal CONTENTS blanked and comments removed,
/// byte offsets preserved so a column in the mask is a column in the source.
pub fn mask_code(text: &str) -> Vec<String> {
    let mut mode = Mode::Code;
    let mut out = Vec::new();
    for line in text.lines() {
        let (masked, next) = mask_line(line, mode);
        out.push(masked);
        mode = next;
    }
    out
}

fn mask_line(line: &str, start: Mode) -> (String, Mode) {
    let b = line.as_bytes();
    let mut out = vec![b' '; b.len()];
    let mut mode = start;
    let mut i = 0;
    while i < b.len() {
        match mode {
            Mode::Comment(d) => {
                if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    mode = if d <= 1 {
                        Mode::Code
                    } else {
                        Mode::Comment(d - 1)
                    };
                    i += 2;
                } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    mode = Mode::Comment(d + 1);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Mode::Str => {
                if b[i] == b'\\' {
                    i += 2;
                } else if b[i] == b'"' {
                    out[i] = b'"';
                    mode = Mode::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            Mode::Raw(h) => {
                let end = i + 1 + h;
                if b[i] == b'"' && end <= b.len() && b[i + 1..end].iter().all(|c| *c == b'#') {
                    out[i] = b'"';
                    mode = Mode::Code;
                    i = end;
                } else {
                    i += 1;
                }
            }
            Mode::Code => {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
                    break; // a line comment is not code
                }
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    mode = Mode::Comment(1);
                    i += 2;
                    continue;
                }
                if let Some((next, after)) = open_literal(b, i) {
                    out[i] = b'"';
                    mode = next;
                    i = after;
                    continue;
                }
                if b[i] == b'\'' {
                    i = skip_char_or_lifetime(b, i, &mut out);
                    continue;
                }
                out[i] = b[i];
                i += 1;
            }
        }
    }
    (
        String::from_utf8(out).expect("mask keeps ascii structure"),
        mode,
    )
}

/// If a string literal opens at `i`, the state to enter and the offset after
/// its opening delimiter. Handles `"`, `b"`, `r"`, `br"` and `r#*"`.
fn open_literal(b: &[u8], i: usize) -> Option<(Mode, usize)> {
    if i > 0 && is_ident_byte(b[i - 1]) && b[i] != b'"' {
        return None; // `for_` is not a byte-string prefix
    }
    let mut j = i;
    if b[j] == b'b' {
        j += 1;
    }
    if b.get(j) == Some(&b'r') {
        j += 1;
        let hashes = b[j..].iter().take_while(|c| **c == b'#').count();
        j += hashes;
        if b.get(j) == Some(&b'"') {
            return Some((Mode::Raw(hashes), j + 1));
        }
        return None;
    }
    if b.get(j) == Some(&b'"') {
        return Some((Mode::Str, j + 1));
    }
    None
}

/// A `'` opens either a char literal (whose contents must be blanked — `'{'`
/// would otherwise unbalance every block below it) or a lifetime (which is
/// code). Returns the offset to resume at.
fn skip_char_or_lifetime(b: &[u8], i: usize, out: &mut [u8]) -> usize {
    if b.get(i + 1) == Some(&b'\\') {
        let mut j = i + 2;
        while j < b.len() && b[j] != b'\'' {
            j += 1;
        }
        return (j + 1).min(b.len());
    }
    if b.get(i + 2) == Some(&b'\'') {
        return i + 3;
    }
    out[i] = b'\'';
    i + 1
}

/// A control-flow head whose block may hold a fail-open early-out.
struct Head {
    scrutinee: String,
    open: (usize, usize),
}

/// The `{` opening a block, searched forward from `(li, col)` at bracket depth
/// zero so a `{` inside the condition's parentheses is not mistaken for it.
fn find_open_brace(code: &[String], li: usize, col: usize) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    let last = (li + MAX_HEAD_SPAN).min(code.len().saturating_sub(1));
    for (n, line) in code.iter().enumerate().take(last + 1).skip(li) {
        let from = if n == li { col } else { 0 };
        for (ci, c) in line.bytes().enumerate().skip(from) {
            match c {
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth -= 1,
                b'{' if depth <= 0 => return Some((n, ci)),
                b';' if depth <= 0 => return None, // statement ended, no block
                _ => {}
            }
        }
    }
    None
}

fn text_between(code: &[String], from: (usize, usize), to: (usize, usize)) -> String {
    let mut s = String::new();
    for (n, line) in code.iter().enumerate().take(to.0 + 1).skip(from.0) {
        let a = if n == from.0 { from.1 } else { 0 };
        let b = if n == to.0 { to.1 } else { line.len() };
        s.push_str(&line[a.min(line.len())..b.min(line.len())]);
        s.push(' ');
    }
    s
}

/// The head opening on `code[li]`, or `None` if this line opens no scrutinised
/// block. `let ... else` is tried first: its scrutinee sits between `=` and
/// `else`, and treating it as an `if` would read the wrong expression.
///
/// A `let` line WITHOUT an `else` used to return `None` outright, which made
/// `let src = match std::fs::read_to_string(p) { Ok(s) => s, Err(_) => return };`
/// invisible twice over — the binding swallowed the line before this was even
/// reached, and this gave up on it if it was. That is the shape
/// `tests/stmt_keyword_recognizer.rs` carried on the TRACKED self-host source.
/// The `let` prefix now only DECIDES the scrutinee when an `else` is present;
/// otherwise the line falls through to the ordinary `if` / `match` search, which
/// reads the initialiser's own head.
fn head_of(code: &[String], li: usize) -> Option<Head> {
    let line = &code[li];
    if line.trim_start().starts_with("let ") {
        if let Some(eq) = plain_assign(line) {
            if let Some(els) = find_word(line, "else", eq) {
                let open = find_open_brace(code, li, els + 4)?;
                return Some(Head {
                    scrutinee: line[eq + 1..els].to_string(),
                    open,
                });
            }
        }
    }
    let (kw, len) = find_word(line, "if", 0)
        .map(|p| (p, 2))
        .or_else(|| find_word(line, "match", 0).map(|p| (p, 5)))?;
    let open = find_open_brace(code, li, kw + len)?;
    Some(Head {
        scrutinee: text_between(code, (li, kw + len), open),
        open,
    })
}

/// The offset of a plain `=` (not `==`, `=>`, `!=`, `<=`, `>=`, `+=` …).
fn plain_assign(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    (0..b.len()).find(|&i| {
        b[i] == b'='
            && b.get(i + 1) != Some(&b'=')
            && b.get(i + 1) != Some(&b'>')
            && !matches!(
                b.get(i.wrapping_sub(1)),
                Some(
                    b'=' | b'!'
                        | b'<'
                        | b'>'
                        | b'+'
                        | b'-'
                        | b'*'
                        | b'/'
                        | b'%'
                        | b'&'
                        | b'|'
                        | b'^'
                )
            )
    })
}

/// The `}` matching the `{` at `open`, or the last line if the file ends first.
fn block_end(code: &[String], open: (usize, usize)) -> (usize, usize) {
    let mut depth = 0i32;
    for (n, line) in code.iter().enumerate().skip(open.0) {
        let from = if n == open.0 { open.1 } else { 0 };
        for (ci, c) in line.bytes().enumerate().skip(from) {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return (n, ci + 1);
                    }
                }
                _ => {}
            }
        }
    }
    (code.len().saturating_sub(1), 0)
}

/// The `{` of an `else` / `else if` block hanging off the `}` at `after`.
///
/// A probe whose early-out sits in the OTHER branch is the same fail-open skip
/// written the other way up (`if tool.exists() { run() } else { return; }`), so
/// the scrutinised region is the whole chain, not the first block of it.
fn else_block_after(code: &[String], after: (usize, usize)) -> Option<(usize, usize)> {
    let last = (after.0 + 2).min(code.len().saturating_sub(1));
    for (n, line) in code.iter().enumerate().take(last + 1).skip(after.0) {
        let from = if n == after.0 { after.1 } else { 0 };
        let rest = line.get(from..)?;
        if rest.trim().is_empty() {
            continue;
        }
        let els = find_word(rest, "else", 0)?;
        if !rest[..els].trim().is_empty() {
            return None; // something else runs first; the chain ended
        }
        return find_open_brace(code, n, from + els + 4);
    }
    None
}

/// Does `expr` CALL `needle`, as a call rather than as a tail of a longer name?
///
/// A needle that opens with an identifier byte (`which(`, `var(`, `metadata(`)
/// must not match `some_which(` or `to_var(`; one that opens with `.` carries
/// its own left boundary. `expr.contains(needle)` had no boundary at all, which
/// is why the narrow vocabulary could only be widened by spelling a leading dot
/// into every entry — a convention nothing enforced.
fn calls(expr: &str, needle: &str) -> bool {
    let b = expr.as_bytes();
    let head_is_ident = needle.as_bytes().first().is_some_and(|c| is_ident_byte(*c));
    let mut from = 0usize;
    while let Some(off) = expr.get(from..).and_then(|s| s.find(needle)) {
        let at = from + off;
        if !head_is_ident || at == 0 || !is_ident_byte(b[at - 1]) {
            return true;
        }
        from = at + needle.len();
    }
    false
}

/// Is this expression's answer derived from a capability probe?
///
/// Both vocabularies count here: this decides what a control-flow HEAD is
/// scrutinising, and a swallowed input-read failure is as fail-open as a
/// swallowed `.exists()`.
fn probes(expr: &str, tainted: &HashSet<String>) -> bool {
    if AVAILABILITY_PROBES
        .iter()
        .chain(INPUT_READS)
        .any(|s| calls(expr, s))
    {
        return true;
    }
    tainted.iter().any(|n| find_word(expr, n, 0).is_some())
}

/// Does `expr` yield an AVAILABILITY ANSWER — the thing a local may carry to a
/// later head — rather than a payload?
///
/// [`INPUT_READS`] are deliberately absent: see that constant for the false
/// positive tainting them produced. The residual this leaves is named in
/// `tests/fail_open_skip_site_ratchet.rs`.
fn binds_probe_answer(expr: &str, tainted: &HashSet<String>) -> bool {
    if AVAILABILITY_PROBES.iter().any(|s| calls(expr, s)) {
        return true;
    }
    tainted.iter().any(|n| find_word(expr, n, 0).is_some())
}

/// The name a simple `let <name> = <expr>;` binds, with its right-hand side.
///
/// Destructuring patterns are deliberately skipped: `let Ok(x) = ..` either
/// carries an `else` (handled as a head above) or cannot early-out at all.
fn simple_binding(line: &str) -> Option<(String, String)> {
    let t = line.trim_start();
    let lhs = t.strip_prefix("let ")?;
    if find_word(line, "else", 0).is_some() {
        return None;
    }
    let lhs = lhs.strip_prefix("mut ").unwrap_or(lhs).trim_start();
    let n = lhs.bytes().take_while(|b| is_ident_byte(*b)).count();
    if n == 0 {
        return None;
    }
    let eq = plain_assign(line)?;
    Some((lhs[..n].to_string(), line[eq + 1..].to_string()))
}

/// Does `text` hand the caller nothing it can tell apart from success?
///
/// A `return` token followed by `;`, `,`, `}` or by `None` / `Ok(())` and then
/// one of those; or a line that IS a bare `None`. `return Some(bin)` and
/// `return Err(e)` are real verdicts and never match.
fn has_bare_exit(text: &str) -> bool {
    let mut from = 0;
    while let Some(at) = find_word(text, "return", from) {
        let rest = text[at + 6..].trim_start();
        let rest = rest
            .strip_prefix("None")
            .or_else(|| rest.strip_prefix("Ok(())"))
            .unwrap_or(rest)
            .trim_start();
        if rest.is_empty() || rest.starts_with([';', ',', '}']) {
            return true;
        }
        from = at + 6;
    }
    text.lines()
        .any(|l| matches!(l.trim(), "None" | "None," | "None;"))
}

/// Every control-flow block in `text` that skips on a capability probe without
/// consulting `common::gate`, as `<rel>:<line> (probe block)`.
pub fn skip_block_sites(rel: &str, text: &str) -> Vec<String> {
    let code = mask_code(text);
    let mut tainted: HashSet<String> = HashSet::new();
    let mut open = Vec::new();
    for li in 0..code.len() {
        if find_word(&code[li], "fn", 0).is_some() {
            tainted.clear(); // a name never crosses a function boundary
        }
        if let Some((name, rhs)) = simple_binding(&code[li]) {
            if binds_probe_answer(&rhs, &tainted) {
                tainted.insert(name);
            }
            // NOT `continue`: an initialiser can BE the scrutinised block
            // (`let s = match read(p) { Err(_) => return, .. };`), and skipping
            // the line after recording the taint hid every one of them.
        }
        let Some(head) = head_of(&code, li) else {
            continue;
        };
        if !probes(&head.scrutinee, &tainted) {
            continue;
        }
        let mut end = block_end(&code, head.open);
        while let Some(next) = else_block_after(&code, end) {
            end = block_end(&code, next);
        }
        let body = text_between(&code, head.open, end);
        if routed_text(&body) || routed_text(&head.scrutinee) {
            continue;
        }
        if has_bare_exit(&body) {
            open.push(format!("{}:{} (probe block)", rel, li + 1));
        }
    }
    open
}

/// Positive controls for the vocabulary and the block scanner.
///
/// They live in this DIRECTORY, beside the rule they prove, rather than in the
/// ratchet that consumes it: a spelling added to the vocabulary without a
/// control is then a hole visible in the same diff, and
/// [`controls::the_vocabulary_and_its_controls_agree`] makes that mechanical
/// rather than a habit.
#[cfg(test)]
mod controls;
