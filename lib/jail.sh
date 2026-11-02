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
# example), which become --allow-host, and ports of the machine's localhost to reach from
# inside (a database in Docker, for example: JAIL_FORWARD_PORTS), which become --forward-port.
# The version managers found in the home (pyenv, nvm, ...) and Playwright's browsers are
# mounted read-only (jail_toolchain_dirs, jail_playwright_browsers), and the agent is told it
# runs inside the jail (jail_agent_notice). This library holds the mechanism: checking that the
# jail works on this machine (jail_check), what to install and how (jail_print_requirements),
# the status shown before launching (jail_print_status), the ai-jail options (jail_options) and
# the launch (jail_exec). The nick never installs or downloads ai-jail, passes it only the
# options of jail_options, and never reads or writes the .ai-jail files: per-project tuning
# lives there, under ai-jail's own trust rules.
#
# Rules (see ABOUT.md, "Código compartilhado entre ferramentas (lib/)"):
#   - loading it only assigns JAIL_BIN, JAIL_OS_RELEASE_FILE and JAIL_LANDLOCK and defines
#     functions: it prints nothing and runs no command;
#   - no shebang and no executable bit; no set -euo pipefail of its own, but it works with
#     or without it in the caller;
#   - every function is prefixed jail_ and every global JAIL_;
#   - it never calls exit, and never uses source, . or eval;
#   - it reads no global of the caller: the JAIL_ENABLED, JAIL_CLIPBOARD and
#     JAIL_FORWARD_PORTS values, the extra hosts, the config path and whether that file is
#     shared with the team all come in as parameters. $PWD, $HOME and the $TMPDIR that mktemp
#     reads are shell state, not caller globals;
#   - it doesn't depend on lib/config.sh (each library loads on its own).
# The one documented exception: jail_exec replaces the shell with exec, so it never returns.
# That is not an exit, it is its job — the last step of the tool.

# The ai-jail command, looked up in the PATH. A variable only so the tests can point it at a
# name that doesn't exist (the "ai-jail missing" case), whatever the machine running them has.
JAIL_BIN="ai-jail"
# Where jail_print_requirements reads the Linux distribution from (overridden by the tests).
JAIL_OS_RELEASE_FILE="/etc/os-release"
# The Landlock state of the last check that passed ("completo", "parcial", "inativo" or "não
# identificado"; jail_landlock_state), for jail_print_status to show. Empty until jail_check
# passes with the jail on — the library's own state, not a caller global.
JAIL_LANDLOCK=""

# jail_print_status <enabled> <clipboard>: prints (stdout) the one line every tool shows before
# launching the agent, so the user sees whether the jail is on and, when it is, the
# JAIL_CLIPBOARD level ("modo de clipboard"), preceded by the Landlock state when JAIL_LANDLOCK
# has one ("Landlock: completo"): ai-jail prints it too, but claude clears the screen at once.
# With the jail on, the line always starts with ⚠️; "brando", the only level where the agent
# can read the clipboard, shows the level in red — only when stdout is a terminal and NO_COLOR
# is unset or empty (the no-color.org convention), so pipes and logs stay plain.
# NO_COLOR is the user's environment, like the $TMPDIR mktemp reads, not a caller global.
# The risk of each level is in the README and in the skeleton comment. jail_check has already
# rejected any other value.
jail_print_status() {
  local level prefix="⚠️ Jail (ai-jail): ativa | "
  if [ "$1" != "true" ]; then
    echo "Jail (ai-jail): desligada — o agente roda direto no seu shell."
    return 0
  fi
  if [ -n "$JAIL_LANDLOCK" ]; then
    prefix="${prefix}Landlock: $JAIL_LANDLOCK | "
  fi
  case "$2" in
    restrito)
      echo "${prefix}modo de clipboard restrito"
      ;;
    brando)
      level="modo de clipboard brando"
      if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
        level=$'\e[31m'"$level"$'\e[0m'
      fi
      echo "$prefix$level"
      ;;
    *)
      echo "${prefix}modo de clipboard médio"
      ;;
  esac
}

