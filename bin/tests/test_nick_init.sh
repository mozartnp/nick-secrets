#!/usr/bin/env bash
#
# test_nick_init: testes em bash puro (sem framework) do bin/nick-init, o comando do operador
# que cria a configuração do nick (.nick/ e .nick/nick.conf) no cwd. Roda o script de ponta a
# ponta, com bash "$NICK_INIT" e direto ("$NICK_INIT", que prova o bit de execução), sempre
# com o cwd num diretório temporário: nada é escrito no repositório.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
NICK_INIT="$ROOT_DIR/bin/nick-init"
CONFIG_SH="$ROOT_DIR/lib/config/config.sh"
KEYS_SH="$ROOT_DIR/lib/config/keys.sh"

failures=0

# Diretório-base temporário único da suíte: cada teste usa um subdiretório dele, e nada é
# escrito dentro do repositório.
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

# shellcheck source=SCRIPTDIR/../../lib/tests/assert.sh
source "$ROOT_DIR/lib/tests/assert.sh"

# code_lines <file>: imprime as linhas de <file> que não são comentário de linha inteira.
code_lines() {
  grep -vE '^[[:space:]]*#' "$1" || true
}

# run_init <cwd> <stdin> <out_prefix> [mode] [script] [arg...]: roda o nick-init ([script],
# padrão: $NICK_INIT) com o cwd em <cwd>, stdout em <out_prefix>.out e stderr em
# <out_prefix>.err. [mode] "bash" (padrão) roda com bash <script>; "direto" roda <script>
# direto, pelo shebang. <stdin> vazio vira < /dev/null (EOF já na primeira leitura). Imprime o
# código de saída.
run_init() {
  local cwd="$1" input="$2" out_prefix="$3" mode="${4:-bash}" script="${5:-$NICK_INIT}" rc=0
  local cmd=()
  shift 5 2>/dev/null || shift "$#"
  if [ "$mode" = "direto" ]; then
    cmd=("$script" "$@")
  else
    cmd=(bash "$script" "$@")
  fi
  if [ -z "$input" ]; then
    (cd "$cwd" && timeout 5 "${cmd[@]}" < /dev/null > "$out_prefix.out" 2> "$out_prefix.err") || rc=$?
  else
    (cd "$cwd" && timeout 5 "${cmd[@]}" <<< "$input" > "$out_prefix.out" 2> "$out_prefix.err") || rc=$?
  fi
  printf '%s' "$rc"
}

# nick_tree <dir>: lista (find | sort, locale C) o que existe em <dir>/.nick, com caminhos
# relativos a <dir>. Sem .nick, imprime vazio.
nick_tree() {
  (cd "$1" && [ -e .nick ] && find .nick | LC_ALL=C sort) || true
}

# init_tree_expected: a árvore de .nick/ que o nick-init deixa.
init_tree_expected() {
  printf '%s\n' .nick .nick/.gitignore .nick/docs .nick/nick.conf .nick/troca
}

# nick_gitignore_expected: o .nick/.gitignore que o nick cria, escrito à mão (três linhas).
nick_gitignore_expected() {
  printf '%s\n' '*' '!.gitignore' '!nick.conf'
}

# init_expected_output <cwd>: o stdout exato do nick-init quando ele cria o arquivo.
init_expected_output() {
  cat <<EOF
Projeto criado em $1/.nick/nick.conf — edite antes de usar.
Estrutura do nick em .nick/:
  .gitignore   versionado: ignora tudo em .nick/ menos ele e o nick.conf
  nick.conf    versionado: configuração do projeto
  troca/       não versionada: troca de arquivos com os agentes
  docs/        não versionada: plano, review e prints de cada ticket (docs/<ID>/)
Versione .nick/nick.conf e .nick/.gitignore junto com o projeto.
EOF
}

# init_legacy_warning <cwd>: o aviso exato do .nick.conf antigo.
init_legacy_warning() {
  printf '%s' "Aviso: o arquivo antigo $1/.nick.conf não foi lido nem alterado — copie os valores dele para $1/.nick/nick.conf e apague o antigo."
}

