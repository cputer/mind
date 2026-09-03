#!/usr/bin/env bash
# Public-artifact hygiene gate: no AI tool/model named as having AUTHORED or
# REVIEWED MIND.
#
# STARGA policy: naming a supported MCP / CLI *client* ("Claude Code",
# "Gemini CLI", "Cursor") as an integration target, or an LLM backend a tool
# shells out to at runtime, is fine. Bare review/authorship attributions —
# "Fable audit", "Copilot", "ChatGPT", "DeepSeek panel", "N-LLM consensus" — are
# not. This scans source + docs + tests, not just Markdown: the earlier md-only,
# 4-term gate let "Fable audit"/"IR-audit" attributions leak into ~60 source
# comments across .rs/.py/.mind/.sh/.toml before this was caught.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

# The patterns live in ONE file, sourced by the three consumers that must agree
# about them: this whole-tree file gate, scripts/check_commit_messages.sh (commit
# MESSAGES over a rev-range) and scripts/commit-msg-hook.sh (the message being
# written). Read scripts/ai_attribution_patterns.sh for the reasoning behind each
# pattern and for what must be measured before widening one. Sourcing FAILS
# CLOSED: a missing definitions file is an error, never an empty pattern that
# silently matches nothing and prints PASS.
# Repo-root-relative, deliberately: the `cd` above already put us there, and a
# $0-relative path breaks when this gate is invoked from a subdirectory.
PATTERNS_FILE="scripts/ai_attribution_patterns.sh"
if [ ! -f "$PATTERNS_FILE" ]; then
  echo "::error::missing $PATTERNS_FILE - the gate has no patterns to apply and"
  echo "         cannot pass. Restore it; do not inline a second copy."
  exit 1
fi
# shellcheck source=scripts/ai_attribution_patterns.sh
. "$PATTERNS_FILE"
if [ -z "${PATTERN:-}" ] || [ -z "${PATTERN_CREDIT:-}" ] || [ -z "${AI_NAMES:-}" ]; then
  echo "::error::$PATTERNS_FILE defined no PATTERN/PATTERN_CREDIT/AI_NAMES -"
  echo "         refusing to pass on an empty pattern set."
  exit 1
fi

# Excludes: vendored node_modules and the two files that necessarily CONTAIN
# the example patterns (this one and the sourced definitions file) - a gate that
# flags its own rulebook can never pass.
#
# ANATOMY.md is NOT excluded, though it used to be as "the generated file
# index". That exclusion inverted the risk: ANATOMY.md is the one tracked file
# whose contents are COPIED from other files (each entry carries a filename and
# that file's first meaningful line), so it is the likeliest carrier of a credit
# nobody typed by hand -- and it was the only file this gate never read. A
# committed ANATOMY.md did in fact ship a line naming a model as this compiler's
# owner while this gate printed PASS. Generated-ness is a reason to scan a
# public artifact, never a reason to skip it; the generator is now restricted to
# tracked files (scripts/anatomy.sh) so it has no private input to launder, and
# this gate reads its output as it reads every other tracked doc.
#
# Measured while removing the exclusion: putting ANATOMY.md in scope is NECESSARY
# but not SUFFICIENT for that leak. The line that actually shipped had the shape
# "Handoff for <vendor> (compiler owner)", and neither PATTERN (which flags a
# vendor token only next to an authorship/review verb) nor PATTERN_CREDIT (same,
# in either order) matches it -- "owner" is not credit vocabulary. Widening the
# shared pattern to flag a BARE vendor name is the wrong fix: a supported-client
# integration target is explicitly allowed policy and appears legitimately in
# README.md and scripts/anatomy.sh. The rule belongs to the ARTIFACT instead --
# a GENERATED index has no legitimate reason to carry a vendor name in any
# position -- so it is the ARTIFACT SCAN at the bottom of this file.
#
# It used to live ONLY in scripts/test_no_ai_attribution.py, which runs in CI.
# That put the rule at the wrong LAYER: .githooks/pre-commit and (now)
# scripts/preflight.sh both run THIS script, so the single leak class that has
# actually shipped was invisible at exactly the two layers where a developer
# could still have stopped it -- a correctly installed hook would have let that
# commit through. The self-test keeps the mutation cases and now calls this gate
# instead of re-implementing the grep, so the rule has one definition.
#
# deferred: SCAN SCOPE is narrower than the tree — .ts (23 files), .yml (10),
# .js (9), .c (7) and .mojo (4) are tracked but never scanned, so an attribution
# in an SDK source or a workflow file is still missed. This is a separate gap
# from the CI TRIGGER scope (closed 2026-08-28 by removing the docs-claims paths
# filter; scripts/check_gate_wiring.py keeps trigger >= scan). Upgrade path: add
# '*.ts' '*.js' '*.c' '*.h' '*.yml' here, but MEASURE false positives first —
# vendored/generated JS and sourcemaps are the risk — and exclude them by
# pathspec rather than weakening PATTERN. The wiring lint picks up any widening
# automatically, since it reads this pathspec rather than a second copy of it.
PATHSPEC=(
  '*.md' '*.rs' '*.py' '*.mind' '*.sh' '*.toml' '*.rst' '*.txt'
  ':!node_modules' ':!**/node_modules'
  ':!scripts/check_no_ai_attribution.sh' ':!scripts/ai_attribution_patterns.sh'
)

