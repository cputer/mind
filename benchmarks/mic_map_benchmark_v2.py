"""
MIC/MAP Patent Reference Benchmark — Reproduces Paper's Canonical Numbers
==========================================================================

Reproduces the measurement numbers recited in the provisional patent
application "BPE-Optimized Serialization Formats and Secure Interaction
Protocol for Generative AI Compiler Integration" (STARGA Inc.,
STARGA Inc.).

Uses the ORIGINAL methodology documented in mind/benchmarks/mic_benchmark.py:
  * 6-node MLP reference IR (4 symbols, 4 types, 6 nodes, 1 output)
  * JSON baseline = json.dumps(..., indent=2) (pretty-printed, verbose keys)
  * Token count = len(text) // 4 (standard GPT-style heuristic)

Reproduces the paper's Table I.3 / APPENDIX C numbers:
  * JSON:  1133 bytes, 278 tokens
  * MIC v1: 209 bytes, 52 tokens  (5.3x token reduction, 81%)

Also reports REAL cl100k_base tiktoken results as a secondary view for
cross-verification at non-provisional conversion.

Usage:
    pip install tiktoken>=0.7.0
    python3 mic_map_benchmark.py

Copyright (c) 2025-2026 STARGA, Inc. All rights reserved.
"""

from __future__ import annotations

import hashlib
import json
import sys
import tomllib
from pathlib import Path
from typing import Any

try:
    import tiktoken
    HAS_TIKTOKEN = True
except ImportError:
    HAS_TIKTOKEN = False
    print(
        "error: tiktoken is not installed — the published cost figure is derived from\n"
        "       real cl100k_base token counts, so this benchmark fails closed rather\n"
        "       than fall back to the chars/4 estimate.  pip install 'tiktoken>=0.7.0'",
        file=sys.stderr,
    )


# =================================================================== #
#  Reference IR — matches benchmarks/mic_benchmark.py                    #
# =================================================================== #

SAMPLE_IR_DICT: dict[str, Any] = {
    "version": 1,
    "symbols": ["input", "weight", "bias", "output"],
    "types": [
        {"id": 0, "dtype": "f32", "shape": [None, 784]},
        {"id": 1, "dtype": "f32", "shape": [784, 256]},
        {"id": 2, "dtype": "f32", "shape": [256]},
        {"id": 3, "dtype": "f32", "shape": [None, 256]},
    ],
    "nodes": [
        {"id": 0, "op": "param",  "symbol": 0, "type": 0},
        {"id": 1, "op": "param",  "symbol": 1, "type": 1},
        {"id": 2, "op": "param",  "symbol": 2, "type": 2},
        {"id": 3, "op": "matmul", "inputs": [0, 1], "type": 3},
        {"id": 4, "op": "add",    "inputs": [3, 2], "type": 3},
        {"id": 5, "op": "relu",   "inputs": [4],    "type": 3},
    ],
    "outputs": [5],
}

SAMPLE_MIC_V1_TEXT: str = """mic@1
S0 "input"
S1 "weight"
S2 "bias"
S3 "output"
T0 [f32;B,784]
T1 [f32;784,256]
T2 [f32;256]
T3 [f32;B,256]
N0 param S0 T0
N1 param S1 T1
N2 param S2 T2
N3 matmul N0 N1 T3
N4 add N3 N2 T3
N5 relu N4 T3
O N5
"""

SAMPLE_MIC_V2_TEXT: str = """mic@2
T0 f32 B 784
T1 f32 784 256
T2 f32 256
T3 f32 B 256
p input T0
p weight T1
p bias T2
m 0 1
+ 4 2
r 5
O 6
"""


# =================================================================== #
#  Rival text encodings of the SAME reference IR                         #
# =================================================================== #
#
# Same single-source rule as the protocol payloads below: format_benchmark.py
# imports these instead of holding a second copy.

SAMPLE_TOON: str = """version: 1
symbols[4]: input,weight,bias,output
outputs[1]: 5
types[4]{id,dtype,shape}:
  0,f32,B:784
  1,f32,784:256
  2,f32,256
  3,f32,B:256
nodes[6]{id,op,inputs,type_id}:
  0,param,S0,0
  1,param,S1,1
  2,param,S2,2
  3,matmul,N0:N1,3
  4,add,N3:N2,3
  5,relu,N4,3"""

