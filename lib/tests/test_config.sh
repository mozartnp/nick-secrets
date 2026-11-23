#!/usr/bin/env bash
#
# test_config: testes em bash puro (sem framework) da biblioteca lib/config/config.sh. Cada caso roda
# num bash -c separado, que dá source na biblioteca e registra só chaves de mentira (FAKE_*):
# nenhuma chave de ferramenta real aparece aqui. A integração com o auto_scrum fica em
# auto_scrum/tests/test_auto_scrum.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_SH="$SCRIPT_DIR/../config/config.sh"

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

# Chaves de mentira, registradas na ordem abaixo por FAKE_KEYS_SH:
#   - FAKE_TEXT: texto, padrão "", sem nota;
#   - FAKE_ENABLED: liga/desliga, padrão "false", sem nota;
#   - FAKE_NOTE_ENABLED: liga/desliga, padrão "false", com nota;
#   - FAKE_ON_ENABLED: liga/desliga, padrão "true", com nota e comentário de duas linhas.
fake_note='a opção de mentira está desligada. Use "true" para ligá-la.'
fake_on_note='a opção de mentira continua ligada. Use "false" para desligá-la.'
FAKE_KEYS_SH="$TEST_TMP/fake_keys.sh"
cat > "$FAKE_KEYS_SH" <<EOF
source '$CONFIG_SH'
config_register_key FAKE_TEXT "" "" 'FAKE_TEXT: texto livre de mentira.'
config_register_key FAKE_ENABLED "false" "" 'FAKE_ENABLED: liga/desliga de mentira.'
config_register_key FAKE_NOTE_ENABLED "false" '$fake_note' 'FAKE_NOTE_ENABLED: liga/desliga de mentira, com nota.'
config_register_key FAKE_ON_ENABLED "true" '$fake_on_note' 'FAKE_ON_ENABLED: liga/desliga de mentira, ligada por padrão.
Ao contrário das outras, vazio a mantém ligada.'
EOF

# run_fake <commands>: num bash separado, com set -euo pipefail, dá source em FAKE_KEYS_SH e
# roda <commands>. O '|| true' mantém a suíte rodando (e reportando FAIL) mesmo se o bash de
# dentro morrer, ex: função ainda inexistente.
run_fake() {
  timeout 2 bash -c "set -euo pipefail; source '$FAKE_KEYS_SH'; $1" 2>/dev/null || true
}

# run_register <err_file> <arg>...: num bash SEM set -e, com as chaves de mentira já
# registradas, chama config_register_key <arg>... (stderr em <err_file>) e imprime
# "<código>|<CONFIG_KEYS>|<tamanho dos quatro arrays>|depois". O "depois" só sai se o shell
# continuou depois da chamada.
run_register() {
  local err_file="$1" call
  shift
  printf -v call '%q ' config_register_key "$@"
  timeout 2 bash -c "source '$FAKE_KEYS_SH'; $call 2>'$err_file'; t_rc=\$?; printf '%s|%s|%s %s %s %s|depois' \"\$t_rc\" \"\${CONFIG_KEYS[*]}\" \"\${#CONFIG_KEYS[@]}\" \"\${#CONFIG_KEY_DEFAULTS[@]}\" \"\${#CONFIG_KEY_NOTES[@]}\" \"\${#CONFIG_KEY_COMMENTS[@]}\"" 2>/dev/null || true
}

# run_load <config_file> <out_prefix> [setup] [shell_options]: num bash separado, com
# [shell_options] (padrão: set -euo pipefail; ":" roda sem set -e), dá source em FAKE_KEYS_SH,
# roda [setup] (padrão: nada), chama config_load <config_file> com stdout em
# <out_prefix>.out, stderr em <out_prefix>.err e o código de saída em <out_prefix>.rc, e
# imprime as quatro chaves de mentira separadas por '|' (chave não atribuída sai vazia). Os
# valores só saem se o shell continuou depois da chamada.
run_load() {
  local config_file="$1" out_prefix="$2" setup="${3:-:}" shell_options="${4:-set -euo pipefail}"
  timeout 2 bash -c "$shell_options; source '$FAKE_KEYS_SH'; $setup; t_rc=0; config_load '$config_file' >'$out_prefix.out' 2>'$out_prefix.err' || t_rc=\$?; printf '%s' \"\$t_rc\" > '$out_prefix.rc'; printf '%s|%s|%s|%s' \"\${FAKE_TEXT-}\" \"\${FAKE_ENABLED-}\" \"\${FAKE_NOTE_ENABLED-}\" \"\${FAKE_ON_ENABLED-}\"" 2>/dev/null || true
}

# fake_keys_blank [except_key]: imprime uma linha CHAVE="" pra cada chave de mentira, menos
# <except_key>. Monta fixtures completas, que não disparam o aviso de chave ausente.
fake_keys_blank() {
  local except_key="${1:-}" key
  for key in FAKE_TEXT FAKE_ENABLED FAKE_NOTE_ENABLED FAKE_ON_ENABLED; do
    [ "$key" = "$except_key" ] || printf '%s=""\n' "$key"
  done
}

# Fim padrão do aviso de valor inválido, pra chave sem nota própria.
default_off_note="a opção está desligada."

# malformed_warning <arquivo:linha> <chave>: imprime, sem \n final, o texto exato do aviso de
# formato.
malformed_warning() {
  printf '%s' "Aviso: $1: linha de $2 ignorada (formato esperado: $2=\"valor\", sem aspas duplas dentro do valor)"
}

# value_warning <arquivo:linha> <chave> <valor> <fim>: imprime, sem \n final, o texto exato
# do aviso de valor inválido.
value_warning() {
  printf '%s' "Aviso: $1: valor \"$3\" inválido para $2 (valores aceitos: \"true\", \"false\" ou \"\") — $4"
}

# missing_warning <arquivo> [linha...]: imprime, sem \n final, o texto exato do aviso de
# chave ausente pra <arquivo>, com uma linha "  <linha>" pra cada argumento.
missing_warning() {
  local file="$1" line
  shift
  printf '%s' "Aviso: $file não tem todas as chaves da configuração — cada chave ausente funciona como o valor sugerido abaixo. Adicione ao arquivo:"
  for line in "$@"; do
    printf '\n  %s' "$line"
  done
}

# count_warnings <arquivo>: imprime quantas linhas de <arquivo> começam com "Aviso:".
count_warnings() {
  grep -c '^Aviso:' "$1" 2>/dev/null || true
}

echo "== carga limpa: source só define nomes config_/CONFIG_, sem imprimir nada =="
clean_dir="$TEST_TMP/clean"
mkdir -p "$clean_dir"
clean_rc=0
# Só redirecionamentos entre os retratos, pra o próprio teste não criar variável nenhuma.
timeout 2 bash --norc --noprofile -c "compgen -v | sort > '$clean_dir/vars_before'; declare -F | sort > '$clean_dir/funcs_before'; source '$CONFIG_SH' > '$clean_dir/out' 2>&1; compgen -v | sort > '$clean_dir/vars_after'; declare -F | sort > '$clean_dir/funcs_after'" < /dev/null || clean_rc=$?
assert_eq "0" "$clean_rc" "source lib/config/config.sh num bash limpo termina com código 0"
assert_eq "" "$(cat "$clean_dir/out" 2>/dev/null || echo 'sem saída gravada')" "source lib/config/config.sh não imprime nada (stdout nem stderr)"
# Variáveis que o próprio bash cria ou mexe entre os retratos (não são da biblioteca).
clean_bash_vars='_|BASH_.*|COLUMNS|LINES|FUNCNAME|LINENO|PIPESTATUS'
clean_new_vars="$(comm -13 "$clean_dir/vars_before" "$clean_dir/vars_after" 2>/dev/null | grep -vxE "$clean_bash_vars" || true)"
clean_new_funcs="$(comm -13 "$clean_dir/funcs_before" "$clean_dir/funcs_after" 2>/dev/null | sed 's/^declare -f //' || true)"
# Num terminal com tamanho, o bash atualiza COLUMNS e LINES (checkwinsize) depois de cada comando
# externo, como o sort do 1º retrato: elas apareceriam como novas sem a biblioteca criar nada.
# O mesmo retrato, num pseudo-terminal 80x24 (script, do util-linux), dá as mesmas variáveis novas.
if script -qec true /dev/null > /dev/null 2>&1; then
  mkdir -p "$clean_dir/tty"
  script -qec "stty cols 80 rows 24; timeout 2 bash --norc --noprofile -c \"compgen -v | sort > '$clean_dir/tty/vars_before'; source '$CONFIG_SH' > /dev/null 2>&1; compgen -v | sort > '$clean_dir/tty/vars_after'\"" /dev/null < /dev/null > /dev/null 2>&1 || true
  clean_tty_new_vars="$(comm -13 "$clean_dir/tty/vars_before" "$clean_dir/tty/vars_after" 2>/dev/null | grep -vxE "$clean_bash_vars" || true)"
  assert_eq "$clean_new_vars" "$clean_tty_new_vars" "num terminal 80x24: as mesmas variáveis novas (COLUMNS e LINES são do bash)"
else
  echo "  PULADO: sem o script do util-linux pra simular um terminal"
fi
assert_contains "$clean_new_vars" "CONFIG_KEYS" "source lib/config/config.sh cria CONFIG_KEYS"
assert_eq "" "$(grep -vE '^CONFIG_' <<< "$clean_new_vars" || true)" "toda variável nova começa com CONFIG_"
assert_eq "" "$(grep -vE '^config_' <<< "$clean_new_funcs" | grep . || true)" "toda função nova começa com config_"
clean_arrays="$(timeout 2 bash --norc --noprofile -c "source '$CONFIG_SH'; printf '%s|%s|%s|%s|%s|%s|%s|%s|%s' \"\$CONFIG_NICK_DIR_NAME\" \"\$CONFIG_EXCHANGE_DIR_NAME\" \"\$CONFIG_DOCS_DIR_NAME\" \"\$CONFIG_CWD_FILE_NAME\" \"\$CONFIG_CWD_LEGACY_FILE_NAME\" \"\${#CONFIG_KEYS[@]}\" \"\${#CONFIG_KEY_DEFAULTS[@]}\" \"\${#CONFIG_KEY_NOTES[@]}\" \"\${#CONFIG_KEY_COMMENTS[@]}\"" 2>/dev/null || true)"
assert_eq ".nick|.nick/troca|.nick/docs|.nick/nick.conf|.nick.conf|0|0|0|0" "$clean_arrays" "CONFIG_NICK_DIR_NAME é .nick, CONFIG_EXCHANGE_DIR_NAME é .nick/troca, CONFIG_DOCS_DIR_NAME é .nick/docs, CONFIG_CWD_FILE_NAME é .nick/nick.conf, CONFIG_CWD_LEGACY_FILE_NAME é .nick.conf e os quatro arrays do registro nascem vazios"
assert_eq "$(cut -d'|' -f1 <<< "$clean_arrays")/nick.conf" "$(cut -d'|' -f4 <<< "$clean_arrays")" "consistência: CONFIG_CWD_FILE_NAME = \$CONFIG_NICK_DIR_NAME/nick.conf"
assert_eq "$(cut -d'|' -f1 <<< "$clean_arrays")/troca|$(cut -d'|' -f1 <<< "$clean_arrays")/docs" "$(cut -d'|' -f2-3 <<< "$clean_arrays")" "consistência: CONFIG_EXCHANGE_DIR_NAME e CONFIG_DOCS_DIR_NAME ficam dentro de CONFIG_NICK_DIR_NAME"

