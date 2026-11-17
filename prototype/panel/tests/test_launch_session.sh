#!/usr/bin/env bash
#
# test_launch_session: testes em bash puro (sem framework) do launcher do protótipo do painel
# (prototype/panel/launch_session.sh, NS-29), que abre o claude de cada sessão pelo mesmo
# caminho do nick: jail_check + claude_settings_load + jail_agent_notice + jail_exec de lib/,
# com a jail sempre ligada. O ai-jail e o claude reais nunca são usados: os casos põem à frente
# do PATH os falsos de prototype/panel/tests/fixtures/bin/ (os mesmos do teste de integração
# Rust), e o argv esperado sai da própria lib (jail_options, jail_agent_notice e o
# permissions.json), nunca escrito à mão. Achada pelo tests/run_all.sh (make test) sozinha.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"
LAUNCHER="$(cd "$SCRIPT_DIR/.." && pwd)/launch_session.sh"
JAIL_SH="$ROOT_DIR/lib/jail/jail.sh"
PERMISSIONS_JSON="$ROOT_DIR/lib/claude/permissions.json"
FIXTURES_BIN="$SCRIPT_DIR/fixtures/bin"
# O rótulo que o launcher passa ao jail_check no lugar do caminho da configuração.
LAUNCHER_LABEL="prototype/panel (protótipo NS-29)"

failures=0

# Diretório-base temporário único da suíte: cada caso usa um subdiretório dele, e nada é
# escrito dentro do repositório.
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

# shellcheck source=SCRIPTDIR/../../../lib/tests/assert.sh
source "$ROOT_DIR/lib/tests/assert.sh"

BASH_BIN="$(command -v bash)"

# code_lines <file>: imprime as linhas de <file> que não são comentário de linha inteira.
code_lines() {
  grep -vE '^[[:space:]]*#' "$1" || true
}

# file_of <path>: o conteúdo de <path>, ou vazio se não existir.
file_of() {
  cat "$1" 2> /dev/null || true
}

# run_launcher <case_dir> [VAR=valor...]: roda o launcher com o cwd em <case_dir>/cwd, HOME em
# <case_dir>/home (vazio: as toolchains que a jail monta vêm do home, e o resultado não pode
# depender da máquina), os falsos à frente do PATH, os logs dos falsos em <case_dir> e sem as
# variáveis NICK_PANEL_*, FAKE_* e PLAYWRIGHT_BROWSERS_PATH do ambiente de quem roda a suíte
# (dentro da jail, o próprio nick define a última). Os [VAR=valor] vêm por último, então
# vencem (ex: um PATH sem o ai-jail falso). Grava rc, out e err em <case_dir>.
run_launcher() {
  local case_dir="$1" rc=0
  shift
  mkdir -p "$case_dir/cwd" "$case_dir/home"
  (cd "$case_dir/cwd" && timeout 10 env -u PLAYWRIGHT_BROWSERS_PATH -u NICK_PANEL_CLIPBOARD \
    -u NICK_PANEL_ALLOW_HOSTS -u NICK_PANEL_FORWARD_PORTS -u FAKE_JAIL_VERSION -u FAKE_CLAUDE_MODE \
    HOME="$case_dir/home" PATH="$FIXTURES_BIN:$PATH" FAKE_JAIL_LOG="$case_dir/jail.argv" \
    FAKE_CLAUDE_LOG="$case_dir/claude.argv" FAKE_CLAUDE_ENV="$case_dir/claude.env" \
    FAKE_ORDER_LOG="$case_dir/order" "$@" "$BASH_BIN" "$LAUNCHER" < /dev/null \
    > "$case_dir/out" 2> "$case_dir/err") || rc=$?
  echo "$rc" > "$case_dir/rc"
}

# lib_call <case_dir> <função> [args...]: roda uma função de lib/jail/jail.sh no mesmo ambiente
# do launcher do caso (HOME do caso, sem PLAYWRIGHT_BROWSERS_PATH; mesmo racional de
# jail_expected_argv em auto_scrum/tests/test_auto_scrum.sh) e imprime a saída.
lib_call() {
  local case_dir="$1"
  shift
  # Aspas simples intencionais: os $1/$@ são do bash -c, não daqui.
  # shellcheck disable=SC2016
  env -u PLAYWRIGHT_BROWSERS_PATH HOME="$case_dir/home" "$BASH_BIN" -c 'source "$1"; shift; "$@"' _ "$JAIL_SH" "$@"
}

