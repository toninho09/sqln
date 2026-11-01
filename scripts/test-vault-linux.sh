#!/bin/sh
# Runs the vault probe on the Secret Service in a throwaway Linux container.
# This covers the service and its errors. It does not cover a desktop session.
# The Docker volumes sqln-vault-cargo and sqln-vault-target keep the build between runs.
set -eu

IMAGE=rust:1.98-slim-bookworm

probe() {
    echo "== $1"
    SQLN_VAULT_PROBE="$2" timeout 60 "$BIN" --ignored --nocapture 2>&1 \
        | grep -E '^(read|write|delete): ' || echo "no answer in 60 seconds"
}

case "${1:-}" in
"")
    cd "$(dirname "$0")/.."
    exec docker run --rm --cap-add IPC_LOCK \
        -v "$PWD:/src:ro" \
        -v sqln-vault-cargo:/usr/local/cargo/registry \
        -v sqln-vault-target:/build \
        -e CARGO_TARGET_DIR=/build/target \
        -e SQLN_TEST_SYSTEM_VAULT=1 \
        -e XDG_RUNTIME_DIR=/run/user/0 \
        -w /src "$IMAGE" sh scripts/test-vault-linux.sh container
    ;;
container)
    mkdir -p "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"
    apt-get -qq update
    apt-get -qq install --no-install-recommends dbus > /dev/null
    cargo test --locked --lib creds:: 2>&1 | grep -E '^test '
    BIN="$(cargo test --locked --test vault_probe --no-run 2>&1 | sed -n 's/.*(\(.*vault_probe[^)]*\)).*/\1/p')"
    export BIN
    probe "No session bus" read,write,delete
    dbus-run-session -- sh "$0" no-service
    apt-get -qq install --no-install-recommends gnome-keyring > /dev/null
    dbus-run-session -- sh "$0" no-collection
    dbus-run-session -- sh "$0" collection
    ;;
no-service)
    probe "Session bus without a Secret Service" read,write,delete
    ;;
no-collection)
    probe "Service without a login collection" read,write,delete
    ;;
collection)
    printf spike | gnome-keyring-daemon --unlock --components=secrets > /dev/null
    probe "Unlocked collection" read,write,read,delete,delete
    SQLN_VAULT_PROBE=write "$BIN" --ignored --nocapture > /dev/null 2>&1
    dbus-send --session --print-reply --dest=org.freedesktop.secrets /org/freedesktop/secrets \
        org.freedesktop.Secret.Service.Lock array:objpath:/org/freedesktop/secrets/collection/login > /dev/null
    probe "Locked collection" read,write,delete
    ;;
esac