echo
echo "== carga limpa: estrutura do arquivo (sem shebang, sem bit de execução, sem set/exit/source/eval) =="
assert_eq "# shellcheck shell=bash" "$(head -n1 "$CONFIG_SH" 2>/dev/null || true)" "a 1ª linha é a diretiva '# shellcheck shell=bash' (sem shebang)"
assert_eq "sem bit de execução" "$([ -e "$CONFIG_SH" ] && [ ! -x "$CONFIG_SH" ] && echo 'sem bit de execução' || echo 'executável ou inexistente')" "lib/config/config.sh existe e não tem bit de execução"
config_code="$(code_lines "$CONFIG_SH" 2>/dev/null || true)"
assert_eq "" "$(grep -nE '(^|[;&|[:space:]])set[[:space:]]+[-+]' <<< "$config_code" || true)" "nenhum set -e/set -o (nem outro set) fora de comentário"
assert_eq "" "$(grep -nwE 'exit|source|eval' <<< "$config_code" || true)" "nenhum exit, source ou eval fora de comentário"
assert_eq "" "$(grep -nE '(^|[;&|])[[:space:]]*\.[[:space:]]+[^[:space:]]' <<< "$config_code" || true)" "nenhum '. arquivo' fora de comentário"

echo
echo "== registro: config_register_key aceita chaves válidas, na ordem de registro =="
fake_keys_all="FAKE_TEXT FAKE_ENABLED FAKE_NOTE_ENABLED FAKE_ON_ENABLED"
assert_eq "$fake_keys_all|4 4 4 4" "$(run_fake "printf '%s|%s %s %s %s' \"\${CONFIG_KEYS[*]}\" \"\${#CONFIG_KEYS[@]}\" \"\${#CONFIG_KEY_DEFAULTS[@]}\" \"\${#CONFIG_KEY_NOTES[@]}\" \"\${#CONFIG_KEY_COMMENTS[@]}\"")" "as quatro chaves de mentira são registradas, CONFIG_KEYS na ordem de registro, com os quatro arrays do mesmo tamanho"
reg_dir="$TEST_TMP/register"
mkdir -p "$reg_dir"
assert_eq "0|$fake_keys_all FAKE_NEW_ENABLED|5 5 5 5|depois" "$(run_register "$reg_dir/accept.err" FAKE_NEW_ENABLED "true" "" "FAKE_NEW_ENABLED: outra.")" "chave nova válida (padrão \"true\"): retorna 0 e entra no fim de CONFIG_KEYS"
assert_eq "" "$(cat "$reg_dir/accept.err" 2>/dev/null || echo 'sem stderr')" "chave nova válida: nada no stderr"
assert_eq "0|$fake_keys_all FAKE_DIGIT_2|5 5 5 5|depois" "$(run_register "$reg_dir/accept_digit.err" FAKE_DIGIT_2 "" "" "FAKE_DIGIT_2: com dígito.")" "nome com dígito e _ (FAKE_DIGIT_2): aceito"
assert_eq "0" "$(run_fake "t_rc=0; config_register_key FAKE_SET_E_ENABLED 'false' '' 'c' || t_rc=\$?; printf '%s' \"\$t_rc\"")" "registro válido sob set -euo pipefail: retorna 0"

echo
echo "== registro: config_register_key recusa com retorno 1, sem registrar e sem sair do shell =="
reg_marker="$reg_dir/marker"
reg_refused="1|$fake_keys_all|4 4 4 4|depois"
reg_case=0
# reg_refuse <descrição> <arg>...: confere que o registro é recusado, sem mudar o registro,
# com o shell seguindo depois da chamada, e com uma mensagem no stderr.
reg_refuse() {
  local description="$1"
  shift
  reg_case=$((reg_case + 1))
  assert_eq "$reg_refused" "$(run_register "$reg_dir/refuse_$reg_case.err" "$@")" "$description: retorna 1, não registra e o comando seguinte roda"
  assert_eq "true" "$([ -s "$reg_dir/refuse_$reg_case.err" ] && echo true || echo false)" "$description: mensagem no stderr"
}
reg_refuse "nome minúsculo (fake_lower)" fake_lower "" "" "c"
reg_refuse "nome que começa com dígito (1FAKE)" 1FAKE "" "" "c"
reg_refuse "nome vazio" "" "" "" "c"
reg_refuse "nome com [ (FAKE[0])" "FAKE[0]" "" "" "c"
reg_refuse "nome com \$ (FAKE\$X)" "FAKE\$X" "" "" "c"
reg_refuse "nome com \$(...) entre colchetes" "a[\$(touch $reg_marker)]" "" "" "c"
assert_not_exists "$reg_marker" "nome com \$(...) entre colchetes: nada é executado"
reg_refuse "prefixo CONFIG_ (CONFIG_FAKE)" CONFIG_FAKE "" "" "c"
reg_refuse "chave já registrada (FAKE_TEXT)" FAKE_TEXT "" "" "c"
reg_refuse "chave _ENABLED com padrão \"\"" FAKE_X_ENABLED "" "" "c"
reg_refuse "chave _ENABLED com padrão \"sim\"" FAKE_X_ENABLED "sim" "" "c"
reg_refuse "chave de texto com padrão \"false\"" FAKE_X "false" "" "c"
reg_refuse "chave de texto com padrão \"x\"" FAKE_X "x" "" "c"
reg_refuse "comentário vazio" FAKE_X "" "" ""
reg_refuse "comentário com espaço no fim de uma linha" FAKE_X "" "" $'primeira \nsegunda'
reg_refuse "comentário com tab no fim da última linha" FAKE_X "" "" $'primeira\nsegunda\t'
reg_refuse "comentário com \\r (CRLF)" FAKE_X "" "" $'primeira\r\nsegunda'
# Variáveis especiais do shell: um .nick.conf editado por outro time não pode mudar o PATH, o
# IFS, o BASH_ENV etc. de quem roda a ferramenta, e um printf -v numa readonly derrubaria o
# chamador (config_load nunca sai).
for reg_special in PATH IFS BASH_ENV BASHOPTS ENV HOME CDPATH PS1 PS4 SHELLOPTS LD_PRELOAD LC_ALL LANG TERM EDITOR SHELL GLOBIGNORE PROMPT_COMMAND; do
  reg_refuse "variável especial do shell ($reg_special)" "$reg_special" "" "" "c"
done
reg_refuse "variável readonly do bash (UID)" UID "" "" "c"
assert_eq "1|$fake_keys_all|depois" "$(run_fake "readonly FAKE_RO=x; t_rc=0; config_register_key FAKE_RO '' '' 'c' 2>/dev/null || t_rc=\$?; printf '%s|%s|depois' \"\$t_rc\" \"\${CONFIG_KEYS[*]}\"")" "variável readonly criada pelo chamador (FAKE_RO): recusada com retorno 1, sem registrar"
assert_eq "0|$fake_keys_all FAKE_BASH_LIKE FAKE_PATH|6 6 6 6|depois" "$(run_fake "config_register_key FAKE_BASH_LIKE '' '' 'c'; t_rc=0; config_register_key FAKE_PATH '' '' 'c' || t_rc=\$?; printf '%s|%s|%s %s %s %s|depois' \"\$t_rc\" \"\${CONFIG_KEYS[*]}\" \"\${#CONFIG_KEYS[@]}\" \"\${#CONFIG_KEY_DEFAULTS[@]}\" \"\${#CONFIG_KEY_NOTES[@]}\" \"\${#CONFIG_KEY_COMMENTS[@]}\"")" "nomes que só contêm um nome especial (FAKE_BASH_LIKE, FAKE_PATH) continuam aceitos"
assert_eq "1|depois" "$(run_fake "t_rc=0; config_register_key fake_lower '' '' 'c' 2>/dev/null || t_rc=\$?; printf '%s|depois' \"\$t_rc\"")" "registro inválido sob set -euo pipefail, antes de ||: retorna 1 e o shell segue"

echo
echo "== liga/desliga: config_is_toggle_key decide pelo sufixo _ENABLED =="
for toggle_key in FAKE_ENABLED FAKE_ON_ENABLED X_ENABLED; do
  assert_eq "0" "$(run_fake "t_rc=0; config_is_toggle_key $toggle_key || t_rc=\$?; printf '%s' \"\$t_rc\"")" "$toggle_key: é liga/desliga"
done
for toggle_key in FAKE_TEXT FAKE_ENABLED_OLD FAKE_ENABLE ENABLED; do
  assert_eq "1" "$(run_fake "t_rc=0; config_is_toggle_key $toggle_key || t_rc=\$?; printf '%s' \"\$t_rc\"")" "$toggle_key: não é liga/desliga"
done

echo
echo "== padrão e nota por chave: config_key_default e config_key_note devolvem o registrado =="
# default_and_note <key>: imprime "<padrão>|<código>|<nota>|<código>".
default_and_note() {
  run_fake "t_rc=0; t_default=\"\$(config_key_default $1)\" || t_rc=\$?; t_rc2=0; t_note=\"\$(config_key_note $1)\" || t_rc2=\$?; printf '%s|%s|%s|%s' \"\$t_default\" \"\$t_rc\" \"\$t_note\" \"\$t_rc2\""
}
assert_eq "|0||0" "$(default_and_note FAKE_TEXT)" "FAKE_TEXT: padrão \"\" e sem nota, retorno 0"
assert_eq "false|0||0" "$(default_and_note FAKE_ENABLED)" "FAKE_ENABLED: padrão \"false\" e sem nota, retorno 0"
assert_eq "false|0|$fake_note|0" "$(default_and_note FAKE_NOTE_ENABLED)" "FAKE_NOTE_ENABLED: padrão \"false\" e a nota registrada"
assert_eq "true|0|$fake_on_note|0" "$(default_and_note FAKE_ON_ENABLED)" "FAKE_ON_ENABLED: padrão \"true\" e a nota registrada"
assert_eq "|1||1" "$(default_and_note FAKE_UNKNOWN_ENABLED)" "chave não registrada: nada impresso e retorno 1 nas duas"

echo
echo '== gramática: config_load lê só CHAVE="valor" das chaves registradas, sem executar nada =='
gram_dir="$TEST_TMP/grammar"
mkdir -p "$gram_dir"

printf '%s\n' '# comentário' 'FAKE_TEXT="texto (com [ ], vírgula)"' '' 'FAKE_ENABLED="true"' 'FAKE_NOTE_ENABLED="false"' 'FAKE_ON_ENABLED=""' > "$gram_dir/complete.conf"
assert_eq "texto (com [ ], vírgula)|true|false|" "$(run_load "$gram_dir/complete.conf" "$gram_dir/complete")" "arquivo completo: cada chave recebe o valor do arquivo"
assert_eq "" "$(cat "$gram_dir/complete.err" "$gram_dir/complete.out" 2>/dev/null || echo 'sem saída gravada')" "arquivo completo: nada no stdout nem no stderr"
assert_eq "0" "$(cat "$gram_dir/complete.rc" 2>/dev/null || true)" "arquivo completo: config_load retorna 0"

printf '%s\n' "$(fake_keys_blank FAKE_TEXT)" "FAKE_TEXT=\"\$HOME \$(touch $gram_dir/marker_subst) \`touch $gram_dir/marker_backtick\`\"" > "$gram_dir/literal.conf"
assert_eq "\$HOME \$(touch $gram_dir/marker_subst) \`touch $gram_dir/marker_backtick\`|||" "$(run_load "$gram_dir/literal.conf" "$gram_dir/literal")" "\$VAR, \$(...) e crase no valor ficam literais"
assert_not_exists "$gram_dir/marker_subst" "\$(...) no valor não é executado"
assert_not_exists "$gram_dir/marker_backtick" "crase no valor não é executada"

printf '%s\n' "$(fake_keys_blank)" 'echo executou' "touch $gram_dir/marker_command" > "$gram_dir/commands.conf"
run_load "$gram_dir/commands.conf" "$gram_dir/commands" >/dev/null
assert_not_contains "$(cat "$gram_dir/commands.out" "$gram_dir/commands.err" 2>/dev/null)" "executou" "linha 'echo executou' não é executada"
assert_not_exists "$gram_dir/marker_command" "linha 'touch <marker>' não é executada"
assert_eq "" "$(cat "$gram_dir/commands.err" 2>/dev/null || echo 'sem stderr')" "linhas de comando são ignoradas em silêncio"

