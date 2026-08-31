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

# Attribution-shaped patterns. `fable` (= an internal model codename) is never a
# legitimate integration target in these repos, so it is flagged bare (word-
# bounded, to spare "affable"/"ineffable"). Other vendors are flagged only when
# adjacent to an authorship/review verb, so legitimate words ("grok" the verb,
# "opus", a "Gemini CLI" integration line) do not false-positive.
PATTERN='\bfable\b|copilot|chatgpt|[0-9]+[- ]llm consensus|claude/[a-z-]+-[A-Za-z0-9]{4,}|\b(deepseek|mistral|grok|gemini|gpt|opus|sonnet|haiku|kimi|qwen|nemotron|glm|moonshot|zhipu|anthropic|openai)[- ]?(audit|panel|review|finding|consensus|converged|driven|flagged|authored)\b'

# Excludes: vendored node_modules, the generated file index, and THIS file
# (which necessarily contains the example patterns above).
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
  ':!node_modules' ':!**/node_modules' ':!ANATOMY.md'
  ':!scripts/check_no_ai_attribution.sh'
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

hits=$(git grep -inE "$PATTERN" -- "${PATHSPEC[@]}")
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
  echo "'cross-model review' / a neutral 'audit finding'. Integration-target and"
  echo "runtime-LLM-backend mentions (Claude Code, Gemini CLI, a shelled-out CLI)"
  echo "are fine."
  exit 1
fi
echo "no-ai-attribution gate: PASS ($scanned tracked files scanned)"
