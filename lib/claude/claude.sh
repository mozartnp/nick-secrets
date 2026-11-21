# shellcheck shell=bash
#
# claude.sh: the Claude Code permission rules (deny/ask) that every session opened by a nick
# tool gets — auto_scrum and bin/nick-agent use it. The rules live in permissions.json, next to
# this file, the single source of them ({"permissions": {"deny": [...], "ask": [...]}} and, since
# the NS-50, "disableAgentView": true, which turns off the background sessions of claude; nothing
# else); this library only finds that file and reads it (claude_settings_load), and prints the error
# when it can't (claude_print_settings_error); since the NS-39 it also says where claude keeps the
# conversation of a session (claude_conversation_file), since the NS-50 which session that
# conversation went on in (claude_conversation_latest), and since the NS-41 it holds the system
# instruction not to work around a command those rules refused (claude_permission_notice), which
# every session gets first in its single --append-system-prompt. The tool passes the content
# inline to the agent, as 'claude --settings "<content>" ...': never as a path (inside the jail a
# path out of the project may not exist) and never written to a .claude/settings*.json of the
# user or of the project. Always on: there is no flag to turn the rules off and no fallback
# without them. The rules match the command text, so they are a guard rail, not a sandbox (see
# ABOUT.md, "Regras de deny/ask do Claude Code (lib/claude/)").
#
# Rules (see ABOUT.md, "Código compartilhado entre ferramentas (lib/)"):
#   - loading it only assigns NICK_CLAUDE_PERMISSIONS_FILE and defines functions: it prints
#     nothing and reads no file (the JSON is read only inside claude_settings_load);
#   - no shebang and no executable bit; no set -euo pipefail of its own, but it works with
#     or without it in the caller;
#   - every function is prefixed claude_ and every global NICK_CLAUDE_ — never CLAUDE_, a
#     prefix Claude Code itself reads from the environment;
#   - it never calls exit, and never uses source, . or eval;
#   - it reads no global of the caller: the destination variable name comes in as a parameter;
#   - it doesn't depend on lib/config/config.sh nor on lib/jail/jail.sh (each library loads on
#     its own), and needs no new runtime dependency (no jq: the JSON is checked by the tests).
# No exceptions to these rules.

# The rules file, found from this library's own path, so it doesn't depend on the cwd nor on
# a relative path in the source. A constant: a variable only so the tests can point it at a
# missing, unreadable or empty file (same reasoning as JAIL_BIN). The cd runs in the $(...)
# subshell, so the caller's cwd doesn't change, and its output (CDPATH) is discarded.
NICK_CLAUDE_PERMISSIONS_FILE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" > /dev/null && pwd)/permissions.json"

# claude_print_settings_error <path>: prints (stderr) why the agent won't be opened. The path
# goes to the terminal, so control characters become "?" ([:cntrl:], not [:print:], which under
# the C locale would also mangle the bytes of "não").
claude_print_settings_error() {
  local __claude_path="${1//[[:cntrl:]]/?}"
  echo "Erro: regras de permissão do Claude Code não encontradas ou ilegíveis: $__claude_path" >&2
  echo "O nick não abre o claude sem essas regras." >&2
}

# claude_settings_load <variable>: reads NICK_CLAUDE_PERMISSIONS_FILE and stores its content in
# <variable> with printf -v (same pattern as read_required; the locals are prefixed __claude_
# so they don't shadow the caller's variable). A missing, unreadable or empty file (whitespace
# only — spaces, tabs, line breaks — counts as empty): prints claude_print_settings_error and
# returns 1, without touching <variable>. It doesn't validate the JSON — the tests do. Its body
# doesn't rely on set -e, so it works the same when called before || or inside an if.
claude_settings_load() {
  local __claude_var="$1" __claude_file="$NICK_CLAUDE_PERMISSIONS_FILE" __claude_content=""
  if [ ! -f "$__claude_file" ] || [ ! -r "$__claude_file" ] ||
    ! __claude_content="$(cat -- "$__claude_file" 2> /dev/null)" || [ -z "${__claude_content//[[:space:]]/}" ]; then
    claude_print_settings_error "$__claude_file"
    return 1
  fi
  printf -v "$__claude_var" '%s' "$__claude_content"
}

# claude_conversation_file <config_dir> <dir> <session_id> <variable>: stores in <variable>, with
# printf -v, the file where claude keeps the conversation <session_id> of a session run with the
# cwd at <dir> (NS-39, the check before a resume): <config_dir>/projects/<dir with every character
# but A-Z, a-z and 0-9 replaced by "-">/<session_id>.jsonl. <config_dir> is the
# ${CLAUDE_CONFIG_DIR:-$HOME/.claude} of the caller (by parameter: this library reads no global of
# the caller, and never names a CLAUDE_* variable), and <dir> its physical path (pwd -P). The
# replacement is by character in the locale of the caller: under a UTF-8 locale an accented letter
# becomes a single "-", as claude does; under LC_ALL=C it would be one per byte, so the caller
# forces a UTF-8 locale. A character out of the BMP (an emoji) is one "-" here but two for claude,
# which replaces by UTF-16 unit: not covered. This is the layout of a claude internal detail
# (accepted in the NS-39): a new claude may change it, and for a very long path claude shortens
# the folder name, so the file is not found there (ABOUT.md, "Rust no projeto: retomar e arquivar
# agentes").
claude_conversation_file() {
  local __claude_dir="$2"
  printf -v "$4" '%s/projects/%s/%s.jsonl' "$1" "${__claude_dir//[!A-Za-z0-9]/-}" "$3"
}