SAMPLE_TOML: str = """version = 1
symbols = ["input", "weight", "bias", "output"]
outputs = [5]

[[types]]
id = 0
dtype = "f32"
shape = ["B", 784]

[[types]]
id = 1
dtype = "f32"
shape = [784, 256]

[[types]]
id = 2
dtype = "f32"
shape = [256]

[[types]]
id = 3
dtype = "f32"
shape = ["B", 256]

[[nodes]]
id = 0
op = "param"
symbol = 0
type_id = 0

[[nodes]]
id = 1
op = "param"
symbol = 1
type_id = 1

[[nodes]]
id = 2
op = "param"
symbol = 2
type_id = 2

[[nodes]]
id = 3
op = "matmul"
inputs = [0, 1]
type_id = 3

[[nodes]]
id = 4
op = "add"
inputs = [3, 2]
type_id = 3

[[nodes]]
id = 5
op = "relu"
inputs = [4]
type_id = 3"""


# =================================================================== #
#  MAP vs JSON-RPC reference session                                     #
# =================================================================== #
#
# SINGLE SOURCE for the protocol payloads: benchmarks/format_benchmark.py
# imports these rather than keeping its own copy, so the published MAP token
# count cannot come from a payload that has drifted from the one measured here.

MAP_SESSION: str = """@1 hello mic=1 map=1
=1 ok version=1.0 features=[patch,check,dump]
@2 load <<EOF
mic@1
T0 f32
N0 const.f32 1.0 T0
N1 const.f32 2.0 T0
N2 add N0 N1 T0
O N2
EOF
=2 ok nodes=3
@3 check
=3 ok valid=true
@4 dump
=4 ok mic@1...
@5 bye
=5 ok"""

JSON_RPC_SESSION: str = """{
  "jsonrpc": "2.0",
  "method": "hello",
  "params": {"mic_version": 1, "map_version": 1},
  "id": 1
}
{
  "jsonrpc": "2.0",
  "result": {"version": "1.0", "features": ["patch", "check", "dump"]},
  "id": 1
}
{
  "jsonrpc": "2.0",
  "method": "load",
  "params": {
    "module": {
      "version": 1,
      "types": [{"id": 0, "dtype": "f32"}],
      "nodes": [
        {"id": 0, "op": "const.f32", "value": 1.0, "type": 0},
        {"id": 1, "op": "const.f32", "value": 2.0, "type": 0},
        {"id": 2, "op": "add", "inputs": [0, 1], "type": 0}
      ],
      "outputs": [2]
    }
  },
  "id": 2
}
{
  "jsonrpc": "2.0",
  "result": {"nodes": 3},
  "id": 2
}
{
  "jsonrpc": "2.0",
  "method": "check",
  "id": 3
}
{
  "jsonrpc": "2.0",
  "result": {"valid": true},
  "id": 3
}
{
  "jsonrpc": "2.0",
  "method": "dump",
  "id": 4
}
{
  "jsonrpc": "2.0",
  "result": {"module": "..."},
  "id": 4
}
{
  "jsonrpc": "2.0",
  "method": "bye",
  "id": 5
}
{
  "jsonrpc": "2.0",
  "result": "ok",
  "id": 5
}"""


# =================================================================== #
#  Token counting — two methodologies                                    #
# =================================================================== #

def tokens_approximate(text: str) -> int:
    """Original GPT-style approximation used in mic_benchmark.py (4 chars/token)."""
    return len(text) // 4


def tokens_tiktoken(text: str, encoder) -> int:
    """Real BPE tokenization with cl100k_base encoder."""
    return len(encoder.encode(text))


# =================================================================== #
#  MIC-B binary (new in MIC v2/binary spec)                              #
# =================================================================== #
#
# deferred: this is a REFERENCE-MODEL binary encoder for the 6-node sample IR,
# not the shipping canonical `mic@3` encoder (libmind::ir::compact::emit_mic3) --
# the published 90 B / 12.4x row is therefore labelled as a reference-model
# measurement on every surface, never as an emit_mic3 output.
# upgrade path: compile a fixed .mind fixture with `mindc <fixture> --emit-mic3 out.mic3`,
# publish `wc -c out.mic3` as the canonical number, and keep this model encoder only
# as the like-for-like counterpart to the JSON row of the SAME sample IR.