# O esqueleto esperado: as 3 linhas de cabeçalho citam o nick (fixadas aqui à mão) e o corpo é
# o do registro de lib/config/keys.sh (fixado byte a byte em lib/tests/test_keys.sh).
skeleton_expected="$TEST_TMP/skeleton_expected.conf"
{
  printf '%s\n' \
    '# Config do projeto pro nick. Preencha as variáveis abaixo e salve.' \
    '# Não é executado: o nick lê só linhas CHAVE="valor" (aspas duplas, sem aspas dentro do valor nem comentário no fim da linha) das chaves abaixo.' \
    '# Mantenha todas as chaves abaixo, mesmo as que não usar (o nick avisa quando falta alguma). Nas chaves *_ENABLED, use só "true", "false" ou "".'
  timeout 2 bash -c "source '$CONFIG_SH'; source '$KEYS_SH'; config_register_nick_keys; config_print_skeleton corpo | tail -n +4" 2>/dev/null || true
} > "$skeleton_expected"

echo "== estrutura: shebang, set -euo pipefail, bit de execução e só as duas bibliotecas de configuração =="
assert_eq "#!/usr/bin/env bash" "$(head -n1 "$NICK_INIT" 2>/dev/null || true)" "a 1ª linha é o shebang #!/usr/bin/env bash"
assert_eq "set -euo pipefail" "$(code_lines "$NICK_INIT" 2>/dev/null | grep -v '^$' | head -n1 || true)" "a 1ª linha de código é set -euo pipefail"
assert_eq "executável" "$([ -x "$NICK_INIT" ] && echo executável || echo 'sem bit de execução ou inexistente')" "bin/nick-init tem bit de execução"
# shellcheck disable=SC2016
# Aspas simples intencionais: '$SCRIPT_DIR' é o texto literal buscado no código-fonte.
assert_eq 'source "$SCRIPT_DIR/../lib/config/config.sh"'$'\n''source "$SCRIPT_DIR/../lib/config/keys.sh"' "$(grep -E '^[[:space:]]*(source|\.)[[:space:]]' "$NICK_INIT" 2>/dev/null || true)" "as únicas linhas source/. são as de lib/config/config.sh e lib/config/keys.sh, nessa ordem"
for init_lib in config/config.sh config/keys.sh; do
  # shellcheck disable=SC2016
  init_source_number="$(grep -nxF "source \"\$SCRIPT_DIR/../lib/$init_lib\"" "$NICK_INIT" 2>/dev/null | cut -d: -f1 || true)"
  assert_eq "# shellcheck source=SCRIPTDIR/../lib/$init_lib" "$( [ -n "$init_source_number" ] && sed -n "$((init_source_number - 1))p" "$NICK_INIT" || true)" "a linha acima do source de lib/$init_lib é a diretiva '# shellcheck source=SCRIPTDIR/../lib/$init_lib'"
done
init_code="$(code_lines "$NICK_INIT" 2>/dev/null || true)"
assert_eq "" "$(grep -n 'auto_scrum' <<< "$init_code" || true)" "bin/nick-init não cita auto_scrum fora de comentário"
assert_eq "" "$(grep -nE 'lib/jail|lib/claude|jail\.sh|claude\.sh' <<< "$init_code" || true)" "bin/nick-init não carrega lib/jail/ nem lib/claude/"
assert_eq "" "$(grep -nwE 'claude|config_load' <<< "$init_code" || true)" "bin/nick-init não chama o claude nem o leitor da configuração"
assert_eq "1" "$(grep -cx 'config_register_nick_keys' "$NICK_INIT" 2>/dev/null || true)" "bin/nick-init registra as chaves do nick (config_register_nick_keys), uma vez, na coluna 0"
assert_eq "1" "$(grep -c 'config_init "nick"' <<< "$init_code" || true)" "bin/nick-init chama config_init \"nick\""
assert_eq "0" "$(timeout 2 bash -c "source '$NICK_INIT' < /dev/null; declare -F main >/dev/null" < /dev/null > "$TEST_TMP/source_guard.out" 2>&1; echo $?)" "source do bin/nick-init não roda main() (guard BASH_SOURCE[0] = \$0)"
assert_eq "" "$(cat "$TEST_TMP/source_guard.out" 2>/dev/null || echo 'sem saída gravada')" "source do bin/nick-init não imprime nada nem cria a configuração"

