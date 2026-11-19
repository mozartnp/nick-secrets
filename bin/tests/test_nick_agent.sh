#!/usr/bin/env bash
#
# test_nick_agent: testes em bash puro (sem framework) do bin/nick-agent (NS-33), o launcher
# que a casca nick usa pra abrir o claude de um agente: carrega o .nick/nick.conf do cwd pelo
# lib/config/ e lança pelo mesmo caminho do auto_scrum.sh (jail_check, claude_settings_load,
# jail_agent_notice e jail_exec do lib/). O ai-jail e o claude reais nunca são usados: os casos
# põem à frente do PATH os falsos de bin/tests/fixtures/bin/ (os mesmos dos testes Rust da
# casca), e o argv esperado sai da própria lib (jail_options, jail_mcp_hosts, jail_agent_notice
# e o permissions.json), nunca escrito à mão. Cada caso roda com o cwd num projeto temporário:
# nada é escrito no repositório. Achada pelo tests/run_all.sh (make test) sozinha.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
LAUNCHER="$ROOT_DIR/bin/nick-agent"
NICK_INIT="$ROOT_DIR/bin/nick-init"
AUTO_SCRUM_SH="$ROOT_DIR/auto_scrum/auto_scrum.sh"
CONFIG_SH="$ROOT_DIR/lib/config/config.sh"
KEYS_SH="$ROOT_DIR/lib/config/keys.sh"
JAIL_SH="$ROOT_DIR/lib/jail/jail.sh"
PERMISSIONS_JSON="$ROOT_DIR/lib/claude/permissions.json"
FIXTURES_BIN="$SCRIPT_DIR/fixtures/bin"

failures=0

# Diretório-base temporário único da suíte: cada caso usa um subdiretório dele, e nada é
# escrito dentro do repositório.
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

# shellcheck source=SCRIPTDIR/../../lib/tests/assert.sh
source "$ROOT_DIR/lib/tests/assert.sh"

BASH_BIN="$(command -v bash)"
# Um UUID e um nome de exemplo, como a casca passa.
SESSION_ID="0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60"
AGENT_NAME="proj · Dev · NS-1"
USAGE="Uso: nick-agent --check | --session-id <uuid> --name <nome>"

# code_lines <file>: imprime as linhas de <file> que não são comentário de linha inteira.
code_lines() {
  grep -vE '^[[:space:]]*#' "$1" || true
}

# file_of <path>: o conteúdo de <path>, ou vazio se não existir.
file_of() {
  cat "$1" 2> /dev/null || true
}

# config_lines [KEY="valor"...]: um .nick/nick.conf completo (todas as chaves registradas, vazias,
# então nenhum aviso de chave ausente), com as linhas dadas no lugar das chaves delas.
config_lines() {
  local keys line key skip
  keys="$("$BASH_BIN" -c "source '$CONFIG_SH'; source '$KEYS_SH'; config_register_nick_keys; printf '%s\n' \"\${CONFIG_KEYS[@]}\"")"
  while IFS= read -r key; do
    skip=0
    for line in "$@"; do
      [ "${line%%=*}" = "$key" ] && skip=1
    done
    [ "$skip" -eq 1 ] || printf '%s=""\n' "$key"
  done <<< "$keys"
  if [ "$#" -gt 0 ]; then
    printf '%s\n' "$@"
  fi
}

# new_project <case_dir> [KEY="valor"...]: cria <case_dir>/proj com o .nick/nick.conf de
# config_lines e <case_dir>/home vazio; imprime o caminho do projeto.
new_project() {
  local case_dir="$1"
  shift
  mkdir -p "$case_dir/proj/.nick" "$case_dir/home"
  config_lines "$@" > "$case_dir/proj/.nick/nick.conf"
  printf '%s' "$case_dir/proj"
}

