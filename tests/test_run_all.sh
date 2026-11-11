#!/usr/bin/env bash
#
# test_run_all: testes em bash puro (sem framework) do runner de suítes do repositório
# (tests/run_all.sh) e do Makefile da raiz. Nunca roda o make test do repositório real (seria
# recursão: esta suíte é achada por ele). Cada caso monta, dentro do TEST_TMP, um repositório
# de mentira com cópias do Makefile e do tests/run_all.sh reais e suítes falsas de poucas
# linhas, que gravam o próprio caminho no log do caso (FAKE_RUN_LOG) e saem com o rc pedido.
# Todo make roda sem MAKEFLAGS/MFLAGS/MAKELEVEL, pra não herdar o -j e o jobserver do make que
# eventualmente roda esta suíte.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
MAKEFILE="$ROOT_DIR/Makefile"
RUN_ALL_SH="$SCRIPT_DIR/run_all.sh"

failures=0

# Diretório-base temporário único da suíte: cada teste usa um subdiretório dele, e nada é
# escrito dentro do repositório.
TEST_TMP="$(mktemp -d)"
trap 'chmod -R u+rwx "$TEST_TMP" 2>/dev/null || true; rm -rf "$TEST_TMP"' EXIT

# shellcheck source=SCRIPTDIR/../lib/tests/assert.sh
source "$SCRIPT_DIR/../lib/tests/assert.sh"

# code_lines <file>: imprime as linhas de <file> que não são comentário de linha inteira.
code_lines() {
  grep -vE '^[[:space:]]*#' "$1" || true
}

# file_of <path>: imprime o conteúdo de <path>, ou "sem arquivo" se ele não existir.
file_of() {
  cat "$1" 2>/dev/null || printf 'sem arquivo'
}

# new_fixture <name>: cria (vazio) e imprime o diretório de um caso, com o repositório de
# mentira em <dir>/repo/ (só cópias do Makefile e do tests/run_all.sh reais) e o log das
# suítes falsas em <dir>/log (vazio). A cópia não derruba a suíte se um dos dois não existir:
# os casos é que falham.
new_fixture() {
  local dir="$TEST_TMP/$1"
  rm -rf "$dir"
  mkdir -p "$dir/repo/tests"
  cp "$MAKEFILE" "$dir/repo/Makefile" 2>/dev/null || true
  cp "$RUN_ALL_SH" "$dir/repo/tests/run_all.sh" 2>/dev/null || true
  : > "$dir/log"
  printf '%s' "$dir"
}

# fake_suite <repo> <rel_path> <rc> [body]: grava em <repo>/<rel_path> (criando os
# diretórios) uma suíte falsa que acrescenta "./<rel_path>" ao FAKE_RUN_LOG, roda [body] e sai
# com <rc>. Sem bit de execução, de propósito: prova que o runner roda com bash, e não ./.
fake_suite() {
  local repo="$1" rel="$2" rc="$3" body="${4:-}"
  mkdir -p "$repo/$(dirname "$rel")"
  {
    printf '%s\n' '#!/usr/bin/env bash'
    # shellcheck disable=SC2016 # o $FAKE_RUN_LOG é texto da suíte falsa, expandido quando ela roda
    printf 'printf "%%s\\n" %q >> "$FAKE_RUN_LOG"\n' "./$rel"
    printf '%s\n' "$body"
    printf 'exit %s\n' "$rc"
  } > "$repo/$rel"
}

# run_make <case_dir> [make args...]: roda make -C <case_dir>/repo [make args...], sem as
# variáveis que um make de fora passaria ao de dentro. stdout em <case_dir>/out, stderr em
# <case_dir>/err e o código de saída em <case_dir>/rc. O '|| t_rc=$?' mantém a suíte rodando
# (e reportando FAIL) mesmo se o make falhar.
run_make() {
  local case_dir="$1" t_rc=0
  shift
  env -u MAKEFLAGS -u MFLAGS -u MAKELEVEL FAKE_RUN_LOG="$case_dir/log" \
    timeout 20 make -C "$case_dir/repo" "$@" < /dev/null > "$case_dir/out" 2> "$case_dir/err" || t_rc=$?
  printf '%s' "$t_rc" > "$case_dir/rc"
}

