#!/usr/bin/env bash
#
# test_install: testes em bash puro (sem framework) do instalador local do nick
# (install/install.sh, chamado pelo make install e pelo make uninstall, NS-52). Nunca roda o
# cargo real (seria um build release a cada caso; o teste de ponta a ponta com o build de
# verdade é o nick/tests/install.rs): o binário vem de --binary (um nick falso que só responde
# ao --version) ou de um cargo falso no PATH. Tudo acontece no TEST_TMP, com HOME e
# XDG_STATE_HOME temporários: nenhum caso grava no repositório nem no ~/.local real.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
INSTALL_SH="$ROOT_DIR/install/install.sh"
MAKEFILE="$ROOT_DIR/Makefile"

failures=0

# Diretório-base temporário único da suíte: cada caso usa um subdiretório dele.
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

# shellcheck source=SCRIPTDIR/../../lib/tests/assert.sh
source "$ROOT_DIR/lib/tests/assert.sh"

BASH_BIN="$(command -v bash)"
FAKE_VERSION="nick 9.9.9 (commit abc1234)"

# file_of <path>: o conteúdo de <path>, ou vazio se não existir.
file_of() {
  cat "$1" 2> /dev/null || true
}

# code_lines <file>: as linhas de <file> que não são comentário de linha inteira.
code_lines() {
  grep -vE '^[[:space:]]*#' "$1" || true
}

# O nick falso: responde ao --version como o de verdade.
FAKE_NICK="$TEST_TMP/fake-nick"
cat > "$FAKE_NICK" <<FAKE
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then
  echo "$FAKE_VERSION"
  exit 0
fi
exit 1
FAKE
chmod +x "$FAKE_NICK"

# PATH mínimo, sem cargo e sem sudo: só o que o install.sh usa. O controle positivo (o primeiro
# caso) prova que esse PATH basta.
MIN_BIN="$TEST_TMP/min_bin"
mkdir -p "$MIN_BIN"
for tool in bash dirname install mkdir rm rmdir find sort cat; do
  ln -s "$(command -v "$tool")" "$MIN_BIN/$tool"
done

# O sudo falso: grava um marcador. O script nunca deve chamá-lo.
SUDO_MARKER="$TEST_TMP/sudo-chamado"
FAKE_SUDO_BIN="$TEST_TMP/fake_sudo_bin"
mkdir -p "$FAKE_SUDO_BIN"
cat > "$FAKE_SUDO_BIN/sudo" <<FAKE
#!/usr/bin/env bash
echo "\$*" >> "$SUDO_MARKER"
exit 1
FAKE
chmod +x "$FAKE_SUDO_BIN/sudo"

# O cargo falso: grava "<argv>|<cwd físico>" em FAKE_CARGO_LOG, sai com 3 quando
# FAKE_CARGO_FAIL está definido e, senão, cria o binário em <alvo>/release/nick (uma cópia do
# nick falso). O <alvo> é o do cargo real: $CARGO_TARGET_DIR, ou target/ ao lado do
# --manifest-path.
FAKE_CARGO_BIN="$TEST_TMP/fake_cargo_bin"
mkdir -p "$FAKE_CARGO_BIN"
cat > "$FAKE_CARGO_BIN/cargo" <<FAKE
#!/usr/bin/env bash
printf '%s|%s\n' "\$*" "\$(pwd -P)" >> "\$FAKE_CARGO_LOG"
if [ -n "\${FAKE_CARGO_FAIL:-}" ]; then
  echo "erro de compilação falso" >&2
  exit 3
fi
manifest=""
while [ "\$#" -gt 0 ]; do
  if [ "\$1" = "--manifest-path" ]; then
    manifest="\$2"
  fi
  shift