# jail_landlock_state <output>: prints, in short, the Landlock state that ai-jail reports in
# <output> (its stderr, a "▸ Landlock: <state>" line): "fully enforced" -> "completo",
# "partially enforced" -> "parcial" (an older kernel lacks part of the rules), "not enforced"
# or "failed to apply" -> "inativo", and no such line at all -> "inativo" (--no-landlock or no
# support). Detail lines ("Landlock: ro map ...") are skipped; a state line with any other
# text -> "não identificado", so a new wording of ai-jail never shows up as a false "completo".
jail_landlock_state() {
  local line rest seen=0
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      *"Landlock: "*) rest="${line#*Landlock: }" ;;
      *) continue ;;
    esac
    # The "~/" below is ai-jail's text, not a path to expand.
    # shellcheck disable=SC2088
    case "$rest" in
      "fully enforced"*) echo "completo"; return 0 ;;
      "partially enforced"*) echo "parcial"; return 0 ;;
      "not enforced"* | "failed to apply"*) echo "inativo"; return 0 ;;
      "ro map"* | "rw map"* | "overlay map"* | "~/"* | "audio socket"* | "browser profile"* | \
        "bwrap"* | "claude-dir"* | "gpu"* | "SSH_AUTH_SOCK"* | "systemd-user"* | "Wayland socket"*) ;;
      *) seen=1 ;;
    esac
  done <<< "$1"
  if [ "$seen" -eq 1 ]; then
    echo "não identificado"
  else
    echo "inativo"
  fi
}

# jail_toolchain_dirs: prints (stdout, one per line, absolute) the version-manager directories
# of the home that exist on this machine, in the order of the list below. The jail's home is
# private, so without them a project's virtualenv or node_modules/.bin, whose interpreter
# lives there (venv/bin/python -> ~/.pyenv/versions/...), breaks inside the jail with a
# misleading "no such file". The list is curated and kept here, so no one has to edit their
# ~/.ai-jail: toolchains only, never config or credential dirs (~/.config, ~/.local, ~/.ssh).
# For uv, only its interpreters (~/.local/share/uv/python): the rest of ~/.local/share/uv
# holds the credentials of 'uv auth login' (credentials/, plain text without a keyring).
# No HOME, nothing.
jail_toolchain_dirs() {
  local home="${HOME:-}" name
  [ -n "$home" ] || return 0
  for name in .pyenv .nvm .rbenv .asdf .local/share/mise .local/share/uv/python .sdkman .volta; do
    if [ -d "$home/$name" ]; then
      printf '%s\n' "$home/$name"
    fi
  done
}

# jail_playwright_browsers: prints (stdout) the directory of Playwright's browsers, for the
# project's E2E tests to run inside the jail: $PLAYWRIGHT_BROWSERS_PATH when set (the user's
# environment, like NO_COLOR), ~/.cache/ms-playwright otherwise (Linux's default). Nothing when
# it isn't a directory, when it isn't absolute ("0", Playwright's "browsers inside the
# project", or a relative path: ambiguous for ai-jail and already inside the visible project)
# or when the path has a ":", which would break ai-jail's --map SOURCE:DEST. It gets its own
# function, not a line in jail_toolchain_dirs, because it can't be mounted at the same path:
# see jail_options.
jail_playwright_browsers() {
  local dir="${PLAYWRIGHT_BROWSERS_PATH:-}"
  if [ -z "$dir" ]; then
    [ -n "${HOME:-}" ] || return 0
    dir="$HOME/.cache/ms-playwright"
  fi
  case "$dir" in
    *:* | [!/]*) return 0 ;;
  esac
  if [ -d "$dir" ]; then
    printf '%s\n' "$dir"
  fi
}

# jail_options <clipboard> [hosts] [ports]: prints (stdout, one per line) the ai-jail options
# for a JAIL_CLIPBOARD level, the extra hosts and the localhost ports — the only options the
# nick ever passes, all on the command line so none depends on anyone's ~/.ai-jail. Every
# level gets:
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
# Then one --map (read-only) per directory of jail_toolchain_dirs and, when
# jail_playwright_browsers finds them, Playwright's browsers mounted read-only at
# ~/.ms-playwright, with PLAYWRIGHT_BROWSERS_PATH pointing there (--env). Not at
# ~/.cache/ms-playwright: ai-jail mounts its own writable ~/.cache and drops it when a
# read-only map lands inside it (seen with --dry-run), which would lose the pip/uv/npm caches.
# Chromium's own sandbox doesn't work inside the jail; Playwright launches it with
# --no-sandbox by default (chromiumSandbox: false).
# Last, one --allow-host per host in [hosts] (space separated, in order): ai-jail adds each
# one, and its subdomains, to its default network allowlist, which stays as it is. Then one
# --forward-port per port in [ports] (space separated, in order; jail_check has validated
# them): ai-jail exposes the machine's 127.0.0.1:<port> on the jail's own loopback, and only
# those ports.
jail_options() {
  local host_list host dir port_list port browsers
  printf '%s\n' --no-save-config --no-status-bar
  case "$1" in
    restrito) ;;
    brando) printf '%s\n' --terminal-passthrough --display ;;
    *) printf '%s\n' --terminal-passthrough ;;
  esac
  while IFS= read -r dir; do
    printf '%s\n' --map "$dir"
  done < <(jail_toolchain_dirs)
  browsers="$(jail_playwright_browsers)"
  if [ -n "$browsers" ] && [ -n "${HOME:-}" ]; then
    printf '%s\n' --map "$browsers:$HOME/.ms-playwright" --env "PLAYWRIGHT_BROWSERS_PATH=$HOME/.ms-playwright"
  fi
  read -ra host_list <<< "${2:-}"
  for host in "${host_list[@]}"; do
    printf '%s\n' --allow-host "$host"
  done
  read -ra port_list <<< "${3:-}"
  for port in "${port_list[@]}"; do
    printf '%s\n' --forward-port "$port"
  done
}

