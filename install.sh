#!/bin/sh
set -eu
REPOSITORY=https://github.com/qhozek/atarustia.git
DESTINATION=${ATARUSTIA_HOME:-"$HOME/atarustia"}
if ! command -v git >/dev/null 2>&1; then
    printf '%s\n' 'Install git first (Debian/Ubuntu: sudo apt install git build-essential pkg-config ca-certificates curl).'
    exit 1
fi
if ! command -v cargo >/dev/null 2>&1; then
    printf '%s\n' 'Install Rust from https://rustup.rs, reopen the shell and run this installer again.'
    exit 1
fi
if [ -e "$DESTINATION" ]; then
    printf 'Directory already exists: %s\nNo files changed. Set ATARUSTIA_HOME to another directory.\n' "$DESTINATION"
    exit 1
fi
git clone --origin origin "$REPOSITORY" "$DESTINATION"
cd "$DESTINATION"
cargo fetch --locked
printf '\nInstalled source in %s\nStarting the login wizard.\n' "$DESTINATION"
exec cargo run --locked -- "$@"
