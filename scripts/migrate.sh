#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "Usage: $0 <source_repo/subdir> <dest_repo/subdir>"
  exit 1
fi

SRC_PATH="$(realpath "$1")"
DEST_PATH="$(realpath "$2")"

# --- helper: find git root ---
find_git_root() {
  local dir="$1"
  while [ "$dir" != "/" ]; do
    if [ -d "$dir/.git" ]; then
      echo "$dir"
      return 0
    fi
    dir="$(dirname "$dir")"
  done
  return 1
}

# --- resolve source repo + subdir ---
SRC_REPO="$(find_git_root "$SRC_PATH")" || {
  echo "❌ Could not find git repo for source path"
  exit 1
}

SRC_SUBDIR="${SRC_PATH#$SRC_REPO/}"

# --- resolve dest repo + subdir ---
DEST_REPO="$(find_git_root "$DEST_PATH")" || {
  echo "❌ Could not find git repo for destination path"
  exit 1
}

DEST_SUBDIR="${DEST_PATH#$DEST_REPO/}"

echo "== Source repo: $SRC_REPO"
echo "== Source subdir: $SRC_SUBDIR"
echo "== Dest repo: $DEST_REPO"
echo "== Dest subdir: $DEST_SUBDIR"

# --- sanity checks ---
if [ -z "$SRC_SUBDIR" ]; then
  echo "❌ Source path must be a subdirectory, not repo root"
  exit 1
fi

if [ "$SRC_REPO" = "$DEST_REPO" ]; then
  echo "❌ Source and destination repos must be different"
  exit 1
fi

# # --- check git-filter-repo ---
# if ! git filter-repo --help >/dev/null 2>&1; then
#   echo "❌ git filter-repo is not installed or not in PATH"
#   echo "Install with: pip install git-filter-repo"
#   exit 1
# fi

WORKDIR="$(mktemp -d)"
echo "== Working dir: $WORKDIR"

cleanup() {
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

# --- clone source safely ---
echo "== Cloning source repo (safe mode) =="
git clone --no-tags --single-branch "$SRC_REPO" "$WORKDIR/src"
cd "$WORKDIR/src"

# clean possible broken refs
git remote prune origin || true

# --- filter + rewrite paths directly into destination subdir ---
echo "== Extracting and rewriting history =="
git filter-repo \
  --path "$SRC_SUBDIR/" \
  --path-rename "$SRC_SUBDIR/":"$DEST_SUBDIR/" \
  --force

# --- prepare destination ---
cd "$DEST_REPO"

BRANCH="import-$(basename "$SRC_SUBDIR")-$(date +%s)"
echo "== Creating branch $BRANCH =="
git checkout -b "$BRANCH"

# --- import history ---
echo "== Importing into destination repo =="
git remote add temp-import "$WORKDIR/src"
git fetch temp-import

git merge temp-import/HEAD \
  --allow-unrelated-histories \
  -m "Import $SRC_SUBDIR into $DEST_SUBDIR (preserve history)"

# --- cleanup ---
git remote remove temp-import

echo ""
echo "✅ Migration complete!"
echo ""
echo "Branch created: $BRANCH"
echo ""
echo "Next steps:"
echo "  cd $DEST_REPO"
echo "  git checkout main"
echo "  git merge $BRANCH"
