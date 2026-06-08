#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${FLOWRT_REMOTE_HOST:-}" || -z "${FLOWRT_REMOTE_DIR:-}" ]]; then
    printf 'FLOWRT_REMOTE_HOST/FLOWRT_REMOTE_DIR not set; skip v0.7.0 remote smoke\n'
    exit 0
fi

flowrt="${FLOWRT_BIN:-flowrt}"
bundle="${FLOWRT_REMOTE_BUNDLE:-}"
target="${FLOWRT_REMOTE_TARGET:-edge}"

if [[ -z "$bundle" || ! -d "$bundle" ]]; then
    printf 'FLOWRT_REMOTE_BUNDLE must point to an existing bundle directory\n' >&2
    exit 1
fi

ssh "$FLOWRT_REMOTE_HOST" 'flowrt --version'
"$flowrt" deploy "$bundle" \
    --host "$FLOWRT_REMOTE_HOST" \
    --target "$target" \
    --remote-dir "$FLOWRT_REMOTE_DIR"

printf 'v0.7.0 remote smoke passed: %s -> %s\n' "$bundle" "$FLOWRT_REMOTE_HOST"
