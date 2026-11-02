#!/usr/bin/env bash
#
# test_jail: testes em bash puro (sem framework) da biblioteca lib/jail.sh. Cada caso roda num
# bash -c separado, que dá source só em lib/jail.sh — nunca em lib/config.sh, o que prova que
# as duas bibliotecas são independentes. O ai-jail real nunca é usado: os casos põem à frente
# do PATH stubs de ai-jail, claude, bwrap e uname, criados no TEST_TMP desta suíte, e o caso
# "ai-jail ausente" sobrescreve JAIL_BIN com um nome que não existe. A integração com o
# auto_scrum fica em auto_scrum/tests/test_auto_scrum.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JAIL_SH="$SCRIPT_DIR/../jail.sh"
README_MD="$SCRIPT_DIR/../../README.md"

failures=0

# Diretório-base temporário único da suíte: cada teste usa um subdiretório dele, e nada é
# escrito dentro do repositório.
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

# shellcheck source=SCRIPTDIR/assert.sh
source "$SCRIPT_DIR/assert.sh"

# code_lines <file>: imprime as linhas de <file> que não são comentário de linha inteira.
code_lines() {
  grep -vE '^[[:space:]]*#' "$1" || true
}

# Os stubs usam o caminho absoluto do bash no shebang: alguns casos rodam com um PATH só de
# stubs, onde um "#!/usr/bin/env bash" não acharia o bash.
BASH_BIN="$(command -v bash)"
MISSING_JAIL_BIN="ai-jail-inexistente-nick-13"

# Stub do ai-jail (diretório próprio, à frente do PATH). Cada chamada grava o argv (um
# argumento por linha) em $FAKE_JAIL_LOG_DIR/call_<n>.argv e o cwd em call_<n>.cwd, pula as
# opções antes do "--" e, sem --no-save-config entre elas, cria um .ai-jail no próprio cwd
# (simula o auto-save do ai-jail real). Se o comando depois do "--" for "true", é a
# verificação: imprime $FAKE_JAIL_STDERR no stderr (se definido) e sai com $FAKE_JAIL_RC
# (padrão 0). Senão, é o lançamento: faz exec do comando depois do "--".
FAKE_JAIL_DIR="$TEST_TMP/stub_jail"
mkdir -p "$FAKE_JAIL_DIR"
cat > "$FAKE_JAIL_DIR/ai-jail" <<EOF
#!$BASH_BIN
n=1
while [ -e "\$FAKE_JAIL_LOG_DIR/call_\$n.argv" ]; do n=\$((n + 1)); done
printf '%s\n' "\$@" > "\$FAKE_JAIL_LOG_DIR/call_\$n.argv"
pwd > "\$FAKE_JAIL_LOG_DIR/call_\$n.cwd"
save=1
while [ "\$#" -gt 0 ] && [ "\$1" != "--" ]; do
  [ "\$1" = "--no-save-config" ] && save=0
  shift
done
[ "\$save" -eq 1 ] && : > .ai-jail
[ "\${1:-}" = "--" ] && shift
if [ "\${1:-}" = "true" ]; then
  [ -n "\${FAKE_JAIL_STDERR:-}" ] && printf '%s\n' "\$FAKE_JAIL_STDERR" >&2
  exit "\${FAKE_JAIL_RC:-0}"
fi
exec "\$@"
EOF
chmod +x "$FAKE_JAIL_DIR/ai-jail"

# Stub do claude (diretório próprio): grava o argv (um argumento por linha) em
# $FAKE_CLAUDE_LOG_DIR/argv e o cwd em $FAKE_CLAUDE_LOG_DIR/cwd.
FAKE_CLAUDE_DIR="$TEST_TMP/stub_claude"
mkdir -p "$FAKE_CLAUDE_DIR"
cat > "$FAKE_CLAUDE_DIR/claude" <<EOF
#!$BASH_BIN
printf '%s\n' "\$@" > "\$FAKE_CLAUDE_LOG_DIR/argv"
pwd > "\$FAKE_CLAUDE_LOG_DIR/cwd"
EOF
chmod +x "$FAKE_CLAUDE_DIR/claude"

# Stubs de uname (devolve $FAKE_UNAME, padrão Linux) e de bwrap, cada um no seu diretório.
FAKE_UNAME_DIR="$TEST_TMP/stub_uname"
FAKE_BWRAP_DIR="$TEST_TMP/stub_bwrap"
mkdir -p "$FAKE_UNAME_DIR" "$FAKE_BWRAP_DIR"
# shellcheck disable=SC2016
# Aspas simples intencionais: o ${FAKE_UNAME...} é expandido pelo stub, não aqui.
printf '%s\n' "#!$BASH_BIN" 'echo "${FAKE_UNAME:-Linux}"' > "$FAKE_UNAME_DIR/uname"
printf '%s\n' "#!$BASH_BIN" 'exit 0' > "$FAKE_BWRAP_DIR/bwrap"
chmod +x "$FAKE_UNAME_DIR/uname" "$FAKE_BWRAP_DIR/bwrap"

# new_case <name>: cria (vazio) e imprime o diretório de um caso, com os subdiretórios jail/
# (chamadas ao stub do ai-jail), claude/ (chamada ao stub do claude) e cwd/ (cwd do teste).
new_case() {
  local dir="$TEST_TMP/$1"
  rm -rf "$dir"
  mkdir -p "$dir/jail" "$dir/claude" "$dir/cwd"
  printf '%s' "$dir"
}

# run_jail <case_dir> <commands> [env]: num bash separado, com set -euo pipefail, cwd em
# <case_dir>/cwd e os stubs de ai-jail e claude à frente do PATH, roda [env] (atribuições,
# antes do source), dá source só em lib/jail.sh e roda <commands>. stdout em
# <case_dir>/out, stderr em <case_dir>/err e o código de saída em <case_dir>/rc. O
# '|| t_rc=$?' mantém a suíte rodando (e reportando FAIL) mesmo se o bash de dentro morrer.
run_jail() {
  local case_dir="$1" commands="$2" env="${3:-:}" t_rc=0
  timeout 5 bash -c "cd '$case_dir/cwd' || exit 99; export PATH='$FAKE_JAIL_DIR:$FAKE_CLAUDE_DIR':\"\$PATH\" FAKE_JAIL_LOG_DIR='$case_dir/jail' FAKE_CLAUDE_LOG_DIR='$case_dir/claude'; $env; set -euo pipefail; source '$JAIL_SH'; $commands" < /dev/null > "$case_dir/out" 2> "$case_dir/err" || t_rc=$?
  printf '%s' "$t_rc" > "$case_dir/rc"
}