done
target="\${CARGO_TARGET_DIR:-\$(dirname "\$manifest")/target}"
mkdir -p "\$target/release"
cp "$FAKE_NICK" "\$target/release/nick"
FAKE
chmod +x "$FAKE_CARGO_BIN/cargo"
# O cp do cargo falso (o install.sh não usa: fica fora do PATH mínimo).
ln -s "$(command -v cp)" "$FAKE_CARGO_BIN/cp"

# PATH padrão dos casos: o sudo falso e o mínimo.
CASE_PATH="$FAKE_SUDO_BIN:$MIN_BIN"

# run_install <case_dir> <args...>: roda o install.sh com o cwd em <case_dir>/cwd, HOME e
# XDG_STATE_HOME do caso, o PATH padrão e sem CARGO_TARGET_DIR. Variáveis a mais vão antes, pelo
# env, com RUN_ENV (um array de VAR=valor, que vence o padrão). Grava rc, out e err no
# <case_dir>.
RUN_ENV=()
run_install() {
  local case_dir="$1" rc=0
  shift
  mkdir -p "$case_dir/cwd" "$case_dir/home"
  (cd "$case_dir/cwd" && timeout 20 env -u CARGO_TARGET_DIR -u FAKE_CARGO_FAIL \
    HOME="$case_dir/home" XDG_STATE_HOME="$case_dir/state" PATH="$CASE_PATH" \
    "${RUN_ENV[@]}" "$BASH_BIN" "$INSTALL_SH" "$@" < /dev/null \
    > "$case_dir/out" 2> "$case_dir/err") || rc=$?
  echo "$rc" > "$case_dir/rc"
}

# expected_tree: a lista esperada dos arquivos instalados (relativos ao prefixo), gerada do
# checkout pela regra do plano (escrita aqui, não lida do script): o binário, o manifesto, todo
# arquivo regular direto em bin/, em cada lib/<dir>/ menos lib/tests/, o auto_scrum/prompt.sh e
# os auto_scrum/templates/*.md.
expected_tree() {
  local file dir
  {
    echo "bin/nick"
    echo "share/nick/.nick-install"
    for file in "$ROOT_DIR"/bin/*; do
      [ -f "$file" ] && echo "share/nick/bin/${file##*/}"
    done
    for dir in "$ROOT_DIR"/lib/*/; do
      dir="${dir%/}"
      [ "${dir##*/}" = "tests" ] && continue
      for file in "$dir"/*; do
        [ -f "$file" ] && echo "share/nick/lib/${dir##*/}/${file##*/}"
      done
    done
    echo "share/nick/auto_scrum/prompt.sh"
    for file in "$ROOT_DIR"/auto_scrum/templates/*.md; do
      echo "share/nick/auto_scrum/templates/${file##*/}"
    done
  } | LC_ALL=C sort
}

# tree_of <prefix>: todo arquivo (e link) sob <prefix>, relativo a ele, ordenado.
tree_of() {
  (cd "$1" && find . \( -type f -o -type l \) | sed 's|^\./||' | LC_ALL=C sort)
}

# mode_of <path>: o modo em octal (ex: 755).
mode_of() {
  stat -c '%a' "$1"
}