# run_agent <case_dir> <cwd> [VAR=valor...] -- [args...]: roda bash <launcher> [args] com o cwd
# em <cwd>, HOME em <case_dir>/home (vazio: as toolchains que a jail monta vêm do home, e o
# resultado não pode depender da máquina), os falsos à frente do PATH, os logs dos falsos em
# <case_dir> (fora do projeto) e sem as variáveis FAKE_*, PLAYWRIGHT_BROWSERS_PATH e as chaves
# da jail do ambiente de quem roda a suíte (dentro da jail, o próprio nick define a
# PLAYWRIGHT_BROWSERS_PATH; mesmo racional de jail_expected_argv). Os [VAR=valor] vêm por
# último, então vencem (ex: um PATH sem o ai-jail falso). Grava rc, out e err em <case_dir>.
run_agent() {
  local case_dir="$1" cwd="$2" rc=0 env_args=()
  shift 2
  while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
    env_args+=("$1")
    shift
  done
  [ "$#" -gt 0 ] && shift
  mkdir -p "$case_dir/home"
  (cd "$cwd" && timeout 10 env -u PLAYWRIGHT_BROWSERS_PATH -u FAKE_JAIL_VERSION -u FAKE_CLAUDE_MODE \
    -u JAIL_ENABLED -u JAIL_CLIPBOARD -u JAIL_ALLOW_HOSTS -u JAIL_FORWARD_PORTS \
    HOME="$case_dir/home" PATH="$FIXTURES_BIN:$PATH" FAKE_JAIL_LOG="$case_dir/jail.argv" \
    FAKE_CLAUDE_LOG="$case_dir/claude.argv" FAKE_CLAUDE_ENV="$case_dir/claude.env" \
    FAKE_ORDER_LOG="$case_dir/order" "${env_args[@]}" "$BASH_BIN" "$LAUNCHER" "$@" < /dev/null \
    > "$case_dir/out" 2> "$case_dir/err") || rc=$?
  echo "$rc" > "$case_dir/rc"
}

# env_pwd <case_dir>: a linha PWD= exata do env que o claude falso gravou (o OLDPWD= também tem o
# caminho, então uma busca por substring passaria com o cwd errado).
env_pwd() {
  grep '^PWD=' "$1/claude.env" 2> /dev/null || true
}

# lib_call <case_dir> <função> [args...]: roda uma função de lib/jail/jail.sh no mesmo ambiente
# do launcher do caso (HOME do caso, sem PLAYWRIGHT_BROWSERS_PATH) e imprime a saída.
lib_call() {
  local case_dir="$1"
  shift
  # Aspas simples intencionais: os $1/$@ são do bash -c, não daqui.
  # shellcheck disable=SC2016
  env -u PLAYWRIGHT_BROWSERS_PATH HOME="$case_dir/home" "$BASH_BIN" -c 'source "$1"; shift; "$@"' _ "$JAIL_SH" "$@"
}

# hosts_of <case_dir> <jira> <sentry> <allow_hosts>: os hosts extras como o launcher (e o
# auto_scrum.sh) os monta: os dos MCPs (jail_mcp_hosts), um espaço e os do JAIL_ALLOW_HOSTS.
hosts_of() {
  printf '%s %s' "$(lib_call "$1" jail_mcp_hosts "$2" "$3")" "$4"
}

# expected_claude_argv <case_dir> <jail> <hosts> <ports>: o argv que o claude falso deve
# receber, um por linha: as regras de deny/ask inline, o aviso da jail (só com <jail> "true"),
# --session-id e --name. Sem --permission-mode e sem prompt.
expected_claude_argv() {
  local case_dir="$1" jail="$2" hosts="$3" ports="$4" args=()
  args=(--settings "$(cat "$PERMISSIONS_JSON")")
  if [ "$jail" = "true" ]; then
    args+=(--append-system-prompt "$(lib_call "$case_dir" jail_agent_notice "$hosts" "$ports")")
  fi
  args+=(--session-id "$SESSION_ID" --name "$AGENT_NAME")
  printf '%s\n' "${args[@]}"
}

# expected_jail_argv <case_dir> <clipboard> <hosts> <ports>: o argv que o ai-jail falso deve
# receber: as opções de jail_options, o "--" e o claude com o argv de expected_claude_argv.
expected_jail_argv() {
  local case_dir="$1" clipboard="$2" hosts="$3" ports="$4" options
  mapfile -t options < <(lib_call "$case_dir" jail_options "$clipboard" "$hosts" "$ports")
  printf '%s\n' "${options[@]}" -- claude
  expected_claude_argv "$case_dir" true "$hosts" "$ports"
}

# tree_snapshot <dir>: retrato recursivo de <dir> (caminho, tipo, tamanho, mtime e o md5 de cada
# arquivo), pra provar que nada mudou nele.
tree_snapshot() {
  (cd "$1" && find . -print0 | LC_ALL=C sort -z | xargs -0 stat -c '%n %F %s %Y' &&
    find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r md5sum)
}

