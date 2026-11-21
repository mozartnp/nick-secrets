#!/usr/bin/env bash
#
# test_nick_agent: testes em bash puro (sem framework) do bin/nick-agent (NS-33, NS-35), o launcher
# que a casca nick usa pra abrir o claude de um agente: carrega o .nick/nick.conf do cwd pelo
# lib/config/, monta o prompt pelo auto_scrum/prompt.sh (--render) e lança pelo mesmo caminho do
# auto_scrum.sh (jail_check, claude_settings_load, jail_agent_notice e jail_exec do lib/). Os logs
# do --render vão pro NICK_PROMPT_LOGS_DIR de cada caso, nunca pro auto_scrum/logs/ real. O
# ai-jail e o claude reais nunca são usados: os casos
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
REAL_LOGS_DIR="$ROOT_DIR/auto_scrum/logs"
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
USAGE="Uso: nick-agent --check | --render <tipo> [respostas] | --session-id <uuid> --name <nome> --type <tipo> --prompt-file <arquivo> | --resume <uuid> --name <nome> --stage <etapa> [--opened-id <id>] (--ticket-id <id> | --epic-id <id>) | --has-conversation <uuid>"
# Os 8 tipos do choose_type do auto_scrum.sh, na ordem do menu com tudo ligado.
ALL_TYPES=(po po_discussion po_jira po_sentry tech_leader tech_leader_jira development review)
# O texto que o EDITOR falso grava (po e tech_leader) e a DESCRIPTION que sai dele ($(cat), sem
# os \n do fim).
EDITOR_TEXT=$'Descrição longa de teste\ncom duas linhas.\n\n'
EDITOR_DESCRIPTION=$'Descrição longa de teste\ncom duas linhas.'
# O prompt de exemplo do lançamento: duas linhas e \n no fim, que o $(cat) tira.
PROMPT_TEXT=$'Prompt de teste\ncom duas linhas.\n\n'
PROMPT_ARG=$'Prompt de teste\ncom duas linhas.'
# Retrato do auto_scrum/logs/ real: nenhum caso pode escrever lá.
real_logs_before="$(ls -la "$REAL_LOGS_DIR" 2> /dev/null || true)"

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
# em <cwd>, os logs do --render em <case_dir>/logs (NICK_PROMPT_LOGS_DIR), HOME em
# <case_dir>/home (vazio: as toolchains que a jail monta vêm do home, e o
# resultado não pode depender da máquina), os falsos à frente do PATH, os logs dos falsos em
# <case_dir> (fora do projeto) e sem as variáveis FAKE_*, PLAYWRIGHT_BROWSERS_PATH,
# CLAUDE_CONFIG_DIR (NS-39: --has-conversation a lê) e as chaves da jail do ambiente de quem roda a suíte (dentro da jail, o próprio nick define a
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
    -u JAIL_ENABLED -u JAIL_CLIPBOARD -u JAIL_ALLOW_HOSTS -u JAIL_FORWARD_PORTS -u CLAUDE_CONFIG_DIR \
    HOME="$case_dir/home" PATH="$FIXTURES_BIN:$PATH" FAKE_JAIL_LOG="$case_dir/jail.argv" \
    FAKE_CLAUDE_LOG="$case_dir/claude.argv" FAKE_CLAUDE_ENV="$case_dir/claude.env" \
    FAKE_ORDER_LOG="$case_dir/order" NICK_PROMPT_LOGS_DIR="$case_dir/logs" "${env_args[@]}" "$BASH_BIN" "$LAUNCHER" "$@" < /dev/null \
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

# expected_claude_argv <case_dir> <jail> <hosts> <ports> [type]: o argv que o claude falso deve
# receber, um por linha: as regras de deny/ask inline, --permission-mode auto (só com [type]
# development), o aviso da jail (só com <jail> "true"), --session-id, --name e o prompt de
# exemplo (PROMPT_ARG) por último. [type] padrão: po_discussion.
expected_claude_argv() {
  local case_dir="$1" jail="$2" hosts="$3" ports="$4" type="${5:-po_discussion}" args=()
  args=(--settings "$(cat "$PERMISSIONS_JSON")")
  if [ "$type" = "development" ]; then
    args+=(--permission-mode auto)
  fi
  if [ "$jail" = "true" ]; then
    args+=(--append-system-prompt "$(lib_call "$case_dir" jail_agent_notice "$hosts" "$ports")")
  fi
  args+=(--session-id "$SESSION_ID" --name "$AGENT_NAME" "$PROMPT_ARG")
  printf '%s\n' "${args[@]}"
}

# launch_args <case_dir> [type]: grava o prompt de exemplo em <case_dir>/prompt.md (fora do
# projeto) e imprime, um por linha, os argumentos do lançamento: --session-id, --name, --type
# ([type], padrão po_discussion) e --prompt-file. Use com mapfile.
launch_args() {
  local case_dir="$1" type="${2:-po_discussion}"
  mkdir -p "$case_dir"
  printf '%s' "$PROMPT_TEXT" > "$case_dir/prompt.md"
  printf '%s\n' --session-id "$SESSION_ID" --name "$AGENT_NAME" --type "$type" --prompt-file "$case_dir/prompt.md"
}

# launch <case_dir> <cwd> [VAR=valor...] [-- type]: run_agent com os argumentos de launch_args.
launch() {
  local case_dir="$1" cwd="$2" env_args=() type="po_discussion" args
  shift 2
  while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
    env_args+=("$1")
    shift
  done
  if [ "$#" -gt 1 ]; then
    type="$2"
  fi
  mapfile -t args < <(launch_args "$case_dir" "$type")
  run_agent "$case_dir" "$cwd" "${env_args[@]}" -- "${args[@]}"
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
launch "$case_dir" "$proj"
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
  "o ai-jail recebe jail_options (brando, MCPs + JAIL_ALLOW_HOSTS, portas), --, claude --settings <regras> --append-system-prompt <aviso> --session-id <uuid> --name <nome> <prompt>"
assert_eq "$(expected_claude_argv "$case_dir" true "$hosts" "$ports_example")" "$(file_of "$case_dir/claude.argv")" \
  "o claude falso recebe exatamente o que vem depois do --"
jail_argv="$(file_of "$case_dir/jail.argv")"
for expected_opt in --display --allow-host mcp.atlassian.com mcp.sentry.dev "$hosts_example" --forward-port 54320; do
  assert_contains "$jail_argv" "$expected_opt" "o argv do ai-jail tem $expected_opt"
done
assert_eq "ai-jail -- true"$'\n'"ai-jail"$'\n'"claude" "$(file_of "$case_dir/order")" "a checagem (-- true), depois o lançamento pelo ai-jail e só então o claude"
assert_contains "$(file_of "$case_dir/out")" "Jail (ai-jail): ativa" "o aviso de jail_print_status aparece no terminal do agente"
assert_eq "PWD=$proj" "$(env_pwd "$case_dir")" "o claude roda com o cwd no projeto"
assert_eq "$PROMPT_ARG" "$(tail -n 2 "$case_dir/claude.argv" 2> /dev/null || true)" "o último argumento do claude é o prompt (o conteúdo do --prompt-file sem os \\n do fim)"
assert_not_contains "$(file_of "$case_dir/claude.argv")" "--permission-mode" "po_discussion: o claude não recebe --permission-mode"

echo
echo "== lançamento com a jail desligada: o ai-jail não é chamado =="
case_dir="$TEST_TMP/jail_off"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"' JIRA_ENABLED='"true"' "JAIL_ALLOW_HOSTS=\"$hosts_example\"")"
launch "$case_dir" "$proj"
assert_eq "0" "$(file_of "$case_dir/rc")" "jail desligada: o launcher sai com 0"
assert_eq "$(expected_claude_argv "$case_dir" false "" "")" "$(file_of "$case_dir/claude.argv")" \
  "jail desligada: claude --settings <regras> --session-id <uuid> --name <nome> <prompt>"
assert_not_exists "$case_dir/jail.argv" "jail desligada: nenhum lançamento pelo ai-jail"
assert_eq "claude" "$(file_of "$case_dir/order")" "jail desligada: o ai-jail não é chamado nem pra checar"
assert_contains "$(file_of "$case_dir/out")" "Jail (ai-jail): desligada" "jail desligada: o aviso de jail_print_status"
assert_eq "PWD=$proj" "$(env_pwd "$case_dir")" "jail desligada: o claude roda com o cwd no projeto"
assert_eq "$PROMPT_ARG" "$(tail -n 2 "$case_dir/claude.argv" 2> /dev/null || true)" "jail desligada: o último argumento é o prompt"
for jail_value in TRUE '' sim; do
  case_dir="$TEST_TMP/jail_value_${jail_value:-vazio}"
  proj="$(new_project "$case_dir" "JAIL_ENABLED=\"$jail_value\"")"
  launch "$case_dir" "$proj"
  assert_not_exists "$case_dir/jail.argv" "JAIL_ENABLED=\"$jail_value\": só \"true\" exato liga a jail"
  assert_eq "$(expected_claude_argv "$case_dir" false "" "")" "$(file_of "$case_dir/claude.argv")" "JAIL_ENABLED=\"$jail_value\": o argv da jail desligada"