# Vacuity floor. `git grep` exits 1 on NO MATCH, and the old `2>/dev/null || true`
# turned every other failure into that same empty result: a pathspec that matches
# nothing (a renamed dir, a widened-then-typo'd extension list, a `git grep` that
# errored) printed PASS while asserting nothing at all. A whole-tree gate must
# prove it actually had a tree to scan, so the corpus size is compared to a floor
# and the grep's own exit status is read. ONE pathspec array feeds both, so the
# scanned set and the counted set can never drift apart.
MIN_SCANNED=500  # tracked matches today: ~1436
scanned=$(git ls-files -- "${PATHSPEC[@]}" | wc -l)
if [ "$scanned" -lt "$MIN_SCANNED" ]; then
  echo "::error::no-ai-attribution gate scanned only $scanned tracked files"
  echo "         (floor $MIN_SCANNED). The scan scope evaporated - a PASS here"
  echo "         would assert nothing. Check the pathspec above."
  exit 1
fi

hits=$(git grep -inE -e "$PATTERN" -e "$PATTERN_CREDIT" -- "${PATHSPEC[@]}")
rc=$?
if [ "$rc" -gt 1 ]; then
  echo "::error::git grep failed (rc=$rc) - the gate did not run; refusing to pass."
  exit 1
fi
if [ -n "$hits" ]; then
  echo "::error::AI-attribution found in tracked files (forbidden by STARGA policy):"
  echo "$hits"
  echo ""
  echo "Replace named-AI review/authorship attributions with 'recent research' /"
  echo "'cross-model review' / a neutral 'audit finding'. A provenance NOUN phrase"
  echo "counts too: write 'PR #216 review finding 2' or 'external correctness"
  echo "review #3', never a vendor/model name next to finding/pr/audit/sweep/scan."
  echo "Integration-target and runtime-LLM-backend mentions (a supported coding-"
  echo "agent client list, a shelled-out CLI backend) are fine."
  exit 1
fi
# ---------------------------------------------------------------------------
# ARTIFACT SCAN - deliberately a DIFFERENT strength from the scan above.
#
# Scope is ONE generated file, and that narrowness is the whole point: a bare
# vendor name is legal prose in README.md (the supported-client plugin list) and
# in scripts/anatomy.sh (the coding-agent client list), so this rule may not be
# applied tree-wide. A GENERATED index carries only filenames and each file's
# first meaningful line, so a vendor token in ANY position there means some
# private input got laundered into a public artifact.
#
# Fails CLOSED on a missing artifact: if ANATOMY.md is not tracked, this rule
# asserted nothing, and "asserted nothing" is a red gate, not a pass.
ARTIFACT='ANATOMY.md'
artifact_files=$(git ls-files -- "$ARTIFACT" | wc -l)
if [ "$artifact_files" -lt 1 ]; then
  echo "::error::no-ai-attribution gate: $ARTIFACT is not tracked, so the"
  echo "         artifact-scoped bare-name rule scanned nothing. A PASS here"
  echo "         would assert nothing; regenerate it with scripts/anatomy.sh."
  exit 1
fi
bare=$(git grep -nIiE "\b(${AI_NAMES})\b" -- "$ARTIFACT")
rc=$?
if [ "$rc" -gt 1 ]; then
  echo "::error::git grep failed (rc=$rc) on $ARTIFACT - the artifact rule did"
  echo "         not run; refusing to pass."
  exit 1
fi
if [ -n "$bare" ]; then
  echo "::error::vendor/model name in the generated index $ARTIFACT (forbidden"
  echo "         in ANY position, not just next to a credit verb):"
  echo "$bare"
  echo ""
  echo "$ARTIFACT is generated from the TRACKED tree by scripts/anatomy.sh, so a"
  echo "vendor name here means a private note was indexed or an indexed file's"
  echo "first line names a model. Fix the SOURCE line, then regenerate:"
  echo "  bash scripts/anatomy.sh"
  exit 1
fi

echo "no-ai-attribution gate: PASS ($scanned tracked files scanned)"
echo "no-ai-attribution artifact rule: PASS (no vendor name in $ARTIFACT)"