echo
echo "== caso feliz: cria .nick/ e .nick/nick.conf no cwd (com bash e direto, pelo shebang) =="
for init_mode in bash direto; do
  init_dir="$TEST_TMP/happy_$init_mode"
  mkdir -p "$init_dir/cwd"
  init_pwd="$(cd "$init_dir/cwd" && pwd)"
  assert_eq "0" "$(run_init "$init_dir/cwd" "" "$init_dir/run" "$init_mode")" "$init_mode: termina com código 0"
  assert_eq "$(init_tree_expected)" "$(nick_tree "$init_dir/cwd")" "$init_mode: cria exatamente .nick/.gitignore, .nick/nick.conf, .nick/troca/ e .nick/docs/"
  assert_eq "$(nick_gitignore_expected)" "$(cat "$init_dir/cwd/.nick/.gitignore" 2>/dev/null || echo 'sem .gitignore')" "$init_mode: .nick/.gitignore tem exatamente *, !.gitignore e !nick.conf"
  assert_eq "true" "$([ -d "$init_dir/cwd/.nick/troca" ] && [ -d "$init_dir/cwd/.nick/docs" ] && echo true || echo false)" "$init_mode: .nick/troca/ e .nick/docs/ são diretórios"
  assert_eq "$(init_expected_output "$init_pwd")" "$(cat "$init_dir/run.out" 2>/dev/null || true)" "$init_mode: stdout exato (caminho do nick.conf e a árvore de .nick/)"
  assert_eq "" "$(cat "$init_dir/run.err" 2>/dev/null || echo 'sem stderr gravado')" "$init_mode: nada no stderr"
  assert_eq "igual" "$(cmp -s "$skeleton_expected" "$init_dir/cwd/.nick/nick.conf" && echo igual || echo diferente)" "$init_mode: .nick/nick.conf é o esqueleto de antes, só com o cabeçalho citando o nick (byte a byte)"
  assert_eq "" "$(find "$init_dir/cwd" -mindepth 1 -not -path "$init_dir/cwd/.nick*" 2>/dev/null)" "$init_mode: nada é criado no cwd fora de .nick/"
done
assert_eq "3" "$(head -n3 "$skeleton_expected" | grep -c 'nick' || true)" "amostra: as 3 linhas de cabeçalho esperadas citam o nick"
assert_eq "true" "$([ "$(wc -l < "$skeleton_expected")" -gt 10 ] && echo true || echo false)" "amostra: o esqueleto esperado tem o corpo (não vazio)"
assert_eq "0" "$(grep -c 'auto_scrum' <(head -n3 "$TEST_TMP/happy_bash/cwd/.nick/nick.conf" 2>/dev/null) || true)" "o cabeçalho do nick.conf não cita mais o auto_scrum"

echo
echo "== esqueleto do nick-init: carrega com config_load sem nenhum aviso =="
load_file="$TEST_TMP/happy_bash/cwd/.nick/nick.conf"
load_rc=0
load_out="$(timeout 2 bash -c "set -euo pipefail; source '$CONFIG_SH'; source '$KEYS_SH'; config_register_nick_keys; config_load '$load_file'; printf '%s|%s|%s' \"\$TDD_ENABLED\" \"\$JAIL_ENABLED\" \"\$STACK_DESCRIPTION\"" 2> "$TEST_TMP/load.err")" || load_rc=$?
assert_eq "0" "$load_rc" "config_load do nick.conf gerado retorna 0"
assert_eq "true|false|" "$load_out" "config_load do nick.conf gerado: as chaves ficam com o padrão (TDD_ENABLED \"true\", JAIL_ENABLED \"false\", STACK_DESCRIPTION vazio)"
assert_eq "" "$(cat "$TEST_TMP/load.err" 2>/dev/null || echo 'sem stderr gravado')" "config_load do nick.conf gerado: nenhum aviso"

