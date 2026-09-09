#!/usr/bin/env bash
# Required-CI execution gate for the actual-CLI PQC-hybrid signing controls.
#
# Sibling of pqc_hybrid_ci.sh, which gates the LIBRARY controls. These drive the
# BUILT BINARY: sign, verify, scheme downgrade, signed-field mutation, and
# role-pinned key-pair trust.
#
# Seeds are the deterministic, non-production values embedded in the test
# source. This gate reads no signing-key environment variable, requires no
# production identity, and signs no release archive. The tests themselves clear
# every signing/trust variable on both the emit and the verify path, so an
# inherited value on a runner cannot change what "trusted" means.
set -euo pipefail

readonly feature_set="std-surface,evidence-mldsa,evidence-slhdsa"
readonly test_target="pqc_hybrid_cli_signing"
expected_tests=(
  "an_allowlist_naming_two_different_identities_is_rejected"
  "a_single_leg_is_refused_at_sign_time_with_no_artifact"
  "a_tampered_signed_artifact_is_rejected"
  "an_un_namespaced_application_key_is_refused_before_signing"
  "a_historical_ed25519_artifact_is_rejected_but_still_inspectable"
  "a_mixed_legacy_and_supported_configuration_refuses"
  "a_retired_ed25519_seed_refuses_instead_of_being_ignored"
  "an_unsigned_artifact_is_emitted_but_rejected_when_a_signature_is_required"
  "the_supported_pair_still_signs_after_the_retirement"
  "the_unsigned_path_is_unchanged_by_the_retirement"
  "mutating_a_signed_application_attribute_is_rejected"
  "one_correct_key_paired_with_one_wrong_key_is_rejected_either_way"
  "pinning_the_signer_accepts_the_right_key_and_rejects_everything_else"
  "rewriting_the_scheme_tag_to_an_unknown_value_is_rejected"
  "the_cli_signs_with_the_pqc_hybrid_and_the_cli_verifies_it"
  "a_historical_old_hybrid_is_rejected_but_still_inspectable"
  "a_wrong_length_slhdsa_seed_refuses_without_an_artifact_or_seed_leak"
  "a_non_ascii_slhdsa_seed_refuses_without_an_artifact_or_seed_leak"
)
# The non-UTF-8 environment control uses OsStringExt and is available on Unix
# runners. Windows still executes every portable malformed-seed control.
case "$(uname -s)" in
  Linux|Darwin) expected_tests+=("a_non_utf8_slhdsa_seed_refuses_without_an_artifact_or_seed_leak") ;;
esac
expected_count="${#expected_tests[@]}"
expected_listing="$(printf '%s\n' "${expected_tests[@]}" | LC_ALL=C sort)"

list_log="$(mktemp)"
run_log="$(mktemp)"
trap 'rm -f "$list_log" "$run_log"' EXIT

cargo_test=(
  cargo test
  --release
  --features "$feature_set"
  --test "$test_target"
  --color never
)
if [[ -f Cargo.lock ]]; then
  cargo_test+=(--locked)
fi

# Ask libtest what this target actually contains BEFORE executing it. A file
# silently cfg'd out compiles to zero tests and would otherwise report a
# cheerful "ok. 0 passed" — the exact vacuous pass this contract exists to
# reject.
"${cargo_test[@]}" -- --list --format terse | tee "$list_log"
actual_tests="$(awk -F ': test$' '$0 ~ /: test$/ { print $1 }' "$list_log" | LC_ALL=C sort)"

if [[ "$actual_tests" != "$expected_listing" ]]; then
  printf 'PQC-CLI-CONTROLS FAIL: expected these exact tests:\n%s\n' "$expected_listing" >&2
  printf 'PQC-CLI-CONTROLS FAIL: libtest discovered:\n%s\n' "${actual_tests:-<none>}" >&2
  exit 1
fi

discovered_count="$(printf '%s\n' "$actual_tests" | awk 'NF { n += 1 } END { print n + 0 }')"
if [[ "$discovered_count" -ne "$expected_count" ]]; then
  printf 'PQC-CLI-CONTROLS FAIL: discovered=%s expected=%s\n' \
    "$discovered_count" "$expected_count" >&2
  exit 1
fi

"${cargo_test[@]}" -- --test-threads=1 | tee "$run_log"

for test_name in "${expected_tests[@]}"; do
  result_count="$(awk -v line="test $test_name ... ok" '$0 == line { n += 1 } END { print n + 0 }' "$run_log")"
  if [[ "$result_count" -ne 1 ]]; then
    printf 'PQC-CLI-CONTROLS FAIL: no unique passing result for %s\n' "$test_name" >&2
    exit 1
  fi
  printf '[PASS] %s executed exactly once\n' "$test_name"
done

if ! grep -Eq \
  "^test result: ok\. ${expected_count} passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in .+\$" \
  "$run_log"; then
  printf 'PQC-CLI-CONTROLS FAIL: cargo did not report exactly %s passing tests\n' \
    "$expected_count" >&2
  exit 1
fi

printf 'PQC-CLI-CONTROLS complete discovered=%s executed=%s features=%s\n' \
  "$discovered_count" "$expected_count" "$feature_set"
