#!/usr/bin/env bash
set -euo pipefail

TOOLCHAIN="1.94.0"
COMPONENTS=(rustfmt clippy)
PYTHON="${PYTHON:-python3}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ "$#" -gt 1 ]]; then
  echo "usage: $0 [X.Y.Z]" >&2
  exit 2
fi

REQUESTED_VERSION="${1:-}"

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

if [[ -n "${REQUESTED_VERSION}" ]]; then
  "${PYTHON}" "${SCRIPT_DIR}/bump_version.py" "${REQUESTED_VERSION}"
fi