# claude_command_lines <file>: imprime as linhas de código de <file> que citam claude ou ai-jail
# como palavra solta (um comando), fora a linha do jail_exec com o JAIL_ENABLED do projeto, que é
# o único lançamento permitido. claude_settings_load, lib/claude/claude.sh e afins não contam.
claude_command_lines() {
  # Aspas simples intencionais: o $JAIL_ENABLED é texto do código procurado.
  # shellcheck disable=SC2016
  code_lines "$1" | grep -E '(^|[^[:alnum:]_./-])(claude|ai-jail)([^[:alnum:]_./-]|$)' |
    grep -vE '^[[:space:]]*jail_exec "\$JAIL_ENABLED" ' || true
}

hosts_example="docs.example.com"
ports_example="5050 54320"
jail_on_conf=(JAIL_ENABLED='"true"' JAIL_CLIPBOARD='"brando"' JIRA_ENABLED='"true"' SENTRY_ENABLED='"true"' "JAIL_ALLOW_HOSTS=\"$hosts_example\"" "JAIL_FORWARD_PORTS=\"$ports_example\"")

echo "== lançamento com a jail ligada: argv exato do ai-jail e do claude (calculado pela lib) =="
case_dir="$TEST_TMP/jail_on"
proj="$(new_project "$case_dir" "${jail_on_conf[@]}")"
run_agent "$case_dir" "$proj" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
assert_eq "0" "$(file_of "$case_dir/rc")" "jail ligada: o launcher sai com 0 (o claude falso saiu com 0)"
hosts="$(hosts_of "$case_dir" true true "$hosts_example")"
assert_eq "mcp.atlassian.com mcp.sentry.dev $hosts_example" "$hosts" "guarda: os hosts saem de jail_mcp_hosts + JAIL_ALLOW_HOSTS"
options_count="$(lib_call "$case_dir" jail_options brando "$hosts" "$ports_example" | grep -c .)"
if [ "$options_count" -gt 13 ]; then
  echo "  OK: guarda: jail_options carregou da lib e gerou $options_count opções (mais que as 13 da base)"
else
  echo "  FAIL: guarda: jail_options gerou só $options_count opções (a lib não carregou?)"
  failures=$((failures + 1))
fi
assert_eq "$(expected_jail_argv "$case_dir" brando "$hosts" "$ports_example")" "$(file_of "$case_dir/jail.argv")" \
  "o ai-jail recebe jail_options (brando, MCPs + JAIL_ALLOW_HOSTS, portas), --, claude --settings <regras> --append-system-prompt <aviso> --session-id <uuid> --name <nome>"
assert_eq "$(expected_claude_argv "$case_dir" true "$hosts" "$ports_example")" "$(file_of "$case_dir/claude.argv")" \
  "o claude falso recebe exatamente o que vem depois do --"
jail_argv="$(file_of "$case_dir/jail.argv")"
for expected_opt in --display --allow-host mcp.atlassian.com mcp.sentry.dev "$hosts_example" --forward-port 54320; do
  assert_contains "$jail_argv" "$expected_opt" "o argv do ai-jail tem $expected_opt"
done
assert_eq "ai-jail -- true"$'\n'"ai-jail"$'\n'"claude" "$(file_of "$case_dir/order")" "a checagem (-- true), depois o lançamento pelo ai-jail e só então o claude"
assert_contains "$(file_of "$case_dir/out")" "Jail (ai-jail): ativa" "o aviso de jail_print_status aparece no terminal do agente"
assert_eq "PWD=$proj" "$(env_pwd "$case_dir")" "o claude roda com o cwd no projeto"
assert_eq "$AGENT_NAME" "$(tail -n 1 "$case_dir/claude.argv" 2> /dev/null || true)" "o último argumento do claude é o nome (nenhum prompt)"
assert_not_contains "$(file_of "$case_dir/claude.argv")" "--permission-mode" "o claude não recebe --permission-mode"

echo
echo "== lançamento com a jail desligada: o ai-jail não é chamado =="
case_dir="$TEST_TMP/jail_off"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"' JIRA_ENABLED='"true"' "JAIL_ALLOW_HOSTS=\"$hosts_example\"")"
run_agent "$case_dir" "$proj" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
assert_eq "0" "$(file_of "$case_dir/rc")" "jail desligada: o launcher sai com 0"
assert_eq "$(expected_claude_argv "$case_dir" false "" "")" "$(file_of "$case_dir/claude.argv")" \
  "jail desligada: claude --settings <regras> --session-id <uuid> --name <nome>"