def _uleb128(v: int) -> bytes:
    out = bytearray()
    while True:
        b = v & 0x7F
        v >>= 7
        if v:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


DTYPE_BYTE = {
    "f16": 0, "f32": 1, "f64": 2, "bf16": 3, "i8": 4, "i16": 5,
    "i32": 6, "i64": 7, "u8": 8, "u16": 9, "u32": 10, "u64": 11, "bool": 12,
}
OPCODE_BYTE = {
    "m": 0, "+": 1, "-": 2, "*": 3, "/": 4, "r": 5, "s": 6, "sig": 7,
    "th": 8, "gelu": 9, "ln": 10, "t": 11, "rshp": 12, "sum": 13,
    "mean": 14, "max": 15, "cat": 16, "split": 17, "gth": 18,
}


def serialize_mic_b(ir: dict[str, Any]) -> bytes:
    """MIC-B binary encoding per patent APPENDIX I §I.5."""
    strs: list[str] = []
    idx: dict[str, int] = {}

    def intern(s: str) -> int:
        if s not in idx:
            idx[s] = len(strs)
            strs.append(s)
        return idx[s]

    # String table order: symbols, dims, value names
    for s in ir["symbols"]:
        intern(s)
    for t in ir["types"]:
        for d in t["shape"]:
            intern(str(d))
    # Custom names for nodes not in this IR.

    out = bytearray()
    out.extend(b"MICB")
    out.append(0x02)
    out.extend(_uleb128(len(strs)))
    for s in strs:
        b = s.encode("utf-8")
        out.extend(_uleb128(len(b)))
        out.extend(b)
    # Symbol table
    out.extend(_uleb128(len(ir["symbols"])))
    for s in ir["symbols"]:
        out.extend(_uleb128(idx[s]))
    # Type table
    out.extend(_uleb128(len(ir["types"])))
    for t in ir["types"]:
        out.append(DTYPE_BYTE[t["dtype"]])
        out.extend(_uleb128(len(t["shape"])))
        for d in t["shape"]:
            out.extend(_uleb128(idx[str(d)]))
    # Value table (arg+param+node in declaration order)
    out.extend(_uleb128(len(ir["nodes"])))
    opmap = {"param": None, "matmul": "m", "add": "+", "relu": "r"}
    for n in ir["nodes"]:
        if n["op"] == "param":
            out.append(0x01)  # Param tag
            out.extend(_uleb128(idx[ir["symbols"][n["symbol"]]]))
            out.extend(_uleb128(n["type"]))
        else:
            out.append(0x02)  # Node tag
            op = opmap[n["op"]]
            out.append(OPCODE_BYTE[op])
            out.extend(_uleb128(len(n["inputs"])))
            for i in n["inputs"]:
                out.extend(_uleb128(i))
    # Output
    out.extend(_uleb128(ir["outputs"][0]))
    return bytes(out)


# =================================================================== #
#  Main                                                                 #
# =================================================================== #

# =================================================================== #
#  Cost model — derived, never typed                                     #
# =================================================================== #
#
# The published headline ("MIC saves $N/year per million IR operations vs
# JSON") is computed HERE from two committed inputs and written into the
# machine-readable report, so scripts/check_claims.py can re-derive it and fail
# the build if any surface disagrees. It once read $6,780 while the cited
# methodology gave $396 — a 17.1x gap resting on a price stated in no file.
#
# Two rules make that unrepeatable:
#   1. Token counts come from the REAL tokenizer column. The chars/4 column is
#      an estimate and is never allowed to back a dollar figure.
#   2. The price is an input in config/token_pricing.toml with its source and
#      date, and the claim sentence restates both inline.

PRICING_PATH = Path(__file__).resolve().parent.parent / "config" / "token_pricing.toml"


def load_pricing() -> dict[str, Any]:
    return tomllib.loads(PRICING_PATH.read_text(encoding="utf-8"))