# run_runner <case_dir> [cwd]: roda bash <case_dir>/repo/tests/run_all.sh direto, sem make,
# com o cwd em [cwd] (padrão: a raiz do repositório de mentira). Saídas como em run_make.
run_runner() {
  local case_dir="$1" cwd="${2:-$1/repo}" t_rc=0
  (cd "$cwd" && FAKE_RUN_LOG="$case_dir/log" timeout 20 bash "$case_dir/repo/tests/run_all.sh") \
    < /dev/null > "$case_dir/out" 2> "$case_dir/err" || t_rc=$?
  printf '%s' "$t_rc" > "$case_dir/rc"
}

echo "== esqueleto: o Makefile e o tests/run_all.sh existem =="
assert_exists "$MAKEFILE" "existe o Makefile na raiz"
assert_exists "$RUN_ALL_SH" "existe o tests/run_all.sh"

echo
echo "== make test com uma suíte que passa =="
case_dir="$(new_fixture one_ok)"
fake_suite "$case_dir/repo" "a/tests/test_ok.sh" 0
run_make "$case_dir" test
assert_eq "0" "$(file_of "$case_dir/rc")" "make test sai com 0"
assert_eq "./a/tests/test_ok.sh" "$(file_of "$case_dir/log")" "a suíte rodou (uma vez)"
assert_contains "$(file_of "$case_dir/out")" "Todas as 1 suítes passaram." "a saída traz a mensagem de sucesso"

echo
echo "== descoberta: test_*.sh direto em qualquer tests/, em ordem LC_ALL=C =="
case_dir="$(new_fixture order)"
fake_suite "$case_dir/repo" "a/tests/test_ok.sh" 0
fake_suite "$case_dir/repo" "b/c/tests/test_ok2.sh" 0
fake_suite "$case_dir/repo" "B/tests/test_upper.sh" 0
fake_suite "$case_dir/repo" "tests/test_root.sh" 0
run_make "$case_dir" test
assert_eq "0" "$(file_of "$case_dir/rc")" "make test sai com 0"
# Em LC_ALL=C, maiúscula vem antes de minúscula ("./B" < "./a"); num locale pt_BR/en_US seria
# o contrário, então a ordem prova o LC_ALL=C.
assert_eq "./B/tests/test_upper.sh
./a/tests/test_ok.sh
./b/c/tests/test_ok2.sh
./tests/test_root.sh" "$(file_of "$case_dir/log")" "as quatro suítes rodaram, uma vez cada, em ordem LC_ALL=C (inclusive a de tests/ na raiz e a de b/c/tests/)"
assert_contains "$(file_of "$case_dir/out")" "Todas as 4 suítes passaram." "a mensagem de sucesso conta as quatro"

# Uma suíte nova, criada depois, roda sem mudar o Makefile.
fake_suite "$case_dir/repo" "d/tests/test_new.sh" 0
: > "$case_dir/log"
run_make "$case_dir" test
assert_eq "0" "$(file_of "$case_dir/rc")" "suíte nova: make test sai com 0"
assert_contains "$(file_of "$case_dir/log")" "./d/tests/test_new.sh" "suíte nova: roda sem mudar o Makefile"
assert_eq "5" "$(wc -l < "$case_dir/log")" "suíte nova: as cinco suítes rodaram"
assert_eq "" "$(cmp "$MAKEFILE" "$case_dir/repo/Makefile" 2>&1 || echo diferente)" "suíte nova: o Makefile da fixture é igual ao real (não foi mudado)"

echo
echo "== descoberta: o que não casa com a regra não roda =="
case_dir="$(new_fixture excluded)"
fake_suite "$case_dir/repo" "a/tests/test_ok.sh" 0
# Cada um, se rodasse, gravaria o próprio caminho no log.
excluded_paths=(
  "lib/tests/assert.sh"
  "a/tests/helper.sh"
  "a/test_fora.sh"
  "test_raiz.sh"
  "a/tests/sub/test_fundo.sh"
  "a/tests/test_dir/x.sh"
  "a/tests/test_dir/test_x.sh"
  ".nick/tests/test_oculto.sh"
  ".nick/docs/27/teste_9_2c.sh"
  ".nick/docs/27/tests/test_doc.sh"
  ".git/tests/test_git.sh"
  "x/.oculto/tests/test_fundo.sh"
)
for excluded_path in "${excluded_paths[@]}"; do
  fake_suite "$case_dir/repo" "$excluded_path" 0
