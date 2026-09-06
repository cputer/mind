"""Owned CLI projects for diagnostic smoke fixtures."""

import os
import subprocess


def write_fixture(src, workdir, idx, *, local_project=False):
    path = os.path.join(workdir, f"case_{idx}.mind")
    if local_project:
        # The call-resolution phase needs a resolved import graph. Own a
        # project per invocation so prior fixtures cannot join its source set.
        project = os.path.join(workdir, f"project_{idx}")
        os.makedirs(project)
        path = os.path.join(project, "main.mind")
        with open(os.path.join(project, "helper.mind"), "w") as f:
            f.write("pub fn fixture_export() -> i64 { return 1; }\n")
        with open(os.path.join(project, "Mind.toml"), "w") as f:
            f.write('[package]\nname="e2003_fixture"\nversion="0.1.0"\n'
                    '[build]\nentry="main.mind"\n[targets.cpu]\n'
                    'backend="cpu"\nsources=["main.mind", "helper.mind"]\n')
    with open(path, "w") as f:
        f.write(src)
    return path


def assert_unscoped_import_refused(mindc, source, workdir):
    standalone = os.path.join(workdir, "unscoped_import.mind")
    with open(standalone, "w") as f:
        f.write(source)
    refused = subprocess.run([mindc, "check", standalone],
                             capture_output=True, text=True)
    refusal = refused.stdout + refused.stderr
    assert refused.returncode != 0 and "enclosing Mind.toml" in refusal, (
        "unscoped local import was not explicitly refused: " + refusal
    )
