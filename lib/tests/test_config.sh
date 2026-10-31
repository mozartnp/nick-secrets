#!/usr/bin/env bash
#
# test_config: testes em bash puro (sem framework) da biblioteca lib/config.sh. Cada caso roda
# num bash -c separado, que dá source na biblioteca e registra só chaves de mentira (FAKE_*):
# nenhuma chave de ferramenta real aparece aqui. A integração com o auto_scrum fica em
# auto_scrum/tests/test_auto_scrum.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_SH="$SCRIPT_DIR/../config.sh"

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
assert_eq "0" "$clean_rc" "source lib/config.sh num bash limpo termina com código 0"
assert_eq "" "$(cat "$clean_dir/out" 2>/dev/null || echo 'sem saída gravada')" "source lib/config.sh não imprime nada (stdout nem stderr)"
clean_new_vars="$(comm -13 "$clean_dir/vars_before" "$clean_dir/vars_after" 2>/dev/null | grep -vxE '_|BASH_.*|FUNCNAME|LINENO|PIPESTATUS' || true)"
clean_new_funcs="$(comm -13 "$clean_dir/funcs_before" "$clean_dir/funcs_after" 2>/dev/null | sed 's/^declare -f //' || true)"
assert_contains "$clean_new_vars" "CONFIG_KEYS" "source lib/config.sh cria CONFIG_KEYS"
assert_eq "" "$(grep -vE '^CONFIG_' <<< "$clean_new_vars" || true)" "toda variável nova começa com CONFIG_"
assert_eq "" "$(grep -vE '^config_' <<< "$clean_new_funcs" | grep . || true)" "toda função nova começa com config_"
clean_arrays="$(timeout 2 bash --norc --noprofile -c "source '$CONFIG_SH'; printf '%s|%s|%s|%s|%s' \"\$CONFIG_CWD_FILE_NAME\" \"\${#CONFIG_KEYS[@]}\" \"\${#CONFIG_KEY_DEFAULTS[@]}\" \"\${#CONFIG_KEY_NOTES[@]}\" \"\${#CONFIG_KEY_COMMENTS[@]}\"" 2>/dev/null || true)"
assert_eq ".nick.conf|0|0|0|0" "$clean_arrays" "CONFIG_CWD_FILE_NAME é .nick.conf e os quatro arrays do registro nascem vazios"

echo
echo "== carga limpa: estrutura do arquivo (sem shebang, sem bit de execução, sem set/exit/source/eval) =="
assert_eq "# shellcheck shell=bash" "$(head -n1 "$CONFIG_SH" 2>/dev/null || true)" "a 1ª linha é a diretiva '# shellcheck shell=bash' (sem shebang)"
assert_eq "sem bit de execução" "$([ -e "$CONFIG_SH" ] && [ ! -x "$CONFIG_SH" ] && echo 'sem bit de execução' || echo 'executável ou inexistente')" "lib/config.sh existe e não tem bit de execução"
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
echo "== config_cwd_file: imprime <cwd>/.nick.conf e retorna 0, só no cwd =="
cwd_dir="$TEST_TMP/cwd_file"
mkdir -p "$cwd_dir/without" "$cwd_dir/with/sub"
: > "$cwd_dir/with/.nick.conf"
# cwd_file <dir>: imprime "<saída de config_cwd_file>|<código>", com o cwd em <dir>.
cwd_file() {
  (cd "$1" && run_fake "t_rc=0; t_out=\"\$(config_cwd_file)\" || t_rc=\$?; printf '%s|%s' \"\$t_out\" \"\$t_rc\"")
}
assert_eq "$(cd "$cwd_dir/without" && pwd)/.nick.conf|0" "$(cwd_file "$cwd_dir/without")" "sem o arquivo: imprime <cwd>/.nick.conf e retorna 0"
assert_eq "$(cd "$cwd_dir/with" && pwd)/.nick.conf|0" "$(cwd_file "$cwd_dir/with")" "com o arquivo: imprime <cwd>/.nick.conf e retorna 0"
assert_eq "$(cd "$cwd_dir/with/sub" && pwd)/.nick.conf|0" "$(cwd_file "$cwd_dir/with/sub")" "num subdiretório de um diretório com .nick.conf: imprime o caminho do subdiretório, sem subir para o pai"

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
echo "== genérico: lib/config.sh não conhece nenhuma ferramenta =="
assert_eq "" "$(grep -n 'auto_scrum' <<< "$(code_lines "$CONFIG_SH")" || true)" "lib/config.sh não cita auto_scrum fora de comentário"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
