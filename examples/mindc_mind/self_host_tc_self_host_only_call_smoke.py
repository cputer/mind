#!/usr/bin/env python3
"""CPU-as-oracle smoke for the pure-MIND E2024 self-host-only-call rule.

Ports the resolve.rs advisory (SELF_HOST_ONLY_CALL_CODE, "E2024"): a callee
that `starts_with("__mind_")` but is NOT one of the registered, arity-checked
`STD_SURFACE_INTRINSICS` entries in src/intrinsics.rs gets flagged as a
Warning — the Rust/MLIR backends cannot emit that call.

The legacy pure-MIND twin `selftest_tc_self_host_only_call(w0, w1, w2, w3,
len)` remains exported for its five-word ABI. The authoritative twin,
`selftest_tc_self_host_only_call_span(buf, len)`, compares the complete source
byte span so a registered name longer than 32 bytes cannot alias a same-prefix,
same-length mutation.

Oracle construction (machine-checked, no hand table):
  1. The STD_SURFACE_INTRINSICS table (name, arity) is PARSED from the Rust
     source src/intrinsics.rs at run time — table drift fails loud.
  2. Every case's expected verdict is recomputed from the exact Rust rule
     (`name.startswith("__mind_") and name not in table`).
  3. Every case is ALSO driven through the LIVE `mindc check` oracle: a
     triggering source calling the name (at registered arity when known) is
     checked and the presence of "E2024" in the output is asserted to equal
     the rule verdict — guarding both the parsed table and the rule itself.

Corpus: every registered entry (negative), per-entry mutations (suffix /
truncation -> positive), a same-prefix/same-length different-tail negative,
unregistered `__mind_*` names incl. the bare prefix and a >32-byte name, and
non-prefixed controls.

Env: MINDC_SO (prebuilt .so, skips the build) or MINDC_BIN (default mindc).
Template: self_host_tc_classify_error_code_smoke.py.
"""
import ctypes
import os
import re
import subprocess
import sys
import tempfile

# The compiler under test is resolved through the SHARED fail-closed resolver,
# never a bare-name PATH lookup: a PATH `mindc` is another checkout's binary and
# lets this gate report green about a tree that never built one.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _selfhost_so import resolve_mindc  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
MAIN_MIND = os.path.join(HERE, "main.mind")
TYPE_CHECKER = os.path.join(ROOT, "src", "intrinsics.rs")

# Count each three-way case through the shared gate contract.  The local `fails`
# tally remains the control-flow guard, while these verdicts prevent an empty
# or unobserved corpus from publishing one green recap.
sys.path.insert(0, os.path.join(ROOT, "scripts"))
from gate_assert import check  # noqa: E402

PREFIX = "__mind_"
LONG_REGISTRY_NAME = "__mind_nerve_blas_matmul_score_q16_i64"


def parse_table_source(src):
    """Parse the actual IntrinsicSpec declarations (name -> arity).

    The registry used to be a tuple table, but its current declarations carry
    contract metadata in constructors.  Parse those declarations directly so
    adding or changing a constructor changes this oracle's input.
    """
    m = re.search(
        r"const STD_SURFACE_INTRINSICS: &\[IntrinsicSpec\] = &\[(.*?)\n\];",
        src,
        re.S,
    )
    if not m:
        raise ValueError("STD_SURFACE_INTRINSICS IntrinsicSpec declarations not found")
    body = m.group(1)
    entries = []
    entries.extend(
        (name, int(arity))
        for name, arity in re.findall(
            r'IntrinsicSpec::legacy\s*\(\s*"([^"]+)"\s*,\s*(\d+)\s*,',
            body,
        )
    )
    arities = {"NO_I64": 0, "ONE_I64": 1, "TWO_I64": 2, "FOUR_I64": 4}
    entries.extend(
        (name, arities[arity])
        for name, arity in re.findall(
            r"IntrinsicSpec::(?:frozen_native_value|frozen_value|frozen_native_only|"
            r'frozen_discard|frozen_io)\s*\(\s*"([^"]+)"\s*,\s*"[^"]+"\s*,\s*'
            r"(NO_I64|ONE_I64|TWO_I64|FOUR_I64)\s*,",
            body,
            re.S,
        )
    )
    declared_kinds = re.findall(
        r"(?m)^\s*IntrinsicSpec::([A-Za-z0-9_]+)\s*\(", body
    )
    supported_kinds = {
        "legacy",
        "frozen_native_value",
        "frozen_value",
        "frozen_native_only",
        "frozen_discard",
        "frozen_io",
    }
    if any(kind not in supported_kinds for kind in declared_kinds):
        raise ValueError(f"unsupported IntrinsicSpec constructor in registry: {declared_kinds!r}")
    if len(entries) != len(declared_kinds):
        raise ValueError(
            f"registry parser consumed {len(entries)} of {len(declared_kinds)} declarations"
        )
    if len(entries) < 30:
        raise ValueError(f"implausibly small parsed table ({len(entries)} entries)")
    table = {name: int(arity) for name, arity in entries}
    if len(table) != len(entries):
        raise ValueError("duplicate names in STD_SURFACE_INTRINSICS")
    long_names = sorted(name for name in table if len(name.encode()) > 32)
    if long_names != [LONG_REGISTRY_NAME]:
        raise ValueError(
            "FAIL: unsupported >32-byte registry rows; regenerate the full-span "
            f"twin for {long_names!r}"
        )
    return table


