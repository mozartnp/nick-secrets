# shellcheck shell=bash
#
# jail.sh: runs the agent of a nick tool inside the ai-jail sandbox (akitaonrails/ai-jail:
# bubblewrap + Landlock on Linux, sandbox-exec on macOS) — today only auto_scrum uses it. It is
# opt-in per project through the JAIL_ENABLED key, a convention of the whole nick: each tool
# registers that key in its own config (config_register_key, lib/config.sh) and passes the
# value here; only "true" exactly turns the jail on. JAIL_CLIPBOARD, the same kind of
# convention, picks how much of the terminal and the clipboard the jail releases: "restrito",
# "medio" (also the empty value, the default) or "brando" (jail_options). Each tool may also
# pass extra hosts to release in the jail's network (the remote MCP servers it relies on, for
# example), which become --allow-host. This library holds
# the mechanism: checking that the jail works on this machine (jail_check), what to install
# and how (jail_print_requirements), the status shown before launching (jail_print_status),
# the ai-jail options (jail_options) and the launch (jail_exec). The nick never installs or
# downloads ai-jail, passes it only the options of jail_options, and never reads or writes the
# .ai-jail files: per-project tuning lives there, under ai-jail's own trust rules.
#
# Rules (see ABOUT.md, "Código compartilhado entre ferramentas (lib/)"):
#   - loading it only assigns JAIL_BIN and JAIL_OS_RELEASE_FILE and defines functions: it
#     prints nothing and runs no command;
#   - no shebang and no executable bit; no set -euo pipefail of its own, but it works with
#     or without it in the caller;
#   - every function is prefixed jail_ and every global JAIL_;
#   - it never calls exit, and never uses source, . or eval;
#   - it reads no global of the caller: the JAIL_ENABLED and JAIL_CLIPBOARD values, the extra
#     hosts, the config path and whether that file is shared with the team all come in as
#     parameters. $PWD and the $TMPDIR that mktemp reads are shell state, not caller globals;
#   - it doesn't depend on lib/config.sh (each library loads on its own).
# The one documented exception: jail_exec replaces the shell with exec, so it never returns.
# That is not an exit, it is its job — the last step of the tool.

# The ai-jail command, looked up in the PATH. A variable only so the tests can point it at a
# name that doesn't exist (the "ai-jail missing" case), whatever the machine running them has.
JAIL_BIN="ai-jail"
# Where jail_print_requirements reads the Linux distribution from (overridden by the tests).
JAIL_OS_RELEASE_FILE="/etc/os-release"

# jail_print_status <enabled> <clipboard>: prints (stdout) the one line every tool shows before
# launching the agent, so the user sees whether the jail is on and, when it is, the
# JAIL_CLIPBOARD level ("modo de clipboard"). With the jail on, the line always starts with
# ⚠️; "brando", the only level where the agent can read the clipboard, shows the level in
# red — only when stdout is a terminal and NO_COLOR is unset or empty (the no-color.org
# convention), so pipes and logs stay plain.
# NO_COLOR is the user's environment, like the $TMPDIR mktemp reads, not a caller global.
# The risk of each level is in the README and in the skeleton comment. jail_check has already
# rejected any other value.
jail_print_status() {
  local level
  if [ "$1" != "true" ]; then
    echo "Jail (ai-jail): desligada — o agente roda direto no seu shell."
    return 0
  fi
  case "$2" in
    restrito)
      echo "⚠️ Jail (ai-jail): ativa | modo de clipboard restrito"
      ;;
    brando)
      level="modo de clipboard brando"
      if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
        level=$'\e[31m'"$level"$'\e[0m'
      fi
      echo "⚠️ Jail (ai-jail): ativa | $level"
      ;;
    *)
      echo "⚠️ Jail (ai-jail): ativa | modo de clipboard médio"
      ;;
  esac
}

