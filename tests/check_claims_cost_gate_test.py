#!/usr/bin/env python3
"""Gate test: the published MIC cost figure must be DERIVED, not typed.

Background — the defect this test exists to prevent
---------------------------------------------------
README.md published "MIC saves $6,780/year per million IR operations vs JSON"
while the methodology it cited (benchmarks/BENCHMARK_RESULTS.md) yielded $396
from the same reference IR: a 17.1x gap that back-solved to a $0.030/1K token
price stated in NO file in the tree. Nothing in CI compared the two, so the
headline could drift from its own benchmark indefinitely.

The fix is arithmetic, not prose: `scripts/check_claims.py` recomputes the
annual saving from (a) the committed price input in `config/token_pricing.toml`
and (b) the tokenizer-measured token counts in the benchmark's machine-readable
output, then requires the resulting sentence to appear verbatim on every
declared surface. This test is the proof that the comparison is load-bearing —
it mutates each input in turn and requires the gate to go red.

Run:  python3 tests/check_claims_cost_gate_test.py
Exit: 0 = all cases pass, 1 = a case failed (prints the offending case).
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CHECK = REPO / "scripts" / "check_claims.py"
PRICING = REPO / "config" / "token_pricing.toml"

# A minimal surface tree: check_claims also runs the forbidden-phrase and
# canonical-IR checks, so the synthetic README must satisfy those to isolate
# the cost assertion. MODE=phrases skips the [counts] gate, which derives from
# a real source tree this fixture deliberately does not have.
README_TEMPLATE = """# Fixture surface

Canonical IR: mic@1 text, mic@3 binary.

| Format | Tokens | Annual (1M IRs) |
|--------|--------|-----------------|
{table}