assert_not_exists "$case_dir/jail.argv" "jail desligada: nenhum lançamento pelo ai-jail"
assert_eq "claude" "$(file_of "$case_dir/order")" "jail desligada: o ai-jail não é chamado nem pra checar"
assert_contains "$(file_of "$case_dir/out")" "Jail (ai-jail): desligada" "jail desligada: o aviso de jail_print_status"
assert_eq "PWD=$proj" "$(env_pwd "$case_dir")" "jail desligada: o claude roda com o cwd no projeto"
assert_eq "$AGENT_NAME" "$(tail -n 1 "$case_dir/claude.argv" 2> /dev/null || true)" "jail desligada: o último argumento é o nome (nenhum prompt)"
for jail_value in TRUE '' sim; do
  case_dir="$TEST_TMP/jail_value_${jail_value:-vazio}"
  proj="$(new_project "$case_dir" "JAIL_ENABLED=\"$jail_value\"")"
  run_agent "$case_dir" "$proj" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
  assert_not_exists "$case_dir/jail.argv" "JAIL_ENABLED=\"$jail_value\": só \"true\" exato liga a jail"
  assert_eq "$(expected_claude_argv "$case_dir" false "" "")" "$(file_of "$case_dir/claude.argv")" "JAIL_ENABLED=\"$jail_value\": o argv da jail desligada"
done

echo
echo "== dois projetos: cada execução usa a configuração do próprio cwd =="
case_a="$TEST_TMP/two_a"
case_b="$TEST_TMP/two_b"
proj_a="$(new_project "$case_a" JAIL_ENABLED='"true"' "JAIL_ALLOW_HOSTS=\"a.example.com\"")"
proj_b="$(new_project "$case_b" JAIL_ENABLED='"false"' "JAIL_ALLOW_HOSTS=\"b.example.com\"")"
run_agent "$case_a" "$proj_a" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
run_agent "$case_b" "$proj_b" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
assert_eq "0|0" "$(file_of "$case_a/rc")|$(file_of "$case_b/rc")" "os dois projetos: o launcher sai com 0"
assert_eq "$(expected_jail_argv "$case_a" "" " a.example.com" "")" "$(file_of "$case_a/jail.argv")" "projeto A (jail ligada): o ai-jail recebe os hosts de A"
assert_not_contains "$(file_of "$case_a/jail.argv")" "b.example.com" "projeto A: nada da configuração de B"
assert_not_exists "$case_b/jail.argv" "projeto B (jail desligada): o ai-jail não é chamado"
assert_eq "$(expected_claude_argv "$case_b" false "" "")" "$(file_of "$case_b/claude.argv")" "projeto B: o argv da jail desligada"
assert_eq "PWD=$proj_a" "$(env_pwd "$case_a")" "projeto A: o claude roda no cwd de A"
assert_eq "PWD=$proj_b" "$(env_pwd "$case_b")" "projeto B: o claude roda no cwd de B"

echo
echo "== --check: só valida, imprime jail=true/jail=false e não grava nada =="
for check_case in "true|jail=true" "false|jail=false" "TRUE|jail=false" "|jail=false"; do
  IFS='|' read -r check_value check_expected <<< "$check_case"
  case_dir="$TEST_TMP/check_${check_value:-vazio}"
  proj="$(new_project "$case_dir" "JAIL_ENABLED=\"$check_value\"" JIRA_ENABLED='"true"' "JAIL_ALLOW_HOSTS=\"$hosts_example\"")"
  # Sem .nick/.gitignore, troca/ nem docs/: o lançamento os criaria (config_dir_prepare).
  before="$(tree_snapshot "$proj")"
  run_agent "$case_dir" "$proj" -- --check
  label="--check com JAIL_ENABLED=\"$check_value\""
  assert_eq "0" "$(file_of "$case_dir/rc")" "$label: sai com 0"
  assert_eq "$check_expected" "$(file_of "$case_dir/out")" "$label: o stdout é só a linha $check_expected"
  if [ "$check_value" = "TRUE" ]; then
    assert_contains "$(file_of "$case_dir/err")" "valor \"TRUE\" inválido para JAIL_ENABLED" "$label: o aviso de valor inválido do config_load vai pro stderr, sem mudar o resultado"
  else
    assert_eq "" "$(file_of "$case_dir/err")" "$label: nada no stderr (configuração completa)"
  fi
  assert_not_exists "$case_dir/claude.argv" "$label: o claude falso não é chamado"
  assert_not_exists "$case_dir/jail.argv" "$label: nenhum lançamento pelo ai-jail"
  if [ "$check_value" = "true" ]; then
    assert_eq "ai-jail -- true" "$(file_of "$case_dir/order")" "$label: o ai-jail só recebe a checagem -- true"
  else
    assert_eq "" "$(file_of "$case_dir/order")" "$label: o ai-jail não é chamado"
  fi
  assert_eq "$before" "$(tree_snapshot "$proj")" "$label: nenhum arquivo do projeto muda"
  assert_not_exists "$proj/.nick/.gitignore" "$label: sem .nick/.gitignore criado"
