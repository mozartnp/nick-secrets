#!/usr/bin/env bash
#
# test_jail: testes em bash puro (sem framework) da biblioteca lib/jail/jail.sh. Cada caso roda
# num bash -c separado, que dá source só em lib/jail/jail.sh — nunca em lib/config.sh, o que
# prova que as duas bibliotecas são independentes. O ai-jail real nunca é usado: os casos põem
# à frente do PATH stubs de ai-jail, claude, bwrap e uname, criados no TEST_TMP desta suíte, e
# o caso "ai-jail ausente" sobrescreve JAIL_BIN com um nome que não existe. A integração com o
# auto_scrum fica em auto_scrum/tests/test_auto_scrum.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JAIL_SH="$SCRIPT_DIR/../jail/jail.sh"
JAIL_HOSTS_TXT="$SCRIPT_DIR/../jail/allow_hosts.txt"
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

# Stub do ai-jail (diretório próprio, à frente do PATH). Com "--version" como 1º argumento,
# só grava uma linha em $FAKE_JAIL_LOG_DIR/version.calls, imprime
# "ai-jail ${FAKE_JAIL_VERSION-3.2.0}" (definida e vazia, só "ai-jail ") e sai com
# ${FAKE_JAIL_VERSION_RC:-0}, sem gravar call_<n>.argv (os jail_calls dos casos não mudam).
# Fora isso, cada chamada grava o argv (um
# argumento por linha) em $FAKE_JAIL_LOG_DIR/call_<n>.argv e o cwd em call_<n>.cwd, pula as
# opções antes do "--" e, sem --no-save-config entre elas, cria um .ai-jail no próprio cwd
# (simula o auto-save do ai-jail real). Se o comando depois do "--" for "true", é a
# verificação: imprime $FAKE_JAIL_STDERR no stderr (se definido) e sai com $FAKE_JAIL_RC
# (padrão 0). Senão, é o lançamento: faz exec do comando depois do "--".
FAKE_JAIL_DIR="$TEST_TMP/stub_jail"
mkdir -p "$FAKE_JAIL_DIR"
cat > "$FAKE_JAIL_DIR/ai-jail" <<EOF
#!$BASH_BIN
if [ "\${1:-}" = "--version" ]; then
  echo version >> "\$FAKE_JAIL_LOG_DIR/version.calls"
  printf 'ai-jail %s\n' "\${FAKE_JAIL_VERSION-3.2.0}"
  exit "\${FAKE_JAIL_VERSION_RC:-0}"
fi
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
  mkdir -p "$dir/jail" "$dir/claude" "$dir/cwd" "$dir/home"
  printf '%s' "$dir"
}

# run_jail <case_dir> <commands> [env]: num bash separado, com set -euo pipefail, cwd em
# <case_dir>/cwd e os stubs de ai-jail e claude à frente do PATH, roda [env] (atribuições,
# antes do source), dá source só em lib/jail/jail.sh e roda <commands>. stdout em
# <case_dir>/out, stderr em <case_dir>/err e o código de saída em <case_dir>/rc. O HOME é
# <case_dir>/home (vazio, a menos que o caso crie algo nele): as toolchains montadas pela jail
# dependem do home, e o resultado não pode depender da máquina que roda os testes. O
# '|| t_rc=$?' mantém a suíte rodando (e reportando FAIL) mesmo se o bash de dentro morrer.
run_jail() {
  local case_dir="$1" commands="$2" env="${3:-:}" t_rc=0
  timeout 5 bash -c "cd '$case_dir/cwd' || exit 99; unset PLAYWRIGHT_BROWSERS_PATH; export HOME='$case_dir/home' PATH='$FAKE_JAIL_DIR:$FAKE_CLAUDE_DIR':\"\$PATH\" FAKE_JAIL_LOG_DIR='$case_dir/jail' FAKE_CLAUDE_LOG_DIR='$case_dir/claude'; $env; set -euo pipefail; source '$JAIL_SH'; $commands" < /dev/null > "$case_dir/out" 2> "$case_dir/err" || t_rc=$?
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

# requirements <case_dir> <uname> <os_release_file> <ai_jail: ok|falta> <bwrap: ok|falta>
# [versão]: roda jail_print_requirements num bash com PATH só de stubs (uname sempre; bwrap só
# com "ok"; o ai-jail com "falta" vira o nome inexistente em JAIL_BIN), JAIL_OS_RELEASE_FILE
# em <os_release_file> e cwd em <case_dir>/cwd. Com [versão], o stub do ai-jail responde
# "ai-jail [versão]" ao --version (sem ela, o padrão do stub, 3.2.0); as chamadas de --version
# ficam em <case_dir>/jail/version.calls. stdout em <case_dir>/out, stderr em <case_dir>/err,
# código em <case_dir>/rc.
requirements() {
  local case_dir="$1" uname_value="$2" os_release="$3" jail_state="$4" bwrap_state="$5"
  local path="$FAKE_UNAME_DIR:$FAKE_JAIL_DIR" overrides="JAIL_OS_RELEASE_FILE='$os_release'" t_rc=0
  local version_env=":"
  [ "$#" -ge 6 ] && version_env="export FAKE_JAIL_VERSION='$6'"
  [ "$bwrap_state" = "ok" ] && path="$path:$FAKE_BWRAP_DIR"
  [ "$jail_state" = "falta" ] && overrides="$overrides; JAIL_BIN='$MISSING_JAIL_BIN'"
  timeout 5 "$BASH_BIN" -c "cd '$case_dir/cwd' || exit 99; PATH='$path'; export FAKE_UNAME='$uname_value' FAKE_JAIL_LOG_DIR='$case_dir/jail'; $version_env; set -euo pipefail; source '$JAIL_SH'; $overrides; jail_print_requirements" < /dev/null > "$case_dir/out" 2> "$case_dir/err" || t_rc=$?
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
CMD_JAIL_MAC_UPGRADE="brew upgrade ai-jail"
CMD_BWRAP_ARCH="sudo pacman -S bubblewrap"
CMD_BWRAP_DEBIAN="sudo apt install bubblewrap"
CMD_BWRAP_FEDORA="sudo dnf install bubblewrap"

# hosts_file_list <arquivo>: os hosts de um arquivo no formato de lib/jail/allow_hosts.txt (um
# por linha; linha em branco, comentário com # e espaços em volta ignorados), numa linha
# separados por espaço. Leitura independente da de jail_nick_hosts, pra o teste não depender
# dela.
hosts_file_list() {
  sed -e 's/#.*//' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' "$1" 2>/dev/null | grep . | paste -sd' ' -
}

# docs_opts: os --allow-host da lista padrão do nick (lib/jail/allow_hosts.txt), que
# jail_options sempre põe depois dos hosts do chamador. Lidos do próprio arquivo: um dev
# acrescenta um host sem mexer nos testes.
docs_opts=()
for docs_host in $(hosts_file_list "$JAIL_HOSTS_TXT"); do
  docs_opts+=(--allow-host "$docs_host")
done

# jail_base_opts: as opções que jail_options põe em todo nível, antes das do JAIL_CLIPBOARD —
# a fonte única desta suíte pra elas (cada caso monta o argv esperado a partir daqui).
jail_base_opts=(--no-save-config --no-status-bar --clean --agent-state --toolchains --no-github --no-ssh --no-aws --no-kube --no-gcloud --no-docker-config --no-docker --no-inherit-env)

# jail_options_of <nível>: o argv esperado da verificação 'ai-jail <opções> -- true' do nível.
jail_options_of() {
  case "$1" in
    restrito) printf '%s\n' "${jail_base_opts[@]}" "${docs_opts[@]}" -- true ;;
    brando) printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough --display "${docs_opts[@]}" -- true ;;
    *) printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough "${docs_opts[@]}" -- true ;;
  esac
}

echo "== carga limpa: source só define nomes jail_/JAIL_, sem imprimir nada =="
clean_dir="$TEST_TMP/clean"
mkdir -p "$clean_dir"
clean_rc=0
# Só redirecionamentos entre os retratos, pra o próprio teste não criar variável nenhuma.
timeout 2 bash --norc --noprofile -c "compgen -v | sort > '$clean_dir/vars_before'; declare -F | sort > '$clean_dir/funcs_before'; source '$JAIL_SH' > '$clean_dir/out' 2>&1; compgen -v | sort > '$clean_dir/vars_after'; declare -F | sort > '$clean_dir/funcs_after'" < /dev/null || clean_rc=$?
assert_eq "0" "$clean_rc" "source lib/jail/jail.sh num bash limpo termina com código 0"
assert_eq "" "$(cat "$clean_dir/out" 2>/dev/null || echo 'sem saída gravada')" "source lib/jail/jail.sh não imprime nada (stdout nem stderr)"
clean_new_vars="$(comm -13 "$clean_dir/vars_before" "$clean_dir/vars_after" 2>/dev/null | grep -vxE '_|BASH_.*|FUNCNAME|LINENO|PIPESTATUS' || true)"
clean_new_funcs="$(comm -13 "$clean_dir/funcs_before" "$clean_dir/funcs_after" 2>/dev/null | sed 's/^declare -f //' || true)"
assert_eq "JAIL_BIN"$'\n'"JAIL_HOSTS_FILE"$'\n'"JAIL_LANDLOCK"$'\n'"JAIL_OS_RELEASE_FILE"$'\n'"JAIL_VERSION_MIN" "$clean_new_vars" "as únicas variáveis novas são JAIL_BIN, JAIL_HOSTS_FILE, JAIL_LANDLOCK, JAIL_OS_RELEASE_FILE e JAIL_VERSION_MIN"
assert_eq "" "$(grep -vE '^jail_' <<< "$clean_new_funcs" | grep . || true)" "toda função nova começa com jail_"
for clean_func in jail_agent_notice jail_check jail_landlock_state jail_exec jail_options jail_playwright_browsers jail_print_requirements jail_print_status jail_toolchain_dirs jail_version_of jail_version_output jail_version_supported; do
  assert_contains "$clean_new_funcs" "$clean_func" "source lib/jail/jail.sh define $clean_func"