# jail_calls <case_dir>: imprime quantas vezes o stub do ai-jail foi chamado no caso.
jail_calls() {
  find "$1/jail" -name 'call_*.argv' 2>/dev/null | grep -c . || true
}

# file_of <path>: imprime o conteúdo de <path>, ou "sem arquivo" se ele não existir.
file_of() {
  cat "$1" 2>/dev/null || printf 'sem arquivo'
}

# requirements <case_dir> <uname> <os_release_file> <ai_jail: ok|falta> <bwrap: ok|falta>:
# roda jail_print_requirements num bash com PATH só de stubs (uname sempre; bwrap só com
# "ok"; o ai-jail com "falta" vira o nome inexistente em JAIL_BIN), JAIL_OS_RELEASE_FILE em
# <os_release_file> e cwd em <case_dir>/cwd. stdout em <case_dir>/out, stderr em
# <case_dir>/err, código em <case_dir>/rc.
requirements() {
  local case_dir="$1" uname_value="$2" os_release="$3" jail_state="$4" bwrap_state="$5"
  local path="$FAKE_UNAME_DIR:$FAKE_JAIL_DIR" overrides="JAIL_OS_RELEASE_FILE='$os_release'" t_rc=0
  [ "$bwrap_state" = "ok" ] && path="$path:$FAKE_BWRAP_DIR"
  [ "$jail_state" = "falta" ] && overrides="$overrides; JAIL_BIN='$MISSING_JAIL_BIN'"
  timeout 5 "$BASH_BIN" -c "cd '$case_dir/cwd' || exit 99; PATH='$path'; export FAKE_UNAME='$uname_value'; set -euo pipefail; source '$JAIL_SH'; $overrides; jail_print_requirements" < /dev/null > "$case_dir/out" 2> "$case_dir/err" || t_rc=$?
  printf '%s' "$t_rc" > "$case_dir/rc"
}

# Fixtures de /etc/os-release.
OS_DIR="$TEST_TMP/os_release"
mkdir -p "$OS_DIR"
printf '%s\n' 'NAME="Arch Linux"' 'ID=arch' 'PRETTY_NAME="Arch Linux"' > "$OS_DIR/arch"
printf '%s\n' 'NAME="Manjaro Linux"' 'ID="manjaro"' 'ID_LIKE="arch"' > "$OS_DIR/manjaro"
printf '%s\n' 'NAME="Ubuntu"' 'ID=ubuntu' 'ID_LIKE=debian' 'VERSION_ID="24.04"' > "$OS_DIR/ubuntu"
printf '%s\n' 'NAME="Fedora Linux"' 'ID=fedora' > "$OS_DIR/fedora"
printf '%s\n' 'NAME="Rocky Linux"' 'ID="rocky"' 'ID_LIKE="rhel centos fedora"' > "$OS_DIR/rocky"
printf '%s\n' 'NAME="Alpine Linux"' 'ID=alpine' > "$OS_DIR/alpine"
# Uma linha que um source executaria: o leitor não pode rodá-la.
printf '%s\n' 'ID=arch' "touch '$OS_DIR/executed'" > "$OS_DIR/with_command"

# Comandos de instalação (fonte: tabela da decisão 8 do plano do ticket 13). O teste README
# × helper confere cada um no README.md.
CMD_JAIL_ARCH="yay -S ai-jail-bin"
CMD_JAIL_CARGO="cargo install --locked ai-jail"
CMD_JAIL_RELEASES="https://github.com/akitaonrails/ai-jail/releases"
CMD_JAIL_MAC="brew tap akitaonrails/tap && brew install ai-jail"
CMD_BWRAP_ARCH="sudo pacman -S bubblewrap"
CMD_BWRAP_DEBIAN="sudo apt install bubblewrap"
CMD_BWRAP_FEDORA="sudo dnf install bubblewrap"

# jail_options_of <nível>: o argv esperado da verificação 'ai-jail <opções> -- true' do nível.
jail_options_of() {
  case "$1" in
    restrito) printf '%s\n' --no-save-config --no-status-bar -- true ;;
    brando) printf '%s\n' --no-save-config --no-status-bar --terminal-passthrough --display -- true ;;
    *) printf '%s\n' --no-save-config --no-status-bar --terminal-passthrough -- true ;;
  esac
}

echo "== carga limpa: source só define nomes jail_/JAIL_, sem imprimir nada =="
clean_dir="$TEST_TMP/clean"
mkdir -p "$clean_dir"
clean_rc=0
# Só redirecionamentos entre os retratos, pra o próprio teste não criar variável nenhuma.
timeout 2 bash --norc --noprofile -c "compgen -v | sort > '$clean_dir/vars_before'; declare -F | sort > '$clean_dir/funcs_before'; source '$JAIL_SH' > '$clean_dir/out' 2>&1; compgen -v | sort > '$clean_dir/vars_after'; declare -F | sort > '$clean_dir/funcs_after'" < /dev/null || clean_rc=$?
assert_eq "0" "$clean_rc" "source lib/jail.sh num bash limpo termina com código 0"
assert_eq "" "$(cat "$clean_dir/out" 2>/dev/null || echo 'sem saída gravada')" "source lib/jail.sh não imprime nada (stdout nem stderr)"
clean_new_vars="$(comm -13 "$clean_dir/vars_before" "$clean_dir/vars_after" 2>/dev/null | grep -vxE '_|BASH_.*|FUNCNAME|LINENO|PIPESTATUS' || true)"
clean_new_funcs="$(comm -13 "$clean_dir/funcs_before" "$clean_dir/funcs_after" 2>/dev/null | sed 's/^declare -f //' || true)"
assert_eq "JAIL_BIN"$'\n'"JAIL_OS_RELEASE_FILE" "$clean_new_vars" "as únicas variáveis novas são JAIL_BIN e JAIL_OS_RELEASE_FILE"
assert_eq "" "$(grep -vE '^jail_' <<< "$clean_new_funcs" | grep . || true)" "toda função nova começa com jail_"
for clean_func in jail_check jail_exec jail_options jail_print_requirements jail_print_status; do
  assert_contains "$clean_new_funcs" "$clean_func" "source lib/jail.sh define $clean_func"
