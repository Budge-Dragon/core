#!/usr/bin/env bash
# Re-creates /tmp/openmu-ref: shallow sparse clone of MUnique/OpenMU with only
# the directories the mu-core extractors read. Idempotent.
set -euo pipefail

DEST="/tmp/openmu-ref"
REPO="https://github.com/MUnique/OpenMU"
SPARSE_DIRS=(src/DataModel src/Persistence/Initialization src/GameLogic)

if [ ! -d "$DEST/.git" ]; then
    rm -rf "$DEST"
    git clone --depth 1 --filter=blob:none --sparse "$REPO" "$DEST"
fi

git -C "$DEST" sparse-checkout set "${SPARSE_DIRS[@]}"
echo "openmu-ref ready at $DEST"
