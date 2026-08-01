#!/usr/bin/env python3
"""Prove `data/` is what the extractors produce — the anti-drift gate.

`data/*.json` is generated. Nothing stopped anyone editing it by hand, and twice
that is exactly what happened: `move_step_units` was typed straight into
`game_config.json` and never taught to its extractor, and `tick_duration_ms` was
edited from 100 to 50 the same way. Both would have been silently reverted by the
next extractor run, and the only thing saying otherwise was prose in a directory
this repo does not publish.

This runs every extractor into a scratch tree and diffs that tree against the
committed one. A hand-edit, a generator its value was never taught to, and a
non-reproducible emission (an absolute path, an unstable ordering) all surface
the same way: a diff, and a non-zero exit.

    python3 tools/extract/verify.py

Needs `reference/openmu` — the gitignored full clone the extractors read. Exits
distinctly when it is absent, because "cannot check" is not "checked and clean".
"""

import filecmp
import os
import subprocess
import sys
import tempfile

from common import OPENMU_ROOT, REPO_ROOT

EXTRACT_DIR = os.path.dirname(os.path.abspath(__file__))
DATA_DIR = os.path.join(REPO_ROOT, "data")

# Every extractor. No extractor reads another's output, so the order is
# arbitrary; each writes only files it owns.
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
    """The three ways the committed tree can fail to be what the extractors write.

    Comparing the two SETS, not just the content of their intersection: a
    committed file nothing re-emits is drift too — it is the shape a hand-authored
    file takes — and it is invisible to a content-only check.
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
        # Deliberately EMPTY. Seeding it with the committed tree would give every
        # committed file a twin whether or not an extractor rewrote it, so a
        # hand-authored file would compare equal to its own copy and pass.
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
