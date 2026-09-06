"""
Self-host mic@3 and native-ELF gate for typed scalar cast expressions.

The bootstrap parser retains each target spelling on ast_cast. Its note path
must emit the same source-kind-dependent `__mind_conv_TARGET` call as the Rust
lowerer; an integer-equivalent shift/mask is not canonical MIC@3. Native output
must still materialise signed and unsigned narrowing with the correct extension.

Two gates per fixture:
  (1) EXECUTION-CORRECTNESS — compile with the live Rust `mindc build --emit=binary`
      and RUN it; exit code must be the truncated-toward-zero cast result (mod 256).
  (2) mic@3 NOTE BYTE-IDENTITY — `selftest_mic3_module_nfn(<fn src>)` must equal
      `mindc --emit-mic3 <fn src>` byte-for-byte.

Run:  MINDC_SO=<.so> MINDC_BIN=./target/release/mindc \
      python3.12 examples/mindc_mind/self_host_cast_mic3_smoke.py
"""

import ctypes
import os
import pathlib
import subprocess
import sys
import tempfile

_HERE = pathlib.Path(__file__).parent.resolve()
sys.path.insert(0, str(_HERE))
from _selfhost_so import resolve_so  # noqa: E402

SO = resolve_so()
MINDC = pathlib.Path(
    os.environ.get("MINDC_BIN", str(_HERE.parents[1] / "target" / "release" / "mindc"))
)

_P = ctypes.POINTER(ctypes.c_int64)


def _i64(addr, off=0):
    return int(ctypes.cast(addr + off, _P)[0])


def _read_string_record(handle):
    if not handle:
        return b""
    addr, length = _i64(handle, 0), _i64(handle, 8)
    if not addr or not length:
        return b""
    p = ctypes.cast(addr, ctypes.POINTER(ctypes.c_int8))
    return bytes(int(p[i]) & 0xFF for i in range(length))


def oracle_mic3(src):
    with tempfile.TemporaryDirectory() as td:
        sp = pathlib.Path(td) / "m.mind"
        op = pathlib.Path(td) / "m.mic3"
        sp.write_text(src)
        subprocess.run([str(MINDC), "--emit-mic3", str(op), str(sp)], capture_output=True)
        return op.read_bytes() if op.exists() else None


def nfn_mic3(src):
    lib = ctypes.CDLL(str(SO))
    fn = lib.selftest_mic3_module_nfn
    fn.restype = ctypes.c_void_p
    fn.argtypes = [ctypes.c_int64] * 5
    sb = src.encode()
    sc = ctypes.create_string_buffer(sb, len(sb))
    strbuf = ctypes.create_string_buffer(1 << 18)
    offs = (ctypes.c_int64 * 8192)()
    cc = (ctypes.c_int64 * 1)()
    es = fn(
        ctypes.cast(sc, ctypes.c_void_p).value,
        len(sb),
        ctypes.cast(strbuf, ctypes.c_void_p).value,
        ctypes.cast(offs, ctypes.c_void_p).value,
        ctypes.cast(cc, ctypes.c_void_p).value,
    )
    return _read_string_record(_i64(es, 0)) if es else b""


import stat

# stdlib blob in the FIXED self-host module order — the native-ELF entry expects
# `combined = std_blob ++ user_src` with user_lo = len(std_blob) (mind-runtime NOT
# required: selftest_native_elf_u emits a standalone ELF, unlike `mindc build`).
STD_MODULES = [
    "arena", "async", "blas", "cli", "fs", "io", "io_canon", "iouring", "json", "map",
    "net", "process", "reactor", "regex", "ring", "sha256", "string", "time", "toml",
    "tui", "vec",
]
_REPO = _HERE.parents[1]


def std_blob() -> bytes:
    return b"\n".join((_REPO / "std" / f"{m}.mind").read_bytes() for m in STD_MODULES) + b"\n"


_LIB = ctypes.CDLL(str(SO)) if SO.exists() else None
if _LIB is not None:
    _LIB.selftest_native_elf_u.restype = ctypes.c_int64
    _LIB.selftest_native_elf_u.argtypes = [ctypes.c_int64, ctypes.c_int64, ctypes.c_int64]