echo "== install: a árvore, os modos e o manifesto =="
case_dir="$TEST_TMP/install"
prefix="$case_dir/p"
run_install "$case_dir" install --prefix "$prefix" --binary "$FAKE_NICK"
assert_eq "0" "$(file_of "$case_dir/rc")" "install --binary: sai com 0 (o PATH mínimo basta)"
assert_eq "" "$(file_of "$case_dir/err")" "install --binary: nada no stderr"
expected="$(expected_tree)"
assert_eq "$expected" "$(tree_of "$prefix")" "install: a árvore instalada é exatamente a da regra"
assert_contains "$expected" "share/nick/bin/nick-agent" "a regra do teste inclui o bin/nick-agent"
assert_contains "$expected" "share/nick/lib/jail/allow_hosts.txt" "a regra do teste inclui dados da lib"
assert_not_contains "$expected" "tests" "a regra do teste não leva testes"
assert_not_contains "$expected" "auto_scrum.sh" "a regra do teste não leva o auto_scrum.sh"
assert_eq "755" "$(mode_of "$prefix/bin/nick")" "bin/nick com 0755"
assert_eq "755" "$(mode_of "$prefix/share/nick/bin/nick-agent")" "bin/nick-agent com 0755"
assert_eq "755" "$(mode_of "$prefix/share/nick/bin/nick-init")" "bin/nick-init com 0755"
assert_eq "644" "$(mode_of "$prefix/share/nick/lib/config/config.sh")" "lib com 0644"
assert_eq "644" "$(mode_of "$prefix/share/nick/auto_scrum/prompt.sh")" "prompt.sh com 0644"
assert_eq "644" "$(mode_of "$prefix/share/nick/auto_scrum/templates/po.md")" "templates com 0644"
assert_eq "644" "$(mode_of "$prefix/share/nick/.nick-install")" "manifesto com 0644"
assert_eq "true" "$(cmp -s "$FAKE_NICK" "$prefix/bin/nick" && echo true || echo false)" "bin/nick é o binário dado"
assert_eq "true" "$(cmp -s "$ROOT_DIR/bin/nick-agent" "$prefix/share/nick/bin/nick-agent" && echo true || echo false)" "bin/nick-agent é o do checkout"
manifest="$(file_of "$prefix/share/nick/.nick-install")"
assert_eq "# $FAKE_VERSION" "$(head -n 1 <<< "$manifest")" "manifesto: a 1ª linha é a versão do binário"
assert_eq "$expected" "$(tail -n +2 <<< "$manifest" | LC_ALL=C sort)" "manifesto: lista tudo que foi instalado"
assert_contains "$(file_of "$case_dir/out")" "$prefix/bin/nick" "install: diz onde instalou"

echo "== install: prefixo padrão, sem HOME, relativo =="
case_dir="$TEST_TMP/default"
run_install "$case_dir" install --binary "$FAKE_NICK"
assert_eq "0" "$(file_of "$case_dir/rc")" "sem --prefix: sai com 0"
assert_exists "$case_dir/home/.local/bin/nick" "sem --prefix: instala em \$HOME/.local"
assert_exists "$case_dir/home/.local/share/nick/.nick-install" "sem --prefix: o manifesto em \$HOME/.local/share/nick"

case_dir="$TEST_TMP/no_home"
mkdir -p "$case_dir/cwd"
rc=0
(cd "$case_dir/cwd" && env -u HOME PATH="$CASE_PATH" "$BASH_BIN" "$INSTALL_SH" install \
  --binary "$FAKE_NICK" > "$case_dir/out" 2> "$case_dir/err") || rc=$?
assert_eq "1" "$rc" "sem HOME e sem --prefix: sai com 1"
assert_eq "Erro: sem HOME e sem --prefix, não sei onde instalar o nick." "$(file_of "$case_dir/err")" "sem HOME e sem --prefix: a mensagem"
assert_eq "" "$(find "$case_dir" -mindepth 1 -not -path "$case_dir/cwd" -not -name out -not -name err)" "sem HOME: nada gravado"
rc=0
(cd "$case_dir/cwd" && env HOME= PATH="$CASE_PATH" "$BASH_BIN" "$INSTALL_SH" uninstall \
  > "$case_dir/out" 2> "$case_dir/err") || rc=$?
assert_eq "1" "$rc" "uninstall com HOME vazio e sem --prefix: sai com 1"

case_dir="$TEST_TMP/relative"
run_install "$case_dir" install --prefix "rel/p" --binary "$FAKE_NICK"
assert_eq "1" "$(file_of "$case_dir/rc")" "prefixo relativo: sai com 1"
assert_eq "Erro: o prefixo precisa ser um caminho absoluto: rel/p." "$(file_of "$case_dir/err")" "prefixo relativo: a mensagem"
assert_eq "" "$(ls -A "$case_dir/cwd")" "prefixo relativo: nada gravado no cwd"
run_install "$case_dir" uninstall --prefix "rel/p"
assert_eq "1" "$(file_of "$case_dir/rc")" "uninstall com prefixo relativo: sai com 1"

