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

# Second pattern: PROVENANCE-shaped credits, which the verb-adjacency rule above
# structurally cannot see. That rule requires the vendor token to sit IMMEDIATELY
# before an authorship verb ([- ]? separator), so a credit written as a noun
# phrase -- "<vendor> PR #216 Finding 2", "(<vendor> corr-audit #3)", "reviewed
# by <vendor>-5" -- read as clean. Eight such credits were live in .rs/.py source
# when this rule was added. It therefore:
#   * matches in BOTH orders (vendor -> credit-word and credit-word -> vendor),
#   * tolerates a SHORT gap (up to two <=4-letter filler words, e.g. "by"), which
#     is what catches the parenthesised "(<vendor> Finding 2)" shape and the
#     trailing "reviewed by <vendor>" shape with one rule,
#   * adds the review-NOUN vocabulary the first pattern lacks: finding(s), pr,
#     corr-audit, sweep, scan, reviewed, verified, found.
# The gap is deliberately short and admits no long word: that is precisely what
# keeps the ALLOWED integration lines clean -- the coding-agent client list in
# scripts/anatomy.sh, the shelled-out CLI backend in tools/mindfuzz, the
# "<vendor>-style token count" heuristics in benchmarks/, the "<vendor> announced
# the Model Hardware Standard" third-party-fact lines in docs/, and the plugin
# install section in README.md. Widening the gap starts flagging those; measure
# against them (git grep -inE the AI_NAMES list alone) before touching it.
AI_NAMES='codex|claude|deepseek|mistral|grok|gemini|gpt|opus|sonnet|haiku|kimi|qwen|nemotron|glm|moonshot|zhipu|anthropic|openai|llama|fable|copilot|chatgpt'
CREDIT_WORDS='corr-audit|audit|panel|review|reviews|reviewer|reviewed|verified|found|finding|findings|sweep|scan|consensus|converged|driven|flagged|authored|pr'
CREDIT_GAP='([^[:alnum:]]{1,3}[a-z]{1,4}){0,2}[^[:alnum:]]{0,3}'
PATTERN_CREDIT="\b(${AI_NAMES})\b${CREDIT_GAP}\b(${CREDIT_WORDS})\b|\b(${CREDIT_WORDS})\b${CREDIT_GAP}\b(${AI_NAMES})\b"

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
echo "no-ai-attribution gate: PASS ($scanned tracked files scanned)"