printf '  \tFAKE_TEXT="aparado" \t\r\nFAKE_ENABLED="true"\r\nFAKE_NOTE_ENABLED=""\r\nFAKE_ON_ENABLED=""\r\n' > "$gram_dir/trim.conf"
assert_eq "aparado|true||" "$(run_load "$gram_dir/trim.conf" "$gram_dir/trim")" "espaços, tabs e o \\r do CRLF em volta da linha são aparados"
assert_eq "" "$(cat "$gram_dir/trim.err" 2>/dev/null || echo 'sem stderr')" "linhas com espaços e CRLF: nenhum aviso"

printf '%s\n' "$(fake_keys_blank FAKE_TEXT)" 'FAKE_TEXT="a"b"' > "$gram_dir/inner_quote.conf"
assert_eq "antes|||" "$(run_load "$gram_dir/inner_quote.conf" "$gram_dir/inner_quote" "FAKE_TEXT=antes")" "aspas dentro do valor: a linha não é aplicada"

printf '%s\n' "$(fake_keys_blank FAKE_ENABLED)" 'FAKE_ENABLED="true" # comentário' > "$gram_dir/trailing_comment.conf"
assert_eq "|antes||" "$(run_load "$gram_dir/trailing_comment.conf" "$gram_dir/trailing_comment" "FAKE_ENABLED=antes")" "comentário no fim da linha: a linha não é aplicada"

{ fake_keys_blank FAKE_ON_ENABLED; printf '%s' 'FAKE_ON_ENABLED="false"'; } > "$gram_dir/no_newline.conf"
assert_eq "|||false" "$(run_load "$gram_dir/no_newline.conf" "$gram_dir/no_newline")" "última linha sem \\n é lida"

printf '%s\n' "$(fake_keys_blank)" 'PATH="/x"' 'CONFIG_KEYS="x"' 'OTHER="x"' 'FAKE_TEXT_EXTRA = "x"' 'FAKE_TEXT_EXTRA="x"' 'fake_text="x"' > "$gram_dir/unknown.conf"
assert_eq "|||" "$(run_load "$gram_dir/unknown.conf" "$gram_dir/unknown")" "chaves desconhecidas não mudam as chaves registradas"
assert_eq "" "$(cat "$gram_dir/unknown.err" 2>/dev/null || echo 'sem stderr')" "chaves desconhecidas (PATH, CONFIG_KEYS, OTHER, FAKE_TEXT_EXTRA = \"x\") são ignoradas em silêncio"
assert_eq "$fake_keys_all|$PATH|" "$(run_fake "config_load '$gram_dir/unknown.conf' 2>/dev/null; printf '%s|%s|%s' \"\${CONFIG_KEYS[*]}\" \"\$PATH\" \"\${OTHER-}\"")" "PATH, CONFIG_KEYS e OTHER do arquivo não são atribuídos"

echo
echo "== aviso de formato: linha de chave registrada fora de CHAVE=\"valor\" =="
fmt_dir="$TEST_TMP/format"
mkdir -p "$fmt_dir"
fmt_case=0
for fmt_line in 'FAKE_ENABLED=true' 'export FAKE_ENABLED="true"' 'FAKE_ENABLED = "true"' 'FAKE_ENABLED ="true"' "FAKE_ENABLED=\"true\" && touch $fmt_dir/marker" 'FAKE_ENABLED=TRUE'; do
  fmt_case=$((fmt_case + 1))
  fmt_file="$fmt_dir/case_$fmt_case.conf"
  printf '%s\n' "$(fake_keys_blank FAKE_ENABLED)" "$fmt_line" > "$fmt_file"
  fmt_result="$(run_load "$fmt_file" "$fmt_dir/case_$fmt_case" "FAKE_ENABLED=antes")"
  assert_eq "$(malformed_warning "$fmt_file:4" FAKE_ENABLED)" "$(cat "$fmt_dir/case_$fmt_case.err" 2>/dev/null || true)" "'$fmt_line': um aviso de formato só, com o texto exato, arquivo e linha (e nenhum de chave ausente nem de valor)"
  assert_eq "|antes||" "$fmt_result" "'$fmt_line': a linha não é aplicada"
done
assert_not_exists "$fmt_dir/marker" "linha híbrida CHAVE=\"x\" && touch: o comando não é executado"

echo
echo "== aviso de valor: só nas chaves _ENABLED, só em linha aceita, e o valor continua atribuído =="
val_dir="$TEST_TMP/value"
mkdir -p "$val_dir"
printf '%s\n' 'FAKE_TEXT="sim"' 'FAKE_ENABLED="sim"' 'FAKE_NOTE_ENABLED="TRUE"' 'FAKE_ON_ENABLED="False"' > "$val_dir/invalid.conf"
assert_eq "sim|sim|TRUE|False" "$(run_load "$val_dir/invalid.conf" "$val_dir/invalid")" "valores inválidos continuam atribuídos como estão"
assert_eq "$(value_warning "$val_dir/invalid.conf:2" FAKE_ENABLED sim "$default_off_note")"$'\n'"$(value_warning "$val_dir/invalid.conf:3" FAKE_NOTE_ENABLED TRUE "$fake_note")"$'\n'"$(value_warning "$val_dir/invalid.conf:4" FAKE_ON_ENABLED False "$fake_on_note")" "$(cat "$val_dir/invalid.err" 2>/dev/null || true)" "um aviso por linha, na ordem: sem nota termina com '$default_off_note', com nota termina com a nota; FAKE_TEXT=\"sim\" não avisa"
for val_value in true false ""; do
  printf '%s\n' "$(fake_keys_blank FAKE_ENABLED)" "FAKE_ENABLED=\"$val_value\"" > "$val_dir/accepted.conf"
  run_load "$val_dir/accepted.conf" "$val_dir/accepted" >/dev/null
  assert_eq "" "$(cat "$val_dir/accepted.err" 2>/dev/null || echo 'sem stderr')" "FAKE_ENABLED=\"$val_value\": nenhum aviso"
done

echo
echo "== caracteres de controle: viram ? no aviso, e a variável fica com o valor bruto =="
ctl_raw="a"$'\e'"[31mb"
printf '%s\n' "$(fake_keys_blank FAKE_ENABLED)" "FAKE_ENABLED=\"$ctl_raw\"" > "$val_dir/control.conf"
ctl_result="$(run_load "$val_dir/control.conf" "$val_dir/control")"
assert_eq "$(value_warning "$val_dir/control.conf:4" FAKE_ENABLED 'a?[31mb' "$default_off_note")" "$(cat "$val_dir/control.err" 2>/dev/null || true)" "ESC no valor sai como ? no aviso"
assert_eq "0" "$(grep -c $'\e' "$val_dir/control.err" 2>/dev/null || true)" "nenhum byte ESC no stderr"
assert_eq "|$ctl_raw||" "$ctl_result" "a variável recebe o valor bruto, com o ESC"
# Com LC_ALL=C, [![:print:]] também trocaria os bytes de "não" por "?"; a troca usa
# [[:cntrl:]], que nesse locale só pega os controles ASCII. O LC_ALL=C vai no setup porque o
# bash reaplica o locale quando LC_ALL muda.
printf '%s\n' "$(fake_keys_blank FAKE_ENABLED)" 'FAKE_ENABLED="não"' > "$val_dir/non_ascii.conf"
run_load "$val_dir/non_ascii.conf" "$val_dir/non_ascii" "LC_ALL=C" >/dev/null
assert_eq "$(value_warning "$val_dir/non_ascii.conf:4" FAKE_ENABLED "não" "$default_off_note")" "$(cat "$val_dir/non_ascii.err" 2>/dev/null || true)" "valor com acento sob LC_ALL=C: sai intacto no aviso"

echo
echo "== aviso de chave ausente: um bloco só, na ordem do registro, com a nota antes da chave =="
miss_dir="$TEST_TMP/missing"
mkdir -p "$miss_dir"
printf '%s\n' 'FAKE_TEXT="x"' > "$miss_dir/some.conf"
run_load "$miss_dir/some.conf" "$miss_dir/some" >/dev/null
assert_eq "$(missing_warning "$miss_dir/some.conf" 'FAKE_ENABLED="false"' "# FAKE_NOTE_ENABLED: $fake_note" 'FAKE_NOTE_ENABLED="false"' "# FAKE_ON_ENABLED: $fake_on_note" 'FAKE_ON_ENABLED="true"')" "$(cat "$miss_dir/some.err" 2>/dev/null || true)" "três ausentes: texto exato, com a nota numa linha de comentário antes da chave que tem nota"
assert_eq "1" "$(count_warnings "$miss_dir/some.err")" "três ausentes: só a 1ª linha começa com 'Aviso:'"
: > "$miss_dir/empty.conf"
run_load "$miss_dir/empty.conf" "$miss_dir/empty" >/dev/null
assert_eq "$(missing_warning "$miss_dir/empty.conf" 'FAKE_TEXT=""' 'FAKE_ENABLED="false"' "# FAKE_NOTE_ENABLED: $fake_note" 'FAKE_NOTE_ENABLED="false"' "# FAKE_ON_ENABLED: $fake_on_note" 'FAKE_ON_ENABLED="true"')" "$(cat "$miss_dir/empty.err" 2>/dev/null || true)" "arquivo vazio: lista todas as chaves, na ordem do registro"
printf '%s\n' "$(fake_keys_blank FAKE_ENABLED)" '# FAKE_ENABLED="false"' 'FAKE_ENABLED_OLD="false"' 'FAKE_ENABLE="false"' > "$miss_dir/lookalike.conf"
run_load "$miss_dir/lookalike.conf" "$miss_dir/lookalike" >/dev/null
assert_eq "$(missing_warning "$miss_dir/lookalike.conf" 'FAKE_ENABLED="false"')" "$(cat "$miss_dir/lookalike.err" 2>/dev/null || true)" "linha comentada e nomes parecidos contam como ausentes"
printf '%s\n' "$(fake_keys_blank FAKE_ENABLED)" 'FAKE_ENABLED=true' > "$miss_dir/malformed.conf"
run_load "$miss_dir/malformed.conf" "$miss_dir/malformed" >/dev/null
assert_eq "$(malformed_warning "$miss_dir/malformed.conf:4" FAKE_ENABLED)" "$(cat "$miss_dir/malformed.err" 2>/dev/null || true)" "linha mal formatada conta como presente: só o aviso de formato"
printf '%s\n' 'FAKE_ENABLED="sim"' > "$miss_dir/invalid_and_missing.conf"
run_load "$miss_dir/invalid_and_missing.conf" "$miss_dir/invalid_and_missing" >/dev/null
assert_eq "$(value_warning "$miss_dir/invalid_and_missing.conf:1" FAKE_ENABLED sim "$default_off_note")"$'\n'"$(missing_warning "$miss_dir/invalid_and_missing.conf" 'FAKE_TEXT=""' "# FAKE_NOTE_ENABLED: $fake_note" 'FAKE_NOTE_ENABLED="false"' "# FAKE_ON_ENABLED: $fake_on_note" 'FAKE_ON_ENABLED="true"')" "$(cat "$miss_dir/invalid_and_missing.err" 2>/dev/null || true)" "linha com valor inválido conta como presente; o aviso de ausentes vem depois do de valor"
assert_eq "0" "$(cat "$miss_dir/invalid_and_missing.rc" 2>/dev/null || true)" "com avisos, config_load continua retornando 0"