echo "== install: argumentos inválidos =="
case_dir="$TEST_TMP/args"
run_install "$case_dir" instalar
assert_eq "1" "$(file_of "$case_dir/rc")" "comando desconhecido: sai com 1"
assert_contains "$(file_of "$case_dir/err")" "Uso: install.sh install [--prefix <dir>] [--binary <arquivo>]" "comando desconhecido: o uso"
run_install "$case_dir" install --prefix
assert_eq "1" "$(file_of "$case_dir/rc")" "--prefix sem valor: sai com 1"
run_install "$case_dir" uninstall --binary "$FAKE_NICK"
assert_eq "1" "$(file_of "$case_dir/rc")" "uninstall não aceita --binary"
run_install "$case_dir" install --prefix "$case_dir/p" --binary "$case_dir/nao-existe"
assert_eq "1" "$(file_of "$case_dir/rc")" "--binary inexistente: sai com 1"
assert_contains "$(file_of "$case_dir/err")" "$case_dir/nao-existe" "--binary inexistente: diz o caminho"
assert_not_exists "$case_dir/p" "--binary inexistente: nada gravado"

echo "== install: um --version que não é de um nick é recusado =="
case_dir="$TEST_TMP/bad_version"
bad_nick="$case_dir/bad-nick"
mkdir -p "$case_dir"
printf '#!/usr/bin/env bash\necho "nick 1"\necho "share/nick/../../../x"\n' > "$bad_nick"
chmod +x "$bad_nick"
run_install "$case_dir" install --prefix "$case_dir/p" --binary "$bad_nick"
assert_eq "1" "$(file_of "$case_dir/rc")" "--version de duas linhas: sai com 1"
assert_eq "Erro: o binário $bad_nick não respondeu ao --version como um nick (uma linha começando com 'nick '). Nada foi instalado." "$(file_of "$case_dir/err")" "--version de duas linhas: a mensagem"
assert_not_exists "$case_dir/p" "--version de duas linhas: nada gravado"
printf '#!/usr/bin/env bash\necho "outro 1.0"\n' > "$bad_nick"
run_install "$case_dir" install --prefix "$case_dir/p2" --binary "$bad_nick"
assert_eq "1" "$(file_of "$case_dir/rc")" "--version sem 'nick ': sai com 1"
assert_not_exists "$case_dir/p2" "--version sem 'nick ': nada gravado"

echo "== install sem --binary: o cargo build =="
case_dir="$TEST_TMP/cargo"
RUN_ENV=(PATH="$FAKE_CARGO_BIN:$CASE_PATH" FAKE_CARGO_LOG="$case_dir/cargo.log" CARGO_TARGET_DIR="$case_dir/target")
run_install "$case_dir" install --prefix "$case_dir/p"
RUN_ENV=()
assert_eq "0" "$(file_of "$case_dir/rc")" "com o cargo: sai com 0"
root_physical="$(cd "$ROOT_DIR" && pwd -P)"
assert_eq "build --release --locked --manifest-path $ROOT_DIR/nick/Cargo.toml|$root_physical" "$(file_of "$case_dir/cargo.log")" "com o cargo: cargo build --release --locked --manifest-path <raiz>/nick/Cargo.toml, com o cwd na raiz"
assert_eq "true" "$(cmp -s "$case_dir/target/release/nick" "$case_dir/p/bin/nick" && echo true || echo false)" "com o cargo: instala o binário de \$CARGO_TARGET_DIR/release/nick"