# expected_jail_argv <case_dir> <clipboard> <hosts> <ports>: o argv que o ai-jail falso deve
# receber, um por linha: as opções de jail_options, o "--", e o claude com as regras de deny/ask
# inline e o aviso da jail.
expected_jail_argv() {
  local case_dir="$1" clipboard="$2" hosts="$3" ports="$4" options
  mapfile -t options < <(lib_call "$case_dir" jail_options "$clipboard" "$hosts" "$ports")
  printf '%s\n' "${options[@]}" -- claude "$(expected_claude_argv "$case_dir" "$hosts" "$ports")"
}

# expected_claude_argv <case_dir> <hosts> <ports>: o argv que o claude falso deve receber.
expected_claude_argv() {
  local case_dir="$1" hosts="$2" ports="$3"
  printf '%s\n' --settings "$(cat "$PERMISSIONS_JSON")" --append-system-prompt \
    "$(lib_call "$case_dir" jail_agent_notice "$hosts" "$ports")"
}

# claude_command_lines <file>: imprime as linhas de código de <file> que citam claude ou ai-jail
# como palavra solta (um comando), fora a linha do jail_exec "true", que é o único lançamento
# permitido. claude_settings_load, lib/claude/claude.sh e afins não contam.
claude_command_lines() {
  code_lines "$1" | grep -E '(^|[^[:alnum:]_./-])(claude|ai-jail)([^[:alnum:]_./-]|$)' |
    grep -vE '^[[:space:]]*jail_exec "true" ' || true
}

hosts_example="docs.example.com mcp.atlassian.com"
ports_example="5050 54320"

echo "== launcher: argv completo dentro da jail (calculado pela própria lib) =="
case_dir="$TEST_TMP/argv_brando"
run_launcher "$case_dir" NICK_PANEL_CLIPBOARD=brando NICK_PANEL_ALLOW_HOSTS="$hosts_example" \
  NICK_PANEL_FORWARD_PORTS="$ports_example"
assert_eq "0" "$(file_of "$case_dir/rc")" "brando + hosts + portas: o launcher sai com 0 (o claude falso saiu com 0)"
options_count="$(lib_call "$case_dir" jail_options brando "$hosts_example" "$ports_example" | grep -c .)"
if [ "$options_count" -gt 13 ]; then
  echo "  OK: guarda: jail_options carregou da lib e gerou $options_count opções (mais que as 13 da base)"
else
  echo "  FAIL: guarda: jail_options gerou só $options_count opções (a lib não carregou?)"
  failures=$((failures + 1))
fi
assert_eq "$(expected_jail_argv "$case_dir" brando "$hosts_example" "$ports_example")" \
  "$(file_of "$case_dir/jail.argv")" "o ai-jail recebe jail_options brando + hosts + portas, --, claude --settings <permissions.json> --append-system-prompt <aviso>"
assert_eq "$(expected_claude_argv "$case_dir" "$hosts_example" "$ports_example")" \
  "$(file_of "$case_dir/claude.argv")" "o claude recebe só --settings e --append-system-prompt (sem --permission-mode nem prompt)"
jail_argv="$(file_of "$case_dir/jail.argv")"
for expected_opt in --terminal-passthrough --display --allow-host docs.example.com mcp.atlassian.com --forward-port 54320; do
  assert_contains "$jail_argv" "$expected_opt" "o argv do ai-jail tem $expected_opt"
done
assert_contains "$(file_of "$case_dir/out")" "Jail (ai-jail): ativa" "o aviso de jail_print_status aparece no terminal da sessão"
assert_contains "$(file_of "$case_dir/out")" "modo de clipboard brando" "o aviso mostra o nível escolhido"
assert_contains "$(file_of "$case_dir/claude.env")" "PWD=$case_dir/cwd" "o claude roda no cwd de quem abriu o launcher (o launcher não troca de diretório)"

