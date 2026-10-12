#!/usr/bin/env bash
# Clippy the `gui` feature in an Arch container (GTK >= 4.18), for dev
# machines whose GTK is too old. Compiles only; it can't test the pen.
set -euo pipefail
cd "$(dirname "$0")/.."
CACHE="${SNOTES_DOCKER_CACHE:-$HOME/.cache/snotes-docker-cargo}"
mkdir -p "$CACHE"
docker run --rm -v "$PWD":/src -v "$CACHE":/cargo \
  -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/cargo/target -w /src archlinux:latest bash -c '
    pacman -Syu --noconfirm --needed base-devel rust gtk4 libxml2 poppler-glib alsa-lib git pkgconf clang >/dev/null
    cargo clippy --features gui --all-targets -- -D warnings'
