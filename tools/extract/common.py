"""Shared helpers for mu-core data extractors (throwaway tooling).

Every extractor imports from here. Conventions come from
docs/specs/2026-07-02-data-schemas.md: file envelope
{"schema_version": 1, "records": [...]}, snake_case slugs, stat references
via stat_map.json.
"""

import json
import os
import re

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DATA_DIR = os.path.join(REPO_ROOT, "data")
COVERAGE_DIR = os.path.join(DATA_DIR, "_coverage")
STAT_MAP_PATH = os.path.join(os.path.dirname(__file__), "stat_map.json")

SCHEMA_VERSION = 1

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
    envelope = {"schema_version": SCHEMA_VERSION, "records": records}
    with open(path, "w", encoding="utf-8") as f:
        json.dump(envelope, f, indent=2, ensure_ascii=False)
        f.write("\n")
    return path


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
