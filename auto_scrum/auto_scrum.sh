#!/usr/bin/env bash
#
# auto_scrum: dispara conversas Claude Code a partir de templates por tipo
# (PO, Tech Leader, Development, Review).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMPLATES_DIR="$SCRIPT_DIR/templates"
LOGS_DIR="$SCRIPT_DIR/logs"
# The command that creates the project config (.nick/nick.conf) in the cwd, absolute and
# normalized so the user can paste it from the error messages: the nick isn't installed in the
# PATH yet.
NICK_INIT_BIN="$(cd "$SCRIPT_DIR/.." && pwd)/bin/nick-init"
# Code shared with the other nick tools lives in lib/: the project config reader
# (lib/config/config.sh) and the registry of its keys (lib/config/keys.sh), the ai-jail
# sandbox (lib/jail/jail.sh) and the Claude Code deny/ask rules (lib/claude/claude.sh, which
# reads lib/claude/permissions.json). The paths are relative to this script, so auto_scrum/
# depends on its sibling lib/.
# shellcheck source=SCRIPTDIR/../lib/config/config.sh
source "$SCRIPT_DIR/../lib/config/config.sh"
# shellcheck source=SCRIPTDIR/../lib/config/keys.sh
source "$SCRIPT_DIR/../lib/config/keys.sh"
# shellcheck source=SCRIPTDIR/../lib/jail/jail.sh
source "$SCRIPT_DIR/../lib/jail/jail.sh"
# shellcheck source=SCRIPTDIR/../lib/claude/claude.sh
source "$SCRIPT_DIR/../lib/claude/claude.sh"
# The prompt assembly (the blocks, the templates, envsubst and the exchange block), shared with
# bin/nick-agent, which builds the same prompt for the agents of the nick shell (NS-35).
# shellcheck source=SCRIPTDIR/prompt.sh
source "$SCRIPT_DIR/prompt.sh"

# The project config keys are registered in lib/config/keys.sh, the only place that lists
# them (see ABOUT.md). A bad registration stops the script on load (set -e): it is a
# programming error.
config_register_nick_keys

# The answers, the blocks and the names of the .nick/ tree the prompt uses live in prompt.sh,
# with the code that assembles it (NS-35). The project config keys stay here: config_load
# writes them.
STACK_DESCRIPTION=""
SENTRY_ENABLED=""
PERMISSION_ENABLED=""
JIRA_ENABLED=""
PRODUCTION_ENABLED=""
TDD_ENABLED=""
JAIL_ENABLED=""
JAIL_CLIPBOARD=""
JAIL_FORWARD_PORTS=""
JAIL_ALLOW_HOSTS=""
# The config file load_project_config loaded (always the cwd .nick/nick.conf): the jail error
# names it.
PROJECT_CONFIG_FILE=""
# The content of lib/claude/permissions.json (claude_settings_load), passed inline to every
# claude launch as --settings. Never CLAUDE_*: Claude Code reads CLAUDE_* from the environment.
NICK_CLAUDE_SETTINGS=""
TYPE=""

check_requirements() {
  local missing=()
  local editor_bin="${EDITOR:-nano}"
  editor_bin="${editor_bin%% *}"
  command -v claude >/dev/null 2>&1 || missing+=("claude")
  command -v envsubst >/dev/null 2>&1 || missing+=("envsubst (pacote gettext)")
  command -v "$editor_bin" >/dev/null 2>&1 || missing+=("$editor_bin (defina \$EDITOR ou instale nano)")
  if [ "${#missing[@]}" -gt 0 ]; then
    echo "Erro: comando(s) necessário(s) não encontrado(s) no PATH: ${missing[*]}" >&2
    echo "Instale antes de continuar (ex: sudo pacman -S gettext para envsubst)." >&2
    exit 1
  fi
}

# read_required <prompt> <variable_name>
read_required() {
  local prompt="$1" __resultvar="$2" value=""
  while [ -z "$value" ]; do
    read -r -p "$prompt" value
    [ -z "$value" ] && echo "Esse campo é obrigatório, tente de novo."
  done
  printf -v "$__resultvar" '%s' "$value"
}