def run_native(prog: str, run_timeout: int = 8):
    """Emit a standalone native ELF via the pure-MIND selftest_native_elf_u (zero
    mind-runtime, zero MLIR) over std_blob++prog, run it, return the exit code.
    None on fail-closed (empty ELF)."""
    combined = std_blob() + prog.encode()
    user_lo = len(std_blob())
    sb = ctypes.create_string_buffer(combined, len(combined))
    es = _LIB.selftest_native_elf_u(
        ctypes.cast(sb, ctypes.c_void_p).value, len(combined), user_lo
    )
    if not es:
        return None
    sh = _i64(es, 0)
    n = _i64(sh, 8)
    elf = ctypes.string_at(_i64(sh, 0), n) if n > 0 else b""
    if len(elf) < 4 or elf[:4] != b"\x7fELF":
        return None
    with tempfile.NamedTemporaryFile(suffix=".elf", delete=False) as f:
        f.write(elf)
        p = f.name
    os.chmod(p, 0o755)
    try:
        return subprocess.run([p], timeout=run_timeout).returncode
    except subprocess.TimeoutExpired:
        return "HANG"
    finally:
        os.unlink(p)


# (name, fn_src, expected_exit mod 256).
FIXTURES = [
    (
        "int_literal_as_i64",
        "fn compute() -> i64 { return 5 as i64; }\n",
        5,
    ),
    (
        "f64_pos_trunc_as_i64",
        "fn compute() -> i64 { let a: f64 = 2.75; return a as i64; }\n",
        2,
    ),
    (
        "f64_neg_trunc_as_i64",
        "fn compute() -> i64 { let a: f64 = 0.0 - 2.75; return a as i64; }\n",
        254,  # -2 truncated toward zero, mod 256
    ),
    ("unary_neg_as_i64", "fn compute() -> i64 { return -(5 as i64); }\n", 251),
    ("signed_i8_wrap", "fn compute() -> i64 { return 300 as i8; }\n", 44),
    ("signed_i16_wrap", "fn compute() -> i64 { return 70000 as i16; }\n", 112),
    ("signed_i32_wrap", "fn compute() -> i64 { return 4294967295 as i32; }\n", 255),
    ("unsigned_u8_wrap", "fn compute() -> i64 { return (0 - 1) as u8; }\n", 255),
    ("unsigned_u8_narrow_return", "fn compute() -> u8 { return 300 as u8; }\n", 44),
    ("unsigned_u16_wrap", "fn compute() -> i64 { return 70000 as u16; }\n", 112),
    ("unsigned_u32_wrap", "fn compute() -> i64 { return 4294967297 as u32; }\n", 1),
]


def _prog(fn_src):
    return fn_src + "fn main() -> i64 { return compute(); }\n"


# These callers inspect bits above the process exit byte. A direct `main -> u8`
# exit cannot distinguish raw 300 from canonical u8 44 because both exit 44.
# Division makes a missing callee-side mask/sign extension observable.
NARROW_RETURN_FIXTURES = [
    (
        "explicit_raw_u8_return",
        "fn compute() -> u8 { return 300; }\n",
        "fn main() -> i64 { return compute() / 256; }\n",
        0,
    ),
    (
        "implicit_raw_u8_return",
        "fn compute() -> u8 { 300 }\n",
        "fn main() -> i64 { return compute() / 256; }\n",
        0,
    ),
    (
        "explicit_raw_u16_return",
        "fn compute() -> u16 { return 70000; }\n",
        "fn main() -> i64 { return compute() / 65536; }\n",
        0,
    ),
    (
        "explicit_raw_u32_return",
        "fn compute() -> u32 { return 4294967297; }\n",
        "fn main() -> i64 { return compute() / 4294967296; }\n",
        0,
    ),
    (
        "explicit_value_if_u8_return",
        "fn compute(x: i64) -> u8 { return if x == 0 { 300 } else { 1 }; }\n",
        "fn main() -> i64 { return compute(0) / 256; }\n",
        0,
    ),
    (
        "implicit_value_if_u8_return",
        "fn compute(x: i64) -> u8 { if x == 0 { 300 } else { 1 } }\n",
        "fn main() -> i64 { return compute(0) / 256; }\n",
        0,
    ),
    (
        "both_branches_return_u8",
        "fn compute(x: i64) -> u8 { if x == 0 { return 300; } else { return 1; } }\n",
        "fn main() -> i64 { return compute(0) / 256; }\n",
        0,
    ),
    (
        "implicit_value_if_i8_return",
        "fn compute(x: i64) -> i8 { if x == 0 { 200 } else { 1 } }\n",
        "fn main() -> i64 { return compute(0) / 100; }\n",
        0,
    ),
    (
        "both_branches_return_i8",
        "fn compute(x: i64) -> i8 { if x == 0 { return 200; } else { return 1; } }\n",
        "fn main() -> i64 { return compute(0) / 100; }\n",
        0,
    ),
    (
        "line_comment_after_u8_return",
        "fn compute() -> u8 // return ABI comment\n{ 300 }\n",
        "fn main() -> i64 { return compute() / 256; }\n",
        0,
    ),
    (
        "line_comment_before_u8_return",
        "fn compute() -> // return ABI comment\nu8 { 300 }\n",
        "fn main() -> i64 { return compute() / 256; }\n",
        0,
    ),
    (
        "u8_spelling_only_in_return_comment",
        "fn compute() -> i64 // u8 is mentioned only in trivia\n{ 300 }\n",
        "fn main() -> i64 { return compute() / 256; }\n",
        1,
    ),
]