**{claim}.**
"""


def cost_table(pricing: dict, results: dict) -> str:
    """The published cost TABLE, rendered from the benchmark output.

    The headline sentence is only half the surface: the table under it carries
    the same numbers and used to be hand-typed too.
    """
    cost = results["cost_model"]
    lines = []
    for display, label in pricing["claim"].get("table_labels", {}).items():
        lines.append(
            f"| {display} | {cost['per_format_tokens'][label]} "
            f"| ${cost['per_format_annual_usd'][label]:,.0f} |"
        )
    return "\n".join(lines)


def run_gate(root: Path) -> tuple[int, str]:
    env = dict(os.environ)
    env["CHECK_CLAIMS_ROOT"] = str(root)
    env["CHECK_CLAIMS_CAPS"] = str(REPO / "config" / "capabilities.toml")
    env["CHECK_CLAIMS_SURFACES"] = "README.md"
    env["CHECK_CLAIMS_MODE"] = "phrases"
    proc = subprocess.run(
        [sys.executable, str(CHECK)],
        env=env, capture_output=True, text=True, check=False,
    )
    return proc.returncode, proc.stdout + proc.stderr


def build_fixture(tmp: Path, *, claim: str, pricing_text: str, results: dict,
                  table: str | None = None) -> None:
    (tmp / "config").mkdir(parents=True, exist_ok=True)
    (tmp / "benchmarks").mkdir(parents=True, exist_ok=True)
    (tmp / "config" / "token_pricing.toml").write_text(pricing_text, encoding="utf-8")
    (tmp / "benchmarks" / "mic_map_benchmark_results.json").write_text(
        json.dumps(results, indent=2), encoding="utf-8"
    )
    pricing = tomllib.loads(pricing_text)
    if table is None:
        table = cost_table(pricing, results)
    body = README_TEMPLATE.format(claim=claim, table=table)
    # Every surface the config declares must carry the claim, so the fixture
    # mirrors the real repo's surface list rather than assuming README alone.
    surfaces = pricing["claim"].get("surfaces", ["README.md"])
    for rel in surfaces:
        path = tmp / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    (tmp / "README.md").write_text(body, encoding="utf-8")


def expected_claim(pricing: dict, results: dict) -> str:
    """Independent re-derivation of the sentence, written from the config spec.

    Deliberately NOT imported from check_claims.py: a test that reuses the
    implementation it checks proves only self-consistency.
    """
    m = pricing["measurement"]
    field = m["token_field"]
    rows = {r["label"]: r for r in results["measurements"]}
    base = rows[m["baseline_label"]][field]
    cand = rows[m["candidate_label"]][field]
    price = pricing["pricing"]["input_usd_per_1k_tokens"]
    volume = pricing["workload"]["ir_operations_per_year"]
    saving = (base - cand) * volume / 1000.0 * price
    return pricing["claim"]["template"].format(
        price_per_1k=f"{price:g}",
        price_as_of=pricing["pricing"]["as_of"],
        annual_savings=f"{saving:,.0f}",
        volume_human=pricing["workload"]["volume_human"],
    )


def main() -> int:
    if not PRICING.is_file():
        print(f"FAIL setup: {PRICING} does not exist — the price input must be committed")
        return 1
    pricing_text = PRICING.read_text(encoding="utf-8")
    pricing = tomllib.loads(pricing_text)
    results_path = REPO / pricing["measurement"]["results_json"]
    if not results_path.is_file():
        print(f"FAIL setup: {results_path} does not exist — benchmark output must be committed")
        return 1
    results = json.loads(results_path.read_text(encoding="utf-8"))
    good = expected_claim(pricing, results)

    failures: list[str] = []

    def case(name: str, *, claim: str, results_mut: dict, pricing_mut: str, want_ok: bool,
             table: str | None = None) -> None:
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            build_fixture(tmp, claim=claim, pricing_text=pricing_mut,
                          results=results_mut, table=table)
            rc, out = run_gate(tmp)
            ok = rc == 0
            if ok != want_ok:
                failures.append(
                    f"{name}: expected {'PASS' if want_ok else 'FAIL'}, got rc={rc}\n{out}"
                )
            elif not want_ok and "DRIFT [cost-claim]" not in out:
                # The tag must be the ARITHMETIC check's, not the forbidden-phrase
                # category's. capabilities.toml also lists the historical $6,780
                # headline, so an overlapping tag would let this assertion pass on
                # the phrase hit alone and the derived-figure check could be deleted
                # without a single case going red.
                failures.append(
                    f"{name}: failed for the wrong reason (no DRIFT [cost-claim])\n{out}"
                )
            # The shared verdict vocabulary (scripts/gate_assert.py counts a
            # PASS/FAIL line as one reported assertion). Printed `ok `/`BAD`
            # before, which counts as nothing: this file ran nine mutation
            # cases and published `asserted=0`, so an emptied case list would
            # still have exited 0 under the gate runner.
            print(f"  [{'PASS' if ok == want_ok else 'FAIL'}] {name} (rc={rc})")

    # 1. Agreement: the derived sentence on the surface passes.
    case("agreement", claim=good, results_mut=results, pricing_mut=pricing_text, want_ok=True)

    # 2. The historical defect: a hand-typed headline the benchmark never produced.
    case(
        "stale_headline_6780",
        claim="MIC saves $6,780/year per million IR operations vs JSON",
        results_mut=results, pricing_mut=pricing_text, want_ok=False,
    )

    # 3. Benchmark output moves, README does not.
    moved = json.loads(json.dumps(results))
    for row in moved["measurements"]:
        if row["label"] == pricing["measurement"]["candidate_label"]:
            row[pricing["measurement"]["token_field"]] += 40
    case("benchmark_moved", claim=good, results_mut=moved, pricing_mut=pricing_text, want_ok=False)

    # 4. Price input moves, README does not.
    repriced = pricing_text.replace(
        f"input_usd_per_1k_tokens = {pricing['pricing']['input_usd_per_1k_tokens']}",
        "input_usd_per_1k_tokens = 0.03",
    )
    if repriced == pricing_text:
        failures.append("repricing mutation did not apply — check the config key spelling")
    case("price_moved", claim=good, results_mut=results, pricing_mut=repriced, want_ok=False)

    # 5. Fail closed when the tokenizer view is missing: a chars/4 estimate must
    #    never silently back a dollar figure.
    untokenized = json.loads(json.dumps(results))
    for row in untokenized["measurements"]:
        row[pricing["measurement"]["token_field"]] = None
    case("tokenizer_missing", claim=good, results_mut=untokenized, pricing_mut=pricing_text, want_ok=False)

    # 6. A hand-edited cost block in the benchmark output cannot override the
    #    gate's own arithmetic.
    forged = json.loads(json.dumps(results))
    forged.setdefault("cost_model", {})["annual_savings_usd"] = 6780.0
    case("forged_cost_block", claim=good, results_mut=forged, pricing_mut=pricing_text, want_ok=False)

    # 7. The table under the headline is derived too: a row hand-edited back to the
    #    old chars/4 estimate must go red even though the sentence still agrees.
    stale_table = "\n".join(
        # 67 tokens / $117 are the pre-fix chars/4 numbers for this row.
        "| TOON | 67 | $117 |" if line.startswith("| TOON ") else line
        for line in cost_table(pricing, results).splitlines()
    )
    case("stale_table_row", claim=good, results_mut=results, pricing_mut=pricing_text,
         want_ok=False, table=stale_table)

    # 8. The ONE silent path, asserted so it stays narrow: a sibling repo with no
    #    config/token_pricing.toml publishes no cost claim, so there is nothing to
    #    verify and the gate must not invent a failure. Every other missing input
    #    above fails closed.
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        build_fixture(tmp, claim=good, pricing_text=pricing_text, results=results)
        (tmp / "config" / "token_pricing.toml").unlink()
        rc, out = run_gate(tmp)
        if rc != 0 or "no cost claim to verify" not in out:
            failures.append(f"sibling_without_config: expected a clean skip, got rc={rc}\n{out}")
        print(f"  [{'PASS' if rc == 0 else 'FAIL'}] sibling_without_config (rc={rc})")

    # 9. The real repo tree must satisfy the gate end to end.
    rc = subprocess.run([sys.executable, str(CHECK)], cwd=REPO,
                        capture_output=True, text=True, check=False)
    if rc.returncode != 0:
        failures.append(f"repo_tree: check_claims.py failed on the real tree\n{rc.stdout}{rc.stderr}")
    print(f"  [{'PASS' if rc.returncode == 0 else 'FAIL'}] repo_tree (rc={rc.returncode})")

    if failures:
        print("\ncheck_claims cost gate: FAILED")
        for f in failures:
            print("-" * 70)
            print(f)
        return 1
    print("\ncheck_claims cost gate: OK — the published figure is derived, not typed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