def registry_source():
    with open(TYPE_CHECKER, encoding="utf-8") as registry:
        return registry.read()


def parse_table():
    """Parse STD_SURFACE_INTRINSICS declarations from the Rust source."""
    try:
        return parse_table_source(registry_source())
    except ValueError as error:
        print(f"FAIL: {error} in {TYPE_CHECKER}")
        sys.exit(1)


def registry_mutation_control(source, table, span_fn):
    """Prove a changed declaration is observed by this oracle.

    This is an in-memory mutation: the compiler under test remains untouched,
    while a changed physical name must change the rule verdict for the real
    registered call.  If the parser silently falls back to a stale list, this
    control fails.
    """
    mutated_source = source.replace(
        '"__mind_argc", "argc"', '"__mind_argc_mutated", "argc"', 1
    )
    if mutated_source == source:
        raise SystemExit("FAIL: registry mutation did not find __mind_argc declaration")
    try:
        mutated = parse_table_source(mutated_source)
    except ValueError as error:
        raise SystemExit(f"FAIL: registry mutation cannot be parsed: {error}") from error
    name = "__mind_argc"
    name_bytes = name.encode()
    name_buf = ctypes.create_string_buffer(name_bytes, len(name_bytes))
    actual = span_fn(ctypes.addressof(name_buf), len(name_bytes))
    observed = check(
        actual == rust_rule(name, table) and actual != rust_rule(name, mutated),
        "changed IntrinsicSpec declaration changes the self-host registry verdict",
    )
    if not observed:
        raise SystemExit("FAIL: self-host registry parser is stale under declaration mutation")


def rust_rule(name, table):
    """The exact resolve.rs decision: prefixed AND unregistered -> 1."""
    return 1 if (name.startswith(PREFIX) and name not in table) else 0


def enc(name):
    """Pack first 32 bytes LE into 4 i64 words + TRUE length."""
    b = name.encode()
    words = [
        int.from_bytes(b[i : i + 8].ljust(8, b"\0"), "little")
        for i in (0, 8, 16, 24)
    ]
    return words + [len(b)]


def live_oracle(mindc, name, table, workdir):
    """Run `mindc check` on a source calling `name`; 1 iff E2024 is emitted."""
    arity = table.get(name, 1)
    args = ", ".join(["1"] * arity) if arity else ""
    src = f"fn main() -> i64 {{\n    let p: i64 = {name}({args});\n    return 0;\n}}\n"
    path = os.path.join(workdir, "case.mind")
    with open(path, "w") as f:
        f.write(src)
    r = subprocess.run(
        [mindc, "check", path], capture_output=True, text=True
    )
    out = r.stdout + r.stderr
    return 1 if "E2024" in out else 0