done

echo
echo "== dois projetos: cada execução usa a configuração do próprio cwd =="
case_a="$TEST_TMP/two_a"
case_b="$TEST_TMP/two_b"
proj_a="$(new_project "$case_a" JAIL_ENABLED='"true"' "JAIL_ALLOW_HOSTS=\"a.example.com\"")"
proj_b="$(new_project "$case_b" JAIL_ENABLED='"false"' "JAIL_ALLOW_HOSTS=\"b.example.com\"")"
launch "$case_a" "$proj_a"
launch "$case_b" "$proj_b"
assert_eq "0|0" "$(file_of "$case_a/rc")|$(file_of "$case_b/rc")" "os dois projetos: o launcher sai com 0"
assert_eq "$(expected_jail_argv "$case_a" "" " a.example.com" "")" "$(file_of "$case_a/jail.argv")" "projeto A (jail ligada): o ai-jail recebe os hosts de A"
assert_not_contains "$(file_of "$case_a/jail.argv")" "b.example.com" "projeto A: nada da configuração de B"
assert_not_exists "$case_b/jail.argv" "projeto B (jail desligada): o ai-jail não é chamado"
assert_eq "$(expected_claude_argv "$case_b" false "" "")" "$(file_of "$case_b/claude.argv")" "projeto B: o argv da jail desligada"
assert_eq "PWD=$proj_a" "$(env_pwd "$case_a")" "projeto A: o claude roda no cwd de A"
assert_eq "PWD=$proj_b" "$(env_pwd "$case_b")" "projeto B: o claude roda no cwd de B"

echo
echo "== --check: só valida, imprime as três linhas jail=/jira=/sentry= e não grava nada =="
for check_case in "true|jail=true" "false|jail=false" "TRUE|jail=false" "|jail=false"; do
  IFS='|' read -r check_value check_expected <<< "$check_case"
  check_expected="$check_expected"$'\n'"jira=true"$'\n'"sentry=false"
  case_dir="$TEST_TMP/check_${check_value:-vazio}"
  proj="$(new_project "$case_dir" "JAIL_ENABLED=\"$check_value\"" JIRA_ENABLED='"true"' "JAIL_ALLOW_HOSTS=\"$hosts_example\"")"
  # Sem .nick/.gitignore, troca/ nem docs/: o lançamento os criaria (config_dir_prepare).
  before="$(tree_snapshot "$proj")"
  run_agent "$case_dir" "$proj" -- --check
  label="--check com JAIL_ENABLED=\"$check_value\""
  assert_eq "0" "$(file_of "$case_dir/rc")" "$label: sai com 0"
  assert_eq "$check_expected" "$(file_of "$case_dir/out")" "$label: o stdout são as três linhas, nesta ordem: ${check_expected//$'\n'/ }"
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
assert_eq "0|jail=false"$'\n'"jira=false"$'\n'"sentry=false" "$(file_of "$case_dir/rc")|$(file_of "$case_dir/out")" "--check com chave ausente: sai com 0 e o stdout continua só as três linhas"
assert_contains "$(file_of "$case_dir/err")" "Aviso: $proj/.nick/nick.conf não tem todas as chaves" "--check com chave ausente: o aviso do config_load vai pro stderr"
# jira=/sentry=: true só com "true" exato (a regra do choose_type e do jail_exec).
for flag_case in "true|true|true" "TRUE|false|false" " true|false|false" "|false|false" "false|false|true"; do
  IFS='|' read -r flag_value flag_jira flag_sentry <<< "$flag_case"
  case_dir="$TEST_TMP/check_flags_${flag_value// /_}_$flag_sentry"
  sentry_value="$flag_value"
  [ "$flag_case" = "false|false|true" ] && sentry_value="true"
  proj="$(new_project "$case_dir" JAIL_ENABLED='"false"' "JIRA_ENABLED=\"$flag_value\"" "SENTRY_ENABLED=\"$sentry_value\"")"
  before="$(tree_snapshot "$proj")"
  run_agent "$case_dir" "$proj" -- --check
  assert_eq "0|jail=false"$'\n'"jira=$flag_jira"$'\n'"sentry=$flag_sentry" "$(file_of "$case_dir/rc")|$(file_of "$case_dir/out")" "--check com JIRA_ENABLED=\"$flag_value\" e SENTRY_ENABLED=\"$sentry_value\": jira=$flag_jira, sentry=$flag_sentry"
  assert_eq "$before" "$(tree_snapshot "$proj")" "--check com JIRA_ENABLED=\"$flag_value\": nenhum arquivo do projeto muda"
done

echo
echo "== lançamento: cria o que o config_dir_prepare cria (paridade com o auto_scrum) =="
case_dir="$TEST_TMP/dir_prepare"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
launch "$case_dir" "$proj"
assert_eq "0" "$(file_of "$case_dir/rc")" "dir_prepare: o launcher sai com 0"
assert_eq "*"$'\n'"!.gitignore"$'\n'"!nick.conf" "$(file_of "$proj/.nick/.gitignore")" "o .nick/.gitignore do config_dir_prepare"
assert_exists "$proj/.nick/troca" "a pasta .nick/troca/"
assert_exists "$proj/.nick/docs" "a pasta .nick/docs/"

echo
echo "== sem configuração: sai com 1, aponta o bin/nick-init e não lança nada =="
for mode in check launch render; do
  case_dir="$TEST_TMP/no_conf_$mode"
  mkdir -p "$case_dir/proj"
  if [ "$mode" = "check" ]; then
    run_agent "$case_dir" "$case_dir/proj" -- --check
  elif [ "$mode" = "render" ]; then
    run_agent "$case_dir" "$case_dir/proj" -- --render po_discussion --title "assunto"
  else
    launch "$case_dir" "$case_dir/proj"
  fi
  assert_eq "1" "$(file_of "$case_dir/rc")" "sem .nick/nick.conf ($mode): sai com 1"
  assert_eq "Erro: não há configuração do projeto em $case_dir/proj/.nick/nick.conf. Rode na raiz do projeto: $NICK_INIT" \
    "$(file_of "$case_dir/err")" "sem .nick/nick.conf ($mode): a mensagem do resolve_stack, com o caminho do bin/nick-init"
  assert_eq "" "$(file_of "$case_dir/out")" "sem .nick/nick.conf ($mode): nada no stdout"
  assert_eq "" "$(file_of "$case_dir/order")" "sem .nick/nick.conf ($mode): nem o ai-jail nem o claude"
  assert_not_exists "$case_dir/proj/.nick" "sem .nick/nick.conf ($mode): nada criado no projeto"
  assert_not_exists "$case_dir/logs" "sem .nick/nick.conf ($mode): nenhum log de prompt"
  case_dir="$TEST_TMP/legacy_conf_$mode"
  mkdir -p "$case_dir/proj"
  config_lines > "$case_dir/proj/.nick.conf"
  if [ "$mode" = "check" ]; then
    run_agent "$case_dir" "$case_dir/proj" -- --check
  elif [ "$mode" = "render" ]; then
    run_agent "$case_dir" "$case_dir/proj" -- --render po_discussion --title "assunto"
  else
    launch "$case_dir" "$case_dir/proj"
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
  elif [ "$mode" = "render" ]; then
    run_agent "$case_dir" "$proj" "${env_args[@]}" -- --render po_discussion --title "assunto"
  elif [ "$mode" = "resume" ]; then
    run_agent "$case_dir" "$proj" "${env_args[@]}" -- --resume "$SESSION_ID" --name "$AGENT_NAME" --stage po --ticket-id NS-1
  else
    launch "$case_dir" "$proj" "${env_args[@]}"
  fi
  assert_eq "1" "$(file_of "$case_dir/rc")" "$name ($mode): sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "$message" "$name ($mode): o erro do jail_check"
  assert_contains "$(file_of "$case_dir/err")" "$proj/.nick/nick.conf" "$name ($mode): a mensagem cita o .nick/nick.conf do projeto"
  assert_eq "" "$(file_of "$case_dir/out")" "$name ($mode): nada no stdout (nem jail=…)"
  assert_not_exists "$case_dir/claude.argv" "$name ($mode): o claude falso não é chamado"
  assert_not_exists "$case_dir/jail.argv" "$name ($mode): nenhum lançamento pelo ai-jail"
  assert_not_exists "$case_dir/logs" "$name ($mode): nenhum log de prompt"
}
for mode in check launch render resume; do
  jail_fail no_jail "$mode" "ai-jail não encontrado no PATH." PATH="$min_bin" -- JAIL_ENABLED='"true"'
  jail_fail old_version "$mode" "versão do ai-jail não suportada: encontrada 2.14.0" FAKE_JAIL_VERSION=2.14.0 -- JAIL_ENABLED='"true"'
  jail_fail bad_clipboard "$mode" "JAIL_CLIPBOARD=\"xyz\" em" -- JAIL_ENABLED='"true"' JAIL_CLIPBOARD='"xyz"'
  jail_fail bad_host "$mode" "https://x.io" -- JAIL_ENABLED='"true"' JAIL_ALLOW_HOSTS='"https://x.io"'