# jail_options <clipboard> [hosts]: prints (stdout, one per line) the ai-jail options for a
# JAIL_CLIPBOARD level and the extra hosts — the only options the nick ever passes, all on the
# command line so none depends on anyone's ~/.ai-jail. Every level gets:
#   - --no-save-config: without it, a project with no .ai-jail yet gets one auto-saved in the
#     cwd holding the whole argv — the agent's prompt included — and a bare 'ai-jail' there
#     would replay it; it only stops that write, the project's .ai-jail is still read;
#   - --no-status-bar: ai-jail's status bar shows the command line (the prompt again) and
#     draws over the bottom rows of claude's interface, garbling the terminal.
# Then, by level:
#   - "restrito": nothing else — ai-jail filters what the agent writes to the terminal, and
#     the filter blocks claude's copy (OSC 52);
#   - "medio" (and empty, the default): --terminal-passthrough — copying works, but the agent
#     can write to the clipboard and send control sequences to the terminal;
#   - "brando": --terminal-passthrough --display — also claude's Ctrl+V, which reads the
#     clipboard through the Wayland/X11 socket; so the agent can read the clipboard and open
#     windows on the user's screen.
# Any other value gets the "medio" options: jail_check rejects it before any launch.
# Last, one --allow-host per host in [hosts] (space separated, in order): ai-jail adds each
# one, and its subdomains, to its default network allowlist, which stays as it is.
jail_options() {
  local host_list host
  printf '%s\n' --no-save-config --no-status-bar
  case "$1" in
    restrito) ;;
    brando) printf '%s\n' --terminal-passthrough --display ;;
    *) printf '%s\n' --terminal-passthrough ;;
  esac
  read -ra host_list <<< "${2:-}"
  for host in "${host_list[@]}"; do
    printf '%s\n' --allow-host "$host"
  done
}

# jail_exec <enabled> <clipboard> <hosts> <command> [args...]: replaces the shell with
# <command> [args...] — inside the jail as '$JAIL_BIN <jail_options of clipboard and hosts> --
# <command> [args...]' when <enabled> is "true" exactly, directly otherwise (the same argv as
# before the jail existed, whatever <clipboard> and <hosts> are; the options are only built
# with the jail on). The
# "--" is ai-jail's documented end of options, so no argument is ever read as an ai-jail
# option; the command stays the first word after it, which ai-jail uses to pick the agent's
# state and settings. Never returns.
jail_exec() {
  local enabled="$1" clipboard="$2" hosts="$3" options
  shift 3
  if [ "$enabled" = "true" ]; then
    mapfile -t options < <(jail_options "$clipboard" "$hosts")
    exec "$JAIL_BIN" "${options[@]}" -- "$@"
  fi
  exec "$@"
}

# jail_check <enabled> <clipboard> <hosts> <conf_path> <shared>: with <enabled> other than
# "true" exactly, returns 0 and does nothing (not even command -v, nor checking <clipboard>).
# With "true", first rejects a <clipboard> other than "restrito", "medio", "brando" or "" (a
# typo must never pick a level silently), then checks that the jail works on this machine by
# running '$JAIL_BIN <jail_options of clipboard and hosts> -- true' — the launch's own
# options, so an ai-jail that doesn't accept them fails here, before the launch — with the cwd
# in a fresh temporary directory (a second guard: no .ai-jail ever lands in the target
# project), and removes the directory afterwards, on success and on failure. Returns 0 quietly
# when the check passes; otherwise prints the error on stderr and returns 1:
# jail_print_clipboard_error for the value, jail_print_check_error for a missing ai-jail, a
# failing mktemp or a non-zero check (missing bwrap, blocked user namespaces and unsupported
# systems all end up there, with ai-jail's output in the message). There is no fallback:
# "true" never runs without the jail. Callers run it before || (it stops the tool, without
# exit) and never inside $(...), so the message reaches the terminal. Being before ||, set -e
# is off in here: every command that can fail is checked explicitly.
jail_check() {
  local enabled="$1" clipboard="$2" hosts="$3" conf_path="$4" shared="$5" options tmp_dir
  local output failed=0
  [ "$enabled" = "true" ] || return 0
  case "$clipboard" in
    "" | restrito | medio | brando) ;;
    *)
      jail_print_clipboard_error "$clipboard" "$conf_path"
      return 1
      ;;
  esac
  mapfile -t options < <(jail_options "$clipboard" "$hosts")
  if ! command -v "$JAIL_BIN" >/dev/null 2>&1; then
    jail_print_check_error "$conf_path" "$shared" "ai-jail não encontrado no PATH."
    return 1
  fi
  if ! tmp_dir="$(mktemp -d 2>/dev/null)"; then
    jail_print_check_error "$conf_path" "$shared" "não foi possível criar o diretório temporário da verificação (mktemp -d falhou)."
    return 1
  fi
  if ! output="$(cd "$tmp_dir" && "$JAIL_BIN" "${options[@]}" -- true 2>&1)"; then
    failed=1
  fi
  rm -rf "$tmp_dir" || true
  if [ "$failed" -eq 1 ]; then
    jail_print_check_error "$conf_path" "$shared" "" "$output"
    return 1
  fi
  return 0
}