# Sem CARGO_TARGET_DIR, o alvo é <raiz>/nick/target: numa cópia do que o script usa, para não
# gravar no nick/target/ do repositório.
case_dir="$TEST_TMP/cargo_default_target"
fake_root="$case_dir/repo"
mkdir -p "$fake_root/auto_scrum" "$fake_root/nick"
cp -r "$ROOT_DIR/install" "$ROOT_DIR/bin" "$ROOT_DIR/lib" "$fake_root/"
cp -r "$ROOT_DIR/auto_scrum/prompt.sh" "$ROOT_DIR/auto_scrum/templates" "$fake_root/auto_scrum/"
mkdir -p "$case_dir/cwd" "$case_dir/home"
rc=0
(cd "$case_dir/cwd" && env -u CARGO_TARGET_DIR HOME="$case_dir/home" \
  PATH="$FAKE_CARGO_BIN:$CASE_PATH" FAKE_CARGO_LOG="$case_dir/cargo.log" \
  "$BASH_BIN" "$fake_root/install/install.sh" install --prefix "$case_dir/p" \
  > "$case_dir/out" 2> "$case_dir/err") || rc=$?
assert_eq "0" "$rc" "sem CARGO_TARGET_DIR: sai com 0"
assert_exists "$fake_root/nick/target/release/nick" "sem CARGO_TARGET_DIR: o cargo falso gravou em <raiz>/nick/target"
assert_eq "true" "$(cmp -s "$fake_root/nick/target/release/nick" "$case_dir/p/bin/nick" && echo true || echo false)" "sem CARGO_TARGET_DIR: instala <raiz>/nick/target/release/nick"

case_dir="$TEST_TMP/no_cargo"
run_install "$case_dir" install --prefix "$case_dir/p"
assert_eq "1" "$(file_of "$case_dir/rc")" "sem cargo: sai com 1"
assert_eq "Erro: o cargo não foi encontrado no PATH. O make install precisa dele: instale o Rust (rustup) e rode de novo." "$(file_of "$case_dir/err")" "sem cargo: a mensagem"
assert_not_exists "$case_dir/p" "sem cargo: nada gravado"

case_dir="$TEST_TMP/cargo_fails"
RUN_ENV=(PATH="$FAKE_CARGO_BIN:$CASE_PATH" FAKE_CARGO_LOG="$case_dir/cargo.log" CARGO_TARGET_DIR="$case_dir/target" FAKE_CARGO_FAIL=1)
run_install "$case_dir" install --prefix "$case_dir/p"
RUN_ENV=()
assert_eq "true" "$([ "$(file_of "$case_dir/rc")" != "0" ] && echo true || echo false)" "cargo falhando: sai com código ≠ 0"
assert_contains "$(file_of "$case_dir/err")" "erro de compilação falso" "cargo falhando: a saída do cargo aparece"
assert_not_exists "$case_dir/p" "cargo falhando: nada gravado"

case_dir="$TEST_TMP/relative_target"
RUN_ENV=(PATH="$FAKE_CARGO_BIN:$CASE_PATH" FAKE_CARGO_LOG="$case_dir/cargo.log" CARGO_TARGET_DIR="target-rel")
run_install "$case_dir" install --prefix "$case_dir/p"
RUN_ENV=()
assert_eq "1" "$(file_of "$case_dir/rc")" "CARGO_TARGET_DIR relativo: sai com 1"
assert_eq "Erro: CARGO_TARGET_DIR precisa ser um caminho absoluto: target-rel." "$(file_of "$case_dir/err")" "CARGO_TARGET_DIR relativo: a mensagem"
assert_not_exists "$case_dir/cargo.log" "CARGO_TARGET_DIR relativo: o cargo nem roda"
assert_not_exists "$case_dir/p" "CARGO_TARGET_DIR relativo: nada gravado"