done
case_dir="$TEST_TMP/no_jail_control"
proj="$(new_project "$case_dir" JAIL_ENABLED='"true"')"
launch "$case_dir" "$proj" PATH="$min_with_jail_bin"
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: o mesmo PATH mínimo com o ai-jail falso: o launcher sai com 0$(file_of "$case_dir/err")"
assert_exists "$case_dir/claude.argv" "controle positivo: com o ai-jail falso, o claude falso é chamado"

echo
echo "== argumentos: qualquer outro uso sai com 1 e mostra o uso =="
case_dir="$TEST_TMP/args"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
arg_case=0
printf '%s' "$PROMPT_TEXT" > "$case_dir/prompt.md"
: > "$case_dir/vazio.md"
pf="$case_dir/prompt.md"
for args in "" "--foo" "--session-id $SESSION_ID" "--name x" "--session-id" "--session-id $SESSION_ID --name" "--check --name x" "--check --check" "--session-id $SESSION_ID --name x --check" "x" \
  "--session-id $SESSION_ID --name x" "--session-id $SESSION_ID --name x --type po" "--session-id $SESSION_ID --name x --prompt-file $pf" \
  "--session-id $SESSION_ID --name x --type xpto --prompt-file $pf" "--session-id $SESSION_ID --name x --type po --prompt-file $case_dir/nao-existe.md" \
  "--session-id $SESSION_ID --name x --type po --prompt-file $case_dir/vazio.md" "--session-id $SESSION_ID --name x --type po --prompt-file $case_dir" \
  "--session-id $SESSION_ID --name x --type po --prompt-file" \
  "--check --render po" "--render" "--session-id $SESSION_ID --render po"; do
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
    run_agent "$case_dir" "$proj" -- --session-id "" --name "$AGENT_NAME" --type po --prompt-file "$pf"
  else
    run_agent "$case_dir" "$proj" -- --session-id "$SESSION_ID" --name "" --type po --prompt-file "$pf"
  fi
  assert_eq "1" "$(file_of "$case_dir/rc")" "valor vazio ($empty_case): sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "$USAGE" "valor vazio ($empty_case): o uso no stderr"
  assert_eq "" "$(file_of "$case_dir/order")" "valor vazio ($empty_case): nada é chamado"
done
assert_not_exists "$proj/.nick/.gitignore" "argumentos inválidos: nada criado no projeto"
rm -f "$case_dir/order"
run_agent "$case_dir" "$proj" -- --prompt-file "$pf" --type po --name "$AGENT_NAME" --session-id "$SESSION_ID"
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: os quatro em qualquer ordem também valem"
assert_not_exists "$case_dir/logs" "argumentos: nenhum log de prompt gravado"

echo
echo "== --render: monta o prompt pelo prompt.sh, grava o log fora do projeto e devolve log= + o status da jail =="
# render_args <type>: as respostas de exemplo de cada tipo, cada argumento terminado por NUL (use
# com mapfile -d ''; a descrição longa tem quebra de linha), já aparadas (é o que a casca manda,
# depois de aparar como o read).
render_args() {
  case "$1" in
    po) printf '%s\0' --title "Título de teste" --description "$EDITOR_DESCRIPTION" ;;
    po_discussion) printf '%s\0' --title "Assunto de teste" ;;
    po_jira) printf '%s\0' --jira-link "https://jira.example.com/browse/SC-1" --description "extra do jira" ;;
    po_sentry) printf '%s\0' --sentry-link "https://sentry.example.com/issues/1" ;;
    tech_leader) printf '%s\0' --jira-id SC-1 --title "Título da tarefa" --description "$EDITOR_DESCRIPTION" ;;
    tech_leader_jira) printf '%s\0' --jira-link "https://jira.example.com/browse/SC-2" --description "contexto extra" ;;
    development) printf '%s\0' --jira-id SC-1 ;;
    review) printf '%s\0' --jira-id SC-2 --plan-path outro/plano.md ;;
  esac
}
# render <case_dir> <cwd> <type> [VAR=valor...]: run_agent com --render <type> e as respostas de
# render_args.
render() {
  local case_dir="$1" cwd="$2" type="$3" args
  shift 3
  mapfile -t -d '' args < <(render_args "$type")
  run_agent "$case_dir" "$cwd" "$@" -- --render "$type" "${args[@]}"
}
render_conf=(JAIL_ENABLED='"false"' JIRA_ENABLED='"true"' SENTRY_ENABLED='"true"' PRODUCTION_ENABLED='"true"' PERMISSION_ENABLED='"true"' STACK_DESCRIPTION='"Bash"')
off_status="Jail (ai-jail): desligada — o agente roda direto no seu shell."
for type in "${ALL_TYPES[@]}"; do
  case_dir="$TEST_TMP/render_$type"
  proj="$(new_project "$case_dir" "${render_conf[@]}")"
  before="$(tree_snapshot "$proj")"
  render "$case_dir" "$proj" "$type"
  label="--render $type"
  assert_eq "0" "$(file_of "$case_dir/rc")" "$label: sai com 0$(file_of "$case_dir/err")"
  log="$(find "$case_dir/logs" -name "${type}_*.md" 2> /dev/null | head -n 1 || true)"
  assert_eq "1" "$(find "$case_dir/logs" -type f 2> /dev/null | wc -l || true)" "$label: um log só, no diretório de logs"
  assert_eq "true" "$([[ "$(basename "$log")" =~ ^${type}_[0-9]{8}_[0-9]{6}\.md$ ]] && echo true || echo false)" "$label: o log se chama <tipo>_<AAAAmmdd_HHMMSS>.md (o nome do auto_scrum.sh)"
  assert_eq "log=$log"$'\n'"$off_status" "$(file_of "$case_dir/out")" "$label: o stdout é log=<caminho absoluto> e o status da jail (jail_print_status)"
  assert_eq "/" "${log:0:1}" "$label: o caminho do log é absoluto"
  assert_eq "" "$(file_of "$case_dir/err")" "$label: nada no stderr"
  assert_contains "$(file_of "$log")" "## Troca de arquivos com o usuário" "$label: o log tem o exchange_block no fim"
  # shellcheck disable=SC2016
  assert_not_contains "$(file_of "$log")" '${' "$label: nenhuma variável sobra sem expandir"
  assert_eq "$before" "$(tree_snapshot "$proj")" "$label: nada muda no projeto (retrato recursivo)"
  assert_not_exists "$proj/.nick/.gitignore" "$label: sem .nick/.gitignore criado"
  assert_eq "" "$(file_of "$case_dir/order")" "$label: nem o ai-jail nem o claude"
done
assert_contains "$(file_of "$(find "$TEST_TMP/render_development/logs" -type f 2> /dev/null | head -n 1 || true)")" ".nick/docs/SC-1/plan.md" "--render development sem --plan-path: o plano padrão .nick/docs/<ID>/plan.md, calculado pelo bash"
review_log="$(file_of "$(find "$TEST_TMP/render_review/logs" -type f 2> /dev/null | head -n 1 || true)")"
assert_contains "$review_log" "outro/plano.md" "--render review com --plan-path: o caminho informado"
assert_contains "$review_log" ".nick/docs/SC-2/review.md" "--render review: o REVIEW_PATH ao lado do plano"
assert_contains "$(file_of "$(find "$TEST_TMP/render_po/logs" -type f 2> /dev/null | head -n 1 || true)")" "$EDITOR_DESCRIPTION" "--render po: a descrição longa vai inteira, com as quebras de linha"
# A jail ligada: o status é o de jail_print_status, depois do jail_check (com o Landlock dele).
case_dir="$TEST_TMP/render_jail_on"
proj="$(new_project "$case_dir" JAIL_ENABLED='"true"' JAIL_CLIPBOARD='"restrito"')"
render "$case_dir" "$proj" po_discussion
assert_eq "0" "$(file_of "$case_dir/rc")" "--render com a jail ligada: sai com 0"
assert_contains "$(sed -n 2p "$case_dir/out" 2> /dev/null || true)" "Jail (ai-jail): ativa" "--render com a jail ligada: a 2ª linha é o status da jail ligada"
assert_contains "$(sed -n 2p "$case_dir/out" 2> /dev/null || true)" "modo de clipboard restrito" "--render com a jail ligada: com o modo do JAIL_CLIPBOARD"
assert_eq "ai-jail -- true" "$(file_of "$case_dir/order")" "--render com a jail ligada: só a checagem -- true, nenhum lançamento"
assert_not_exists "$case_dir/claude.argv" "--render com a jail ligada: o claude não é chamado"

