#!/usr/bin/env bash
# Smoke tests: verifies core wt + ticket workflows against a hermetic temp repo.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
FLOWY="$ROOT_DIR/target/debug/flowy"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

pass() { printf "${GREEN}PASS${NC} %s\n" "$1"; }
fail() { printf "${RED}FAIL${NC} %s: %s\n" "$1" "${2:-}" >&2; exit 1; }

if [ ! -f "$FLOWY" ]; then
    echo "Building flowy..." >&2
    cargo build --manifest-path "$ROOT_DIR/Cargo.toml" 2>&1 >&2
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

REPO="$TMP/test-repo"
mkdir -p "$REPO"

# --- scaffold: bare repo with one commit so worktree add works ---
SRC="$TMP/source"
git init -b main "$SRC" -q
git -C "$SRC" commit --allow-empty -m "init" -q
git clone --bare "$SRC" "$REPO/.bare" -q
printf 'gitdir: ./.bare\n' > "$REPO/.git"
mkdir -p "$REPO/worktrees"
mkdir -p "$REPO/notes/workstreams/archive"

F="$FLOWY --repo-root $REPO"

# --- 1. wt new ---
$F wt new -b test/branch
[ -d "$REPO/worktrees/test-branch" ] \
    || fail "wt new" "worktrees/test-branch/ not created"
[ -f "$REPO/notes/workstreams/test-branch/meta.toml" ] \
    || fail "wt new" "meta.toml not written"
grep -q 'branch' "$REPO/notes/workstreams/test-branch/meta.toml" \
    || fail "wt new" "meta.toml missing branch field"
grep -q 'created' "$REPO/notes/workstreams/test-branch/meta.toml" \
    || fail "wt new" "meta.toml missing created field"
pass "wt new: worktree + meta.toml created with correct fields"

# --- 2. t add ---
$F t add -w test-branch "My ticket"
TICKET="$REPO/notes/workstreams/test-branch/tickets/001-my-ticket.md"
[ -f "$TICKET" ] || fail "t add" "001-my-ticket.md not created"
grep -q '^# My ticket$' "$TICKET" || fail "t add" "h1 heading wrong"
pass "t add: ticket file created with correct h1"

# --- 3. t done ---
$F t done -w test-branch 001 --summary "Done"
grep -q '^# completed: My ticket$' "$TICKET" \
    || fail "t done" "h1 not rewritten with 'completed:' prefix"
grep -q '## Summary' "$TICKET" || fail "t done" "summary section not appended"
pass "t done: h1 rewritten and summary appended"

# --- 4. t list --json ---
LIST_JSON=$($F t list -w test-branch --json --status all)
echo "$LIST_JSON" | jq . > /dev/null 2>&1 \
    || fail "t list --json" "output is not valid JSON"
COUNT=$(echo "$LIST_JSON" | jq 'length')
[ "$COUNT" -eq 1 ] || fail "t list --json" "expected 1 ticket, got $COUNT"
STATUS=$(echo "$LIST_JSON" | jq -r '.[0].status')
[ "$STATUS" = "complete" ] || fail "t list --json" "expected status=complete, got $STATUS"
NUMBER=$(echo "$LIST_JSON" | jq -r '.[0].number')
[ "$NUMBER" = "1" ] || fail "t list --json" "expected number=1, got $NUMBER"
pass "t list --json: valid JSON with correct ticket fields"

# --- 5. wt ls --json ---
LS_JSON=$($F wt ls --json)
echo "$LS_JSON" | jq . > /dev/null 2>&1 \
    || fail "wt ls --json" "output is not valid JSON"
PENDING=$(echo "$LS_JSON" | jq '[.[] | select(.name == "test-branch")] | .[0].tickets_pending')
TOTAL=$(echo "$LS_JSON" | jq '[.[] | select(.name == "test-branch")] | .[0].tickets_total')
[ "$PENDING" -eq 0 ] || fail "wt ls --json" "expected 0 pending, got $PENDING"
[ "$TOTAL" -eq 1 ] || fail "wt ls --json" "expected 1 total, got $TOTAL"
pass "wt ls --json: correct ticket counts for test-branch"

# --- 6. wt archive ---
$F wt archive test-branch
[ ! -d "$REPO/worktrees/test-branch" ] \
    || fail "wt archive" "worktree dir still exists"
[ -d "$REPO/notes/workstreams/archive/test-branch" ] \
    || fail "wt archive" "archive dir not created"
[ ! -d "$REPO/notes/workstreams/test-branch" ] \
    || fail "wt archive" "notes not moved out of workstreams/"
pass "wt archive: worktree removed and notes moved to archive"

# --- 7. wt unarchive ---
$F wt unarchive test-branch
[ -d "$REPO/worktrees/test-branch" ] \
    || fail "wt unarchive" "worktree not restored"
[ -d "$REPO/notes/workstreams/test-branch" ] \
    || fail "wt unarchive" "notes not restored"
[ ! -d "$REPO/notes/workstreams/archive/test-branch" ] \
    || fail "wt unarchive" "archive dir still exists"
pass "wt unarchive: worktree and notes restored"

printf "\n${GREEN}All smoke tests passed.${NC}\n"