done
clean_values="$(timeout 2 bash --norc --noprofile -c "source '$JAIL_SH'; printf '%s|%s|%s|%s|%s' \"\$JAIL_BIN\" \"\$JAIL_OS_RELEASE_FILE\" \"\$JAIL_LANDLOCK\" \"\$JAIL_HOSTS_FILE\" \"\$JAIL_VERSION_MIN\"" 2>/dev/null || true)"
assert_eq "ai-jail|/etc/os-release||$(cd "$SCRIPT_DIR/../jail" && pwd)/allow_hosts.txt|3.2.0" "$clean_values" "JAIL_BIN é ai-jail (nome no PATH, sem caminho), JAIL_OS_RELEASE_FILE é /etc/os-release, JAIL_LANDLOCK começa vazia, JAIL_HOSTS_FILE é o allow_hosts.txt ao lado da biblioteca (caminho absoluto) e JAIL_VERSION_MIN é 3.2.0"
clean_values="$(cd "$SCRIPT_DIR/../jail" && timeout 2 bash --norc --noprofile -c "source ./jail.sh; cd /; printf '%s' \"\$JAIL_HOSTS_FILE\"" 2>/dev/null || true)"
assert_eq "$(cd "$SCRIPT_DIR/../jail" && pwd)/allow_hosts.txt" "$clean_values" "carregada por caminho relativo, JAIL_HOSTS_FILE continua absoluto (vale depois de um cd)"

echo
echo "== carga limpa: estrutura do arquivo (sem shebang, sem bit de execução, sem set/exit/source/eval) =="
assert_eq "# shellcheck shell=bash" "$(head -n1 "$JAIL_SH" 2>/dev/null || true)" "a 1ª linha é a diretiva '# shellcheck shell=bash' (sem shebang)"
assert_eq "sem bit de execução" "$([ -e "$JAIL_SH" ] && [ ! -x "$JAIL_SH" ] && echo 'sem bit de execução' || echo 'executável ou inexistente')" "lib/jail/jail.sh existe e não tem bit de execução"
jail_code="$(code_lines "$JAIL_SH" 2>/dev/null || true)"
assert_eq "" "$(grep -nE '(^|[;&|[:space:]])set[[:space:]]+[-+]' <<< "$jail_code" || true)" "nenhum set -e/set -o (nem outro set) fora de comentário"
assert_eq "" "$(grep -nwE 'exit|source|eval' <<< "$jail_code" || true)" "nenhum exit, source ou eval fora de comentário"
assert_eq "" "$(grep -nE '\[\[[[:space:]]' <<< "$jail_code" || true)" "nenhum teste com [[ ]] (a convenção do projeto é [ ])"
assert_eq "" "$(grep -nE '(^|[;&|])[[:space:]]*\.[[:space:]]+[^[:space:]]' <<< "$jail_code" || true)" "nenhum '. arquivo' fora de comentário"

echo
echo "== independência: lib/jail/jail.sh não depende de lib/config.sh =="
assert_eq "" "$(grep -nE 'config_|CONFIG_' <<< "$jail_code" || true)" "lib/jail/jail.sh não cita config_ nem CONFIG_ fora de comentário"
assert_eq "" "$(grep -nF 'config.sh' <<< "$jail_code" || true)" "lib/jail/jail.sh não cita config.sh fora de comentário"
# O padrão não casa com a própria linha: o [^;]* para no ';' do colchete, e a descrição não
# usa a palavra.
assert_eq "" "$(grep -nE 'source[^;]*config' <<< "$(code_lines "$0")" || true)" "esta suíte nunca carrega lib/config.sh"

echo
echo "== estrutural: as opções do ai-jail saem só de jail_options; o nick não mexe no .ai-jail =="
# Também as que abrem credencial ou autoridade do host (o contrário dos --no-* de
# jail_options), o --no-agent-state e o --no-network (que derruba a lista padrão de hosts).
for jail_forbidden in --bootstrap --init --rw-map --overlay-map --network --no-private-home --exec \
  --github --ssh --aws --kube --gcloud --docker-config --docker --inherit-env --no-agent-state --no-network; do
  assert_eq "" "$(grep -nF -- "$jail_forbidden" <<< "$jail_code" || true)" "lib/jail/jail.sh não cita $jail_forbidden fora de comentário"
done
# O grep -F acima não confunde a opção proibida com o --no-* dela (senão o laço nunca passaria).
for jail_forbidden in --github --ssh --docker --docker-config --inherit-env --network; do
  assert_eq "" "$(grep -F -- "$jail_forbidden" <<< "--no-${jail_forbidden#--}" || true)" "grep -F de $jail_forbidden não casa com --no-${jail_forbidden#--}"
done
# --allow-host (os hosts extras do chamador) só sai de jail_options, uma vez por host.
jail_options_body="$(awk '/^jail_options\(\) \{/{found=1} found{print} found && /^\}/{exit}' "$JAIL_SH" 2>/dev/null || true)"
# As opções de todo nível (jail_base_opts) aparecem uma vez em código, e só no corpo de
# jail_options.
for jail_base_opt in "${jail_base_opts[@]}"; do
  assert_eq "1" "$(grep -cE -- "(^|[[:space:]])$jail_base_opt([[:space:]]|\$)" <<< "$jail_code" || true)" "lib/jail/jail.sh cita $jail_base_opt uma vez fora de comentário"
  assert_eq "1" "$(grep -vE '^[[:space:]]*#' <<< "$jail_options_body" | grep -cE -- "(^|[[:space:]])$jail_base_opt([[:space:]]|\$)" || true)" "$jail_base_opt fica no corpo de jail_options"
done
assert_eq "1" "$(grep -cF -- --allow-host <<< "$jail_code" || true)" "lib/jail/jail.sh cita --allow-host uma vez fora de comentário"
# shellcheck disable=SC2016
assert_contains "$jail_options_body" '--allow-host "$host"' "o --allow-host fica no corpo de jail_options"
# --forward-port (as portas de localhost do chamador) só sai de jail_options.
assert_eq "1" "$(grep -cF -- --forward-port <<< "$jail_code" || true)" "lib/jail/jail.sh cita --forward-port uma vez fora de comentário"
# shellcheck disable=SC2016
assert_contains "$jail_options_body" '--forward-port "$port"' "o --forward-port fica no corpo de jail_options"
# --map (somente leitura; nunca --rw-map) só sai de jail_options, pras toolchains.
assert_eq "2" "$(grep -cE -- '(^|[[:space:]])--map([[:space:]]|$)' <<< "$jail_code" || true)" "lib/jail/jail.sh cita --map duas vezes fora de comentário (toolchains e navegadores do Playwright)"
assert_eq "1" "$(grep -cE -- '(^|[[:space:]])--env([[:space:]]|$)' <<< "$jail_code" || true)" "lib/jail/jail.sh cita --env uma vez fora de comentário (PLAYWRIGHT_BROWSERS_PATH)"
# shellcheck disable=SC2016
assert_contains "$jail_options_body" '--env "PLAYWRIGHT_BROWSERS_PATH=$' "o --env fica no corpo de jail_options"
# shellcheck disable=SC2016
assert_contains "$jail_options_body" '--map "$dir"' "o --map fica no corpo de jail_options, com as toolchains de jail_toolchain_dirs"
assert_eq "" "$(grep -nE '>[[:space:]]*[^[:space:]]*\.ai-jail' <<< "$jail_code" || true)" "lib/jail/jail.sh não escreve em .ai-jail"
assert_eq "" "$(grep -nE '(^|[^-])ai-jail[^[:space:]]* --[[:space:]]|exec[[:space:]]+ai-jail' <<< "$jail_code" || true)" "lib/jail/jail.sh só chama o ai-jail por \$JAIL_BIN (nunca pelo nome literal)"
# Toda chamada de "$JAIL_BIN" (fora o command -v) é '"$JAIL_BIN" "${options[@]}" -- ...' (a
# verificação e o lançamento) ou, uma única vez, '"$JAIL_BIN" --version 2>/dev/null'
# (jail_version_output, que não abre sandbox); e options só vem de jail_options. O $JAIL_BIN,
# o ${options[@]} e o $clipboard entre aspas simples são o texto literal procurado no código.
# shellcheck disable=SC2016
jail_bin_calls="$(grep -F '"$JAIL_BIN"' <<< "$jail_code" | grep -vF 'command -v "$JAIL_BIN"' || true)"
assert_eq "3" "$(grep -c . <<< "$jail_bin_calls" || true)" "lib/jail/jail.sh chama \$JAIL_BIN em três lugares (verificação, lançamento e --version)"
# shellcheck disable=SC2016
assert_eq "1" "$(grep -cxE '[[:space:]]*"\$JAIL_BIN" --version 2>/dev/null' <<< "$jail_bin_calls" || true)" "uma chamada de \$JAIL_BIN é exatamente '\$JAIL_BIN --version 2>/dev/null'"
# shellcheck disable=SC2016
assert_eq "" "$(grep -vF '"$JAIL_BIN" "${options[@]}" -- ' <<< "$jail_bin_calls" | grep -vxE '[[:space:]]*"\$JAIL_BIN" --version 2>/dev/null' || true)" "fora o --version, toda chamada de \$JAIL_BIN é '\$JAIL_BIN \"\${options[@]}\" -- ...'"
# shellcheck disable=SC2016
jail_version_body="$(awk '/^jail_version_output\(\) \{/{found=1} found{print} found && /^\}/{exit}' "$JAIL_SH" 2>/dev/null || true)"
# shellcheck disable=SC2016
assert_contains "$jail_version_body" '"$JAIL_BIN" --version 2>/dev/null' "o --version fica no corpo de jail_version_output"
# shellcheck disable=SC2016
assert_eq "2" "$(grep -cF 'mapfile -t options < <(jail_options "$clipboard" "$hosts" "$ports")' <<< "$jail_code" || true)" "a verificação e o lançamento montam options com jail_options"
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

# Com JAIL_LANDLOCK preenchida (por jail_check), o estado entra entre o "ativa" e o modo.
for status_landlock in completo parcial inativo; do
  status_dir="$(new_case "status_landlock_$status_landlock")"
  run_jail "$status_dir" "JAIL_LANDLOCK='$status_landlock'; jail_print_status true ''"
  assert_eq "⚠️ Jail (ai-jail): ativa | Landlock: $status_landlock | modo de clipboard médio" "$(file_of "$status_dir/out")" "JAIL_LANDLOCK=\"$status_landlock\": a linha mostra 'Landlock: $status_landlock'"
done
status_dir="$(new_case status_landlock_off)"
run_jail "$status_dir" "JAIL_LANDLOCK=completo; jail_print_status false ''"
assert_eq "$status_off" "$(file_of "$status_dir/out")" "jail desligada: a linha não fala de Landlock"

echo
echo "== jail_landlock_state: o estado do Landlock na saída do ai-jail, resumido =="
for landlock_case in "fully enforced|completo" "partially enforced|parcial" "partially enforced (ABI 3)|parcial" "not enforced|inativo" "not enforced (kernel sem suporte)|inativo" "algo novo|não identificado"; do
  landlock_text="${landlock_case%%|*}"
  landlock_expected="${landlock_case#*|}"
  landlock_dir="$(new_case landlock_state)"
  run_jail "$landlock_dir" "jail_landlock_state \$'▸ Jail Active: /proj\\n▸ Landlock: $landlock_text'"
  assert_eq "$landlock_expected" "$(file_of "$landlock_dir/out")" "'▸ Landlock: $landlock_text' vira \"$landlock_expected\""
done
landlock_dir="$(new_case landlock_state_sem_linha)"
run_jail "$landlock_dir" "jail_landlock_state '▸ Jail Active: /proj'"
assert_eq "inativo" "$(file_of "$landlock_dir/out")" "sem a linha do Landlock (--no-landlock): \"inativo\""
landlock_dir="$(new_case landlock_state_outra)"
run_jail "$landlock_dir" "jail_landlock_state \$'▸ Landlock: ro map /x\\n▸ Landlock: fully enforced'"
assert_eq "completo" "$(file_of "$landlock_dir/out")" "linhas de detalhe ('Landlock: ro map ...') não confundem: vale a do estado"
# Saída real do ai-jail 3.2.0 na verificação (2026-11-08): a linha "⚠ Mask: ..." não tem
# "Landlock: " e é ignorada.
landlock_dir="$(new_case landlock_state_320)"
run_jail "$landlock_dir" "jail_landlock_state \$'▸ Jail Active: /tmp/x\\n⚠ Mask: /tmp/x/.ai-jail does not exist; rule not applied\\n▸ Landlock: fully enforced'"
assert_eq "completo" "$(file_of "$landlock_dir/out")" "saída do ai-jail 3.2.0 (com '⚠ Mask: ... rule not applied'): \"completo\""

echo
echo "== jail_version_of / jail_version_supported: a versão da saída de 'ai-jail --version' e a faixa suportada =="
# Cada caso: <saída de --version>|<jail_version_of esperado>|<rc esperado de jail_version_supported>.
# A comparação é numérica (3.10 > 3.2); a faixa é >= JAIL_VERSION_MIN e < a próxima major.
version_case_n=0
for version_case in "ai-jail 3.2.0|3.2.0|0" "ai-jail 3.2.1|3.2.1|0" "ai-jail 3.10.0|3.10.0|0" "ai-jail 3.2.10|3.2.10|0" $'ai-jail 3.2.0\n|3.2.0|0' \
  "ai-jail 2.14.0|2.14.0|1" "ai-jail 3.1.9|3.1.9|1" "ai-jail 3.0.1|3.0.1|1" "ai-jail 4.0.0|4.0.0|1" "ai-jail 10.0.0|10.0.0|1" \
  "||1" "ai-jail||1" "ai-jail ||1" "ai-jail 3.x||1" "ai-jail 03.2.0||1" "ai-jail 3.02.0||1" "ai-jail 3.2||1" "ai-jail 3.2.0.1||1" \
  "3.2.0||1" "outro 3.2.0||1" "ai-jail 3.2.0-rc1||1" "ai-jail 99999999999.0.0||1" $'ai-jail 3.2.0\e[2J||1' $'ai-jail 3.2.0\nai-jail 3.2.0||1'; do
  version_case_n=$((version_case_n + 1))
  version_text="${version_case%%|*}"
  version_rest="${version_case#*|}"
  version_expected="${version_rest%%|*}"
  version_rc_expected="${version_rest#*|}"
  printf -v version_q '%q' "$version_text"
  version_dir="$(new_case "version_$version_case_n")"
  run_jail "$version_dir" "jail_version_of $version_q; jail_version_supported $version_q && echo rc=0 || echo rc=\$?"
  assert_eq "$version_expected"$'\n'"rc=$version_rc_expected" "$(file_of "$version_dir/out")" "saída $version_q: jail_version_of \"$version_expected\", jail_version_supported retorna $version_rc_expected"
  assert_eq "" "$(file_of "$version_dir/err")" "saída $version_q: nada no stderr"
done
# Nunca roda o ai-jail: são funções puras sobre o texto.
assert_eq "sem arquivo" "$(file_of "$version_dir/jail/version.calls")" "jail_version_of e jail_version_supported não chamam o ai-jail"

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
    restrito) opt_expected=("${jail_base_opts[@]}") ;;
    brando) opt_expected=("${jail_base_opts[@]}" --terminal-passthrough --display) ;;
    *) opt_expected=("${jail_base_opts[@]}" --terminal-passthrough) ;;
  esac
  opt_expected+=("${docs_opts[@]}")
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
  opt_expected=("${jail_base_opts[@]}" --terminal-passthrough --display)
  read -ra opt_host_list <<< "$opt_hosts"
  for opt_host in "${opt_host_list[@]}"; do
    opt_expected+=(--allow-host "$opt_host")
  done
  opt_expected+=("${docs_opts[@]}")
  assert_eq "$(printf '%s\n' "${opt_expected[@]}")" "$(file_of "$opt_dir/out")" "jail_options brando \"$opt_hosts\": ${opt_expected[*]}"
