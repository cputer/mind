#!/usr/bin/env bash
# Copyright 2026 STARGA Inc.
# Licensed under the Apache License, Version 2.0.
# Sourced helper for the executable-semantics driver; not a standalone gate.

run_tier_cargo() {
  local tier=$1 features=$2 require_toolchain=$3
  local -a tier_env=()
  # Eligible VNNI hosts must execute the manifest-bound native workload.
  # Missing opt-in is a verification failure, so every exec caller sets it.
  if [ "$tier" = exec ]; then
    tier_env+=(MIND_INTDOT_VNNI_VERIFY=1)
  fi
  if [ "$require_toolchain" = 1 ]; then
    tier_env+=(MIND_BENCH_REQUIRE=1)
  fi
  env "${tier_env[@]}" cargo test --no-default-features --features "$features" \
    --no-fail-fast
}