def build_cost_model(pricing: dict[str, Any], rows: list[dict[str, Any]]) -> dict[str, Any]:
    """Derive the annual cost/saving figures from the tokenizer-measured counts.

    Raises RuntimeError if the real-tokenizer column is absent: a cost claim
    backed by a chars/4 heuristic is exactly the defect this replaces.
    """
    meas = pricing["measurement"]
    field = meas["token_field"]
    by_label = {r["label"]: r for r in rows}
    counts: dict[str, int] = {}
    for role in ("baseline_label", "candidate_label"):
        label = meas[role]
        value = by_label[label].get(field)
        if not isinstance(value, int):
            raise RuntimeError(
                f"{label}: no {field} count — install the tokenizer "
                f"({meas['tokenizer_library']}, encoding {meas['tokenizer']}) and re-run. "
                "The chars/4 estimate must never back a price."
            )
        counts[role] = value

    price = float(pricing["pricing"]["input_usd_per_1k_tokens"])
    volume = int(pricing["workload"]["ir_operations_per_year"])
    base_tokens, cand_tokens = counts["baseline_label"], counts["candidate_label"]
    base_cost = base_tokens * volume / 1000.0 * price
    cand_cost = cand_tokens * volume / 1000.0 * price
    saving = base_cost - cand_cost

    sentence = pricing["claim"]["template"].format(
        price_per_1k=f"{price:g}",
        price_as_of=pricing["pricing"]["as_of"],
        annual_savings=f"{saving:,.0f}",
        volume_human=pricing["workload"]["volume_human"],
    )
    return {
        "pricing_config": "config/token_pricing.toml",
        "price_usd_per_1k_input_tokens": price,
        "price_as_of": pricing["pricing"]["as_of"],
        "price_source_kind": pricing["pricing"]["source_kind"],
        "ir_operations_per_year": volume,
        "tokenizer": meas["tokenizer"],
        "token_field": field,
        "baseline_label": meas["baseline_label"],
        "candidate_label": meas["candidate_label"],
        "baseline_tokens_per_ir": base_tokens,
        "candidate_tokens_per_ir": cand_tokens,
        "token_reduction_ratio": round(base_tokens / cand_tokens, 4),
        "baseline_annual_usd": round(base_cost, 2),
        "candidate_annual_usd": round(cand_cost, 2),
        "annual_savings_usd": round(saving, 2),
        # Every text encoding of the same reference IR at the same price, so the
        # published comparison table is derived rather than transcribed.
        "per_format_annual_usd": {
            r["label"]: round(r[field] * volume / 1000.0 * price, 2)
            for r in rows
            if isinstance(r.get(field), int)
        },
        "per_format_tokens": {
            r["label"]: r[field] for r in rows if isinstance(r.get(field), int)
        },
        "claim_sentence": sentence,
    }


