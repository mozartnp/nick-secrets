#!/usr/bin/env bash
#
# test_cargo_suite: testes em bash puro (sem framework) da suíte Rust da casca nick
# (nick/tests/test_cargo.sh), a peça que faz o make test rodar o cargo fmt --check, o
# cargo clippy e o cargo test do crate nick/. Nunca roda o cargo real (mesmo racional de
# tests/test_run_all.sh: seria caro e repetiria a própria suíte Rust, que o make test já roda):
# cada caso põe no PATH um cargo falso, gerado no TEST_TMP, que grava o argv e o cwd num log e
# falha no subcomando pedido por FAKE_CARGO_FAIL.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$ROOT_DIR/nick"
CARGO_SUITE="$CRATE_DIR/tests/test_cargo.sh"
FAKE_INIT="$CRATE_DIR/tests/fixtures/bin/nick-init"
# The fake $EDITOR of the stage forms (NS-35).
FAKE_EDITOR="$CRATE_DIR/tests/fixtures/bin/editor"

failures=0

# Diretório-base temporário único da suíte: cada caso usa um subdiretório dele, e nada é
# escrito dentro do repositório.
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

# shellcheck source=SCRIPTDIR/../lib/tests/assert.sh
source "$SCRIPT_DIR/../lib/tests/assert.sh"

BASH_BIN="$(command -v bash)"

# code_lines <file>: imprime as linhas de <file> que não são comentário de linha inteira.
code_lines() {
  grep -vE '^[[:space:]]*#' "$1" || true
}

# file_of <path>: o conteúdo de <path>, ou vazio se não existir.
file_of() {
  cat "$1" 2> /dev/null || true
}

# PATH mínimo, sem nenhum cargo (o real pode estar em ~/.cargo/bin ou em /usr/bin): só o que a
# suíte usa antes de chamar o cargo. O controle positivo prova que esse PATH basta.
MIN_BIN="$TEST_TMP/min_bin"
mkdir -p "$MIN_BIN"
for tool in bash dirname; do
  ln -s "$(command -v "$tool")" "$MIN_BIN/$tool"
done

# O cargo falso: grava "<argv>|<cwd físico>" em FAKE_CARGO_LOG e sai com 3 quando o
# subcomando (1º argumento) é o de FAKE_CARGO_FAIL.
FAKE_BIN="$TEST_TMP/fake_bin"
mkdir -p "$FAKE_BIN"
cat > "$FAKE_BIN/cargo" <<'FAKE'
#!/usr/bin/env bash
printf '%s|%s\n' "$*" "$(pwd -P)" >> "$FAKE_CARGO_LOG"
if [ -n "${FAKE_CARGO_FAIL:-}" ] && [ "$1" = "$FAKE_CARGO_FAIL" ]; then
  exit 3
fi
exit 0
FAKE
chmod +x "$FAKE_BIN/cargo"
MIN_WITH_CARGO_BIN="$TEST_TMP/min_with_cargo_bin"
cp -a "$MIN_BIN" "$MIN_WITH_CARGO_BIN"
ln -s "$FAKE_BIN/cargo" "$MIN_WITH_CARGO_BIN/cargo"

# run_suite <case_dir> [VAR=valor...]: roda nick/tests/test_cargo.sh com o cwd em
# <case_dir>/cwd (fora do crate, de propósito), o log do cargo falso em <case_dir>/cargo.log e
# sem FAKE_CARGO_FAIL do ambiente. Os [VAR=valor] vêm por último, então vencem (ex: o PATH).
# Grava rc, out e err em <case_dir>.
run_suite() {
  local case_dir="$1" rc=0
  shift
  mkdir -p "$case_dir/cwd"
  (cd "$case_dir/cwd" && timeout 10 env -u FAKE_CARGO_FAIL FAKE_CARGO_LOG="$case_dir/cargo.log" \
    PATH="$MIN_WITH_CARGO_BIN" "$@" "$BASH_BIN" "$CARGO_SUITE" < /dev/null \
    > "$case_dir/out" 2> "$case_dir/err") || rc=$?
  echo "$rc" > "$case_dir/rc"
}

