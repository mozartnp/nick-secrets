# shellcheck shell=bash
#
# jail.sh: runs the agent of a nick tool inside the ai-jail sandbox (akitaonrails/ai-jail:
# bubblewrap + Landlock on Linux, sandbox-exec on macOS) — today only auto_scrum uses it. It is
# opt-in per project through the JAIL_ENABLED key, a convention of the whole nick: each tool
# registers that key in its own config (config_register_key, lib/config.sh) and passes the
# value here; only "true" exactly turns the jail on. JAIL_CLIPBOARD, the same kind of
# convention, picks how much of the terminal and the clipboard the jail releases: "restrito",
# "medio" (also the empty value, the default) or "brando" (jail_options). Each tool may also
# pass extra hosts to release in the jail's network (the remote MCP servers it relies on, the
# docs of the project's stack: JAIL_ALLOW_HOSTS), which become --allow-host next to the Claude
# documentation and the other hosts of the nick's own list, lib/jail/allow_hosts.txt, always
# released (jail_nick_hosts), and ports of the machine's localhost to
# reach from inside (a database in Docker, for example: JAIL_FORWARD_PORTS), which become
# --forward-port.
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
#   - loading it only assigns JAIL_BIN, JAIL_VERSION_MIN, JAIL_OS_RELEASE_FILE, JAIL_HOSTS_FILE
#     and JAIL_LANDLOCK and defines functions: it prints nothing and runs no command but the builtins (cd, pwd)
#     that make JAIL_HOSTS_FILE absolute, as lib/claude/claude.sh does for its rules file;
#   - no shebang and no executable bit; no set -euo pipefail of its own, but it works with
#     or without it in the caller;
#   - every function is prefixed jail_ and every global JAIL_;
#   - it never calls exit, and never uses source, . or eval;
#   - it reads no global of the caller: the JAIL_ENABLED, JAIL_CLIPBOARD and
#     JAIL_FORWARD_PORTS values, the extra hosts (MCPs and JAIL_ALLOW_HOSTS), the config path
#     and whether that file is shared with the team all come in as parameters. $PWD, $HOME and
#     the $TMPDIR that mktemp reads are shell state, not caller globals;
#   - it doesn't depend on lib/config.sh (each library loads on its own).
# The one documented exception: jail_exec replaces the shell with exec, so it never returns.
# That is not an exit, it is its job — the last step of the tool.

# The ai-jail command, looked up in the PATH. A variable only so the tests can point it at a
# name that doesn't exist (the "ai-jail missing" case), whatever the machine running them has.
JAIL_BIN="ai-jail"
# The oldest ai-jail the nick supports; the newest is any later one of the same major series
# (3.x: >= 3.2.0 and < 4.0.0). The options of jail_options were checked against 3.2.0 only, and
# the approval screen that made the nick adopt them came with a major release, so a new major
# needs a new check (ABOUT.md). The single source of the number: jail_check, the messages and
# the README test all read it from here.
JAIL_VERSION_MIN="3.2.0"
# Where jail_print_requirements reads the Linux distribution from (overridden by the tests).
JAIL_OS_RELEASE_FILE="/etc/os-release"
# The nick's own list of hosts always released in the jail's network (jail_nick_hosts): a text
# file next to this library, one host per line, so a dev adds one without touching the code.
# Absolute, so it holds after a cd whatever path the library was loaded by; only builtins, so
# it works with any PATH.
JAIL_HOSTS_FILE="${BASH_SOURCE[0]%/*}"
[ "$JAIL_HOSTS_FILE" != "${BASH_SOURCE[0]}" ] || JAIL_HOSTS_FILE="."
JAIL_HOSTS_FILE="$(cd -- "$JAIL_HOSTS_FILE" > /dev/null && pwd)/allow_hosts.txt"
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

# jail_version_of <output>: prints (one line) the X.Y.Z version in <output>, the stdout of
# 'ai-jail --version' ("ai-jail 3.2.0", one trailing line break allowed), or an empty line when
# it isn't exactly that: another text, more lines, control characters, a part that isn't a
# number, a leading zero ("03"), more than 9 digits or a suffix ("-rc1"). Pure: never runs
# ai-jail.
jail_version_of() {
  local text="${1%$'\n'}" version major minor patch rest part
  case "$text" in
    "ai-jail "*) version="${text#ai-jail }" ;;
    *) echo; return 0 ;;
  esac
  case "$version" in
    *[!0-9.]* | *.*.*.*) echo; return 0 ;;
    *.*.*) ;;
    *) echo; return 0 ;;
  esac
  major="${version%%.*}"
  rest="${version#*.}"
  minor="${rest%%.*}"
  patch="${rest#*.}"
  for part in "$major" "$minor" "$patch"; do
    case "$part" in
      "" | 0?* | ??????????*) echo; return 0 ;;
    esac
  done
  echo "$version"
}