echo
echo "== não depende do auto_scrum: numa cópia só com bin/ e lib/, funciona igual =="
copy_root="$TEST_TMP/copy_root"
mkdir -p "$copy_root" "$TEST_TMP/copy_run/cwd"
cp -R "$ROOT_DIR/bin" "$ROOT_DIR/lib" "$copy_root/"
assert_not_exists "$copy_root/auto_scrum" "a cópia não tem auto_scrum/"
copy_pwd="$(cd "$TEST_TMP/copy_run/cwd" && pwd)"
assert_eq "0" "$(run_init "$TEST_TMP/copy_run/cwd" "" "$TEST_TMP/copy_run/run" direto "$copy_root/bin/nick-init")" "cópia sem auto_scrum/: termina com código 0"
assert_eq "$(init_tree_expected)" "$(nick_tree "$TEST_TMP/copy_run/cwd")" "cópia sem auto_scrum/: a mesma árvore"
assert_eq "igual" "$(cmp -s "$skeleton_expected" "$TEST_TMP/copy_run/cwd/.nick/nick.conf" && echo igual || echo diferente)" "cópia sem auto_scrum/: o mesmo nick.conf"
assert_eq "$(init_expected_output "$copy_pwd")" "$(cat "$TEST_TMP/copy_run/run.out" 2>/dev/null || true)" "cópia sem auto_scrum/: o mesmo stdout"
assert_eq "" "$(cat "$TEST_TMP/copy_run/run.err" 2>/dev/null || echo 'sem stderr gravado')" "cópia sem auto_scrum/: nada no stderr"

echo
echo "== sobrescrita: pergunta antes; N cancela (0), Ctrl+D cancela (1), s substitui e completa =="
ow_dir="$TEST_TMP/overwrite"
mkdir -p "$ow_dir/cwd/.nick"
printf '%s\n' 'STACK_DESCRIPTION="antigo"' > "$ow_dir/cwd/.nick/nick.conf"
ow_pwd="$(cd "$ow_dir/cwd" && pwd)"
assert_eq "0" "$(run_init "$ow_dir/cwd" "N" "$ow_dir/no")" "resposta N: termina com código 0"
assert_eq "Cancelado. Nada foi alterado." "$(cat "$ow_dir/no.out" 2>/dev/null || true)" "resposta N: stdout é só 'Cancelado. Nada foi alterado.'"
assert_eq "" "$(cat "$ow_dir/no.err" 2>/dev/null || echo 'sem stderr gravado')" "resposta N (nick.conf incompleto): nada no stderr (nenhum aviso)"
assert_eq 'STACK_DESCRIPTION="antigo"' "$(cat "$ow_dir/cwd/.nick/nick.conf")" "resposta N: arquivo inalterado"
assert_eq "$(printf '%s\n' .nick .nick/nick.conf)" "$(nick_tree "$ow_dir/cwd")" "resposta N: nada é criado nem completado"
assert_eq "1" "$(run_init "$ow_dir/cwd" "" "$ow_dir/eof")" "stdin fecha na pergunta (Ctrl+D): termina com código 1"
assert_eq "Cancelado. Nada foi alterado." "$(cat "$ow_dir/eof.err" 2>/dev/null || true)" "Ctrl+D: 'Cancelado. Nada foi alterado.' no stderr"
assert_eq "" "$(cat "$ow_dir/eof.out" 2>/dev/null || echo 'sem stdout gravado')" "Ctrl+D: nada no stdout"
assert_eq 'STACK_DESCRIPTION="antigo"' "$(cat "$ow_dir/cwd/.nick/nick.conf")" "Ctrl+D: arquivo inalterado"
assert_eq "$(printf '%s\n' .nick .nick/nick.conf)" "$(nick_tree "$ow_dir/cwd")" "Ctrl+D: nada é criado nem completado"
assert_eq "0" "$(run_init "$ow_dir/cwd" "s" "$ow_dir/yes")" "resposta s: termina com código 0"
assert_eq "igual" "$(cmp -s "$skeleton_expected" "$ow_dir/cwd/.nick/nick.conf" && echo igual || echo diferente)" "resposta s: o arquivo vira o esqueleto"
assert_eq "$(init_tree_expected)" "$(nick_tree "$ow_dir/cwd")" "resposta s: a árvore de .nick/ é completada"
assert_eq "$(nick_gitignore_expected)" "$(cat "$ow_dir/cwd/.nick/.gitignore" 2>/dev/null || echo 'sem .gitignore')" "resposta s: .nick/.gitignore com o conteúdo exato"
assert_eq "$(init_expected_output "$ow_pwd")" "$(cat "$ow_dir/yes.out" 2>/dev/null || true)" "resposta s: stdout exato"