done

# Portas (separadas por espaço): um --forward-port por porta, depois dos hosts.
opt_dir="$(new_case options_ports)"
run_jail "$opt_dir" "jail_options restrito 'mcp.sentry.dev' '  54320   6379 '"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --allow-host mcp.sentry.dev "${docs_opts[@]}" --forward-port 54320 --forward-port 6379)" "$(file_of "$opt_dir/out")" "jail_options com portas: um --forward-port por porta, na ordem, depois do --allow-host"
opt_dir="$(new_case options_ports_vazio)"
run_jail "$opt_dir" "jail_options restrito '' ' '"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" "${docs_opts[@]}")" "$(file_of "$opt_dir/out")" "jail_options com portas só espaços: nenhum --forward-port"

echo
echo "== jail_toolchain_dirs: os gerenciadores de versão do home que existem, na ordem da lista =="
tc_dir="$(new_case toolchains_vazio)"
run_jail "$tc_dir" "jail_toolchain_dirs"
assert_eq "0||" "$(file_of "$tc_dir/rc")|$(file_of "$tc_dir/out")|$(file_of "$tc_dir/err")" "home sem toolchains: nada impresso, código 0"
tc_dir="$(new_case toolchains_todas)"
tc_all=(.pyenv .nvm .rbenv .asdf .local/share/mise .local/share/uv/python .sdkman .volta)
for tc_name in "${tc_all[@]}"; do mkdir -p "$tc_dir/home/$tc_name"; done
run_jail "$tc_dir" "jail_toolchain_dirs"
assert_eq "$(printf "$tc_dir/home/%s\n" "${tc_all[@]}")" "$(file_of "$tc_dir/out")" "todas existem: as oito, caminho absoluto, na ordem da lista"
tc_dir="$(new_case toolchains_algumas)"
mkdir -p "$tc_dir/home/.pyenv/versions" "$tc_dir/home/.local/share/uv/python" "$tc_dir/home/.local/share/uv/credentials" "$tc_dir/home/.ssh" "$tc_dir/home/.config"
: > "$tc_dir/home/.nvm"
run_jail "$tc_dir" "jail_toolchain_dirs"
assert_eq "$tc_dir/home/.pyenv"$'\n'"$tc_dir/home/.local/share/uv/python" "$(file_of "$tc_dir/out")" "só os diretórios da lista que existem (do uv, só os interpretadores: nunca ~/.local/share/uv, onde o 'uv auth login' guarda credenciais) (um arquivo .nvm não conta; .ssh e .config nunca)"
tc_dir="$(new_case toolchains_sem_home)"
run_jail "$tc_dir" "jail_toolchain_dirs" "unset HOME"
assert_eq "0|" "$(file_of "$tc_dir/rc")|$(file_of "$tc_dir/out")" "sem HOME: nada impresso (nunca monta /.pyenv), código 0"

echo
echo "== jail_playwright_browsers: o diretório dos navegadores do Playwright, se existir =="
pw_dir="$(new_case pw_sem)"
run_jail "$pw_dir" "jail_playwright_browsers"
assert_eq "0||" "$(file_of "$pw_dir/rc")|$(file_of "$pw_dir/out")|$(file_of "$pw_dir/err")" "sem ~/.cache/ms-playwright: nada impresso, código 0"
pw_dir="$(new_case pw_padrao)"
mkdir -p "$pw_dir/home/.cache/ms-playwright"
run_jail "$pw_dir" "jail_playwright_browsers"
assert_eq "$pw_dir/home/.cache/ms-playwright" "$(file_of "$pw_dir/out")" "com ~/.cache/ms-playwright: imprime o caminho padrão do Linux"
pw_dir="$(new_case pw_env)"
mkdir -p "$pw_dir/home/.cache/ms-playwright" "$pw_dir/outro/navegadores"
run_jail "$pw_dir" "jail_playwright_browsers" "export PLAYWRIGHT_BROWSERS_PATH='$pw_dir/outro/navegadores'"
assert_eq "$pw_dir/outro/navegadores" "$(file_of "$pw_dir/out")" "PLAYWRIGHT_BROWSERS_PATH definida: vale ela, e não o padrão"
for pw_value in 0 "$TEST_TMP/nao-existe-pw" "$TEST_TMP/com:dois-pontos" pw-relativo; do
  pw_dir="$(new_case pw_ignorado)"
  mkdir -p "$pw_dir/home/.cache/ms-playwright" "$TEST_TMP/com:dois-pontos" "$pw_dir/cwd/pw-relativo"
  run_jail "$pw_dir" "jail_playwright_browsers" "export PLAYWRIGHT_BROWSERS_PATH='$pw_value'"
  assert_eq "0|" "$(file_of "$pw_dir/rc")|$(file_of "$pw_dir/out")" "PLAYWRIGHT_BROWSERS_PATH=\"$pw_value\": nada (0 = navegadores no projeto; inexistente; ':' quebraria o --map ORIGEM:DESTINO; relativo seria ambíguo e já fica no projeto)"
