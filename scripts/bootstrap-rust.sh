#!/usr/bin/env bash
set -euo pipefail

TOOLCHAIN="1.94.0"
COMPONENTS=(rustfmt clippy)

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain "${TOOLCHAIN}"
  export PATH="${HOME}/.cargo/bin:${PATH}"
fi

rustup toolchain install "${TOOLCHAIN}" --profile minimal \
  --component "${COMPONENTS[0]}" \
  --component "${COMPONENTS[1]}"
rustup default "${TOOLCHAIN}"

rustc --version
cargo --version