echo "== install: o aviso do PATH =="
case_dir="$TEST_TMP/path_hint"
run_install "$case_dir" install --prefix "$case_dir/p" --binary "$FAKE_NICK"
out="$(file_of "$case_dir/out")"
assert_contains "$out" "$case_dir/p/bin não está no PATH" "fora do PATH: o aviso"
assert_contains "$out" "export PATH=\"$case_dir/p/bin:\$PATH\"" "fora do PATH: a linha do bash/zsh, com o prefixo real"
# shellcheck disable=SC2088
# The ~ is the literal text of the message (where to put the line), not a path to expand.
assert_contains "$out" "~/.bashrc" "fora do PATH: cita o ~/.bashrc"
# shellcheck disable=SC2088
# Same as above.
assert_contains "$out" "~/.zshrc" "fora do PATH: cita o ~/.zshrc"
assert_contains "$out" "fish_add_path $case_dir/p/bin" "fora do PATH: a linha do fish"
case_dir="$TEST_TMP/path_ok"
RUN_ENV=(PATH="$CASE_PATH:$case_dir/p/bin")
run_install "$case_dir" install --prefix "$case_dir/p" --binary "$FAKE_NICK"
RUN_ENV=()
assert_eq "0" "$(file_of "$case_dir/rc")" "dentro do PATH: sai com 0"
assert_not_contains "$(file_of "$case_dir/out")" "não está no PATH" "dentro do PATH: sem o aviso"
RUN_ENV=(PATH="$CASE_PATH:$case_dir/p/bin/")
run_install "$case_dir" install --prefix "$case_dir/p/" --binary "$FAKE_NICK"
RUN_ENV=()
assert_not_contains "$(file_of "$case_dir/out")" "não está no PATH" "dentro do PATH com barra no fim: sem o aviso"

echo "== uninstall: remove o manifesto e mantém o estado =="
case_dir="$TEST_TMP/uninstall"
prefix="$case_dir/p"
state="$case_dir/state/nick"
mkdir -p "$state/agents" "$state/logs/proj-1" "$prefix/bin" "$prefix/share/outro"
echo "banco" > "$state/nick.db"
echo "trava" > "$state/agents/x.lock"
echo "log" > "$state/logs/proj-1/po_1.md"
echo "alheio" > "$prefix/bin/outro-programa"
echo "alheio" > "$prefix/share/outro/arquivo"
state_before="$(cd "$state" && find . -type f -exec cat {} + ; find . | LC_ALL=C sort)"
run_install "$case_dir" install --prefix "$prefix" --binary "$FAKE_NICK"
run_install "$case_dir" uninstall --prefix "$prefix"
assert_eq "0" "$(file_of "$case_dir/rc")" "uninstall: sai com 0"
assert_eq "nick desinstalado de $prefix. O banco e os logs em $state foram mantidos." "$(file_of "$case_dir/out")" "uninstall: a mensagem final, com o state dir"
assert_eq "" "$(file_of "$case_dir/err")" "uninstall: nada no stderr"
assert_not_exists "$prefix/bin/nick" "uninstall: o binário saiu"
assert_not_exists "$prefix/share/nick" "uninstall: share/nick saiu inteiro (vazio)"
assert_exists "$prefix/bin/outro-programa" "uninstall: um programa alheio em bin/ fica"
assert_exists "$prefix/share/outro/arquivo" "uninstall: outro diretório de share/ fica"
assert_exists "$prefix/share" "uninstall: share/ fica (rmdir só até share/nick)"
assert_eq "$state_before" "$(cd "$state" && find . -type f -exec cat {} + ; find . | LC_ALL=C sort)" "uninstall: banco, agents/ e logs/ do state dir continuam iguais"

