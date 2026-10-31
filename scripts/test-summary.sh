#!/bin/sh
# Run cargo test and print a one-line summary. On failure, list the failing tests.
# Usage: scripts/test-summary.sh [cargo test flags]

log="$(mktemp)"
status=0
cargo test "$@" --quiet >"$log" 2>&1 || status=$?

awk '
  /^test result:/ {
    for (i = 1; i <= NF; i++) {
      if ($i == "passed;") passed += $(i - 1)
      if ($i == "failed;") failed += $(i - 1)
      if ($i == "ignored;") ignored += $(i - 1)
    }
  }
  END { printf "tests: %d passed, %d failed, %d ignored\n", passed, failed, ignored }
' "$log"

if [ "$status" -ne 0 ]; then
  if grep -q '^failures:' "$log"; then
    echo "failing tests:"
    sed -n '/^failures:/,$p' "$log"
  else
    cat "$log"
  fi
fi

rm -f "$log"
exit "$status"