# read_jira_id <variable_name>: asks for the Jira ID (read_required) until jira_id_valid accepts
# it. The message doesn't echo the value, which may carry control characters. Locals are
# prefixed with __jira_ so they can't shadow the caller's <variable_name>.
read_jira_id() {
  local __jira_result="$1" __jira_value=""
  while true; do
    read_required "ID do Jira (ex: SC-123): " __jira_value
    jira_id_valid "$__jira_value" && break
    echo "ID do Jira inválido: não pode ter /, .., espaço nem caractere de controle (ele vira o nome da pasta $NICK_DOCS_DIR/<ID>/). Tente de novo."
  done
  printf -v "$__jira_result" '%s' "$__jira_value"
}

# read_optional <prompt> <variable_name>
read_optional() {
  local prompt="$1" __resultvar="$2" value=""
  read -r -p "$prompt" value
  printf -v "$__resultvar" '%s' "$value"
}

# read_via_editor <prompt> <variable_name>
# Opens $EDITOR (fallback: nano) on a temp file so the text can be typed/pasted
# there instead of read line-by-line in the terminal — pasting a large block of
# text straight into read locks up/glitches the terminal.
read_via_editor() {
  local prompt="$1" __resultvar="$2"
  local editor="${EDITOR:-nano}"
  local tmpfile value
  echo "$prompt — abrindo $editor (salve e feche o editor para continuar)."
  while true; do
    tmpfile="$(mktemp)"
    "$editor" "$tmpfile"
    if [ -s "$tmpfile" ]; then
      break
    fi
    rm -f "$tmpfile"
    echo "Esse campo é obrigatório, tente de novo."
  done
  value="$(cat "$tmpfile")"
  rm -f "$tmpfile"
  printf -v "$__resultvar" '%s' "$value"
}

# quit_from_menu: the only way out through a menu ("Sair (q)"). Ends the script right away with
# code 0, before anything with a side effect: no log, no .nick/ tree and no claude call. Menus
# run in the main shell, never inside $(...), so this exit ends the script and not a subshell.
quit_from_menu() {
  echo "Saindo. Nada foi feito."
  exit 0
}

# select_menu <variable_name> <option>...: the only select in the script — every menu goes
# through here, so every menu gets the way out. Shows <option>... plus "Sair (q)" as the last
# numbered item, and stores in <variable_name> the 0-based index of the chosen option. The
# number of "Sair (q)" moves with the project flags, so q/Q also quits; any other input is
# invalid. The index comes from comparing REPLY with [ -eq ], not from $((REPLY - 1)):
# select accepts "08", which bash arithmetic reads as a bad octal.
# When stdin closes (Ctrl+D) it leaves <variable_name> empty and returns 1, the same status
# a bare select gave — each caller keeps its own handling for that.
# Locals are prefixed with __menu_ so they can't shadow the caller's <variable_name>; REPLY
# is local too, so what the user typed doesn't leak into the global scope.
select_menu() {
  local __menu_result="$1"
  shift
  local __menu_options=("$@" "Sair (q)")
  local __menu_choice __menu_index REPLY
  printf -v "$__menu_result" '%s' ""
  select __menu_choice in "${__menu_options[@]}"; do
    case "$REPLY" in
      q | Q) quit_from_menu ;;
    esac
    if [ -z "${__menu_choice:-}" ]; then
      echo "Opção inválida, tente novamente."
      continue
    fi
    for __menu_index in "${!__menu_options[@]}"; do
      if [ "$REPLY" -eq "$((__menu_index + 1))" ]; then
        break
      fi
    done
    if [ "$__menu_index" -eq "$#" ]; then
      quit_from_menu
    fi
    printf -v "$__menu_result" '%s' "$__menu_index"
    return 0
  done
  return 1
}

# choose_type: builds the main menu, shown through select_menu (which appends "Sair (q)" as
# the last item and ends the script on it). Two project flags (opt-in, empty/"false" by
# default) gate entry points that depend on an MCP server from the target project's
# .mcp.json, so a project without it isn't offered an option that would fail right away:
#   - JIRA_ENABLED="true" adds "po_jira" and "tech_leader_jira";
#   - SENTRY_ENABLED="true" adds "po_sentry".
# Each one goes right after the manual entry points of its role: po_jira after
# po_discussion, po_sentry after po_jira (or after po_discussion when Jira is off) and
# always before tech_leader, tech_leader_jira after tech_leader.
choose_type() {
  local options=("PO - Criar ticket" "PO - Conversar para definir ticket")
  local keys=("po" "po_discussion")
  if [ "$JIRA_ENABLED" = "true" ]; then
    options+=("PO - Criar ticket a partir do Jira")
    keys+=("po_jira")
  fi
  if [ "$SENTRY_ENABLED" = "true" ]; then
    options+=("PO - Criar ticket a partir do Sentry")
    keys+=("po_sentry")
  fi
  options+=("Tech Leader - Criar plano")
  keys+=("tech_leader")
  if [ "$JIRA_ENABLED" = "true" ]; then
    options+=("Tech Leader - Criar plano a partir do Jira")
    keys+=("tech_leader_jira")
  fi
  options+=("Desenvolvimento" "Review")
  keys+=("development" "review")
  local choice
  echo "Qual tipo de conversa você quer iniciar?"
  # stdin closed (Ctrl+D): return 1, so main stops with code 1 (set -e), as before.
  select_menu choice "${options[@]}" || return 1
  TYPE="${keys[$choice]}"
}