done
case_dir="$TEST_TMP/check_warning"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
grep -v '^STACK_DESCRIPTION=' "$proj/.nick/nick.conf" > "$case_dir/conf" && mv "$case_dir/conf" "$proj/.nick/nick.conf"
run_agent "$case_dir" "$proj" -- --check
assert_eq "0|jail=false" "$(file_of "$case_dir/rc")|$(file_of "$case_dir/out")" "--check com chave ausente: sai com 0 e o stdout continua só jail=false"
assert_contains "$(file_of "$case_dir/err")" "Aviso: $proj/.nick/nick.conf não tem todas as chaves" "--check com chave ausente: o aviso do config_load vai pro stderr"

echo
echo "== lançamento: cria o que o config_dir_prepare cria (paridade com o auto_scrum) =="
case_dir="$TEST_TMP/dir_prepare"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
run_agent "$case_dir" "$proj" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
assert_eq "0" "$(file_of "$case_dir/rc")" "dir_prepare: o launcher sai com 0"
assert_eq "*"$'\n'"!.gitignore"$'\n'"!nick.conf" "$(file_of "$proj/.nick/.gitignore")" "o .nick/.gitignore do config_dir_prepare"
assert_exists "$proj/.nick/troca" "a pasta .nick/troca/"
assert_exists "$proj/.nick/docs" "a pasta .nick/docs/"

echo
echo "== sem configuração: sai com 1, aponta o bin/nick-init e não lança nada =="
for mode in check launch; do
  case_dir="$TEST_TMP/no_conf_$mode"
  mkdir -p "$case_dir/proj"
  if [ "$mode" = "check" ]; then
    run_agent "$case_dir" "$case_dir/proj" -- --check
  else
    run_agent "$case_dir" "$case_dir/proj" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
  fi
  assert_eq "1" "$(file_of "$case_dir/rc")" "sem .nick/nick.conf ($mode): sai com 1"
  assert_eq "Erro: não há configuração do projeto em $case_dir/proj/.nick/nick.conf. Rode na raiz do projeto: $NICK_INIT" \
    "$(file_of "$case_dir/err")" "sem .nick/nick.conf ($mode): a mensagem do resolve_stack, com o caminho do bin/nick-init"
  assert_eq "" "$(file_of "$case_dir/out")" "sem .nick/nick.conf ($mode): nada no stdout"
  assert_eq "" "$(file_of "$case_dir/order")" "sem .nick/nick.conf ($mode): nem o ai-jail nem o claude"
  assert_not_exists "$case_dir/proj/.nick" "sem .nick/nick.conf ($mode): nada criado no projeto"
  case_dir="$TEST_TMP/legacy_conf_$mode"
  mkdir -p "$case_dir/proj"
  config_lines > "$case_dir/proj/.nick.conf"
  if [ "$mode" = "check" ]; then
    run_agent "$case_dir" "$case_dir/proj" -- --check
  else
    run_agent "$case_dir" "$case_dir/proj" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
  fi
  assert_eq "1" "$(file_of "$case_dir/rc")" "só o .nick.conf antigo ($mode): sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "Erro: a configuração do projeto agora fica em .nick/nick.conf, e $case_dir/proj/.nick.conf não é mais lido." \
    "só o .nick.conf antigo ($mode): a mensagem de migração do config_cwd_legacy_check"
  assert_eq "" "$(file_of "$case_dir/order")" "só o .nick.conf antigo ($mode): nem o ai-jail nem o claude"
done

echo
echo "== falhas do jail_check: sai com 1, a mensagem cita o .nick/nick.conf e nada é lançado =="
# PATH mínimo, sem nenhum ai-jail (o real pode estar em /usr/bin): só o que o launcher, a lib e
# os falsos usam. O controle positivo, logo abaixo, prova que esse PATH basta.
min_bin="$TEST_TMP/min_bin"
mkdir -p "$min_bin"
for tool in bash dirname basename uname mktemp rm cat env mkdir; do
  ln -s "$(command -v "$tool")" "$min_bin/$tool"