# jail_print_clipboard_error <clipboard> <conf_path>: the error of jail_check for a
# JAIL_CLIPBOARD value it doesn't accept, on stderr: the value, the file and the accepted
# values. No requirements list: it is a config typo, not something to install. Both go to the
# terminal, so control characters become "?".
jail_print_clipboard_error() {
  local clipboard="${1//[[:cntrl:]]/?}" conf_path="${2//[[:cntrl:]]/?}"
  {
    echo "Erro: JAIL_CLIPBOARD=\"$clipboard\" em $conf_path não é um valor aceito."
    echo "Valores aceitos: \"restrito\", \"medio\", \"brando\" ou \"\" (vazio, que vale \"medio\")."
    echo "Corrija o valor: a jail não escolhe um nível sozinha."
  } >&2
}

# jail_print_check_error <conf_path> <shared> <reason> [output]: the single error text of
# jail_check, on stderr: the config file that turned the jail on, the reason (or, when
# <reason> is empty, ai-jail's captured output), the two ways out, a warning when <shared> is
# "true" (the file is shared with the target team, so turning the jail off there affects
# everyone once committed) and, last, the requirements list. With ai-jail's output, the first
# way out is fixing the problem shown: ai-jail and bwrap may both be installed (blocked user
# namespaces, a bwrap failing ai-jail's trusted-binary validation), so the list may be all
# [ok]. The output and the path go to
# the terminal, so control characters become "?" line by line, keeping the line breaks
# ([:cntrl:], not [:print:], which under the C locale would also mangle the bytes of "não").
jail_print_check_error() {
  local conf_path="${1//[[:cntrl:]]/?}" shared="$2" reason="$3" output="${4:-}" line
  {
    echo "Erro: JAIL_ENABLED=\"true\" em $conf_path, mas a jail (ai-jail) não funciona nesta máquina."
    if [ -n "$reason" ]; then
      echo "  $reason"
    elif [ -n "$output" ]; then
      echo "  Saída do ai-jail:"
      while IFS= read -r line || [ -n "$line" ]; do
        echo "    ${line//[[:cntrl:]]/?}"
      done <<< "$output"
    else
      echo "  A verificação com o ai-jail falhou, sem nenhuma saída."
    fi
    echo "Para continuar, escolha uma das saídas:"
    if [ -n "$reason" ]; then
      echo "  1. instale o que falta (lista abaixo);"
    else
      echo "  1. corrija o problema acima (veja \"Problemas comuns\" na seção \"Sandbox (ai-jail)\""
      echo "     do README) ou instale o que falta (lista abaixo);"
    fi
    echo "  2. ou defina JAIL_ENABLED=\"false\" em $conf_path pra rodar sem jail."
    if [ "$shared" = "true" ]; then
      echo "Atenção: $conf_path é compartilhado com o time do projeto. Desligar a jail nele vale para"
      echo "todos se a mudança for commitada; prefira instalar o que falta."
    fi
    echo
    jail_print_requirements
  } >&2
}