echo "== uninstall: um arquivo alheio dentro de share/nick sobrevive =="
case_dir="$TEST_TMP/uninstall_extra"
prefix="$case_dir/p"
run_install "$case_dir" install --prefix "$prefix" --binary "$FAKE_NICK"
mkdir -p "$prefix/share/nick/lib"
echo "meu" > "$prefix/share/nick/lib/meu-arquivo"
run_install "$case_dir" uninstall --prefix "$prefix"
assert_eq "0" "$(file_of "$case_dir/rc")" "com arquivo alheio: sai com 0"
assert_exists "$prefix/share/nick/lib/meu-arquivo" "o arquivo alheio fica"
assert_contains "$(file_of "$case_dir/out")" "Mantive $prefix/share/nick/lib: tem arquivos que não foram instalados pelo nick." "o aviso do diretório mantido"
assert_not_contains "$(file_of "$case_dir/out")" "Mantive $prefix/share/nick:" "sem aviso do pai, que só tem diretórios"
assert_not_exists "$prefix/share/nick/lib/config" "os subdiretórios vazios saem"
assert_not_exists "$prefix/share/nick/.nick-install" "o manifesto sai"
assert_eq "meu-arquivo" "$(ls -A "$prefix/share/nick/lib")" "só o alheio em lib/"

echo "== uninstall: um arquivo alheio numa pasta que o nick não criou também avisa =="
case_dir="$TEST_TMP/uninstall_extra_dir"
prefix="$case_dir/p"
run_install "$case_dir" install --prefix "$prefix" --binary "$FAKE_NICK"
mkdir -p "$prefix/share/nick/extra"
echo "meu" > "$prefix/share/nick/extra/meu"
run_install "$case_dir" uninstall --prefix "$prefix"
assert_eq "0" "$(file_of "$case_dir/rc")" "pasta alheia: sai com 0"
assert_exists "$prefix/share/nick/extra/meu" "pasta alheia: o arquivo fica"
assert_contains "$(file_of "$case_dir/out")" "Mantive $prefix/share/nick: tem arquivos que não foram instalados pelo nick." "pasta alheia: o aviso cita share/nick, que ficou por causa dela"
assert_eq "extra" "$(ls -A "$prefix/share/nick")" "pasta alheia: só ela sobra em share/nick/"

echo "== uninstall: sem manifesto, nada é feito =="
case_dir="$TEST_TMP/uninstall_none"
prefix="$case_dir/p"
mkdir -p "$prefix/bin"
echo "outro nick" > "$prefix/bin/nick"
run_install "$case_dir" uninstall --prefix "$prefix"
assert_eq "0" "$(file_of "$case_dir/rc")" "sem manifesto: sai com 0"
assert_eq "Não há instalação do nick em $prefix. Nada foi feito." "$(file_of "$case_dir/out")" "sem manifesto: a mensagem"
assert_eq "outro nick" "$(file_of "$prefix/bin/nick")" "sem manifesto: um bin/nick que não é nosso fica"
run_install "$case_dir" uninstall --prefix "$prefix"
assert_eq "0" "$(file_of "$case_dir/rc")" "sem manifesto: de novo, sai com 0 (idempotente)"

echo "== uninstall: um manifesto com caminho de fora é recusado =="
case_dir="$TEST_TMP/uninstall_bad"
echo "alvo" > "$TEST_TMP/alvo"
# Cada linha ruim numa instalação nova, num prefixo novo: o caso não depende do anterior.
n=0
for bad in "../../alvo" "share/nick/../../../../alvo" "/etc/passwd" "share/outro/x"; do
  n=$((n + 1))
  prefix="$case_dir/p$n"
  run_install "$case_dir" install --prefix "$prefix" --binary "$FAKE_NICK"
  echo "$bad" >> "$prefix/share/nick/.nick-install"
  run_install "$case_dir" uninstall --prefix "$prefix"
  assert_eq "1" "$(file_of "$case_dir/rc")" "manifesto com '$bad': sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "$bad" "manifesto com '$bad': diz a linha"
  assert_exists "$prefix/bin/nick" "manifesto com '$bad': nada removido"
  assert_exists "$TEST_TMP/alvo" "manifesto com '$bad': o alvo de fora continua lá"
done