done
ln -s "$FIXTURES_BIN/claude" "$min_bin/claude"
min_with_jail_bin="$TEST_TMP/min_with_jail_bin"
cp -a "$min_bin" "$min_with_jail_bin"
ln -s "$FIXTURES_BIN/ai-jail" "$min_with_jail_bin/ai-jail"
# jail_fail <name> <mode> <message> <env...> -- <config...>: um caso de falha do jail_check.
jail_fail() {
  local name="$1" mode="$2" message="$3" env_args=() conf=() case_dir proj
  shift 3
  while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
    env_args+=("$1")
    shift
  done
  shift
  conf=("$@")
  case_dir="$TEST_TMP/fail_${name}_$mode"
  proj="$(new_project "$case_dir" "${conf[@]}")"
  if [ "$mode" = "check" ]; then
    run_agent "$case_dir" "$proj" "${env_args[@]}" -- --check
  else
    run_agent "$case_dir" "$proj" "${env_args[@]}" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
  fi
  assert_eq "1" "$(file_of "$case_dir/rc")" "$name ($mode): sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "$message" "$name ($mode): o erro do jail_check"
  assert_contains "$(file_of "$case_dir/err")" "$proj/.nick/nick.conf" "$name ($mode): a mensagem cita o .nick/nick.conf do projeto"
  assert_eq "" "$(file_of "$case_dir/out")" "$name ($mode): nada no stdout (nem jail=…)"
  assert_not_exists "$case_dir/claude.argv" "$name ($mode): o claude falso não é chamado"
  assert_not_exists "$case_dir/jail.argv" "$name ($mode): nenhum lançamento pelo ai-jail"
}
for mode in check launch; do
  jail_fail no_jail "$mode" "ai-jail não encontrado no PATH." PATH="$min_bin" -- JAIL_ENABLED='"true"'
  jail_fail old_version "$mode" "versão do ai-jail não suportada: encontrada 2.14.0" FAKE_JAIL_VERSION=2.14.0 -- JAIL_ENABLED='"true"'
  jail_fail bad_clipboard "$mode" "JAIL_CLIPBOARD=\"xyz\" em" -- JAIL_ENABLED='"true"' JAIL_CLIPBOARD='"xyz"'
  jail_fail bad_host "$mode" "https://x.io" -- JAIL_ENABLED='"true"' JAIL_ALLOW_HOSTS='"https://x.io"'
done
case_dir="$TEST_TMP/no_jail_control"
proj="$(new_project "$case_dir" JAIL_ENABLED='"true"')"
run_agent "$case_dir" "$proj" PATH="$min_with_jail_bin" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: o mesmo PATH mínimo com o ai-jail falso: o launcher sai com 0$(file_of "$case_dir/err")"
assert_exists "$case_dir/claude.argv" "controle positivo: com o ai-jail falso, o claude falso é chamado"

echo
echo "== argumentos: qualquer outro uso sai com 1 e mostra o uso =="
case_dir="$TEST_TMP/args"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
arg_case=0
for args in "" "--foo" "--session-id $SESSION_ID" "--name x" "--session-id" "--session-id $SESSION_ID --name" "--check --name x" "--check --check" "--session-id $SESSION_ID --name x --check" "x"; do
  arg_case=$((arg_case + 1))
  rm -f "$case_dir/order"
  # Lista de propósito: cada palavra é um argumento.
  # shellcheck disable=SC2086
  run_agent "$case_dir" "$proj" -- $args
  assert_eq "1" "$(file_of "$case_dir/rc")" "argumentos '$args': sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "Erro: " "argumentos '$args': Erro: no stderr"
  assert_contains "$(file_of "$case_dir/err")" "$USAGE" "argumentos '$args': o uso no stderr"
  assert_eq "" "$(file_of "$case_dir/order")" "argumentos '$args': nada é chamado"
done
for empty_case in "session" "name"; do
  rm -f "$case_dir/order"
  if [ "$empty_case" = "session" ]; then
    run_agent "$case_dir" "$proj" -- --session-id "" --name "$AGENT_NAME"
  else
    run_agent "$case_dir" "$proj" -- --session-id "$SESSION_ID" --name ""
  fi
  assert_eq "1" "$(file_of "$case_dir/rc")" "valor vazio ($empty_case): sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "$USAGE" "valor vazio ($empty_case): o uso no stderr"
  assert_eq "" "$(file_of "$case_dir/order")" "valor vazio ($empty_case): nada é chamado"