echo
echo "== NS-53: a pergunta da JIRA_URL, depois da sobrescrita, grava a resposta no nick.conf =="
# jira_url_expected <valor>: o esqueleto esperado com JIRA_URL="<valor>".
jira_url_expected() {
  sed "s#^JIRA_URL=\"\"\$#JIRA_URL=\"$1\"#" "$skeleton_expected"
}
assert_eq "1" "$(grep -cx 'JIRA_URL=""' "$skeleton_expected" || true)" "amostra: o esqueleto esperado tem JIRA_URL=\"\""
ju_dir="$TEST_TMP/jira_url"
mkdir -p "$ju_dir/fresh"
assert_eq "0" "$(run_init "$ju_dir/fresh" "https://j/browse/" "$ju_dir/fresh_run")" "cwd vazio, URL no stdin: termina com código 0"
jira_url_expected "https://j/browse/" > "$ju_dir/fresh.expected"
assert_eq "igual" "$(cmp -s "$ju_dir/fresh.expected" "$ju_dir/fresh/.nick/nick.conf" && echo igual || echo diferente)" "cwd vazio, URL no stdin: grava JIRA_URL=\"https://j/browse/\" e o resto é o esqueleto"
assert_eq "$(init_expected_output "$(cd "$ju_dir/fresh" && pwd)")" "$(cat "$ju_dir/fresh_run.out" 2>/dev/null || true)" "cwd vazio, URL no stdin: stdout de sempre"
assert_eq "" "$(cat "$ju_dir/fresh_run.err" 2>/dev/null || echo 'sem stderr gravado')" "cwd vazio, URL no stdin: nada no stderr"
ju_load_rc=0
ju_load_out="$(timeout 2 bash -c "set -euo pipefail; source '$CONFIG_SH'; source '$KEYS_SH'; config_register_nick_keys; config_load '$ju_dir/fresh/.nick/nick.conf'; printf '%s' \"\$JIRA_URL\"" 2> "$ju_dir/load.err")" || ju_load_rc=$?
assert_eq "0|https://j/browse/" "$ju_load_rc|$ju_load_out" "o nick.conf com a URL carrega com o valor"
assert_eq "" "$(cat "$ju_dir/load.err" 2>/dev/null || echo 'sem stderr gravado')" "o nick.conf com a URL carrega sem aviso"
mkdir -p "$ju_dir/over/.nick"
printf '%s\n' 'STACK_DESCRIPTION="antigo"' > "$ju_dir/over/.nick/nick.conf"
assert_eq "0" "$(run_init "$ju_dir/over" $'s\nhttps://j/browse/' "$ju_dir/over_run")" "nick.conf existente, s e depois a URL: termina com código 0"
assert_eq "igual" "$(cmp -s "$ju_dir/fresh.expected" "$ju_dir/over/.nick/nick.conf" && echo igual || echo diferente)" "nick.conf existente, s e depois a URL: sobrescreve com a URL"
printf '%s\n' 'STACK_DESCRIPTION="antigo"' > "$ju_dir/over/.nick/nick.conf"
assert_eq "0" "$(run_init "$ju_dir/over" $'N\nhttps://j/browse/' "$ju_dir/over_no")" "nick.conf existente, N: termina com código 0"
assert_eq 'STACK_DESCRIPTION="antigo"' "$(cat "$ju_dir/over/.nick/nick.conf")" "nick.conf existente, N: nada muda (a pergunta da URL não vem)"
mkdir -p "$ju_dir/quote"
assert_eq "0" "$(run_init "$ju_dir/quote" $'https://j/"x\nhttps://ok/' "$ju_dir/quote_run")" "resposta com aspas, depois uma válida: termina com código 0"
assert_eq 'JIRA_URL="https://ok/"' "$(grep '^JIRA_URL=' "$ju_dir/quote/.nick/nick.conf" 2>/dev/null || true)" "resposta com aspas: recusada, e a seguinte é gravada"
assert_eq 'Erro: a resposta não pode ter aspas duplas (") nem caracteres de controle. Tente de novo.' "$(cat "$ju_dir/quote_run.err" 2>/dev/null || true)" "resposta com aspas: a recusa no stderr, sem ecoar o valor"
assert_eq "" "$(grep -nw 'JIRA_URL' <<< "$init_code" || true)" "bin/nick-init não tem código específico da JIRA_URL (a pergunta vem do registro)"