echo
echo "== --render: recusas (saída 1, uso no stderr, nenhum log) =="
case_dir="$TEST_TMP/render_refusals"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
refusal_case=0
# render_refused <message> <args...>: --render com <args>, recusado com <message> no stderr.
render_refused() {
  local message="$1"
  shift
  refusal_case=$((refusal_case + 1))
  rm -rf "$case_dir/logs" "$case_dir/order"
  run_agent "$case_dir" "$proj" -- --render "$@"
  local label="--render $*"
  assert_eq "1" "$(file_of "$case_dir/rc")" "$label: sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "Erro: $message" "$label: Erro: $message"
  assert_contains "$(file_of "$case_dir/err")" "$USAGE" "$label: o uso no stderr"
  assert_eq "" "$(file_of "$case_dir/out")" "$label: nada no stdout"
  assert_not_exists "$case_dir/logs" "$label: nenhum log gravado"
  assert_eq "" "$(file_of "$case_dir/order")" "$label: nada é chamado"
}
render_refused "informe o tipo depois do --render."
render_refused "o tipo 'xpto' não existe." xpto --title a
render_refused "o tipo '../po' não existe." ../po --title a
render_refused "o tipo 'po_jira' não está disponível neste projeto." po_jira --jira-link x
render_refused "o tipo 'tech_leader_jira' não está disponível neste projeto." tech_leader_jira --jira-link x
render_refused "o tipo 'po_sentry' não está disponível neste projeto." po_sentry --sentry-link x
render_refused "a resposta --description não pertence ao tipo 'po_discussion'." po_discussion --title a --description b
render_refused "a resposta --plan-path não pertence ao tipo 'tech_leader'." tech_leader --jira-id SC-1 --title a --description b --plan-path x
render_refused "a resposta --title foi repetida." po_discussion --title a --title b
render_refused "falta a resposta --description do tipo 'po'." po --title a
render_refused "falta a resposta --title do tipo 'po'." po --title "" --description b
render_refused "falta a resposta --jira-id do tipo 'development'." development
render_refused "falta a resposta --jira-id do tipo 'review'." review --jira-id ""
# Só espaço, tab e \n: vazio para o read do script, numa resposta lida por ele (review do
# NS-35). A descrição longa vem do read_via_editor, que aceita um texto só de espaços (o arquivo
# não é vazio): o controle positivo fica logo abaixo.
render_refused "falta a resposta --title do tipo 'po_discussion'." po_discussion --title "   "
render_refused "falta a resposta --jira-id do tipo 'development'." development --jira-id $'\t'
render_refused "falta a resposta --jira-link do tipo 'po_jira'." po_jira --jira-link " "
for bad_id in a/b .. "a b" "."; do
  render_refused "ID do Jira inválido: não pode ter /, .., espaço nem caractere de controle (ele vira o nome da pasta .nick/docs/<ID>/)." development --jira-id "$bad_id"
done
render_refused "falta o valor de --title." po_discussion --title
render_refused "argumento desconhecido: '--foo'" po_discussion --title a --foo b
render_refused "argumento desconhecido: 'x'" po_discussion x
case_dir="$TEST_TMP/render_blank_description"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
run_agent "$case_dir" "$proj" -- --render po --title a --description "   "
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: a descrição longa só com espaços é aceita, como no read_via_editor"
# Os tipos com Jira/Sentry, com as flags ligadas: o controle positivo da recusa acima.
case_dir="$TEST_TMP/render_flags_on"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"' JIRA_ENABLED='"true"' SENTRY_ENABLED='"true"')"
run_agent "$case_dir" "$proj" -- --render po_jira --jira-link x
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: po_jira com JIRA_ENABLED=\"true\" sai com 0"
case_dir="$TEST_TMP/render_jira_typo"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"' JIRA_ENABLED='"TRUE"')"
run_agent "$case_dir" "$proj" -- --render po_jira --jira-link x
assert_eq "1" "$(file_of "$case_dir/rc")" "po_jira com JIRA_ENABLED=\"TRUE\" (não exato): recusado, a regra do choose_type"

echo
echo "== --render: warn_legacy_plan no stderr para development e review =="
for type in development review; do
  case_dir="$TEST_TMP/render_legacy_$type"
  proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
  mkdir -p "$proj/docs/plans"
  : > "$proj/docs/plans/SC-1_x.md"
  run_agent "$case_dir" "$proj" -- --render "$type" --jira-id SC-1
  assert_eq "0" "$(file_of "$case_dir/rc")" "$type com docs/plans/SC-1_x.md: sai com 0"
  assert_eq "Aviso: .nick/docs/SC-1/plan.md não existe, mas há um plano no lugar antigo: docs/plans/SC-1_x.md — para usá-lo, informe esse caminho na pergunta abaixo." \
    "$(file_of "$case_dir/err")" "$type: o aviso do warn_legacy_plan no stderr"
  assert_contains "$(file_of "$case_dir/out")" "log=" "$type: o stdout continua log= e o status"
done

echo
echo "== lançamento: o prompt é o último argumento, e só o development tem --permission-mode auto =="
for type in "${ALL_TYPES[@]}"; do
  case_dir="$TEST_TMP/launch_$type"
  proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
  launch "$case_dir" "$proj" -- "$type"
  assert_eq "0" "$(file_of "$case_dir/rc")" "lançamento --type $type: sai com 0"
  assert_eq "$(expected_claude_argv "$case_dir" false "" "" "$type")" "$(file_of "$case_dir/claude.argv")" \
    "lançamento --type $type: claude --settings <regras>$([ "$type" = development ] && echo ' --permission-mode auto') --session-id <uuid> --name <nome> <prompt>"
  if [ "$type" = "development" ]; then
    assert_contains "$(file_of "$case_dir/claude.argv")" "--permission-mode"$'\n'"auto" "lançamento --type development: --permission-mode auto"
  else
    assert_not_contains "$(file_of "$case_dir/claude.argv")" "--permission-mode" "lançamento --type $type: sem --permission-mode"
  fi
  assert_eq "$PROMPT_ARG" "$(tail -n 2 "$case_dir/claude.argv" 2> /dev/null || true)" "lançamento --type $type: o prompt é o último argumento"
done
assert_not_exists "$TEST_TMP/launch_po/logs" "lançamento: nenhum log de prompt (quem grava é o --render)"

