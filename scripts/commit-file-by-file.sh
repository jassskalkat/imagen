#!/usr/bin/env bash
# Commit pending worktree changes one file at a time.
#
# Usage: scripts/commit-file-by-file.sh --message <path>=<description> ... [--path <file> ...] [--type <commit-type>] [--dry-run]
#
# Options:
#   --message <mapping>   Per-file description in the form path=description. Repeat for each file.
#   --path <file>         Limit commits to the listed paths. Repeat as needed.
#   --type <commit-type>  Conventional commit type. Defaults to "chore".
#   --dry-run             Print the commit plan without committing.
# Paths are interpreted relative to the repository root.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

die() { echo -e "${RED}FATAL:${NC} $*" >&2; exit 1; }
info() { echo -e "${GREEN}OK:${NC} $*"; }
warn() { echo -e "${YELLOW}WARN:${NC} $*"; }
plan() { echo -e "PLAN:${NC} $*"; }

commit_scope() {
  case "$1" in
    .agents/*) echo "agents" ;;
    .github/*) echo "ci" ;;
    AGENTS.md) echo "agents" ;;
    scripts/*) echo "git" ;;
    */*) echo "${1%%/*}" ;;
    *) echo "repo" ;;
  esac
}

VALID_TYPES="feat|fix|refactor|chore|docs|test|ci|perf|style|build|revert"
COMMIT_TYPE="chore"
message_paths=()
message_descriptions=()
selected_paths=()
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --type)
      [[ $# -ge 2 ]] || die "Missing value after --type"
      COMMIT_TYPE="$2"
      [[ "$COMMIT_TYPE" =~ ^($VALID_TYPES)$ ]] || die "Invalid commit type: $COMMIT_TYPE. Valid: $VALID_TYPES"
      shift 2
      ;;
    --message)
      [[ $# -ge 2 ]] || die "Missing value after --message"
      mapping="$2"
      [[ "$mapping" == *=* ]] || die "--message must use path=description"
      message_path="${mapping%%=*}"
      message_description="${mapping#*=}"
      [[ -n "$message_path" ]] || die "--message path cannot be blank"
      [[ "$message_description" != *$'\n'* ]] || die "Commit description must be one line"
      [[ "$message_description" != *$'\r'* ]] || die "Commit description must not contain carriage returns"
      [[ "$message_description" =~ [^[:space:]] ]] || die "Commit description cannot be blank"
      [[ "$message_description" != [[:space:]]* && "$message_description" != *[[:space:]] ]] || die "Commit description cannot start or end with whitespace"
      if [[ ${#message_paths[@]} -gt 0 ]]; then
        for existing_path in "${message_paths[@]}"; do
          [[ "$existing_path" != "$message_path" ]] || die "Duplicate --message path: $message_path"
        done
      fi
      message_paths+=("$message_path")
      message_descriptions+=("$message_description")
      shift 2
      ;;
    --path)
      [[ $# -ge 2 ]] || die "Missing value after --path"
      selected_paths+=("$2")
      shift 2
      ;;
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --help)
      echo "Usage: $0 --message <path>=<description> ... [--path <file> ...] [--type <type>] [--dry-run]"
      echo "  --message <mapping>  Per-file description; repeat for each changed file"
      echo "  --path <file>       Limit commits to the listed repository-root-relative paths"
      echo "  --type <type>     Commit type (default: chore)"
      echo "  --dry-run         Show the plan without committing"
      exit 0
      ;;
    *)
      die "Unknown argument: $1"
      ;;
  esac
done

repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || die "Not inside a Git repository"
cd "$repo_root" || die "Could not enter repository root: $repo_root"

if ! git diff --cached --quiet; then
  die "Index contains staged changes. Unstage them before running this script."
fi

files=()
actions=()
while IFS= read -r -d '' status; do
  code="${status:0:2}"
  file="${status:3}"

  case "$code" in
    R*|*R|C*|*C)
      die "Renames and copies are not supported: $file"
      ;;
    U*|*U|AA|DD)
      die "Unmerged changes are not supported: $file"
      ;;
    *)
      files+=("$file")
      case "$code" in
        *D|D*) actions+=("remove") ;;
        \?\?) actions+=("add") ;;
        *) actions+=("update") ;;
      esac
      ;;
  esac