echo
echo "== globais escritas: config_load só escreve as globais das chaves registradas =="
glob_file="$TEST_TMP/globals.conf"
printf '%s\n' 'FAKE_TEXT="t"' 'FAKE_ENABLED="true"' 'FAKE_NOTE_ENABLED="sim"' 'FAKE_ON_ENABLED="false"' 'OTHER="x"' 'PATH="/x"' 'CONFIG_KEYS="x"' 'FAKE_TEXT=sem-aspas' 'export FAKE_ENABLED="x"' > "$glob_file"
# Retrato por nome: declare -p de cada variável, menos as voláteis do bash (BASHPID muda a
# cada $(...)) e as do teste (t_*).
# t_changed lista o nome de toda variável nova, alterada ou removida entre os dois retratos.
glob_changed="$(run_fake "
declare -A t_snap
t_skip() { case \"\$1\" in _ | BASH_* | BASHPID | FUNCNAME | LINENO | RANDOM | SRANDOM | SECONDS | EPOCH* | PIPESTATUS | t_*) return 0 ;; esac; return 1; }
for t_name in \$(compgen -v); do t_skip \"\$t_name\" || t_snap[\$t_name]=\"\$(declare -p \"\$t_name\")\"; done
config_load '$glob_file' 2>/dev/null
t_changed=()
for t_name in \$(compgen -v); do
  t_skip \"\$t_name\" && continue
  if [ -z \"\${t_snap[\$t_name]+set}\" ] || [ \"\${t_snap[\$t_name]}\" != \"\$(declare -p \"\$t_name\")\" ]; then t_changed+=(\"\$t_name\"); fi
done
for t_name in \"\${!t_snap[@]}\"; do declare -p \"\$t_name\" >/dev/null 2>&1 || t_changed+=(\"\$t_name\"); done
printf '%s\n' \"\${t_changed[@]}\" | LC_ALL=C sort | paste -sd ' ' -
")"
assert_eq "FAKE_ENABLED FAKE_NOTE_ENABLED FAKE_ON_ENABLED FAKE_TEXT" "$glob_changed" "a diferença do declare -p antes e depois é exatamente as quatro chaves registradas (nem OTHER, PATH, CONFIG_* ou outra global)"

echo
echo "== funciona sem set -e: mesmos valores e avisos com e sem set -e no chamador =="
nose_dir="$TEST_TMP/no_set_e"
mkdir -p "$nose_dir"
nose_case=0
for nose_file in "$fmt_dir/case_5.conf" "$val_dir/invalid.conf" "$val_dir/control.conf" "$miss_dir/invalid_and_missing.conf"; do
  nose_case=$((nose_case + 1))
  nose_with="$(run_load "$nose_file" "$nose_dir/with_$nose_case" "FAKE_ENABLED=antes")"
  nose_without="$(run_load "$nose_file" "$nose_dir/without_$nose_case" "FAKE_ENABLED=antes" ":")"
  assert_eq "$nose_with" "$nose_without" "$(basename "$nose_file"): mesmos valores sem set -e"
  assert_eq "$(cat "$nose_dir/with_$nose_case.err" 2>/dev/null || echo 'sem stderr com set -e')" "$(cat "$nose_dir/without_$nose_case.err" 2>/dev/null || echo 'sem stderr sem set -e')" "$(basename "$nose_file"): mesmos avisos sem set -e"
  assert_eq "true" "$([ -s "$nose_dir/without_$nose_case.err" ] && echo true || echo false)" "$(basename "$nose_file"): sem set -e, o caso tem aviso (amostra não vazia)"
  assert_eq "0" "$(cat "$nose_dir/without_$nose_case.rc" 2>/dev/null || true)" "$(basename "$nose_file"): sem set -e, config_load retorna 0"
done

echo
echo "== config_cwd_file: imprime <cwd>/.nick/nick.conf e retorna 0, só no cwd =="
cwd_dir="$TEST_TMP/cwd_file"
mkdir -p "$cwd_dir/without" "$cwd_dir/with/.nick" "$cwd_dir/with/sub"
: > "$cwd_dir/with/.nick/nick.conf"
# cwd_file <dir>: imprime "<saída de config_cwd_file>|<código>", com o cwd em <dir>.
cwd_file() {
  (cd "$1" && run_fake "t_rc=0; t_out=\"\$(config_cwd_file)\" || t_rc=\$?; printf '%s|%s' \"\$t_out\" \"\$t_rc\"")
}
assert_eq "$(cd "$cwd_dir/without" && pwd)/.nick/nick.conf|0" "$(cwd_file "$cwd_dir/without")" "sem o arquivo: imprime <cwd>/.nick/nick.conf e retorna 0"
assert_eq "$(cd "$cwd_dir/with" && pwd)/.nick/nick.conf|0" "$(cwd_file "$cwd_dir/with")" "com o arquivo: imprime <cwd>/.nick/nick.conf e retorna 0"
assert_eq "$(cd "$cwd_dir/with/sub" && pwd)/.nick/nick.conf|0" "$(cwd_file "$cwd_dir/with/sub")" "num subdiretório de um diretório com .nick/nick.conf: imprime o caminho do subdiretório, sem subir para o pai"

echo
echo "== config_cwd_legacy_check: .nick.conf antigo no cwd =="
legacy_dir="$TEST_TMP/legacy"
mkdir -p "$legacy_dir/none" "$legacy_dir/new/.nick" "$legacy_dir/old" "$legacy_dir/both/.nick"
: > "$legacy_dir/new/.nick/nick.conf"
printf '%s\n' 'FAKE_TEXT="antigo"' > "$legacy_dir/old/.nick.conf"
printf '%s\n' 'FAKE_TEXT="antigo"' > "$legacy_dir/both/.nick.conf"
: > "$legacy_dir/both/.nick/nick.conf"
# legacy_check <dir>: com o cwd em <dir>, chama config_cwd_legacy_check (stdout em
# <dir>.out, stderr em <dir>.err) e imprime "<código>|depois"; o "depois" só sai se o shell
# continuou depois da chamada (a função não pode chamar exit). Grava também a lista do
# diretório antes e depois (<dir>.before/<dir>.after).
legacy_check() {
  (cd "$1" && find . | sort > "$1.before" && run_fake "t_rc=0; config_cwd_legacy_check >'$1.out' 2>'$1.err' || t_rc=\$?; printf '%s|depois' \"\$t_rc\"" && find . | sort > "$1.after")
}
for legacy_case in none new old both; do
  legacy_rc="$(legacy_check "$legacy_dir/$legacy_case")"
  case "$legacy_case" in
    old) legacy_expected_rc="1|depois" ;;
    *) legacy_expected_rc="0|depois" ;;
  esac
  assert_eq "$legacy_expected_rc" "$legacy_rc" "$legacy_case: código de retorno e o shell segue depois (sem exit)"
  assert_eq "" "$(cat "$legacy_dir/$legacy_case.out" 2>/dev/null || echo 'sem stdout gravado')" "$legacy_case: nada no stdout"
  assert_eq "igual" "$(cmp -s "$legacy_dir/$legacy_case.before" "$legacy_dir/$legacy_case.after" && echo igual || echo diferente)" "$legacy_case: não move, cria nem apaga nada no cwd"
done
assert_eq "" "$(cat "$legacy_dir/none.err" 2>/dev/null || echo 'sem stderr gravado')" "nenhum arquivo: nada no stderr"
assert_eq "" "$(cat "$legacy_dir/new.err" 2>/dev/null || echo 'sem stderr gravado')" "só o novo: nada no stderr"
legacy_old_pwd="$(cd "$legacy_dir/old" && pwd)"
legacy_old_expected="Erro: a configuração do projeto agora fica em .nick/nick.conf, e $legacy_old_pwd/.nick.conf não é mais lido.
Mova o arquivo e rode de novo (na raiz do projeto):
  mkdir -p .nick && mv .nick.conf .nick/nick.conf
Depois, versione .nick/nick.conf e o .nick/.gitignore que o nick cria na próxima execução."
assert_eq "$legacy_old_expected" "$(cat "$legacy_dir/old.err" 2>/dev/null || true)" "só o antigo: erro exato no stderr, com a linha do mv"
legacy_both_pwd="$(cd "$legacy_dir/both" && pwd)"
assert_eq "Aviso: $legacy_both_pwd/.nick.conf não é mais lido — a configuração vem de $legacy_both_pwd/.nick/nick.conf. Apague o arquivo antigo." "$(cat "$legacy_dir/both.err" 2>/dev/null || true)" "os dois: aviso exato no stderr"
assert_eq 'FAKE_TEXT="antigo"' "$(cat "$legacy_dir/old/.nick.conf")" "só o antigo: o arquivo antigo continua igual"
assert_eq "1|depois" "$(cd "$legacy_dir/old" && timeout 2 bash -c "source '$FAKE_KEYS_SH'; config_cwd_legacy_check 2>/dev/null; printf '%s|depois' \"\$?\"" 2>/dev/null || true)" "só o antigo, sem set -e: retorna 1 e o shell segue"

echo
echo "== esqueleto: config_print_skeleton <tool_name> =="
skel_dir="$TEST_TMP/skeleton"
mkdir -p "$skel_dir"
run_fake "config_print_skeleton ferramenta-x > '$skel_dir/skeleton.conf'" >/dev/null
cat > "$skel_dir/expected.conf" <<EOF
# Config do projeto pro ferramenta-x. Preencha as variáveis abaixo e salve.
# Não é executado: o ferramenta-x lê só linhas CHAVE="valor" (aspas duplas, sem aspas dentro do valor nem comentário no fim da linha) das chaves abaixo.
# Mantenha todas as chaves abaixo, mesmo as que não usar (o ferramenta-x avisa quando falta alguma). Nas chaves *_ENABLED, use só "true", "false" ou "".
# FAKE_TEXT: texto livre de mentira.
FAKE_TEXT=""
# FAKE_ENABLED: liga/desliga de mentira.
FAKE_ENABLED="false"
# FAKE_NOTE_ENABLED: liga/desliga de mentira, com nota.
FAKE_NOTE_ENABLED="false"
# FAKE_ON_ENABLED: liga/desliga de mentira, ligada por padrão.
# Ao contrário das outras, vazio a mantém ligada.
FAKE_ON_ENABLED="true"
EOF
assert_eq "igual" "$(cmp -s "$skel_dir/expected.conf" "$skel_dir/skeleton.conf" && echo igual || echo diferente)" "cabeçalho com o nome da ferramenta e, por chave na ordem do registro, o comentário com '# ' por linha e CHAVE=\"padrão\" (byte a byte)"
assert_eq "sem shebang" "$(case "$(head -n1 "$skel_dir/skeleton.conf" 2>/dev/null || true)" in '#!'*) echo 'com shebang' ;; *) echo 'sem shebang' ;; esac)" "esqueleto não começa com shebang"
assert_eq "0" "$(grep -c '[[:space:]]$' "$skel_dir/skeleton.conf" 2>/dev/null || true)" "esqueleto não tem espaço (nem \\r) no fim de nenhuma linha"
assert_eq '\n' "$(tail -c1 "$skel_dir/skeleton.conf" 2>/dev/null | od -An -c | tr -d ' ')" "esqueleto termina com \\n"
run_fake "config_register_key FAKE_BLANK '' '' \$'primeira\n\núltima'; config_print_skeleton ferramenta-x > '$skel_dir/blank_line.conf'" >/dev/null
assert_eq "$(printf '%s\n' '# primeira' '#' '# última' 'FAKE_BLANK=""')" "$(tail -n4 "$skel_dir/blank_line.conf" 2>/dev/null || true)" "linha vazia no comentário sai como '#', sem espaço no fim"
assert_eq "0" "$(grep -c '[[:space:]]$' "$skel_dir/blank_line.conf" 2>/dev/null || true)" "esqueleto com linha vazia no comentário: nenhum espaço no fim de linha"
assert_eq "|false|false|true" "$(run_load "$skel_dir/skeleton.conf" "$skel_dir/round_trip" "FAKE_TEXT=antes; FAKE_ON_ENABLED=antes")" "ida e volta: o esqueleto carregado por config_load deixa cada chave com o padrão"
assert_eq "" "$(cat "$skel_dir/round_trip.err" 2>/dev/null || echo 'sem stderr')" "ida e volta: o esqueleto carrega sem nenhum aviso"