def main() -> int:
    print("MIC/MAP Patent Benchmark — reproducing paper's canonical numbers\n")

    # Paper's baseline: JSON with indent=2 (pretty-printed)
    json_pretty = json.dumps(SAMPLE_IR_DICT, indent=2)
    json_mini   = json.dumps(SAMPLE_IR_DICT, separators=(",", ":"))
    mic_v1      = SAMPLE_MIC_V1_TEXT
    mic_v2      = SAMPLE_MIC_V2_TEXT
    mic_b_bytes = serialize_mic_b(SAMPLE_IR_DICT)

    encoder = tiktoken.get_encoding("cl100k_base") if HAS_TIKTOKEN else None

    rows: list[dict[str, Any]] = []
    for label, payload, is_binary in [
        ("json_indent2_pretty", json_pretty, False),
        ("json_minified",       json_mini,   False),
        ("toml",                SAMPLE_TOML, False),
        ("toon",                SAMPLE_TOON, False),
        ("mic_v1",              mic_v1,      False),
        ("mic_v2",              mic_v2,      False),
        ("mic_b_binary",        mic_b_bytes, True),
        ("jsonrpc_session",     JSON_RPC_SESSION, False),
        ("map_session",         MAP_SESSION,      False),
    ]:
        b = payload if is_binary else payload.encode("utf-8")
        row = {
            "label":            label,
            "bytes":            len(b),
            "tokens_approx":    tokens_approximate(payload) if not is_binary else None,
            "tokens_tiktoken":  tokens_tiktoken(payload, encoder) if (not is_binary and encoder) else None,
            "sha256":           hashlib.sha256(b).hexdigest(),
            "first_bytes_hex":  b[:48].hex(),
            "is_binary":        is_binary,
        }
        rows.append(row)

    print("=" * 86)
    print("METHODOLOGY 1: Paper's original — JSON indent=2 baseline + len(text)//4 tokens")
    print("=" * 86)
    print(f"\n  {'FORMAT':<24}{'BYTES':>8}{'TOKENS (~len/4)':>18}{'vs JSON':>12}{'REDUCTION':>14}")
    j = next(r for r in rows if r["label"] == "json_indent2_pretty")
    # Protocol-session rows are a different comparison (MAP vs JSON-RPC) and are
    # reported separately below; ratios against the IR baseline would be noise.
    ir_rows = [r for r in rows if not r["label"].endswith("_session")]
    for r in ir_rows:
        if r["tokens_approx"] is None:
            bytes_ratio = j["bytes"] / r["bytes"]
            byte_reduction = (1 - r["bytes"] / j["bytes"]) * 100
            print(f"  {r['label']:<24}{r['bytes']:>8}{'  (binary)':>18}"
                  f"{bytes_ratio:>10.2f}x {f'{byte_reduction:.0f}% bytes':>14}")
        else:
            ratio = j["tokens_approx"] / r["tokens_approx"] if r["tokens_approx"] else 0
            reduction = (1 - r["tokens_approx"] / j["tokens_approx"]) * 100 if j["tokens_approx"] else 0
            print(f"  {r['label']:<24}{r['bytes']:>8}{r['tokens_approx']:>18}"
                  f"{ratio:>10.2f}x {f'{reduction:.0f}%':>14}")

    if encoder is not None:
        print("\n" + "=" * 86)
        print("METHODOLOGY 2: Real cl100k_base tokenization (secondary view)")
        print("=" * 86)
        print(f"\n  {'FORMAT':<24}{'BYTES':>8}{'TOKENS (tiktoken)':>20}{'vs JSON':>12}")
        for r in ir_rows:
            if r["tokens_tiktoken"] is None:
                print(f"  {r['label']:<24}{r['bytes']:>8}{'  (binary)':>20}"
                      f"{j['bytes']/r['bytes']:>10.2f}x")
            else:
                ratio = j["tokens_tiktoken"] / r["tokens_tiktoken"] if r["tokens_tiktoken"] else 0
                print(f"  {r['label']:<24}{r['bytes']:>8}{r['tokens_tiktoken']:>20}"
                      f"{ratio:>10.2f}x")

    # Paper claim verification
    mic1 = next(r for r in rows if r["label"] == "mic_v1")
    mic2 = next(r for r in rows if r["label"] == "mic_v2")
    micb = next(r for r in rows if r["label"] == "mic_b_binary")
    ratio_mic1_paper = j["tokens_approx"] / mic1["tokens_approx"]
    ratio_mic2_paper = j["tokens_approx"] / mic2["tokens_approx"] if mic2["tokens_approx"] else 0
    ratio_micb_bytes = j["bytes"] / micb["bytes"]

    print("\n" + "=" * 86)
    print("CLAIM THRESHOLD VERIFICATION (paper methodology)")
    print("=" * 86)
    checks = [
        ("Paper FIG 1 claim: JSON = 278 tokens (±5%)",
         abs(j["tokens_approx"] - 278) <= 14,
         f"measured: {j['tokens_approx']}"),
        ("Paper FIG 1 claim: MIC v1 = 52 tokens (±5%)",
         abs(mic1["tokens_approx"] - 52) <= 3,
         f"measured: {mic1['tokens_approx']}"),
        ("Paper FIG 1 claim: MIC v1 achieves ~5.3x token reduction",
         abs(ratio_mic1_paper - 5.3) <= 0.5,
         f"measured: {ratio_mic1_paper:.2f}x"),
        ("Claim 25: ≥80% token reduction (MIC v1 vs JSON)",
         (1 - mic1["tokens_approx"] / j["tokens_approx"]) >= 0.80,
         f"measured: {(1 - mic1['tokens_approx']/j['tokens_approx'])*100:.1f}%"),
        ("Claim 1B: MIC v1 ≥4.0x fewer tokens than JSON",
         ratio_mic1_paper >= 4.0,
         f"measured: {ratio_mic1_paper:.2f}x"),
        ("Claim 66A: MIC v2 ≥4.0x fewer tokens than JSON",
         ratio_mic2_paper >= 4.0,
         f"measured: {ratio_mic2_paper:.2f}x"),
        ("Claim 87: MIC-B ≥5.0x fewer bytes than JSON",
         ratio_micb_bytes >= 5.0,
         f"measured: {ratio_micb_bytes:.2f}x"),
    ]
    all_pass = all(c[1] for c in checks)
    for desc, ok, detail in checks:
        mark = "✓ PASS" if ok else "✗ FAIL"
        print(f"  {mark}  {desc}")
        print(f"           ({detail})")
    print(f"\n  OVERALL: {'ALL CLAIMS VERIFIED ✓' if all_pass else 'SOME CLAIMS FAILED ✗'}")

    # Cost model — the published headline, derived from committed inputs.
    pricing = load_pricing()
    try:
        cost = build_cost_model(pricing, rows)
    except RuntimeError as err:
        # Fail closed: no tokenizer, no published figure. Emitting the size
        # tables while silently dropping the cost model is how a chars/4
        # estimate got published as a measured dollar amount in the first place.
        print(f"\nerror: cannot derive the cost model: {err}", file=sys.stderr)
        return 1
    print("\n" + "=" * 86)
    print("COST MODEL (real-tokenizer counts x committed price input)")
    print("=" * 86)
    print(f"  price:      ${cost['price_usd_per_1k_input_tokens']:g}/1K input tokens "
          f"(as of {cost['price_as_of']}, {cost['price_source_kind']})")
    print(f"  workload:   {cost['ir_operations_per_year']:,} IR operations/year, "
          f"one {cost['baseline_label']} document each")
    print(f"  tokenizer:  {cost['tokenizer']} ({cost['token_field']})")
    print(f"  {cost['baseline_label']:<24}{cost['baseline_tokens_per_ir']:>6} tok/IR"
          f"   ${cost['baseline_annual_usd']:>12,.2f}/year")
    print(f"  {cost['candidate_label']:<24}{cost['candidate_tokens_per_ir']:>6} tok/IR"
          f"   ${cost['candidate_annual_usd']:>12,.2f}/year")
    print(f"\n  PUBLISHED CLAIM: {cost['claim_sentence']}")
    print("  (scripts/check_claims.py fails the build if a surface disagrees)")

    # Write machine-readable report
    report = {
        "metadata": {
            "script":            "mic_map_benchmark.py",
            "version":           "2.0",
            "patent_reference":  "STARGA Inc., BPE-Optimized Serialization Formats and Secure Interaction Protocol",
            "reference_ir":      "6-node MLP layer (param, matmul, add, relu)",
            "methodology_1":     "JSON indent=2 baseline + len(text)//4 token approximation (paper original)",
            "methodology_2":     "Real cl100k_base tokenization (cross-verification)",
            "tiktoken_version":  tiktoken.__version__ if HAS_TIKTOKEN else None,
            "python_version":    sys.version.split()[0],
        },
        "measurements":        rows,
        "paper_figures_verified": {
            "JSON_278_tokens":    j["tokens_approx"],
            "MIC_v1_52_tokens":   mic1["tokens_approx"],
            "MIC_v1_5.3x_ratio":  round(ratio_mic1_paper, 2),
        },
        "claim_checks": [
            {"claim": d, "pass": ok, "detail": det} for d, ok, det in checks
        ],
        "cost_model": cost,
        "all_claims_verified": all_pass,
    }
    out_path = Path(__file__).parent / "mic_map_benchmark_results.json"
    out_path.write_text(json.dumps(report, indent=2, ensure_ascii=False))
    print(f"\nMachine-readable report: {out_path}")
    return 0 if all_pass else 1


if __name__ == "__main__":
    sys.exit(main())