# load_project_config <file>: loads a project config file through config_load
# (lib/config/config.sh), which holds the reader, its grammar and the three warnings
# (format, missing key, invalid toggle value) — see the comment there. It stays here, as a
# thin call, because the tests and the rest of the script call this name, and because this is
# the auto_scrum place to record anything about the loaded file without putting a caller
# global inside the library: PROJECT_CONFIG_FILE keeps its path, for the jail error message.
load_project_config() {
  PROJECT_CONFIG_FILE="$1"
  config_load "$1"
}

# resolve_stack: sets the project config (STACK_DESCRIPTION and the *_ENABLED flags) from
# $PWD/.nick/nick.conf (config_cwd_file, lib/config/config.sh) — the only place the config
# comes from: only the cwd itself, no parent directories, no other destination. In order:
#   1. an old $PWD/.nick.conf without the new file stops the script with the migration error
#      (config_cwd_legacy_check, lib/config/config.sh);
#   2. without $PWD/.nick/nick.conf, the script stops with an error pointing to bin/nick-init
#      (NICK_INIT_BIN), which creates it — before any menu, log or claude;
#   3. otherwise load_project_config loads it and the path is shown.
resolve_stack() {
  local cwd_config
  config_cwd_legacy_check || exit 1
  cwd_config="$(config_cwd_file)"
  if [ ! -f "$cwd_config" ]; then
    echo "Erro: não há configuração do projeto em $cwd_config. Rode na raiz do projeto: $NICK_INIT_BIN" >&2
    exit 1
  fi
  load_project_config "$cwd_config"
  echo "Configuração do projeto carregada de: $cwd_config"
}

# mcp_jail_hosts: prints (space separated) the hosts of the remote MCP servers this project
# relies on, for lib/jail/jail.sh to release in the jail's network (--allow-host): ai-jail's
# default allowlist has neither. JIRA_ENABLED="true" adds mcp.atlassian.com and
# SENTRY_ENABLED="true" adds mcp.sentry.dev — the hosted servers; a self-hosted one goes in
# JAIL_ALLOW_HOSTS (README). Without the jail, the list changes nothing. The list itself lives
# in lib/ (jail_mcp_hosts), shared with bin/nick-agent (NS-33); this name stays as a thin call
# because the tests and main() use it.
mcp_jail_hosts() {
  jail_mcp_hosts "$JIRA_ENABLED" "$SENTRY_ENABLED"
}

# ask_questions_po: always the same two questions, whatever the project flags — a request
# that starts from a Sentry issue has its own entry point (ask_questions_po_sentry).
ask_questions_po() {
  read_required "Título do pedido: " TITLE
  read_via_editor "Descrição do pedido" DESCRIPTION
}

# ask_questions_po_discussion: sem título/descrição prontos — só um assunto solto pra
# puxar a conversa. O template (po_discussion.md) instrui o PO a discutir antes de
# aplicar os critérios de validade, então não faz sentido exigir aqui o que a conversa
# ainda vai construir.
ask_questions_po_discussion() {
  read_required "Assunto para discutir: " TITLE
}

# ask_questions_po_jira: o pedido já existe como ticket no Jira — pede só o link
# (obrigatório) e uma descrição extra opcional. O PO (po_jira.md) é quem busca título,
# ID, descrição e anexos via MCP do Jira a partir do link; a descrição extra aqui serve
# de complemento manual do analista, não substitui a busca.
ask_questions_po_jira() {
  read_required "Link do ticket no Jira: " JIRA_LINK
  read_optional "Descrição extra (opcional, Enter para pular): " DESCRIPTION
}