crate_physical="$(cd "$CRATE_DIR" && pwd -P)"

echo "== sem cargo no PATH =="
case_dir="$TEST_TMP/no_cargo"
run_suite "$case_dir" PATH="$MIN_BIN"
assert_eq "1" "$(file_of "$case_dir/rc")" "sem cargo no PATH: a suíte sai com 1"
assert_eq "Erro: o cargo não foi encontrado no PATH. A suíte Rust do nick (nick/) precisa dele: instale o Rust (rustup) e rode de novo." "$(file_of "$case_dir/err")" "sem cargo: a mensagem exata no stderr"
assert_eq "" "$(file_of "$case_dir/out")" "sem cargo: nada no stdout"
case_dir="$TEST_TMP/no_cargo_control"
run_suite "$case_dir" PATH="$MIN_WITH_CARGO_BIN"
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: o mesmo PATH mínimo com o cargo falso: a suíte sai com 0"

echo
echo "== as três chamadas do cargo, em ordem, com o cwd no crate =="
case_dir="$TEST_TMP/calls"
run_suite "$case_dir"
assert_eq "0" "$(file_of "$case_dir/rc")" "com o cargo falso passando: a suíte sai com 0"
assert_eq "fmt --check|$crate_physical
clippy --all-targets -- -D warnings|$crate_physical
test|$crate_physical" "$(file_of "$case_dir/cargo.log")" "cargo fmt --check, cargo clippy --all-targets -- -D warnings e cargo test, nessa ordem, no crate nick/ (de outro cwd)"

echo
echo "== uma chamada que falha faz a suíte falhar =="
for failing in fmt clippy test; do
  case_dir="$TEST_TMP/fail_$failing"
  run_suite "$case_dir" FAKE_CARGO_FAIL="$failing"
  assert_eq "true" "$([ "$(file_of "$case_dir/rc")" != "0" ] && echo true || echo false)" "cargo $failing falhando: a suíte sai com código ≠ 0 (obtido: $(file_of "$case_dir/rc"))"
  assert_contains "$(file_of "$case_dir/cargo.log")" "$failing" "cargo $failing falhando: ele foi chamado"
done

echo
echo "== estrutural =="
assert_exists "$CARGO_SUITE" "a suíte fica em nick/tests/test_cargo.sh (test_*.sh direto num tests/: o tests/run_all.sh a acha sozinho)"
assert_eq "#!/usr/bin/env bash" "$(head -n 1 "$CARGO_SUITE" 2> /dev/null || true)" "nick/tests/test_cargo.sh começa com o shebang #!/usr/bin/env bash"
assert_eq "set -euo pipefail" "$(code_lines "$CARGO_SUITE" | grep -m 1 -v '^$' || true)" "a 1ª linha de código de nick/tests/test_cargo.sh é set -euo pipefail"
assert_eq "" "$(code_lines "$CARGO_SUITE" | grep -n '\[\[' || true)" "nick/tests/test_cargo.sh usa [ ], não [[ ]]"
assert_eq "true" "$([ -x "$CARGO_SUITE" ] && [ -x "$SCRIPT_DIR/test_cargo_suite.sh" ] && echo true || echo false)" "as duas suítes têm bit de execução"

echo
echo "== shellcheck -x nas suítes, no init falso e no editor falso =="
if command -v shellcheck > /dev/null 2>&1; then
  for checked in "$CARGO_SUITE" "$SCRIPT_DIR/test_cargo_suite.sh" "$FAKE_INIT" "$FAKE_EDITOR"; do
    sc_rc=0
    sc_out="$(shellcheck -x "$checked" 2>&1)" || sc_rc=$?
    assert_eq "0" "$sc_rc" "shellcheck -x limpo: ${checked#"$ROOT_DIR"/}${sc_out:+ — $sc_out}"
  done
else
  echo "  PULADO: shellcheck não instalado"
fi

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