echo
echo "== diferencial com o auto_scrum.sh: os 8 tipos, a matriz de flags, o mesmo log byte a byte, o mesmo argv do claude e as mesmas opções do ai-jail =="
# O EDITOR falso (po e tech_leader): grava EDITOR_TEXT no arquivo.
fake_editor="$TEST_TMP/fake_editor"
# Aspas simples intencionais: o "$1" é do script gerado, não daqui.
# shellcheck disable=SC2016
printf '#!/usr/bin/env bash\nprintf %%s %q > "$1"\n' "$EDITOR_TEXT" > "$fake_editor"
chmod +x "$fake_editor"
# script_answers <type>: o stdin do auto_scrum.sh depois do número do menu (as respostas cruas,
# com os espaços que o read apara; a descrição longa vem do EDITOR falso).
script_answers() {
  case "$1" in
    po) printf '%s\n' '  Título de teste  ' ;;
    po_discussion) printf '%s\n' $'\tAssunto de teste\t' ;;
    po_jira) printf '%s\n' 'https://jira.example.com/browse/SC-1' '  extra do jira  ' ;;
    po_sentry) printf '%s\n' 'https://sentry.example.com/issues/1' '' ;;
    tech_leader) printf '%s\n' 'SC-1' 'Título da tarefa' ;;
    tech_leader_jira) printf '%s\n' 'https://jira.example.com/browse/SC-2' 'contexto extra' ;;
    development) printf '%s\n' 'SC-1' '' ;;
    review) printf '%s\n' ' SC-2 ' 'outro/plano.md' ;;
  esac
}
# run_auto_scrum <case_dir> <cwd> <type> <menu number>: roda main() do auto_scrum.sh de ponta a
# ponta (o número do menu, as respostas e s), com os mesmos falsos e o mesmo ambiente do
# run_agent, o EDITOR falso, check_requirements stubado e LOGS_DIR temporário (como o run_main de
# auto_scrum/tests/test_auto_scrum.sh).
run_auto_scrum() {
  local case_dir="$1" cwd="$2" type="$3" number="$4" rc=0
  mkdir -p "$case_dir/home"
  (cd "$cwd" && timeout 10 env -u PLAYWRIGHT_BROWSERS_PATH -u FAKE_JAIL_VERSION -u FAKE_CLAUDE_MODE \
    HOME="$case_dir/home" PATH="$FIXTURES_BIN:$PATH" FAKE_JAIL_LOG="$case_dir/jail.argv" \
    FAKE_CLAUDE_LOG="$case_dir/claude.argv" FAKE_ORDER_LOG="$case_dir/order" EDITOR="$fake_editor" \
    "$BASH_BIN" -c "source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$case_dir/logs'; main" \
    <<< "$(printf '%s\n' "$number"; script_answers "$type"; printf 's\n')" > "$case_dir/out" 2> "$case_dir/err") || rc=$?
  echo "$rc" > "$case_dir/rc"
}
# options_until_dashes <file>: as linhas de <file> até o "--" (as opções do ai-jail).
options_until_dashes() {
  sed '/^--$/q' "$1" 2> /dev/null || true
}
# without_session <file>: o argv de <file> sem as 4 linhas --session-id <uuid> --name <nome>.
without_session() {
  awk -v id="$SESSION_ID" -v name="$AGENT_NAME" '
    skip > 0 { skip--; next }
    !done && $0 == "--session-id" { getline uuid; getline flag; getline value
      if (uuid == id && flag == "--name" && value == name) { done = 1; next }
      print; print uuid; print flag; print value; next }
    { print }' "$1" 2> /dev/null || true
}
# A matriz (plano do NS-35, 4.2): nome|jail|chaves. Tudo desligado (TDD_ENABLED="false"), tudo
# ligado (com STACK_DESCRIPTION preenchido e a jail ligada, que cobre cada papel com a jail
# ligada; os outros com a jail desligada) e cada flag ligada sozinha.
diff_matrix=(
  'tudo_desligado|false|TDD_ENABLED="false"'
  'tudo_ligado|true|JIRA_ENABLED="true";SENTRY_ENABLED="true";PRODUCTION_ENABLED="true";TDD_ENABLED="true";PERMISSION_ENABLED="true";STACK_DESCRIPTION="Bash e Rust"'
  'so_jira|false|JIRA_ENABLED="true";TDD_ENABLED="false"'
  'so_sentry|false|SENTRY_ENABLED="true";TDD_ENABLED="false"'
  'so_production|false|PRODUCTION_ENABLED="true";TDD_ENABLED="false"'
  'so_tdd|false|TDD_ENABLED="true"'
  'so_permission|false|PERMISSION_ENABLED="true";TDD_ENABLED="false"'
)
diff_types_seen=""
for diff_combo in "${diff_matrix[@]}"; do
  IFS='|' read -r diff_name diff_jail diff_keys <<< "$diff_combo"
  IFS=';' read -r -a diff_conf <<< "$diff_keys"
  diff_conf+=("JAIL_ENABLED=\"$diff_jail\"" JAIL_CLIPBOARD='"brando"' "JAIL_ALLOW_HOSTS=\"$hosts_example\"" "JAIL_FORWARD_PORTS=\"$ports_example\"")
  # O menu do choose_type com estas flags (a posição dos tipos com Jira/Sentry muda).
  diff_menu=(po po_discussion)
  [[ " ${diff_conf[*]} " == *' JIRA_ENABLED="true" '* ]] && diff_menu+=(po_jira)
  [[ " ${diff_conf[*]} " == *' SENTRY_ENABLED="true" '* ]] && diff_menu+=(po_sentry)
  diff_menu+=(tech_leader)
  [[ " ${diff_conf[*]} " == *' JIRA_ENABLED="true" '* ]] && diff_menu+=(tech_leader_jira)
  diff_menu+=(development review)
  for diff_index in "${!diff_menu[@]}"; do
    type="${diff_menu[$diff_index]}"
    diff_types_seen="$diff_types_seen $type"
    as_dir="$TEST_TMP/diff_${diff_name}_${type}_auto_scrum"
    ag_dir="$TEST_TMP/diff_${diff_name}_${type}_agent"
    as_proj="$(new_project "$as_dir" "${diff_conf[@]}")"
    mkdir -p "$ag_dir"
    cp -a "$as_proj" "$ag_dir/proj"
    ag_proj="$ag_dir/proj"
    label="diferencial $diff_name, $type (jail $diff_jail)"
    run_auto_scrum "$as_dir" "$as_proj" "$type" "$((diff_index + 1))"
    # O mesmo HOME nos dois (o aviso da jail cita as toolchains do home).
    render "$ag_dir" "$ag_proj" "$type" HOME="$as_dir/home"
    ag_log="$(sed -n '1s/^log=//p' "$ag_dir/out" 2> /dev/null || true)"
    as_log="$(find "$as_dir/logs" -name "${type}_*.md" 2> /dev/null | head -n 1 || true)"
    assert_eq "0|0" "$(file_of "$as_dir/rc")|$(file_of "$ag_dir/rc")" "$label: o auto_scrum e o --render saem com 0$(file_of "$ag_dir/err")"
    assert_eq "true" "$([ -s "$as_log" ] && cmp -s "$as_log" "$ag_log" && echo true || echo false)" "$label: o log do --render é igual ao do auto_scrum.sh, byte a byte"
    rm -f "$ag_dir/order"
    run_agent "$ag_dir" "$ag_proj" HOME="$as_dir/home" -- --session-id "$SESSION_ID" --name "$AGENT_NAME" --type "$type" --prompt-file "$ag_log"
    assert_eq "0" "$(file_of "$ag_dir/rc")" "$label: o lançamento sai com 0"
    assert_contains "$(file_of "$as_dir/claude.argv")" "## Troca de arquivos com o usuário" "$label: guarda: o claude do auto_scrum recebeu o prompt"
    assert_eq "$(file_of "$as_dir/claude.argv")" "$(without_session "$ag_dir/claude.argv")" "$label: o argv do claude é o do auto_scrum.sh, tirando --session-id <uuid> --name <nome>"
    assert_eq "--session-id"$'\n'"$SESSION_ID"$'\n'"--name"$'\n'"$AGENT_NAME" "$(grep -A3 -x -- '--session-id' "$ag_dir/claude.argv" 2> /dev/null || true)" "$label: guarda: o launcher passa --session-id <uuid> --name <nome>"
    if [ "$diff_jail" = "true" ]; then
      assert_contains "$(options_until_dashes "$as_dir/jail.argv")" "--forward-port" "$label: guarda: o ai-jail do auto_scrum foi chamado com as opções"
      assert_eq "$(options_until_dashes "$as_dir/jail.argv")" "$(options_until_dashes "$ag_dir/jail.argv")" "$label: o ai-jail recebe as mesmas opções até o --"
    else
      assert_not_exists "$as_dir/jail.argv" "$label: o auto_scrum não chama o ai-jail"
      assert_not_exists "$ag_dir/jail.argv" "$label: o launcher não chama o ai-jail"
    fi
  done
done
for type in "${ALL_TYPES[@]}"; do
  assert_contains "$diff_types_seen " " $type " "diferencial: o tipo $type entrou na matriz"
done
# Controle positivo do without_session: só as 4 linhas certas saem.
printf '%s\n' a --session-id "$SESSION_ID" --name "$AGENT_NAME" b > "$TEST_TMP/ws_sample"
assert_eq "a"$'\n'"b" "$(without_session "$TEST_TMP/ws_sample")" "controle positivo: without_session tira só --session-id <uuid> --name <nome>"