def build_cases(table):
    cases = []
    # Every registered entry: must be 0 (no E2024).
    for name in sorted(table):
        cases.append((name, "registered entry"))
    # Per-entry mutations: suffix + last-char truncation (positives for the
    # __mind_-prefixed ones; the truncation of a prefixed name stays prefixed
    # unless it collides with another registered entry).
    for name in sorted(table):
        if name.startswith(PREFIX):
            cases.append((name + "_x", "registered + suffix"))
            cases.append((name[:-1], "registered truncated by 1"))
    # Unregistered prefixed names.
    cases.append(("__mind_", "bare prefix, len 7"))
    cases.append(("__mind_bogus_thing", "unregistered prefixed"))
    cases.append(("__mind_alloc2", "registered + digit"))
    cases.append(
        (
            "__mind_blas_matmul_rmajor_f32_v_extended_long",
            "unregistered prefixed name, len > 32",
        )
    )
    cases.append(
        (
            "__mind_nerve_blas_matmul_score_q17_i64",
            "same first 32 bytes and length as registered long name, tail differs",
        )
    )
    # Non-prefixed controls (never E2024 regardless of resolvability).
    cases.append(("main", "plain fn name"))
    cases.append(("mind_alloc", "missing leading underscores"))
    cases.append(("_mind_alloc", "single leading underscore"))
    cases.append(("x__mind_", "prefix not at start"))
    cases.append(("__mind", "len 6, one short of the prefix"))
    cases.append(("byte", "registered non-prefixed entry"))
    return cases


def build_so():
    so = os.environ.get("MINDC_SO")
    if so:
        return so, False
    mindc = resolve_mindc()
    out = tempfile.NamedTemporaryFile(suffix=".so", delete=False).name
    cmd = [mindc, MAIN_MIND, "--emit-shared", out]
    print("BUILD:", " ".join(cmd), flush=True)
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        print("BUILD FAILED rc=", r.returncode)
        print(r.stdout[-4000:])
        print(r.stderr[-4000:])
        sys.exit(1)
    return out, True


def main():
    source = registry_source()
    table = parse_table_source(source)
    print(f"table: {len(table)} STD_SURFACE_INTRINSICS entries parsed from intrinsics.rs")
    so, built = build_so()
    st = os.stat(so)
    print(f"SO: {so} ({st.st_size} bytes)")
    if st.st_size < 4096:
        print("FAIL: .so too small (stub?)")
        sys.exit(1)
    lib = ctypes.CDLL(so)
    fn = lib.selftest_tc_self_host_only_call
    fn.argtypes = [ctypes.c_int64] * 5
    fn.restype = ctypes.c_int64
    span_fn = lib.selftest_tc_self_host_only_call_span
    span_fn.argtypes = [ctypes.c_int64, ctypes.c_int64]
    span_fn.restype = ctypes.c_int64
    # Keep the historical five-word ABI live while the span ABI handles the
    # complete identifier.  This is a symbol/behavior probe, not the oracle.
    if fn(*enc("__mind_alloc")) != 0:
        print("FAIL: legacy selftest_tc_self_host_only_call ABI drifted")
        sys.exit(1)
    registry_mutation_control(source, table, span_fn)

    mindc = resolve_mindc()
    cases = build_cases(table)
    total = fails = positives = negatives = 0
    with tempfile.TemporaryDirectory() as workdir:
        for name, note in cases:
            w = enc(name)
            name_bytes = name.encode()
            name_buf = ctypes.create_string_buffer(name_bytes, len(name_bytes))
            got = span_fn(ctypes.addressof(name_buf), len(name_bytes))
            exp = rust_rule(name, table)
            live = live_oracle(mindc, name, table, workdir)
            total += 1
            ok = check(
                got == exp == live,
                f"{name!r}: got={got} rule={exp} live={live} ({note})",
            )
            if exp == 1:
                positives += 1
            else:
                negatives += 1
            if not ok:
                fails += 1
            if not ok:
                print(
                    f"  DIFF got={got} rule={exp} live={live} len={w[4]:>2} "
                    f"{name!r} | {note}"
                )

    print(
        f"self_host_only_call: cases={total} positives={positives} "
        f"negatives={negatives} fails={fails}"
    )
    if positives < 1 or negatives < 1:
        print("FAIL: vacuous corpus")
        sys.exit(1)
    if fails:
        print("FAIL: pure-MIND E2024 core diverges from the Rust oracle")
        sys.exit(1)
    print("ALL PASS")
    if built:
        try:
            os.unlink(so)
        except OSError:
            pass


if __name__ == "__main__":
    main()