# The source-level classifier must match a complete builtin return token. A
# longer user type ending in the same bytes must retain the oracle's old path.
NON_BUILTIN_SUFFIX_FIXTURES = [
    (
        "user_type_ending_u8",
        "type myu8 = u8;\nfn compute() -> myu8 { return 300; }\n",
    ),
]


def main():
    if not SO.exists():
        print(f"BLOCKED: {SO} not found")
        return 1
    if not MINDC.exists():
        print(f"BLOCKED: mindc not found at {MINDC}")
        return 1

    fails = 0
    for name, fn_src, want in FIXTURES:
        o = oracle_mic3(fn_src)
        m = nfn_mic3(fn_src)
        ol = len(o) if o else -1
        ml = len(m) if m else 0
        byte_ok = (m == o and o is not None)
        rc = run_native(_prog(fn_src))
        exec_ok = (rc == (want & 0xFF))
        ok = byte_ok and exec_ok
        status = "PASS" if ok else "FAIL"
        print(
            f"  {status}  {name}  mic3 nb_len={ml} oracle_len={ol} byte_id={byte_ok}"
            f"  exec rc={rc}(want {want})"
        )
        if not ok:
            fails += 1
            if not byte_ok and o and m:
                k = min(len(o), len(m))
                di = next((i for i in range(k) if o[i] != m[i]), k)
                lo = max(0, di - 6)
                print(f"       first diff @ {di}  nb={list(m[lo:di+10])}  oracle={list(o[lo:di+10])}")

    for name, fn_src, main_src, want in NARROW_RETURN_FIXTURES:
        o = oracle_mic3(fn_src)
        m = nfn_mic3(fn_src)
        ol = len(o) if o else -1
        ml = len(m) if m else 0
        byte_ok = (m == o and o is not None)
        rc = run_native(fn_src + main_src)
        exec_ok = (rc == (want & 0xFF))
        ok = byte_ok and exec_ok
        status = "PASS" if ok else "FAIL"
        print(
            f"  {status}  {name}  mic3 nb_len={ml} oracle_len={ol} byte_id={byte_ok}"
            f"  exec rc={rc}(want {want})"
        )
        if not ok:
            fails += 1
            if not byte_ok and o and m:
                k = min(len(o), len(m))
                di = next((i for i in range(k) if o[i] != m[i]), k)
                lo = max(0, di - 6)
                print(f"       first diff @ {di}  nb={list(m[lo:di+10])}  oracle={list(o[lo:di+10])}")

    for name, src in NON_BUILTIN_SUFFIX_FIXTURES:
        o = oracle_mic3(src)
        m = nfn_mic3(src)
        ol = len(o) if o else -1
        ml = len(m) if m else 0
        byte_ok = (m == o and o is not None)
        status = "PASS" if byte_ok else "FAIL"
        print(f"  {status}  {name}  mic3 nb_len={ml} oracle_len={ol} byte_id={byte_ok}")
        if not byte_ok:
            fails += 1
            if o and m:
                k = min(len(o), len(m))
                di = next((i for i in range(k) if o[i] != m[i]), k)
                lo = max(0, di - 6)
                print(f"       first diff @ {di}  nb={list(m[lo:di+10])}  oracle={list(o[lo:di+10])}")

    if fails:
        print(f"FAIL: {fails} typed-cast/narrow-return fixture(s) diverged")
        return 1
    print("ALL PASS  (typed scalar casts and narrow returns: mic@3 byte-identical AND native-ELF run-correct)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