# jail_version_supported <output>: returns 0 when the version in <output> (jail_version_of) is
# in the supported range — JAIL_VERSION_MIN or later, in the same major series (numeric
# comparison: 3.10.0 is newer than 3.2.0) — and 1 otherwise, an unreadable version included.
jail_version_supported() {
  local version major minor patch rest min_major min_minor min_patch
  version="$(jail_version_of "$1")"
  [ -n "$version" ] || return 1
  major="${version%%.*}"
  rest="${version#*.}"
  minor="${rest%%.*}"
  patch="${rest#*.}"
  min_major="${JAIL_VERSION_MIN%%.*}"
  rest="${JAIL_VERSION_MIN#*.}"
  min_minor="${rest%%.*}"
  min_patch="${rest#*.}"
  if [ "$major" -ne "$min_major" ]; then
    return 1
  fi
  if [ "$minor" -gt "$min_minor" ]; then
    return 0
  fi
  if [ "$minor" -eq "$min_minor" ] && [ "$patch" -ge "$min_patch" ]; then
    return 0
  fi
  return 1
}

# jail_version_output: runs '$JAIL_BIN --version' and prints its stdout (stderr dropped),
# returning its exit code. The one place both jail_check and jail_print_requirements ask
# ai-jail for its version; cheap, outside any sandbox, writes nothing.
jail_version_output() {
  "$JAIL_BIN" --version 2>/dev/null
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

# jail_nick_hosts: prints (one line, space separated) the hosts of JAIL_HOSTS_FILE, the nick's
# own list, released in the jail's network whatever the caller passes (today the Claude
# documentation, which ai-jail's default allowlist lacks). One host per line; blank lines,
# everything after a # and the spaces around are ignored. A missing file is an empty list.
# The list is kept by the nick, not the user's ~/.ai-jail; a project's own hosts are the
# project's choice (auto_scrum: JAIL_ALLOW_HOSTS). jail_check validates each one.
jail_nick_hosts() {
  local line hosts=""
  if [ -r "$JAIL_HOSTS_FILE" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
      line="${line%%#*}"
      read -r line <<< "$line" || true
      [ -n "$line" ] && hosts="${hosts:+$hosts }$line"
    done < "$JAIL_HOSTS_FILE"
  fi
  echo "$hosts"
}

# jail_host_valid <host>: returns 0 when <host> is a plain host name — lowercase letters,
# digits, dots and hyphens, with a dot, not starting or ending with either, no ".." nor a
# hyphen next to a dot — and 1 otherwise (a scheme, port, path, wildcard or bare name).
jail_host_valid() {
  case "$1" in
    *[!a-z0-9.-]* | [.-]* | *[.-] | *..* | *.-* | *-.*) return 1 ;;
    *.*) return 0 ;;
  esac
  return 1
}

