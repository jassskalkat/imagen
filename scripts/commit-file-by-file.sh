#!/usr/bin/env bash
# Commit pending changes one file at a time.
#
# Usage: scripts/commit-by-file.sh [--type <commit-type>] [--dry-run]
#
# Options:
#   --type <commit-type>  Conventional commit type. Defaults to "chore".
#   --dry-run            Print commit plan without committing.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

die() { echo -e "${RED}FATAL:${NC} $*" >&2; exit 1; }
info() { echo -e "${GREEN}OK:${NC} $*"; }
warn() { echo -e "${YELLOW}WARN:${NC} $*"; }

VALID_TYPES="feat|fix|refactor|chore|docs|test|ci|perf|style|build|revert"
COMMIT_TYPE="chore"
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --type)
      [[ $# -ge 2 ]] || die "Missing value after --type"
      COMMIT_TYPE="$2"
      if ! echo "$COMMIT_TYPE" | grep -qE "^($VALID_TYPES)$"; then
        die "Invalid commit type: $COMMIT_TYPE. Valid: $VALID_TYPES"
      fi
      shift 2
      ;;
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --help)
      echo "Usage: $0 [--type <type>] [--dry-run]"
      echo "  --type <type>  Commit type (default: chore)"
      echo "  --dry-run      Show plan without committing"
      exit 0
      ;;
    *)
      die "Unknown argument: $1"
      ;;
  esac
done

get_changed_files() {
  local unstaged staged untracked
  unstaged=$(git diff --name-only --diff-filter=ACDMRTUXB -z 2>/dev/null | tr '\0' '\n' || true)
  staged=$(git diff --cached --name-only --diff-filter=ACDMRTUXB -z 2>/dev/null | tr '\0' '\n' || true)
  untracked=$(git ls-files --others --exclude-standard -z 2>/dev/null | tr '\0' '\n' || true)

  echo "$unstaged"$'\n'"$staged"$'\n'"$untracked" | grep -v '^$' | sort -u || true
}

changed_files=$(get_changed_files)

if [[ -z "$changed_files" ]]; then
  info "No pending changes to commit."
  exit 0
fi

file_count=$(echo "$changed_files" | wc -l | tr -d ' ')
info "Planned $file_count commit(s) using type \"$COMMIT_TYPE\""

while IFS= read -r file; do
  [[ -z "$file" ]] && continue

  folder=$(dirname "$file")
  if [[ "$folder" == "." ]]; then
    commit_msg="$COMMIT_TYPE: commit $file"
  else
    commit_msg="$COMMIT_TYPE: commit $file"
  fi

  echo ""
  warn "File: $file"
  info "Message: $commit_msg"

  if [[ "$DRY_RUN" == "true" ]]; then
    continue
  fi

  git add -- "$file" 2>/dev/null || git rm --cached -- "$file" 2>/dev/null || true
  git commit --no-verify -m "$commit_msg"

done <<< "$changed_files"

if [[ "$DRY_RUN" == "true" ]]; then
  echo ""
  warn "Dry run complete. No commits created."
  exit 0
fi

echo ""
info "File-by-file commit flow complete."