echo
echo "== launcher: sem NICK_PANEL_*, o nível padrão (medio) e nenhum host ou porta extra =="
case_dir="$TEST_TMP/argv_default"
run_launcher "$case_dir"
assert_eq "0" "$(file_of "$case_dir/rc")" "sem variáveis: o launcher sai com 0"
assert_eq "$(expected_jail_argv "$case_dir" "" "" "")" "$(file_of "$case_dir/jail.argv")" \
  "sem variáveis: o ai-jail recebe jail_options \"\" \"\" \"\" + -- claude ..."
assert_contains "$(file_of "$case_dir/jail.argv")" "--terminal-passthrough" "sem variáveis: o nível médio (--terminal-passthrough)"
assert_not_contains "$(file_of "$case_dir/jail.argv")" "--display" "sem variáveis: sem --display (não é o brando)"
assert_contains "$(file_of "$case_dir/out")" "modo de clipboard médio" "sem variáveis: o aviso mostra o nível médio"

echo
echo "== launcher: o claude só roda via jail =="
case_dir="$TEST_TMP/order"
run_launcher "$case_dir"
assert_eq "ai-jail"$'\n'"claude" "$(file_of "$case_dir/order")" "o ai-jail é chamado e só então o claude (dentro dele)"
assert_exists "$case_dir/claude.argv" "o claude falso foi chamado"

# PATH mínimo, sem nenhum ai-jail (o real pode estar em /usr/bin): só o que o launcher, a lib e
# os falsos usam. O controle positivo, logo abaixo, prova que esse PATH basta.
min_bin="$TEST_TMP/min_bin"
mkdir -p "$min_bin"
for tool in bash dirname uname mktemp rm cat env; do
  ln -s "$(command -v "$tool")" "$min_bin/$tool"
done
ln -s "$FIXTURES_BIN/claude" "$min_bin/claude"
min_with_jail_bin="$TEST_TMP/min_with_jail_bin"
cp -a "$min_bin" "$min_with_jail_bin"
ln -s "$FIXTURES_BIN/ai-jail" "$min_with_jail_bin/ai-jail"

case_dir="$TEST_TMP/no_jail"
run_launcher "$case_dir" PATH="$min_bin"
assert_eq "1" "$(file_of "$case_dir/rc")" "sem ai-jail no PATH: o launcher sai com 1"
assert_contains "$(file_of "$case_dir/err")" "a jail (ai-jail) não funciona nesta máquina" "sem ai-jail: o erro do jail_check"
assert_contains "$(file_of "$case_dir/err")" "ai-jail não encontrado no PATH." "sem ai-jail: o motivo"
assert_contains "$(file_of "$case_dir/err")" "$LAUNCHER_LABEL" "sem ai-jail: o erro cita o rótulo do protótipo no lugar da configuração"
assert_not_exists "$case_dir/claude.argv" "sem ai-jail: o claude falso não é chamado"
case_dir="$TEST_TMP/no_jail_control"
run_launcher "$case_dir" PATH="$min_with_jail_bin"
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: o mesmo PATH mínimo com o ai-jail falso: o launcher sai com 0"
assert_exists "$case_dir/claude.argv" "controle positivo: com o ai-jail falso, o claude falso é chamado"

echo
echo "== launcher: versão do ai-jail fora da faixa =="
case_dir="$TEST_TMP/old_version"
run_launcher "$case_dir" FAKE_JAIL_VERSION=2.14.0
assert_eq "1" "$(file_of "$case_dir/rc")" "ai-jail 2.14.0: o launcher sai com 1"
assert_contains "$(file_of "$case_dir/err")" "versão do ai-jail não suportada: encontrada 2.14.0" "ai-jail 2.14.0: o erro de versão do jail_check"
assert_not_exists "$case_dir/claude.argv" "ai-jail 2.14.0: o claude falso não é chamado"
assert_not_exists "$case_dir/jail.argv" "ai-jail 2.14.0: nenhum lançamento pelo ai-jail"

