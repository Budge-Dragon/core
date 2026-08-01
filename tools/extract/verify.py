#!/usr/bin/env python3
"""Prove `data/` is what the extractors produce.

    python3 tools/extract/verify.py

Runs every extractor into a scratch tree and diffs. A hand-edit, a value its
generator was never taught, and a non-reproducible emission all surface the same
way: a diff and a non-zero exit.

Exit 0 clean, 1 drift, 2 `reference/openmu` absent — "cannot check" is not
"checked and clean".
"""

import filecmp
import os
import subprocess
import sys
import tempfile

from common import OPENMU_ROOT, REPO_ROOT

EXTRACT_DIR = os.path.dirname(os.path.abspath(__file__))
DATA_DIR = os.path.join(REPO_ROOT, "data")

# Order is arbitrary — no extractor reads another's output.
EXTRACTORS = [
    "items.py",
    "monsters.py",
    "maps.py",
    "skills-effects.py",
    "classes.py",
    "shops.py",
    "chaos.py",
    "options-sets.py",
    "drops.py",
    "constants-exp.py",
]

EXIT_OK = 0
EXIT_DRIFT = 1
EXIT_CANNOT_CHECK = 2


def run_extractors(into):
    """Run every extractor with its writes redirected under `into`."""
    env = dict(os.environ, MU_CORE_DATA_ROOT=into)
    for script in EXTRACTORS:
        result = subprocess.run(
            [sys.executable, os.path.join(EXTRACT_DIR, script)],
            cwd=EXTRACT_DIR,
            env=env,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            print("FAIL %s exited %d" % (script, result.returncode))
            print(result.stdout[-4000:])
            print(result.stderr[-4000:])
            return False
    return True


def relative_files(root):
    """Every file under `root`, as paths relative to it."""
    found = set()
    for parent, _dirs, files in os.walk(root):
        for name in files:
            found.add(os.path.relpath(os.path.join(parent, name), root))
    return found


def drifted(committed, produced):
    """Changed, committed-but-unwritten, produced-but-uncommitted.

    Set comparison, not just content: a committed file nothing re-emits is the
    shape a hand-authored file takes, and a content-only check cannot see it.
    """
    here, there = relative_files(committed), relative_files(produced)
    return (
        sorted(
            rel
            for rel in here & there
            if not filecmp.cmp(
                os.path.join(committed, rel), os.path.join(produced, rel), shallow=False
            )
        ),
        sorted(here - there),
        sorted(there - here),
    )


def main():
    if not os.path.isdir(OPENMU_ROOT):
        print("cannot check: %s is absent" % OPENMU_ROOT)
        print("the extractors read the gitignored OpenMU clone; see docs/openmu-reference.md")
        return EXIT_CANNOT_CHECK

    with tempfile.TemporaryDirectory(prefix="mu-core-verify-") as scratch:
        # Deliberately empty: seeding it would let a hand-authored file compare
        # equal to its own copy and pass.
        if not run_extractors(scratch):
            return EXIT_CANNOT_CHECK

        changed, unwritten, uncommitted = drifted(DATA_DIR, os.path.join(scratch, "data"))

    if not (changed or unwritten or uncommitted):
        print("data/ reproduces from tools/extract/ — no drift")
        return EXIT_OK

    print("data/ is not what the extractors produce:")
    for rel in changed:
        print("  differs           data/%s" % rel)
    for rel in unwritten:
        print("  no extractor emits it   data/%s" % rel)
    for rel in uncommitted:
        print("  produced, uncommitted   data/%s" % rel)
    print()
    print("Fix the value at its extractor in tools/extract/ and re-run it.")
    print("Editing data/*.json by hand is reverted by the next extractor run.")
    return EXIT_DRIFT


if __name__ == "__main__":
    sys.exit(main())