done
# Um diretório com nome de suíte não é suíte.
mkdir -p "$case_dir/repo/a/tests/test_d.sh"
run_make "$case_dir" test
assert_eq "0" "$(file_of "$case_dir/rc")" "make test sai com 0"
assert_eq "./a/tests/test_ok.sh" "$(file_of "$case_dir/log")" "só a/tests/test_ok.sh rodou"
for excluded_path in "${excluded_paths[@]}"; do
  assert_not_contains "$(file_of "$case_dir/log")" "./$excluded_path" "não roda $excluded_path"
done
assert_contains "$(file_of "$case_dir/out")" "Todas as 1 suítes passaram." "o diretório a/tests/test_d.sh/ não conta como suíte"
# Controle positivo: rodados à mão, os excluídos gravam no log, então o "não roda" acima morde.
: > "$case_dir/control_log"
for excluded_path in "${excluded_paths[@]}"; do
  FAKE_RUN_LOG="$case_dir/control_log" bash "$case_dir/repo/$excluded_path" < /dev/null || true
done
assert_eq "${#excluded_paths[@]}" "$(wc -l < "$case_dir/control_log")" "controle positivo: cada excluído, rodado à mão, grava no log"

echo
echo "== descoberta: caminho com espaço =="
case_dir="$(new_fixture space)"
fake_suite "$case_dir/repo" "com espaco/tests/test_ok.sh" 0
run_make "$case_dir" test
assert_eq "0" "$(file_of "$case_dir/rc")" "make test sai com 0"
assert_eq "./com espaco/tests/test_ok.sh" "$(file_of "$case_dir/log")" "a suíte em 'com espaco/tests/' rodou"

echo
echo "== descoberta: nenhuma suíte achada é falha =="
case_dir="$(new_fixture empty)"
fake_suite "$case_dir/repo" "a/tests/helper.sh" 0
run_runner "$case_dir"
assert_eq "1" "$(file_of "$case_dir/rc")" "bash tests/run_all.sh sai com 1"
assert_contains "$(file_of "$case_dir/err")" "Nenhuma suíte de teste encontrada" "a mensagem de erro vai pro stderr"
assert_not_contains "$(file_of "$case_dir/out")" "passaram" "sem mensagem de sucesso"
assert_eq "" "$(file_of "$case_dir/log")" "nada rodou"
run_make "$case_dir" test
assert_eq "true" "$([ "$(file_of "$case_dir/rc")" != "0" ] && echo true || echo false)" "make test sai com código ≠ 0"

echo
echo "== descoberta: busca incompleta (diretório ilegível) é falha, sem rodar nada =="
case_dir="$(new_fixture unreadable)"
fake_suite "$case_dir/repo" "a/tests/test_ok.sh" 0
fake_suite "$case_dir/repo" "z/tests/test_z.sh" 0
chmod 000 "$case_dir/repo/z"
if [ -r "$case_dir/repo/z" ]; then
  # Como root, o chmod 000 não impede a leitura: o caso não tem como montar a falha.
  echo "  (pulado: o diretório continua legível com chmod 000, ex: rodando como root)"
else
  run_runner "$case_dir"
  assert_eq "1" "$(file_of "$case_dir/rc")" "bash tests/run_all.sh sai com 1"
  assert_contains "$(file_of "$case_dir/err")" "Não deu pra procurar as suítes de teste" "a mensagem de erro vai pro stderr"
  assert_eq "" "$(file_of "$case_dir/log")" "nenhuma suíte rodou (nem a legível)"
  assert_not_contains "$(file_of "$case_dir/out")" "passaram" "sem mensagem de sucesso"
fi
chmod 755 "$case_dir/repo/z"

echo
echo "== falha: todas as suítes rodam, e o resumo lista as que falharam =="
case_dir="$(new_fixture failures)"
fake_suite "$case_dir/repo" "a/tests/test_fail.sh" 1
fake_suite "$case_dir/repo" "b/tests/test_ok.sh" 0
fake_suite "$case_dir/repo" "c/tests/test_fail2.sh" 3
run_make "$case_dir" test
assert_eq "true" "$([ "$(file_of "$case_dir/rc")" != "0" ] && echo true || echo false)" "make test sai com código ≠ 0"
assert_eq "./a/tests/test_fail.sh
./b/tests/test_ok.sh
./c/tests/test_fail2.sh" "$(file_of "$case_dir/log")" "as três suítes rodaram, na ordem, apesar da falha da primeira"
fail_out="$(file_of "$case_dir/out")"
assert_contains "$fail_out" "2 de 3 suítes falharam:" "o resumo conta as que falharam e o total"
assert_contains "$fail_out" "  - ./a/tests/test_fail.sh" "o resumo lista ./a/tests/test_fail.sh"
assert_contains "$fail_out" "  - ./c/tests/test_fail2.sh" "o resumo lista ./c/tests/test_fail2.sh"
assert_not_contains "$fail_out" "  - ./b/tests/test_ok.sh" "o resumo não lista ./b/tests/test_ok.sh, que passou"
assert_not_contains "$fail_out" "passaram" "sem mensagem de sucesso"
: > "$case_dir/log"
run_runner "$case_dir"
assert_eq "1" "$(file_of "$case_dir/rc")" "bash tests/run_all.sh sai com 1"
assert_eq "3" "$(wc -l < "$case_dir/log")" "bash tests/run_all.sh: as três suítes rodaram"

