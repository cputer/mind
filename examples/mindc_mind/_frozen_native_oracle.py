"""Integrity-checked frozen references for the native ELF smoke."""

import hashlib
import pathlib


def load_manifest(manifest_path: pathlib.Path) -> dict[str, tuple[int, str, str]]:
    """Parse testdata/native_elf_oracle/MANIFEST.txt into
    {name: (size_bytes, sha256_hex, expected_exit_code)}."""
    manifest: dict[str, tuple[int, str, str]] = {}
    for line in manifest_path.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        name, size, sha256_hex, expected_exit = line.split("\t")
        manifest[name] = (int(size), sha256_hex, expected_exit)
    return manifest


def read_oracle(name: str, directory: pathlib.Path, manifest: dict) -> bytes:
    """Return the FROZEN native-ELF oracle reference bytes for fixture `name`
    (testdata/native_elf_oracle/{name}.elf), verified against the recorded
    size/sha256 in MANIFEST.txt so a corrupted or stale frozen reference fails
    LOUD rather than silently poisoning the byte-identity gate."""
    data = (directory / f"{name}.elf").read_bytes()
    recorded = manifest.get(name)
    if recorded is not None:
        exp_size, exp_sha256, _exit = recorded
        got_sha256 = hashlib.sha256(data).hexdigest()
        if len(data) != exp_size or got_sha256 != exp_sha256:
            raise RuntimeError(
                f"frozen oracle {name}.elf integrity check FAILED: "
                f"size {len(data)} (expected {exp_size}), "
                f"sha256 {got_sha256} (expected {exp_sha256})"
            )
    return data