done
pw_dir="$(new_case pw_sem_home)"
run_jail "$pw_dir" "jail_playwright_browsers" "unset HOME"
assert_eq "0|" "$(file_of "$pw_dir/rc")|$(file_of "$pw_dir/out")" "sem HOME e sem PLAYWRIGHT_BROWSERS_PATH: nada, código 0"

echo
echo "== jail_options: os navegadores do Playwright montados fora do ~/.cache, com PLAYWRIGHT_BROWSERS_PATH =="
# Montar em ~/.cache/ms-playwright faria o ai-jail descartar o ~/.cache gravável que ele
# mesmo monta (conferido com --dry-run): o destino é ~/.ms-playwright.
opt_dir="$(new_case options_playwright)"
mkdir -p "$opt_dir/home/.pyenv" "$opt_dir/home/.cache/ms-playwright"
run_jail "$opt_dir" "jail_options restrito 'mcp.sentry.dev'"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --map "$opt_dir/home/.pyenv" --map "$opt_dir/home/.cache/ms-playwright:$opt_dir/home/.ms-playwright" --env "PLAYWRIGHT_BROWSERS_PATH=$opt_dir/home/.ms-playwright" --allow-host mcp.sentry.dev "${docs_opts[@]}")" "$(file_of "$opt_dir/out")" "jail_options com Playwright: --map ORIGEM:~/.ms-playwright e --env PLAYWRIGHT_BROWSERS_PATH, depois das toolchains e antes dos hosts"

echo
echo "== jail_options: as toolchains entram como --map (somente leitura), antes dos hosts =="
opt_dir="$(new_case options_toolchains)"
mkdir -p "$opt_dir/home/.pyenv" "$opt_dir/home/.nvm"
run_jail "$opt_dir" "jail_options restrito 'mcp.sentry.dev'"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --map "$opt_dir/home/.pyenv" --map "$opt_dir/home/.nvm" --allow-host mcp.sentry.dev "${docs_opts[@]}")" "$(file_of "$opt_dir/out")" "jail_options com .pyenv e .nvm no home: um --map por toolchain, depois o --allow-host"

echo
echo "== jail_agent_notice: o aviso ao agente de que ele roda dentro da jail =="
notice_dir="$(new_case notice_vazio)"
run_jail "$notice_dir" "jail_agent_notice ''"
notice_text="$(file_of "$notice_dir/out")"
assert_eq "0|" "$(file_of "$notice_dir/rc")|$(file_of "$notice_dir/err")" "jail_agent_notice: código 0 e nada no stderr"
# O '~/.ssh' é texto do aviso, não caminho a expandir.
# shellcheck disable=SC2088
for notice_word in "sandbox ai-jail" "nick" 'JAIL_ENABLED="true"' "só o diretório do projeto aceita escrita" "somente leitura" "o home é privado" '~/.ssh' "não existem aqui, mesmo existindo na máquina" "API do Claude" "registries de pacotes" "GitHub" "localhost" "não consegue ver nem conferir nada fora da jail" "a primeira hipótese é a jail, não a máquina" "não conclua que algo foi removido" "avise o usuário"; do
  assert_contains "$notice_text" "$notice_word" "o aviso cita: $notice_word"
done
# Credenciais: sem token do GitHub e sem SSH; o git local funciona; gh e push ficam com o
# usuário, fora da jail — antes da linha "não consegue ver nem conferir".
notice_github="$(grep -F 'token do GitHub' <<< "$notice_text" || true)"
for notice_word in "token do GitHub" "SSH" "não estão disponíveis" "git local" "funciona" "gh" "push" "o usuário" "fora da jail"; do
  assert_contains "$notice_github" "$notice_word" "a linha das credenciais cita: $notice_word"
done
assert_eq "true" "$(n1="$(grep -nF 'token do GitHub' <<< "$notice_text" | cut -d: -f1)"; n2="$(grep -nF 'não consegue ver nem conferir' <<< "$notice_text" | cut -d: -f1)"; [ -n "$n1" ] && [ -n "$n2" ] && [ "$n1" -lt "$n2" ] && echo true || echo false)" "a linha das credenciais vem antes de 'você não consegue ver nem conferir nada fora da jail'"
# O --toolchains do ai-jail também monta coisas no home (caches graváveis, ~/.cargo/bin e
# ~/.rustup somente leitura): o aviso cita isso, pra "não existem aqui" não ficar falso.
# shellcheck disable=SC2088 # o "~/" é texto do aviso, não caminho a expandir
for notice_word in "caches de dependência" "graváveis" "~/.cargo/bin" "~/.rustup"; do
  assert_contains "$(grep -F 'o home é privado' <<< "$notice_text" || true)" "$notice_word" "a linha do home cita o que o --toolchains do ai-jail monta: $notice_word"
done
# shellcheck disable=SC2088 # o "~/" é texto do aviso, não caminho a expandir
assert_not_contains "$notice_text" "~/.local etc." "o aviso não diz que ~/.local inteiro não existe (o ai-jail e o nick montam partes dele)"
assert_not_contains "$notice_text" "gerenciadores de versão" "sem toolchain no home: o aviso não lista gerenciadores de versão"
assert_not_contains "$notice_text" "e também:" "sem hosts extras: o aviso não lista hosts extras"
assert_eq "" "$(LC_ALL=C grep -n '[[:cntrl:]]' <<< "$notice_text" | grep -v $'\t' || true)" "o aviso não tem caracteres de controle"
notice_dir="$(new_case notice_completo)"
mkdir -p "$notice_dir/home/.pyenv" "$notice_dir/home/.local/share/uv/python"
run_jail "$notice_dir" "jail_agent_notice 'mcp.atlassian.com mcp.sentry.dev'"
notice_text="$(file_of "$notice_dir/out")"
assert_contains "$notice_text" "gerenciadores de versão desta máquina (~/.pyenv, ~/.local/share/uv/python)" "com toolchains: o aviso lista as montadas, com ~ no lugar do home"
assert_contains "$notice_text" "e também: mcp.atlassian.com, mcp.sentry.dev" "com hosts: o aviso lista os hosts extras liberados"
assert_not_contains "$notice_text" "portas liberadas" "sem portas: o aviso não lista portas"
assert_not_contains "$notice_text" "Playwright" "sem os navegadores do Playwright: o aviso não fala deles"
notice_dir="$(new_case notice_playwright)"
mkdir -p "$notice_dir/home/.cache/ms-playwright"
run_jail "$notice_dir" "jail_agent_notice '' ''"
notice_text="$(file_of "$notice_dir/out")"
assert_contains "$notice_text" "os navegadores do Playwright (PLAYWRIGHT_BROWSERS_PATH=~/.ms-playwright)" "com Playwright: o aviso diz onde estão os navegadores"
assert_contains "$notice_text" "--no-sandbox" "com Playwright: o aviso diz que o sandbox do Chromium não funciona na jail"
notice_dir="$(new_case notice_ports)"
run_jail "$notice_dir" "jail_agent_notice '' '54320 6379'"
notice_text="$(file_of "$notice_dir/out")"
assert_contains "$notice_text" "dos serviços da máquina em localhost, só as portas liberadas pelo nick (54320, 6379) são alcançáveis" "com portas: o aviso lista as portas de localhost liberadas"
# A lista padrão do nick sempre aparece no aviso; o resto da web, não, e o agente sabe o que
# dizer ao usuário (JAIL_ALLOW_HOSTS).
notice_nick_hosts="$(hosts_file_list "$JAIL_HOSTS_TXT")"
assert_contains "$notice_text" "os hosts liberados pelo nick (${notice_nick_hosts// /, })" "o aviso cita os hosts da lista padrão do nick"
assert_contains "$notice_text" "proxy refused the connection" "o aviso cita o erro que a jail dá num host fora da lista"
assert_contains "$notice_text" "JAIL_ALLOW_HOSTS" "o aviso diz ao agente que o host pode ser liberado em JAIL_ALLOW_HOSTS"

echo
echo "== lib/jail/allow_hosts.txt: a lista padrão do nick, editável à mão =="
assert_exists "$JAIL_HOSTS_TXT" "lib/jail/allow_hosts.txt existe"
assert_eq "" "$([ -x "$JAIL_HOSTS_TXT" ] && echo executável || true)" "lib/jail/allow_hosts.txt não é executável (é dado)"
# O problema que criou a lista: a documentação do Claude.
for docs_host in docs.claude.com code.claude.com docs.anthropic.com; do
  assert_contains " $(hosts_file_list "$JAIL_HOSTS_TXT") " " $docs_host " "a lista padrão tem $docs_host"
done
# Um host mal escrito no arquivo faria a jail parar em todo projeto: a lista real passa na
# mesma validação dos hosts do chamador.
docs_dir="$(new_case nick_hosts_real)"
run_jail "$docs_dir" "jail_check true '' '' '' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
assert_eq "rc=0|" "$(file_of "$docs_dir/out")|$(file_of "$docs_dir/err")" "todo host de lib/jail/allow_hosts.txt passa na validação"
assert_contains "$(head -n5 "$JAIL_HOSTS_TXT" 2>/dev/null)" "Um por linha" "o cabeçalho do arquivo explica o formato"