# claude_conversation_latest <config_dir> <dir> <session_id> <variable>: stores in <variable>, with
# printf -v, the last session of the chain that starts at <session_id> (NS-50): claude may go on
# with a conversation in another session, and then it appends to the file of the old one a record
# {"type":"continued-in",...,"sessionId":"<old>","continuedInSessionId":"<new>"} (claude 2.1.328).
# The parameters are the ones of claude_conversation_file, which builds the file of each hop, and
# the file of <session_id> must exist (the caller checked). The rules: in the file of the current
# session only the last line with "type":"continued-in" counts, read with grep -F (only those
# lines, never the whole conversation), even when there is conversation after it; its sessionId
# must be the current session and its continuedInSessionId a lowercase UUID (8-4-4-4-12 hex),
# otherwise the record is ignored and the chain stops; the next hop needs its file in the same
# project folder; an unreadable file, a session already visited (a cycle) or 50 hops also stop the
# chain. It never fails: no record at all gives <session_id> back, and nothing goes to stderr.
# Like claude_conversation_file, this is a claude internal detail (accepted in the NS-50): a new
# claude may change the record, and then the chain just stops at the session given (ABOUT.md,
# "Rust no projeto: retomada segue a troca de sessão do claude (NS-50)").
claude_conversation_latest() {
  local __claude_current="$3" __claude_seen="" __claude_left=50 __claude_file="" __claude_line=""
  local __claude_owner="" __claude_next="" __claude_next_file=""
  __claude_seen=" $3 "
  while [ "$__claude_left" -gt 0 ]; do
    __claude_left=$((__claude_left - 1))
    claude_conversation_file "$1" "$2" "$__claude_current" __claude_file
    # grep exits with 1 with no record and with 2 on an unreadable file: both stop the chain.
    __claude_line="$(grep -F '"type":"continued-in"' -- "$__claude_file" 2> /dev/null | tail -n 1)" || __claude_line=""
    [ -n "$__claude_line" ] || break
    __claude_owner="$(printf '%s\n' "$__claude_line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')"
    __claude_next="$(printf '%s\n' "$__claude_line" | sed -n 's/.*"continuedInSessionId":"\([^"]*\)".*/\1/p')"
    [ "$__claude_owner" = "$__claude_current" ] || break
    printf '%s\n' "$__claude_next" | LC_ALL=C grep -Eqx '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}' || break
    case "$__claude_seen" in
      *" $__claude_next "*) break ;;
    esac
    claude_conversation_file "$1" "$2" "$__claude_next" __claude_next_file
    [ -f "$__claude_next_file" ] || break
    __claude_seen="$__claude_seen$__claude_next "
    __claude_current="$__claude_next"
  done
  printf -v "$4" '%s' "$__claude_current"
}

# claude_permission_notice: prints (stdout) the fixed instruction, in PT-BR, that every session
# opened by a nick tool gets by --append-system-prompt, before any other notice (NS-41): a refusal
# by one of the deny/ask rules above (or a confirmation the user denied) is the user's decision,
# so the agent stops and asks instead of getting the same effect another way (a script, bash -c,
# python -c...), which the rules can't catch since they match the command text. It applies only
# to what a rule refused: allowed commands stay allowed. It takes no parameter, reads no file and
# no global, and returns 0. Plain echos, like jail_agent_notice; this is the single source of the
# text (a test checks that no tool, prompt or template copies it).
claude_permission_notice() {
  echo "Regras de permissão: esta sessão foi aberta pelo nick com regras de permissão do Claude Code (deny e ask) escolhidas pelo usuário. Quando uma dessas regras recusar um comando ou o uso de uma ferramenta, ou quando o usuário negar um pedido de confirmação, a recusa é uma decisão do usuário, não um obstáculo técnico a contornar:"
  echo "- não obtenha o mesmo efeito por outro meio: reproduzir a ação recusada com um script (no scratchpad, no projeto ou em qualquer outro lugar), com bash -c, sh -c, python -c, outro interpretador, outra ferramenta ou outra forma de escrever o mesmo comando é contornar a regra, e isso não é permitido;"
  echo "- pare, diga ao usuário qual comando foi recusado e por quê (a regra de permissão que o recusou) e pergunte como ele quer seguir;"
  echo "- isso vale só para o que uma regra recusou: comandos permitidos continuam liberados (ex: um rm de arquivo, sem -rf), e você não precisa evitá-los nem pedir licença para usá-los."
}