echo
echo "== .nick/ existente: .gitignore editado e conteúdo de troca/ e docs/ ficam intactos =="
ed_dir="$TEST_TMP/edited"
mkdir -p "$ed_dir/cwd/.nick/troca" "$ed_dir/cwd/.nick/docs/SC-1"
printf '%s\n' '*' '!.gitignore' '!nick.conf' '!docs/' '!docs/**' > "$ed_dir/cwd/.nick/.gitignore"
cp "$ed_dir/cwd/.nick/.gitignore" "$ed_dir/gitignore.before"
printf 'print antiga' > "$ed_dir/cwd/.nick/troca/prova.png"
printf 'plano antigo' > "$ed_dir/cwd/.nick/docs/SC-1/plan.md"
assert_eq "0" "$(run_init "$ed_dir/cwd" "" "$ed_dir/run")" ".nick/.gitignore editado: termina com código 0"
assert_eq "igual" "$(cmp -s "$ed_dir/gitignore.before" "$ed_dir/cwd/.nick/.gitignore" && echo igual || echo diferente)" ".nick/.gitignore editado: fica idêntico byte a byte"
assert_eq "igual" "$(cmp -s "$skeleton_expected" "$ed_dir/cwd/.nick/nick.conf" && echo igual || echo diferente)" ".nick/.gitignore editado: o nick.conf é criado"
assert_eq "print antiga|plano antigo" "$(cat "$ed_dir/cwd/.nick/troca/prova.png" 2>/dev/null || true)|$(cat "$ed_dir/cwd/.nick/docs/SC-1/plan.md" 2>/dev/null || true)" ".nick/ existente: o conteúdo de troca/ e docs/ fica intacto"
assert_not_exists "$ed_dir/cwd/.nick/troca/.gitignore" ".nick/ existente: nenhum .gitignore é criado em troca/"

echo
echo "== .nick.conf antigo: não para, avisa (só ele; os dois com s), nada com N =="
lg_dir="$TEST_TMP/legacy"
mkdir -p "$lg_dir/cwd"
printf '%s\n' 'STACK_DESCRIPTION="antigo"' > "$lg_dir/cwd/.nick.conf"
lg_pwd="$(cd "$lg_dir/cwd" && pwd)"
assert_eq "0" "$(run_init "$lg_dir/cwd" "" "$lg_dir/only")" "só o .nick.conf antigo: termina com código 0"
assert_eq "$(init_tree_expected)" "$(nick_tree "$lg_dir/cwd")" "só o .nick.conf antigo: cria a árvore inteira de .nick/"
assert_eq "igual" "$(cmp -s "$skeleton_expected" "$lg_dir/cwd/.nick/nick.conf" && echo igual || echo diferente)" "só o .nick.conf antigo: .nick/nick.conf é o esqueleto"
assert_eq "$(init_legacy_warning "$lg_pwd")" "$(cat "$lg_dir/only.err" 2>/dev/null || true)" "só o .nick.conf antigo: o stderr é só o aviso exato"
assert_eq "$(init_expected_output "$lg_pwd")" "$(cat "$lg_dir/only.out" 2>/dev/null || true)" "só o .nick.conf antigo: stdout exato, como sem o legado"
assert_eq 'STACK_DESCRIPTION="antigo"' "$(cat "$lg_dir/cwd/.nick.conf")" "só o .nick.conf antigo: o arquivo antigo fica intacto"
mkdir -p "$lg_dir/cwd/.nick"
printf '%s\n' 'STACK_DESCRIPTION="novo"' > "$lg_dir/cwd/.nick/nick.conf"
assert_eq "0" "$(run_init "$lg_dir/cwd" "N" "$lg_dir/both_no")" "os dois arquivos, resposta N: termina com código 0"
assert_eq "" "$(cat "$lg_dir/both_no.err" 2>/dev/null || echo 'sem stderr gravado')" "os dois arquivos, resposta N: nenhum aviso"
assert_eq 'STACK_DESCRIPTION="novo"' "$(cat "$lg_dir/cwd/.nick/nick.conf")" "os dois arquivos, resposta N: .nick/nick.conf inalterado"
assert_eq "0" "$(run_init "$lg_dir/cwd" "s" "$lg_dir/both_yes")" "os dois arquivos, resposta s: termina com código 0"
assert_eq "$(init_legacy_warning "$lg_pwd")" "$(cat "$lg_dir/both_yes.err" 2>/dev/null || true)" "os dois arquivos, resposta s: avisa do arquivo antigo"
assert_eq "igual" "$(cmp -s "$skeleton_expected" "$lg_dir/cwd/.nick/nick.conf" && echo igual || echo diferente)" "os dois arquivos, resposta s: .nick/nick.conf vira o esqueleto"
assert_eq 'STACK_DESCRIPTION="antigo"' "$(cat "$lg_dir/cwd/.nick.conf")" "os dois arquivos: o arquivo antigo fica intacto"