echo
echo "== jail_nick_hosts: lê JAIL_HOSTS_FILE (um por linha, comentários e linhas em branco ignorados) =="
docs_dir="$(new_case nick_hosts)"
run_jail "$docs_dir" "jail_nick_hosts"
assert_eq "$(hosts_file_list "$JAIL_HOSTS_TXT")" "$(file_of "$docs_dir/out")" "jail_nick_hosts: os hosts de lib/jail/allow_hosts.txt, numa linha, separados por espaço"
assert_eq "0|" "$(file_of "$docs_dir/rc")|$(file_of "$docs_dir/err")" "jail_nick_hosts: código 0 e nada no stderr"
docs_dir="$(new_case nick_hosts_fake)"
printf '%s\n' '# cabeçalho' '' '  a.io  ' 'b.io # comentário no fim' '	c.io' '#d.io' > "$docs_dir/hosts.txt"
printf 'e.io' >> "$docs_dir/hosts.txt"
run_jail "$docs_dir" "JAIL_HOSTS_FILE='$docs_dir/hosts.txt'; jail_nick_hosts"
assert_eq "a.io b.io c.io e.io" "$(file_of "$docs_dir/out")" "jail_nick_hosts: ignora comentário, linha em branco e espaços; lê a última linha sem \\n"
docs_dir="$(new_case nick_hosts_missing)"
run_jail "$docs_dir" "JAIL_HOSTS_FILE='$docs_dir/nao-existe.txt'; jail_nick_hosts; echo \"rc=\$?\""
assert_eq "rc=0|" "$(file_of "$docs_dir/out" | grep -v '^$')|$(file_of "$docs_dir/err")" "jail_nick_hosts sem o arquivo: nenhum host, código 0, nada no stderr"
docs_dir="$(new_case nick_hosts_options)"
printf '%s\n' 'a.io' 'b.io' > "$docs_dir/hosts.txt"
run_jail "$docs_dir" "JAIL_HOSTS_FILE='$docs_dir/hosts.txt'; jail_options restrito 'mcp.sentry.dev' '54320'"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --allow-host mcp.sentry.dev --allow-host a.io --allow-host b.io --forward-port 54320)" "$(file_of "$docs_dir/out")" "jail_options: os hosts do arquivo vêm depois dos do chamador e antes das portas"
# Host inválido no arquivo: para antes do ai-jail, com o arquivo no erro (não é culpa do
# projeto).
docs_dir="$(new_case nick_hosts_bad)"
printf '%s\n' 'a.io' 'https://b.io' > "$docs_dir/hosts.txt"
run_jail "$docs_dir" "JAIL_HOSTS_FILE='$docs_dir/hosts.txt'; jail_check true '' 'x.io' '' '/proj/.nick/nick.conf' false || echo \"rc=\$?\""
assert_eq "rc=1" "$(file_of "$docs_dir/out")" "host inválido em JAIL_HOSTS_FILE: jail_check retorna 1"
assert_eq "0" "$(jail_calls "$docs_dir")" "host inválido em JAIL_HOSTS_FILE: o ai-jail não é chamado"
assert_not_exists "$docs_dir/jail/version.calls" "host inválido em JAIL_HOSTS_FILE: para antes de 'ai-jail --version'"
assert_contains "$(file_of "$docs_dir/err")" "Erro: $docs_dir/hosts.txt (lista padrão do nick) tem um host inválido: \"https://b.io\"." "host inválido em JAIL_HOSTS_FILE: o erro cita o arquivo e o host"
assert_not_contains "$(file_of "$docs_dir/err")" "JAIL_ALLOW_HOSTS em" "host inválido em JAIL_HOSTS_FILE: o erro não culpa a configuração do projeto"


echo
echo "== jail_exec: desligada lança o comando direto; ligada lança 'ai-jail <opções> -- <comando>' =="
# Argumentos difíceis: espaço, aspas, começando com '-', vazio, glob e várias linhas.
exec_args=(--permission-mode auto "com espaço" "aspas \"duplas\" e 'simples'" "-x" "--" "" "*" $'linha 1\nlinha 2')
printf -v exec_call '%q ' "${exec_args[@]}"
exec_expected="$(printf '%s\n' "${exec_args[@]}")"
for exec_value in "" false TRUE; do
  exec_dir="$(new_case "exec_off_${exec_value:-vazio}")"
  run_jail "$exec_dir" "jail_exec '$exec_value' brando '' '' claude $exec_call; echo 'não deveria chegar aqui'"
  assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"$exec_value\": o claude recebe exatamente o argv passado"
  assert_eq "0" "$(jail_calls "$exec_dir")" "jail_exec \"$exec_value\": o ai-jail não é chamado"
  assert_not_contains "$(file_of "$exec_dir/out")" "não deveria chegar aqui" "jail_exec \"$exec_value\": faz exec (não retorna)"
done
exec_dir="$(new_case exec_on)"
run_jail "$exec_dir" "jail_exec true '' '' '' claude $exec_call; echo 'não deveria chegar aqui'"
assert_eq "1" "$(jail_calls "$exec_dir")" "jail_exec \"true\": o ai-jail é chamado uma vez"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough "${docs_opts[@]}" -- claude "${exec_args[@]}")" "$(file_of "$exec_dir/jail/call_1.argv")" "jail_exec \"true\" \"\": o ai-jail recebe as opções base, '--terminal-passthrough', '--', 'claude' e o argv intacto"
assert_not_exists "$exec_dir/cwd/.ai-jail" "jail_exec \"true\": o lançamento não deixa .ai-jail no cwd (o prompt não fica gravado no projeto)"
assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"true\": o claude recebe o mesmo argv do caso desligado"
assert_eq "$exec_dir/cwd" "$(file_of "$exec_dir/jail/call_1.cwd")" "jail_exec \"true\": o ai-jail roda no cwd do chamador"
assert_not_contains "$(file_of "$exec_dir/out")" "não deveria chegar aqui" "jail_exec \"true\": faz exec (não retorna)"
# Cada nível lança com as opções de jail_options.
for exec_clip in medio restrito brando; do
  exec_dir="$(new_case "exec_on_$exec_clip")"
  run_jail "$exec_dir" "jail_exec true '$exec_clip' '' '' claude $exec_call; echo 'não deveria chegar aqui'"
  case "$exec_clip" in
    restrito) exec_options=("${jail_base_opts[@]}") ;;
    brando) exec_options=("${jail_base_opts[@]}" --terminal-passthrough --display) ;;
    *) exec_options=("${jail_base_opts[@]}" --terminal-passthrough) ;;
  esac
  exec_options+=("${docs_opts[@]}")
  assert_eq "$(printf '%s\n' "${exec_options[@]}" -- claude "${exec_args[@]}")" "$(file_of "$exec_dir/jail/call_1.argv")" "jail_exec \"true\" \"$exec_clip\": opções ${exec_options[*]}"
  assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"true\" \"$exec_clip\": o claude recebe o argv intacto"
done
# As portas viram --forward-port antes do "--".
exec_dir="$(new_case exec_on_ports)"
run_jail "$exec_dir" "jail_exec true '' '' '54320 6379' claude $exec_call; echo 'não deveria chegar aqui'"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough "${docs_opts[@]}" --forward-port 54320 --forward-port 6379 -- claude "${exec_args[@]}")" "$(file_of "$exec_dir/jail/call_1.argv")" "jail_exec \"true\" com portas: um --forward-port por porta, antes do '--'"
# As toolchains do home viram --map; com a jail desligada, não mudam nada.
exec_dir="$(new_case exec_on_toolchains)"
mkdir -p "$exec_dir/home/.pyenv"
run_jail "$exec_dir" "jail_exec true '' '' '' claude $exec_call; echo 'não deveria chegar aqui'"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough --map "$exec_dir/home/.pyenv" "${docs_opts[@]}" -- claude "${exec_args[@]}")" "$(file_of "$exec_dir/jail/call_1.argv")" "jail_exec \"true\" com ~/.pyenv: --map da toolchain antes do '--'"
# Os hosts extras viram --allow-host antes do "--"; com a jail desligada, não mudam nada.
exec_dir="$(new_case exec_on_hosts)"
run_jail "$exec_dir" "jail_exec true '' 'mcp.atlassian.com mcp.sentry.dev' '' claude $exec_call; echo 'não deveria chegar aqui'"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough --allow-host mcp.atlassian.com --allow-host mcp.sentry.dev "${docs_opts[@]}" -- claude "${exec_args[@]}")" "$(file_of "$exec_dir/jail/call_1.argv")" "jail_exec \"true\" com hosts: um --allow-host por host, antes do '--'"
assert_eq "$exec_expected" "$(file_of "$exec_dir/claude/argv")" "jail_exec \"true\" com hosts: o claude recebe o argv intacto"
exec_dir="$(new_case exec_off_hosts)"
run_jail "$exec_dir" "jail_exec false '' 'mcp.atlassian.com mcp.sentry.dev' '' claude $exec_call; echo 'não deveria chegar aqui'"
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
echo "== jail_print_requirements: a versão suportada e a instalada ([ok] <versão> / [atualizar]) =="
req_supported="Versão suportada do ai-jail: 3.2.0 ou mais nova da série 3.x."
req_dir="$(new_case req_version_ok)"
requirements "$req_dir" Linux "$OS_DIR/arch" ok ok
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "  [ok]    ai-jail 3.2.0"$'\n' "versão suportada: [ok] com a versão instalada"
assert_contains "$req_out" "$req_supported" "versão suportada: a linha da versão suportada aparece"
assert_not_contains "$req_out" "[atualizar]" "versão suportada: nenhum [atualizar]"
assert_eq "version" "$(file_of "$req_dir/jail/version.calls")" "com o ai-jail no PATH: roda 'ai-jail --version' uma vez"
assert_eq "0|" "$(file_of "$req_dir/rc")|$(file_of "$req_dir/err")" "versão suportada: código 0 e nada no stderr"
req_dir="$(new_case req_version_310)"
requirements "$req_dir" Linux "$OS_DIR/arch" ok ok 3.10.0
assert_contains "$(file_of "$req_dir/out")" "  [ok]    ai-jail 3.10.0"$'\n' "3.10.0: [ok] (comparação numérica)"
for req_version_case in "Linux|arch|2.14.0|$CMD_JAIL_ARCH" "Linux|arch|4.0.0|$CMD_JAIL_ARCH" "Darwin|arch|2.14.0|$CMD_JAIL_MAC_UPGRADE" "Linux|ubuntu|2.14.0|$CMD_JAIL_CARGO" "Linux|fedora|3.1.9|$CMD_JAIL_CARGO"; do
  IFS='|' read -r req_uname req_os req_version req_cmd <<< "$req_version_case"
  req_dir="$(new_case "req_version_${req_uname}_${req_os}_$req_version")"
  requirements "$req_dir" "$req_uname" "$OS_DIR/$req_os" ok ok "$req_version"
  req_out="$(file_of "$req_dir/out")"
  req_label="$req_uname/$req_os com ai-jail $req_version"
  assert_contains "$req_out" "  [atualizar] ai-jail $req_version  →  $req_cmd" "$req_label: [atualizar] com a versão instalada e o comando '$req_cmd'"
  assert_not_contains "$req_out" "[ok]    ai-jail" "$req_label: sem [ok] no ai-jail"
  assert_contains "$req_out" "$req_supported" "$req_label: a linha da versão suportada aparece"
  assert_eq "0|" "$(file_of "$req_dir/rc")|$(file_of "$req_dir/err")" "$req_label: código 0 e nada no stderr"