echo
echo "== config_dir_prepare: cria a árvore .nick/ no cwd (.gitignore único, troca/ e docs/) =="
# nick_tree <dir>: lista (find | sort, locale C) o que existe em <dir>/.nick, com caminhos
# relativos a <dir>. Sem .nick, imprime vazio.
nick_tree() {
  (cd "$1" && [ -e .nick ] && find .nick | LC_ALL=C sort) || true
}
# nick_gitignore_expected: o .nick/.gitignore que a biblioteca cria, escrito à mão (três linhas).
nick_gitignore_expected() {
  printf '%s\n' '*' '!.gitignore' '!nick.conf'
}
# run_in_dir <dir> <stdin> <out_prefix> <commands> [shell_options]: com o cwd em <dir>, num
# bash separado com [shell_options] (padrão: set -euo pipefail; ":" roda sem set -e), dá source
# em FAKE_KEYS_SH e roda <commands>, com stdout em <out_prefix>.out e stderr em
# <out_prefix>.err. <stdin> vazio vira < /dev/null (EOF já na primeira leitura). Imprime o
# código de saída do bash.
run_in_dir() {
  local dir="$1" input="$2" out_prefix="$3" commands="$4" shell_options="${5:-set -euo pipefail}" rc=0
  if [ -z "$input" ]; then
    (cd "$dir" && timeout 2 bash -c "$shell_options; source '$FAKE_KEYS_SH'; $commands" < /dev/null > "$out_prefix.out" 2> "$out_prefix.err") || rc=$?
  else
    (cd "$dir" && timeout 2 bash -c "$shell_options; source '$FAKE_KEYS_SH'; $commands" <<< "$input" > "$out_prefix.out" 2> "$out_prefix.err") || rc=$?
  fi
  printf '%s' "$rc"
}
# prep_blocked_error <cwd>: a mensagem exata de quando a árvore não pode ser criada.
prep_blocked_error() {
  printf '%s' "Erro: não foi possível criar a estrutura .nick/ (.gitignore, troca/ e docs/) em $(cd "$1" && pwd)."
}
prep_dir="$TEST_TMP/dir_prepare"
mkdir -p "$prep_dir/fresh"
assert_eq "0" "$(run_in_dir "$prep_dir/fresh" "" "$prep_dir/fresh" "config_dir_prepare")" "cwd vazio: config_dir_prepare retorna 0"
assert_eq "$(printf '%s\n' .nick .nick/.gitignore .nick/docs .nick/troca)" "$(nick_tree "$prep_dir/fresh")" "cwd vazio: cria exatamente .nick/.gitignore, .nick/troca/ e .nick/docs/ (sem nick.conf nem .gitignore em troca/ ou docs/)"
assert_eq "$(nick_gitignore_expected)" "$(cat "$prep_dir/fresh/.nick/.gitignore" 2>/dev/null || echo 'sem .gitignore')" "cwd vazio: .nick/.gitignore tem exatamente *, !.gitignore e !nick.conf"
assert_eq "true" "$([ -d "$prep_dir/fresh/.nick/troca" ] && [ -d "$prep_dir/fresh/.nick/docs" ] && echo true || echo false)" "cwd vazio: .nick/troca/ e .nick/docs/ são diretórios"
assert_eq "" "$(cat "$prep_dir/fresh.out" "$prep_dir/fresh.err" 2>/dev/null || echo 'sem saída gravada')" "cwd vazio: nada no stdout nem no stderr"

mkdir -p "$prep_dir/existing/.nick/troca" "$prep_dir/existing/.nick/docs/SC-1"
printf '%s\n' '*' '!.gitignore' '!nick.conf' '!docs/' '!docs/**' > "$prep_dir/existing/.nick/.gitignore"
cp "$prep_dir/existing/.nick/.gitignore" "$prep_dir/existing_gitignore.before"
printf '%s\n' '# do usuário' '*' > "$prep_dir/existing/.nick/troca/.gitignore"
printf 'print antiga' > "$prep_dir/existing/.nick/troca/prova.png"
printf 'plano antigo' > "$prep_dir/existing/.nick/docs/SC-1/plan.md"
assert_eq "0" "$(run_in_dir "$prep_dir/existing" "" "$prep_dir/existing" "config_dir_prepare")" "árvore já existente: config_dir_prepare retorna 0"
assert_eq "igual" "$(cmp -s "$prep_dir/existing_gitignore.before" "$prep_dir/existing/.nick/.gitignore" && echo igual || echo diferente)" "árvore já existente: .nick/.gitignore editado fica idêntico byte a byte"
assert_eq "$(printf '%s\n' '# do usuário' '*')" "$(cat "$prep_dir/existing/.nick/troca/.gitignore" 2>/dev/null || true)" "árvore já existente: o .nick/troca/.gitignore do usuário não é tocado"
assert_eq "print antiga" "$(cat "$prep_dir/existing/.nick/troca/prova.png" 2>/dev/null || true)" "árvore já existente: os arquivos de troca/ continuam lá"
assert_eq "plano antigo" "$(cat "$prep_dir/existing/.nick/docs/SC-1/plan.md" 2>/dev/null || true)" "árvore já existente: os arquivos de docs/<ID>/ continuam lá"
assert_not_exists "$prep_dir/existing/.nick/docs/.gitignore" "árvore já existente: nenhum .gitignore é criado em docs/"

mkdir -p "$prep_dir/blocked/.nick"
printf 'arquivo no lugar da pasta' > "$prep_dir/blocked/.nick/troca"
assert_eq "1" "$(run_in_dir "$prep_dir/blocked" "" "$prep_dir/blocked" "t_rc=0; config_dir_prepare || t_rc=\$?; printf '%s|depois' \"\$t_rc\" > '$prep_dir/blocked.after'; exit \"\$t_rc\"")" ".nick/troca sendo um arquivo: config_dir_prepare retorna 1"
assert_eq "1|depois" "$(cat "$prep_dir/blocked.after" 2>/dev/null || true)" ".nick/troca sendo um arquivo: retorna 1 sem exit (o shell segue)"
assert_eq "$(prep_blocked_error "$prep_dir/blocked")" "$(cat "$prep_dir/blocked.err" 2>/dev/null || true)" ".nick/troca sendo um arquivo: o stderr é só a mensagem exata"
assert_eq "arquivo no lugar da pasta" "$(cat "$prep_dir/blocked/.nick/troca" 2>/dev/null || true)" ".nick/troca sendo um arquivo: o arquivo fica intacto"
# .nick/ sem permissão de escrita, com troca/ e docs/ já existentes: o mkdir passa e só o
# .gitignore falha. O stderr tem de ser só a mensagem da biblioteca, sem o erro cru do bash. O
# root ignora a permissão, então o caso é pulado.
if [ "$(id -u)" -ne 0 ]; then
  mkdir -p "$prep_dir/readonly/.nick/troca" "$prep_dir/readonly/.nick/docs"
  chmod a-w "$prep_dir/readonly/.nick"
  prep_ro_rc="$(run_in_dir "$prep_dir/readonly" "" "$prep_dir/readonly" "config_dir_prepare")"
  chmod u+w "$prep_dir/readonly/.nick"
  assert_eq "1" "$prep_ro_rc" ".nick/ somente leitura: config_dir_prepare retorna 1"
  assert_eq "$(prep_blocked_error "$prep_dir/readonly")" "$(cat "$prep_dir/readonly.err" 2>/dev/null || true)" ".nick/ somente leitura: o stderr é só a mensagem da biblioteca (sem o erro cru do bash)"
  assert_not_exists "$prep_dir/readonly/.nick/.gitignore" ".nick/ somente leitura: nenhum .gitignore é criado"
else
  echo "  PULADO: .nick/ somente leitura não vale como root"
fi
mkdir -p "$prep_dir/no_set_e"
assert_eq "0" "$(run_in_dir "$prep_dir/no_set_e" "" "$prep_dir/no_set_e" "config_dir_prepare" ":")" "sem set -e no chamador: config_dir_prepare retorna 0"
assert_eq "$(printf '%s\n' .nick .nick/.gitignore .nick/docs .nick/troca)" "$(nick_tree "$prep_dir/no_set_e")" "sem set -e no chamador: a mesma árvore"

echo
echo "== config_init <tool_name>: cria .nick/ e .nick/nick.conf no cwd, com confirmação de sobrescrita =="
init_dir="$TEST_TMP/init"
mkdir -p "$init_dir"
run_fake "config_print_skeleton ferramenta-x > '$init_dir/skeleton.conf'" >/dev/null
# init_tree_expected: a árvore de .nick/ que config_init deixa.
init_tree_expected() {
  printf '%s\n' .nick .nick/.gitignore .nick/docs .nick/nick.conf .nick/troca
}
# init_expected_output <cwd>: o stdout exato de config_init quando ele cria o arquivo.
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
# run_init <dir> <stdin> <out_prefix> [shell_options]: config_init ferramenta-x com o cwd em
# <dir>; grava "<código>|depois" em <out_prefix>.after (o "depois" só sai se o shell seguiu
# depois da chamada, provando que não houve exit). Imprime o código de saída do bash.
run_init() {
  local dir="$1" input="$2" out_prefix="$3" shell_options="${4:-set -euo pipefail}"
  run_in_dir "$dir" "$input" "$out_prefix" "t_rc=0; config_init ferramenta-x || t_rc=\$?; printf '%s|depois' \"\$t_rc\" > '$out_prefix.after'; exit \"\$t_rc\"" "$shell_options"
}

mkdir -p "$init_dir/fresh"
init_fresh_pwd="$(cd "$init_dir/fresh" && pwd)"
assert_eq "0" "$(run_init "$init_dir/fresh" "" "$init_dir/fresh")" "cwd vazio: config_init retorna 0"
assert_eq "$(init_tree_expected)" "$(nick_tree "$init_dir/fresh")" "cwd vazio: cria exatamente .nick/.gitignore, .nick/nick.conf, .nick/troca/ e .nick/docs/"
assert_eq "$(nick_gitignore_expected)" "$(cat "$init_dir/fresh/.nick/.gitignore" 2>/dev/null || echo 'sem .gitignore')" "cwd vazio: .nick/.gitignore tem exatamente *, !.gitignore e !nick.conf"
assert_eq "igual" "$(cmp -s "$init_dir/skeleton.conf" "$init_dir/fresh/.nick/nick.conf" && echo igual || echo diferente)" "cwd vazio: .nick/nick.conf é config_print_skeleton ferramenta-x, byte a byte"
assert_eq "$(init_expected_output "$init_fresh_pwd")" "$(cat "$init_dir/fresh.out" 2>/dev/null || true)" "cwd vazio: stdout exato (caminho do nick.conf e a árvore de .nick/)"
assert_eq "" "$(cat "$init_dir/fresh.err" 2>/dev/null || echo 'sem stderr gravado')" "cwd vazio: nada no stderr (nenhum aviso: o init não chama o leitor)"

