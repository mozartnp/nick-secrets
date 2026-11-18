#!/usr/bin/env bash
#
# test_cargo: the Rust suite of the nick shell (the crate nick/), found by tests/run_all.sh
# (make test) because of its name and place. It runs, in order and with the cwd at the crate,
# cargo fmt --check, cargo clippy --all-targets -- -D warnings and cargo test, and fails at the
# first one that fails. So make test needs cargo: without it, the suite fails with a clear
# message instead of passing without testing the shell. Its guard suite is
# tests/test_cargo_suite.sh (it never runs the real cargo).
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

main() {
  if ! command -v cargo > /dev/null 2>&1; then
    echo "Erro: o cargo não foi encontrado no PATH. A suíte Rust do nick (nick/) precisa dele: instale o Rust (rustup) e rode de novo." >&2
    exit 1
  fi
  cd "$CRATE_DIR"
  echo "== cargo fmt --check =="
  cargo fmt --check
  echo "== cargo clippy --all-targets -- -D warnings =="
  cargo clippy --all-targets -- -D warnings
  echo "== cargo test =="
  cargo test
}

main "$@"