done
assert_not_contains "$(file_of "$TEST_TMP/req_version_Darwin_arch_2.14.0/out")" "$CMD_JAIL_MAC" "macOS com ai-jail antigo: manda atualizar (brew upgrade), não instalar (brew tap ... install)"
req_dir="$(new_case req_version_unknown)"
requirements "$req_dir" Linux "$OS_DIR/arch" ok ok ""
assert_contains "$(file_of "$req_dir/out")" "  [atualizar] ai-jail (versão não identificada)  →  $CMD_JAIL_ARCH" "versão ilegível: [atualizar] com 'versão não identificada'"
req_dir="$(new_case req_version_falta)"
requirements "$req_dir" Linux "$OS_DIR/arch" falta ok
req_out="$(file_of "$req_dir/out")"
assert_contains "$req_out" "  [falta] ai-jail  →  $CMD_JAIL_ARCH" "ai-jail ausente: a linha [falta] continua a de antes"
assert_contains "$req_out" "$req_supported" "ai-jail ausente: a linha da versão suportada aparece"
assert_not_exists "$req_dir/jail/version.calls" "ai-jail ausente: não roda --version"
req_dir="$(new_case req_version_bwrap)"
requirements "$req_dir" Linux "$OS_DIR/arch" ok falta 2.14.0
assert_contains "$(file_of "$req_dir/out")" "  [falta] bwrap (bubblewrap)  →  $CMD_BWRAP_ARCH" "a linha do bwrap não muda com a versão do ai-jail"
# O número vem de JAIL_VERSION_MIN: com outro valor, a linha acompanha.
req_dir="$(new_case req_version_min)"
timeout 5 "$BASH_BIN" -c "cd '$req_dir/cwd' || exit 99; PATH='$FAKE_UNAME_DIR:$FAKE_JAIL_DIR'; export FAKE_UNAME=Linux FAKE_JAIL_LOG_DIR='$req_dir/jail'; source '$JAIL_SH'; JAIL_OS_RELEASE_FILE='$OS_DIR/arch'; JAIL_VERSION_MIN=5.1.0; jail_print_requirements" < /dev/null > "$req_dir/out" 2>&1 || true
assert_contains "$(file_of "$req_dir/out")" "Versão suportada do ai-jail: 5.1.0 ou mais nova da série 5.x." "a versão suportada sai de JAIL_VERSION_MIN"
assert_contains "$(file_of "$req_dir/out")" "[atualizar] ai-jail 3.2.0" "com JAIL_VERSION_MIN=5.1.0, o 3.2.0 do stub vira [atualizar]"

echo
echo "== jail_print_requirements: só mostra os comandos, nunca os executa =="
# Tira as strings entre aspas das linhas de código: o que sobrar não pode citar um
# instalador (os comandos só podem aparecer dentro de texto impresso).
jail_unquoted="$(sed -E "s/\"([^\"\\\\]|\\\\.)*\"//g; s/'[^']*'//g" <<< "$jail_code")"
assert_eq "" "$(grep -nwE 'pacman|yay|apt|apt-get|dnf|brew|cargo|sudo|curl|wget' <<< "$jail_unquoted" || true)" "nenhuma linha de lib/jail/jail.sh executa pacman/yay/apt/dnf/brew/cargo/sudo/curl/wget"
req_body="$(awk '/^jail_print_requirements\(\) \{/{found=1} found{print} found && /^\}/{exit}' "$JAIL_SH" 2>/dev/null || true)"
assert_contains "$req_body" "$CMD_JAIL_ARCH" "o comando do Arch fica no corpo de jail_print_requirements (fonte única)"

echo
echo "== README × helper: cada comando de instalação do helper aparece literal no README.md =="
readme_text="$(cat "$README_MD" 2>/dev/null || true)"
for readme_cmd in "$CMD_JAIL_ARCH" "$CMD_JAIL_CARGO" "$CMD_JAIL_RELEASES" "$CMD_JAIL_MAC" "$CMD_JAIL_MAC_UPGRADE" "$CMD_BWRAP_ARCH" "$CMD_BWRAP_DEBIAN" "$CMD_BWRAP_FEDORA"; do
  assert_contains "$readme_text" "$readme_cmd" "README.md cita '$readme_cmd'"
  assert_contains "$req_body" "$readme_cmd" "jail_print_requirements cita '$readme_cmd'"
done