# jail_os_release_ids <file>: prints the ID and ID_LIKE values of an os-release file, space
# separated and without quotes. The file is read line by line, never sourced: the project
# never runs a config file, even a system one. A missing or unreadable file prints nothing.
jail_os_release_ids() {
  local file="$1" line value ids=""
  if [ -f "$file" ] && [ -r "$file" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
      case "$line" in
        ID=* | ID_LIKE=*)
          value="${line#*=}"
          value="${value#[\"\']}"
          value="${value%[\"\']}"
          ids="$ids $value"
          ;;
      esac
    done < "$file"
  fi
  printf '%s\n' "${ids# }"
}

# jail_print_requirements: prints (stdout) what the jail needs on this machine — ai-jail and,
# on Linux, bwrap (bubblewrap) — each as [ok] or [falta] (command -v only), and the install
# command of the detected platform for each missing one. The platform comes from uname -s
# and, on Linux, from the ID/ID_LIKE of JAIL_OS_RELEASE_FILE. This is the single source of
# "what to install and how": jail_check's error ends with it, each tool exposes it through a
# flag, and the README repeats the commands (a test checks they match). It only shows the
# commands — it never runs an installer, never downloads anything.
jail_print_requirements() {
  local system ids label jail_cmd bwrap_cmd="" linux=0
  system="$(uname -s 2>/dev/null || true)"
  case "$system" in
    Linux)
      linux=1
      ids=" $(jail_os_release_ids "$JAIL_OS_RELEASE_FILE") "
      jail_cmd="cargo install --locked ai-jail  (ou o binário dos Releases: https://github.com/akitaonrails/ai-jail/releases)"
      case "$ids" in
        *" arch "*)
          label="Linux (Arch)"
          jail_cmd="yay -S ai-jail-bin  (AUR; com outro helper do AUR, o pacote é o mesmo)"
          bwrap_cmd="sudo pacman -S bubblewrap"
          ;;
        *" debian "* | *" ubuntu "*)
          label="Linux (Debian/Ubuntu)"
          bwrap_cmd="sudo apt install bubblewrap"
          ;;
        *" fedora "* | *" rhel "*)
          label="Linux (Fedora/RHEL)"
          bwrap_cmd="sudo dnf install bubblewrap"
          ;;
        *)
          label="Linux (outra distribuição)"
          bwrap_cmd="instale o pacote bubblewrap da sua distribuição"
          ;;
      esac
      ;;
    Darwin)
      label="macOS"
      jail_cmd="brew tap akitaonrails/tap && brew install ai-jail"
      ;;
    *)
      label="outro sistema (${system:-desconhecido})"
      jail_cmd="não suportado: use o WSL 2 e rode o nick dentro dele"
      ;;
  esac
  echo "Requisitos da jail (JAIL_ENABLED=\"true\") — sistema detectado: $label"
  if command -v "$JAIL_BIN" >/dev/null 2>&1; then
    echo "  [ok]    ai-jail"
  else
    echo "  [falta] ai-jail  →  $jail_cmd"
  fi
  if [ "$linux" -eq 1 ]; then
    if command -v bwrap >/dev/null 2>&1; then
      echo "  [ok]    bwrap (bubblewrap)"
    else
      echo "  [falta] bwrap (bubblewrap)  →  $bwrap_cmd"
    fi
  fi
  echo "O nick não instala nada: rode os comandos acima você mesmo."
  if [ "$linux" -eq 1 ]; then
    echo "Ubuntu 24.04+ e Debian 13+ podem bloquear user namespaces sem privilégio (AppArmor): veja os problemas comuns na seção \"Sandbox (ai-jail)\" do README."
    echo "Depois de instalar, confira com: ai-jail --dry-run claude (na raiz do projeto alvo; não grava nada)."
  fi
}