mkdir -p "$init_dir/overwrite/.nick"
printf '%s\n' 'FAKE_TEXT="antigo"' > "$init_dir/overwrite/.nick/nick.conf"
assert_eq "0" "$(run_init "$init_dir/overwrite" "N" "$init_dir/overwrite_no")" "nick.conf existente, resposta N: retorna 0"
assert_eq "Cancelado. Nada foi alterado." "$(cat "$init_dir/overwrite_no.out" 2>/dev/null || true)" "nick.conf existente, resposta N: stdout é só 'Cancelado. Nada foi alterado.'"
assert_eq "" "$(cat "$init_dir/overwrite_no.err" 2>/dev/null || echo 'sem stderr gravado')" "nick.conf existente (incompleto), resposta N: nada no stderr (nenhum aviso)"
assert_eq 'FAKE_TEXT="antigo"' "$(cat "$init_dir/overwrite/.nick/nick.conf")" "nick.conf existente, resposta N: arquivo inalterado"
assert_eq "$(printf '%s\n' .nick .nick/nick.conf)" "$(nick_tree "$init_dir/overwrite")" "nick.conf existente, resposta N: nada é criado nem completado (sem .gitignore, troca/ ou docs/)"

assert_eq "1" "$(run_init "$init_dir/overwrite" "" "$init_dir/overwrite_eof")" "nick.conf existente, stdin fecha na pergunta (Ctrl+D): retorna 1"
assert_eq "1|depois" "$(cat "$init_dir/overwrite_eof.after" 2>/dev/null || true)" "nick.conf existente, Ctrl+D: retorna 1 sem exit (o shell segue)"
assert_eq "Cancelado. Nada foi alterado." "$(cat "$init_dir/overwrite_eof.err" 2>/dev/null || true)" "nick.conf existente, Ctrl+D: 'Cancelado. Nada foi alterado.' no stderr"
assert_eq "" "$(cat "$init_dir/overwrite_eof.out" 2>/dev/null || echo 'sem stdout gravado')" "nick.conf existente, Ctrl+D: nada no stdout"
assert_eq 'FAKE_TEXT="antigo"' "$(cat "$init_dir/overwrite/.nick/nick.conf")" "nick.conf existente, Ctrl+D: arquivo inalterado"
assert_eq "$(printf '%s\n' .nick .nick/nick.conf)" "$(nick_tree "$init_dir/overwrite")" "nick.conf existente, Ctrl+D: nada é criado nem completado"

assert_eq "0" "$(run_init "$init_dir/overwrite" "s" "$init_dir/overwrite_yes")" "nick.conf existente, resposta s: retorna 0"
assert_eq "igual" "$(cmp -s "$init_dir/skeleton.conf" "$init_dir/overwrite/.nick/nick.conf" && echo igual || echo diferente)" "nick.conf existente, resposta s: arquivo substituído pelo esqueleto"
assert_eq "$(init_tree_expected)" "$(nick_tree "$init_dir/overwrite")" "nick.conf existente, resposta s: a árvore de .nick/ é completada"
assert_eq "$(nick_gitignore_expected)" "$(cat "$init_dir/overwrite/.nick/.gitignore" 2>/dev/null || echo 'sem .gitignore')" "nick.conf existente, resposta s: .nick/.gitignore com o conteúdo exato"
assert_eq "$(init_expected_output "$(cd "$init_dir/overwrite" && pwd)")" "$(cat "$init_dir/overwrite_yes.out" 2>/dev/null || true)" "nick.conf existente, resposta s: stdout exato, como sem o arquivo"
assert_eq "" "$(cat "$init_dir/overwrite_yes.err" 2>/dev/null || echo 'sem stderr gravado')" "nick.conf existente, resposta s: nada no stderr"
for init_answer in S sim Sim SIM; do
  mkdir -p "$init_dir/answer_$init_answer/.nick"
  printf '%s\n' 'FAKE_TEXT="antigo"' > "$init_dir/answer_$init_answer/.nick/nick.conf"
  run_init "$init_dir/answer_$init_answer" "$init_answer" "$init_dir/answer_$init_answer" >/dev/null
  assert_eq "igual" "$(cmp -s "$init_dir/skeleton.conf" "$init_dir/answer_$init_answer/.nick/nick.conf" && echo igual || echo diferente)" "nick.conf existente, resposta $init_answer: sobrescreve"
done
init_answer_case=0
for init_answer in n x não; do
  init_answer_case=$((init_answer_case + 1))
  init_answer_dir="$init_dir/answer_no_$init_answer_case"
  mkdir -p "$init_answer_dir/.nick"
  printf '%s\n' 'FAKE_TEXT="antigo"' > "$init_answer_dir/.nick/nick.conf"
  run_init "$init_answer_dir" "$init_answer" "$init_answer_dir" >/dev/null
  assert_eq 'FAKE_TEXT="antigo"' "$(cat "$init_answer_dir/.nick/nick.conf")" "nick.conf existente, resposta '$init_answer': não sobrescreve"
done

mkdir -p "$init_dir/edited/.nick/troca" "$init_dir/edited/.nick/docs/SC-1"
printf '%s\n' '*' '!.gitignore' '!nick.conf' '!docs/' '!docs/**' > "$init_dir/edited/.nick/.gitignore"
cp "$init_dir/edited/.nick/.gitignore" "$init_dir/edited_gitignore.before"
printf 'print antiga' > "$init_dir/edited/.nick/troca/prova.png"
printf 'plano antigo' > "$init_dir/edited/.nick/docs/SC-1/plan.md"
assert_eq "0" "$(run_init "$init_dir/edited" "" "$init_dir/edited")" ".nick/.gitignore editado: config_init retorna 0"
assert_eq "igual" "$(cmp -s "$init_dir/edited_gitignore.before" "$init_dir/edited/.nick/.gitignore" && echo igual || echo diferente)" ".nick/.gitignore editado: o arquivo fica idêntico byte a byte"
assert_eq "igual" "$(cmp -s "$init_dir/skeleton.conf" "$init_dir/edited/.nick/nick.conf" && echo igual || echo diferente)" ".nick/.gitignore editado: o nick.conf é criado com o esqueleto"
assert_eq "print antiga|plano antigo" "$(cat "$init_dir/edited/.nick/troca/prova.png" 2>/dev/null || true)|$(cat "$init_dir/edited/.nick/docs/SC-1/plan.md" 2>/dev/null || true)" ".nick/.gitignore editado: o conteúdo de troca/ e docs/ fica intacto"

mkdir -p "$init_dir/legacy"
printf '%s\n' 'FAKE_TEXT="antigo"' > "$init_dir/legacy/.nick.conf"
init_legacy_pwd="$(cd "$init_dir/legacy" && pwd)"
assert_eq "0" "$(run_init "$init_dir/legacy" "" "$init_dir/legacy_only")" "só o .nick.conf antigo: config_init retorna 0 (não para no legado)"
assert_eq "$(init_tree_expected)" "$(nick_tree "$init_dir/legacy")" "só o .nick.conf antigo: cria a árvore inteira de .nick/"
assert_eq "igual" "$(cmp -s "$init_dir/skeleton.conf" "$init_dir/legacy/.nick/nick.conf" && echo igual || echo diferente)" "só o .nick.conf antigo: .nick/nick.conf é o esqueleto"
assert_eq 'FAKE_TEXT="antigo"' "$(cat "$init_dir/legacy/.nick.conf")" "só o .nick.conf antigo: o arquivo antigo fica intacto"
assert_eq "$(init_legacy_warning "$init_legacy_pwd")" "$(cat "$init_dir/legacy_only.err" 2>/dev/null || true)" "só o .nick.conf antigo: o stderr é só o aviso exato (copie os valores e apague o antigo)"
assert_eq "$(init_expected_output "$init_legacy_pwd")" "$(cat "$init_dir/legacy_only.out" 2>/dev/null || true)" "só o .nick.conf antigo: stdout exato, como sem o legado"
printf '%s\n' 'FAKE_TEXT="novo"' > "$init_dir/legacy/.nick/nick.conf"
assert_eq "0" "$(run_init "$init_dir/legacy" "N" "$init_dir/legacy_both_no")" "os dois arquivos, resposta N: retorna 0"
assert_eq 'FAKE_TEXT="novo"' "$(cat "$init_dir/legacy/.nick/nick.conf")" "os dois arquivos, resposta N: .nick/nick.conf inalterado"
assert_eq "" "$(cat "$init_dir/legacy_both_no.err" 2>/dev/null || echo 'sem stderr gravado')" "os dois arquivos, resposta N: nenhum aviso"
assert_eq "0" "$(run_init "$init_dir/legacy" "s" "$init_dir/legacy_both_yes")" "os dois arquivos, resposta s: retorna 0"
assert_eq "igual" "$(cmp -s "$init_dir/skeleton.conf" "$init_dir/legacy/.nick/nick.conf" && echo igual || echo diferente)" "os dois arquivos, resposta s: .nick/nick.conf vira o esqueleto"
assert_eq "$(init_legacy_warning "$init_legacy_pwd")" "$(cat "$init_dir/legacy_both_yes.err" 2>/dev/null || true)" "os dois arquivos, resposta s: avisa do arquivo antigo"
assert_eq 'FAKE_TEXT="antigo"' "$(cat "$init_dir/legacy/.nick.conf")" "os dois arquivos: o arquivo antigo fica intacto"

mkdir -p "$init_dir/blocked/.nick"
printf 'arquivo no lugar da pasta' > "$init_dir/blocked/.nick/troca"
assert_eq "1" "$(run_init "$init_dir/blocked" "" "$init_dir/blocked")" ".nick/troca sendo um arquivo: config_init retorna 1"
assert_eq "1|depois" "$(cat "$init_dir/blocked.after" 2>/dev/null || true)" ".nick/troca sendo um arquivo: retorna 1 sem exit (o shell segue)"
assert_eq "$(prep_blocked_error "$init_dir/blocked")" "$(cat "$init_dir/blocked.err" 2>/dev/null || true)" ".nick/troca sendo um arquivo: o stderr é só a mensagem exata"
assert_eq "" "$(cat "$init_dir/blocked.out" 2>/dev/null || echo 'sem stdout gravado')" ".nick/troca sendo um arquivo: nada no stdout"
assert_not_exists "$init_dir/blocked/.nick/nick.conf" ".nick/troca sendo um arquivo: o nick.conf não é gravado"
assert_eq "arquivo no lugar da pasta" "$(cat "$init_dir/blocked/.nick/troca" 2>/dev/null || true)" ".nick/troca sendo um arquivo: o arquivo fica intacto"

# Número de argumentos errado (API pública: a casca e outros módulos chamam): erro da
# biblioteca no stderr, retorno 1 sem exit, nada criado — com e sem set -u/-e no chamador.
init_args_case=0
for init_args in "sem argumento|0|" "nome vazio|1|''" "dois argumentos|2|ferramenta-x extra"; do
  init_args_case=$((init_args_case + 1))
  init_args_label="${init_args%%|*}"
  init_args_rest="${init_args#*|}"
  init_args_count="${init_args_rest%%|*}"
  init_args_call="config_init ${init_args_rest#*|}"
  for init_args_opts in "set -euo pipefail" ":"; do
    init_args_dir="$init_dir/args_${init_args_case}_${init_args_opts%% *}"
    mkdir -p "$init_args_dir/cwd"
    assert_eq "1" "$(run_in_dir "$init_args_dir/cwd" "" "$init_args_dir/run" "t_rc=0; $init_args_call || t_rc=\$?; printf '%s|depois' \"\$t_rc\" > '$init_args_dir/run.after'; exit \"\$t_rc\"" "$init_args_opts")" "config_init $init_args_label ($init_args_opts): retorna 1"
    assert_eq "1|depois" "$(cat "$init_args_dir/run.after" 2>/dev/null || true)" "config_init $init_args_label ($init_args_opts): sem exit (o shell segue)"
    assert_eq "Erro: config_init espera 1 argumento (o nome da ferramenta), recebeu $init_args_count." "$(cat "$init_args_dir/run.err" 2>/dev/null || true)" "config_init $init_args_label ($init_args_opts): o stderr é só a mensagem da biblioteca"
    assert_eq "" "$(cat "$init_args_dir/run.out" 2>/dev/null || echo 'sem stdout gravado')" "config_init $init_args_label ($init_args_opts): nada no stdout"
    assert_not_exists "$init_args_dir/cwd/.nick" "config_init $init_args_label ($init_args_opts): nada é criado"
  done