done
assert_not_exists "$proj/.nick/.gitignore" "argumentos inválidos: nada criado no projeto"
rm -f "$case_dir/order"
run_agent "$case_dir" "$proj" -- --name "$AGENT_NAME" --session-id "$SESSION_ID"
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: --name antes de --session-id também vale"

echo
echo "== diferencial com o auto_scrum.sh: as mesmas opções do ai-jail e o mesmo argv do claude =="
# run_auto_scrum <case_dir> <cwd>: roda main() do auto_scrum.sh de ponta a ponta (etapa PO,
# po_discussion, com o menu de Jira e Sentry ligados), com os mesmos falsos e o mesmo ambiente
# do run_agent, check_requirements stubado e LOGS_DIR temporário (como o run_main de
# auto_scrum/tests/test_auto_scrum.sh).
run_auto_scrum() {
  local case_dir="$1" cwd="$2" rc=0
  mkdir -p "$case_dir/home"
  (cd "$cwd" && timeout 10 env -u PLAYWRIGHT_BROWSERS_PATH -u FAKE_JAIL_VERSION -u FAKE_CLAUDE_MODE \
    HOME="$case_dir/home" PATH="$FIXTURES_BIN:$PATH" FAKE_JAIL_LOG="$case_dir/jail.argv" \
    FAKE_CLAUDE_LOG="$case_dir/claude.argv" FAKE_ORDER_LOG="$case_dir/order" \
    "$BASH_BIN" -c "source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$case_dir/logs'; main" \
    <<< $'2\nassunto de teste\ns' > "$case_dir/out" 2> "$case_dir/err") || rc=$?
  echo "$rc" > "$case_dir/rc"
}
# options_until_dashes <file>: as linhas de <file> até o "--" (as opções do ai-jail).
options_until_dashes() {
  sed '/^--$/q' "$1" 2> /dev/null || true
}
for diff_jail in true false; do
  diff_conf=("JAIL_ENABLED=\"$diff_jail\"" JAIL_CLIPBOARD='"brando"' JIRA_ENABLED='"true"' SENTRY_ENABLED='"true"' "JAIL_ALLOW_HOSTS=\"$hosts_example\"" "JAIL_FORWARD_PORTS=\"$ports_example\"")
  as_dir="$TEST_TMP/diff_auto_scrum_$diff_jail"
  ag_dir="$TEST_TMP/diff_agent_$diff_jail"
  as_proj="$(new_project "$as_dir" "${diff_conf[@]}")"
  # O mesmo HOME nos dois (o aviso da jail cita as toolchains do home).
  mkdir -p "$ag_dir"
  cp -a "$as_proj" "$ag_dir/proj"
  ag_proj="$ag_dir/proj"
  # O projeto do auto_scrum e o do launcher são pastas diferentes; o mesmo caminho no aviso
  # exige o mesmo HOME, então os dois usam o home do caso do auto_scrum.
  run_auto_scrum "$as_dir" "$as_proj"
  run_agent "$ag_dir" "$ag_proj" HOME="$as_dir/home" -- --session-id "$SESSION_ID" --name "$AGENT_NAME"
  label="diferencial, jail $diff_jail"
  assert_eq "0|0" "$(file_of "$as_dir/rc")|$(file_of "$ag_dir/rc")" "$label: o auto_scrum e o launcher saem com 0"
  as_log="$(find "$as_dir/logs" -name 'po_discussion_*.md' 2> /dev/null | head -n 1)"
  as_prompt="$(file_of "$as_log")"
  as_claude="$(file_of "$as_dir/claude.argv")"
  assert_contains "$as_claude" "assunto de teste" "$label: guarda: o claude do auto_scrum recebeu o prompt"
  # O argv do auto_scrum sem o prompt final; o do launcher sem --session-id/--name (as 4 últimas
  # linhas: o UUID e o nome não têm quebra de linha).
  as_prefix="${as_claude%"$as_prompt"}"
  ag_prefix="$(head -n -4 "$ag_dir/claude.argv" 2> /dev/null || true)"
  assert_eq "$as_prefix" "$ag_prefix"$'\n' "$label: o argv do claude do launcher é o do auto_scrum sem o prompt (e sem --session-id/--name)"
  assert_eq "--session-id"$'\n'"$SESSION_ID"$'\n'"--name"$'\n'"$AGENT_NAME" "$(tail -n 4 "$ag_dir/claude.argv" 2> /dev/null || true)" \
    "$label: o launcher termina com --session-id <uuid> --name <nome>"
  if [ "$diff_jail" = "true" ]; then
    assert_contains "$(options_until_dashes "$as_dir/jail.argv")" "--forward-port" "$label: guarda: o ai-jail do auto_scrum foi chamado com as opções"
    assert_eq "$(options_until_dashes "$as_dir/jail.argv")" "$(options_until_dashes "$ag_dir/jail.argv")" "$label: o ai-jail recebe as mesmas opções até o --"
  else
    assert_not_exists "$as_dir/jail.argv" "$label: o auto_scrum não chama o ai-jail"
    assert_not_exists "$ag_dir/jail.argv" "$label: o launcher não chama o ai-jail"
  fi
