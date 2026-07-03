"""Shared helpers for mu-core data extractors (throwaway tooling).

Every extractor imports from here. The file envelope is {"records": [...]};
records deserialize into the core's `DataFile<T>` types.
"""

import json
import os
import re

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DATA_DIR = os.path.join(REPO_ROOT, "data")
COVERAGE_DIR = os.path.join(DATA_DIR, "_coverage")
STAT_MAP_PATH = os.path.join(os.path.dirname(__file__), "stat_map.json")

# Boundary between a lowercase/digit and an uppercase letter, or between an
# acronym and the next word ("PvM" -> "pv_m" is avoided by the second branch).
_CAMEL_BOUNDARY = re.compile(r"(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])")
_NON_ALNUM = re.compile(r"[^a-z0-9]+")


def slugify(name):
    """'TotalEnergy minus 15' -> 'total_energy_minus_15'."""
    s = _CAMEL_BOUNDARY.sub("_", name)
    s = _NON_ALNUM.sub("_", s.lower())
    return s.strip("_")


def write_datafile(path, records):
    """Write the spec envelope. `path` is absolute or relative to data/."""
    if not os.path.isabs(path):
        path = os.path.join(DATA_DIR, path)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    envelope = {"records": records}
    with open(path, "w", encoding="utf-8") as f:
        json.dump(envelope, f, indent=2, ensure_ascii=False)
        f.write("\n")
    return path


NAMES_DIR = os.path.join(DATA_DIR, "names")


def write_names(filename, payload):
    """Write a HOST-OWNED display-name sidecar to data/names/<filename>.

    Display names are extracted identity->name mappings, kept out of the pure
    core data files (which carry only identities and rules). `filename` shadows
    the core data file the names re-attach to (e.g. "item_definitions.json").
    `validate_refs.py` ignores data/names/ (it globs only top-level *.json).
    """
    path = os.path.join(NAMES_DIR, filename)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)
        f.write("\n")
    return path


def without_name(record):
    """A copy of a built record with the display `name` dropped — the core data
    files carry no name (it lives in the data/names/ sidecar)."""
    return {k: v for k, v in record.items() if k != "name"}


def item_ref(group, number):
    return {"group": int(group), "number": int(number)}


def map_ref(number, discriminator=0):
    return {"number": int(number), "discriminator": int(discriminator)}


def load_stat_map():
    """OpenMU Stats.cs property name (or inline designation) -> mu-core slug."""
    with open(STAT_MAP_PATH, encoding="utf-8") as f:
        return json.load(f)


def coverage(category, info):
    """Write data/_coverage/<category>.json for the orchestrator."""
    os.makedirs(COVERAGE_DIR, exist_ok=True)
    path = os.path.join(COVERAGE_DIR, category + ".json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump(info, f, indent=2, ensure_ascii=False)
        f.write("\n")
    return path