done

# init_write_error <cwd>: a mensagem exata de quando o nick.conf não pode ser gravado.
init_write_error() {
  printf '%s' "Erro: não foi possível gravar $(cd "$1" && pwd)/.nick/nick.conf."
}
# .nick/nick.conf sendo um diretório (vale também como root): a árvore é completada, mas a
# gravação falha com a mensagem da biblioteca, sem o erro cru do bash.
mkdir -p "$init_dir/conf_dir/.nick/nick.conf"
assert_eq "1" "$(run_init "$init_dir/conf_dir" "" "$init_dir/conf_dir")" ".nick/nick.conf sendo um diretório: config_init retorna 1"
assert_eq "1|depois" "$(cat "$init_dir/conf_dir.after" 2>/dev/null || true)" ".nick/nick.conf sendo um diretório: retorna 1 sem exit (o shell segue)"
assert_eq "$(init_write_error "$init_dir/conf_dir")" "$(cat "$init_dir/conf_dir.err" 2>/dev/null || true)" ".nick/nick.conf sendo um diretório: o stderr é só a mensagem da biblioteca (sem o erro cru do bash)"
assert_eq "" "$(cat "$init_dir/conf_dir.out" 2>/dev/null || echo 'sem stdout gravado')" ".nick/nick.conf sendo um diretório: nada no stdout"
assert_eq "true" "$([ -d "$init_dir/conf_dir/.nick/nick.conf" ] && echo true || echo false)" ".nick/nick.conf sendo um diretório: o diretório fica lá"
# .nick/nick.conf somente leitura, resposta s: o mesmo erro e o arquivo intacto. O root ignora a
# permissão, então o caso é pulado.
if [ "$(id -u)" -ne 0 ]; then
  mkdir -p "$init_dir/conf_ro/.nick"
  printf '%s\n' 'FAKE_TEXT="antigo"' > "$init_dir/conf_ro/.nick/nick.conf"
  chmod a-w "$init_dir/conf_ro/.nick/nick.conf"
  init_ro_rc="$(run_init "$init_dir/conf_ro" "s" "$init_dir/conf_ro")"
  chmod u+w "$init_dir/conf_ro/.nick/nick.conf"
  assert_eq "1" "$init_ro_rc" ".nick/nick.conf somente leitura, resposta s: config_init retorna 1"
  assert_eq "$(init_write_error "$init_dir/conf_ro")" "$(cat "$init_dir/conf_ro.err" 2>/dev/null || true)" ".nick/nick.conf somente leitura, resposta s: o stderr é só a mensagem da biblioteca"
  assert_eq 'FAKE_TEXT="antigo"' "$(cat "$init_dir/conf_ro/.nick/nick.conf" 2>/dev/null || true)" ".nick/nick.conf somente leitura, resposta s: o arquivo fica intacto"
else
  echo "  PULADO: .nick/nick.conf somente leitura não vale como root"
fi

# Sem set -e no chamador: os mesmos códigos, saídas e arquivos.
init_nose_case=0
for init_nose in "fresh|" "overwrite|N" "overwrite|" "overwrite|s" "blocked|"; do
  init_nose_case=$((init_nose_case + 1))
  init_nose_kind="${init_nose%%|*}"
  init_nose_input="${init_nose#*|}"
  for init_nose_opts in with without; do
    init_nose_dir="$init_dir/no_set_e_${init_nose_case}_$init_nose_opts"
    mkdir -p "$init_nose_dir/cwd"
    case "$init_nose_kind" in
      overwrite) mkdir -p "$init_nose_dir/cwd/.nick"; printf '%s\n' 'FAKE_TEXT="antigo"' > "$init_nose_dir/cwd/.nick/nick.conf" ;;
      blocked) mkdir -p "$init_nose_dir/cwd/.nick"; printf 'arquivo' > "$init_nose_dir/cwd/.nick/troca" ;;
    esac
    if [ "$init_nose_opts" = with ]; then
      run_init "$init_nose_dir/cwd" "$init_nose_input" "$init_nose_dir/run" >/dev/null
    else
      run_init "$init_nose_dir/cwd" "$init_nose_input" "$init_nose_dir/run" ":" >/dev/null
    fi
    (cd "$init_nose_dir/cwd" && find . -type f -exec cksum {} + | LC_ALL=C sort) > "$init_nose_dir/files"
    cat "$init_nose_dir/run.out" "$init_nose_dir/run.err" "$init_nose_dir/run.after" 2>/dev/null | sed "s#$init_nose_dir#<dir>#g" > "$init_nose_dir/all" || true
  done
  init_nose_label="$init_nose_kind (entrada '${init_nose_input:-EOF}')"
  assert_eq "igual" "$(cmp -s "$init_dir/no_set_e_${init_nose_case}_with/all" "$init_dir/no_set_e_${init_nose_case}_without/all" && echo igual || echo diferente)" "$init_nose_label: mesmo código e mesmas saídas sem set -e no chamador"
  assert_eq "igual" "$(cmp -s "$init_dir/no_set_e_${init_nose_case}_with/files" "$init_dir/no_set_e_${init_nose_case}_without/files" && echo igual || echo diferente)" "$init_nose_label: mesmos arquivos sem set -e no chamador"
  assert_eq "true" "$([ -s "$init_dir/no_set_e_${init_nose_case}_without/all" ] && echo true || echo false)" "$init_nose_label: sem set -e, o caso tem saída (amostra não vazia)"
done

echo
echo "== NS-53: registro com pergunta do init (5º argumento de config_register_key) =="
# A pergunta do init é o 5º argumento, opcional. Sem ele (4 argumentos) a chave não tem pergunta.
prompt_reg_dir="$TEST_TMP/prompt_register"
mkdir -p "$prompt_reg_dir"
assert_eq "0" "$(timeout 2 bash --norc --noprofile -c "source '$CONFIG_SH'; printf '%s' \"\${#CONFIG_KEY_PROMPTS[@]}\"" 2>/dev/null || echo 'sem CONFIG_KEY_PROMPTS')" "source lib/config/config.sh cria CONFIG_KEY_PROMPTS vazio"
assert_eq "4|[][][][]" "$(run_fake "printf '%s|' \"\${#CONFIG_KEY_PROMPTS[@]}\"; printf '[%s]' \"\${CONFIG_KEY_PROMPTS[@]}\"")" "registro com 4 argumentos: a chave entra em CONFIG_KEY_PROMPTS sem pergunta"
prompt_fake_question='URL de mentira (Enter para pular): '
assert_eq "0|$fake_keys_all FAKE_URL|5 5 5 5 5|[][][][][$prompt_fake_question]" "$(run_fake "t_rc=0; config_register_key FAKE_URL '' '' 'FAKE_URL: url de mentira.' '$prompt_fake_question' || t_rc=\$?; printf '%s|%s|%s %s %s %s %s|' \"\$t_rc\" \"\${CONFIG_KEYS[*]}\" \"\${#CONFIG_KEYS[@]}\" \"\${#CONFIG_KEY_DEFAULTS[@]}\" \"\${#CONFIG_KEY_NOTES[@]}\" \"\${#CONFIG_KEY_COMMENTS[@]}\" \"\${#CONFIG_KEY_PROMPTS[@]}\"; printf '[%s]' \"\${CONFIG_KEY_PROMPTS[@]}\"")" "registro com 5 argumentos: retorna 0 e guarda a pergunta, com os cinco arrays do mesmo tamanho"
assert_eq "0|5|[][][][][]" "$(run_fake "t_rc=0; config_register_key FAKE_URL '' '' 'FAKE_URL: url de mentira.' '' || t_rc=\$?; printf '%s|%s|' \"\$t_rc\" \"\${#CONFIG_KEY_PROMPTS[@]}\"; printf '[%s]' \"\${CONFIG_KEY_PROMPTS[@]}\"")" "pergunta vazia equivale a sem pergunta"
prompt_count_message() {
  printf '%s' "Erro: config_register_key espera 4 ou 5 argumentos (nome, padrão, nota, comentário e, opcional, a pergunta do init), recebeu $1."
}
reg_refuse "3 argumentos" FAKE_X "" ""
assert_eq "$(prompt_count_message 3)" "$(cat "$reg_dir/refuse_$reg_case.err" 2>/dev/null || true)" "3 argumentos: a mensagem diz que espera 4 ou 5"
reg_refuse "6 argumentos" FAKE_X "" "" "c" "p" "extra"
assert_eq "$(prompt_count_message 6)" "$(cat "$reg_dir/refuse_$reg_case.err" 2>/dev/null || true)" "6 argumentos: a mensagem diz que espera 4 ou 5"
reg_refuse "pergunta numa chave _ENABLED" FAKE_X_ENABLED "false" "" "c" "Ligar? "
assert_eq "Erro: config_register_key: a chave liga/desliga 'FAKE_X_ENABLED' não pode ter pergunta do init." "$(cat "$reg_dir/refuse_$reg_case.err" 2>/dev/null || true)" "pergunta numa chave _ENABLED: mensagem exata"
assert_eq "1|$fake_keys_all|4|depois" "$(run_fake "t_rc=0; config_register_key FAKE_Y_ENABLED 'false' '' 'c' 'Ligar? ' 2>/dev/null || t_rc=\$?; printf '%s|%s|%s|depois' \"\$t_rc\" \"\${CONFIG_KEYS[*]}\" \"\${#CONFIG_KEY_PROMPTS[@]}\"")" "pergunta numa chave _ENABLED: nada entra em CONFIG_KEY_PROMPTS"

echo
echo "== NS-53: config_print_skeleton <tool_name> [CHAVE=valor...] =="
pskel_dir="$TEST_TMP/skeleton_pairs"
mkdir -p "$pskel_dir"
run_fake "config_print_skeleton ferramenta-x > '$pskel_dir/no_pairs.conf'" >/dev/null
assert_eq "igual" "$(cmp -s "$skel_dir/expected.conf" "$pskel_dir/no_pairs.conf" && echo igual || echo diferente)" "sem pares: a saída é o retrato fixado de antes, byte a byte"
run_fake "config_print_skeleton ferramenta-x 'FAKE_TEXT=https://x/browse/' > '$pskel_dir/one_pair.conf'" >/dev/null
sed 's#^FAKE_TEXT=""$#FAKE_TEXT="https://x/browse/"#' "$skel_dir/expected.conf" > "$pskel_dir/one_pair.expected"
assert_eq "igual" "$(cmp -s "$pskel_dir/one_pair.expected" "$pskel_dir/one_pair.conf" && echo igual || echo diferente)" "com FAKE_TEXT=valor: só a linha FAKE_TEXT muda"
assert_eq "1" "$(diff "$skel_dir/expected.conf" "$pskel_dir/one_pair.conf" | grep -c '^>' || true)" "com FAKE_TEXT=valor: exatamente uma linha diferente do esqueleto sem pares"
# shellcheck disable=SC2016 # o $(id) é literal de propósito: o valor nunca é expandido.
pskel_literal='a $(id) `id` \n %s b=c'
run_fake "config_print_skeleton ferramenta-x 'FAKE_ON_ENABLED=false' $(printf '%q' "FAKE_TEXT=$pskel_literal") > '$pskel_dir/two_pairs.conf'" >/dev/null
assert_eq "FAKE_TEXT=\"$pskel_literal\"|FAKE_ON_ENABLED=\"false\"" "$(grep -E '^(FAKE_TEXT|FAKE_ON_ENABLED)=' "$pskel_dir/two_pairs.conf" 2>/dev/null | paste -sd '|' -)" "dois pares, em qualquer ordem: cada um troca a linha da sua chave, com o valor literal (sem expansão nem escape, e o = dentro do valor fica)"
for pskel_bad in "FAKE_UNKNOWN=x" "FAKE_TEXT" "=x"; do
  pskel_out="$(run_fake "t_rc=0; config_print_skeleton ferramenta-x 'FAKE_TEXT=ok' '$pskel_bad' > '$pskel_dir/bad.out' 2> '$pskel_dir/bad.err' || t_rc=\$?; printf '%s|depois' \"\$t_rc\"")"
  assert_eq "1|depois" "$pskel_out" "par '$pskel_bad': retorna 1, sem exit"
  assert_eq "" "$(cat "$pskel_dir/bad.out" 2>/dev/null || echo 'sem stdout gravado')" "par '$pskel_bad': nada impresso no stdout"
  assert_eq "Erro: config_print_skeleton: par inválido '$pskel_bad' (use CHAVE=valor com uma chave registrada)." "$(cat "$pskel_dir/bad.err" 2>/dev/null || true)" "par '$pskel_bad': mensagem exata no stderr"