echo
echo "== --has-conversation: diz se o claude tem a conversa, sem configuração e sem gravar nada (NS-39) =="
# conv_file <config_dir> <dir>: o arquivo que o claude grava para a sessão SESSION_ID com o cwd
# em <dir>, calculado aqui por outro caminho (o sed com locale UTF-8, um caractere por vez): todo
# caractere fora de A-Z, a-z e 0-9 vira "-".
conv_file() {
  printf '%s/projects/%s/%s.jsonl' "$1" "$(printf '%s' "$2" | LC_ALL=C.UTF-8 sed 's/[^A-Za-z0-9]/-/g')" "$SESSION_ID"
}
# has_conv <case_dir> <cwd> [VAR=valor...]: run_agent com --has-conversation SESSION_ID.
has_conv() {
  local case_dir="$1" cwd="$2"
  shift 2
  run_agent "$case_dir" "$cwd" "$@" -- --has-conversation "$SESSION_ID"
}
# Um projeto sem .nick/nick.conf: o modo não depende da configuração.
case_dir="$TEST_TMP/conv_cfg"
proj="$case_dir/proj"
mkdir -p "$proj" "$case_dir/cfg"
before="$(tree_snapshot "$proj")"
has_conv "$case_dir" "$proj" CLAUDE_CONFIG_DIR="$case_dir/cfg"
assert_eq "0|conversation=false" "$(file_of "$case_dir/rc")|$(file_of "$case_dir/out")" "sem o arquivo da conversa: conversation=false, saída 0"
assert_eq "" "$(file_of "$case_dir/err")" "sem o arquivo: nada no stderr"
expected_conv="$(conv_file "$case_dir/cfg" "$proj")"
mkdir -p "$(dirname "$expected_conv")"
: > "$expected_conv"
has_conv "$case_dir" "$proj" CLAUDE_CONFIG_DIR="$case_dir/cfg"
assert_eq "0|conversation=true" "$(file_of "$case_dir/rc")|$(file_of "$case_dir/out")" "com <CLAUDE_CONFIG_DIR>/projects/<cwd>/<uuid>.jsonl: conversation=true"
assert_eq "" "$(file_of "$case_dir/order")" "--has-conversation: nem o ai-jail nem o claude"
assert_eq "$before" "$(tree_snapshot "$proj")" "--has-conversation: nada muda no projeto (retrato recursivo)"
assert_not_exists "$proj/.nick" "--has-conversation: sem .nick/ criado"
# Outra sessão do mesmo projeto não conta.
run_agent "$case_dir" "$proj" CLAUDE_CONFIG_DIR="$case_dir/cfg" -- --has-conversation "1a2b3c4d-0000-4000-8000-000000000000"
assert_eq "conversation=false" "$(file_of "$case_dir/out")" "o arquivo de outra sessão não conta"
# Sem CLAUDE_CONFIG_DIR: o ~/.claude do HOME.
case_dir="$TEST_TMP/conv_home"
proj="$case_dir/proj"
mkdir -p "$proj"
home_conv="$(conv_file "$case_dir/home/.claude" "$proj")"
mkdir -p "$(dirname "$home_conv")"
: > "$home_conv"
has_conv "$case_dir" "$proj"
assert_eq "0|conversation=true" "$(file_of "$case_dir/rc")|$(file_of "$case_dir/out")" "sem CLAUDE_CONFIG_DIR: o \$HOME/.claude/projects/"
has_conv "$case_dir" "$proj" CLAUDE_CONFIG_DIR="$case_dir/outro"
assert_eq "conversation=false" "$(file_of "$case_dir/out")" "controle: com CLAUDE_CONFIG_DIR, o \$HOME/.claude não é olhado"
# Um cwd com _, ., espaço e acento: cada caractere vira um "-" (o é também, como o claude faz),
# mesmo com o locale C de quem chama (o launcher força UTF-8).
case_dir="$TEST_TMP/conv_chars"
proj="$case_dir/meu_proj.v2 com é"
mkdir -p "$proj" "$case_dir/cfg"
chars_conv="$(conv_file "$case_dir/cfg" "$proj")"
assert_contains "$chars_conv" "-meu-proj-v2-com--/" "guarda: o caminho esperado troca _, ., espaço e o é (um - só)"
mkdir -p "$(dirname "$chars_conv")"
: > "$chars_conv"
for locale_value in C.UTF-8 C POSIX; do
  has_conv "$case_dir" "$proj" CLAUDE_CONFIG_DIR="$case_dir/cfg" LC_ALL="$locale_value" LANG="$locale_value"
  assert_eq "0|conversation=true" "$(file_of "$case_dir/rc")|$(file_of "$case_dir/out")" "cwd com _, ., espaço e acento, LC_ALL=$locale_value: achada"
done
# O caminho físico (pwd -P), e não o do link.
case_dir="$TEST_TMP/conv_link"
mkdir -p "$case_dir/real/proj" "$case_dir/cfg"
ln -s "$case_dir/real" "$case_dir/link"
link_conv="$(conv_file "$case_dir/cfg" "$case_dir/real/proj")"
mkdir -p "$(dirname "$link_conv")"
: > "$link_conv"
has_conv "$case_dir" "$case_dir/link/proj" CLAUDE_CONFIG_DIR="$case_dir/cfg"
assert_eq "conversation=true" "$(file_of "$case_dir/out")" "cwd por um link: vale o caminho físico (pwd -P)"
# Argumentos inválidos.
case_dir="$TEST_TMP/conv_args"
mkdir -p "$case_dir/proj"
for args in "--has-conversation" "--has-conversation $SESSION_ID x" "--has-conversation $SESSION_ID --check"; do
  # Lista de propósito: cada palavra é um argumento.
  # shellcheck disable=SC2086
  run_agent "$case_dir" "$case_dir/proj" -- $args
  assert_eq "1" "$(file_of "$case_dir/rc")" "argumentos '$args': sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "$USAGE" "argumentos '$args': o uso no stderr"
  assert_eq "" "$(file_of "$case_dir/out")" "argumentos '$args': nada no stdout"
done
run_agent "$case_dir" "$case_dir/proj" -- --has-conversation ""
assert_eq "1" "$(file_of "$case_dir/rc")" "--has-conversation com UUID vazio: sai com 1"

echo
echo "== --resume: reabre a conversa pelo mesmo caminho do lançamento, sem prompt (NS-39) =="
# O aviso do ID novo, escrito aqui à mão (o texto é do bash, auto_scrum/prompt.sh).
id_notice() {
  printf 'O ID deste ticket mudou de %s para %s depois que esta conversa começou. A pasta do ticket agora é .nick/docs/%s/ (antes era .nick/docs/%s/): use o caminho novo para o plano, o review e os artefatos.' "$1" "$2" "$2" "$1"
}
# expected_resume_argv <case_dir> <jail> <hosts> <ports> <stage> <notice do ID>: o argv do claude
# na retomada: as regras, --permission-mode auto só com development, um --append-system-prompt só
# (o aviso da jail e/ou o do ID, separados por uma linha em branco) e --resume/--name por último.
expected_resume_argv() {
  local case_dir="$1" jail="$2" hosts="$3" ports="$4" stage="$5" id_text="$6" notice="" args=()
  args=(--settings "$(cat "$PERMISSIONS_JSON")")
  [ "$stage" = "development" ] && args+=(--permission-mode auto)
  if [ "$jail" = "true" ]; then
    notice="$(lib_call "$case_dir" jail_agent_notice "$hosts" "$ports")"
  fi
  if [ -n "$id_text" ]; then
    if [ -n "$notice" ]; then
      notice="$notice"$'\n\n'"$id_text"
    else
      notice="$id_text"
    fi
  fi
  if [ -n "$notice" ]; then
    args+=(--append-system-prompt "$notice")
  fi
  args+=(--resume "$SESSION_ID" --name "$AGENT_NAME")
  printf '%s\n' "${args[@]}"
}
# resume <case_dir> <cwd> <stage> <opened> <ticket> [VAR=valor...]: run_agent com --resume (sem
# --opened-id quando <opened> é vazio).
resume() {
  local case_dir="$1" cwd="$2" stage="$3" opened="$4" ticket="$5" args
  shift 5
  args=(--resume "$SESSION_ID" --name "$AGENT_NAME" --stage "$stage")
  [ -n "$opened" ] && args+=(--opened-id "$opened")
  args+=(--ticket-id "$ticket")
  run_agent "$case_dir" "$cwd" "$@" -- "${args[@]}"
}
# Jail ligada, o mesmo ID: o argv do ai-jail é o do lançamento, e o claude recebe só o aviso da
# jail.
case_dir="$TEST_TMP/resume_jail_on"
proj="$(new_project "$case_dir" "${jail_on_conf[@]}")"
resume "$case_dir" "$proj" po NS-1 NS-1
hosts="$(hosts_of "$case_dir" true true "$hosts_example")"
assert_eq "0" "$(file_of "$case_dir/rc")" "retomada com a jail ligada: sai com 0$(file_of "$case_dir/err")"
assert_eq "$(expected_resume_argv "$case_dir" true "$hosts" "$ports_example" po "")" "$(file_of "$case_dir/claude.argv")" \
  "jail ligada, mesmo ID: claude --settings <regras> --append-system-prompt <aviso da jail> --resume <uuid> --name <nome>"
assert_eq "ai-jail -- true"$'\n'"ai-jail"$'\n'"claude" "$(file_of "$case_dir/order")" "jail ligada: a checagem, depois o lançamento pelo ai-jail"
assert_contains "$(file_of "$case_dir/out")" "Jail (ai-jail): ativa" "jail ligada: o jail_print_status no terminal"
assert_eq "PWD=$proj" "$(env_pwd "$case_dir")" "a retomada roda com o cwd no projeto"
assert_eq "--name"$'\n'"$AGENT_NAME" "$(tail -n 2 "$case_dir/claude.argv" 2> /dev/null || true)" "sem prompt posicional: o último argumento é o nome"
assert_exists "$proj/.nick/.gitignore" "a retomada roda o config_dir_prepare, como o lançamento"
# Diferencial: as opções do ai-jail até o -- são as do lançamento normal com a mesma configuração.
diff_launch="$TEST_TMP/resume_jail_on_launch"
mkdir -p "$diff_launch"
cp -a "$proj" "$diff_launch/proj"
launch "$diff_launch" "$diff_launch/proj" HOME="$case_dir/home"
assert_contains "$(options_until_dashes "$diff_launch/jail.argv")" "--forward-port" "guarda: o lançamento chamou o ai-jail com as opções"
assert_eq "$(options_until_dashes "$diff_launch/jail.argv")" "$(options_until_dashes "$case_dir/jail.argv")" \
  "diferencial: a retomada chama o ai-jail com as mesmas opções do lançamento até o --"
