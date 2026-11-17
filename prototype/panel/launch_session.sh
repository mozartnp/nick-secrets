#!/usr/bin/env bash
#
# launch_session.sh: opens the claude of one session of the panel prototype (prototype/panel,
# NS-29, a throwaway prototype of the NS-28 epic) by the same launch path as the nick:
# jail_check + claude_settings_load + jail_agent_notice + jail_exec from lib/. The panel runs
# it in a PTY (bash <absolute path of this script>), one per session, with the cwd of the
# project being worked on.
# The jail is always on: the "true" given to the lib is a literal, and no flag nor variable
# turns it off. The project config (.nick/nick.conf) is not read — the sessions are fictitious
# data, and the config is out of scope; the values come from the panel's environment instead:
#   NICK_PANEL_CLIPBOARD     the JAIL_CLIPBOARD level ("restrito", "medio", "brando"; empty is
#                            "medio"), to try copy and paste;
#   NICK_PANEL_ALLOW_HOSTS   extra hosts, space separated (e.g. mcp.atlassian.com);
#   NICK_PANEL_FORWARD_PORTS localhost ports, space separated.
# NICK_ and never CLAUDE_: Claude Code reads CLAUDE_* from the environment (ABOUT.md).
# The session opens interactive, in the default permission mode and without a prompt.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=SCRIPTDIR/../../lib/jail/jail.sh
source "$SCRIPT_DIR/../../lib/jail/jail.sh"
# shellcheck source=SCRIPTDIR/../../lib/claude/claude.sh
source "$SCRIPT_DIR/../../lib/claude/claude.sh"

# The label jail_check shows in its errors where a tool shows its config path.
LAUNCH_LABEL="prototype/panel (protótipo NS-29)"

main() {
  local clipboard="${NICK_PANEL_CLIPBOARD:-}" hosts="${NICK_PANEL_ALLOW_HOSTS:-}"
  local ports="${NICK_PANEL_FORWARD_PORTS:-}" settings="" args
  jail_check "true" "$clipboard" "$hosts" "$ports" "$LAUNCH_LABEL" "false" || exit 1
  claude_settings_load settings || exit 1
  jail_print_status "true" "$clipboard"
  args=(--settings "$settings" --append-system-prompt "$(jail_agent_notice "$hosts" "$ports")")
  jail_exec "true" "$clipboard" "$hosts" "$ports" claude "${args[@]}"
}

main "$@"