# ask_questions_po_sentry: the request starts from a Sentry issue — asks only for the link
# (required) and an optional extra description. The PO (po_sentry.md) fetches the error
# details itself via the target project's Sentry MCP; the extra description is the
# analyst's complement, not a replacement for that fetch.
ask_questions_po_sentry() {
  read_required "Link do issue no Sentry: " SENTRY_LINK
  read_optional "Descrição extra (opcional, Enter para pular): " DESCRIPTION
}

ask_questions_tech_leader() {
  read_jira_id JIRA_ID
  read_required "Título da tarefa: " TITLE
  read_via_editor "Descrição / contexto" DESCRIPTION
}

# ask_questions_tech_leader_jira: o ticket já existe no Jira — pede só o link
# (obrigatório) e um contexto extra opcional. Reaproveita JIRA_LINK/DESCRIPTION (mesmas
# variáveis de ask_questions_po_jira) em vez de criar variáveis novas: JIRA_LINK já tem a
# semântica certa de URL de entrada, e DESCRIPTION já serve de complemento opcional nesse
# padrão. O Tech Leader (tech_leader_jira.md) é quem busca título, ID, descrição e anexos
# via MCP do Jira a partir do link — JIRA_ID não é pedido aqui, só é conhecido depois,
# dentro da própria sessão do Claude.
ask_questions_tech_leader_jira() {
  read_required "Link do ticket no Jira: " JIRA_LINK
  read_optional "Contexto extra (opcional, Enter para pular): " DESCRIPTION
}

# ask_questions_development: the plan defaults to $NICK_DOCS_DIR/<ID>/plan.md, the fixed path
# the Tech Leader writes to.
ask_questions_development() {
  read_jira_id JIRA_ID
  warn_legacy_plan
  read_optional "Arquivo do plano (opcional, Enter para usar $NICK_DOCS_DIR/$JIRA_ID/plan.md): " PLAN_PATH
  PLAN_PATH="${PLAN_PATH:-$NICK_DOCS_DIR/$JIRA_ID/plan.md}"
}

# ask_questions_review: besides the plan (same default as Dev), computes REVIEW_PATH — a fixed
# path next to the plan, not asked of the user, since it's always the same kind of content
# (review result).
ask_questions_review() {
  read_jira_id JIRA_ID
  warn_legacy_plan
  read_optional "Arquivo do plano (opcional, Enter para usar $NICK_DOCS_DIR/$JIRA_ID/plan.md): " PLAN_PATH
  PLAN_PATH="${PLAN_PATH:-$NICK_DOCS_DIR/$JIRA_ID/plan.md}"
  REVIEW_PATH="$NICK_DOCS_DIR/$JIRA_ID/review.md"
}

