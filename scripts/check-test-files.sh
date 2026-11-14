#!/bin/sh
# Fail if a test file would never run. A file in tests/ of the root package must
# be a [[test]] target in Cargo.toml or a mod of another file in tests/. That
# file has the same check, so each file reaches a target. A file in a folder
# crates/*/tests/<name>/ must be main.rs or a mod of that main.rs.

roots="$(sed -n 's/^path = "\(tests\/[a-z_]*\.rs\)"/\1/p' Cargo.toml)"
if [ -z "$roots" ]; then
  echo "check-test-files: no [[test]] paths found in Cargo.toml" >&2
  exit 1
fi

status=0
for file in tests/*.rs; do
  case "$roots" in
    *"$file"*) continue ;;
  esac
  stem="${file#tests/}"
  stem="${stem%.rs}"
  if ! grep -q "^mod ${stem};" tests/*.rs; then
    echo "check-test-files: $file is in no [[test]] target and no file declares it" >&2
    status=1
  fi
done

for file in crates/*/tests/*/*.rs; do
  [ -e "$file" ] || continue
  dir="${file%/*}"
  stem="${file##*/}"
  stem="${stem%.rs}"
  [ "$stem" = main ] && continue
  if ! grep -q "^mod ${stem};" "$dir/main.rs" 2>/dev/null; then
    echo "check-test-files: $file is not a mod of $dir/main.rs" >&2
    status=1
  fi
done

exit "$status"
