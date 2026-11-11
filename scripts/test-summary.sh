#!/bin/sh
# Run cargo test and print a one-line summary. On failure, list the failing tests.
# Usage: scripts/test-summary.sh [cargo test target flags]
#
# Test binaries run in parallel, with one binary per two logical CPUs and
# four test threads each. This overlaps waits without excessive CPU load.
# Time-limit tests can still fail under this load, so each failing
# binary runs again alone and is reported; only a second failure counts. Test
# filters and libtest arguments are not supported; run cargo test directly for
# those.

dir="$(mktemp -d)"
status=0

# Build only; the binaries run separately below.
if ! cargo test --no-run --message-format=json "$@" >"$dir/build.json" 2>"$dir/build.err"; then
  cat "$dir/build.err"
  rm -rf "$dir"
  exit 1
fi

# Collect the test executables. Keep only deps/ paths: target/debug/sqln is
# the application binary, not a test harness. The largest binaries hold the
# longest waits, so they start first and their waits overlap with the rest.
awk 'match($0, /"executable":"[^"]*\/deps\/[^"]+"/) {
  print substr($0, RSTART + 14, RLENGTH - 15)
}' "$dir/build.json" | sort -u | while IFS= read -r bin; do
  printf '%s %s\n' "$(wc -c <"$bin")" "$bin"
done | sort -rn | awk '{$1 = ""; sub(/^ /, ""); print}' >"$dir/bins"

cpus="$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)"
jobs="$(( (cpus + 1) / 2 ))"
[ "$jobs" -lt 1 ] && jobs=1
threads=4
tr '\n' '\0' <"$dir/bins" | DIR="$dir" THREADS="$threads" xargs -0 -P "$jobs" -n 1 sh -c '
  bin="$1"
  name="$(basename "$bin")"
  if "$bin" --quiet --test-threads="$THREADS" >"$DIR/log-$name" 2>&1; then
    :
  else
    printf "%s\n" "$bin" >>"$DIR/failed"
  fi
' sh


# Retry the failing binaries alone, so load does not fail a time-limit test.
if [ -f "$dir/failed" ]; then
  while IFS= read -r bin; do
    name="$(basename "$bin")"
    if "$bin" --quiet --test-threads="$threads" >"$dir/log-$name" 2>&1; then
      printf "%s\n" "$name" >>"$dir/retried"
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

if [ -f "$dir/retried" ]; then
  names="$(sed 's/-[0-9a-f]*$//' "$dir/retried" | sort -u | tr '\n' ' ')"
  echo "retried alone after a first failure: ${names% }"
fi

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