# jail_options <clipboard> [hosts] [ports]: prints (stdout, one per line) the ai-jail options
# for a JAIL_CLIPBOARD level, the extra hosts and the localhost ports — the only options the
# nick ever passes, all on the command line so none depends on anyone's ~/.ai-jail. Every
# level gets:
#   - --no-save-config: without it, a project with no .ai-jail yet gets one auto-saved in the
#     cwd holding the whole argv — the agent's prompt included — and a bare 'ai-jail' there
#     would replay it; it only stops that write, the project's .ai-jail is still read;
#   - --no-status-bar: ai-jail's status bar shows the command line (the prompt again) and
#     draws over the bottom rows of claude's interface, garbling the terminal;
#   - --clean: ignores the project profile saved by ai-jail's approval screen
#     (~/.config/ai-jail/projects/), and so the screen itself, which ai-jail 3.x opens on the
#     first run of a known harness ("claude") in a terminal and whose suggested preset opens
#     the GitHub token; the command line alone decides. The global config.toml still applies;
#   - --agent-state: claude's login and state (~/.claude, ~/.claude.json), released by the nick
#     instead of by the screen;
#   - --toolchains: ai-jail's dependency tools (the screen's "Dependency tools"): its own
#     writable caches (~/.local/share/ai-jail/cache on the host, seen as ~/.cache, ~/.npm,
#     ~/.cargo/registry...) — persistent and shared by every project and session, so one
#     project's agent can leave a tampered package there for another (risk accepted, in the
#     README and ABOUT.md) — and, read-only, toolchains such as ~/.cargo/bin and ~/.rustup; the
#     home's version managers still come in as --map, below;
#   - --no-github --no-ssh --no-aws --no-kube --no-gcloud --no-docker-config --no-docker
#     --no-inherit-env: close, explicitly, the host credentials and authorities that a global
#     config.toml could open — the GitHub token, SSH and its agent, AWS, kube, gcloud, the
#     Docker config, the Docker socket (root on the host) and the whole environment (which
#     would carry a GH_TOKEN from the shell in). Not --no-network: with --allow-host it drops
#     ai-jail's default host list, the Claude API included.
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
# Last, one --allow-host per host in [hosts] (space separated, in order; jail_check has
# validated them), followed by the nick's own list (jail_nick_hosts): ai-jail adds each
# one, and its subdomains, to its default network allowlist, which stays as it is. Then one
# --forward-port per port in [ports] (space separated, in order; jail_check has validated
# them): ai-jail exposes the machine's 127.0.0.1:<port> on the jail's own loopback, and only
# those ports.
jail_options() {
  local host_list host dir port_list port browsers
  printf '%s\n' --no-save-config --no-status-bar
  printf '%s\n' --clean --agent-state --toolchains --no-github --no-ssh --no-aws --no-kube \
    --no-gcloud --no-docker-config --no-docker --no-inherit-env
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
  read -ra host_list <<< "${2:-} $(jail_nick_hosts)"
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
# toolchains, with ~ for the home), the extra hosts and the localhost ports, says that the
# GitHub token and SSH are not there (jail_options closes them: local git works, gh and push
# are the user's, outside the jail) and says what to do instead: stop and tell the user. In
# PT-BR, like the prompts.
jail_agent_notice() {
  local home="${HOME:-}" dir toolchains="" host_list hosts="" port_list ports="" localhost
  local playwright="" docs
  docs="$(jail_nick_hosts)"
  docs="${docs:+, os hosts liberados pelo nick (${docs// /, })}"
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
    echo "- o home é privado: fora do projeto, por padrão só aparece o que a jail monta (o estado do claude, a configuração do git e os gerenciadores de versão desta máquina ($toolchains), somente leitura, além do que o próprio ai-jail monta pras ferramentas de dependência: caches de dependência graváveis (~/.cache, ~/.npm, ~/.cargo/registry e afins) e, somente leitura, toolchains como ~/.cargo/bin e ~/.rustup, quando existem na máquina); os outros diretórios do home (~/.ssh, ~/.config etc.) não existem aqui, mesmo existindo na máquina;"
  else
    echo "- o home é privado: fora do projeto, por padrão só aparece o que a jail monta (o estado do claude, a configuração do git e o que o próprio ai-jail monta pras ferramentas de dependência: caches de dependência graváveis (~/.cache, ~/.npm, ~/.cargo/registry e afins) e, somente leitura, toolchains como ~/.cargo/bin e ~/.rustup, quando existem na máquina); os outros diretórios do home (~/.ssh, ~/.config etc.) não existem aqui, mesmo existindo na máquina;"
  fi
  if [ -n "$hosts" ]; then
    echo "- por padrão, a rede só alcança a API do Claude, os registries de pacotes e o GitHub$docs, e também: $hosts; $localhost;"
  else
    echo "- por padrão, a rede só alcança a API do Claude, os registries de pacotes e o GitHub$docs; $localhost;"
  fi
  echo "- outro site (ex: a documentação de uma biblioteca) é recusado pela jail com \"proxy refused the connection\": não tente contornar; diga ao usuário qual host faltou, que ele pode liberar na chave JAIL_ALLOW_HOSTS da configuração do projeto;"
  if [ -n "$playwright" ]; then
    echo "$playwright"
  fi
  echo "- o token do GitHub e o SSH (chaves e agente) desta máquina não estão disponíveis aqui: o git local (status, diff, log, commit) funciona, e o fetch de repositório público também, mas o que exige a conta do GitHub (gh, git push, PR, fetch de repositório privado) falha por falta de credencial (o git pede usuário e senha, que não existem aqui, e pode ficar esperando a resposta) e fica com o usuário, fora da jail — não tente configurar credencial nem responder ao pedido; peça a ele;"
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
# <clipboard>, <hosts> or <ports>). With "true", first rejects a <clipboard> other than
# "restrito", "medio", "brando" or "" (a typo must never pick a level silently), <hosts> that
# aren't plain host names (jail_host_valid; the nick's own list, jail_nick_hosts, is checked
# the same way, first) and <ports> that aren't space-separated numbers from 1 to 65535
# without leading zeros nor repeats (a typo must never open another host or port nor reach
# ai-jail as an option), then checks that the jail works on this
# machine: after command -v, it asks for the version ('$JAIL_BIN --version', stdout only) and
# stops when it is outside the supported range (jail_version_supported) or unreadable, before
# running anything in a sandbox; then it runs
# '$JAIL_BIN <jail_options of clipboard, hosts and ports> -- true' — the
# launch's own options, so an ai-jail that doesn't accept them fails here, before the launch —
# with the cwd in a fresh temporary directory (a second guard: no .ai-jail ever lands in the
# target project), and removes the directory afterwards, on success and on failure. Returns 0
# quietly when the check passes; otherwise prints the error on stderr and returns 1:
# jail_print_clipboard_error, jail_print_hosts_error or jail_print_ports_error for the value,
# jail_print_check_error for a missing ai-jail, an unsupported version (way out: update it), a
# failing mktemp or a non-zero check (missing
# bwrap, blocked user namespaces and unsupported systems all end up there, with ai-jail's
# output in the message).
# There is no fallback: "true" never runs without the jail. Callers run it before || (it stops
# the tool, without exit) and never inside $(...), so the message reaches the terminal. Being
# before ||, set -e is off in here: every command that can fail is checked explicitly.
jail_check() {
  local enabled="$1" clipboard="$2" hosts="$3" ports="$4" conf_path="$5" shared="$6" options
  local tmp_dir output failed=0 port_list port seen=" " bad host_list host version_output version
  local version_rc
  JAIL_LANDLOCK=""
  [ "$enabled" = "true" ] || return 0
  case "$clipboard" in
    "" | restrito | medio | brando) ;;
    *)
      jail_print_clipboard_error "$clipboard" "$conf_path"
      return 1
      ;;
  esac
  read -ra host_list <<< "$(jail_nick_hosts)"
  for host in "${host_list[@]}"; do
    if ! jail_host_valid "$host"; then
      jail_print_hosts_error "$host" "$JAIL_HOSTS_FILE (lista padrão do nick)"
      return 1
    fi
  done
  read -ra host_list <<< "$hosts"
  for host in "${host_list[@]}"; do
    if ! jail_host_valid "$host"; then
      jail_print_hosts_error "$host" "JAIL_ALLOW_HOSTS em $conf_path"
      return 1
    fi
  done
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
  version_rc=0
  version_output="$(jail_version_output)" || version_rc=$?
  if [ "$version_rc" -ne 0 ] || ! jail_version_supported "$version_output"; then
    version="$(jail_version_of "$version_output")"
    if [ "$version_rc" -ne 0 ]; then
      version="não identificada (\"ai-jail --version\" falhou com código $version_rc)"
    elif [ -n "$version" ]; then
      version="encontrada $version"
    elif [ -n "$version_output" ]; then
      version="não identificada (saída de \"ai-jail --version\": \"${version_output//[[:cntrl:]]/?}\")"
    else
      version="não identificada (\"ai-jail --version\" não respondeu)"
    fi
    jail_print_check_error "$conf_path" "$shared" "versão do ai-jail não suportada: $version; suportada: $JAIL_VERSION_MIN ou mais nova da série ${JAIL_VERSION_MIN%%.*}.x." "" "atualize o ai-jail para uma versão suportada (comando na lista abaixo);"
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

# jail_print_hosts_error <host> <source>: the error of jail_check for a host that is not a
# plain host name (jail_host_valid), on stderr: where it came from, the host and the accepted
# format. <source> is "JAIL_ALLOW_HOSTS em <conf_path>" for the caller's hosts (the MCPs' are
# always valid, so the bad one came from that key) or the nick's own list file. No
# requirements list: it is a typo. Control characters become "?".
jail_print_hosts_error() {
  local host="${1//[[:cntrl:]]/?}" origin="${2//[[:cntrl:]]/?}"
  {
    echo "Erro: $origin tem um host inválido: \"$host\"."
    echo 'Use só nomes de host (letras minúsculas, números, ponto e hífen), sem https://, porta ou caminho, separados por espaço, ex: JAIL_ALLOW_HOSTS="docs.djangoproject.com tailwindcss.com". Cada host libera também os subdomínios dele.'
  } >&2
}

# jail_print_check_error <conf_path> <shared> <reason> [output] [way_out]: the single error
# text of jail_check, on stderr: the config file that turned the jail on, the reason (or, when
# <reason> is empty, ai-jail's captured output), the two ways out — the first replaced by
# [way_out] when it is given (an unsupported version: update ai-jail; without it, the text is
# the same as before [way_out] existed) — a warning when <shared> is "true"
# (the file is shared with the target team, so turning the jail off there affects everyone once
# committed) and, last, the requirements list. With ai-jail's output, the first way out is
# fixing the problem shown: ai-jail and bwrap may both be installed (blocked user namespaces, a
# bwrap failing ai-jail's trusted-binary validation), so the list may be all [ok]. The output
# and the path go to the terminal, so control characters become "?" line by line, keeping the
# line breaks ([:cntrl:], not [:print:], which under the C locale would also mangle the bytes
# of "não").
jail_print_check_error() {
  local conf_path="${1//[[:cntrl:]]/?}" shared="$2" reason="$3" output="${4:-}" way_out="${5:-}" line
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
    if [ -n "$way_out" ]; then
      echo "  1. $way_out"
    elif [ -n "$reason" ]; then
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
# on Linux, bwrap (bubblewrap) — each as [ok] or [falta] (command -v), and the install
# command of the detected platform for each missing one. An ai-jail in the PATH is also asked
# its version (jail_version_output, cheap and outside any sandbox): "[ok]    ai-jail <X.Y.Z>"
# in the supported range, "[atualizar] ai-jail <X.Y.Z or versão não identificada>  →  <update
# command>" otherwise (the install command, but brew upgrade on macOS). A line with the
# supported range (JAIL_VERSION_MIN) always follows. The platform comes from uname -s
# and, on Linux, from the ID/ID_LIKE of JAIL_OS_RELEASE_FILE. This is the single source of
# "what to install and how": jail_check's error ends with it, each tool exposes it through a
# flag, and the README repeats the commands (a test checks they match). It only shows the
# commands — it never runs an installer, never downloads anything.
jail_print_requirements() {
  local system ids label jail_cmd upgrade_cmd="" bwrap_cmd="" linux=0 version_output version
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
      upgrade_cmd="brew upgrade ai-jail"
      ;;
    *)
      label="outro sistema (${system:-desconhecido})"
      jail_cmd="não suportado: use o WSL 2 e rode o nick dentro dele"
      ;;
  esac
  echo "Requisitos da jail (JAIL_ENABLED=\"true\") — sistema detectado: $label"
  if command -v "$JAIL_BIN" >/dev/null 2>&1; then
    if ! version_output="$(jail_version_output)"; then
      version_output=""
    fi
    version="$(jail_version_of "$version_output")"
    if jail_version_supported "$version_output"; then
      echo "  [ok]    ai-jail $version"
    else
      echo "  [atualizar] ai-jail ${version:-(versão não identificada)}  →  ${upgrade_cmd:-$jail_cmd}"
    fi
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
  echo "Versão suportada do ai-jail: $JAIL_VERSION_MIN ou mais nova da série ${JAIL_VERSION_MIN%%.*}.x."
  echo "O nick não instala nada: rode os comandos acima você mesmo."
  if [ "$linux" -eq 1 ]; then
    echo "Ubuntu 24.04+ e Debian 13+ podem bloquear user namespaces sem privilégio (AppArmor): veja os problemas comuns na seção \"Sandbox (ai-jail)\" do README."
    echo "Depois de instalar, confira com: ai-jail --dry-run claude (na raiz do projeto alvo; não grava nada)."
  fi
}