# jail_agent_notice <hosts> [ports]: prints (stdout) the text that tells the agent it runs
# inside the jail — for the tool to hand it over (auto_scrum: claude's --append-system-prompt),
# only with the jail on. Without it, an agent that hits something the jail hides (a toolchain,
# a home dir, a localhost service, a host) spends tokens diagnosing the machine and reaches
# wrong conclusions ("~/.pyenv was removed"). It lists what is visible (the mounted
# toolchains, with ~ for the home), the extra hosts and the localhost ports, and says what to
# do instead: stop and tell the user. In PT-BR, like the prompts.
jail_agent_notice() {
  local home="${HOME:-}" dir toolchains="" host_list hosts="" port_list ports="" localhost
  local playwright=""
  while IFS= read -r dir; do
    if [ -n "$home" ] && [ "${dir#"$home"/}" != "$dir" ]; then
      # A literal ~ on purpose: it is text for the agent, not a path to expand.
      # shellcheck disable=SC2088
      dir='~/'"${dir#"$home"/}"
    fi
    toolchains="${toolchains:+$toolchains, }$dir"
  done < <(jail_toolchain_dirs)
  read -ra host_list <<< "${1:-}"
  for dir in "${host_list[@]}"; do
    hosts="${hosts:+$hosts, }$dir"
  done
  read -ra port_list <<< "${2:-}"
  for dir in "${port_list[@]}"; do
    ports="${ports:+$ports, }$dir"
  done
  if [ -n "$home" ] && [ -n "$(jail_playwright_browsers)" ]; then
    # shellcheck disable=SC2088
    playwright='- os navegadores do Playwright (PLAYWRIGHT_BROWSERS_PATH=~/.ms-playwright) estão montados, somente leitura; o sandbox do próprio Chromium não funciona na jail, então ele precisa rodar com --no-sandbox (o padrão do Playwright);'
  fi
  if [ -n "$ports" ]; then
    localhost="dos serviços da máquina em localhost, só as portas liberadas pelo nick ($ports) são alcançáveis; os demais (banco, servidor de desenvolvimento) não"
  else
    localhost="serviços da máquina em localhost (banco, servidor de desenvolvimento) não são alcançáveis"
  fi
  echo "Ambiente: você está rodando dentro do sandbox ai-jail, ligado pelo nick (JAIL_ENABLED=\"true\" na configuração do projeto). O que muda:"
  echo "- só o diretório do projeto aceita escrita; o resto do sistema é somente leitura;"
  if [ -n "$toolchains" ]; then
    echo "- o home é privado: fora do projeto, por padrão só aparece o que a jail monta (o estado do claude, a configuração do git e os gerenciadores de versão desta máquina ($toolchains), somente leitura); outros diretórios do home (~/.ssh, ~/.config, ~/.local etc.) não existem aqui, mesmo existindo na máquina;"
  else
    echo "- o home é privado: fora do projeto, por padrão só aparece o que a jail monta (o estado do claude e a configuração do git); outros diretórios do home (~/.ssh, ~/.config, ~/.local etc.) não existem aqui, mesmo existindo na máquina;"
  fi
  if [ -n "$hosts" ]; then
    echo "- por padrão, a rede só alcança a API do Claude, os registries de pacotes e o GitHub, e também: $hosts; $localhost;"
  else
    echo "- por padrão, a rede só alcança a API do Claude, os registries de pacotes e o GitHub; $localhost;"
  fi
  if [ -n "$playwright" ]; then
    echo "$playwright"
  fi
  echo "- você não consegue ver nem conferir nada fora da jail."
  echo "Se algo falhar por arquivo, diretório, interpretador, ferramenta, serviço ou host ausente ou bloqueado, a primeira hipótese é a jail, não a máquina: não conclua que algo foi removido ou está quebrado e não gaste tempo investigando o sistema. Pare e avise o usuário com o caminho, serviço ou host exato, dizendo que pode ser uma limitação da jail do nick."
}

