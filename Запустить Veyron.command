#!/bin/zsh
set -e

# Resolve the project from this file so Finder launches use the right assets.
cd -- "${0:A:h}"
export PATH="$HOME/.cargo/bin:$PATH"

if ! command -v cargo >/dev/null 2>&1; then
    print -u2 "Для запуска нужен Rust (cargo). Требуется Rust 1.95 или новее."
    exit 1
fi

cargo run --release --locked