echo
echo "== README × jail_options: os níveis do JAIL_CLIPBOARD e as opções de cada um aparecem no README.md =="
# jail_options roda num bash que carrega a biblioteca (esta suíte não a carrega), com home
# vazio e sem a lista de hosts: só as opções (as linhas com --) contam, não os valores.
readme_home="$TEST_TMP/readme_home"
mkdir -p "$readme_home"
for readme_level in restrito medio brando; do
  assert_contains "$readme_text" "\"$readme_level\"" "README.md cita o nível \"$readme_level\""
  readme_checked=0
  while IFS= read -r readme_option; do
    case "$readme_option" in
      --*) ;;
      *) continue ;;
    esac
    readme_checked=$((readme_checked + 1))
    assert_contains "$readme_text" "$readme_option" "README.md cita a opção $readme_option (nível $readme_level)"
  done < <(HOME="$readme_home" timeout 5 bash -c "unset PLAYWRIGHT_BROWSERS_PATH; source '$JAIL_SH'; JAIL_HOSTS_FILE='$TEST_TMP/nao-existe-hosts'; jail_options '$readme_level' 'x.io' '1'" 2>/dev/null)
  # Guarda: o laço confere de fato as opções (a base, --allow-host e --forward-port).
  assert_eq "true" "$([ "$readme_checked" -ge "$((${#jail_base_opts[@]} + 2))" ] && echo true || echo false)" "README × jail_options \"$readme_level\": o laço conferiu as opções de jail_options ($readme_checked)"
done
# A versão suportada: o número vem de JAIL_VERSION_MIN (lido da biblioteca, não digitado aqui).
readme_version_min="$(timeout 2 bash -c "source '$JAIL_SH'; printf '%s' \"\$JAIL_VERSION_MIN\"" 2>/dev/null || true)"
assert_eq "true" "$([ -n "$readme_version_min" ] && echo true || echo false)" "JAIL_VERSION_MIN lido da biblioteca"
assert_contains "$readme_text" "$readme_version_min ou mais nova da série ${readme_version_min%%.*}.x" "README.md cita a versão suportada ($readme_version_min ou mais nova da série ${readme_version_min%%.*}.x), a de JAIL_VERSION_MIN"
# A mensagem de erro da verificação remete a essas seções do README.
assert_contains "$readme_text" "## Sandbox (ai-jail)" "README.md tem a seção \"Sandbox (ai-jail)\" citada no erro"
assert_contains "$readme_text" "### Problemas comuns" "README.md tem a subseção \"Problemas comuns\" citada no erro"
assert_not_contains "$readme_text" "trust_project_config = " "README.md não ensina mais trust_project_config (é do 2.x; no 3.x só vale pra arquivo legado)"
# O "~/" é texto do README, não caminho a expandir.
# shellcheck disable=SC2088
assert_contains "$readme_text" "~/.config/ai-jail/projects/" "README.md diz onde fica o perfil salvo pela tela de aprovação"
assert_contains "$readme_text" "ai-jail configure claude" "README.md diz como rever ou remover o perfil salvo pela tela"
# O "~/" é texto do README, não caminho a expandir.
# shellcheck disable=SC2088
assert_contains "$readme_text" "~/.config/ai-jail/config.toml" "README.md cita a configuração global do ai-jail 3.x"
assert_contains "$readme_text" "validação de binário confiável" "README.md, em Problemas comuns, explica o bwrap reprovado na validação de binário confiável"

echo
echo "== jail_check desligada: retorna 0 sem imprimir nada nem chamar o ai-jail =="
for check_value in "" false TRUE; do
  check_dir="$(new_case "check_off_${check_value:-vazio}")"
  run_jail "$check_dir" "jail_check '$check_value' total '' '' '/proj/.nick/nick.conf' true; echo depois" "export FAKE_JAIL_RC=1"
  assert_eq "0|depois|" "$(file_of "$check_dir/rc")|$(file_of "$check_dir/out")|$(file_of "$check_dir/err")" "jail_check \"$check_value\" (JAIL_CLIPBOARD inválido): código 0, nada impresso (só o 'depois' do teste)"
  assert_eq "0" "$(jail_calls "$check_dir")" "jail_check \"$check_value\": o ai-jail não é chamado"
  assert_not_exists "$check_dir/jail/version.calls" "jail_check \"$check_value\": nem 'ai-jail --version' roda"
done

echo
echo "== jail_check ligada, jail funcionando: verifica num diretório temporário, removido em seguida =="
check_dir="$(new_case check_ok)"
run_jail "$check_dir" "jail_check true '' '' '' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
assert_eq "rc=0" "$(file_of "$check_dir/out")" "jail ok: retorna 0 sem imprimir nada no stdout"
assert_eq "" "$(file_of "$check_dir/err")" "jail ok: nada no stderr"
assert_eq "1" "$(jail_calls "$check_dir")" "jail ok: uma chamada ao ai-jail"
assert_eq "version" "$(file_of "$check_dir/jail/version.calls")" "jail ok: 'ai-jail --version' roda uma vez, antes da verificação"
assert_eq "$(jail_options_of '')" "$(file_of "$check_dir/jail/call_1.argv")" "jail ok, JAIL_CLIPBOARD vazio: a verificação é 'ai-jail <opções base> --terminal-passthrough -- true' (as mesmas opções do lançamento)"
check_cwd="$(file_of "$check_dir/jail/call_1.cwd")"
assert_eq "true" "$([ -n "$check_cwd" ] && [ "$check_cwd" != "sem arquivo" ] && [ "$check_cwd" != "$check_dir/cwd" ] && echo true || echo false)" "jail ok: a verificação roda com o cwd fora do cwd do chamador"
assert_not_exists "$check_dir/cwd/.ai-jail" "jail ok: nenhum .ai-jail criado no cwd do chamador"
assert_eq "true" "$([ "$check_cwd" != "sem arquivo" ] && [ ! -e "$check_cwd" ] && echo true || echo false)" "jail ok: o diretório temporário da verificação foi removido"
assert_eq "" "$(find "$check_dir/cwd" -mindepth 1 2>/dev/null || true)" "jail ok: nada criado no cwd do chamador"
check_ports_ok_case=0
check_ports_bad_case=0
# Verificação que passa: JAIL_LANDLOCK fica com o estado da saída do ai-jail, sem imprimir nada.
check_dir="$(new_case check_landlock)"
run_jail "$check_dir" "jail_check true '' '' '' '/proj/.nick/nick.conf' false; echo \"rc=\$? landlock=\$JAIL_LANDLOCK\"" "export FAKE_JAIL_STDERR=\$'▸ Jail Active: /tmp/x\\n▸ Landlock: partially enforced'"
assert_eq "rc=0 landlock=parcial|" "$(file_of "$check_dir/out")|$(file_of "$check_dir/err")" "jail ok: JAIL_LANDLOCK=\"parcial\" e nada impresso"
check_dir="$(new_case check_landlock_falha)"
run_jail "$check_dir" "JAIL_LANDLOCK=completo; jail_check true '' '' '' '/proj/.nick/nick.conf' false || echo \"rc=\$? landlock=[\$JAIL_LANDLOCK]\"" "export FAKE_JAIL_RC=1 FAKE_JAIL_STDERR='▸ Landlock: fully enforced'"
assert_eq "rc=1 landlock=[]" "$(file_of "$check_dir/out")" "verificação que falha: JAIL_LANDLOCK fica vazia (mesmo com um estado de antes)"
check_dir="$(new_case check_landlock_off)"
run_jail "$check_dir" "JAIL_LANDLOCK=completo; jail_check false '' '' '' '/proj/.nick/nick.conf' false; echo \"landlock=[\$JAIL_LANDLOCK]\""
assert_eq "landlock=[]" "$(file_of "$check_dir/out")" "jail desligada: JAIL_LANDLOCK fica vazia (mesmo com um estado de antes)"
# As portas também entram na verificação.
check_dir="$(new_case check_ok_ports)"
run_jail "$check_dir" "jail_check true '' '' '54320' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
assert_eq "rc=0" "$(file_of "$check_dir/out")" "jail ok com portas: retorna 0"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough "${docs_opts[@]}" --forward-port 54320 -- true)" "$(file_of "$check_dir/jail/call_1.argv")" "jail ok com portas: a verificação usa o mesmo --forward-port do lançamento"
for check_ports in "1" "65535" "54320 6379" "  54320  "; do
  check_ports_ok_case=$((check_ports_ok_case + 1))
  check_dir="$(new_case "check_ports_ok_${check_ports_ok_case}")"
  run_jail "$check_dir" "jail_check true '' '' '$check_ports' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
  assert_eq "rc=0|" "$(file_of "$check_dir/out")|$(file_of "$check_dir/err")" "JAIL_FORWARD_PORTS \"$check_ports\": aceito"
done
for check_ports in abc 0 65536 054320 -1 "54320,6379" "54320 x" "99999999999999999999" $'54320\e[2J' "54320 6379 54320"; do
  check_ports_bad_case=$((check_ports_bad_case + 1))
  check_dir="$(new_case "check_ports_bad_${check_ports_bad_case}")"
  printf -v check_ports_q '%q' "$check_ports"
  run_jail "$check_dir" "jail_check true '' '' $check_ports_q '/proj/um projeto/.nick/nick.conf' false || echo \"rc=\$?\"; echo depois"
  check_err="$(file_of "$check_dir/err")"
  check_label="JAIL_FORWARD_PORTS $check_ports_q"
  assert_eq "rc=1"$'\n'"depois" "$(file_of "$check_dir/out")" "$check_label: retorna 1, sem sair do shell"
  assert_eq "0" "$(jail_calls "$check_dir")" "$check_label: o ai-jail não é chamado"
  assert_not_exists "$check_dir/jail/version.calls" "$check_label: para antes de 'ai-jail --version'"
  assert_contains "$check_err" "Erro: JAIL_FORWARD_PORTS=\"${check_ports//[[:cntrl:]]/?}\" em /proj/um projeto/.nick/nick.conf não é uma lista de portas válida." "$check_label: o erro cita o valor (controles trocados por '?') e o arquivo"
  assert_contains "$check_err" 'Use números de 1 a 65535, sem repetir, separados por espaço, ex: JAIL_FORWARD_PORTS="54320 6379".' "$check_label: o erro diz o formato aceito"
  assert_not_contains "$check_err" "Requisitos da jail" "$check_label: não mostra a lista de requisitos"
done
check_dir="$(new_case check_ports_off)"
run_jail "$check_dir" "jail_check false '' '' abc '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
assert_eq "rc=0|" "$(file_of "$check_dir/out")|$(file_of "$check_dir/err")" "jail desligada: JAIL_FORWARD_PORTS inválido passa quieto (a chave não tem efeito)"
# Hosts extras (os dos MCPs e os de JAIL_ALLOW_HOSTS): só nomes de host, que o ai-jail libera
# com os subdomínios. URL, porta, caminho, curinga, maiúscula ou nome sem ponto param antes do
# ai-jail, com o host errado no erro.
check_hosts_ok_case=0
for check_hosts in "docs.djangoproject.com" "mcp.atlassian.com docs.djangoproject.com tailwindcss.com" "  a-b.c1.io  " "x.io x.io"; do
  check_hosts_ok_case=$((check_hosts_ok_case + 1))
  check_dir="$(new_case "check_hosts_ok_${check_hosts_ok_case}")"
  run_jail "$check_dir" "jail_check true '' '$check_hosts' '' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
  assert_eq "rc=0|" "$(file_of "$check_dir/out")|$(file_of "$check_dir/err")" "hosts \"$check_hosts\": aceitos"
done
check_hosts_bad_case=0
for check_hosts_case in "https://docs.djangoproject.com|https://docs.djangoproject.com" "docs.djangoproject.com/pt-br|docs.djangoproject.com/pt-br" "mcp.sentry.dev pypi.org:443|pypi.org:443" "*.djangoproject.com|*.djangoproject.com" "Docs.Djangoproject.com|Docs.Djangoproject.com" "localhost|localhost" ".x.io|.x.io" "x.io.|x.io." "x..io|x..io" "-x.io|-x.io" "x-.io|x-.io" "x.-io|x.-io" "x.io,y.io|x.io,y.io" $'x.io\e[2J|x.io?[2J'; do
  check_hosts="${check_hosts_case%%|*}"
  check_bad_host="${check_hosts_case#*|}"
  check_hosts_bad_case=$((check_hosts_bad_case + 1))
  check_dir="$(new_case "check_hosts_bad_${check_hosts_bad_case}")"
  printf -v check_hosts_q '%q' "$check_hosts"
  run_jail "$check_dir" "jail_check true '' $check_hosts_q '' '/proj/um projeto/.nick/nick.conf' false || echo \"rc=\$?\"; echo depois"
  check_err="$(file_of "$check_dir/err")"
  check_label="hosts $check_hosts_q"
  assert_eq "rc=1"$'\n'"depois" "$(file_of "$check_dir/out")" "$check_label: retorna 1, sem sair do shell"
  assert_eq "0" "$(jail_calls "$check_dir")" "$check_label: o ai-jail não é chamado"
  assert_not_exists "$check_dir/jail/version.calls" "$check_label: para antes de 'ai-jail --version'"
  assert_contains "$check_err" "Erro: JAIL_ALLOW_HOSTS em /proj/um projeto/.nick/nick.conf tem um host inválido: \"$check_bad_host\"." "$check_label: o erro cita o host errado (controles trocados por '?') e o arquivo"
  assert_contains "$check_err" 'Use só nomes de host (letras minúsculas, números, ponto e hífen), sem https://, porta ou caminho, separados por espaço, ex: JAIL_ALLOW_HOSTS="docs.djangoproject.com tailwindcss.com". Cada host libera também os subdomínios dele.' "$check_label: o erro diz o formato aceito"
  assert_not_contains "$check_err" "Requisitos da jail" "$check_label: não mostra a lista de requisitos"
done
check_dir="$(new_case check_hosts_off)"
run_jail "$check_dir" "jail_check false '' 'https://x' '' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
assert_eq "rc=0|" "$(file_of "$check_dir/out")|$(file_of "$check_dir/err")" "jail desligada: host inválido passa quieto (a chave não tem efeito)"
# As toolchains também entram na verificação.
check_dir="$(new_case check_ok_toolchains)"
mkdir -p "$check_dir/home/.pyenv"
run_jail "$check_dir" "jail_check true '' '' '' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough --map "$check_dir/home/.pyenv" "${docs_opts[@]}" -- true)" "$(file_of "$check_dir/jail/call_1.argv")" "jail ok com ~/.pyenv: a verificação usa o mesmo --map do lançamento"
# Os hosts extras também entram na verificação: um ai-jail que recuse --allow-host falha aqui.
check_dir="$(new_case check_ok_hosts)"
run_jail "$check_dir" "jail_check true '' 'mcp.atlassian.com mcp.sentry.dev' '' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
assert_eq "rc=0" "$(file_of "$check_dir/out")" "jail ok com hosts: retorna 0"
assert_eq "$(printf '%s\n' "${jail_base_opts[@]}" --terminal-passthrough --allow-host mcp.atlassian.com --allow-host mcp.sentry.dev "${docs_opts[@]}" -- true)" "$(file_of "$check_dir/jail/call_1.argv")" "jail ok com hosts: a verificação usa os mesmos --allow-host do lançamento"
for check_clip in restrito brando; do
  check_dir="$(new_case "check_ok_$check_clip")"
  run_jail "$check_dir" "jail_check true '$check_clip' '' '' '/proj/.nick/nick.conf' false; echo \"rc=\$?\""
  assert_eq "rc=0" "$(file_of "$check_dir/out")" "jail ok, JAIL_CLIPBOARD \"$check_clip\": retorna 0"
  assert_eq "$(jail_options_of "$check_clip")" "$(file_of "$check_dir/jail/call_1.argv")" "jail ok, JAIL_CLIPBOARD \"$check_clip\": a verificação usa as opções do nível"
done

echo
echo "== jail_check ligada, JAIL_CLIPBOARD inválido: retorna 1 antes de chamar o ai-jail, com o erro de valor =="
for check_clip in total RESTRITO " medio" $'brando\e[2J'; do
  check_dir="$(new_case "check_bad_clip_$(printf '%s' "$check_clip" | tr -c 'a-zA-Z' '_')")"
  printf -v check_clip_q '%q' "$check_clip"
  run_jail "$check_dir" "jail_check true $check_clip_q '' '' '/proj/um projeto/.nick/nick.conf' false || echo \"rc=\$?\"; echo depois"
  check_err="$(file_of "$check_dir/err")"
  check_label="JAIL_CLIPBOARD $check_clip_q"
  assert_eq "rc=1"$'\n'"depois" "$(file_of "$check_dir/out")" "$check_label: retorna 1, sem sair do shell"
  assert_eq "0" "$(jail_calls "$check_dir")" "$check_label: o ai-jail não é chamado"
  assert_not_exists "$check_dir/jail/version.calls" "$check_label: para antes de 'ai-jail --version'"
  assert_contains "$check_err" "Erro: JAIL_CLIPBOARD=\"${check_clip//[[:cntrl:]]/?}\" em /proj/um projeto/.nick/nick.conf não é um valor aceito." "$check_label: o erro cita o valor (controles trocados por '?') e o arquivo"
  assert_contains "$check_err" 'Valores aceitos: "restrito", "medio", "brando" ou "" (vazio, que vale "medio").' "$check_label: o erro lista os valores aceitos"
  assert_not_contains "$check_err" "Requisitos da jail" "$check_label: não mostra a lista de requisitos (não é problema de instalação)"
done

echo
echo "== jail_check ligada, ai-jail ausente: retorna 1 (sem sair do shell) com a mensagem de erro =="
check_dir="$(new_case check_missing)"
run_jail "$check_dir" "JAIL_BIN='$MISSING_JAIL_BIN'; jail_check true '' '' '' '/proj/um projeto/.nick/nick.conf' false || echo \"rc=\$?\"; echo depois"
check_err="$(file_of "$check_dir/err")"
assert_eq "rc=1"$'\n'"depois" "$(file_of "$check_dir/out")" "ai-jail ausente: retorna 1 e o shell continua (um comando depois da chamada roda)"
assert_contains "$check_err" 'Erro: JAIL_ENABLED="true" em /proj/um projeto/.nick/nick.conf' "ai-jail ausente: a mensagem cita o caminho da configuração"
assert_contains "$check_err" "não encontrado" "ai-jail ausente: diz que o ai-jail não foi encontrado"
assert_contains "$check_err" 'JAIL_ENABLED="false" em /proj/um projeto/.nick/nick.conf' "ai-jail ausente: oferece JAIL_ENABLED=\"false\" no mesmo arquivo"
assert_contains "$check_err" "[falta] ai-jail" "ai-jail ausente: termina com a lista de requisitos ([falta] ai-jail)"
assert_not_contains "$check_err" "Saída do ai-jail" "ai-jail ausente: sem bloco de saída do ai-jail"
assert_contains "$check_err" "  1. instale o que falta (lista abaixo);" "ai-jail ausente: a saída 1 manda instalar o que falta"
assert_not_contains "$check_err" "corrija o problema acima" "ai-jail ausente: não manda corrigir um problema que não foi mostrado"
assert_eq "0" "$(jail_calls "$check_dir")" "ai-jail ausente: o stub não é chamado"
assert_not_exists "$check_dir/jail/version.calls" "ai-jail ausente: nem 'ai-jail --version' roda"

echo
echo "== jail_check ligada, versão do ai-jail fora da faixa: retorna 1 antes da verificação, mandando atualizar =="
for check_version_case in "2.14.0|2.14.0" "4.0.0|4.0.0" "3.1.9|3.1.9" "|não identificada" "rc1|não identificada" $'ctrl|não identificada'; do
  check_version="${check_version_case%%|*}"
  check_found="${check_version_case#*|}"
  check_env="export FAKE_JAIL_VERSION='$check_version'"
  case "$check_version" in
    rc1) check_env="export FAKE_JAIL_VERSION_RC=1" ;;
    ctrl) check_env="export FAKE_JAIL_VERSION=\$'3.2.0\\e[2J'" ;;
  esac
  check_dir="$(new_case "check_version_${check_version:-vazia}")"
  run_jail "$check_dir" "JAIL_LANDLOCK=completo; jail_check true '' '' '' '/proj/um projeto/.nick/nick.conf' false || echo \"rc=\$? landlock=[\$JAIL_LANDLOCK]\"; echo depois" "$check_env"
  check_err="$(file_of "$check_dir/err")"
  check_label="ai-jail --version \"${check_version:-}\""
  assert_eq "rc=1 landlock=[]"$'\n'"depois" "$(file_of "$check_dir/out")" "$check_label: retorna 1 sem sair do shell, JAIL_LANDLOCK vazia"
  assert_eq "0" "$(jail_calls "$check_dir")" "$check_label: a verificação '-- true' não roda"
  assert_contains "$check_err" 'Erro: JAIL_ENABLED="true" em /proj/um projeto/.nick/nick.conf' "$check_label: a mensagem cita o caminho da configuração"
  assert_contains "$check_err" "versão do ai-jail" "$check_label: a mensagem fala da versão"
  case "$check_found" in
    "não identificada") assert_contains "$check_err" "não identificada" "$check_label: diz que a versão não foi identificada" ;;
    *) assert_contains "$check_err" "encontrada $check_found" "$check_label: diz a versão encontrada ($check_found)" ;;
  esac
  assert_contains "$check_err" "suportada: 3.2.0 ou mais nova da série 3.x" "$check_label: diz a versão suportada"
  assert_contains "$check_err" "  1. atualize o ai-jail" "$check_label: a saída 1 manda atualizar o ai-jail"
  assert_not_contains "$check_err" "instale o que falta" "$check_label: a saída 1 não manda instalar o que falta"
  assert_not_contains "$check_err" "corrija o problema acima" "$check_label: a saída 1 não manda corrigir um problema mostrado"
  assert_contains "$check_err" 'JAIL_ENABLED="false" em /proj/um projeto/.nick/nick.conf' "$check_label: oferece JAIL_ENABLED=\"false\" no mesmo arquivo"
  assert_contains "$check_err" "[atualizar] ai-jail" "$check_label: termina com a lista de requisitos, com [atualizar]"
  assert_not_contains "$check_err" $'\e' "$check_label: nenhum ESC chega ao terminal"