done
clean_values="$(timeout 2 bash --norc --noprofile -c "source '$JAIL_SH'; printf '%s|%s' \"\$JAIL_BIN\" \"\$JAIL_OS_RELEASE_FILE\"" 2>/dev/null || true)"
assert_eq "ai-jail|/etc/os-release" "$clean_values" "JAIL_BIN é ai-jail (nome no PATH, sem caminho) e JAIL_OS_RELEASE_FILE é /etc/os-release"

echo
echo "== carga limpa: estrutura do arquivo (sem shebang, sem bit de execução, sem set/exit/source/eval) =="
assert_eq "# shellcheck shell=bash" "$(head -n1 "$JAIL_SH" 2>/dev/null || true)" "a 1ª linha é a diretiva '# shellcheck shell=bash' (sem shebang)"
assert_eq "sem bit de execução" "$([ -e "$JAIL_SH" ] && [ ! -x "$JAIL_SH" ] && echo 'sem bit de execução' || echo 'executável ou inexistente')" "lib/jail.sh existe e não tem bit de execução"
jail_code="$(code_lines "$JAIL_SH" 2>/dev/null || true)"
assert_eq "" "$(grep -nE '(^|[;&|[:space:]])set[[:space:]]+[-+]' <<< "$jail_code" || true)" "nenhum set -e/set -o (nem outro set) fora de comentário"
assert_eq "" "$(grep -nwE 'exit|source|eval' <<< "$jail_code" || true)" "nenhum exit, source ou eval fora de comentário"
assert_eq "" "$(grep -nE '(^|[;&|])[[:space:]]*\.[[:space:]]+[^[:space:]]' <<< "$jail_code" || true)" "nenhum '. arquivo' fora de comentário"

echo
echo "== independência: lib/jail.sh não depende de lib/config.sh =="
assert_eq "" "$(grep -nE 'config_|CONFIG_' <<< "$jail_code" || true)" "lib/jail.sh não cita config_ nem CONFIG_ fora de comentário"
assert_eq "" "$(grep -nF 'config.sh' <<< "$jail_code" || true)" "lib/jail.sh não cita config.sh fora de comentário"
# O padrão não casa com a própria linha: o [^;]* para no ';' do colchete, e a descrição não
# usa a palavra.
assert_eq "" "$(grep -nE 'source[^;]*config' <<< "$(code_lines "$0")" || true)" "esta suíte nunca carrega lib/config.sh"

echo
echo "== estrutural: as opções do ai-jail saem só de jail_options; o nick não mexe no .ai-jail =="
for jail_forbidden in --bootstrap --init --map --rw-map --network --exec; do
  assert_eq "" "$(grep -nF -- "$jail_forbidden" <<< "$jail_code" || true)" "lib/jail.sh não cita $jail_forbidden fora de comentário"
done
# --allow-host (os hosts extras do chamador) só sai de jail_options, uma vez por host.
jail_options_body="$(awk '/^jail_options\(\) \{/{found=1} found{print} found && /^\}/{exit}' "$JAIL_SH" 2>/dev/null || true)"
assert_eq "1" "$(grep -cF -- --allow-host <<< "$jail_code" || true)" "lib/jail.sh cita --allow-host uma vez fora de comentário"
# shellcheck disable=SC2016
assert_contains "$jail_options_body" '--allow-host "$host"' "o --allow-host fica no corpo de jail_options"
assert_eq "" "$(grep -nE '>[[:space:]]*[^[:space:]]*\.ai-jail' <<< "$jail_code" || true)" "lib/jail.sh não escreve em .ai-jail"
assert_eq "" "$(grep -nE '(^|[^-])ai-jail[^[:space:]]* --[[:space:]]|exec[[:space:]]+ai-jail' <<< "$jail_code" || true)" "lib/jail.sh só chama o ai-jail por \$JAIL_BIN (nunca pelo nome literal)"
# Toda chamada de "$JAIL_BIN" (fora o command -v) é '"$JAIL_BIN" "${options[@]}" -- ...': a
# verificação e o lançamento, nada além disso; e options só vem de jail_options. O $JAIL_BIN,
# o ${options[@]} e o $clipboard entre aspas simples são o texto literal procurado no código.
# shellcheck disable=SC2016
jail_bin_calls="$(grep -F '"$JAIL_BIN"' <<< "$jail_code" | grep -vF 'command -v "$JAIL_BIN"' || true)"
assert_eq "2" "$(grep -c . <<< "$jail_bin_calls" || true)" "lib/jail.sh chama \$JAIL_BIN em dois lugares (verificação e lançamento)"
# shellcheck disable=SC2016
assert_eq "" "$(grep -vF '"$JAIL_BIN" "${options[@]}" -- ' <<< "$jail_bin_calls" || true)" "toda chamada de \$JAIL_BIN é '\$JAIL_BIN \"\${options[@]}\" -- ...'"
# shellcheck disable=SC2016
assert_eq "2" "$(grep -cF 'mapfile -t options < <(jail_options "$clipboard" "$hosts")' <<< "$jail_code" || true)" "a verificação e o lançamento montam options com jail_options"
assert_eq "" "$(grep -nE '(^|[[:space:]])options\+?=' <<< "$jail_code" || true)" "options nunca é montado à mão (só por jail_options)"

