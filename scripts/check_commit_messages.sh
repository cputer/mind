#!/usr/bin/env bash
# check_commit_messages.sh <rev-range> — no model or tool credit in commit
# HISTORY. Companion to scripts/check_no_ai_attribution.sh (which scans tracked
# FILE contents) and to scripts/commit-msg-hook.sh (which checks the message
# being written locally). All three source ONE definition,
# scripts/ai_attribution_patterns.sh, so the file gate and the message gate can
# never disagree about what a credit is.
#
# Why this exists: nothing checked commit messages at all. There was no
# commit-msg hook, and no CI step read `git log` — so the very same credit the
# file gate rejects inside a comment could ride into a PUBLIC repo's permanent
# history in the message beside it, where (unlike a file) it cannot be edited
# out without rewriting every descendant commit.
#
# Three rules, applied to the FULL message (%B, i.e. subject + body + trailers)
# and to the author/committer identities:
#   1. the shared attribution patterns (bare codename, vendor-next-to-verb,
#      vendor-next-to-provenance-noun in either order);
#   2. ANY co-authorship trailer, whatever name follows it — this project is
#      single-author by policy, so the trailer is forbidden by shape, not by who
#      it credits (that also makes the rule un-evadable by renaming);
#   3. a "Generated with/by <tool>" footer line.
#
# GATE-EVIDENCE RULE (Constitution XII.2): exit 0 answers "did anything that ran
# fail", never "did the gate run anything". A rev-range that resolves to nothing
# — a wrong base ref, a force-pushed before-sha, a shallow clone — would
# otherwise print nothing and exit 0 while asserting nothing at all. So the
# commit COUNT is derived first and printed always, an empty range is reported as
# empty rather than as a pass, and any git failure fails CLOSED.
set -uo pipefail

RANGE="${1:-}"
if [ -z "$RANGE" ]; then
  echo "usage: $0 <rev-range>            e.g. $0 origin/main..HEAD" >&2
  echo "       (a range, not a single rev: a lone sha scans the whole ancestry)" >&2
  exit 2
fi

cd "$(git rev-parse --show-toplevel)" || exit 1

PATTERNS_FILE="scripts/ai_attribution_patterns.sh"
if [ ! -f "$PATTERNS_FILE" ]; then
  echo "::error::missing $PATTERNS_FILE - the gate has no patterns to apply and"
  echo "         cannot pass. Restore it; do not inline a second copy."
  exit 1
fi
# shellcheck source=scripts/ai_attribution_patterns.sh
. "$PATTERNS_FILE"
if [ -z "${PATTERN:-}" ] || [ -z "${PATTERN_CREDIT:-}" ] ||
   [ -z "${PATTERN_COAUTHOR:-}" ] || [ -z "${PATTERN_GENERATED:-}" ]; then
  echo "::error::$PATTERNS_FILE defined no usable pattern set - refusing to pass"
  echo "         on an empty pattern (that would scan every commit for nothing)."
  exit 1
fi

# Count FIRST, and read git's own exit status: `git rev-list` prints nothing both
# for a legitimately empty range and for a range it could not resolve.
errfile=$(mktemp "${TMPDIR:-/tmp}/ccm-revlist.XXXXXX")
trap 'rm -f "$errfile"' EXIT
count=$(git rev-list --count "$RANGE" 2>"$errfile")
rc=$?
if [ "$rc" -ne 0 ]; then
  echo "::error::commit-message gate could not resolve the range '$RANGE'"
  echo "         (git rev-list exited $rc) - failing closed rather than reporting"
  echo "         a pass over commits it never read:"
  sed 's/^/         /' "$errfile" >&2
  exit 1
fi
if [ "$count" = 0 ]; then
  echo "commit-message gate: scanned 0 commits (empty range) - range '$RANGE'"
  echo "resolved to no commits, so this gate asserted nothing. That is a correct"
  echo "PASS only because there was provably nothing to check."
  exit 0
fi

shas=$(git rev-list "$RANGE" 2>/dev/null)
rc=$?
if [ "$rc" -ne 0 ] || [ -z "$shas" ]; then
  echo "::error::rev-list --count said $count commits but the enumeration"
  echo "         returned $( [ -z "$shas" ] && echo none || echo 'an error' ) (rc=$rc) - failing closed."
  exit 1
fi

offenders=0
scanned=0
while read -r sha; do
  [ -z "$sha" ] && continue
  scanned=$((scanned + 1))
  subject=$(git log -1 --format=%s "$sha")
  # %B is the raw message: subject, body and every trailer. The author and
  # committer identities are appended as synthetic lines so a credit hidden in a
  # NAME rather than in the message text is caught by the same rules.
  text=$(git log -1 --format='%B%nAuthor: %an <%ae>%nCommitter: %cn <%ce>' "$sha")
  hits=$(printf '%s\n' "$text" |
         grep -inE -e "$PATTERN" -e "$PATTERN_CREDIT" \
                   -e "$PATTERN_COAUTHOR" -e "$PATTERN_GENERATED")
  grc=$?
  if [ "$grc" -gt 1 ]; then
    echo "::error::grep failed (rc=$grc) on $sha - the gate did not run; refusing to pass."
    exit 1
  fi
  if [ -n "$hits" ]; then
    offenders=$((offenders + 1))
    echo "::error::${sha} ${subject}"
    printf '%s\n' "$hits" | sed 's/^/    msg:/'
  fi
done <<< "$shas"

if [ "$scanned" != "$count" ]; then
  echo "::error::enumerated $scanned commits but the range holds $count - the gate"
  echo "         did not see the whole range; failing closed."
  exit 1
fi

echo "commit-message gate: scanned $count commits in '$RANGE', $offenders offending"
if [ "$offenders" -gt 0 ]; then
  echo ""
  echo "A commit message may not credit a model or a tool with authoring or"
  echo "reviewing this work, and may not carry a co-authorship trailer (this"
  echo "project is single-author by policy) or a 'Generated with' footer."
  echo "Describe the CHANGE, not what produced it: 'external correctness review"
  echo "finding 2' rather than a tool name beside a PR/finding number."
  echo ""
  echo "Unpushed commits: rewrite the messages (git rebase -i / git commit"
  echo "--amend) BEFORE pushing - history on a public repo cannot be edited"
  echo "afterwards without rewriting every descendant commit."
  exit 1
fi
exit 0