done

echo
echo "== NS-53: config_init pergunta as chaves com pergunta, depois da sobrescrita e antes de criar =="
pinit_dir="$TEST_TMP/init_prompt"
mkdir -p "$pinit_dir"
# A chave com pergunta só existe nestes casos: FAKE_URL, de texto, registrada depois das quatro.
pinit_register="config_register_key FAKE_URL '' '' 'FAKE_URL: url de mentira.' '$prompt_fake_question'"
run_fake "$pinit_register; config_print_skeleton ferramenta-x > '$pinit_dir/skeleton.conf'" >/dev/null
assert_eq "true" "$(grep -qx 'FAKE_URL=""' "$pinit_dir/skeleton.conf" && echo true || echo false)" "o esqueleto com FAKE_URL registrada tem FAKE_URL=\"\" (amostra)"
# pinit_expected <valor>: o esqueleto com FAKE_URL="<valor>".
pinit_expected() {
  local value="$1"
  awk -v value="$value" '$0 == "FAKE_URL=\"\"" { print "FAKE_URL=\"" value "\""; next } { print }' "$pinit_dir/skeleton.conf" 2>/dev/null || true
}
# run_pinit <dir> <stdin> <out_prefix>: config_init ferramenta-x com FAKE_URL registrada.
run_pinit() {
  local dir="$1" input="$2" out_prefix="$3"
  run_in_dir "$dir" "$input" "$out_prefix" "$pinit_register; t_rc=0; config_init ferramenta-x || t_rc=\$?; printf '%s|depois' \"\$t_rc\" > '$out_prefix.after'; exit \"\$t_rc\""
}
pinit_refusal='Erro: a resposta não pode ter aspas duplas (") nem caracteres de controle. Tente de novo.'
pinit_case=0
# pinit_ok <descrição> <stdin> <valor gravado> <refusals>
pinit_ok() {
  local description="$1" input="$2" value="$3" refusals="$4" dir expected_err=""
  pinit_case=$((pinit_case + 1))
  dir="$pinit_dir/case_$pinit_case"
  mkdir -p "$dir/cwd"
  assert_eq "0" "$(run_pinit "$dir/cwd" "$input" "$dir/run")" "$description: config_init retorna 0"
  pinit_expected "$value" > "$dir/expected.conf"
  assert_eq "igual" "$(cmp -s "$dir/expected.conf" "$dir/cwd/.nick/nick.conf" && echo igual || echo diferente)" "$description: grava FAKE_URL=\"$value\" e o resto é o esqueleto"
  assert_eq "$(init_tree_expected)" "$(nick_tree "$dir/cwd")" "$description: a árvore de .nick/ é a de sempre"
  assert_eq "$(init_expected_output "$(cd "$dir/cwd" && pwd)")" "$(cat "$dir/run.out" 2>/dev/null || true)" "$description: stdout de sempre"
  for _ in $(seq 1 "$refusals"); do
    expected_err+="${expected_err:+$'\n'}$pinit_refusal"
  done
  assert_eq "$expected_err" "$(cat "$dir/run.err" 2>/dev/null || true)" "$description: stderr com $refusals recusa(s) e nada mais"
}
pinit_ok "resposta https://x/browse/" "https://x/browse/" "https://x/browse/" 0
pinit_ok "Enter (resposta vazia)" $'\n' "" 0
pinit_ok "EOF (Ctrl+D) na pergunta" "" "" 0
pinit_ok "espaços e tab nas pontas" $' \t https://x/browse/ \t' "https://x/browse/" 0
pinit_ok "espaço no meio fica" "a b" "a b" 0
# shellcheck disable=SC2016 # o $(touch ...) é literal de propósito.
pinit_ok "\$(...) e crase ficam literais" "\$(touch $pinit_dir/marker) \`touch $pinit_dir/marker\`" "\$(touch $pinit_dir/marker) \`touch $pinit_dir/marker\`" 0
assert_not_exists "$pinit_dir/marker" "\$(...) e crase na resposta: nada é executado"
pinit_ok "aspas duplas recusadas, depois uma resposta válida" $'a"b\nhttps://ok/' "https://ok/" 1
assert_not_contains "$(cat "$pinit_dir/case_$pinit_case/run.err" 2>/dev/null)" 'a"b' "aspas duplas: a mensagem não ecoa o valor"
pinit_ok "caractere de controle (ESC) recusado, depois uma resposta válida" $'a\e[31mb\nhttps://ok/' "https://ok/" 1
assert_eq "0" "$(grep -c $'\e' "$pinit_dir/case_$pinit_case/run.err" 2>/dev/null || true)" "caractere de controle: nenhum ESC no stderr (o valor não é ecoado)"
pinit_ok "tab no meio é caractere de controle" $'a\tb\nok' "ok" 1
pinit_ok "duas recusas seguidas: a pergunta se repete até uma resposta válida" $'"\n\x01\nfim' "fim" 2
pinit_ok "recusa seguida de EOF grava vazio" $'"' "" 1
pinit_case=$((pinit_case + 1))
pinit_c_dir="$pinit_dir/case_$pinit_case"
mkdir -p "$pinit_c_dir/cwd"
run_in_dir "$pinit_c_dir/cwd" "não" "$pinit_c_dir/run" "LC_ALL=C; $pinit_register; config_init ferramenta-x" >/dev/null
assert_eq 'FAKE_URL="não"' "$(grep '^FAKE_URL=' "$pinit_c_dir/cwd/.nick/nick.conf" 2>/dev/null || true)" "sob LC_ALL=C, acento não é caractere de controle: aceito"
assert_eq "" "$(cat "$pinit_c_dir/run.err" 2>/dev/null || true)" "sob LC_ALL=C, acento: nenhuma recusa"

# Arquivo existente: a pergunta vem só depois do "s". Com N (ou EOF), uma resposta inválida a
# seguir no stdin provaria que a pergunta foi feita (a recusa iria para o stderr).
mkdir -p "$pinit_dir/exists/.nick"
printf '%s\n' 'FAKE_TEXT="antigo"' > "$pinit_dir/exists/.nick/nick.conf"
assert_eq "0" "$(run_pinit "$pinit_dir/exists" $'N\na"b' "$pinit_dir/exists_no")" "arquivo existente, N: retorna 0"
assert_eq "Cancelado. Nada foi alterado." "$(cat "$pinit_dir/exists_no.out" 2>/dev/null || true)" "arquivo existente, N: stdout é só o cancelamento"
assert_eq "" "$(cat "$pinit_dir/exists_no.err" 2>/dev/null || true)" "arquivo existente, N: a pergunta não é feita (nenhuma recusa no stderr)"
assert_eq "$(printf '%s\n' .nick .nick/nick.conf)|FAKE_TEXT=\"antigo\"" "$(nick_tree "$pinit_dir/exists")|$(cat "$pinit_dir/exists/.nick/nick.conf")" "arquivo existente, N: nada muda"
assert_eq "1" "$(run_pinit "$pinit_dir/exists" "" "$pinit_dir/exists_eof")" "arquivo existente, EOF na sobrescrita: retorna 1"
assert_eq "Cancelado. Nada foi alterado." "$(cat "$pinit_dir/exists_eof.err" 2>/dev/null || true)" "arquivo existente, EOF na sobrescrita: só o cancelamento no stderr"
assert_eq "$(printf '%s\n' .nick .nick/nick.conf)|FAKE_TEXT=\"antigo\"" "$(nick_tree "$pinit_dir/exists")|$(cat "$pinit_dir/exists/.nick/nick.conf")" "arquivo existente, EOF na sobrescrita: nada muda"
assert_eq "0" "$(run_pinit "$pinit_dir/exists" $'s\nhttps://novo/' "$pinit_dir/exists_yes")" "arquivo existente, s e depois a URL: retorna 0"
pinit_expected "https://novo/" > "$pinit_dir/exists_yes.expected"
assert_eq "igual" "$(cmp -s "$pinit_dir/exists_yes.expected" "$pinit_dir/exists/.nick/nick.conf" && echo igual || echo diferente)" "arquivo existente, s e depois a URL: sobrescreve com a URL"

# Nada é criado antes das perguntas: com o stdin num FIFO, depois de uma resposta inválida a
# pergunta está esperando de novo, e a árvore .nick/ ainda não existe. O EOF depois disso grava "".
pfifo_dir="$pinit_dir/fifo"
mkdir -p "$pfifo_dir/cwd"
mkfifo "$pfifo_dir/in"
(cd "$pfifo_dir/cwd" && timeout 5 bash -c "set -euo pipefail; source '$FAKE_KEYS_SH'; $pinit_register; config_init ferramenta-x" < "$pfifo_dir/in" > "$pfifo_dir/run.out" 2> "$pfifo_dir/run.err") &
pfifo_pid=$!
exec {pfifo_fd}> "$pfifo_dir/in"
printf '%s\n' 'a"b' >&"$pfifo_fd"
for _ in $(seq 1 50); do
  grep -qF "$pinit_refusal" "$pfifo_dir/run.err" 2>/dev/null && break
  sleep 0.1
done
assert_eq "$pinit_refusal" "$(cat "$pfifo_dir/run.err" 2>/dev/null || true)" "FIFO: a resposta inválida foi recusada (a pergunta espera de novo)"
assert_not_exists "$pfifo_dir/cwd/.nick" "FIFO: com a pergunta ainda esperando, nada de .nick/ foi criado"
exec {pfifo_fd}>&-
pfifo_rc=0
wait "$pfifo_pid" || pfifo_rc=$?
assert_eq "0" "$pfifo_rc" "FIFO: o EOF depois da recusa não cancela (retorna 0)"
assert_eq 'FAKE_URL=""' "$(grep '^FAKE_URL=' "$pfifo_dir/cwd/.nick/nick.conf" 2>/dev/null || true)" "FIFO: o EOF grava FAKE_URL=\"\""
assert_eq "$(init_tree_expected)" "$(nick_tree "$pfifo_dir/cwd")" "FIFO: a árvore aparece depois das perguntas"

# O arquivo gerado carrega sem aviso, com o valor respondido.
assert_eq "https://x/browse/" "$(timeout 2 bash -c "source '$FAKE_KEYS_SH'; $pinit_register; config_load '$pinit_dir/case_1/cwd/.nick/nick.conf' 2> '$pinit_dir/load.err'; printf '%s' \"\$FAKE_URL\"" 2>/dev/null || true)" "o arquivo gerado carrega com config_load, com o valor respondido"
assert_eq "" "$(cat "$pinit_dir/load.err" 2>/dev/null || echo 'sem stderr')" "o arquivo gerado carrega sem nenhum aviso"

echo
echo "== genérico: lib/config/config.sh não conhece nenhuma ferramenta =="
assert_eq "" "$(grep -n 'auto_scrum' <<< "$(code_lines "$CONFIG_SH")" || true)" "lib/config/config.sh não cita auto_scrum fora de comentário"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