done

echo
echo "== estrutura do launcher =="
launcher_code="$(code_lines "$LAUNCHER")"
assert_contains "$(head -n 1 "$LAUNCHER")" "#!/usr/bin/env bash" "o launcher é um script bash (shebang)"
assert_eq "set -euo pipefail" "$(printf '%s\n' "$launcher_code" | grep -v '^[[:space:]]*$' | sed -n 1p)" "a 1ª linha de código depois do shebang (e do cabeçalho) é set -euo pipefail"
assert_eq "x" "$([ -x "$LAUNCHER" ] && echo x || true)" "o launcher tem bit de execução (como o bin/nick-init)"
# shellcheck disable=SC2016
assert_eq 'source "$SCRIPT_DIR/../lib/config/config.sh"'$'\n''source "$SCRIPT_DIR/../lib/config/keys.sh"'$'\n''source "$SCRIPT_DIR/../lib/jail/jail.sh"'$'\n''source "$SCRIPT_DIR/../lib/claude/claude.sh"' \
  "$(printf '%s\n' "$launcher_code" | grep -E '^[[:space:]]*(source|\.)[[:space:]]' || true)" "as únicas linhas source são as quatro do lib/, na ordem do auto_scrum.sh"
assert_eq "config_register_nick_keys" "$(printf '%s\n' "$launcher_code" | grep -v '^[[:space:]]*$' | grep -A1 'lib/claude/claude.sh"$' | tail -n 1)" \
  "config_register_nick_keys vem logo depois das linhas source, no topo (fora de função)"
assert_eq "" "$(claude_command_lines "$LAUNCHER")" "nenhuma linha de código chama claude ou ai-jail fora do jail_exec \"\$JAIL_ENABLED\""
assert_eq "1" "$(printf '%s\n' "$launcher_code" | grep -c 'jail_exec ' || true)" "uma única chamada de jail_exec"
sample="$TEST_TMP/sample.sh"
# Aspas simples intencionais: é texto de código bash, comparado literalmente.
# shellcheck disable=SC2016
printf '%s\n' '# claude no comentário não conta' 'claude_settings_load NICK_CLAUDE_SETTINGS' 'exec claude --settings "$x"' 'ai-jail -- claude' '  jail_exec "$JAIL_ENABLED" "" "" "" claude "${args[@]}"' > "$sample"
# shellcheck disable=SC2016
assert_eq 'exec claude --settings "$x"'$'\n''ai-jail -- claude' "$(claude_command_lines "$sample")" \
  "controle positivo: o filtro acha claude/ai-jail chamados direto e ignora comentário, claude_* e o jail_exec"
assert_not_contains "$launcher_code" "--permission-mode" "o launcher nunca passa --permission-mode"
assert_eq "" "$(printf '%s\n' "$launcher_code" | grep -E '(^|[^A-Z_])CLAUDE_[A-Z_]*=' || true)" "nenhuma global CLAUDE_* atribuída (só NICK_CLAUDE_*)"
# shellcheck disable=SC2016
assert_eq "x" "$(printf '%s\n' 'CLAUDE_X=1' | grep -qE '(^|[^A-Z_])CLAUDE_[A-Z_]*=' && echo x || true)" "controle positivo: o filtro acha CLAUDE_X="
assert_contains "$launcher_code" "jail_mcp_hosts " "os hosts dos MCPs saem da lib (jail_mcp_hosts)"
assert_not_contains "$launcher_code" "mcp.atlassian.com" "o launcher não repete os hosts dos MCPs"

echo
echo "== shellcheck -x no launcher, nesta suíte e nos falsos =="
if command -v shellcheck > /dev/null 2>&1; then
  for checked in "$LAUNCHER" "$SCRIPT_DIR/test_nick_agent.sh" "$FIXTURES_BIN/ai-jail" "$FIXTURES_BIN/claude"; do
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