echo
echo "== jail_print_status: uma linha só; ativa com ⚠️ e o modo de clipboard =="
status_off="Jail (ai-jail): desligada — o agente roda direto no seu shell."
status_restrito="⚠️ Jail (ai-jail): ativa | modo de clipboard restrito"
status_medio="⚠️ Jail (ai-jail): ativa | modo de clipboard médio"
status_brando="⚠️ Jail (ai-jail): ativa | modo de clipboard brando"
for status_value in true "" false TRUE; do
  for status_clip in "" medio restrito brando; do
    status_dir="$(new_case "status_${status_value:-vazio}_${status_clip:-vazio}")"
    run_jail "$status_dir" "jail_print_status '$status_value' '$status_clip'"
    status_label="jail_print_status \"$status_value\" \"$status_clip\""
    if [ "$status_value" != "true" ]; then
      status_expected="$status_off"
    else
      case "$status_clip" in
        restrito) status_expected="$status_restrito" ;;
        brando) status_expected="$status_brando" ;;
        *) status_expected="$status_medio" ;;
      esac
    fi
    # Fora de um terminal (aqui, um arquivo), sem cor nenhuma.
    assert_eq "$status_expected" "$(file_of "$status_dir/out")" "$status_label: uma linha, sem cor fora do terminal"
    assert_eq "0|" "$(file_of "$status_dir/rc")|$(file_of "$status_dir/err")" "$status_label: código 0 e nada no stderr"
  done
done

echo
echo "== jail_print_status num terminal: só o 'modo de clipboard brando' em vermelho (e nada com NO_COLOR) =="
# script (util-linux) roda o comando num pseudo-terminal, pra o [ -t 1 ] valer.
if script -qec true /dev/null > /dev/null 2>&1; then
  status_red=$'\e[31m'
  status_reset=$'\e[0m'
  for status_clip in brando medio restrito; do
    status_tty="$(script -qec "bash --norc --noprofile -c 'unset NO_COLOR; source \"$JAIL_SH\"; jail_print_status true $status_clip'" /dev/null < /dev/null | tr -d '\r' || true)"
    case "$status_clip" in
      brando) status_tty_expected="⚠️ Jail (ai-jail): ativa | ${status_red}modo de clipboard brando${status_reset}" ;;
      medio) status_tty_expected="$status_medio" ;;
      *) status_tty_expected="$status_restrito" ;;
    esac
    assert_eq "$status_tty_expected" "$status_tty" "num terminal, \"$status_clip\": $([ "$status_clip" = brando ] && echo 'o modo em vermelho' || echo 'sem cor')"
  done
  status_tty="$(script -qec "bash --norc --noprofile -c 'export NO_COLOR=1; source \"$JAIL_SH\"; jail_print_status true brando'" /dev/null < /dev/null | tr -d '\r' || true)"
  assert_eq "$status_brando" "$status_tty" "num terminal com NO_COLOR, \"brando\": sem cor"
else
  echo "  PULADO: sem o script do util-linux pra simular um terminal"
fi

echo
echo "== jail_options: as opções do ai-jail de cada nível, uma por linha =="
for opt_clip in "" medio restrito brando; do
  opt_dir="$(new_case "options_${opt_clip:-vazio}")"
  run_jail "$opt_dir" "jail_options '$opt_clip'"
  case "$opt_clip" in
    restrito) opt_expected=(--no-save-config --no-status-bar) ;;
    brando) opt_expected=(--no-save-config --no-status-bar --terminal-passthrough --display) ;;
    *) opt_expected=(--no-save-config --no-status-bar --terminal-passthrough) ;;
  esac
  assert_eq "$(printf '%s\n' "${opt_expected[@]}")" "$(file_of "$opt_dir/out")" "jail_options \"$opt_clip\": ${opt_expected[*]}"
  assert_eq "0|" "$(file_of "$opt_dir/rc")|$(file_of "$opt_dir/err")" "jail_options \"$opt_clip\": código 0 e nada no stderr"
done
# Hosts extras (separados por espaço, como o chamador passa): um --allow-host por host, na
# ordem, depois das opções do nível. Vazio ou só espaços: nenhum.
opt_hosts_case=0
for opt_hosts in "" " " "mcp.atlassian.com" "mcp.atlassian.com mcp.sentry.dev" "  mcp.atlassian.com   mcp.sentry.dev "; do
  opt_hosts_case=$((opt_hosts_case + 1))
  opt_dir="$(new_case "options_hosts_$opt_hosts_case")"
  run_jail "$opt_dir" "jail_options brando '$opt_hosts'"
  opt_expected=(--no-save-config --no-status-bar --terminal-passthrough --display)
  read -ra opt_host_list <<< "$opt_hosts"
  for opt_host in "${opt_host_list[@]}"; do
    opt_expected+=(--allow-host "$opt_host")
  done
  assert_eq "$(printf '%s\n' "${opt_expected[@]}")" "$(file_of "$opt_dir/out")" "jail_options brando \"$opt_hosts\": ${opt_expected[*]}"
done

echo
echo "== jail_exec: desligada lança o comando direto; ligada lança 'ai-jail <opções> -- <comando>' =="
# Argumentos difíceis: espaço, aspas, começando com '-', vazio, glob e várias linhas.
exec_args=(--permission-mode auto "com espaço" "aspas \"duplas\" e 'simples'" "-x" "--" "" "*" $'linha 1\nlinha 2')
printf -v exec_call '%q ' "${exec_args[@]}"
exec_expected="$(printf '%s\n' "${exec_args[@]}")"
for exec_value in "" false TRUE; do
  exec_dir="$(new_case "exec_off_${exec_value:-vazio}")"
  run_jail "$exec_dir" "jail_exec '$exec_value' brando '' claude $exec_call; echo 'não deveria chegar aqui'"
  assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"$exec_value\": o claude recebe exatamente o argv passado"
  assert_eq "0" "$(jail_calls "$exec_dir")" "jail_exec \"$exec_value\": o ai-jail não é chamado"
  assert_not_contains "$(file_of "$exec_dir/out")" "não deveria chegar aqui" "jail_exec \"$exec_value\": faz exec (não retorna)"