# Jail ligada, o ID mudou: os dois avisos num --append-system-prompt só.
case_dir="$TEST_TMP/resume_jail_on_id"
proj="$(new_project "$case_dir" "${jail_on_conf[@]}")"
resume "$case_dir" "$proj" review NS-1 NS-9
hosts="$(hosts_of "$case_dir" true true "$hosts_example")"
assert_eq "$(expected_resume_argv "$case_dir" true "$hosts" "$ports_example" review "$(id_notice NS-1 NS-9)")" "$(file_of "$case_dir/claude.argv")" \
  "jail ligada, ID mudou: o aviso da jail, uma linha em branco e o aviso do ID, num --append-system-prompt só"
assert_eq "1" "$(grep -cx -- '--append-system-prompt' "$case_dir/claude.argv" || true)" "um único --append-system-prompt"
assert_contains "$(file_of "$case_dir/claude.argv")" ".nick/docs/NS-9/" "o aviso cita a pasta nova"
# Jail desligada: só o aviso do ID, ou nenhum.
case_dir="$TEST_TMP/resume_jail_off_id"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
resume "$case_dir" "$proj" tech_leader 3f9a0c7e SC-7
assert_eq "0" "$(file_of "$case_dir/rc")" "jail desligada: sai com 0"
assert_eq "$(expected_resume_argv "$case_dir" false "" "" tech_leader "$(id_notice 3f9a0c7e SC-7)")" "$(file_of "$case_dir/claude.argv")" \
  "jail desligada, ID mudou: só o aviso do ID"
assert_not_exists "$case_dir/jail.argv" "jail desligada: o ai-jail não é chamado"
assert_contains "$(file_of "$case_dir/out")" "Jail (ai-jail): desligada" "jail desligada: o jail_print_status"
for same_case in "NS-1|NS-1" "|NS-1"; do
  IFS='|' read -r same_opened same_ticket <<< "$same_case"
  case_dir="$TEST_TMP/resume_jail_off_same_${same_opened:-sem}"
  proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
  resume "$case_dir" "$proj" po "$same_opened" "$same_ticket"
  assert_eq "$(expected_resume_argv "$case_dir" false "" "" po "")" "$(file_of "$case_dir/claude.argv")" \
    "jail desligada, --opened-id '${same_opened}' e --ticket-id $same_ticket: nenhum --append-system-prompt"
  assert_not_contains "$(file_of "$case_dir/claude.argv")" "--append-system-prompt" "sem nenhum aviso, nenhum --append-system-prompt (opened '${same_opened}')"
done
# A etapa decide o modo auto: só development.
for stage in po tech_leader development review; do
  case_dir="$TEST_TMP/resume_stage_$stage"
  proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
  resume "$case_dir" "$proj" "$stage" NS-1 NS-1
  assert_eq "$(expected_resume_argv "$case_dir" false "" "" "$stage" "")" "$(file_of "$case_dir/claude.argv")" "--stage $stage: o argv da retomada"
  if [ "$stage" = "development" ]; then
    assert_contains "$(file_of "$case_dir/claude.argv")" "--permission-mode"$'\n'"auto" "--stage development: --permission-mode auto"
  else
    assert_not_contains "$(file_of "$case_dir/claude.argv")" "--permission-mode" "--stage $stage: sem --permission-mode"
  fi
done
assert_not_exists "$TEST_TMP/resume_stage_po/logs" "retomada: nenhum log de prompt"
# Recusas dos argumentos: saída 1, o uso, nada chamado e nada criado no projeto.
case_dir="$TEST_TMP/resume_args"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
r="--resume $SESSION_ID --name x"
for args in "--resume" "$r" "$r --stage po" "$r --ticket-id NS-1" "$r --stage xpto --ticket-id NS-1" "$r --stage PO --ticket-id NS-1" \
  "$r --stage po_jira --ticket-id NS-1" "$r --stage po --ticket-id a/b" "$r --stage po --ticket-id .." "$r --stage po --opened-id a/b --ticket-id NS-1" \
  "$r --stage po --ticket-id NS-1 --stage review" "$r --stage po --ticket-id NS-1 --name y" "$r --stage po --ticket-id NS-1 --foo" \
  "$r --stage po --ticket-id NS-1 --type po" "$r --stage po --ticket-id NS-1 --session-id $SESSION_ID" "$r --stage po --ticket-id NS-1 --check" \
  "$r --stage po --ticket-id" "$r --stage po --ticket-id NS-1 --resume $SESSION_ID" "--session-id $SESSION_ID --name x --type po --resume $SESSION_ID"; do
  rm -f "$case_dir/order"
  # Lista de propósito: cada palavra é um argumento.
  # shellcheck disable=SC2086
  run_agent "$case_dir" "$proj" -- $args
  assert_eq "1" "$(file_of "$case_dir/rc")" "argumentos '$args': sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "Erro: " "argumentos '$args': Erro: no stderr"
  assert_contains "$(file_of "$case_dir/err")" "$USAGE" "argumentos '$args': o uso no stderr"
  assert_eq "" "$(file_of "$case_dir/order")" "argumentos '$args': nada é chamado"
done
for empty_case in resume name ticket; do
  rm -f "$case_dir/order"
  case "$empty_case" in
    resume) run_agent "$case_dir" "$proj" -- --resume "" --name x --stage po --ticket-id NS-1 ;;
    name) run_agent "$case_dir" "$proj" -- --resume "$SESSION_ID" --name "" --stage po --ticket-id NS-1 ;;
    ticket) run_agent "$case_dir" "$proj" -- --resume "$SESSION_ID" --name x --stage po --ticket-id "" ;;
  esac
  assert_eq "1" "$(file_of "$case_dir/rc")" "valor vazio ($empty_case): sai com 1"
  assert_eq "" "$(file_of "$case_dir/order")" "valor vazio ($empty_case): nada é chamado"
done
assert_not_exists "$proj/.nick/.gitignore" "argumentos inválidos da retomada: nada criado no projeto"
rm -f "$case_dir/order"
run_agent "$case_dir" "$proj" -- --ticket-id NS-2 --stage review --opened-id NS-1 --name "$AGENT_NAME" --resume "$SESSION_ID"
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: os argumentos da retomada em qualquer ordem também valem"
assert_contains "$(file_of "$case_dir/claude.argv")" ".nick/docs/NS-2/" "controle positivo: em qualquer ordem, o aviso do ID novo"

echo
echo "== --resume com --epic-id: o agente de um épico (NS-40) =="
# O aviso do ID novo de um épico, escrito aqui à mão (o texto é do bash, auto_scrum/prompt.sh).
epic_id_notice() {
  printf 'O ID deste épico mudou de %s para %s depois que esta conversa começou. A pasta do épico agora é .nick/docs/%s/ (antes era .nick/docs/%s/): use o caminho novo para o plano e os artefatos.' "$1" "$2" "$2" "$1"
}
# resume_epic <case_dir> <cwd> <stage> <opened> <epic> [VAR=valor...]: o resume com --epic-id.
resume_epic() {
  local case_dir="$1" cwd="$2" stage="$3" opened="$4" epic="$5" args
  shift 5
  args=(--resume "$SESSION_ID" --name "$AGENT_NAME" --stage "$stage")
  [ -n "$opened" ] && args+=(--opened-id "$opened")
  args+=(--epic-id "$epic")
  run_agent "$case_dir" "$cwd" "$@" -- "${args[@]}"
}
# Jail ligada, o ID do épico mudou: o aviso da jail e o do épico num --append-system-prompt só.
case_dir="$TEST_TMP/resume_epic_jail_on_id"
proj="$(new_project "$case_dir" "${jail_on_conf[@]}")"
resume_epic "$case_dir" "$proj" tech_leader EPIC-1 EPIC-2
hosts="$(hosts_of "$case_dir" true true "$hosts_example")"
assert_eq "0" "$(file_of "$case_dir/rc")" "--epic-id com a jail ligada: sai com 0$(file_of "$case_dir/err")"
assert_eq "$(expected_resume_argv "$case_dir" true "$hosts" "$ports_example" tech_leader "$(epic_id_notice EPIC-1 EPIC-2)")" "$(file_of "$case_dir/claude.argv")" \
  "--epic-id, ID mudou: o aviso da jail, uma linha em branco e o aviso do épico, num --append-system-prompt só"
assert_eq "1" "$(grep -cx -- '--append-system-prompt' "$case_dir/claude.argv" || true)" "--epic-id: um único --append-system-prompt"
assert_contains "$(file_of "$case_dir/claude.argv")" "O ID deste épico mudou" "--epic-id: o texto do épico"
assert_not_contains "$(file_of "$case_dir/claude.argv")" "O ID deste ticket" "--epic-id: não o texto do ticket"
# Jail desligada: só o aviso do épico, ou nenhum com o mesmo ID (ou sem --opened-id).
case_dir="$TEST_TMP/resume_epic_jail_off_id"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
resume_epic "$case_dir" "$proj" po 3f9a0c7e NS-28
assert_eq "$(expected_resume_argv "$case_dir" false "" "" po "$(epic_id_notice 3f9a0c7e NS-28)")" "$(file_of "$case_dir/claude.argv")" \
  "--epic-id com a jail desligada, ID mudou: só o aviso do épico"
