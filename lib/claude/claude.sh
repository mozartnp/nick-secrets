# shellcheck shell=bash
#
# claude.sh: the Claude Code permission rules (deny/ask) that every session opened by a nick
# tool gets — today only auto_scrum uses it. The rules live in permissions.json, next to this
# file, the single source of them ({"permissions": {"deny": [...], "ask": [...]}}, nothing
# else); this library only finds that file and reads it (claude_settings_load), and prints the
# error when it can't (claude_print_settings_error). The tool passes the content inline to the
# agent, as 'claude --settings "<content>" ...': never as a path (inside the jail a path out of
# the project may not exist) and never written to a .claude/settings*.json of the user or of
# the project. Always on: there is no flag to turn the rules off and no fallback without them.
# The rules match the command text, so they are a guard rail, not a sandbox (see ABOUT.md,
# "Regras de deny/ask do Claude Code (lib/claude/)").
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
#   - it doesn't depend on lib/config.sh nor on lib/jail.sh (each library loads on its own),
#     and needs no new runtime dependency (no jq: the JSON is checked by the tests).
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
