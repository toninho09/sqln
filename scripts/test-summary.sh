#!/bin/sh
# Run cargo test and print a one-line summary. On failure, list the failing tests.
# Usage: scripts/test-summary.sh [cargo test target flags]
#
# Test binaries run in parallel instead of one by one, which is the main cost
# of the suite. Time-limit tests can fail under this load, so each failing
# binary runs again alone; only a second failure counts. Test filters and
# libtest arguments are not supported; run cargo test directly for those.

dir="$(mktemp -d)"
status=0

# Build only; the binaries run separately below.
if ! cargo test --no-run --message-format=json "$@" >"$dir/build.json" 2>"$dir/build.err"; then
  cat "$dir/build.err"
  rm -rf "$dir"
  exit 1
fi

# Collect the test executables. Keep only deps/ paths: target/debug/sqln is
# the application binary, not a test harness.
awk 'match($0, /"executable":"[^"]*\/deps\/[^"]+"/) {
  print substr($0, RSTART + 14, RLENGTH - 15)
}' "$dir/build.json" | sort -u >"$dir/bins"

jobs="$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"
tr '\n' '\0' <"$dir/bins" | DIR="$dir" xargs -0 -P "$jobs" -n 1 sh -c '
  bin="$1"
  name="$(basename "$bin")"
  if "$bin" --quiet >"$DIR/log-$name" 2>&1; then
    :
  else
    printf "%s\n" "$bin" >>"$DIR/failed"
  fi
' sh


# Retry the failing binaries alone, so load does not fail a time-limit test.
if [ -f "$dir/failed" ]; then
  while IFS= read -r bin; do
    name="$(basename "$bin")"
    if "$bin" --quiet >"$dir/log-$name" 2>&1; then
      :
    else
      status=1
      printf "%s\n" "$bin" >>"$dir/really-failed"
    fi
  done <"$dir/failed"
fi

awk '
  /^test result:/ {
    for (i = 1; i <= NF; i++) {
      if ($i == "passed;") passed += $(i - 1)
      if ($i == "failed;") failed += $(i - 1)
      if ($i == "ignored;") ignored += $(i - 1)
    }
  }
  END { printf "tests: %d passed, %d failed, %d ignored\n", passed, failed, ignored }
' "$dir"/log-* 2>/dev/null

if [ "$status" -ne 0 ]; then
  echo "failing tests:"
  printed=0
  while IFS= read -r bin; do
    name="$(basename "$bin")"
    log="$dir/log-$name"
    if grep -q "^failures:" "$log"; then
      sed -n "/^failures:/,\$p" "$log"
    else
      cat "$log"
    fi
    printed=1
  done <"$dir/really-failed"
  if [ "$printed" -eq 0 ]; then
    echo "no test output; see the build or run logs"
  fi
fi

rm -rf "$dir"
exit "$status"
