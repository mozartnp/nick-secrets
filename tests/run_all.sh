#!/usr/bin/env bash
#
# run_all.sh: runs every test suite of the repository, one at a time, with the cwd at the
# repository root (the suites use the real LOGS_DIR, PROJECTS_DIR and cwd, so they can't run
# in parallel nor from another directory). Called by `make test` (Makefile at the root), and
# also runnable directly: bash tests/run_all.sh.
# A suite is any test_*.sh directly inside a directory named tests, outside hidden
# directories (.git/, .nick/, .claude/...): a new suite runs without touching this script or
# the Makefile. Every suite runs even if an earlier one fails; the summary lists the ones that
# failed (any exit code other than 0) and the script exits with 1. Finding no suite, or not
# being able to search every directory, is also a failure: a broken discovery must not report
# success without running anything.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# find_suites: prints, NUL-separated and in LC_ALL=C order, the path (relative to the cwd,
# with the leading ./) of every suite under the cwd. Returns the find exit code (via
# pipefail), so an unreadable directory makes it fail.
find_suites() {
  local dir file
  find . -mindepth 1 -name '.*' -prune -o -type d -name tests -print0 |
    while IFS= read -r -d '' dir; do
      # The glob only looks directly inside tests/ (a find -path '*/tests/test_*.sh' would
      # also match tests/test_dir/x.sh, since its * matches /). Without a match the glob
      # stays literal, and the -f check discards it (and also a directory named test_*.sh).
      for file in "$dir"/test_*.sh; do
        if [ -f "$file" ]; then
          printf '%s\0' "$file"
        fi
      done
    done | LC_ALL=C sort -z
}

main() {
  local suites=() failed=() suite rc find_rc=0
  cd "$ROOT_DIR"

  mapfile -d '' -t suites < <(find_suites)
  wait "$!" || find_rc=$?
  if [ "$find_rc" -ne 0 ]; then
    echo "Não deu pra procurar as suítes de teste em todos os diretórios (código $find_rc). Nenhuma suíte foi rodada." >&2
    exit 1
  fi
  if [ "${#suites[@]}" -eq 0 ]; then
    echo "Nenhuma suíte de teste encontrada (test_*.sh direto num diretório tests/, fora de diretórios ocultos)." >&2
    exit 1
  fi

  # One at a time, without stopping at the first failure. bash, and not ./, so the suite
  # doesn't need the executable bit; stdin is inherited, as when running a suite by hand.
  for suite in "${suites[@]}"; do
    echo "######## $suite"
    rc=0
    bash "$suite" || rc=$?
    if [ "$rc" -ne 0 ]; then
      failed+=("$suite (código $rc)")
    fi
    echo
  done

  if [ "${#failed[@]}" -gt 0 ]; then
    echo "${#failed[@]} de ${#suites[@]} suítes falharam:"
    printf '  - %s\n' "${failed[@]}"
    exit 1
  fi
  echo "Todas as ${#suites[@]} suítes passaram."
}

main "$@"