done
exec_dir="$(new_case exec_on)"
run_jail "$exec_dir" "jail_exec true '' '' claude $exec_call; echo 'não deveria chegar aqui'"
assert_eq "1" "$(jail_calls "$exec_dir")" "jail_exec \"true\": o ai-jail é chamado uma vez"
assert_eq "$(printf '%s\n' --no-save-config --no-status-bar --terminal-passthrough -- claude "${exec_args[@]}")" "$(file_of "$exec_dir/jail/call_1.argv")" "jail_exec \"true\" \"\": o ai-jail recebe '--no-save-config', '--no-status-bar', '--terminal-passthrough', '--', 'claude' e o argv intacto"
assert_not_exists "$exec_dir/cwd/.ai-jail" "jail_exec \"true\": o lançamento não deixa .ai-jail no cwd (o prompt não fica gravado no projeto)"
assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"true\": o claude recebe o mesmo argv do caso desligado"
assert_eq "$exec_dir/cwd" "$(file_of "$exec_dir/jail/call_1.cwd")" "jail_exec \"true\": o ai-jail roda no cwd do chamador"
assert_not_contains "$(file_of "$exec_dir/out")" "não deveria chegar aqui" "jail_exec \"true\": faz exec (não retorna)"
# Cada nível lança com as opções de jail_options.
for exec_clip in medio restrito brando; do
  exec_dir="$(new_case "exec_on_$exec_clip")"
  run_jail "$exec_dir" "jail_exec true '$exec_clip' '' claude $exec_call; echo 'não deveria chegar aqui'"
  case "$exec_clip" in
    restrito) exec_options=(--no-save-config --no-status-bar) ;;
    brando) exec_options=(--no-save-config --no-status-bar --terminal-passthrough --display) ;;
    *) exec_options=(--no-save-config --no-status-bar --terminal-passthrough) ;;
  esac
  assert_eq "$(printf '%s\n' "${exec_options[@]}" -- claude "${exec_args[@]}")" "$(file_of "$exec_dir/jail/call_1.argv")" "jail_exec \"true\" \"$exec_clip\": opções ${exec_options[*]}"
  assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"true\" \"$exec_clip\": o claude recebe o argv intacto"
done
# Os hosts extras viram --allow-host antes do "--"; com a jail desligada, não mudam nada.
exec_dir="$(new_case exec_on_hosts)"
run_jail "$exec_dir" "jail_exec true '' 'mcp.atlassian.com mcp.sentry.dev' claude $exec_call; echo 'não deveria chegar aqui'"
assert_eq "$(printf '%s\n' --no-save-config --no-status-bar --terminal-passthrough --allow-host mcp.atlassian.com --allow-host mcp.sentry.dev -- claude "${exec_args[@]}")" "$(file_of "$exec_dir/jail/call_1.argv")" "jail_exec \"true\" com hosts: um --allow-host por host, antes do '--'"
assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"true\" com hosts: o claude recebe o argv intacto"
exec_dir="$(new_case exec_off_hosts)"
run_jail "$exec_dir" "jail_exec false '' 'mcp.atlassian.com mcp.sentry.dev' claude $exec_call; echo 'não deveria chegar aqui'"
assert_eq "0" "$(jail_calls "$exec_dir")" "jail_exec \"false\" com hosts: o ai-jail não é chamado"
assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"false\" com hosts: o claude recebe exatamente o argv passado"

echo
echo "== jail_print_requirements: plataforma, [ok]/[falta] e o comando de cada item que falta =="
req_dir="$(new_case req_arch)"
requirements "$req_dir" Linux "$OS_DIR/arch" falta ok
req_out="$(file_of "$req_dir/out")"
assert_eq "0" "$(file_of "$req_dir/rc")" "Arch: código 0"
assert_contains "$req_out" "sistema detectado: Linux (Arch)" "Arch: nome da plataforma"
assert_contains "$req_out" "[falta] ai-jail" "Arch sem ai-jail: [falta] ai-jail"
assert_contains "$req_out" "$CMD_JAIL_ARCH" "Arch sem ai-jail: comando '$CMD_JAIL_ARCH'"
assert_contains "$req_out" "[ok]    bwrap" "Arch com bwrap: [ok] bwrap"
assert_not_contains "$req_out" "$CMD_BWRAP_ARCH" "Arch com bwrap: sem comando de instalação do bubblewrap"
assert_not_contains "$req_out" "$CMD_JAIL_CARGO" "Arch: sem o comando do cargo"
assert_contains "$req_out" "ai-jail --dry-run claude" "Linux: nota de conferência com ai-jail --dry-run claude"
assert_contains "$req_out" "AppArmor" "Linux: nota de user namespaces bloqueados (AppArmor)"
assert_contains "$req_out" "O nick não instala nada" "a saída diz que o nick não instala nada"
assert_eq "" "$(file_of "$req_dir/err")" "Arch: nada no stderr"

req_dir="$(new_case req_arch_ok)"
requirements "$req_dir" Linux "$OS_DIR/arch" ok ok
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "[ok]    ai-jail" "Arch com os dois: [ok] ai-jail"
assert_contains "$req_out" "[ok]    bwrap" "Arch com os dois: [ok] bwrap"
assert_not_contains "$req_out" "[falta]" "Arch com os dois: nenhum [falta]"
assert_not_contains "$req_out" "$CMD_JAIL_ARCH" "Arch com os dois: nenhum comando de instalação"

req_dir="$(new_case req_arch_no_bwrap)"
requirements "$req_dir" Linux "$OS_DIR/arch" ok falta
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "[falta] bwrap" "Arch sem bwrap: [falta] bwrap"
assert_contains "$req_out" "$CMD_BWRAP_ARCH" "Arch sem bwrap: comando '$CMD_BWRAP_ARCH'"

req_dir="$(new_case req_manjaro)"
requirements "$req_dir" Linux "$OS_DIR/manjaro" falta falta
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "Linux (Arch)" "Manjaro (ID_LIKE=\"arch\" entre aspas): reconhecida como Arch"
assert_contains "$req_out" "$CMD_JAIL_ARCH" "Manjaro: comando do ai-jail do Arch"
assert_contains "$req_out" "$CMD_BWRAP_ARCH" "Manjaro: comando do bubblewrap do Arch"

req_dir="$(new_case req_ubuntu)"
requirements "$req_dir" Linux "$OS_DIR/ubuntu" falta falta
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "Linux (Debian/Ubuntu)" "Ubuntu (ID=ubuntu ID_LIKE=debian): nome da plataforma"
assert_contains "$req_out" "$CMD_JAIL_CARGO" "Ubuntu: comando '$CMD_JAIL_CARGO'"
assert_contains "$req_out" "$CMD_JAIL_RELEASES" "Ubuntu: link dos Releases do ai-jail"
assert_contains "$req_out" "$CMD_BWRAP_DEBIAN" "Ubuntu: comando '$CMD_BWRAP_DEBIAN'"
assert_not_contains "$req_out" "$CMD_JAIL_ARCH" "Ubuntu: sem o comando do Arch"

