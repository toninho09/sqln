#!/bin/sh
# Runs the vault probe on a throwaway Keychain in a temporary home folder.
# The login Keychain stays out of this test. Use --dialogs for the steps that need a person.
set -eu

cd "$(dirname "$0")/.."
BIN="$PWD/$(cargo test --test vault_probe --no-run 2>&1 | sed -n 's/.*(\(target[^)]*\)).*/\1/p')"

HOME_DIR="$(mktemp -d)"
trap 'rm -rf "$HOME_DIR"' EXIT
KEYCHAIN="$HOME_DIR/Library/Keychains/probe.keychain-db"
mkdir -p "$HOME_DIR/Library/Keychains" "$HOME_DIR/Library/Preferences"
export HOME="$HOME_DIR" SQLN_TEST_SYSTEM_VAULT=1

security create-keychain -p spike "$KEYCHAIN"
security list-keychains -d user -s "$KEYCHAIN"
security default-keychain -d user -s "$KEYCHAIN"
security set-keychain-settings "$KEYCHAIN"

probe() {
    echo "== $1"
    SQLN_VAULT_PROBE="$2" perl -e 'alarm 90; exec @ARGV' "$BIN" --ignored --nocapture 2>&1 \
        | grep -E '^(read|write|delete): ' || echo "no answer in 90 seconds"
}

account="$(SQLN_VAULT_PROBE=account "$BIN" --ignored --nocapture 2>&1 | sed -n 's/^account: //p')"

probe "Unlocked Keychain" read,write,read,delete,delete

if [ "${1:-}" = "--dialogs" ]; then
    # The Keychain stays locked after a canceled dialog, so this scenario comes last.
    security add-generic-password -s sqln -a "$account" -w other -T /usr/bin/security "$KEYCHAIN"
    echo "Press Deny in each macOS dialog."
    probe "Item of another application, Deny in the dialog" read
    security delete-generic-password -s sqln -a "$account" "$KEYCHAIN" > /dev/null 2>&1 || true
    SQLN_VAULT_PROBE=write "$BIN" --ignored --nocapture > /dev/null 2>&1
    security lock-keychain "$KEYCHAIN"
    echo "Press Cancel in each macOS dialog."
    probe "Locked Keychain, Cancel in the dialog" read
    exit 0
fi

export SQLN_VAULT_PROBE_NO_UI=1
SQLN_VAULT_PROBE=write "$BIN" --ignored --nocapture > /dev/null 2>&1
security lock-keychain "$KEYCHAIN"
probe "Locked Keychain, no window session" read,write,delete

security unlock-keychain -p spike "$KEYCHAIN"
security delete-generic-password -s sqln -a "$account" "$KEYCHAIN" > /dev/null 2>&1 || true
security add-generic-password -s sqln -a "$account" -w other -T /usr/bin/security "$KEYCHAIN"
probe "Item of another application, no window session" read,write,delete

rm -f "$KEYCHAIN"
probe "Keychain file removed" read,write,delete

security delete-keychain "$KEYCHAIN" 2> /dev/null || true
probe "No default Keychain" read,write,delete