done
# "--version" saindo com erro (mesmo imprimindo uma versão): o erro diz que falhou e com qual
# código, e não que ele não respondeu.
assert_contains "$(file_of "$TEST_TMP/check_version_rc1/err")" 'não identificada ("ai-jail --version" falhou com código 1)' "--version com código 1: o erro diz que ele falhou e o código"
assert_not_contains "$(file_of "$TEST_TMP/check_version_rc1/err")" "não respondeu" "--version com código 1: o erro não diz que ele não respondeu"
assert_contains "$(file_of "$TEST_TMP/check_version_vazia/err")" 'não identificada (saída de "ai-jail --version": "ai-jail ")' "--version com código 0 e sem número: o erro mostra a saída"
# A saída ilegível aparece no erro, com os controles trocados por "?".
assert_contains "$(file_of "$TEST_TMP/check_version_ctrl/err")" '"ai-jail 3.2.0?[2J"' "versão com caractere de controle: a saída aparece com '?' no lugar do controle"

echo
echo "== jail_check ligada, ai-jail falhando: retorna 1 com a saída do ai-jail na mensagem =="
check_dir="$(new_case check_fail)"
run_jail "$check_dir" "jail_check true '' '' '' '/proj/.nick/nick.conf' false || echo \"rc=\$?\"" "export FAKE_JAIL_RC=1 FAKE_JAIL_STDERR='bwrap: setting up uid map: Permission denied'"
check_err="$(file_of "$check_dir/err")"
assert_eq "rc=1" "$(file_of "$check_dir/out")" "ai-jail falhando: retorna 1"
assert_contains "$check_err" "Saída do ai-jail:" "ai-jail falhando: a mensagem tem o bloco de saída"
assert_contains "$check_err" "    bwrap: setting up uid map: Permission denied" "ai-jail falhando: a saída do ai-jail aparece, indentada"
assert_contains "$check_err" 'JAIL_ENABLED="false" em /proj/.nick/nick.conf' "ai-jail falhando: oferece JAIL_ENABLED=\"false\""
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
run_jail "$check_dir" "jail_check true '' '' '' '/proj/.nick/nick.conf' false || :" "export FAKE_JAIL_RC=1 FAKE_JAIL_STDERR=\$'primeira \\e[2J linha\\nsegunda \\e[2J linha'"
check_err="$(file_of "$check_dir/err")"
assert_contains "$check_err" "    primeira ?[2J linha"$'\n'"    segunda ?[2J linha" "ESC vira '?' e cada linha da saída continua numa linha própria"
assert_not_contains "$check_err" $'\e' "nenhum ESC chega ao terminal"
check_dir="$(new_case check_control_c)"
run_jail "$check_dir" "jail_check true '' '' '' '/proj/.nick/nick.conf' false || :" "export LC_ALL=C FAKE_JAIL_RC=1 FAKE_JAIL_STDERR='falhou: não há namespace'"
assert_contains "$(file_of "$check_dir/err")" "    falhou: não há namespace" "com LC_ALL=C, o 'não' da saída sai intacto"

echo
echo "== jail_check: aviso de arquivo compartilhado só com shared=\"true\" =="
check_dir="$(new_case check_shared)"
run_jail "$check_dir" "JAIL_BIN='$MISSING_JAIL_BIN'; jail_check true '' '' '' '/proj/.nick/nick.conf' true || :"
assert_contains "$(file_of "$check_dir/err")" "Atenção: /proj/.nick/nick.conf é compartilhado com o time do projeto" "shared=\"true\": aviso de arquivo compartilhado"
check_dir="$(new_case check_not_shared)"
run_jail "$check_dir" "JAIL_BIN='$MISSING_JAIL_BIN'; jail_check true '' '' '' '/nick/projects/alpha.conf' false || :"
assert_not_contains "$(file_of "$check_dir/err")" "compartilhado" "shared=\"false\": sem aviso de arquivo compartilhado"

echo
echo "== jail_check: não depende do set -e (chamada antes de ||, com mktemp falhando) =="
check_dir="$(new_case check_mktemp)"
run_jail "$check_dir" "jail_check true '' '' '' '/proj/.nick/nick.conf' false || echo \"rc=\$?\"" "export TMPDIR='$TEST_TMP/nao-existe'"
assert_eq "rc=1" "$(file_of "$check_dir/out")" "mktemp falhando: retorna 1 em vez de seguir como se a jail funcionasse"
assert_contains "$(file_of "$check_dir/err")" 'JAIL_ENABLED="true" em /proj/.nick/nick.conf' "mktemp falhando: mensagem de erro com o caminho"
assert_eq "0" "$(jail_calls "$check_dir")" "mktemp falhando: o ai-jail não é chamado"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