echo
echo "== .nick/troca sendo um arquivo: erro em PT-BR, código ≠ 0, sem gravar o nick.conf =="
bl_dir="$TEST_TMP/blocked"
mkdir -p "$bl_dir/cwd/.nick"
printf 'arquivo no lugar da pasta' > "$bl_dir/cwd/.nick/troca"
assert_eq "1" "$(run_init "$bl_dir/cwd" "" "$bl_dir/run")" ".nick/troca sendo um arquivo: termina com código 1"
assert_eq "Erro: não foi possível criar a estrutura .nick/ (.gitignore, troca/ e docs/) em $(cd "$bl_dir/cwd" && pwd)." "$(cat "$bl_dir/run.err" 2>/dev/null || true)" ".nick/troca sendo um arquivo: o stderr é só a mensagem exata"
assert_eq "" "$(cat "$bl_dir/run.out" 2>/dev/null || echo 'sem stdout gravado')" ".nick/troca sendo um arquivo: nada no stdout"
assert_not_exists "$bl_dir/cwd/.nick/nick.conf" ".nick/troca sendo um arquivo: o nick.conf não é gravado"
assert_eq "arquivo no lugar da pasta" "$(cat "$bl_dir/cwd/.nick/troca" 2>/dev/null || true)" ".nick/troca sendo um arquivo: o arquivo fica intacto"

echo
echo "== argumentos: qualquer um é recusado (código 1, uso no stderr, nada criado) =="
arg_case=0
for init_arg in --init x --projeto=alpha ""; do
  arg_case=$((arg_case + 1))
  arg_dir="$TEST_TMP/args_$arg_case"
  mkdir -p "$arg_dir/cwd"
  assert_eq "1" "$(run_init "$arg_dir/cwd" "s" "$arg_dir/run" bash "$NICK_INIT" "$init_arg")" "argumento '$init_arg': termina com código 1"
  assert_eq "Erro: argumento desconhecido: '$init_arg'"$'\n'"Uso: nick-init (rode na raiz do projeto)" "$(cat "$arg_dir/run.err" 2>/dev/null || true)" "argumento '$init_arg': stderr com o erro e o uso, exatos"
  assert_eq "" "$(cat "$arg_dir/run.out" 2>/dev/null || echo 'sem stdout gravado')" "argumento '$init_arg': nada no stdout"
  assert_eq "" "$(find "$arg_dir/cwd" -mindepth 1 2>/dev/null)" "argumento '$init_arg': nada é criado no cwd"
done
arg_dir="$TEST_TMP/args_two"
mkdir -p "$arg_dir/cwd"
assert_eq "1" "$(run_init "$arg_dir/cwd" "" "$arg_dir/run" direto "$NICK_INIT" a b)" "dois argumentos, rodando direto: termina com código 1"
assert_eq "Erro: argumento desconhecido: 'a'"$'\n'"Uso: nick-init (rode na raiz do projeto)" "$(cat "$arg_dir/run.err" 2>/dev/null || true)" "dois argumentos: o erro cita o primeiro"
assert_eq "" "$(find "$arg_dir/cwd" -mindepth 1 2>/dev/null)" "dois argumentos: nada é criado no cwd"

echo
echo "== shellcheck -x no bin/nick-init e nesta suíte =="
if command -v shellcheck > /dev/null 2>&1; then
  for checked in "$NICK_INIT" "$SCRIPT_DIR/test_nick_init.sh"; do
    sc_rc=0
    sc_out="$(shellcheck -x "$checked" 2>&1)" || sc_rc=$?
    assert_eq "0" "$sc_rc" "shellcheck -x limpo: ${checked#"$ROOT_DIR"/}${sc_out:+ — $sc_out}"
  done
else
  echo "  PULADO: shellcheck não instalado"
fi

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