# jail_exec <enabled> <clipboard> <hosts> <ports> <command> [args...]: replaces the shell with
# <command> [args...] — inside the jail as '$JAIL_BIN <jail_options of clipboard, hosts and
# ports> -- <command> [args...]' when <enabled> is "true" exactly, directly otherwise (the same
# argv as before the jail existed, whatever <clipboard>, <hosts> and <ports> are; the options
# are only built with the jail on). The
# "--" is ai-jail's documented end of options, so no argument is ever read as an ai-jail
# option; the command stays the first word after it, which ai-jail uses to pick the agent's
# state and settings. Never returns.
jail_exec() {
  local enabled="$1" clipboard="$2" hosts="$3" ports="$4" options
  shift 4
  if [ "$enabled" = "true" ]; then
    mapfile -t options < <(jail_options "$clipboard" "$hosts" "$ports")
    exec "$JAIL_BIN" "${options[@]}" -- "$@"
  fi
  exec "$@"
}

# jail_check <enabled> <clipboard> <hosts> <ports> <conf_path> <shared>: with <enabled> other
# than "true" exactly, returns 0 and does nothing (not even command -v, nor checking
# <clipboard> or <ports>). With "true", first rejects a <clipboard> other than "restrito",
# "medio", "brando" or "" (a typo must never pick a level silently) and <ports> that aren't
# space-separated numbers from 1 to 65535 without leading zeros nor repeats (a typo must never
# open another port nor reach ai-jail as an option), then checks that the jail works on this
# machine by running '$JAIL_BIN <jail_options of clipboard, hosts and ports> -- true' — the
# launch's own options, so an ai-jail that doesn't accept them fails here, before the launch —
# with the cwd in a fresh temporary directory (a second guard: no .ai-jail ever lands in the
# target project), and removes the directory afterwards, on success and on failure. Returns 0
# quietly when the check passes; otherwise prints the error on stderr and returns 1:
# jail_print_clipboard_error or jail_print_ports_error for the value, jail_print_check_error
# for a missing ai-jail, a failing mktemp or a non-zero check (missing bwrap, blocked user
# namespaces and unsupported systems all end up there, with ai-jail's output in the message).
# There is no fallback: "true" never runs without the jail. Callers run it before || (it stops
# the tool, without exit) and never inside $(...), so the message reaches the terminal. Being
# before ||, set -e is off in here: every command that can fail is checked explicitly.
jail_check() {
  local enabled="$1" clipboard="$2" hosts="$3" ports="$4" conf_path="$5" shared="$6" options
  local tmp_dir output failed=0 port_list port seen=" " bad
  JAIL_LANDLOCK=""
  [ "$enabled" = "true" ] || return 0
  case "$clipboard" in
    "" | restrito | medio | brando) ;;
    *)
      jail_print_clipboard_error "$clipboard" "$conf_path"
      return 1
      ;;
  esac
  read -ra port_list <<< "$ports"
  for port in "${port_list[@]}"; do
    bad=0
    case "$port" in
      0* | *[!0-9]* | ??????*) bad=1 ;;
    esac
    case "$seen" in
      *" $port "*) bad=1 ;;
    esac
    if [ "$bad" -eq 1 ] || [ "$port" -gt 65535 ]; then
      jail_print_ports_error "$ports" "$conf_path"
      return 1
    fi
    seen="$seen$port "
  done
  mapfile -t options < <(jail_options "$clipboard" "$hosts" "$ports")
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
  JAIL_LANDLOCK="$(jail_landlock_state "$output")"
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

# jail_print_ports_error <ports> <conf_path>: the error of jail_check for a JAIL_FORWARD_PORTS
# value it doesn't accept, on stderr: the value, the file and the accepted format. No
# requirements list: it is a config typo. Control characters become "?".
jail_print_ports_error() {
  local ports="${1//[[:cntrl:]]/?}" conf_path="${2//[[:cntrl:]]/?}"
  {
    echo "Erro: JAIL_FORWARD_PORTS=\"$ports\" em $conf_path não é uma lista de portas válida."
    echo "Use números de 1 a 65535, sem repetir, separados por espaço, ex: JAIL_FORWARD_PORTS=\"54320 6379\"."
  } >&2
}

# jail_print_check_error <conf_path> <shared> <reason> [output]: the single error text of
# jail_check, on stderr: the config file that turned the jail on, the reason (or, when <reason>
# is empty, ai-jail's captured output), the two ways out, a warning when <shared> is "true"
# (the file is shared with the target team, so turning the jail off there affects everyone once
# committed) and, last, the requirements list. With ai-jail's output, the first way out is
# fixing the problem shown: ai-jail and bwrap may both be installed (blocked user namespaces, a
# bwrap failing ai-jail's trusted-binary validation), so the list may be all [ok]. The output
# and the path go to the terminal, so control characters become "?" line by line, keeping the
# line breaks ([:cntrl:], not [:print:], which under the C locale would also mangle the bytes
# of "não").
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