echo
echo "== falha: suíte que morre no meio com set -e conta como falha =="
case_dir="$(new_fixture errexit)"
fake_suite "$case_dir/repo" "a/tests/test_dies.sh" 0 "set -e; false"
fake_suite "$case_dir/repo" "b/tests/test_ok.sh" 0
run_runner "$case_dir"
assert_eq "1" "$(file_of "$case_dir/rc")" "bash tests/run_all.sh sai com 1"
assert_eq "./a/tests/test_dies.sh
./b/tests/test_ok.sh" "$(file_of "$case_dir/log")" "as duas rodaram"
assert_contains "$(file_of "$case_dir/out")" "  - ./a/tests/test_dies.sh" "o resumo lista a que morreu no meio"
assert_contains "$(file_of "$case_dir/out")" "1 de 2 suítes falharam:" "o resumo conta 1 de 2"

echo
echo "== cwd: cada suíte roda com o cwd na raiz, de qualquer cwd de quem chama =="
case_dir="$(new_fixture cwd)"
# shellcheck disable=SC2016 # o $PWD é texto da suíte falsa, expandido quando ela roda
fake_suite "$case_dir/repo" "a/b/tests/test_pwd.sh" 0 'printf "pwd=%s\n" "$(pwd -P)" >> "$FAKE_RUN_LOG"'
repo_real="$(cd "$case_dir/repo" && pwd -P)"
run_make "$case_dir" test
assert_contains "$(file_of "$case_dir/log")" "pwd=$repo_real" "make -C <raiz> test: a suíte roda com o cwd na raiz"
: > "$case_dir/log"
mkdir -p "$case_dir/elsewhere"
run_runner "$case_dir" "$case_dir/elsewhere"
assert_eq "0" "$(file_of "$case_dir/rc")" "bash <raiz>/tests/run_all.sh de outro cwd sai com 0"
assert_contains "$(file_of "$case_dir/log")" "pwd=$repo_real" "bash <raiz>/tests/run_all.sh de outro cwd: a suíte roda com o cwd na raiz"

echo
echo "== saída: cada suíte aparece ao vivo, depois de um cabeçalho com o caminho dela =="
case_dir="$(new_fixture output)"
fake_suite "$case_dir/repo" "a/tests/test_ok.sh" 0 'echo "saida-da-suite-a"; echo "erro-da-suite-a" >&2'
run_runner "$case_dir"
assert_eq "true" "$(grep -A1 -F './a/tests/test_ok.sh' "$case_dir/out" | grep -qx 'saida-da-suite-a' && echo true || echo false)" "o stdout da suíte vem logo depois do cabeçalho com o caminho"
assert_contains "$(file_of "$case_dir/err")" "erro-da-suite-a" "o stderr da suíte não é engolido"

echo
echo "== Makefile: make sem alvo roda o test =="
case_dir="$(new_fixture default_goal)"
fake_suite "$case_dir/repo" "a/tests/test_ok.sh" 0
fake_suite "$case_dir/repo" "b/tests/test_ok.sh" 0
run_make "$case_dir" test
with_target_log="$(file_of "$case_dir/log")"
: > "$case_dir/log"
run_make "$case_dir"
assert_eq "0" "$(file_of "$case_dir/rc")" "make (sem alvo) sai com 0"
assert_eq "$with_target_log" "$(file_of "$case_dir/log")" "make (sem alvo) roda as mesmas suítes do make test"
assert_contains "$(file_of "$case_dir/out")" "Todas as 2 suítes passaram." "make (sem alvo) traz a mensagem de sucesso"

