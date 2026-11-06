#!/bin/sh
# Fail if a file in tests/ is neither a [[test]] target in Cargo.toml nor a
# mod of a root file. A forgotten file would never run.

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
  if ! grep -q "^mod ${stem};" $roots; then
    echo "check-test-files: $file is in no [[test]] target and no root declares it" >&2
    status=1
  fi
done

exit "$status"