req_dir="$(new_case req_fedora)"
requirements "$req_dir" Linux "$OS_DIR/fedora" falta falta
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "Linux (Fedora/RHEL)" "Fedora: nome da plataforma"
assert_contains "$req_out" "$CMD_JAIL_CARGO" "Fedora: comando '$CMD_JAIL_CARGO'"
assert_contains "$req_out" "$CMD_BWRAP_FEDORA" "Fedora: comando '$CMD_BWRAP_FEDORA'"

req_dir="$(new_case req_rocky)"
requirements "$req_dir" Linux "$OS_DIR/rocky" falta falta
assert_contains "$(file_of "$req_dir/out")" "$CMD_BWRAP_FEDORA" "Rocky (ID_LIKE=\"rhel centos fedora\" entre aspas): comando do Fedora/RHEL"

req_dir="$(new_case req_alpine)"
requirements "$req_dir" Linux "$OS_DIR/alpine" falta falta
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "Linux (outra distribuição)" "Alpine: outro Linux"
assert_contains "$req_out" "$CMD_JAIL_CARGO" "Alpine: comando '$CMD_JAIL_CARGO'"
assert_contains "$req_out" "instale o pacote bubblewrap da sua distribuição" "Alpine: bubblewrap pelo pacote da distribuição"

req_dir="$(new_case req_no_os_release)"
requirements "$req_dir" Linux "$OS_DIR/nao-existe" falta falta
req_out="$(file_of "$req_dir/out")"
assert_eq "0" "$(file_of "$req_dir/rc")" "os-release ausente: código 0"
assert_contains "$req_out" "Linux (outra distribuição)" "os-release ausente: outro Linux"
assert_eq "" "$(file_of "$req_dir/err")" "os-release ausente: nada no stderr"

req_dir="$(new_case req_os_release_dir)"
requirements "$req_dir" Linux "$OS_DIR" falta falta
assert_contains "$(file_of "$req_dir/out")" "Linux (outra distribuição)" "os-release ilegível (diretório): outro Linux"
assert_eq "0|" "$(file_of "$req_dir/rc")|$(file_of "$req_dir/err")" "os-release ilegível (diretório): código 0 e nada no stderr"

req_dir="$(new_case req_os_release_command)"
requirements "$req_dir" Linux "$OS_DIR/with_command" falta falta
assert_contains "$(file_of "$req_dir/out")" "Linux (Arch)" "os-release com uma linha de comando: ID=arch ainda é lido"
assert_not_exists "$OS_DIR/executed" "os-release é lido sem source: a linha de comando não roda"

req_dir="$(new_case req_mac)"
requirements "$req_dir" Darwin "$OS_DIR/arch" falta falta
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "sistema detectado: macOS" "Darwin: macOS (ignora o os-release)"
assert_contains "$req_out" "$CMD_JAIL_MAC" "macOS: comando '$CMD_JAIL_MAC'"
assert_not_contains "$req_out" "bwrap" "macOS: sem linha de bwrap"
assert_not_contains "$req_out" "$CMD_JAIL_ARCH" "macOS: sem o comando do Arch"

req_dir="$(new_case req_windows)"
requirements "$req_dir" MINGW64_NT-10.0 "$OS_DIR/arch" falta falta
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "outro sistema" "MINGW64_NT: outro sistema"
assert_contains "$req_out" "WSL 2" "MINGW64_NT: cita o WSL 2"
assert_not_contains "$req_out" "bwrap" "MINGW64_NT: sem linha de bwrap"

echo
echo "== jail_print_requirements: só mostra os comandos, nunca os executa =="
# Tira as strings entre aspas das linhas de código: o que sobrar não pode citar um
# instalador (os comandos só podem aparecer dentro de texto impresso).
jail_unquoted="$(sed -E "s/\"([^\"\\\\]|\\\\.)*\"//g; s/'[^']*'//g" <<< "$jail_code")"
assert_eq "" "$(grep -nwE 'pacman|yay|apt|apt-get|dnf|brew|cargo|sudo|curl|wget' <<< "$jail_unquoted" || true)" "nenhuma linha de lib/jail.sh executa pacman/yay/apt/dnf/brew/cargo/sudo/curl/wget"
req_body="$(awk '/^jail_print_requirements\(\) \{/{found=1} found{print} found && /^\}/{exit}' "$JAIL_SH" 2>/dev/null || true)"
assert_contains "$req_body" "$CMD_JAIL_ARCH" "o comando do Arch fica no corpo de jail_print_requirements (fonte única)"

echo
echo "== README × helper: cada comando de instalação do helper aparece literal no README.md =="
readme_text="$(cat "$README_MD" 2>/dev/null || true)"
for readme_cmd in "$CMD_JAIL_ARCH" "$CMD_JAIL_CARGO" "$CMD_JAIL_RELEASES" "$CMD_JAIL_MAC" "$CMD_BWRAP_ARCH" "$CMD_BWRAP_DEBIAN" "$CMD_BWRAP_FEDORA"; do
  assert_contains "$readme_text" "$readme_cmd" "README.md cita '$readme_cmd'"
  assert_contains "$req_body" "$readme_cmd" "jail_print_requirements cita '$readme_cmd'"
done

echo
echo "== README × jail_options: os níveis do JAIL_CLIPBOARD e as opções de cada um aparecem no README.md =="
for readme_level in restrito medio brando; do
  assert_contains "$readme_text" "\"$readme_level\"" "README.md cita o nível \"$readme_level\""
  while IFS= read -r readme_option; do
    assert_contains "$readme_text" "$readme_option" "README.md cita a opção $readme_option (nível $readme_level)"
  done < <(jail_options "$readme_level")
done
# A mensagem de erro da verificação remete a essas seções do README.
assert_contains "$readme_text" "## Sandbox (ai-jail)" "README.md tem a seção \"Sandbox (ai-jail)\" citada no erro"
assert_contains "$readme_text" "### Problemas comuns" "README.md tem a subseção \"Problemas comuns\" citada no erro"
assert_contains "$readme_text" "validação de binário confiável" "README.md, em Problemas comuns, explica o bwrap reprovado na validação de binário confiável"