done < <(git status --porcelain=v1 -z --untracked-files=all)

pending_files=("${files[@]}")

if [[ ${#selected_paths[@]} -gt 0 ]]; then
  filtered_files=()
  filtered_actions=()
  for index in "${!files[@]}"; do
    for selected_path in "${selected_paths[@]}"; do
      if [[ "${files[$index]}" == "$selected_path" ]]; then
        filtered_files+=("${files[$index]}")
        filtered_actions+=("${actions[$index]}")
        break
      fi
    done
  done
  files=("${filtered_files[@]}")
  actions=("${filtered_actions[@]}")
fi

if [[ ${#files[@]} -eq 0 ]]; then
  if [[ ${#selected_paths[@]} -gt 0 ]]; then
    info "No pending worktree changes match the selected paths."
  else
    info "No pending worktree changes to commit."
  fi
  exit 0
fi

[[ ${#message_paths[@]} -gt 0 ]] || die "At least one --message path=description is required"

info "Planned ${#files[@]} commit(s) using type \"$COMMIT_TYPE\""

for message_path in "${message_paths[@]}"; do
  message_path_found=false
  for file in "${pending_files[@]}"; do
    if [[ "$file" == "$message_path" ]]; then
      message_path_found=true
      break
    fi
  done
  [[ "$message_path_found" == "true" ]] || die "--message path is not a pending file: $message_path"
done

for index in "${!files[@]}"; do
  file="${files[$index]}"
  action="${actions[$index]}"
  description_index=-1
  for message_index in "${!message_paths[@]}"; do
    if [[ "${message_paths[$message_index]}" == "$file" ]]; then
      description_index="$message_index"
      break
    fi
  done
  [[ "$description_index" -ge 0 ]] || die "Missing --message for pending file: $file"
  scope=$(commit_scope "$file")
  commit_msg="$COMMIT_TYPE($scope): ${message_descriptions[$description_index]} [$action $file]"

  echo ""
  plan "File: $file"
  plan "Message: $commit_msg"

  if [[ "$DRY_RUN" == "true" ]]; then
    continue
  fi

  literal_path=":(literal)$file"
  git add -A -- "$literal_path" || die "Could not stage $file"
  staged_files=()
  while IFS= read -r -d '' staged_file; do
    staged_files+=("$staged_file")
  done < <(git diff --cached --name-only -z)
  [[ ${#staged_files[@]} -eq 1 && "${staged_files[0]}" == "$file" ]] || {
    git reset --quiet -- "$literal_path" || true
    die "Refusing to commit more than one file"
  }
  previous_commit=$(git rev-parse --verify HEAD 2>/dev/null || true)
  if ! git commit --only -m "$commit_msg" -- "$literal_path"; then
    git reset --quiet -- . || true
    die "Commit failed for $file; worktree changes were preserved and staged changes were cleared"
  fi
  committed_files=()
  while IFS= read -r -d '' committed_file; do
    committed_files+=("$committed_file")
  done < <(git diff-tree --root --no-commit-id --name-only -r -z HEAD)
  [[ ${#committed_files[@]} -eq 1 && "${committed_files[0]}" == "$file" ]] || {
    die "Commit ${previous_commit:-<unborn>}..HEAD contains unexpected files; inspect HEAD before continuing"
  }
done

if [[ "$DRY_RUN" == "true" ]]; then
  echo ""
  warn "Dry run complete. No commits created."
else
  echo ""
  info "File-by-file commit flow complete."
fi
