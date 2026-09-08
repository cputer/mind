#!/usr/bin/env bash
# Required-CI execution gate for the existing MIC3 two-leg PQC-hybrid controls.
#
# This uses only the deterministic, non-production seeds embedded in the Rust
# unit tests. It neither reads signing-key environment variables nor emits a
# release artifact.
set -euo pipefail

readonly feature_set="evidence-mldsa,evidence-slhdsa"
readonly test_prefix="ir::compact::v3::evidence::tests::pqc_hybrid_"
readonly expected_count=2
readonly -a expected_tests=(
  "${test_prefix}non_degradable"
  "${test_prefix}valid_and_byte_identical"
)
expected_listing="$(printf '%s\n' "${expected_tests[@]}" | LC_ALL=C sort)"

list_log="$(mktemp)"
run_log="$(mktemp)"
trap 'rm -f "$list_log" "$run_log"' EXIT

cargo_test=(
  cargo test
  --no-default-features
  --features "$feature_set"
  --lib
  --color never
)
if [[ -f Cargo.lock ]]; then
  cargo_test+=(--locked)
fi

# Ask libtest which tests the prefix selects before executing it. This is the
# authority for the count: a source comment or substring assumption cannot turn
# an empty/over-broad filter green.
"${cargo_test[@]}" -- --list --format terse | tee "$list_log"
actual_tests="$({
  awk -F ': test$' \
    -v prefix="$test_prefix" \
    'index($0, prefix) == 1 && $0 ~ /: test$/ { print $1 }' "$list_log"
} | LC_ALL=C sort)"

if [[ "$actual_tests" != "$expected_listing" ]]; then
  printf 'PQC-HYBRID-CONTROLS FAIL: expected these exact tests:\n%s\n' "$expected_listing" >&2
  printf 'PQC-HYBRID-CONTROLS FAIL: libtest discovered:\n%s\n' \
    "${actual_tests:-<none>}" >&2
  exit 1
fi

discovered_count="$(printf '%s\n' "$actual_tests" | awk 'NF { n += 1 } END { print n + 0 }')"
if [[ "$discovered_count" -ne "$expected_count" ]]; then
  printf 'PQC-HYBRID-CONTROLS FAIL: discovered=%s expected=%s\n' \
    "$discovered_count" "$expected_count" >&2
  exit 1
fi

"${cargo_test[@]}" "$test_prefix" -- --test-threads=1 | tee "$run_log"

for test_name in "${expected_tests[@]}"; do
  result_count="$(awk -v line="test $test_name ... ok" '$0 == line { n += 1 } END { print n + 0 }' "$run_log")"
  if [[ "$result_count" -ne 1 ]]; then
    printf 'PQC-HYBRID-CONTROLS FAIL: no unique passing result for %s\n' \
      "$test_name" >&2
    exit 1
  fi
  printf '[PASS] %s executed exactly once\n' "$test_name"
done

if ! grep -Eq \
  '^test result: ok\. 2 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in .+$' \
  "$run_log"; then
  printf 'PQC-HYBRID-CONTROLS FAIL: cargo did not report exactly 2 passing tests\n' >&2
  exit 1
fi

printf 'PQC-HYBRID-CONTROLS complete discovered=%s executed=%s features=%s\n' \
  "$discovered_count" "$expected_count" "$feature_set"