echo
echo "== jail_check desligada: retorna 0 sem imprimir nada nem chamar o ai-jail =="
for check_value in "" false TRUE; do
  check_dir="$(new_case "check_off_${check_value:-vazio}")"
  run_jail "$check_dir" "jail_check '$check_value' total '' '/proj/.nick.conf' true; echo depois" "export FAKE_JAIL_RC=1"
  assert_eq "0|depois|" "$(file_of "$check_dir/rc")|$(file_of "$check_dir/out")|$(file_of "$check_dir/err")" "jail_check \"$check_value\" (JAIL_CLIPBOARD inválido): código 0, nada impresso (só o 'depois' do teste)"
  assert_eq "0" "$(jail_calls "$check_dir")" "jail_check \"$check_value\": o ai-jail não é chamado"
done

echo
echo "== jail_check ligada, jail funcionando: verifica num diretório temporário, removido em seguida =="
check_dir="$(new_case check_ok)"
run_jail "$check_dir" "jail_check true '' '' '/proj/.nick.conf' false; echo \"rc=\$?\""
assert_eq "rc=0" "$(file_of "$check_dir/out")" "jail ok: retorna 0 sem imprimir nada no stdout"
assert_eq "" "$(file_of "$check_dir/err")" "jail ok: nada no stderr"
assert_eq "1" "$(jail_calls "$check_dir")" "jail ok: uma chamada ao ai-jail"
assert_eq "$(jail_options_of '')" "$(file_of "$check_dir/jail/call_1.argv")" "jail ok, JAIL_CLIPBOARD vazio: a verificação é 'ai-jail --no-save-config --no-status-bar --terminal-passthrough -- true' (as mesmas opções do lançamento)"
check_cwd="$(file_of "$check_dir/jail/call_1.cwd")"
assert_eq "true" "$([ -n "$check_cwd" ] && [ "$check_cwd" != "sem arquivo" ] && [ "$check_cwd" != "$check_dir/cwd" ] && echo true || echo false)" "jail ok: a verificação roda com o cwd fora do cwd do chamador"
assert_not_exists "$check_dir/cwd/.ai-jail" "jail ok: nenhum .ai-jail criado no cwd do chamador"
assert_eq "true" "$([ "$check_cwd" != "sem arquivo" ] && [ ! -e "$check_cwd" ] && echo true || echo false)" "jail ok: o diretório temporário da verificação foi removido"
assert_eq "" "$(find "$check_dir/cwd" -mindepth 1 2>/dev/null || true)" "jail ok: nada criado no cwd do chamador"
# Os hosts extras também entram na verificação: um ai-jail que recuse --allow-host falha aqui.
check_dir="$(new_case check_ok_hosts)"
run_jail "$check_dir" "jail_check true '' 'mcp.atlassian.com mcp.sentry.dev' '/proj/.nick.conf' false; echo \"rc=\$?\""
assert_eq "rc=0" "$(file_of "$check_dir/out")" "jail ok com hosts: retorna 0"
assert_eq "$(printf '%s\n' --no-save-config --no-status-bar --terminal-passthrough --allow-host mcp.atlassian.com --allow-host mcp.sentry.dev -- true)" "$(file_of "$check_dir/jail/call_1.argv")" "jail ok com hosts: a verificação usa os mesmos --allow-host do lançamento"
for check_clip in restrito brando; do
  check_dir="$(new_case "check_ok_$check_clip")"
  run_jail "$check_dir" "jail_check true '$check_clip' '' '/proj/.nick.conf' false; echo \"rc=\$?\""
  assert_eq "rc=0" "$(file_of "$check_dir/out")" "jail ok, JAIL_CLIPBOARD \"$check_clip\": retorna 0"
  assert_eq "$(jail_options_of "$check_clip")" "$(file_of "$check_dir/jail/call_1.argv")" "jail ok, JAIL_CLIPBOARD \"$check_clip\": a verificação usa as opções do nível"
done

echo
echo "== jail_check ligada, JAIL_CLIPBOARD inválido: retorna 1 antes de chamar o ai-jail, com o erro de valor =="
for check_clip in total RESTRITO " medio" $'brando\e[2J'; do
  check_dir="$(new_case "check_bad_clip_$(printf '%s' "$check_clip" | tr -c 'a-zA-Z' '_')")"
  printf -v check_clip_q '%q' "$check_clip"
  run_jail "$check_dir" "jail_check true $check_clip_q '' '/proj/um projeto/.nick.conf' false || echo \"rc=\$?\"; echo depois"
  check_err="$(file_of "$check_dir/err")"
  check_label="JAIL_CLIPBOARD $check_clip_q"
  assert_eq "rc=1"$'\n'"depois" "$(file_of "$check_dir/out")" "$check_label: retorna 1, sem sair do shell"
  assert_eq "0" "$(jail_calls "$check_dir")" "$check_label: o ai-jail não é chamado"
  assert_contains "$check_err" "Erro: JAIL_CLIPBOARD=\"${check_clip//[[:cntrl:]]/?}\" em /proj/um projeto/.nick.conf não é um valor aceito." "$check_label: o erro cita o valor (controles trocados por '?') e o arquivo"
  assert_contains "$check_err" 'Valores aceitos: "restrito", "medio", "brando" ou "" (vazio, que vale "medio").' "$check_label: o erro lista os valores aceitos"
  assert_not_contains "$check_err" "Requisitos da jail" "$check_label: não mostra a lista de requisitos (não é problema de instalação)"
done