main() {
  local arg jail_requirements_flag=0
  for arg in "$@"; do
    case "$arg" in
      # The two flags that left auto_scrum (the config now lives only in the cwd
      # .nick/nick.conf, created by bin/nick-init) are refused before anything else, pointing
      # to the command that replaces them.
      --init)
        echo "Erro: o --init saiu do auto_scrum. Para criar a configuração do projeto, rode na raiz dele: $NICK_INIT_BIN" >&2
        exit 1
        ;;
      --projeto=*)
        echo "Erro: o --projeto= saiu do auto_scrum: a configuração vem só do .nick/nick.conf do diretório atual. Para criá-la, rode na raiz do projeto: $NICK_INIT_BIN" >&2
        exit 1
        ;;
      --requisitos-jail) jail_requirements_flag=1 ;;
      *)
        echo "Erro: argumento desconhecido: '$arg'" >&2
        echo "Uso: $(basename "$0") [--requisitos-jail]" >&2
        exit 1
        ;;
    esac
  done

  # What the jail needs on this machine, before anyone turns JAIL_ENABLED on: no config is
  # loaded, nothing runs and nothing is written.
  if [ "$jail_requirements_flag" -eq 1 ]; then
    jail_print_requirements
    exit 0
  fi

  check_requirements
  resolve_stack
  # First point where JAIL_ENABLED is known, before any question: with "true", a jail that
  # doesn't work here stops the script (no log, no claude). The .nick/nick.conf in the cwd is
  # shared with the target team, so the error warns about turning the jail off there (always
  # the case now, but the comparison stays: simplifying the jail call is out of scope).
  local config_shared="false" jail_hosts
  [ "$PROJECT_CONFIG_FILE" = "$(config_cwd_file)" ] && config_shared="true"
  # The MCP hosts first, then the project's own (JAIL_ALLOW_HOSTS); jail_check validates both.
  jail_hosts="$(mcp_jail_hosts) $JAIL_ALLOW_HOSTS"
  jail_check "$JAIL_ENABLED" "$JAIL_CLIPBOARD" "$jail_hosts" "$JAIL_FORWARD_PORTS" "$PROJECT_CONFIG_FILE" "$config_shared" || exit 1
  # The Claude Code deny/ask rules (lib/claude/) go to every session, always: without them
  # the script stops here, before any question, log or claude. --requisitos-jail never opens
  # claude, so it exits above without loading them.
  claude_settings_load NICK_CLAUDE_SETTINGS || exit 1
  # The jail status (and the JAIL_CLIPBOARD warning) shows up front, before anything else
  # runs, and again right before the confirmation.
  jail_print_status "$JAIL_ENABLED" "$JAIL_CLIPBOARD"
  build_prompt_blocks
  choose_type

  case "$TYPE" in
    po) ask_questions_po ;;
    po_discussion) ask_questions_po_discussion ;;
    po_jira) ask_questions_po_jira ;;
    po_sentry) ask_questions_po_sentry ;;
    tech_leader) ask_questions_tech_leader ;;
    tech_leader_jira) ask_questions_tech_leader_jira ;;
    development) ask_questions_development ;;
    review) ask_questions_review ;;
  esac

  mkdir -p "$LOGS_DIR"
  local timestamp log_file
  timestamp="$(date +%Y%m%d_%H%M%S)"
  log_file="$LOGS_DIR/${TYPE}_${timestamp}.md"
  render_prompt "$TEMPLATES_DIR" "$TYPE" "$log_file"

  echo
  echo "----- Prompt final (salvo em $log_file) -----"
  cat "$log_file"
  echo "----------------------------------------------"
  echo

  jail_print_status "$JAIL_ENABLED" "$JAIL_CLIPBOARD"
  local confirm
  read -r -p "Confirma o envio para o claude? (s/N): " confirm
  case "$confirm" in
    s|S|sim|Sim|SIM)
      config_dir_prepare || exit 1
      # The deny/ask rules come first, inline (the content, not a path: a path out of the
      # project may not exist inside the jail), in every stage and with or without the jail.
      local claude_args=(--settings "$NICK_CLAUDE_SETTINGS")
      # Only the Development stage runs in auto mode — it's the only one where the
      # agent actually edits code/runs commands without stepping in at every turn;
      # PO/Tech Leader/Review are text conversations, no reason to let go of the wheel.
      [ "$TYPE" = "development" ] && claude_args+=(--permission-mode auto)
      # With the jail on, the agent is told so (jail_agent_notice, lib/jail/jail.sh):
      # otherwise it spends tokens diagnosing the machine for what the jail hides (a
      # toolchain, a home dir, a localhost service, a website). Off, the argv stays the same
      # as before the jail existed.
      if [ "$JAIL_ENABLED" = "true" ]; then
        claude_args+=(--append-system-prompt "$(jail_agent_notice "$jail_hosts" "$JAIL_FORWARD_PORTS")")
      fi
      # Nothing to wire here for Sentry: the MCP server itself lives in the target
      # project's own .mcp.json (Claude Code auto-loads it from the cwd), not in
      # auto_scrum. On the MCP side, SENTRY_ENABLED just shows the po_sentry entry point
      # in the menu (choose_type), whose template (po_sentry.md) tells the PO to use that
      # tool.
      # With JAIL_ENABLED="true", jail_exec (lib/jail/jail.sh) launches the same argv as
      # 'ai-jail <jail_options of JAIL_CLIPBOARD, the MCP hosts, JAIL_ALLOW_HOSTS and
      # JAIL_FORWARD_PORTS> -- claude --settings <rules> ...'; otherwise it runs
      # 'claude --settings <rules> ...' directly.
      jail_exec "$JAIL_ENABLED" "$JAIL_CLIPBOARD" "$jail_hosts" "$JAIL_FORWARD_PORTS" claude "${claude_args[@]}" "$(cat "$log_file")"
      ;;
    *)
      echo "Cancelado. O prompt gerado continua salvo em: $log_file"
      exit 0
      ;;
  esac
}

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
  main "$@"
fi
