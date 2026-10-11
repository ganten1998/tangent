#!/usr/bin/env bash
# The test census: a crate's #[test] count may not DROP without saying why.
#
# A test deleted with the code it covered is a deletion nobody reviews: the
# suite stays green because the thing that would have gone red is gone. The
# widths and identity work rewrites hundreds of tests that pin today's fixed
# desk, and "re-pinned" must never quietly become "removed". So every commit
# that lowers a crate's count has to carry a ledger in its body, one line per
# test it takes away:
#
#     census(<crate>): <removed test> -> <its replacement, or why none>
#
# (`→` is accepted for `->`.) Commits that keep or raise every count need
# nothing.
#
# **A ledger can be written late**, in `scripts/test-census.ledger`, for a
# commit already in history whose body carries none (or carries one in a
# shape this script does not read): history is never rewritten to fix a
# message, and a red census on every later commit is not a fix either. Each
# line there is a ledger line prefixed with the FULL hash of the commit it
# answers for, so it can excuse no other commit:
#
#     <40-hex commit> census(<crate>): <removed test> -> <its replacement>
#
# It is read from the working tree, which is where a late ledger has to
# live: the commit it speaks for cannot contain it.
#
# **Where the census starts.** At the commit that added this script, found
# from history rather than written down, so the census can never be pointed
# at a range that predates it (973 commits of history before it carry no
# ledgers, and were never asked to). CENSUS_BASE=<rev> overrides the start for
# a branch that wants to check less.
#
# **Merges are skipped**, as commits: a lane's own commits carry its ledgers,
# and each is compared with its first parent. The WORKING TREE is compared
# with HEAD as well, and a drop there fails too: it has no body to carry a
# ledger yet, so commit it with one.
#
# Counted: lines that are exactly a `#[test]` attribute (leading whitespace
# allowed) in every .rs file under the crate, src/ and tests/ alike. A
# `#[test]` inside a string or a comment is not on a line of its own and is
# not counted.
set -euo pipefail
cd "$(dirname "$0")/.."

CRATES="tangent-core tangent-ui tangent tangent-record tangent-host"
PATTERN='^[[:space:]]*#\[test\][[:space:]]*$'

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || {
  echo "test-census: not a git work tree; the census reads history" >&2
  exit 1
}

# `count <rev> <crate>`: the crate's #[test] lines at a revision, or in the
# working tree when <rev> is empty (tracked and untracked files both).
count() {
  local rev="$1" crate="$2"
  # `|| true`: git grep exits 1 when nothing matches, which for a crate with
  # no tests is the right answer (zero), not a failure.
  if [ -z "$rev" ]; then
    { git grep --untracked -h -c -E "$PATTERN" -- "$crate/*.rs" \
        ":(exclude)$crate/vendor/*" 2>/dev/null || true; } \
      | awk -F: '{s += $NF} END {print s + 0}'
  else
    { git grep -h -c -E "$PATTERN" "$rev" -- "$crate/*.rs" \
        ":(exclude)$crate/vendor/*" 2>/dev/null || true; } \
      | awk -F: '{s += $NF} END {print s + 0}'
  fi
}

# `ledger_lines <body> <crate>`: how many ledger lines the body gives a crate.
ledger_lines() {
  printf '%s\n' "$1" | grep -c -E "^census\\($2\\): .+ (->|→) .+" || true
}

LATE_LEDGER="scripts/test-census.ledger"

# `late_lines <commit> <crate>`: how many late ledger lines (see the header)
# answer for that commit and crate.
late_lines() {
  [ -f "$LATE_LEDGER" ] || { echo 0; return; }
  grep -c -E "^$1 census\\($2\\): .+ (->|→) .+" "$LATE_LEDGER" || true
}

if [ -n "${CENSUS_BASE:-}" ]; then
  start="$CENSUS_BASE"
else
  start="$(git log --diff-filter=A --format=%H -- scripts/test-census.sh | tail -1)"
  if [ -z "$start" ]; then
    # Not committed yet: only the working tree can be judged.
    start="HEAD"
  fi
fi

fail=0
checked=0
# The commit that introduced the census is judged too: `start^..HEAD`, or
# from the root when it has no parent.
if git rev-parse -q --verify "$start^" >/dev/null; then
  range="$start^..HEAD"
else
  range="HEAD"
fi
for c in $(git rev-list --no-merges --reverse "$range"); do
  parent="$(git rev-parse -q --verify "$c^" || true)"
  [ -n "$parent" ] || continue
  body="$(git log -1 --format=%B "$c")"
  checked=$((checked + 1))
  for crate in $CRATES; do
    was="$(count "$parent" "$crate")"
    now="$(count "$c" "$crate")"
    if [ "$now" -lt "$was" ]; then
      drop=$((was - now))
      have=$(( $(ledger_lines "$body" "$crate") + $(late_lines "$c" "$crate") ))
      if [ "$have" -lt "$drop" ]; then
        printf '  %s %s: %s -> %s tests, %s ledger line(s) for %s removed\n' \
          "$(git log -1 --format=%h "$c")" "$crate" "$was" "$now" "$have" "$drop" >&2
        fail=1
      fi
    fi
  done
done

for crate in $CRATES; do
  was="$(count HEAD "$crate")"
  now="$(count "" "$crate")"
  if [ "$now" -lt "$was" ]; then
    printf '  working tree %s: %s -> %s tests; commit it with a census(%s) ledger\n' \
      "$crate" "$was" "$now" "$crate" >&2
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "test-census: a crate lost tests without a ledger (see above)" >&2
  exit 1
fi
echo "test-census: $checked commit(s) and the working tree keep every count or say why"