echo
echo "== jail_check ligada, ai-jail ausente: retorna 1 (sem sair do shell) com a mensagem de erro =="
check_dir="$(new_case check_missing)"
run_jail "$check_dir" "JAIL_BIN='$MISSING_JAIL_BIN'; jail_check true '' '' '/proj/um projeto/.nick.conf' false || echo \"rc=\$?\"; echo depois"
check_err="$(file_of "$check_dir/err")"
assert_eq "rc=1"$'\n'"depois" "$(file_of "$check_dir/out")" "ai-jail ausente: retorna 1 e o shell continua (um comando depois da chamada roda)"
assert_contains "$check_err" 'Erro: JAIL_ENABLED="true" em /proj/um projeto/.nick.conf' "ai-jail ausente: a mensagem cita o caminho da configuração"
assert_contains "$check_err" "não encontrado" "ai-jail ausente: diz que o ai-jail não foi encontrado"
assert_contains "$check_err" 'JAIL_ENABLED="false" em /proj/um projeto/.nick.conf' "ai-jail ausente: oferece JAIL_ENABLED=\"false\" no mesmo arquivo"
assert_contains "$check_err" "[falta] ai-jail" "ai-jail ausente: termina com a lista de requisitos ([falta] ai-jail)"
assert_not_contains "$check_err" "Saída do ai-jail" "ai-jail ausente: sem bloco de saída do ai-jail"
assert_contains "$check_err" "  1. instale o que falta (lista abaixo);" "ai-jail ausente: a saída 1 manda instalar o que falta"
assert_not_contains "$check_err" "corrija o problema acima" "ai-jail ausente: não manda corrigir um problema que não foi mostrado"
assert_eq "0" "$(jail_calls "$check_dir")" "ai-jail ausente: o stub não é chamado"

echo
echo "== jail_check ligada, ai-jail falhando: retorna 1 com a saída do ai-jail na mensagem =="
check_dir="$(new_case check_fail)"
run_jail "$check_dir" "jail_check true '' '' '/proj/.nick.conf' false || echo \"rc=\$?\"" "export FAKE_JAIL_RC=1 FAKE_JAIL_STDERR='bwrap: setting up uid map: Permission denied'"
check_err="$(file_of "$check_dir/err")"
assert_eq "rc=1" "$(file_of "$check_dir/out")" "ai-jail falhando: retorna 1"
assert_contains "$check_err" "Saída do ai-jail:" "ai-jail falhando: a mensagem tem o bloco de saída"
assert_contains "$check_err" "    bwrap: setting up uid map: Permission denied" "ai-jail falhando: a saída do ai-jail aparece, indentada"
assert_contains "$check_err" 'JAIL_ENABLED="false" em /proj/.nick.conf' "ai-jail falhando: oferece JAIL_ENABLED=\"false\""
assert_contains "$check_err" "Requisitos da jail" "ai-jail falhando: termina com a lista de requisitos"
assert_not_contains "$check_err" "não encontrado" "ai-jail falhando: não diz que o ai-jail não foi encontrado"
# Com o ai-jail instalado, a causa costuma não ser falta de nada (user namespaces bloqueados,
# bwrap reprovado na validação de binário confiável): a lista pode vir toda [ok].
assert_contains "$check_err" '  1. corrija o problema acima (veja "Problemas comuns" na seção "Sandbox (ai-jail)"' "ai-jail falhando: a saída 1 manda corrigir o problema mostrado, com a seção do README"
assert_contains "$check_err" "     do README) ou instale o que falta (lista abaixo);" "ai-jail falhando: a saída 1 ainda cita a lista de requisitos"
assert_not_contains "$check_err" "  1. instale o que falta" "ai-jail falhando: a saída 1 não manda só instalar (a lista pode estar toda [ok])"
check_cwd="$(file_of "$check_dir/jail/call_1.cwd")"
assert_eq "true" "$([ "$check_cwd" != "sem arquivo" ] && [ ! -e "$check_cwd" ] && echo true || echo false)" "ai-jail falhando: o diretório temporário foi removido"
assert_not_exists "$check_dir/cwd/.ai-jail" "ai-jail falhando: nenhum .ai-jail no cwd do chamador"

echo
echo "== jail_check: caracteres de controle da saída viram '?', mantendo as quebras de linha =="
check_dir="$(new_case check_control)"
run_jail "$check_dir" "jail_check true '' '' '/proj/.nick.conf' false || :" "export FAKE_JAIL_RC=1 FAKE_JAIL_STDERR=\$'primeira \\e[2J linha\\nsegunda \\e[2J linha'"
check_err="$(file_of "$check_dir/err")"
assert_contains "$check_err" "    primeira ?[2J linha"$'\n'"    segunda ?[2J linha" "ESC vira '?' e cada linha da saída continua numa linha própria"
assert_not_contains "$check_err" $'\e' "nenhum ESC chega ao terminal"
check_dir="$(new_case check_control_c)"
run_jail "$check_dir" "jail_check true '' '' '/proj/.nick.conf' false || :" "export LC_ALL=C FAKE_JAIL_RC=1 FAKE_JAIL_STDERR='falhou: não há namespace'"
assert_contains "$(file_of "$check_dir/err")" "    falhou: não há namespace" "com LC_ALL=C, o 'não' da saída sai intacto"

echo
echo "== jail_check: aviso de arquivo compartilhado só com shared=\"true\" =="
check_dir="$(new_case check_shared)"
run_jail "$check_dir" "JAIL_BIN='$MISSING_JAIL_BIN'; jail_check true '' '' '/proj/.nick.conf' true || :"
assert_contains "$(file_of "$check_dir/err")" "Atenção: /proj/.nick.conf é compartilhado com o time do projeto" "shared=\"true\": aviso de arquivo compartilhado"
check_dir="$(new_case check_not_shared)"
run_jail "$check_dir" "JAIL_BIN='$MISSING_JAIL_BIN'; jail_check true '' '' '/nick/projects/alpha.conf' false || :"
assert_not_contains "$(file_of "$check_dir/err")" "compartilhado" "shared=\"false\": sem aviso de arquivo compartilhado"

echo
echo "== jail_check: não depende do set -e (chamada antes de ||, com mktemp falhando) =="
check_dir="$(new_case check_mktemp)"
run_jail "$check_dir" "jail_check true '' '' '/proj/.nick.conf' false || echo \"rc=\$?\"" "export TMPDIR='$TEST_TMP/nao-existe'"
assert_eq "rc=1" "$(file_of "$check_dir/out")" "mktemp falhando: retorna 1 em vez de seguir como se a jail funcionasse"
assert_contains "$(file_of "$check_dir/err")" 'JAIL_ENABLED="true" em /proj/.nick.conf' "mktemp falhando: mensagem de erro com o caminho"
assert_eq "0" "$(jail_calls "$check_dir")" "mktemp falhando: o ai-jail não é chamado"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
