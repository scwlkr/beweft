#!/bin/sh
# Sourced by maintained operator commands. Never source the installation env file.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fail() { printf '%s\n' "$*" >&2; exit 1; }
installation() {
    [ "$#" -ge 1 ] && [ -d "$1" ] || fail 'Specify an existing installation directory.'
    INSTALL=$(CDPATH= cd -- "$1" && pwd)
    ENV_FILE="$INSTALL/installation.env"
    [ -f "$ENV_FILE" ] || fail 'Missing installation.env; run scripts/setup for a new installation.'
    command -v docker >/dev/null 2>&1 || fail 'Docker with Compose is required.'
    docker compose version >/dev/null
}
setting() { sed -n "s/^$1=//p" "$ENV_FILE"; }
dc() { docker compose --env-file "$ENV_FILE" -f "$ROOT/compose.yaml" "$@"; }
key_digest() {
    # Only the fingerprint reaches stdout; the secret passes over stdin.
    setting BEWEFT_ENCRYPTION_KEY | docker run --rm -i --network none alpine:3.22 sha256sum | cut -d ' ' -f 1
}