echo "== install por cima: o arquivo que saiu da lista não fica órfão =="
case_dir="$TEST_TMP/reinstall"
prefix="$case_dir/p"
run_install "$case_dir" install --prefix "$prefix" --binary "$FAKE_NICK"
mkdir -p "$prefix/share/nick/lib/config"
echo "velho" > "$prefix/share/nick/lib/config/velho.sh"
echo "share/nick/lib/config/velho.sh" >> "$prefix/share/nick/.nick-install"
run_install "$case_dir" install --prefix "$prefix" --binary "$FAKE_NICK"
assert_eq "0" "$(file_of "$case_dir/rc")" "reinstalar: sai com 0"
assert_not_exists "$prefix/share/nick/lib/config/velho.sh" "reinstalar: o arquivo só do manifesto antigo saiu"
assert_eq "$(expected_tree)" "$(tree_of "$prefix")" "reinstalar: a árvore é a da regra"
assert_not_contains "$(file_of "$case_dir/out")" "Mantive" "reinstalar: sem aviso de diretório mantido"

echo "== nunca chama o sudo =="
assert_not_exists "$SUDO_MARKER" "o sudo falso nunca foi chamado em nenhum caso"
assert_eq "" "$(code_lines "$INSTALL_SH" | grep -w sudo || true)" "o install.sh não tem sudo no código"
assert_eq "" "$(code_lines "$INSTALL_SH" | grep -E 'rm -[a-zA-Z]*r' || true)" "o install.sh não usa rm -r"

echo "== Makefile: install e uninstall chamam o script =="
make_n() {
  (cd "$ROOT_DIR" && env -u MAKEFLAGS -u MFLAGS -u MAKELEVEL make -n "$@" 2>&1) || true
}
assert_eq "bash install/install.sh install --prefix \"/x\"" "$(make_n install PREFIX=/x)" "make -n install PREFIX=/x"
assert_eq "bash install/install.sh uninstall --prefix \"/x\"" "$(make_n uninstall PREFIX=/x)" "make -n uninstall PREFIX=/x"
assert_eq "bash install/install.sh install --prefix \"$TEST_TMP/h/.local\"" "$(HOME="$TEST_TMP/h" make_n install)" "make -n install: PREFIX padrão é \$HOME/.local"
assert_eq "bash tests/run_all.sh" "$(make_n)" "make sem alvo continua rodando o test (não instala)"
makefile_text="$(file_of "$MAKEFILE")"
assert_eq "true" "$(grep -qx '\.PHONY: install uninstall' <<< "$makefile_text" && echo true || echo false)" "o Makefile declara .PHONY: install uninstall"

echo "== estrutural: o install.sh e esta suíte =="
assert_eq "#!/usr/bin/env bash" "$(head -n 1 "$INSTALL_SH")" "install.sh começa com o shebang"
assert_eq "set -euo pipefail" "$(code_lines "$INSTALL_SH" | grep -v '^$' | sed -n 1p)" "a 1ª linha de código do install.sh é set -euo pipefail"
assert_eq "true" "$([ -x "$INSTALL_SH" ] && [ -x "${BASH_SOURCE[0]}" ] && echo true || echo false)" "install.sh e a suíte têm bit de execução"
assert_eq "" "$(code_lines "$INSTALL_SH" | grep -F '[[' || true)" "install.sh usa [ ], não [[ ]]"
if command -v shellcheck > /dev/null 2>&1; then
  assert_eq "" "$(cd "$ROOT_DIR" && shellcheck -x install/install.sh install/tests/test_install.sh 2>&1 || true)" "shellcheck -x do install.sh e da suíte"
else
  echo "  FAIL: o shellcheck não foi encontrado no PATH"
  failures=$((failures + 1))
fi

echo
if [ "$failures" -eq 0 ]; then
  echo "Todos os testes passaram."
else
  echo "$failures teste(s) falharam."
  exit 1
fi