for same_case in "EPIC-1|EPIC-1" "|EPIC-1"; do
  IFS='|' read -r same_opened same_epic <<< "$same_case"
  case_dir="$TEST_TMP/resume_epic_same_${same_opened:-sem}"
  proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
  resume_epic "$case_dir" "$proj" po "$same_opened" "$same_epic"
  assert_eq "$(expected_resume_argv "$case_dir" false "" "" po "")" "$(file_of "$case_dir/claude.argv")" \
    "--epic-id $same_epic, --opened-id '${same_opened}': nenhum aviso"
done
# Recusas: os dois juntos, nenhum, repetido, sem valor, inválido e vazio.
case_dir="$TEST_TMP/resume_epic_args"
proj="$(new_project "$case_dir" JAIL_ENABLED='"false"')"
r="--resume $SESSION_ID --name x --stage po"
for args in "$r --ticket-id NS-1 --epic-id E-1" "$r --epic-id E-1 --ticket-id NS-1" "$r" "$r --epic-id E-1 --epic-id E-2" \
  "$r --epic-id" "$r --epic-id a/b" "$r --epic-id .." "$r --opened-id a/b --epic-id E-1"; do
  rm -f "$case_dir/order"
  # Lista de propósito: cada palavra é um argumento.
  # shellcheck disable=SC2086
  run_agent "$case_dir" "$proj" -- $args
  assert_eq "1" "$(file_of "$case_dir/rc")" "argumentos '$args': sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "$USAGE" "argumentos '$args': o uso no stderr"
  assert_eq "" "$(file_of "$case_dir/order")" "argumentos '$args': nada é chamado"
done
run_agent "$case_dir" "$proj" -- --resume "$SESSION_ID" --name x --stage po --ticket-id NS-1 --epic-id E-1
assert_contains "$(file_of "$case_dir/err")" "informe só um dos dois: --ticket-id ou --epic-id." "os dois juntos: a recusa diz por quê"
run_agent "$case_dir" "$proj" -- --resume "$SESSION_ID" --name x --stage po
assert_contains "$(file_of "$case_dir/err")" "informe o --ticket-id <id> ou o --epic-id <id>." "nenhum dos dois: a recusa diz por quê"
rm -f "$case_dir/order"
run_agent "$case_dir" "$proj" -- --resume "$SESSION_ID" --name x --stage po --epic-id ""
assert_eq "1" "$(file_of "$case_dir/rc")" "--epic-id vazio: sai com 1"
assert_eq "" "$(file_of "$case_dir/order")" "--epic-id vazio: nada é chamado"
assert_not_exists "$proj/.nick/.gitignore" "argumentos inválidos do --epic-id: nada criado no projeto"
run_agent "$case_dir" "$proj" -- --epic-id E-2 --stage po --opened-id E-1 --name "$AGENT_NAME" --resume "$SESSION_ID"
assert_eq "0" "$(file_of "$case_dir/rc")" "controle positivo: --epic-id em qualquer ordem também vale"
assert_contains "$(file_of "$case_dir/claude.argv")" "A pasta do épico agora é .nick/docs/E-2/" "controle positivo: o aviso do épico"

echo
echo "== resume_id_notice: o texto do ticket byte a byte, e o do épico (NS-40) =="
# notice_of <args...>: a saída de resume_id_notice do prompt.sh real com <args>.
notice_of() {
  # Aspas simples intencionais: os $1/$@ são do bash -c, não daqui.
  # shellcheck disable=SC2016
  "$BASH_BIN" -c 'source "$1"; source "$2"; shift 2; resume_id_notice "$@"' _ "$ROOT_DIR/lib/config/config.sh" "$ROOT_DIR/auto_scrum/prompt.sh" "$@"
}
assert_eq "$(id_notice NS-1 NS-9)" "$(notice_of NS-1 NS-9)" "sem o terceiro argumento: o texto do ticket de hoje, byte a byte"
assert_eq "$(id_notice NS-1 NS-9)" "$(notice_of NS-1 NS-9 ticket)" "com ticket: o mesmo texto"
assert_eq "$(epic_id_notice E-1 E-2)" "$(notice_of E-1 E-2 epic)" "com epic: o texto do épico"

echo
echo "== estrutura do launcher =="
launcher_code="$(code_lines "$LAUNCHER")"
assert_contains "$(head -n 1 "$LAUNCHER")" "#!/usr/bin/env bash" "o launcher é um script bash (shebang)"
assert_eq "set -euo pipefail" "$(printf '%s\n' "$launcher_code" | grep -v '^[[:space:]]*$' | sed -n 1p)" "a 1ª linha de código depois do shebang (e do cabeçalho) é set -euo pipefail"
assert_eq "x" "$([ -x "$LAUNCHER" ] && echo x || true)" "o launcher tem bit de execução (como o bin/nick-init)"
# shellcheck disable=SC2016
assert_eq 'source "$SCRIPT_DIR/../lib/config/config.sh"'$'\n''source "$SCRIPT_DIR/../lib/config/keys.sh"'$'\n''source "$SCRIPT_DIR/../lib/jail/jail.sh"'$'\n''source "$SCRIPT_DIR/../lib/claude/claude.sh"'$'\n''source "$SCRIPT_DIR/../auto_scrum/prompt.sh"' \
  "$(printf '%s\n' "$launcher_code" | grep -E '^[[:space:]]*(source|\.)[[:space:]]' || true)" "as únicas linhas source são as quatro do lib/, na ordem do auto_scrum.sh, e a quinta, do auto_scrum/prompt.sh"
assert_eq "config_register_nick_keys" "$(printf '%s\n' "$launcher_code" | grep -v '^[[:space:]]*$' | grep -A1 'auto_scrum/prompt.sh"$' | tail -n 1)" \
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
# --permission-mode: uma única ocorrência no código, condicionada ao development (adaptado no
# NS-35: até o NS-34 o launcher nunca o passava).
# permission_lines <file>: as linhas de código de <file> com --permission-mode.
permission_lines() {
  code_lines "$1" | grep -F -- '--permission-mode' || true
}
# shellcheck disable=SC2016
assert_eq '  [ "$type" = "development" ] && claude_args+=(--permission-mode auto)' "$(permission_lines "$LAUNCHER")" \
  "o launcher passa --permission-mode numa linha só, condicionada a --type development"
# shellcheck disable=SC2016
printf '%s\n' '# --permission-mode no comentário' 'claude_args+=(--permission-mode auto)' '  [ "$type" = "development" ] && claude_args+=(--permission-mode auto)' > "$sample"
# shellcheck disable=SC2016
assert_eq 'claude_args+=(--permission-mode auto)'$'\n''  [ "$type" = "development" ] && claude_args+=(--permission-mode auto)' "$(permission_lines "$sample")" \
  "controle positivo: o filtro acha toda linha de código com --permission-mode (e ignora comentário)"
assert_eq "" "$(printf '%s\n' "$launcher_code" | grep -E '(^|[^A-Z_])CLAUDE_[A-Z_]*=' || true)" "nenhuma global CLAUDE_* atribuída (só NICK_CLAUDE_*)"
# shellcheck disable=SC2016
assert_eq "x" "$(printf '%s\n' 'CLAUDE_X=1' | grep -qE '(^|[^A-Z_])CLAUDE_[A-Z_]*=' && echo x || true)" "controle positivo: o filtro acha CLAUDE_X="
assert_contains "$launcher_code" "jail_mcp_hosts " "os hosts dos MCPs saem da lib (jail_mcp_hosts)"
assert_not_contains "$launcher_code" "mcp.atlassian.com" "o launcher não repete os hosts dos MCPs"
assert_eq "" "$(printf '%s\n' "$launcher_code" | grep -nE "envsubst|templates/|build_[a-z]+_blocks[^_]" || true)" "o launcher não monta prompt por conta própria (sem envsubst, templates/ nem build_*_blocks: tudo pelo prompt.sh)"
# Os tipos do launcher são os do choose_type (com tudo ligado), na mesma ordem.
choose_keys="$(timeout 5 "$BASH_BIN" -c "source '$AUTO_SCRUM_SH'; JIRA_ENABLED=true; SENTRY_ENABLED=true; for n in 1 2 3 4 5 6 7 8; do choose_type <<< \"\$n\" > /dev/null 2>&1; printf '%s\n' \"\$TYPE\"; done" 2> /dev/null || true)"
assert_eq "$(printf '%s\n' "${ALL_TYPES[@]}")" "$choose_keys" "guarda: os 8 tipos desta suíte são os do choose_type, na ordem"
assert_eq "$(printf '%s ' "${ALL_TYPES[@]}")" "$(sed -n 's/^AGENT_TYPES=(\(.*\))$/\1 /p' "$LAUNCHER")" "AGENT_TYPES do launcher são os 8 tipos do choose_type, na ordem"
assert_eq "$real_logs_before" "$(ls -la "$REAL_LOGS_DIR" 2> /dev/null || true)" "nenhum caso escreveu no auto_scrum/logs/ real"
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
