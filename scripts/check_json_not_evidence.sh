#!/usr/bin/env bash
# Wedge-integrity gate: JSON is never an evidence-hash preimage.
#
# MIND's canonical / evidence format is mic@3 + the MAP epilogue (trace_hash =
# sha256 of canonical mic@3 bytes), canonical BY CONSTRUCTION. std/json's dump is
# DETERMINISTIC but NOT canonical (object key order = insertion order), so feeding
# a json serialization into a hash/evidence preimage reintroduces the exact
# canonicalization footgun the wedge exists to kill. App-level evidence records
# serialize via canonical MIND records (fixed-layout binary + std.sha256; see
# std/io_canon.mind), never JSON. JSON is for the interop boundary only
# (HTTP, MCP, LLM tool-calling, config).
#
# This gate flags a json serialize call (jv_dump/jv_encode/json.encode/json_dump)
# appearing directly inside a hash/evidence call on the same line — the
# `sha256(jv_encode(...))` anti-pattern. Cross-line dataflow is out of grep's
# reach; that's the mind-auditor agent's job. Conservative by design: a real
# non-evidence json hash (e.g. a cache key) can opt out with a trailing
# `// json-hash-ok: <reason>` marker.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

# A hash/evidence sink taking a json-serialize result as an argument, same line.
SINK='sha256|mini_sha256|trace_hash|nb_sha256|evidence_hash|preimage|anchor_hash'
JSON='jv_dump|jv_encode|json_encode|json_dump|json\.encode|json\.dump'
PATTERN="(${SINK})[a-z_]*[[:space:]]*\([^;]*\b(${JSON})\b"

PATHSPEC=( '*.mind' ':!node_modules' ':!**/node_modules' )

# Vacuity floor. This gate scans ONLY *.mind, so it is one pathspec typo away
# from scanning nothing at all -- and `2>/dev/null || true` used to render that
# indistinguishable from a clean tree: empty `hits` printed PASS whether the scan
# found no violation, matched no file, or failed outright. The corpus size is now
# compared to a floor and `git grep`'s own status is read (0 match / 1 no match /
# >=2 error). ONE pathspec array feeds the count and the scan, so they cannot drift.
MIN_SCANNED=100  # tracked *.mind today: ~345
scanned=$(git ls-files -- "${PATHSPEC[@]}" | wc -l)
if [ "$scanned" -lt "$MIN_SCANNED" ]; then
  echo "::error::json-not-evidence gate scanned only $scanned tracked .mind files"
  echo "         (floor $MIN_SCANNED). The scan scope evaporated - a PASS here"
  echo "         would assert nothing. Check the pathspec above."
  exit 1
fi

raw=$(git grep -nE "$PATTERN" -- "${PATHSPEC[@]}")
rc=$?
if [ "$rc" -gt 1 ]; then
  echo "::error::git grep failed (rc=$rc) - the gate did not run; refusing to pass."
  exit 1
fi
hits=$(printf '%s' "$raw" | grep -viE 'json-hash-ok')

if [ -n "$hits" ]; then
  echo "::error::JSON fed into an evidence/hash preimage (forbidden — wedge integrity):"
  echo "$hits"
  echo ""
  echo "MIND's canonical/evidence bytes are mic@3 + MAP (trace_hash = sha256 of"
  echo "canonical mic@3 bytes). json dump is deterministic but NOT canonical."
  echo "Serialize evidence records via canonical MIND records (fixed-layout binary"
  echo "+ std.sha256; see std/io_canon.mind), NOT JSON. If this hash is genuinely"
  echo "non-evidence (e.g. a cache key), append '// json-hash-ok: <reason>'."
  exit 1
fi
echo "json-not-evidence gate: PASS ($scanned tracked .mind files scanned)"