echo
echo "== Makefile: um arquivo chamado test na raiz não impede o make test (.PHONY) =="
case_dir="$(new_fixture phony)"
fake_suite "$case_dir/repo" "a/tests/test_ok.sh" 0
: > "$case_dir/repo/test"
run_make "$case_dir" test
assert_eq "0" "$(file_of "$case_dir/rc")" "com o arquivo test: make test sai com 0"
assert_eq "./a/tests/test_ok.sh" "$(file_of "$case_dir/log")" "com o arquivo test: a suíte rodou"
# Controle positivo: sem o .PHONY, o mesmo arquivo faz o make achar o alvo em dia, e nada roda.
grep -v '^\.PHONY:' "$MAKEFILE" > "$case_dir/repo/Makefile"
: > "$case_dir/log"
run_make "$case_dir" test
assert_eq "" "$(file_of "$case_dir/log")" "controle positivo: sem o .PHONY, o arquivo test impede o make test de rodar as suítes"

echo
echo "== Makefile: make -j4 test roda as suítes uma de cada vez =="
# Cada suíte falsa pega uma trava (mkdir é atômico): se outra estiver rodando ao mesmo tempo, o
# mkdir falha e ela grava CONCORRENTE no log.
# shellcheck disable=SC2016 # o $FAKE_RUN_LOG é texto da suíte falsa, expandido quando ela roda
lock_body='if ! mkdir "$FAKE_RUN_LOG.lock" 2> /dev/null; then echo CONCORRENTE >> "$FAKE_RUN_LOG"; fi; sleep 0.2; rmdir "$FAKE_RUN_LOG.lock" 2> /dev/null || true'
case_dir="$(new_fixture parallel)"
for lock_suite in a b c; do
  fake_suite "$case_dir/repo" "$lock_suite/tests/test_lock.sh" 0 "$lock_body"
done
for lock_round in 1 2 3; do
  : > "$case_dir/log"
  run_make "$case_dir" -j4 test
  assert_eq "0" "$(file_of "$case_dir/rc")" "rodada $lock_round: make -j4 test sai com 0"
  assert_eq "./a/tests/test_lock.sh
./b/tests/test_lock.sh
./c/tests/test_lock.sh" "$(file_of "$case_dir/log")" "rodada $lock_round: as três rodaram, sem CONCORRENTE no log"
done
# Controle positivo: as três rodadas ao mesmo tempo, à mão, a trava acusa a concorrência.
: > "$case_dir/log"
for lock_suite in a b c; do
  FAKE_RUN_LOG="$case_dir/log" bash "$case_dir/repo/$lock_suite/tests/test_lock.sh" < /dev/null &
done
wait
assert_contains "$(file_of "$case_dir/log")" "CONCORRENTE" "controle positivo: em paralelo, a trava grava CONCORRENTE"

echo
echo "== estrutural: o Makefile e o tests/run_all.sh reais =="
makefile_text="$(file_of "$MAKEFILE")"
assert_eq "true" "$(grep -qx '\.PHONY: test' <<< "$makefile_text" && echo true || echo false)" "o Makefile declara .PHONY: test"
assert_eq "true" "$(grep -qx '\.NOTPARALLEL:' <<< "$makefile_text" && echo true || echo false)" "o Makefile tem .NOTPARALLEL:"
assert_eq "true" "$(grep -qx '\.DEFAULT_GOAL := test' <<< "$makefile_text" && echo true || echo false)" "o Makefile tem .DEFAULT_GOAL := test"
assert_eq "true" "$(grep -qx '	@bash tests/run_all.sh' <<< "$makefile_text" && echo true || echo false)" "a receita do test só chama bash tests/run_all.sh"
assert_eq "1" "$(grep -c '^	' <<< "$makefile_text" || true)" "o Makefile tem uma linha de receita só (a lógica fica no script)"
assert_eq "#!/usr/bin/env bash" "$(head -n 1 "$RUN_ALL_SH")" "tests/run_all.sh começa com o shebang #!/usr/bin/env bash"
assert_eq "set -euo pipefail" "$(code_lines "$RUN_ALL_SH" | grep -m 1 -v '^$')" "a 1ª linha de código do tests/run_all.sh é set -euo pipefail"
assert_eq "true" "$([ -x "$RUN_ALL_SH" ] && [ -x "$SCRIPT_DIR/test_run_all.sh" ] && echo true || echo false)" "tests/run_all.sh e tests/test_run_all.sh têm bit de execução"
assert_eq "" "$(grep -n '\[\[' <<< "$(code_lines "$RUN_ALL_SH")" || true)" "tests/run_all.sh usa [ ], não [[ ]]"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