echo
echo "== launcher: nível de clipboard inválido =="
case_dir="$TEST_TMP/bad_clipboard"
run_launcher "$case_dir" NICK_PANEL_CLIPBOARD=xyz
assert_eq "1" "$(file_of "$case_dir/rc")" "NICK_PANEL_CLIPBOARD=xyz: o launcher sai com 1"
assert_contains "$(file_of "$case_dir/err")" "JAIL_CLIPBOARD=\"xyz\" em $LAUNCHER_LABEL não é um valor aceito." "NICK_PANEL_CLIPBOARD=xyz: o erro do jail_check"
assert_not_exists "$case_dir/claude.argv" "NICK_PANEL_CLIPBOARD=xyz: o claude falso não é chamado"
assert_not_exists "$case_dir/jail.argv" "NICK_PANEL_CLIPBOARD=xyz: nenhum lançamento pelo ai-jail"

# Regras ilegíveis (permissions.json ausente): coberto por lib/tests/test_claude.sh. De fora não
# dá pra apontar NICK_CLAUDE_PERMISSIONS_FILE pra outro arquivo (a lib a atribui ao ser
# carregada), e o launcher não ganha uma variável nova só pra isso (plano do NS-29).

echo
echo "== launcher: estrutura =="
launcher_code="$(code_lines "$LAUNCHER")"
assert_contains "$(head -n 1 "$LAUNCHER")" "#!/usr/bin/env bash" "o launcher é um script bash (shebang)"
assert_contains "$launcher_code" "set -euo pipefail" "o launcher tem set -euo pipefail"
assert_contains "$launcher_code" 'jail_check "true" ' "jail_check com \"true\" literal (a jail é sempre ligada)"
assert_contains "$launcher_code" 'jail_exec "true" ' "jail_exec com \"true\" literal (a jail é sempre ligada)"
assert_contains "$launcher_code" 'jail_print_status "true" ' "jail_print_status com \"true\" literal"
assert_eq "" "$(printf '%s\n' "$launcher_code" | grep -E 'jail_(check|exec|print_status) "\$' || true)" "nenhuma chamada da jail com o liga/desliga vindo de variável"
assert_eq "" "$(claude_command_lines "$LAUNCHER")" "nenhuma linha de código chama claude ou ai-jail fora do jail_exec \"true\""
sample="$TEST_TMP/sample.sh"
# Aspas simples intencionais nas duas linhas abaixo e na lista de source: é texto de código
# bash, comparado literalmente.
# shellcheck disable=SC2016
printf '%s\n' '# claude no comentário não conta' 'claude_settings_load settings' 'exec claude --settings "$x"' 'ai-jail -- claude' '  jail_exec "true" "" "" "" claude "${args[@]}"' > "$sample"
# shellcheck disable=SC2016
assert_eq 'exec claude --settings "$x"'$'\n''ai-jail -- claude' "$(claude_command_lines "$sample")" "controle positivo: o filtro acha claude/ai-jail chamados direto e ignora comentário, claude_* e o jail_exec \"true\""
assert_not_contains "$launcher_code" "--permission-mode" "o launcher não passa --permission-mode (modo padrão)"
assert_not_contains "$launcher_code" "config.sh" "o launcher não carrega lib/config.sh"
assert_not_contains "$launcher_code" "nick.conf" "o launcher não lê .nick/nick.conf"
# shellcheck disable=SC2016
assert_eq 'source "$SCRIPT_DIR/../../lib/jail/jail.sh"'$'\n''source "$SCRIPT_DIR/../../lib/claude/claude.sh"' \
  "$(printf '%s\n' "$launcher_code" | grep -E '^[[:space:]]*(source|\.)[[:space:]]' || true)" "as únicas linhas source são as de lib/jail e lib/claude, nessa ordem"

echo
echo "== shellcheck -x no launcher e nesta suíte =="
if command -v shellcheck > /dev/null 2>&1; then
  for checked in "$LAUNCHER" "$SCRIPT_DIR/test_launch_session.sh" "$FIXTURES_BIN/ai-jail" "$FIXTURES_BIN/claude"; do
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
