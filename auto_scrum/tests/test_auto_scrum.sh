#!/usr/bin/env bash
#
# test_auto_scrum: testes em bash puro (sem framework) pro auto_scrum.sh. Dá source no
# script (que precisa do guard BASH_SOURCE[0] = $0 pra não disparar main() sozinho) e
# roda asserts simples contra as funções expostas.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AUTO_SCRUM_SH="$SCRIPT_DIR/../auto_scrum.sh"

failures=0

# Diretório-base temporário único da suíte: cada teste usa um subdiretório dele, e nada é
# escrito dentro do repositório.
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

# Os assert_* (e o contador failures que eles incrementam) vêm do helper compartilhado.
# shellcheck source=SCRIPTDIR/../../lib/tests/assert.sh
source "$SCRIPT_DIR/../../lib/tests/assert.sh"

# run_loader <config_file> <out_prefix> [setup]: dá source no auto_scrum.sh num bash
# separado, roda [setup] (padrão: nada), chama load_project_config <config_file> com
# stdout em <out_prefix>.out e stderr em <out_prefix>.err e imprime as 5 chaves conhecidas
# separadas por '|'. O '|| true' mantém a suíte rodando (e reportando FAIL) mesmo se o
# bash de dentro morrer, ex: função ainda inexistente.
run_loader() {
  local config_file="$1" out_prefix="$2" setup="${3:-:}"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $setup; load_project_config '$config_file' >'$out_prefix.out' 2>'$out_prefix.err'; printf '%s|%s|%s|%s|%s' \"\$STACK_DESCRIPTION\" \"\$SENTRY_ENABLED\" \"\$PERMISSION_ENABLED\" \"\$JIRA_ENABLED\" \"\$PRODUCTION_ENABLED\"" 2>/dev/null || true
}

# run_resolve_stack <cwd> <projects_dir> <project_arg> <stdin> <out_prefix>: num bash
# separado, faz cd pra <cwd> ANTES do source (resolve_stack procura config no cwd), aponta
# PROJECTS_DIR pra <projects_dir> e roda resolve_stack <project_arg>, com stdout em
# <out_prefix>.out, stderr em <out_prefix>.err e o código de saída em <out_prefix>.rc.
# <stdin> vazio vira < /dev/null. Imprime as 5 chaves conhecidas separadas por '|'.
run_resolve_stack() {
  local cwd="$1" projects_dir="$2" project_arg="$3" input="$4" out_prefix="$5" rc=0 vars=""
  local cmd="cd '$cwd' || exit 99; source '$AUTO_SCRUM_SH'; PROJECTS_DIR='$projects_dir'; resolve_stack '$project_arg' >'$out_prefix.out' 2>'$out_prefix.err'; printf '%s|%s|%s|%s|%s' \"\$STACK_DESCRIPTION\" \"\$SENTRY_ENABLED\" \"\$PERMISSION_ENABLED\" \"\$JIRA_ENABLED\" \"\$PRODUCTION_ENABLED\""
  if [ -z "$input" ]; then
    vars="$(timeout 2 bash -c "$cmd" < /dev/null 2>/dev/null)" || rc=$?
  else
    vars="$(timeout 2 bash -c "$cmd" <<< "$input" 2>/dev/null)" || rc=$?
  fi
  echo "$rc" > "$out_prefix.rc"
  printf '%s' "$vars"
}

# run_init_project <cwd> <projects_dir> <stdin> <log_file>: num bash separado, faz cd pra
# <cwd> antes do source, aponta PROJECTS_DIR pra <projects_dir> e roda init_project com
# <stdin>; stdout+stderr vão pra <log_file>. Imprime o código de saída.
run_init_project() {
  local cwd="$1" projects_dir="$2" input="$3" log_file="$4" rc=0
  timeout 2 bash -c "cd '$cwd' || exit 99; source '$AUTO_SCRUM_SH'; PROJECTS_DIR='$projects_dir'; init_project" <<< "$input" > "$log_file" 2>&1 || rc=$?
  printf '%s' "$rc"
}

# config_keys_blank [except_key]: imprime uma linha CHAVE="" pra cada chave de
# CONFIG_KEYS (registrada pelo próprio script), menos <except_key>. Serve pra montar
# fixtures completas, que não disparam o aviso de chave ausente.
config_keys_blank() {
  local except_key="${1:-}"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; for key in \"\${CONFIG_KEYS[@]}\"; do [ \"\$key\" = '$except_key' ] || printf '%s=\"\"\n' \"\$key\"; done"
}

# Nota de PRODUCTION_ENABLED: é a única chave com texto próprio nos avisos, porque o padrão
# desligado dela tira do prompt do PO uma pergunta que já existia.
production_note='a pergunta de impacto em produção está desligada no prompt do PO. Use "true" para ligá-la ou "false" para mantê-la desligada.'
production_note_line="# PRODUCTION_ENABLED: $production_note"
# Nota de TDD_ENABLED: é a única flag ligada por padrão, então a nota diz que a TDD continua
# ligada (e não "a opção está desligada.").
tdd_note='a metodologia TDD continua ligada nos prompts de Tech Leader, Desenvolvimento e Review. Use "false" só em projeto sem testes automatizados.'
tdd_note_line="# TDD_ENABLED: $tdd_note"
# "Aviso de chave ausente" é o bloco do stderr cuja 1ª linha contém missing_marker.
missing_marker="não tem todas as chaves da configuração"

# missing_warning <file> [linha...]: imprime, sem \n final, o texto exato do aviso de chave
# ausente pra <file>, com uma linha "  <linha>" pra cada argumento.
missing_warning() {
  local file="$1" line
  shift
  printf '%s' "Aviso: $file não tem todas as chaves da configuração — cada chave ausente funciona como o valor sugerido abaixo. Adicione ao arquivo:"
  for line in "$@"; do
    printf '\n  %s' "$line"
  done
}

# count_missing_warnings <file>: imprime quantos avisos de chave ausente há em <file>
# (nada, se o arquivo não existir).
count_missing_warnings() {
  local file="$1"
  grep -cF "$missing_marker" "$file" 2>/dev/null || true
}

# count_warnings <file>: imprime quantos avisos há em <file> (linhas que começam com
# "Aviso:"; nada, se o arquivo não existir). No menu de projetos, o select deixa o prompt
# "#? " sem \n logo antes do aviso, na mesma linha — por isso o prefixo opcional.
count_warnings() {
  local file="$1"
  grep -cE '^(#\? )?Aviso:' "$file" 2>/dev/null || true
}

# Fim padrão do aviso de valor inválido, pra chave sem nota própria.
default_off_note="a opção está desligada."

# value_warning <arquivo:linha> <chave> <valor> <desligada>: imprime, sem \n final, o texto
# exato do aviso de valor inválido.
value_warning() {
  local location="$1" key="$2" value="$3" off="$4"
  printf '%s' "Aviso: $location: valor \"$value\" inválido para $key (valores aceitos: \"true\", \"false\" ou \"\") — $off"
}

# loaded_value <arquivo> <chave>: imprime o valor que load_project_config atribui a <chave>.
loaded_value() {
  local file="$1" key="$2"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; load_project_config '$file' 2>/dev/null; loaded_key='$key'; printf '%s' \"\${!loaded_key}\"" 2>/dev/null || true
}

echo "== source guard: sourcing auto_scrum.sh não deve disparar main() =="
smoke_result=0
timeout 2 bash -c "source '$AUTO_SCRUM_SH' < /dev/null; declare -F choose_type >/dev/null" \
  < /dev/null > /dev/null 2>&1 || smoke_result=$?
assert_eq "0" "$smoke_result" "source não roda main() nem trava esperando input"

echo
echo "== choose_type: po_jira só aparece com JIRA_ENABLED=true (opt-in por projeto) =="
type_result=""
type_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; JIRA_ENABLED=true; choose_type <<< '3' >/dev/null 2>&1; printf '%s' \"\$TYPE\"")"
assert_eq "po_jira" "$type_result" "com JIRA_ENABLED=true, opção 3 seleciona TYPE=po_jira"

type_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; JIRA_ENABLED=true; choose_type <<< '4' >/dev/null 2>&1; printf '%s' \"\$TYPE\"")"
assert_eq "tech_leader" "$type_result" "com JIRA_ENABLED=true, opção 4 continua tech_leader"

type_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; choose_type <<< '3' >/dev/null 2>&1; printf '%s' \"\$TYPE\"")"
assert_eq "tech_leader" "$type_result" "sem JIRA_ENABLED (padrão false), po_jira some do menu — opção 3 vira tech_leader"

type_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; JIRA_ENABLED=false; choose_type <<< '3' >/dev/null 2>&1; printf '%s' \"\$TYPE\"")"
assert_eq "tech_leader" "$type_result" "com JIRA_ENABLED=false explícito, po_jira também some do menu"

echo
echo "== init_project: pergunta o destino (.nick.conf no cwd ou projects/<nome>.conf) e gera o mesmo esqueleto =="
init_dir="$TEST_TMP/init_project"
init_cwd_dest="$init_dir/cwd_dest"
init_projects_dest="$init_dir/projects_dest"
init_projects="$init_dir/projects"
init_projects_unused="$init_dir/projects_nao_criado"
mkdir -p "$init_cwd_dest" "$init_projects_dest"

init_rc="$(run_init_project "$init_cwd_dest" "$init_projects_unused" "1" "$init_dir/cwd_dest.log")"
init_cwd_file="$init_cwd_dest/.nick.conf"
assert_eq "0" "$init_rc" "destino cwd: init_project termina com código 0"
assert_exists "$init_cwd_file" "destino cwd (opção 1): cria .nick.conf no diretório atual"
assert_contains "$(cat "$init_cwd_file" 2>/dev/null)" 'JIRA_ENABLED="false"' "destino cwd: esqueleto declara JIRA_ENABLED=\"false\" por padrão"
assert_contains "$(cat "$init_cwd_file" 2>/dev/null)" 'PRODUCTION_ENABLED="false"' "destino cwd: esqueleto declara PRODUCTION_ENABLED=\"false\" por padrão"
assert_contains "$(cat "$init_cwd_file" 2>/dev/null)" 'Ligue ("true") quando o projeto entrar em produção.' "destino cwd: comentário do esqueleto manda ligar PRODUCTION_ENABLED quando o projeto entrar em produção"
assert_contains "$(cat "$init_cwd_file" 2>/dev/null)" 'TDD_ENABLED="true"' "destino cwd: esqueleto declara TDD_ENABLED=\"true\" por padrão (a única flag ligada por padrão)"
assert_contains "$(cat "$init_cwd_file" 2>/dev/null)" 'só "false" desliga.' "destino cwd: comentário do esqueleto diz que só \"false\" desliga a TDD"
assert_contains "$(cat "$init_cwd_file" 2>/dev/null)" '# Mantenha todas as chaves abaixo, mesmo as que não usar (o auto_scrum avisa quando falta alguma). Nas chaves *_ENABLED, use só "true", "false" ou "".' "destino cwd: comentário do esqueleto traz as regras de chave ausente e de valor das chaves *_ENABLED"
assert_not_exists "$init_projects_unused" "destino cwd: PROJECTS_DIR não é criado"

init_rc="$(run_init_project "$init_projects_dest" "$init_projects" $'2\nprojeto-teste' "$init_dir/projects_dest.log")"
init_projects_file="$init_projects/projeto-teste.conf"
assert_eq "0" "$init_rc" "destino projects/: init_project termina com código 0"
assert_exists "$init_projects_file" "destino projects/ (opção 2): cria \$PROJECTS_DIR/projeto-teste.conf"
assert_contains "$(cat "$init_projects_file" 2>/dev/null)" 'JIRA_ENABLED="false"' "destino projects/: esqueleto declara JIRA_ENABLED=\"false\" por padrão"
assert_contains "$(cat "$init_projects_file" 2>/dev/null)" 'PRODUCTION_ENABLED="false"' "destino projects/: esqueleto declara PRODUCTION_ENABLED=\"false\" por padrão"
assert_contains "$(cat "$init_projects_file" 2>/dev/null)" 'Ligue ("true") quando o projeto entrar em produção.' "destino projects/: comentário do esqueleto manda ligar PRODUCTION_ENABLED quando o projeto entrar em produção"
assert_contains "$(cat "$init_projects_file" 2>/dev/null)" 'TDD_ENABLED="true"' "destino projects/: esqueleto declara TDD_ENABLED=\"true\" por padrão (a única flag ligada por padrão)"
assert_contains "$(cat "$init_projects_file" 2>/dev/null)" 'só "false" desliga.' "destino projects/: comentário do esqueleto diz que só \"false\" desliga a TDD"
assert_contains "$(cat "$init_projects_file" 2>/dev/null)" '# Mantenha todas as chaves abaixo, mesmo as que não usar (o auto_scrum avisa quando falta alguma). Nas chaves *_ENABLED, use só "true", "false" ou "".' "destino projects/: comentário do esqueleto traz as regras de chave ausente e de valor das chaves *_ENABLED"
assert_not_exists "$init_projects/projeto-teste.sh" "destino projects/: não cria projeto-teste.sh"
assert_not_exists "$init_projects_dest/.nick.conf" "destino projects/: não cria .nick.conf no diretório atual"

assert_eq "$(cat "$init_cwd_file" 2>/dev/null || echo 'sem cwd')" "$(cat "$init_projects_file" 2>/dev/null || echo 'sem projects')" "os dois destinos geram o mesmo conteúdo"
assert_eq "igual" "$(cmp -s "$init_cwd_file" "$init_projects_file" && echo igual || echo diferente)" "os dois destinos geram arquivos idênticos byte a byte (cmp, inclusive \\n final)"

init_overwrite="$init_dir/overwrite"
mkdir -p "$init_overwrite"
printf '%s\n' 'STACK_DESCRIPTION="antigo"' > "$init_overwrite/.nick.conf"
run_init_project "$init_overwrite" "$init_projects_unused" $'1\nN' "$init_dir/overwrite_no.log" >/dev/null
assert_eq 'STACK_DESCRIPTION="antigo"' "$(cat "$init_overwrite/.nick.conf")" "destino cwd com .nick.conf existente e resposta N: arquivo inalterado"
assert_contains "$(cat "$init_dir/overwrite_no.log")" "Cancelado." "destino cwd com .nick.conf existente e resposta N: cancela"
run_init_project "$init_overwrite" "$init_projects_unused" $'1\ns' "$init_dir/overwrite_yes.log" >/dev/null
assert_eq "igual" "$(cmp -s "$init_overwrite/.nick.conf" "$init_cwd_file" && echo igual || echo diferente)" "destino cwd com .nick.conf existente e resposta s: arquivo substituído pelo esqueleto"

printf '%s\n' 'STACK_DESCRIPTION="antigo"' > "$init_projects/projeto-teste.conf"
run_init_project "$init_projects_dest" "$init_projects" $'2\nprojeto-teste\nN' "$init_dir/overwrite_projects_no.log" >/dev/null
assert_eq 'STACK_DESCRIPTION="antigo"' "$(cat "$init_projects/projeto-teste.conf")" "destino projects/ com <nome>.conf existente e resposta N: arquivo inalterado"
assert_contains "$(cat "$init_dir/overwrite_projects_no.log")" "Cancelado." "destino projects/ com <nome>.conf existente e resposta N: cancela"

# init_project não chama o leitor, então não avisa — nem nos casos de sobrescrita, em que o
# arquivo que já existia é incompleto e um aviso seria um erro real.
for init_log in cwd_dest projects_dest overwrite_no overwrite_yes overwrite_projects_no; do
  assert_not_contains "$(cat "$init_dir/$init_log.log" 2>/dev/null || true)" "Aviso:" "init_project ($init_log.log): nenhum aviso"
done

init_main="$init_dir/main_init"
mkdir -p "$init_main/cwd"
printf '%s\n' 'STACK_DESCRIPTION="antigo"' > "$init_main/cwd/.nick.conf"
init_main_rc=0
# stdin: destino 1 (cwd) e N pra não sobrescrever. LOGS_DIR e PROJECTS_DIR temporários pra
# não gravar em auto_scrum/logs/ nem ler auto_scrum/projects/.
init_main_output="$(timeout 5 bash -c "cd '$init_main/cwd' || exit 99; source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$init_main/logs'; PROJECTS_DIR='$init_main/projects'; main --init" <<< $'1\nN' 2>&1)" || init_main_rc=$?
assert_eq "0" "$init_main_rc" "main --init sobre .nick.conf incompleto, resposta N: termina com código 0"
assert_contains "$init_main_output" "Cancelado." "main --init sobre .nick.conf incompleto, resposta N: cancela"
assert_not_contains "$init_main_output" "Aviso:" "main --init sobre .nick.conf incompleto: nenhum aviso"
assert_eq 'STACK_DESCRIPTION="antigo"' "$(cat "$init_main/cwd/.nick.conf")" "main --init sobre .nick.conf incompleto, resposta N: arquivo inalterado"

init_eof="$init_dir/eof"
mkdir -p "$init_eof"
for init_eof_input in "" "9"; do
  init_rc="$(run_init_project "$init_eof" "$init_projects_unused" "$init_eof_input" "$init_dir/eof.log")"
  assert_eq "1" "$init_rc" "stdin fecha no select de destino (entrada: '$init_eof_input' + EOF): init_project sai com código 1"
  assert_contains "$(cat "$init_dir/eof.log")" "Cancelado. Nada foi alterado." "stdin fecha no select de destino (entrada: '$init_eof_input' + EOF): mensagem de cancelado"
  assert_not_exists "$init_eof/.nick.conf" "stdin fecha no select de destino (entrada: '$init_eof_input' + EOF): nenhum .nick.conf é criado"
  assert_not_exists "$init_projects_unused" "stdin fecha no select de destino (entrada: '$init_eof_input' + EOF): PROJECTS_DIR não é criado"
done

echo
echo "== init_project: esqueleto amigável a lint/pre-commit e consistente com load_project_config =="
init_first_line="$(head -n1 "$init_cwd_file" 2>/dev/null || true)"
assert_eq "sem shebang" "$(case "$init_first_line" in '#!'*) echo 'com shebang' ;; *) echo 'sem shebang' ;; esac)" "esqueleto não começa com shebang"
assert_eq "0" "$(grep -c '[[:space:]]$' "$init_cwd_file" 2>/dev/null || true)" "esqueleto não tem espaço (nem \\r) no fim de nenhuma linha"
assert_eq '\n' "$(tail -c1 "$init_cwd_file" 2>/dev/null | od -An -c | tr -d ' ')" "esqueleto termina com \\n"

skeleton_result="$(run_loader "$init_cwd_file" "$init_dir/skeleton_loader" "STACK_DESCRIPTION=antes")"
assert_eq "|false|false|false|false" "$skeleton_result" "esqueleto carregado pelo leitor: flags false e STACK_DESCRIPTION vazio"
assert_eq "" "$(cat "$init_dir/skeleton_loader.err" 2>/dev/null || echo 'sem stderr')" "esqueleto carregado pelo leitor não gera aviso (nem pelos comentários que citam CHAVE=\"valor\")"
assert_eq "true" "$(loaded_value "$init_cwd_file" TDD_ENABLED)" "esqueleto carregado pelo leitor: TDD_ENABLED vem \"true\""

skeleton_keys="$(grep -oE '^[A-Z_]+=' "$init_cwd_file" 2>/dev/null | tr -d '=' | sort || true)"
config_keys="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; printf '%s\n' \"\${CONFIG_KEYS[@]}\" | sort")"
assert_eq "$config_keys" "$skeleton_keys" "chaves atribuídas no esqueleto são exatamente as de CONFIG_KEYS"

# Testes-guarda da regra liga/desliga (config_is_toggle_key, em lib/config.sh): o esqueleto é o modelo, então
# toda chave com "false" ou "true" nele precisa passar pela checagem de valor, e toda chave
# com "" não. skeleton_toggle_lines guarda a linha inteira, com o padrão da chave.
skeleton_guard_dir="$init_dir/skeleton_guard"
mkdir -p "$skeleton_guard_dir"
skeleton_toggle_lines=()
skeleton_text_keys=()
skeleton_other_lines=""
while IFS= read -r skeleton_line; do
  skeleton_key="${skeleton_line%%=*}"
  if [ "$skeleton_line" = "$skeleton_key=\"false\"" ] || [ "$skeleton_line" = "$skeleton_key=\"true\"" ]; then
    skeleton_toggle_lines+=("$skeleton_line")
  elif [ "$skeleton_line" = "$skeleton_key=\"\"" ]; then
    skeleton_text_keys+=("$skeleton_key")
  else
    skeleton_other_lines+="$skeleton_line"$'\n'
  fi
done < <(grep -E '^[A-Z_]+=' "$init_cwd_file" 2>/dev/null || true)
assert_eq "" "$skeleton_other_lines" "toda chave do esqueleto é liga/desliga (=\"false\"/=\"true\") ou texto (=\"\")"
assert_eq "true" "$([ "${#skeleton_toggle_lines[@]}" -gt 0 ] && [ "${#skeleton_text_keys[@]}" -gt 0 ] && echo true || echo false)" "o esqueleto tem pelo menos uma chave liga/desliga e uma de texto"

for skeleton_line in "${skeleton_toggle_lines[@]}"; do
  skeleton_key="${skeleton_line%%=*}"
  skeleton_copy="$skeleton_guard_dir/toggle_$skeleton_key.conf"
  sed "s/^$skeleton_line\$/$skeleton_key=\"sim\"/" "$init_cwd_file" > "$skeleton_copy"
  run_loader "$skeleton_copy" "$skeleton_guard_dir/toggle_$skeleton_key" >/dev/null
  assert_eq "true" "$(grep -qxF "$skeleton_key=\"sim\"" "$skeleton_copy" && echo true || echo false)" "esqueleto com $skeleton_line trocado por $skeleton_key=\"sim\": a cópia tem a linha trocada"
  assert_eq "1" "$(count_warnings "$skeleton_guard_dir/toggle_$skeleton_key.err")" "esqueleto com $skeleton_key=\"sim\": exatamente um aviso (toda liga/desliga do modelo é checada)"
  assert_contains "$(cat "$skeleton_guard_dir/toggle_$skeleton_key.err" 2>/dev/null || true)" "valor \"sim\" inválido para $skeleton_key" "esqueleto com $skeleton_key=\"sim\": o aviso é o de valor inválido"
done

for skeleton_key in "${skeleton_text_keys[@]}"; do
  skeleton_copy="$skeleton_guard_dir/text_$skeleton_key.conf"
  sed "s/^$skeleton_key=\"\"\$/$skeleton_key=\"sim\"/" "$init_cwd_file" > "$skeleton_copy"
  run_loader "$skeleton_copy" "$skeleton_guard_dir/text_$skeleton_key" >/dev/null
  assert_eq "true" "$(grep -qxF "$skeleton_key=\"sim\"" "$skeleton_copy" && echo true || echo false)" "esqueleto com $skeleton_key=\"sim\": a cópia tem a linha trocada"
  assert_eq "0" "$(count_warnings "$skeleton_guard_dir/text_$skeleton_key.err")" "esqueleto com $skeleton_key=\"sim\": nenhum aviso (chave de texto não é checada)"
done

# Linhas sugeridas pelo aviso de chave ausente, sem os dois espaços do começo.
: > "$skeleton_guard_dir/empty.conf"
run_loader "$skeleton_guard_dir/empty.conf" "$skeleton_guard_dir/empty" >/dev/null
skeleton_suggested_keys=""
skeleton_unmatched_lines=""
while IFS= read -r skeleton_line; do
  skeleton_line="${skeleton_line#  }"
  skeleton_suggested_keys+="${skeleton_line%%=*}"$'\n'
  if ! grep -qxF "$skeleton_line" "$init_cwd_file"; then
    skeleton_unmatched_lines+="$skeleton_line"$'\n'
  fi
done <<< "$(grep -E '^  [A-Z_]+=' "$skeleton_guard_dir/empty.err" 2>/dev/null || true)"
assert_eq "" "$skeleton_unmatched_lines" "cada linha sugerida pelo aviso de chave ausente é idêntica à linha do esqueleto"
config_keys_ordered="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; printf '%s\n' \"\${CONFIG_KEYS[@]}\"")"
assert_eq "$config_keys_ordered" "$(printf '%s' "$skeleton_suggested_keys")" "as chaves sugeridas pelo aviso de chave ausente são exatamente as de CONFIG_KEYS, na mesma ordem"

# Ida e volta: o bloco sugerido (o aviso menos a 1ª linha, inclusive as linhas de nota),
# colado como está num arquivo novo, carrega sem nenhum aviso.
tail -n +2 "$skeleton_guard_dir/empty.err" > "$skeleton_guard_dir/pasted.conf" 2>/dev/null || true
skeleton_pasted_result="$(run_loader "$skeleton_guard_dir/pasted.conf" "$skeleton_guard_dir/pasted" "STACK_DESCRIPTION=antes")"
assert_eq "|false|false|false|false" "$skeleton_pasted_result" "bloco do aviso de chave ausente colado num arquivo: as chaves carregam com o padrão do esqueleto"
assert_eq "" "$(cat "$skeleton_guard_dir/pasted.err" 2>/dev/null || echo 'sem stderr')" "bloco do aviso de chave ausente colado num arquivo: carrega sem nenhum aviso"

# Chave nova só no registro (config_register_key): entra nas duas checagens sem mudar a lógica.
skeleton_new_setup="config_register_key NEW_FLAG_ENABLED 'false' '' 'NEW_FLAG_ENABLED: flag nova.'; config_register_key NEW_TEXT '' '' 'NEW_TEXT: texto novo.'"
skeleton_new_file="$skeleton_guard_dir/new_keys.conf"
{ cat "$init_cwd_file"; printf '%s\n' 'NEW_FLAG_ENABLED="sim"' 'NEW_TEXT="sim"'; } > "$skeleton_new_file"
skeleton_new_line="$(grep -nxF 'NEW_FLAG_ENABLED="sim"' "$skeleton_new_file" | cut -d: -f1)"
run_loader "$skeleton_new_file" "$skeleton_guard_dir/new_keys" "$skeleton_new_setup" >/dev/null
assert_eq "1" "$(count_warnings "$skeleton_guard_dir/new_keys.err")" "chaves novas NEW_FLAG_ENABLED/NEW_TEXT com \"sim\": exatamente um aviso"
assert_eq "$(value_warning "$skeleton_new_file:$skeleton_new_line" NEW_FLAG_ENABLED sim "$default_off_note")" "$(cat "$skeleton_guard_dir/new_keys.err" 2>/dev/null || true)" "chaves novas com \"sim\": só NEW_FLAG_ENABLED recebe o aviso de valor"

run_loader "$init_cwd_file" "$skeleton_guard_dir/new_keys_missing" "$skeleton_new_setup" >/dev/null
assert_eq "1" "$(count_warnings "$skeleton_guard_dir/new_keys_missing.err")" "chaves novas ausentes do esqueleto: exatamente um aviso"
assert_eq "$(missing_warning "$init_cwd_file" 'NEW_FLAG_ENABLED="false"' 'NEW_TEXT=""')" "$(cat "$skeleton_guard_dir/new_keys_missing.err" 2>/dev/null || true)" "chaves novas ausentes do esqueleto: o aviso de chave ausente lista as duas, com o padrão de cada tipo"

config_keys_blank > "$skeleton_guard_dir/blank.conf"
run_loader "$skeleton_guard_dir/blank.conf" "$skeleton_guard_dir/blank" >/dev/null
assert_eq "" "$(cat "$skeleton_guard_dir/blank.err" 2>/dev/null || echo 'sem stderr')" "config_keys_blank monta uma fixture completa: carrega sem nenhum aviso"

echo
echo "== ask_questions_po_jira: pede link (obrigatório) e descrição extra (opcional) =="
jira_input=$'https://jira.example.com/browse/SC-123\n'
jira_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; ask_questions_po_jira >/dev/null 2>&1; printf '%s|%s' \"\$JIRA_LINK\" \"\$DESCRIPTION\"" <<< "$jira_input")"
assert_eq "https://jira.example.com/browse/SC-123|" "$jira_result" "link setado em JIRA_LINK e DESCRIPTION vazio quando não informada"

jira_input_retry=$'\nhttps://jira.example.com/browse/SC-123\n'
jira_result_retry="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; ask_questions_po_jira >/dev/null 2>&1; printf '%s|%s' \"\$JIRA_LINK\" \"\$DESCRIPTION\"" <<< "$jira_input_retry")"
assert_eq "https://jira.example.com/browse/SC-123|" "$jira_result_retry" "read_required repete quando a primeira linha vem vazia, só aceita a segunda"

echo
echo "== main(): roteamento estrutural pro po_jira (case/export/envsubst) =="
main_src="$(cat "$AUTO_SCRUM_SH")"
assert_contains "$main_src" "po_jira) ask_questions_po_jira ;;" "case \"\$TYPE\" roteia po_jira pra ask_questions_po_jira"

export_block="$(grep -A3 '^  export ' "$AUTO_SCRUM_SH")"
assert_contains "$export_block" "JIRA_LINK" "JIRA_LINK está na lista de export de main()"

envsubst_line="$(grep -F "envsubst '" "$AUTO_SCRUM_SH")"
# shellcheck disable=SC2016
# Aspas simples intencionais: '$JIRA_LINK' é o texto literal buscado dentro da linha do
# envsubst, não uma variável pra expandir aqui.
assert_contains "$envsubst_line" '$JIRA_LINK' "\$JIRA_LINK está na string de variáveis passada pro envsubst"

echo
echo "== template po_jira.md: renderização via envsubst =="
PO_JIRA_TEMPLATE="$SCRIPT_DIR/../templates/po_jira.md"
if [ -f "$PO_JIRA_TEMPLATE" ]; then
  # shellcheck disable=SC2016
  # Aspas simples intencionais: é a lista de variáveis pro envsubst expandir, não
  # queremos que o bash expanda antes (mesmo padrão do envsubst em auto_scrum.sh).
  rendered="$(PO_VALIDATION_BLOCK="[validação de exemplo]" PO_TICKET_FORMAT_BLOCK="[formato de exemplo]" JIRA_LINK="https://jira.example.com/browse/SC-999" DESCRIPTION="descrição extra de exemplo" envsubst '$PO_VALIDATION_BLOCK $PO_TICKET_FORMAT_BLOCK $JIRA_LINK $DESCRIPTION' < "$PO_JIRA_TEMPLATE")"
  assert_contains "$rendered" "https://jira.example.com/browse/SC-999" "JIRA_LINK de exemplo aparece literalmente no output"
  assert_contains "$rendered" "descrição extra de exemplo" "DESCRIPTION de exemplo aparece literalmente no output"
  # shellcheck disable=SC2016
  # Aspas simples intencionais: '${' é o texto literal buscado no output renderizado.
  assert_not_contains "$rendered" '${' "nenhuma variável \${...} sobra sem substituir no output"
else
  echo "  FAIL: templates/po_jira.md não existe"
  failures=$((failures + 1))
fi

echo
echo "== choose_type: tech_leader_jira só aparece com JIRA_ENABLED=true, logo após tech_leader =="
type_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; JIRA_ENABLED=true; choose_type <<< '5' >/dev/null 2>&1; printf '%s' \"\$TYPE\"")"
assert_eq "tech_leader_jira" "$type_result" "com JIRA_ENABLED=true, opção 5 seleciona TYPE=tech_leader_jira"

type_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; JIRA_ENABLED=true; choose_type <<< '6' >/dev/null 2>&1; printf '%s' \"\$TYPE\"")"
assert_eq "development" "$type_result" "com JIRA_ENABLED=true, opção 6 seleciona TYPE=development"

type_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; JIRA_ENABLED=true; choose_type <<< '7' >/dev/null 2>&1; printf '%s' \"\$TYPE\"")"
assert_eq "review" "$type_result" "com JIRA_ENABLED=true, opção 7 seleciona TYPE=review"

type_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; choose_type <<< '4' >/dev/null 2>&1; printf '%s' \"\$TYPE\"")"
assert_eq "development" "$type_result" "sem JIRA_ENABLED (padrão false), menu continua com 5 itens — opção 4 é development"

echo
echo "== ask_questions_tech_leader_jira: pede link (obrigatório) e contexto extra (opcional) =="
tl_jira_input=$'https://jira.example.com/browse/SC-321\n'
tl_jira_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; ask_questions_tech_leader_jira >/dev/null 2>&1; printf '%s|%s' \"\$JIRA_LINK\" \"\$DESCRIPTION\"" <<< "$tl_jira_input")"
assert_eq "https://jira.example.com/browse/SC-321|" "$tl_jira_result" "link setado em JIRA_LINK e DESCRIPTION vazio quando não informada"

tl_jira_input_retry=$'\nhttps://jira.example.com/browse/SC-321\n'
tl_jira_result_retry="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; ask_questions_tech_leader_jira >/dev/null 2>&1; printf '%s|%s' \"\$JIRA_LINK\" \"\$DESCRIPTION\"" <<< "$tl_jira_input_retry")"
assert_eq "https://jira.example.com/browse/SC-321|" "$tl_jira_result_retry" "read_required repete quando a primeira linha vem vazia, só aceita a segunda"

echo
echo "== main(): roteamento estrutural pro tech_leader_jira (case) =="
assert_contains "$main_src" "tech_leader_jira) ask_questions_tech_leader_jira ;;" "case \"\$TYPE\" roteia tech_leader_jira pra ask_questions_tech_leader_jira"

echo
echo "== build_tech_leader_blocks: monta TECH_LEADER_PLAN_RULES_BLOCK (compartilhado entre tech_leader.md e tech_leader_jira.md) =="
tl_rules_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; build_tdd_blocks; build_tech_leader_blocks; printf '%s' \"\$TECH_LEADER_PLAN_RULES_BLOCK\"")"
assert_contains "$tl_rules_result" "## Critérios de aceite" "TECH_LEADER_PLAN_RULES_BLOCK inclui a seção Critérios de aceite"
assert_contains "$tl_rules_result" "## Habilidades necessárias" "TECH_LEADER_PLAN_RULES_BLOCK inclui a seção Habilidades necessárias"
assert_contains "$tl_rules_result" "metodologia TDD" "TECH_LEADER_PLAN_RULES_BLOCK menciona a metodologia TDD"

echo
echo "== main(): TECH_LEADER_PLAN_RULES_BLOCK exportada e na lista do envsubst =="
export_block="$(grep -A3 '^  export ' "$AUTO_SCRUM_SH")"
assert_contains "$export_block" "TECH_LEADER_PLAN_RULES_BLOCK" "TECH_LEADER_PLAN_RULES_BLOCK está na lista de export de main()"
envsubst_line="$(grep -F "envsubst '" "$AUTO_SCRUM_SH")"
# shellcheck disable=SC2016
# Aspas simples intencionais: '$TECH_LEADER_PLAN_RULES_BLOCK' é o texto literal buscado
# dentro da linha do envsubst, não uma variável pra expandir aqui.
assert_contains "$envsubst_line" '$TECH_LEADER_PLAN_RULES_BLOCK' "\$TECH_LEADER_PLAN_RULES_BLOCK está na string de variáveis passada pro envsubst"

echo
echo "== templates tech_leader.md / tech_leader_jira.md: regra compartilhada não duplicada como texto literal =="
TECH_LEADER_TEMPLATE="$SCRIPT_DIR/../templates/tech_leader.md"
TECH_LEADER_JIRA_TEMPLATE="$SCRIPT_DIR/../templates/tech_leader_jira.md"
for tpl in "$TECH_LEADER_TEMPLATE" "$TECH_LEADER_JIRA_TEMPLATE"; do
  tpl_src="$(cat "$tpl")"
  # shellcheck disable=SC2016
  # Aspas simples intencionais: '${TECH_LEADER_PLAN_RULES_BLOCK}' é o texto literal buscado
  # no fonte do template, não uma variável pra expandir aqui.
  assert_contains "$tpl_src" '${TECH_LEADER_PLAN_RULES_BLOCK}' "$(basename "$tpl") usa \${TECH_LEADER_PLAN_RULES_BLOCK} em vez de repetir o texto"
  assert_not_contains "$tpl_src" "O plano deve ser pensado para ser executado usando a metodologia TDD" "$(basename "$tpl") não duplica o texto da regra de TDD (deve vir só de TECH_LEADER_PLAN_RULES_BLOCK)"
done

echo
echo "== template tech_leader.md: renderização via envsubst =="
if [ -f "$TECH_LEADER_TEMPLATE" ]; then
  # shellcheck disable=SC2016
  # Aspas simples intencionais: é a lista de variáveis pro envsubst expandir, não
  # queremos que o bash expanda antes (mesmo padrão dos outros templates).
  rendered_tl="$(STACK_BLOCK_TL=" especialista em stack de exemplo." SENTRY_BLOCK_TL="[bloco de sentry de exemplo]" TECH_LEADER_PLAN_RULES_BLOCK="[regras de exemplo]" JIRA_ID="SC-555" TITLE="título de exemplo" DESCRIPTION="descrição de exemplo" envsubst '$STACK_BLOCK_TL $SENTRY_BLOCK_TL $TECH_LEADER_PLAN_RULES_BLOCK $JIRA_ID $TITLE $DESCRIPTION' < "$TECH_LEADER_TEMPLATE")"
  assert_contains "$rendered_tl" "[regras de exemplo]" "TECH_LEADER_PLAN_RULES_BLOCK de exemplo aparece literalmente no output"
  assert_contains "$rendered_tl" "SC-555" "JIRA_ID de exemplo aparece literalmente no output"
  # shellcheck disable=SC2016
  # Aspas simples intencionais: '${' é o texto literal buscado no output renderizado.
  assert_not_contains "$rendered_tl" '${' "nenhuma variável \${...} sobra sem substituir no output"
else
  echo "  FAIL: templates/tech_leader.md não existe"
  failures=$((failures + 1))
fi

echo
echo "== template tech_leader_jira.md: renderização via envsubst e ausência de \${JIRA_ID} =="
if [ -f "$TECH_LEADER_JIRA_TEMPLATE" ]; then
  # shellcheck disable=SC2016
  # Aspas simples intencionais: é a lista de variáveis pro envsubst expandir, não
  # queremos que o bash expanda antes (mesmo padrão dos outros templates).
  rendered_tl_jira="$(STACK_BLOCK_TL=" especialista em stack de exemplo." SENTRY_BLOCK_TL="[bloco de sentry de exemplo]" TECH_LEADER_PLAN_RULES_BLOCK="[regras de exemplo]" JIRA_LINK="https://jira.example.com/browse/SC-777" DESCRIPTION="contexto extra de exemplo" envsubst '$STACK_BLOCK_TL $SENTRY_BLOCK_TL $TECH_LEADER_PLAN_RULES_BLOCK $JIRA_LINK $DESCRIPTION' < "$TECH_LEADER_JIRA_TEMPLATE")"
  assert_contains "$rendered_tl_jira" "https://jira.example.com/browse/SC-777" "JIRA_LINK de exemplo aparece literalmente no output"
  assert_contains "$rendered_tl_jira" "contexto extra de exemplo" "DESCRIPTION de exemplo aparece literalmente no output"
  assert_contains "$rendered_tl_jira" "[regras de exemplo]" "TECH_LEADER_PLAN_RULES_BLOCK de exemplo aparece literalmente no output"
  # shellcheck disable=SC2016
  # Aspas simples intencionais: '${' é o texto literal buscado no output renderizado.
  assert_not_contains "$rendered_tl_jira" '${' "nenhuma variável \${...} sobra sem substituir no output"

  template_src="$(cat "$TECH_LEADER_JIRA_TEMPLATE")"
  # shellcheck disable=SC2016
  # Aspas simples intencionais: '${JIRA_ID}' é a string literal buscada no código-fonte
  # do template — JIRA_ID não é passado pelo script pra este TYPE (fica vazio), então
  # usá-la aqui quebraria o nome do arquivo do plano silenciosamente.
  assert_not_contains "$template_src" '${JIRA_ID}' "template não referencia \${JIRA_ID} (ficaria vazio, script não passa JIRA_ID pra este TYPE)"
else
  echo "  FAIL: templates/tech_leader_jira.md não existe"
  failures=$((failures + 1))
fi

echo
echo '== load_project_config: lê só CHAVE="valor" das chaves conhecidas, sem executar nada =='
lpc_dir="$TEST_TMP/load_project_config"
mkdir -p "$lpc_dir"

printf '%s\n' \
  "$(config_keys_blank)" \
  '# Config de exemplo, no formato do esqueleto.' \
  '# Exemplo: STACK_DESCRIPTION="não deve ser lido"' \
  'STACK_DESCRIPTION="Bash (POSIX [ ], set -euo pipefail), pytest e TDD"' \
  '' \
  'SENTRY_ENABLED="true"' \
  'PERMISSION_ENABLED="false"' \
  'JIRA_ENABLED="true"' \
  'PRODUCTION_ENABLED="true"' \
  'TDD_ENABLED="true"' > "$lpc_dir/skeleton.conf"
lpc_result="$(run_loader "$lpc_dir/skeleton.conf" "$lpc_dir/skeleton")"
assert_eq "Bash (POSIX [ ], set -euo pipefail), pytest e TDD|true|false|true|true" "$lpc_result" "formato do esqueleto: as 5 chaves carregam com o valor do arquivo (inclusive [ ], vírgula e parênteses)"
assert_eq "" "$(cat "$lpc_dir/skeleton.err" 2>/dev/null || echo 'sem stderr')" "formato do esqueleto: nenhum aviso no stderr (nem pela linha '# Exemplo: STACK_DESCRIPTION=...')"

printf '%s\n' 'echo executou' "touch \"$lpc_dir/marker1\"" > "$lpc_dir/commands.conf"
run_loader "$lpc_dir/commands.conf" "$lpc_dir/commands" >/dev/null
assert_not_contains "$(cat "$lpc_dir/commands.out" "$lpc_dir/commands.err" 2>/dev/null)" "executou" "linha 'echo executou' não é executada"
assert_not_exists "$lpc_dir/marker1" "linha 'touch <marker>' não é executada"

printf '%s\n' "STACK_DESCRIPTION=\"\$(touch $lpc_dir/marker2)\"" > "$lpc_dir/subst.conf"
lpc_result="$(run_loader "$lpc_dir/subst.conf" "$lpc_dir/subst")"
assert_not_exists "$lpc_dir/marker2" "\$(...) dentro do valor não é executado"
assert_eq "\$(touch $lpc_dir/marker2)||||" "$lpc_result" "\$(...) dentro do valor vira texto literal em STACK_DESCRIPTION"

printf '%s\n' '# linha 1' "JIRA_ENABLED=\"true\" && touch \"$lpc_dir/marker3\"" > "$lpc_dir/hybrid.conf"
lpc_result="$(run_loader "$lpc_dir/hybrid.conf" "$lpc_dir/hybrid")"
assert_not_exists "$lpc_dir/marker3" "linha 'CHAVE=\"x\" && touch <marker>' não executa o comando"
assert_eq "||||" "$lpc_result" "linha 'CHAVE=\"x\" && touch <marker>' não altera JIRA_ENABLED"
assert_contains "$(cat "$lpc_dir/hybrid.err" 2>/dev/null)" "Aviso: $lpc_dir/hybrid.conf:2: linha de JIRA_ENABLED ignorada" "linha híbrida gera aviso no stderr com arquivo e número da linha"

printf '%s\n' 'SENTRY_ENABLED=true' > "$lpc_dir/unquoted.conf"
lpc_result="$(run_loader "$lpc_dir/unquoted.conf" "$lpc_dir/unquoted")"
assert_eq "||||" "$lpc_result" "valor sem aspas (SENTRY_ENABLED=true) é ignorado"
assert_contains "$(cat "$lpc_dir/unquoted.err" 2>/dev/null)" "Aviso: $lpc_dir/unquoted.conf:1: linha de SENTRY_ENABLED ignorada (formato esperado: SENTRY_ENABLED=\"valor\", sem aspas duplas dentro do valor)" "valor sem aspas gera aviso no stderr"

printf '%s\n' "$(config_keys_blank)" 'FOO="bar"' 'TYPE="review"' 'PATH="/nao/existe"' > "$lpc_dir/unknown.conf"
unknown_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; load_project_config '$lpc_dir/unknown.conf' 2>'$lpc_dir/unknown.err'; printf '%s|' \"\$TYPE\"; command -v ls >/dev/null && printf 'ls ok'" 2>/dev/null || true)"
assert_eq "|ls ok" "$unknown_result" "chaves desconhecidas (FOO, TYPE, PATH) não alteram variáveis nem quebram a execução"
assert_eq "" "$(cat "$lpc_dir/unknown.err" 2>/dev/null || echo 'sem stderr')" "chaves desconhecidas não geram aviso"

printf '%s\n' '  PERMISSION_ENABLED="true"' > "$lpc_dir/indented.conf"
lpc_result="$(run_loader "$lpc_dir/indented.conf" "$lpc_dir/indented")"
assert_eq "||true||" "$lpc_result" "linha com espaços no início é aceita"

printf 'JIRA_ENABLED="true"\r' > "$lpc_dir/crlf.conf"
lpc_result="$(run_loader "$lpc_dir/crlf.conf" "$lpc_dir/crlf")"
assert_eq "|||true|" "$lpc_result" "última linha com CRLF e sem \\n final é aceita, sem o \\r no valor"

printf '%s\n' "$(config_keys_blank)" 'STACK_DESCRIPTION_EXTRA="x"' > "$lpc_dir/prefix.conf"
lpc_result="$(run_loader "$lpc_dir/prefix.conf" "$lpc_dir/prefix")"
assert_eq "||||" "$lpc_result" "chave com prefixo parecido (STACK_DESCRIPTION_EXTRA) é ignorada"
assert_eq "" "$(cat "$lpc_dir/prefix.err" 2>/dev/null || echo 'sem stderr')" "chave com prefixo parecido não gera aviso"

config_keys_blank > "$lpc_dir/empty.conf"
lpc_result="$(run_loader "$lpc_dir/empty.conf" "$lpc_dir/empty" "STACK_DESCRIPTION=antes")"
assert_eq "||||" "$lpc_result" "CHAVE=\"\" atribui valor vazio"
assert_eq "" "$(cat "$lpc_dir/empty.err" 2>/dev/null || echo 'sem stderr')" "CHAVE=\"\" não gera aviso"

# Formas "quase certas" de chave conhecida: continuam não aceitas, mas avisam (uma vez só
# por linha), senão a flag ficaria desligada sem explicação.
lpc_near_case=0
for lpc_near_line in 'export JIRA_ENABLED="true"' 'SENTRY_ENABLED = "true"' 'PERMISSION_ENABLED ="true"'; do
  lpc_near_case=$((lpc_near_case + 1))
  lpc_near_key="${lpc_near_line#export }"
  lpc_near_key="${lpc_near_key%%[ =]*}"
  printf '%s\n' '# linha 1' "$lpc_near_line" "$(config_keys_blank "$lpc_near_key")" > "$lpc_dir/near_$lpc_near_case.conf"
  lpc_result="$(run_loader "$lpc_dir/near_$lpc_near_case.conf" "$lpc_dir/near_$lpc_near_case")"
  lpc_near_err="$(cat "$lpc_dir/near_$lpc_near_case.err" 2>/dev/null || true)"
  assert_eq "||||" "$lpc_result" "linha '$lpc_near_line' não altera $lpc_near_key"
  assert_contains "$lpc_near_err" "Aviso: $lpc_dir/near_$lpc_near_case.conf:2: linha de $lpc_near_key ignorada" "linha '$lpc_near_line' gera aviso no stderr com arquivo e número da linha"
  assert_eq "1" "$(grep -c 'Aviso:' "$lpc_dir/near_$lpc_near_case.err" 2>/dev/null || true)" "linha '$lpc_near_line' gera um aviso só"
done

printf '%s\n' "$(config_keys_blank)" 'STACK_DESCRIPTION_EXTRA = "x"' > "$lpc_dir/prefix_spaced.conf"
lpc_result="$(run_loader "$lpc_dir/prefix_spaced.conf" "$lpc_dir/prefix_spaced")"
assert_eq "||||" "$lpc_result" "chave com prefixo parecido e espaço em volta do = (STACK_DESCRIPTION_EXTRA = \"x\") é ignorada"
assert_eq "" "$(cat "$lpc_dir/prefix_spaced.err" 2>/dev/null || echo 'sem stderr')" "chave com prefixo parecido e espaço em volta do = não gera aviso"

echo
echo '== load_project_config: aviso de chave ausente (um bloco só, com as linhas a adicionar) =='
miss_dir="$TEST_TMP/missing_keys"
mkdir -p "$miss_dir"

config_keys_blank PRODUCTION_ENABLED > "$miss_dir/only_production.conf"
miss_result="$(run_loader "$miss_dir/only_production.conf" "$miss_dir/only_production")"
miss_err="$(cat "$miss_dir/only_production.err" 2>/dev/null || true)"
assert_eq "||||" "$miss_result" "só PRODUCTION_ENABLED ausente: as outras chaves carregam e PRODUCTION_ENABLED fica vazio"
assert_eq "1" "$(count_warnings "$miss_dir/only_production.err")" "só PRODUCTION_ENABLED ausente: exatamente um aviso no stderr"
assert_eq "$(missing_warning "$miss_dir/only_production.conf" "$production_note_line" 'PRODUCTION_ENABLED="false"')" "$miss_err" "só PRODUCTION_ENABLED ausente: o aviso tem o texto exato, com o caminho do arquivo, a nota e a linha a adicionar"
assert_not_contains "$miss_err" "Existe impacto em produção?" "só PRODUCTION_ENABLED ausente: o aviso não repete a pergunta literal do prompt"
assert_not_contains "$miss_err" "--init" "só PRODUCTION_ENABLED ausente: o aviso não sugere --init (sobrescreveria o arquivo)"

config_keys_blank TDD_ENABLED > "$miss_dir/only_tdd.conf"
run_loader "$miss_dir/only_tdd.conf" "$miss_dir/only_tdd" >/dev/null
miss_err="$(cat "$miss_dir/only_tdd.err" 2>/dev/null || true)"
assert_eq "1" "$(count_warnings "$miss_dir/only_tdd.err")" "só TDD_ENABLED ausente: exatamente um aviso no stderr"
assert_eq "$(missing_warning "$miss_dir/only_tdd.conf" "$tdd_note_line" 'TDD_ENABLED="true"')" "$miss_err" "só TDD_ENABLED ausente: o aviso sugere TDD_ENABLED=\"true\", precedido da nota da chave"
assert_not_contains "$miss_err" "--init" "só TDD_ENABLED ausente: o aviso não sugere --init (sobrescreveria o arquivo)"
assert_not_contains "$miss_err" "ficam desligadas" "só TDD_ENABLED ausente: o cabeçalho do aviso não diz que as opções ausentes ficam desligadas"

printf '%s\n' 'STACK_DESCRIPTION="stack-secreta-19"' 'JIRA_ENABLED="true"' > "$miss_dir/several.conf"
miss_result="$(run_loader "$miss_dir/several.conf" "$miss_dir/several")"
miss_err="$(cat "$miss_dir/several.err" 2>/dev/null || true)"
assert_eq "stack-secreta-19|||true|" "$miss_result" "várias ausentes: as chaves presentes carregam"
assert_eq "1" "$(count_warnings "$miss_dir/several.err")" "várias ausentes: exatamente um aviso no stderr"
assert_eq "$(missing_warning "$miss_dir/several.conf" 'SENTRY_ENABLED="false"' 'PERMISSION_ENABLED="false"' "$production_note_line" 'PRODUCTION_ENABLED="false"' "$tdd_note_line" 'TDD_ENABLED="true"')" "$miss_err" "várias ausentes: um bloco só, com as linhas na ordem de CONFIG_KEYS"
assert_not_contains "$miss_err" "stack-secreta-19" "várias ausentes: o aviso não mostra o valor de STACK_DESCRIPTION"

: > "$miss_dir/empty_file.conf"
miss_result="$(run_loader "$miss_dir/empty_file.conf" "$miss_dir/empty_file")"
assert_eq "||||" "$miss_result" "arquivo vazio (0 byte): todas as chaves ficam vazias"
assert_eq "1" "$(count_warnings "$miss_dir/empty_file.err")" "arquivo vazio (0 byte): exatamente um aviso no stderr"
assert_eq "$(missing_warning "$miss_dir/empty_file.conf" 'STACK_DESCRIPTION=""' 'SENTRY_ENABLED="false"' 'PERMISSION_ENABLED="false"' 'JIRA_ENABLED="false"' "$production_note_line" 'PRODUCTION_ENABLED="false"' "$tdd_note_line" 'TDD_ENABLED="true"')" "$(cat "$miss_dir/empty_file.err" 2>/dev/null || true)" "arquivo vazio (0 byte): o aviso lista as seis chaves na ordem de CONFIG_KEYS"

# Contam como ausentes: o leitor não vê a chave em nenhum destes. A linha vem depois das
# outras chaves, então o bloco só pode listar a chave do caso.
miss_absent_case=0
for miss_absent in \
  'PRODUCTION_ENABLED|# PRODUCTION_ENABLED="true"' \
  'PRODUCTION_ENABLED|PRODUCTION_ENABLED_OLD="true"' \
  'JIRA_ENABLED|JIRA_ENABLE="true"' \
  'JIRA_ENABLED|# JIRA_ENABLED="true"' \
  'JIRA_ENABLED|JIRA_ENABLED_OLD="true"' \
  'STACK_DESCRIPTION|# STACK_DESCRIPTION="x"'; do
  miss_absent_case=$((miss_absent_case + 1))
  miss_absent_key="${miss_absent%%|*}"
  miss_absent_line="${miss_absent#*|}"
  miss_absent_file="$miss_dir/absent_$miss_absent_case.conf"
  printf '%s\n' "$(config_keys_blank "$miss_absent_key")" "$miss_absent_line" > "$miss_absent_file"
  case "$miss_absent_key" in
    PRODUCTION_ENABLED) miss_expected="$(missing_warning "$miss_absent_file" "$production_note_line" 'PRODUCTION_ENABLED="false"')" ;;
    JIRA_ENABLED) miss_expected="$(missing_warning "$miss_absent_file" 'JIRA_ENABLED="false"')" ;;
    *) miss_expected="$(missing_warning "$miss_absent_file" 'STACK_DESCRIPTION=""')" ;;
  esac
  miss_result="$(run_loader "$miss_absent_file" "$miss_dir/absent_$miss_absent_case")"
  assert_eq "||||" "$miss_result" "linha '$miss_absent_line': $miss_absent_key fica vazia"
  assert_eq "1" "$(count_warnings "$miss_dir/absent_$miss_absent_case.err")" "linha '$miss_absent_line': exatamente um aviso no stderr"
  assert_eq "$miss_expected" "$(cat "$miss_dir/absent_$miss_absent_case.err" 2>/dev/null || true)" "linha '$miss_absent_line': o aviso de chave ausente lista só $miss_absent_key"
done

# Linha fora do formato conta como presente: sai só o aviso de formato, que já aponta
# arquivo e linha — dois avisos pro mesmo problema seriam ruído. JIRA_ENABLED=TRUE também
# garante que linha fora do formato não recebe aviso de valor.
miss_bad_case=0
for miss_bad_line in 'PRODUCTION_ENABLED=true' 'PRODUCTION_ENABLED="true" # comentário' 'export PRODUCTION_ENABLED="true"' 'PRODUCTION_ENABLED = "true"' 'STACK_DESCRIPTION=x' 'JIRA_ENABLED=TRUE'; do
  miss_bad_case=$((miss_bad_case + 1))
  miss_bad_key="${miss_bad_line#export }"
  miss_bad_key="${miss_bad_key%%[ =]*}"
  miss_bad_file="$miss_dir/malformed_$miss_bad_case.conf"
  printf '%s\n' '# linha 1' "$miss_bad_line" "$(config_keys_blank "$miss_bad_key")" > "$miss_bad_file"
  miss_result="$(run_loader "$miss_bad_file" "$miss_dir/malformed_$miss_bad_case")"
  miss_err="$(cat "$miss_dir/malformed_$miss_bad_case.err" 2>/dev/null || true)"
  assert_eq "||||" "$miss_result" "linha '$miss_bad_line': $miss_bad_key fica vazia"
  assert_eq "1" "$(count_warnings "$miss_dir/malformed_$miss_bad_case.err")" "linha '$miss_bad_line': exatamente um aviso no stderr"
  assert_contains "$miss_err" "$miss_bad_file:2: linha de $miss_bad_key ignorada" "linha '$miss_bad_line': o aviso é o de formato, com arquivo e número da linha"
  assert_eq "0" "$(count_missing_warnings "$miss_dir/malformed_$miss_bad_case.err")" "linha '$miss_bad_line': conta como presente, sem aviso de chave ausente"
done

# Com a linha e um valor aceito, nenhum aviso (inclusive "", que não liga a pergunta).
miss_with_case=0
for miss_value in true false ""; do
  miss_with_case=$((miss_with_case + 1))
  printf '%s\n' "$(config_keys_blank PRODUCTION_ENABLED)" "PRODUCTION_ENABLED=\"$miss_value\"" > "$miss_dir/with_$miss_with_case.conf"
  miss_result="$(run_loader "$miss_dir/with_$miss_with_case.conf" "$miss_dir/with_$miss_with_case")"
  assert_eq "||||$miss_value" "$miss_result" "PRODUCTION_ENABLED=\"$miss_value\": o valor é carregado"
  assert_eq "" "$(cat "$miss_dir/with_$miss_with_case.err" 2>/dev/null || echo 'sem stderr')" "PRODUCTION_ENABLED=\"$miss_value\": nenhum aviso no stderr"
done

miss_continue="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; load_project_config '$miss_dir/empty_file.conf' 2>/dev/null; printf fim" 2>/dev/null || true)"
assert_eq "fim" "$miss_continue" "o aviso de chave ausente não interrompe: load_project_config retorna 0"

echo
echo '== load_project_config: aviso de valor inválido em chave liga/desliga (um por linha, o valor continua atribuído) =='
val_dir="$TEST_TMP/invalid_toggle_value"
mkdir -p "$val_dir"

# Nas fixtures, a linha testada vai sempre por último: o número dela é o wc -l do arquivo.
val_case=0
for val_key in SENTRY_ENABLED PERMISSION_ENABLED JIRA_ENABLED PRODUCTION_ENABLED TDD_ENABLED; do
  case "$val_key" in
    PRODUCTION_ENABLED) val_off="$production_note" ;;
    TDD_ENABLED) val_off="$tdd_note" ;;
    *) val_off="$default_off_note" ;;
  esac
  for val_value in TRUE True sim; do
    val_case=$((val_case + 1))
    val_label="$val_key=\"$val_value\""
    val_file="$val_dir/invalid_$val_case.conf"
    printf '%s\n' "$(config_keys_blank "$val_key")" "$val_label" > "$val_file"
    val_line="$(wc -l < "$val_file")"
    run_loader "$val_file" "$val_dir/invalid_$val_case" >/dev/null
    val_err="$(cat "$val_dir/invalid_$val_case.err" 2>/dev/null || true)"
    assert_eq "1" "$(count_warnings "$val_dir/invalid_$val_case.err")" "$val_label: exatamente um aviso no stderr"
    assert_eq "$(value_warning "$val_file:$val_line" "$val_key" "$val_value" "$val_off")" "$val_err" "$val_label: o aviso de valor tem o texto exato (arquivo, linha, chave, valor, valores aceitos e opção desligada)"
    assert_eq "0" "$(count_missing_warnings "$val_dir/invalid_$val_case.err")" "$val_label: a linha conta como presente, sem aviso de chave ausente"
    assert_eq "$val_value" "$(loaded_value "$val_file" "$val_key")" "$val_label: a variável recebe o valor literal (o leitor não mudou)"
    if [ "$val_key" = "PRODUCTION_ENABLED" ]; then
      assert_not_contains "$val_err" "Existe impacto em produção?" "$val_label: o aviso não repete a pergunta literal do prompt"
    fi
  done
done

val_case=0
for val_key in SENTRY_ENABLED PERMISSION_ENABLED JIRA_ENABLED PRODUCTION_ENABLED TDD_ENABLED; do
  for val_value in true false ""; do
    val_case=$((val_case + 1))
    val_label="$val_key=\"$val_value\""
    val_file="$val_dir/accepted_$val_case.conf"
    printf '%s\n' "$(config_keys_blank "$val_key")" "$val_label" > "$val_file"
    run_loader "$val_file" "$val_dir/accepted_$val_case" >/dev/null
    assert_eq "" "$(cat "$val_dir/accepted_$val_case.err" 2>/dev/null || echo 'sem stderr')" "$val_label: valor aceito, nenhum aviso no stderr"
    assert_eq "$val_value" "$(loaded_value "$val_file" "$val_key")" "$val_label: o valor é carregado"
  done
done

# TDD_ENABLED é a única flag ligada por padrão: o aviso de valor diz que a TDD continua
# ligada, e não que a opção está desligada.
val_file="$val_dir/tdd_sim.conf"
printf '%s\n' "$(config_keys_blank TDD_ENABLED)" 'TDD_ENABLED="sim"' > "$val_file"
run_loader "$val_file" "$val_dir/tdd_sim" >/dev/null
assert_not_contains "$(cat "$val_dir/tdd_sim.err" 2>/dev/null || true)" "a opção está desligada" "TDD_ENABLED=\"sim\": o aviso não diz que a opção está desligada"
assert_contains "$(cat "$val_dir/tdd_sim.err" 2>/dev/null || true)" "continua ligada" "TDD_ENABLED=\"sim\": o aviso diz que a TDD continua ligada"

# Guardas do lado perigoso: nada além de uma linha TDD_ENABLED="false" válida desliga a TDD.
# A linha híbrida não é aplicada (só o aviso de formato, e ela conta como presente); a
# comentada não é vista pelo leitor (só o aviso de chave ausente, que sugere "true").
val_file="$val_dir/tdd_hybrid.conf"
printf '%s\n' "$(config_keys_blank TDD_ENABLED)" "TDD_ENABLED=\"false\" && touch \"$val_dir/tdd_marker\"" > "$val_file"
run_loader "$val_file" "$val_dir/tdd_hybrid" >/dev/null
assert_eq "" "$(loaded_value "$val_file" TDD_ENABLED)" "linha 'TDD_ENABLED=\"false\" && touch <marker>': TDD_ENABLED não recebe \"false\""
assert_not_exists "$val_dir/tdd_marker" "linha 'TDD_ENABLED=\"false\" && touch <marker>': o comando não é executado"
assert_eq "1" "$(count_warnings "$val_dir/tdd_hybrid.err")" "linha 'TDD_ENABLED=\"false\" && touch <marker>': exatamente um aviso"
assert_eq "Aviso: $val_file:$(wc -l < "$val_file"): linha de TDD_ENABLED ignorada (formato esperado: TDD_ENABLED=\"valor\", sem aspas duplas dentro do valor)" "$(cat "$val_dir/tdd_hybrid.err" 2>/dev/null || true)" "linha 'TDD_ENABLED=\"false\" && touch <marker>': só o aviso de formato, sem aviso de chave ausente"

val_file="$val_dir/tdd_commented.conf"
printf '%s\n' "$(config_keys_blank TDD_ENABLED)" '# TDD_ENABLED="false"' > "$val_file"
run_loader "$val_file" "$val_dir/tdd_commented" >/dev/null
assert_eq "" "$(loaded_value "$val_file" TDD_ENABLED)" "linha '# TDD_ENABLED=\"false\"': TDD_ENABLED não recebe \"false\""
assert_eq "$(missing_warning "$val_file" "$tdd_note_line" 'TDD_ENABLED="true"')" "$(cat "$val_dir/tdd_commented.err" 2>/dev/null || true)" "linha '# TDD_ENABLED=\"false\"': só o aviso de chave ausente, sugerindo TDD_ENABLED=\"true\" com a nota"

# STACK_DESCRIPTION é texto livre: nenhum valor gera aviso de valor.
val_case=0
for val_value in "" TRUE sim "Django, DRF"; do
  val_case=$((val_case + 1))
  val_label="STACK_DESCRIPTION=\"$val_value\""
  val_file="$val_dir/stack_$val_case.conf"
  printf '%s\n' "$(config_keys_blank STACK_DESCRIPTION)" "$val_label" > "$val_file"
  run_loader "$val_file" "$val_dir/stack_$val_case" >/dev/null
  assert_eq "" "$(cat "$val_dir/stack_$val_case.err" 2>/dev/null || echo 'sem stderr')" "$val_label: STACK_DESCRIPTION não é checada, nenhum aviso no stderr"
done

val_file="$val_dir/two_lines.conf"
printf '%s\n' "$(config_keys_blank)" 'SENTRY_ENABLED="TRUE"' 'PERMISSION_ENABLED="sim"' > "$val_file"
val_line="$(wc -l < "$val_file")"
run_loader "$val_file" "$val_dir/two_lines" >/dev/null
assert_eq "2" "$(count_warnings "$val_dir/two_lines.err")" "duas linhas com valor inválido: dois avisos no stderr"
assert_eq "$(value_warning "$val_file:$((val_line - 1))" SENTRY_ENABLED TRUE "$default_off_note")"$'\n'"$(value_warning "$val_file:$val_line" PERMISSION_ENABLED sim "$default_off_note")" "$(cat "$val_dir/two_lines.err" 2>/dev/null || true)" "duas linhas com valor inválido: um aviso por linha (as duas últimas), na ordem do arquivo"

val_file="$val_dir/invalid_and_missing.conf"
printf '%s\n' 'JIRA_ENABLED="sim"' > "$val_file"
run_loader "$val_file" "$val_dir/invalid_and_missing" >/dev/null
assert_eq "2" "$(count_warnings "$val_dir/invalid_and_missing.err")" "valor inválido e chaves ausentes: dois avisos no stderr"
assert_eq "$(value_warning "$val_file:1" JIRA_ENABLED sim "$default_off_note")"$'\n'"$(missing_warning "$val_file" 'STACK_DESCRIPTION=""' 'SENTRY_ENABLED="false"' 'PERMISSION_ENABLED="false"' "$production_note_line" 'PRODUCTION_ENABLED="false"' "$tdd_note_line" 'TDD_ENABLED="true"')" "$(cat "$val_dir/invalid_and_missing.err" 2>/dev/null || true)" "valor inválido e chaves ausentes: primeiro o de valor (:1:), depois o de chave ausente, que não lista JIRA_ENABLED"

val_file="$val_dir/control_char.conf"
val_raw="a"$'\e'"[31mb"
printf '%s\n' "$(config_keys_blank PERMISSION_ENABLED)" "PERMISSION_ENABLED=\"$val_raw\"" > "$val_file"
run_loader "$val_file" "$val_dir/control_char" >/dev/null
assert_contains "$(cat "$val_dir/control_char.err" 2>/dev/null || true)" 'valor "a?[31mb"' "caractere de controle no valor: sai como ? no aviso"
assert_eq "0" "$(grep -c $'\e' "$val_dir/control_char.err" 2>/dev/null || true)" "caractere de controle no valor: nenhum byte ESC no stderr"
assert_eq "$val_raw" "$(loaded_value "$val_file" PERMISSION_ENABLED)" "caractere de controle no valor: a variável recebe o valor bruto, com o ESC"

# Com LC_ALL=C, [![:print:]] também trocaria os bytes de "não" por "?"; a troca usa
# [[:cntrl:]], que nesse locale só pega os controles ASCII. O LC_ALL=C vai no setup porque o
# bash reaplica o locale quando LC_ALL muda.
val_file="$val_dir/non_ascii.conf"
printf '%s\n' "$(config_keys_blank JIRA_ENABLED)" 'JIRA_ENABLED="não"' > "$val_file"
run_loader "$val_file" "$val_dir/non_ascii" "LC_ALL=C" >/dev/null
assert_eq "$(value_warning "$val_file:$(wc -l < "$val_file")" JIRA_ENABLED "não" "$default_off_note")" "$(cat "$val_dir/non_ascii.err" 2>/dev/null || true)" "valor com acento sob LC_ALL=C: sai intacto no aviso (a troca usa [[:cntrl:]], não [![:print:]])"
assert_not_contains "$(cat "$val_dir"/*.err 2>/dev/null || true)" "--init" "nenhum aviso desta seção sugere --init"

echo
echo "== resolve_stack: projects/ usa extensão .conf e é lido por load_project_config (sem source) =="
rs_dir="$TEST_TMP/resolve_stack_conf"
rs_cwd="$rs_dir/cwd"
rs_projects="$rs_dir/projects"
mkdir -p "$rs_cwd" "$rs_projects"
printf '%s\n' '# alpha' 'STACK_DESCRIPTION="stack-alpha"' 'SENTRY_ENABLED="true"' 'PERMISSION_ENABLED="false"' 'JIRA_ENABLED="true"' > "$rs_projects/alpha.conf"
printf '%s\n' '# beta' 'STACK_DESCRIPTION="stack-beta"' 'SENTRY_ENABLED="false"' 'PERMISSION_ENABLED="true"' 'JIRA_ENABLED="false"' > "$rs_projects/beta.conf"

rs_result="$(run_resolve_stack "$rs_cwd" "$rs_projects" "alpha" "" "$rs_dir/by_arg")"
assert_eq "stack-alpha|true|false|true|" "$rs_result" "--projeto=alpha carrega os valores de alpha.conf"

rs_result="$(run_resolve_stack "$rs_cwd" "$rs_projects" "" "1" "$rs_dir/menu")"
assert_eq "stack-alpha|true|false|true|" "$rs_result" "menu: opção 1 carrega alpha.conf (glob em ordem alfabética)"
assert_contains "$(cat "$rs_dir/menu.err")" "1) alpha" "menu lista alpha (nome vindo de basename .conf)"
assert_contains "$(cat "$rs_dir/menu.err")" "2) beta" "menu lista beta (nome vindo de basename .conf)"

rs_result="$(run_resolve_stack "$rs_cwd" "$rs_projects" "" "3" "$rs_dir/none")"
assert_eq "||||" "$rs_result" "menu: opção Nenhum (3, antes do Sair) deixa STACK_DESCRIPTION vazio"

rs_result="$(run_resolve_stack "$rs_cwd" "$rs_projects" "inexistente" "" "$rs_dir/missing")"
assert_eq "1" "$(cat "$rs_dir/missing.rc")" "--projeto=inexistente sai com código 1"
assert_contains "$(cat "$rs_dir/missing.err")" "projeto 'inexistente' não encontrado" "--projeto=inexistente mostra erro de projeto não encontrado"

for rs_case in by_arg menu none missing; do
  assert_not_contains "$(cat "$rs_dir/$rs_case.out" "$rs_dir/$rs_case.err")" "não é mais lido" "caso '$rs_case' sem .sh legado não imprime aviso de legado"
done

rs_touch_projects="$rs_dir/projects_touch"
mkdir -p "$rs_touch_projects"
printf '%s\n' 'STACK_DESCRIPTION="stack-alpha"' "touch \"$rs_dir/marker_conf\"" > "$rs_touch_projects/alpha.conf"
run_resolve_stack "$rs_cwd" "$rs_touch_projects" "alpha" "" "$rs_dir/touch" >/dev/null
assert_not_exists "$rs_dir/marker_conf" "linha 'touch <marker>' em alpha.conf não é executada"

echo
echo "== resolve_stack: projects/*.sh legado não é lido, só gera aviso com o mv sugerido =="
legacy_dir="$TEST_TMP/resolve_stack_legacy"
legacy_cwd="$legacy_dir/cwd"
legacy_projects="$legacy_dir/projects"
mkdir -p "$legacy_cwd" "$legacy_projects"
printf '%s\n' 'STACK_DESCRIPTION="stack-alpha"' > "$legacy_projects/alpha.conf"
printf '%s\n' 'STACK_DESCRIPTION="stack-gamma"' "touch \"$legacy_dir/marker_sh\"" > "$legacy_projects/gamma.sh"

legacy_result="$(run_resolve_stack "$legacy_cwd" "$legacy_projects" "" "1" "$legacy_dir/menu")"
assert_eq "stack-alpha||||" "$legacy_result" "menu com gamma.sh legado: opção 1 continua sendo alpha"
assert_not_contains "$(cat "$legacy_dir/menu.out" "$legacy_dir/menu.err")" ") gamma" "menu não lista gamma (arquivo .sh)"
assert_contains "$(cat "$legacy_dir/menu.err")" "gamma.sh não é mais lido" "menu: stderr avisa que gamma.sh não é mais lido"
assert_contains "$(cat "$legacy_dir/menu.err")" "mv '$legacy_projects/gamma.sh' '$legacy_projects/gamma.conf'" "menu: aviso sugere o mv para .conf"

legacy_result="$(run_resolve_stack "$legacy_cwd" "$legacy_projects" "gamma" "" "$legacy_dir/by_arg")"
legacy_err="$(cat "$legacy_dir/by_arg.err")"
assert_eq "1" "$(cat "$legacy_dir/by_arg.rc")" "--projeto=gamma (só existe gamma.sh) sai com código 1"
assert_contains "$legacy_err" "gamma.sh não é mais lido" "--projeto=gamma: stderr avisa que gamma.sh não é mais lido"
assert_contains "$legacy_err" "projeto 'gamma' não encontrado" "--projeto=gamma: stderr mostra erro de projeto não encontrado"
legacy_before_warning="${legacy_err%%não é mais lido*}"
legacy_before_error="${legacy_err%%não encontrado*}"
assert_eq "true" "$([ "${#legacy_before_warning}" -lt "${#legacy_before_error}" ] && echo true || echo false)" "--projeto=gamma: aviso de legado aparece antes do erro de projeto não encontrado"
assert_not_contains "$legacy_err" "- gamma" "--projeto=gamma: lista de projetos disponíveis não inclui gamma"
assert_not_exists "$legacy_dir/marker_sh" "linha 'touch <marker>' em gamma.sh não é executada"

echo
echo "== resolve_stack: .nick.conf no cwd tem precedência sobre --projeto= e projects/ =="
cwd_dir="$TEST_TMP/resolve_stack_cwd"
cwd_with="$cwd_dir/with_config"
cwd_without="$cwd_dir/without_config"
cwd_projects="$cwd_dir/projects"
mkdir -p "$cwd_with" "$cwd_without" "$cwd_projects"
printf '%s\n' '# cwd' 'STACK_DESCRIPTION="stack-do-cwd"' 'JIRA_ENABLED="true"' > "$cwd_with/.nick.conf"
printf '%s\n' 'STACK_DESCRIPTION="stack-alpha"' 'SENTRY_ENABLED="true"' > "$cwd_projects/alpha.conf"
printf '%s\n' 'STACK_DESCRIPTION="stack-gamma"' > "$cwd_projects/gamma.sh"

cwd_result="$(run_resolve_stack "$cwd_with" "$cwd_projects" "" "" "$cwd_dir/load")"
cwd_output="$(cat "$cwd_dir/load.out" "$cwd_dir/load.err")"
assert_eq "stack-do-cwd|||true|" "$cwd_result" "com .nick.conf no cwd, as variáveis vêm dele"
assert_contains "$(cat "$cwd_dir/load.out")" "Configuração do projeto carregada de: $cwd_with/.nick.conf" "com .nick.conf no cwd, stdout mostra de onde a configuração foi carregada"
assert_not_contains "$cwd_output" "Qual projeto" "com .nick.conf no cwd, o menu de projetos não aparece"
assert_not_contains "$cwd_output" "alpha" "com .nick.conf no cwd, os projetos de projects/ não são listados"
assert_not_contains "$cwd_output" "não é mais lido" "com .nick.conf no cwd, não aparece aviso de .sh legado"

cwd_result="$(run_resolve_stack "$cwd_with" "$cwd_projects" "alpha" "" "$cwd_dir/arg_existing")"
assert_eq "stack-do-cwd|||true|" "$cwd_result" "com .nick.conf no cwd, --projeto=alpha é ignorado e valem os valores do cwd"
assert_contains "$(cat "$cwd_dir/arg_existing.err")" "Aviso: --projeto=alpha ignorado — a configuração do diretório atual ($cwd_with/.nick.conf) tem precedência." "com .nick.conf no cwd, stderr avisa que --projeto=alpha foi ignorado"

cwd_result="$(run_resolve_stack "$cwd_with" "$cwd_projects" "inexistente" "" "$cwd_dir/arg_missing")"
assert_eq "0" "$(cat "$cwd_dir/arg_missing.rc")" "com .nick.conf no cwd, --projeto=inexistente não dá exit 1"
assert_eq "stack-do-cwd|||true|" "$cwd_result" "com .nick.conf no cwd, --projeto=inexistente usa os valores do cwd"
assert_not_contains "$(cat "$cwd_dir/arg_missing.out" "$cwd_dir/arg_missing.err")" "não encontrado" "com .nick.conf no cwd, --projeto=inexistente não mostra erro de projeto não encontrado"
assert_contains "$(cat "$cwd_dir/arg_missing.err")" "Aviso: --projeto=inexistente ignorado" "com .nick.conf no cwd, stderr avisa que --projeto=inexistente foi ignorado"

cwd_result="$(run_resolve_stack "$cwd_without" "$cwd_projects" "" "1" "$cwd_dir/fallback_menu")"
cwd_output="$(cat "$cwd_dir/fallback_menu.out" "$cwd_dir/fallback_menu.err")"
assert_eq "stack-alpha|true|||" "$cwd_result" "sem .nick.conf no cwd, menu opção 1 carrega alpha.conf"
assert_contains "$cwd_output" "Qual projeto" "sem .nick.conf no cwd, o menu de projetos aparece"
assert_not_contains "$cwd_output" "Configuração do projeto carregada de" "sem .nick.conf no cwd, não aparece a linha 'carregada de'"

cwd_result="$(run_resolve_stack "$cwd_without" "$cwd_projects" "alpha" "" "$cwd_dir/fallback_arg")"
cwd_output="$(cat "$cwd_dir/fallback_arg.out" "$cwd_dir/fallback_arg.err")"
assert_eq "stack-alpha|true|||" "$cwd_result" "sem .nick.conf no cwd, --projeto=alpha carrega alpha.conf"
assert_not_contains "$cwd_output" "ignorado" "sem .nick.conf no cwd, --projeto=alpha não gera aviso de flag ignorada"
assert_not_contains "$cwd_output" "Configuração do projeto carregada de" "sem .nick.conf no cwd, --projeto=alpha não imprime a linha 'carregada de'"

echo
echo "== resolve_stack: aviso de chave ausente só pro arquivo carregado (cwd, --projeto=, menu; nunca em 'Nenhum') =="
rs_miss_dir="$TEST_TMP/resolve_stack_missing_keys"
rs_miss_cwd_incomplete="$rs_miss_dir/cwd_incompleto"
rs_miss_cwd_complete="$rs_miss_dir/cwd_completo"
rs_miss_cwd_none="$rs_miss_dir/cwd_sem_config"
rs_miss_projects="$rs_miss_dir/projects"
mkdir -p "$rs_miss_cwd_incomplete" "$rs_miss_cwd_complete" "$rs_miss_cwd_none" "$rs_miss_projects"
printf '%s\n' 'STACK_DESCRIPTION="stack-do-cwd"' > "$rs_miss_cwd_incomplete/.nick.conf"
printf '%s\n' "$(config_keys_blank PRODUCTION_ENABLED)" 'STACK_DESCRIPTION="stack-do-cwd"' 'PRODUCTION_ENABLED="false"' > "$rs_miss_cwd_complete/.nick.conf"
printf '%s\n' "$(config_keys_blank PRODUCTION_ENABLED)" 'STACK_DESCRIPTION="stack-com-linha"' 'PRODUCTION_ENABLED="true"' > "$rs_miss_projects/comlinha.conf"
printf '%s\n' 'STACK_DESCRIPTION="stack-sem-linha"' > "$rs_miss_projects/semlinha.conf"

# rs_miss_expected <file>: o aviso exato pra um arquivo que só tem STACK_DESCRIPTION.
rs_miss_expected() {
  local file="$1"
  missing_warning "$file" 'SENTRY_ENABLED="false"' 'PERMISSION_ENABLED="false"' 'JIRA_ENABLED="false"' "$production_note_line" 'PRODUCTION_ENABLED="false"' "$tdd_note_line" 'TDD_ENABLED="true"'
}

rs_miss_result="$(run_resolve_stack "$rs_miss_cwd_incomplete" "$rs_miss_projects" "" "" "$rs_miss_dir/cwd_incomplete")"
rs_miss_err="$(cat "$rs_miss_dir/cwd_incomplete.err" 2>/dev/null || true)"
assert_eq "stack-do-cwd||||" "$rs_miss_result" "cwd incompleto: a configuração do cwd é carregada"
assert_eq "1" "$(count_missing_warnings "$rs_miss_dir/cwd_incomplete.err")" "cwd incompleto: exatamente um aviso de chave ausente"
assert_contains "$rs_miss_err" "Aviso: $rs_miss_cwd_incomplete/.nick.conf não tem todas as chaves" "cwd incompleto: o aviso mostra o caminho do .nick.conf do cwd"
assert_contains "$rs_miss_err" "$(rs_miss_expected "$rs_miss_cwd_incomplete/.nick.conf")" "cwd incompleto: o aviso tem o texto exato, com as linhas a adicionar"

rs_miss_result="$(run_resolve_stack "$rs_miss_cwd_complete" "$rs_miss_projects" "" "" "$rs_miss_dir/cwd_complete")"
assert_eq "stack-do-cwd||||false" "$rs_miss_result" "cwd completo: a configuração do cwd é carregada"
assert_eq "0" "$(count_warnings "$rs_miss_dir/cwd_complete.err")" "cwd completo: nenhum aviso"

rs_miss_result="$(run_resolve_stack "$rs_miss_cwd_none" "$rs_miss_projects" "semlinha" "" "$rs_miss_dir/arg_incomplete")"
assert_eq "stack-sem-linha||||" "$rs_miss_result" "--projeto=semlinha: semlinha.conf é carregado"
assert_eq "1" "$(count_warnings "$rs_miss_dir/arg_incomplete.err")" "--projeto=semlinha: exatamente um aviso"
assert_eq "1" "$(count_missing_warnings "$rs_miss_dir/arg_incomplete.err")" "--projeto=semlinha: o aviso é o de chave ausente"
assert_contains "$(cat "$rs_miss_dir/arg_incomplete.err" 2>/dev/null || true)" "$(rs_miss_expected "$rs_miss_projects/semlinha.conf")" "--projeto=semlinha: o aviso mostra o caminho de projects/semlinha.conf"

# Menu: 1) comlinha, 2) semlinha, 3) Nenhum, 4) Sair (q) (glob em ordem alfabética).
rs_miss_result="$(run_resolve_stack "$rs_miss_cwd_none" "$rs_miss_projects" "" "2" "$rs_miss_dir/menu_incomplete")"
assert_eq "stack-sem-linha||||" "$rs_miss_result" "menu escolhendo semlinha: semlinha.conf é carregado"
assert_eq "1" "$(count_warnings "$rs_miss_dir/menu_incomplete.err")" "menu escolhendo semlinha: exatamente um aviso"
assert_eq "1" "$(count_missing_warnings "$rs_miss_dir/menu_incomplete.err")" "menu escolhendo semlinha: o aviso é o de chave ausente"
assert_contains "$(cat "$rs_miss_dir/menu_incomplete.err" 2>/dev/null || true)" "$(rs_miss_expected "$rs_miss_projects/semlinha.conf")" "menu escolhendo semlinha: o aviso mostra o caminho de projects/semlinha.conf"

rs_miss_result="$(run_resolve_stack "$rs_miss_cwd_none" "$rs_miss_projects" "" "1" "$rs_miss_dir/menu_complete")"
assert_eq "stack-com-linha||||true" "$rs_miss_result" "menu escolhendo comlinha: comlinha.conf é carregado"
assert_eq "0" "$(count_warnings "$rs_miss_dir/menu_complete.err")" "menu escolhendo comlinha (completo): nenhum aviso"

rs_miss_result="$(run_resolve_stack "$rs_miss_cwd_none" "$rs_miss_projects" "" "3" "$rs_miss_dir/menu_none")"
assert_eq "||||" "$rs_miss_result" "menu escolhendo Nenhum: nenhuma configuração é carregada"
assert_eq "0" "$(count_warnings "$rs_miss_dir/menu_none.err")" "menu escolhendo Nenhum (com semlinha.conf em projects/): nenhum aviso"

echo
echo "== resolve_stack: aviso de valor inválido nos três caminhos (cwd, --projeto=, menu; nunca em 'Nenhum') =="
# PROJECTS_DIR próprio, pra não mudar a numeração do menu da seção acima.
rs_val_dir="$TEST_TMP/resolve_stack_invalid_value"
rs_val_cwd="$rs_val_dir/cwd_invalido"
rs_val_cwd_none="$rs_val_dir/cwd_sem_config"
rs_val_projects="$rs_val_dir/projects"
mkdir -p "$rs_val_cwd" "$rs_val_cwd_none" "$rs_val_projects"
rs_val_conf="$(config_keys_blank SENTRY_ENABLED)"$'\n''SENTRY_ENABLED="sim"'
printf '%s\n' "$rs_val_conf" > "$rs_val_cwd/.nick.conf"
printf '%s\n' "$rs_val_conf" > "$rs_val_projects/invalido.conf"
rs_val_line="$(wc -l < "$rs_val_projects/invalido.conf")"

rs_val_result="$(run_resolve_stack "$rs_val_cwd" "$rs_val_projects" "" "" "$rs_val_dir/cwd")"
assert_eq "|sim|||" "$rs_val_result" "cwd com SENTRY_ENABLED=\"sim\": o valor literal é carregado"
assert_eq "1" "$(count_warnings "$rs_val_dir/cwd.err")" "cwd com SENTRY_ENABLED=\"sim\": exatamente um aviso"
assert_contains "$(cat "$rs_val_dir/cwd.err" 2>/dev/null || true)" "$(value_warning "$rs_val_cwd/.nick.conf:$rs_val_line" SENTRY_ENABLED sim "$default_off_note")" "cwd com SENTRY_ENABLED=\"sim\": o aviso de valor aponta o .nick.conf do cwd, última linha"

rs_val_result="$(run_resolve_stack "$rs_val_cwd_none" "$rs_val_projects" "invalido" "" "$rs_val_dir/arg")"
assert_eq "|sim|||" "$rs_val_result" "--projeto=invalido: o valor literal é carregado"
assert_eq "1" "$(count_warnings "$rs_val_dir/arg.err")" "--projeto=invalido: exatamente um aviso"
assert_contains "$(cat "$rs_val_dir/arg.err" 2>/dev/null || true)" "$(value_warning "$rs_val_projects/invalido.conf:$rs_val_line" SENTRY_ENABLED sim "$default_off_note")" "--projeto=invalido: o aviso de valor aponta projects/invalido.conf, última linha"

# Menu: 1) invalido, 2) Nenhum, 3) Sair (q).
rs_val_result="$(run_resolve_stack "$rs_val_cwd_none" "$rs_val_projects" "" "1" "$rs_val_dir/menu")"
assert_eq "|sim|||" "$rs_val_result" "menu escolhendo invalido: o valor literal é carregado"
assert_eq "1" "$(count_warnings "$rs_val_dir/menu.err")" "menu escolhendo invalido: exatamente um aviso"
assert_contains "$(cat "$rs_val_dir/menu.err" 2>/dev/null || true)" "$(value_warning "$rs_val_projects/invalido.conf:$rs_val_line" SENTRY_ENABLED sim "$default_off_note")" "menu escolhendo invalido: o aviso de valor aponta projects/invalido.conf, última linha"

rs_val_result="$(run_resolve_stack "$rs_val_cwd_none" "$rs_val_projects" "" "2" "$rs_val_dir/menu_none")"
assert_eq "||||" "$rs_val_result" "menu escolhendo Nenhum: nenhuma configuração é carregada"
assert_eq "0" "$(count_warnings "$rs_val_dir/menu_none.err")" "menu escolhendo Nenhum (com invalido.conf em projects/): nenhum aviso"

echo
echo "== main(): com .nick.conf no cwd, 'carregada de' vem antes do menu de tipo e o JIRA_ENABLED do cwd chega a choose_type =="
e2e_dir="$TEST_TMP/main_e2e"
e2e_cwd="$e2e_dir/cwd"
mkdir -p "$e2e_cwd" "$e2e_dir/projects"
printf '%s\n' 'JIRA_ENABLED="true"' > "$e2e_cwd/.nick.conf"
e2e_rc=0
# stdin: opção 2 (po_discussion), o assunto e N pra recusar o envio pro claude. LOGS_DIR e
# PROJECTS_DIR temporários pra não gravar em auto_scrum/logs/ nem ler auto_scrum/projects/.
e2e_output="$(timeout 5 bash -c "cd '$e2e_cwd' || exit 99; source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$e2e_dir/logs'; PROJECTS_DIR='$e2e_dir/projects'; main --projeto=inexistente" <<< $'2\nassunto de teste\nN' 2>&1)" || e2e_rc=$?
assert_eq "0" "$e2e_rc" "main() termina com código 0 ao recusar o envio"
assert_contains "$e2e_output" "Cancelado." "main() cancela ao receber N na confirmação"
assert_contains "$e2e_output" "Configuração do projeto carregada de: $e2e_cwd/.nick.conf" "main() imprime de onde a configuração foi carregada"
assert_contains "$e2e_output" "Qual tipo de conversa você quer iniciar?" "main() mostra o menu de tipo de conversa"
e2e_before_loaded="${e2e_output%%Configuração do projeto carregada de:*}"
e2e_before_type_menu="${e2e_output%%Qual tipo de conversa você quer iniciar?*}"
assert_eq "true" "$([ "${#e2e_before_loaded}" -lt "${#e2e_before_type_menu}" ] && echo true || echo false)" "linha 'carregada de' aparece antes do menu de tipo de conversa"
assert_contains "$e2e_output" "PO - Criar ticket a partir do Jira" "JIRA_ENABLED=\"true\" do .nick.conf chega a choose_type()"
assert_not_contains "$e2e_output" "Qual projeto" "main() não mostra o menu de projetos"
assert_contains "$e2e_output" "Aviso: --projeto=inexistente ignorado" "main() avisa que --projeto=inexistente foi ignorado"

echo
echo "== choose_type: po_sentry só aparece com SENTRY_ENABLED=true, depois dos outros pontos de entrada do PO =="
# run_choose_type <setup> <option>: dá source num bash separado, roda <setup> (ajuste das
# flags), alimenta choose_type com <option> e imprime o TYPE escolhido.
run_choose_type() {
  local setup="$1" option="$2"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $setup; choose_type <<< '$option' >/dev/null 2>&1; printf '%s' \"\$TYPE\"" 2>/dev/null || true
}

assert_eq "po_sentry" "$(run_choose_type "SENTRY_ENABLED=true" 3)" "com SENTRY_ENABLED=true (Jira desligado), opção 3 seleciona TYPE=po_sentry"
assert_eq "tech_leader" "$(run_choose_type "SENTRY_ENABLED=true" 4)" "com SENTRY_ENABLED=true (Jira desligado), opção 4 é tech_leader"
assert_eq "review" "$(run_choose_type "SENTRY_ENABLED=true" 6)" "com SENTRY_ENABLED=true (Jira desligado), opção 6 é review"

assert_eq "po_jira" "$(run_choose_type "SENTRY_ENABLED=true; JIRA_ENABLED=true" 3)" "com SENTRY_ENABLED=true e JIRA_ENABLED=true, opção 3 é po_jira"
assert_eq "po_sentry" "$(run_choose_type "SENTRY_ENABLED=true; JIRA_ENABLED=true" 4)" "com SENTRY_ENABLED=true e JIRA_ENABLED=true, opção 4 é po_sentry"
assert_eq "tech_leader" "$(run_choose_type "SENTRY_ENABLED=true; JIRA_ENABLED=true" 5)" "com SENTRY_ENABLED=true e JIRA_ENABLED=true, opção 5 é tech_leader"
assert_eq "tech_leader_jira" "$(run_choose_type "SENTRY_ENABLED=true; JIRA_ENABLED=true" 6)" "com SENTRY_ENABLED=true e JIRA_ENABLED=true, opção 6 é tech_leader_jira"
assert_eq "review" "$(run_choose_type "SENTRY_ENABLED=true; JIRA_ENABLED=true" 8)" "com SENTRY_ENABLED=true e JIRA_ENABLED=true, opção 8 é review"

assert_eq "tech_leader" "$(run_choose_type ":" 3)" "sem SENTRY_ENABLED (padrão vazio), po_sentry some do menu — opção 3 é tech_leader"
assert_eq "tech_leader" "$(run_choose_type "SENTRY_ENABLED=false" 3)" "com SENTRY_ENABLED=false explícito, po_sentry também some do menu"

# O select imprime o menu no stderr mesmo sem terminal, então dá pra checar a label.
sentry_menu_label="PO - Criar ticket a partir do Sentry"
sentry_menu="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; SENTRY_ENABLED=true; choose_type <<< '1' 2>&1" 2>/dev/null || true)"
assert_contains "$sentry_menu" "$sentry_menu_label" "com SENTRY_ENABLED=true, o menu mostra a label do po_sentry"
sentry_menu="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; choose_type <<< '1' 2>&1" 2>/dev/null || true)"
assert_not_contains "$sentry_menu" "$sentry_menu_label" "sem SENTRY_ENABLED (padrão vazio), o menu não mostra a label do po_sentry"
sentry_menu="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; SENTRY_ENABLED=false; choose_type <<< '1' 2>&1" 2>/dev/null || true)"
assert_not_contains "$sentry_menu" "$sentry_menu_label" "com SENTRY_ENABLED=false, o menu não mostra a label do po_sentry"

echo
echo "== ask_questions_po_sentry: pede link (obrigatório) e descrição extra (opcional), sem título =="
# O read -p não imprime o prompt fora de terminal, então o teste é pelo efeito nas variáveis:
# se houvesse uma pergunta de título, o link cairia em TITLE.
sentry_link="https://sentry.example.com/organizations/acme/issues/42/"
run_ask_po_sentry() {
  local input="$1"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; ask_questions_po_sentry >/dev/null 2>&1; printf '%s|%s|%s' \"\$SENTRY_LINK\" \"\$DESCRIPTION\" \"\$TITLE\"" <<< "$input" 2>/dev/null || true
}
assert_eq "$sentry_link||" "$(run_ask_po_sentry "$sentry_link"$'\n')" "link setado em SENTRY_LINK, DESCRIPTION e TITLE vazios quando a descrição não é informada"
assert_eq "$sentry_link||" "$(run_ask_po_sentry $'\n'"$sentry_link"$'\n')" "read_required repete quando a primeira linha vem vazia, só aceita a segunda"
assert_eq "$sentry_link|descrição extra de exemplo|" "$(run_ask_po_sentry "$sentry_link"$'\ndescrição extra de exemplo')" "link e descrição setados, TITLE vazio (não há pergunta de título)"

# template_vars <file>: imprime, numa linha só e separadas por espaço, as variáveis
# referenciadas no fonte de um template ($VAR ou ${VAR}), ordenadas e sem repetição.
template_vars() {
  local file="$1"
  # shellcheck disable=SC2016
  # Aspas simples intencionais: '${}' é a lista de caracteres pro tr apagar, não expansão.
  { grep -oE '\$\{?[A-Za-z_][A-Za-z0-9_]*\}?' "$file" 2>/dev/null || true; } | tr -d '${}' | LC_ALL=C sort -u | paste -sd ' ' -
}

echo
echo "== template po_sentry.md: renderização via envsubst e conjunto exato de variáveis =="
PO_SENTRY_TEMPLATE="$SCRIPT_DIR/../templates/po_sentry.md"
if [ -f "$PO_SENTRY_TEMPLATE" ]; then
  # shellcheck disable=SC2016
  # Aspas simples intencionais: é a lista de variáveis pro envsubst expandir, não
  # queremos que o bash expanda antes (mesmo padrão dos outros templates).
  rendered_po_sentry="$(PO_VALIDATION_BLOCK="[validação de exemplo]" PO_TICKET_FORMAT_BLOCK="[formato de exemplo]" SENTRY_LINK="$sentry_link" DESCRIPTION="descrição extra de exemplo" envsubst '$PO_VALIDATION_BLOCK $PO_TICKET_FORMAT_BLOCK $SENTRY_LINK $DESCRIPTION' < "$PO_SENTRY_TEMPLATE")"
  assert_contains "$rendered_po_sentry" "$sentry_link" "SENTRY_LINK de exemplo aparece literalmente no output"
  assert_contains "$rendered_po_sentry" "descrição extra de exemplo" "DESCRIPTION de exemplo aparece literalmente no output"
  assert_contains "$rendered_po_sentry" "[validação de exemplo]" "PO_VALIDATION_BLOCK de exemplo aparece literalmente no output"
  assert_contains "$rendered_po_sentry" "[formato de exemplo]" "PO_TICKET_FORMAT_BLOCK de exemplo aparece literalmente no output"
  # shellcheck disable=SC2016
  # Aspas simples intencionais: '${' é o texto literal buscado no output renderizado.
  assert_not_contains "$rendered_po_sentry" '${' "nenhuma variável \${...} sobra sem substituir no output"

  # Uma variável que ask_questions_po_sentry não preenche (ex: TITLE) passaria no teste de
  # consistência genérico, mas seria substituída silenciosamente por vazio neste TYPE.
  assert_eq "DESCRIPTION PO_TICKET_FORMAT_BLOCK PO_VALIDATION_BLOCK SENTRY_LINK" "$(template_vars "$PO_SENTRY_TEMPLATE")" "po_sentry.md referencia exatamente as variáveis preenchidas pra esse TYPE"

  po_sentry_src="$(cat "$PO_SENTRY_TEMPLATE")"
  assert_not_contains "$po_sentry_src" "Considere o pedido válido quando" "po_sentry.md não duplica os critérios de validade (vêm de PO_VALIDATION_BLOCK)"
  assert_not_contains "$po_sentry_src" "## Critérios de aceite" "po_sentry.md não duplica o formato do ticket (vem de PO_TICKET_FORMAT_BLOCK)"
else
  echo "  FAIL: templates/po_sentry.md não existe"
  failures=$((failures + 1))
fi

# run_main <case_dir> <nick_conf_line> <stdin> [setup]: roda main() de ponta a ponta num
# bash separado, com cwd em <case_dir>/cwd contendo um .nick.conf de uma linha,
# check_requirements stubado e LOGS_DIR/PROJECTS_DIR temporários (nada é gravado em
# auto_scrum/logs/ nem lido de auto_scrum/projects/). [setup] roda antes de main (ex:
# apontar EDITOR). stdout+stderr vão pra <case_dir>/output; imprime o código de saída.
run_main() {
  local case_dir="$1" conf_line="$2" input="$3" setup="${4:-:}" rc=0
  mkdir -p "$case_dir/cwd" "$case_dir/projects"
  printf '%s\n' "$conf_line" > "$case_dir/cwd/.nick.conf"
  timeout 5 bash -c "cd '$case_dir/cwd' || exit 99; source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$case_dir/logs'; PROJECTS_DIR='$case_dir/projects'; $setup; main" <<< "$input" > "$case_dir/output" 2>&1 || rc=$?
  printf '%s' "$rc"
}

echo
echo "== main(): po_sentry de ponta a ponta (menu, case, export, envsubst e prompt renderizado) =="
e2e_sentry_dir="$TEST_TMP/main_e2e_po_sentry"
# stdin: opção 3 (po_sentry, com Jira desligado), o link, descrição vazia e N pra recusar.
e2e_sentry_rc="$(run_main "$e2e_sentry_dir" 'SENTRY_ENABLED="true"' "3"$'\n'"$sentry_link"$'\n\nN')"
e2e_sentry_output="$(cat "$e2e_sentry_dir/output")"
assert_eq "0" "$e2e_sentry_rc" "main() termina com código 0 ao recusar o envio"
assert_contains "$e2e_sentry_output" "Cancelado." "main() cancela ao receber N na confirmação"
assert_contains "$e2e_sentry_output" "$sentry_menu_label" "SENTRY_ENABLED=\"true\" do .nick.conf mostra a opção po_sentry no menu"
assert_contains "$e2e_sentry_output" "Link do issue no Sentry: $sentry_link" "prompt renderizado contém o link informado"
assert_contains "$e2e_sentry_output" "## Critérios de aceite" "prompt renderizado contém o formato do ticket (PO_TICKET_FORMAT_BLOCK)"
assert_contains "$e2e_sentry_output" "link de erro externo" "prompt renderizado contém a referência a link de erro externo (SENTRY_BLOCK_PO)"
# shellcheck disable=SC2016
# Aspas simples intencionais: '${' é o texto literal buscado no output renderizado.
assert_not_contains "$e2e_sentry_output" '${' "nenhuma variável \${...} sobra sem substituir no prompt"

echo
echo "== ask_questions_po: só título e descrição pelo editor, com ou sem SENTRY_ENABLED =="
# Editor falso: read_via_editor chama "$EDITOR" "$tmpfile", então EDITOR precisa ser o
# caminho de um executável sem argumentos. Grava um texto fixo no arquivo e toca um
# marcador, que prova que o editor foi de fato chamado.
fake_editor_dir="$TEST_TMP/fake_editor"
fake_editor="$fake_editor_dir/editor"
fake_editor_marker="$fake_editor_dir/called"
fake_editor_text="descrição vinda do editor falso"
mkdir -p "$fake_editor_dir"
printf '%s\n' '#!/usr/bin/env bash' "printf '%s\n' '$fake_editor_text' > \"\$1\"" "touch '$fake_editor_marker'" > "$fake_editor"
chmod +x "$fake_editor"

# A 2ª linha do stdin é um link: no fluxo antigo ela virava a resposta da pergunta do
# Sentry e o read da descrição curta encontrava EOF, sem chamar o editor. Uma 2ª linha
# vazia faria o fluxo antigo cair no editor e o teste passaria sem provar nada.
po_input=$'Título de teste\nhttps://sentry.example.com/issues/42/'
for po_flag_setup in "SENTRY_ENABLED=true" "SENTRY_ENABLED="; do
  po_flag_label="$po_flag_setup"
  if [ "$po_flag_setup" = "SENTRY_ENABLED=" ]; then
    po_flag_label="SENTRY_ENABLED vazio"
  fi
  rm -f "$fake_editor_marker"
  po_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $po_flag_setup; EDITOR='$fake_editor'; ask_questions_po >/dev/null 2>&1; printf '%s|%s' \"\$TITLE\" \"\$DESCRIPTION\"" <<< "$po_input" 2>/dev/null || true)"
  assert_eq "Título de teste|$fake_editor_text" "$po_result" "$po_flag_label: TITLE vem da 1ª linha e DESCRIPTION do editor"
  assert_exists "$fake_editor_marker" "$po_flag_label: o editor é chamado pra descrição"
done

echo
echo "== main(): po de ponta a ponta com SENTRY_ENABLED=\"true\" (título + editor, sem link) =="
rm -f "$fake_editor_marker"
e2e_po_dir="$TEST_TMP/main_e2e_po"
e2e_po_rc="$(run_main "$e2e_po_dir" 'SENTRY_ENABLED="true"' $'1\nTítulo de teste\nN' "EDITOR='$fake_editor'")"
e2e_po_output="$(cat "$e2e_po_dir/output")"
assert_eq "0" "$e2e_po_rc" "main() termina com código 0 ao recusar o envio"
assert_contains "$e2e_po_output" "Cancelado." "main() cancela ao receber N na confirmação"
assert_contains "$e2e_po_output" "Título do pedido: Título de teste" "prompt renderizado contém o título"
assert_contains "$e2e_po_output" "Descrição do pedido: $fake_editor_text" "prompt renderizado contém a descrição vinda do editor"
assert_exists "$fake_editor_marker" "main() chama o editor pra descrição do po"
# shellcheck disable=SC2016
# Aspas simples intencionais: '${' é o texto literal buscado no output renderizado.
assert_not_contains "$e2e_po_output" '${' "nenhuma variável \${...} sobra sem substituir no prompt"

echo
echo "== po.md e auto_scrum.sh: sem EXTRA/SENTRY_LINE_EXTRA =="
PO_TEMPLATE="$SCRIPT_DIR/../templates/po.md"
assert_eq "DESCRIPTION PO_TICKET_FORMAT_BLOCK PO_VALIDATION_BLOCK TITLE" "$(template_vars "$PO_TEMPLATE")" "po.md referencia exatamente as variáveis preenchidas pra esse TYPE"
# -w não casa STACK_DESCRIPTION_EXTRA (o _ conta como caractere de palavra).
extra_lines="$(grep -nwE 'EXTRA|SENTRY_LINE_EXTRA' "$AUTO_SCRUM_SH" "$SCRIPT_DIR"/../templates/*.md || true)"
assert_eq "" "$extra_lines" "nenhuma referência a EXTRA/SENTRY_LINE_EXTRA no script nem nos templates"

echo
echo "== build_sentry_blocks: SENTRY_BLOCK_PO só referencia o link (MCP e fallback vivem em po_sentry.md) =="
sentry_block_po="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; SENTRY_ENABLED=true; build_sentry_blocks; printf '%s' \"\$SENTRY_BLOCK_PO\"" 2>/dev/null || true)"
assert_contains "$sentry_block_po" "link de erro externo, referencie-o" "com SENTRY_ENABLED=true, SENTRY_BLOCK_PO instrui a referenciar o link de erro externo"
assert_not_contains "$sentry_block_po" "MCP" "SENTRY_BLOCK_PO não fala de MCP"
assert_not_contains "$sentry_block_po" "traceback" "SENTRY_BLOCK_PO não fala de traceback"

# po_validation_block_exact <setup>: PO_VALIDATION_BLOCK real, montado na mesma ordem de
# main() e seguido do marcador <FIM> — o $(...) de quem chama apagaria o \n final que o
# bloco tem quando SENTRY_BLOCK_PO é vazio, e a comparação exata precisa dele.
po_validation_block_exact() {
  local setup="$1"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $setup; build_sentry_blocks; build_permission_blocks; build_production_blocks; build_po_blocks; printf '%s<FIM>' \"\$PO_VALIDATION_BLOCK\"" 2>/dev/null || true
}

# po_validation_block <setup>: o mesmo bloco, sem o marcador. Reusa
# po_validation_block_exact pra ordem dos build_* ficar num lugar só.
po_validation_block() {
  local setup="$1" out
  out="$(po_validation_block_exact "$setup")"
  printf '%s' "${out%<FIM>}"
}
po_validation_sentry="$(po_validation_block "SENTRY_ENABLED=true")"
for po_shared_tpl in po_discussion po_jira; do
  # shellcheck disable=SC2016
  # Aspas simples intencionais: é a lista de variáveis pro envsubst expandir, não
  # queremos que o bash expanda antes (mesmo padrão dos outros templates).
  rendered_po_shared="$(PO_VALIDATION_BLOCK="$po_validation_sentry" PO_TICKET_FORMAT_BLOCK="[formato de exemplo]" TITLE="assunto de exemplo" JIRA_LINK="https://jira.example.com/browse/SC-999" DESCRIPTION="descrição de exemplo" envsubst '$PO_VALIDATION_BLOCK $PO_TICKET_FORMAT_BLOCK $TITLE $JIRA_LINK $DESCRIPTION' < "$SCRIPT_DIR/../templates/$po_shared_tpl.md")"
  assert_contains "$rendered_po_shared" "link de erro externo" "$po_shared_tpl.md com SENTRY_ENABLED=true continua instruindo a referenciar link de erro externo"
  assert_not_contains "$rendered_po_shared" "traceback" "$po_shared_tpl.md com SENTRY_ENABLED=true não recebe o fallback de traceback"
done
assert_not_contains "$(po_validation_block "SENTRY_ENABLED=false")" "Sentry" "com SENTRY_ENABLED=false, PO_VALIDATION_BLOCK não menciona Sentry"

echo
echo "== build_production_blocks: PRODUCTION_ENABLED liga a pergunta de impacto em produção no PO_VALIDATION_BLOCK =="
# Referência fixa: com a chave ligada, o bloco é byte a byte o texto de antes da flag.
po_validation_expected_all_on='Considere o pedido válido quando, ao mesmo tempo:
- resolve um problema real de usuário ou de negócio;
- está dentro do propósito/escopo atual do sistema;
- o esforço e o risco envolvidos parecem proporcionais ao benefício.

Se faltar informação essencial para decidir (pedido vago, sem contexto suficiente),
pergunte antes de concluir — não presuma.

Análise obrigatória, independente do resultado:
- É necessário adicionar uma nova permissão para esse fluxo?
- Existe impacto em produção?
- Quais são os impactos negativos possíveis?
- Se o pedido tiver relação com Sentry ou outro link de erro externo, referencie-o no
  ticket (ou na explicação de rejeição, se for o caso).<FIM>'
assert_eq "$po_validation_expected_all_on" "$(po_validation_block_exact "PERMISSION_ENABLED=true; SENTRY_ENABLED=true; PRODUCTION_ENABLED=true")" "PRODUCTION_ENABLED=true com permissão e Sentry ligados: bloco idêntico ao texto de antes da flag"

po_validation_expected_only_production='Considere o pedido válido quando, ao mesmo tempo:
- resolve um problema real de usuário ou de negócio;
- está dentro do propósito/escopo atual do sistema;
- o esforço e o risco envolvidos parecem proporcionais ao benefício.

Se faltar informação essencial para decidir (pedido vago, sem contexto suficiente),
pergunte antes de concluir — não presuma.

Análise obrigatória, independente do resultado:
- Existe impacto em produção?
- Quais são os impactos negativos possíveis?
<FIM>'
assert_eq "$po_validation_expected_only_production" "$(po_validation_block_exact "PERMISSION_ENABLED=; SENTRY_ENABLED=; PRODUCTION_ENABLED=true")" "PRODUCTION_ENABLED=true com permissão e Sentry desligados: bloco idêntico ao texto de antes da flag (inclusive o \\n final)"

# Desligar só tira a linha, nas quatro combinações de permissão/Sentry. As aspas em
# "$production_line" tornam o padrão literal (sem elas, o ? seria curinga).
production_line=$'\n- Existe impacto em produção?'
for po_combo_permission in "" true; do
  for po_combo_sentry in "" true; do
    po_combo="PERMISSION_ENABLED=$po_combo_permission; SENTRY_ENABLED=$po_combo_sentry"
    po_combo_label="PERMISSION_ENABLED='$po_combo_permission', SENTRY_ENABLED='$po_combo_sentry'"
    po_block_on="$(po_validation_block_exact "$po_combo; PRODUCTION_ENABLED=true")"
    po_block_off="$(po_validation_block_exact "$po_combo; PRODUCTION_ENABLED=false")"
    assert_eq "${po_block_on/"$production_line"/}" "$po_block_off" "$po_combo_label: desligar PRODUCTION_ENABLED só remove a linha da pergunta, sem deixar linha em branco"
    assert_contains "$po_block_on" "$production_line" "$po_combo_label: com PRODUCTION_ENABLED=true, o bloco tem a linha da pergunta de impacto em produção"
    assert_not_contains "$po_block_off" "Existe impacto em produção?" "$po_combo_label: com PRODUCTION_ENABLED=false, o bloco não tem a pergunta de impacto em produção"
    assert_contains "$po_block_off" "- Quais são os impactos negativos possíveis?" "$po_combo_label: com PRODUCTION_ENABLED=false, a pergunta de impactos negativos continua"
  done
done

# Só "true" (exato, minúsculo) liga. Conferir a pergunta de impactos negativos impede que um
# bloco vazio (helper que morreu) passe por "não tem a pergunta".
for production_value in "" false TRUE sim 1; do
  po_block="$(po_validation_block "PRODUCTION_ENABLED='$production_value'")"
  assert_not_contains "$po_block" "Existe impacto em produção?" "PRODUCTION_ENABLED='$production_value': o bloco não tem a pergunta de impacto em produção"
  assert_contains "$po_block" "- Quais são os impactos negativos possíveis?" "PRODUCTION_ENABLED='$production_value': o bloco continua montado"
done
po_block="$(po_validation_block ":")"
assert_not_contains "$po_block" "Existe impacto em produção?" "PRODUCTION_ENABLED com o valor global padrão: o bloco não tem a pergunta de impacto em produção"
assert_contains "$po_block" "- Quais são os impactos negativos possíveis?" "PRODUCTION_ENABLED com o valor global padrão: o bloco continua montado"

echo
echo "== main(): PRODUCTION_ENABLED de ponta a ponta nos quatro pontos de entrada do PO =="
# logs_for_type <logs_dir> <type>: imprime, um por linha, os logs <type>_<timestamp>.md de
# <logs_dir>. O [0-9] logo depois do '_' impede que 'po' case com po_discussion_*.md: se a
# numeração do menu mudar, o teste não pode passar olhando o log do template errado.
logs_for_type() {
  local logs_dir="$1" type="$2" f
  for f in "$logs_dir/${type}_"[0-9]*.md; do
    [ -e "$f" ] || continue
    printf '%s\n' "$f"
  done
}

# check_production_e2e <type> <variant> <extra_conf_line> <stdin> [setup]: roda main() com
# run_main, num case_dir próprio, e confere o prompt no log (o texto exato enviado ao
# claude). O .nick.conf tem as outras chaves vazias (config_keys_blank), depois a
# <extra_conf_line> (que vence o valor vazio da mesma chave) e, fora da variante sem_linha,
# PRODUCTION_ENABLED="<variant>". <variant> é true, false, TRUE (valor inválido, que desliga
# com aviso) ou sem_linha.
check_production_e2e() {
  local type="$1" variant="$2" extra_line="$3" input="$4" setup="${5:-:}"
  local case_dir="$TEST_TMP/main_e2e_production/${type}_$variant"
  local conf_line label rc logs log_text output
  conf_line="$(config_keys_blank PRODUCTION_ENABLED)"
  if [ -n "$extra_line" ]; then
    conf_line="$conf_line"$'\n'"$extra_line"
  fi
  if [ "$variant" = "sem_linha" ]; then
    label="$type, .nick.conf sem a linha PRODUCTION_ENABLED"
  else
    label="$type, PRODUCTION_ENABLED=\"$variant\""
    conf_line="$conf_line"$'\n'"PRODUCTION_ENABLED=\"$variant\""
  fi
  rc="$(run_main "$case_dir" "$conf_line" "$input" "$setup")"
  logs="$(logs_for_type "$case_dir/logs" "$type")"
  log_text="$(cat "$(head -n1 <<< "$logs")" 2>/dev/null || true)"
  output="$(cat "$case_dir/output" 2>/dev/null || true)"
  assert_eq "0" "$rc" "$label: main() termina com código 0"
  assert_eq "1" "$(grep -c . <<< "$logs" || true)" "$label: gera exatamente um log ${type}_<timestamp>.md"
  assert_contains "$log_text" "- Quais são os impactos negativos possíveis?" "$label: o prompt tem a pergunta de impactos negativos"
  if [ "$variant" = "true" ]; then
    assert_contains "$log_text" "- Existe impacto em produção?" "$label: o prompt tem a pergunta de impacto em produção"
  else
    assert_not_contains "$log_text" "Existe impacto em produção?" "$label: o prompt não tem a pergunta de impacto em produção"
    assert_not_contains "$output" "Existe impacto em produção?" "$label: a saída (stdout+stderr) não tem a pergunta de impacto em produção"
  fi
  case "$variant" in
    sem_linha)
      assert_eq "1" "$(count_warnings "$case_dir/output")" "$label: exatamente um aviso na saída"
      assert_contains "$output" "$(missing_warning "$case_dir/cwd/.nick.conf" "$production_note_line" 'PRODUCTION_ENABLED="false"')" "$label: o aviso de chave ausente mostra o caminho do .nick.conf e só PRODUCTION_ENABLED"
      ;;
    TRUE)
      assert_eq "1" "$(count_warnings "$case_dir/output")" "$label: exatamente um aviso na saída"
      assert_contains "$output" "$(value_warning "$case_dir/cwd/.nick.conf:$(wc -l < "$case_dir/cwd/.nick.conf")" PRODUCTION_ENABLED TRUE "$production_note")" "$label: o aviso de valor mostra o .nick.conf, a linha e a nota de PRODUCTION_ENABLED"
      ;;
    *)
      assert_eq "0" "$(count_warnings "$case_dir/output")" "$label: nenhum aviso na saída"
      ;;
  esac
}

# stdin de cada TYPE: a opção do menu, as respostas do roteiro e N pra recusar o envio. Com
# Jira ou Sentry ligado (e o outro desligado), a opção 3 é po_jira ou po_sentry.
for production_variant in true false TRUE sem_linha; do
  check_production_e2e po "$production_variant" "" $'1\nTítulo de teste\nN' "EDITOR='$fake_editor'"
  check_production_e2e po_discussion "$production_variant" "" $'2\nassunto de teste\nN'
  check_production_e2e po_jira "$production_variant" 'JIRA_ENABLED="true"' $'3\nhttps://jira.example.com/browse/SC-14\n\nN'
  check_production_e2e po_sentry "$production_variant" 'SENTRY_ENABLED="true"' "3"$'\n'"$sentry_link"$'\n\nN'
done

echo
echo "== main(): projeto 'Nenhum' no menu não tem a pergunta de impacto em produção =="
none_dir="$TEST_TMP/main_e2e_production_none"
mkdir -p "$none_dir/cwd" "$none_dir/projects"
printf '%s\n' 'STACK_DESCRIPTION="x"' > "$none_dir/projects/alpha.conf"
none_rc=0
# cwd sem .nick.conf, pra cair no menu de projetos. stdin: projeto 2 (Nenhum), tipo 2
# (po_discussion), o assunto e N pra recusar o envio.
timeout 5 bash -c "cd '$none_dir/cwd' || exit 99; source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$none_dir/logs'; PROJECTS_DIR='$none_dir/projects'; main" <<< $'2\n2\nassunto de teste\nN' > "$none_dir/output" 2>&1 || none_rc=$?
none_logs="$(logs_for_type "$none_dir/logs" po_discussion)"
none_log_text="$(cat "$(head -n1 <<< "$none_logs")" 2>/dev/null || true)"
none_output="$(cat "$none_dir/output" 2>/dev/null || true)"
assert_eq "0" "$none_rc" "Nenhum: main() termina com código 0"
assert_eq "1" "$(grep -c . <<< "$none_logs" || true)" "Nenhum: gera exatamente um log po_discussion_<timestamp>.md"
assert_contains "$none_log_text" "- Quais são os impactos negativos possíveis?" "Nenhum: o prompt tem a pergunta de impactos negativos"
assert_not_contains "$none_log_text" "Existe impacto em produção?" "Nenhum: o prompt não tem a pergunta de impacto em produção"
assert_not_contains "$none_output" "Existe impacto em produção?" "Nenhum: a saída (stdout+stderr) não tem a pergunta de impacto em produção"
assert_eq "0" "$(count_warnings "$none_dir/output")" "Nenhum (com alpha.conf incompleto em projects/): nenhum aviso na saída"

echo
echo "== main(): TDD_ENABLED de ponta a ponta nos prompts de Tech Leader, Desenvolvimento e Review =="
# Referências fixas dos blocos que dependem de TDD_ENABLED. Com a TDD ligada, cada uma é o
# texto de antes da flag.
tdd_tl_rules_on='Se faltar informação técnica essencial para montar o plano, pergunte antes de concluir —
não presuma.

O plano deve ser pensado para ser executado usando a metodologia TDD. Quem vai executar o
plano é um(a) desenvolvedor(a) IA em outra sessão — não é você.

O plano deve terminar com estas duas seções:
## Critérios de aceite
Se o ticket já trouxer critérios de aceite na descrição, não copie cegamente: avalie
cada um à luz do plano técnico. Se algum não fizer mais sentido, remova e explique o
porquê; se faltar algo que o plano exige, adicione. Essa seção é obrigatória — a etapa
de review usa ela para validar o trabalho feito.
## Habilidades necessárias
As habilidades que o(a) desenvolvedor(a) precisa para executar o serviço.'
tdd_dev_block_on='Siga a metodologia TDD e garanta que todos os critérios de aceite do plano sejam
atendidos.

Antes de finalizar, rode a suíte completa de testes (não só os novos) e confirme que
passam.

No final, traga um resumo do que foi feito, incluindo os testes criados/alterados.'
tdd_review_block_on='Isso
inclui, no mínimo: rodar a suíte de testes e confirmar que passam, e validar se os
critérios de aceite do plano foram cumpridos.'
# Com TDD_ENABLED="false": nenhum menciona TDD, e cada critério de aceite é verificado pela
# forma descrita no plano.
tdd_tl_rules_off='Se faltar informação técnica essencial para montar o plano, pergunte antes de concluir —
não presuma.

Quem vai executar o plano é um(a) desenvolvedor(a) IA em outra sessão — não é você. O
plano não deve exigir testes automatizados nem planejar infraestrutura de testes que o
ticket não peça.

O plano deve terminar com estas duas seções:
## Critérios de aceite
Se o ticket já trouxer critérios de aceite na descrição, não copie cegamente: avalie
cada um à luz do plano técnico. Se algum não fizer mais sentido, remova e explique o
porquê; se faltar algo que o plano exige, adicione. Essa seção é obrigatória — a etapa
de review usa ela para validar o trabalho feito.
Para cada critério, diga como ele será verificado (por exemplo, verificação manual ou
visual, com o passo a passo) — é por essa descrição que o review vai validá-lo.
## Habilidades necessárias
As habilidades que o(a) desenvolvedor(a) precisa para executar o serviço.'
tdd_dev_block_off='Garanta que todos os critérios de aceite do plano sejam atendidos, verificando cada um
pela forma descrita no plano.

Antes de finalizar, se o projeto tiver uma suíte de testes automatizados, rode-a completa
e confirme que passa. Se não tiver, não crie testes nem infraestrutura de testes que o
plano não peça.

No final, traga um resumo do que foi feito e, para cada critério de aceite, como ele foi
verificado.'
tdd_review_block_off='Isso
inclui, no mínimo: rodar a suíte de testes, se o projeto tiver uma, e confirmar que
passam; e validar cada critério de aceite do plano pela forma de verificação descrita
nele.'

# Referências fixas do prompt completo de cada TYPE com a TDD ligada, com as entradas de
# check_tdd_e2e e STACK_DESCRIPTION vazia (os blocos de stack viram "."). Terminam no
# marcador <FIM>, como po_validation_expected_all_on: o $(...) apagaria o \n final do log.
# São lidas por indireção (tdd_ref_<type>_on), por isso o SC2034 desligado em cada uma.
# shellcheck disable=SC2034
tdd_ref_tech_leader_on='Você é o tech leader Senior da empresa.

Você recebeu um ticket do Jira com as informações abaixo. Analise-o e monte o plano de
ação em docs/plans/SC-20_<resumo-curto>.md — troque <resumo-curto> por um resumo do
título da tarefa (poucas palavras, kebab-case), escolhido por você. Esse resumo serve só
para facilitar a leitura humana do nome do arquivo; o identificador único continua sendo
o ID do Jira.

'"$tdd_tl_rules_on"'



Se já existir um arquivo docs/plans/SC-20_*.md, pergunte antes de sobrescrever.

---
DADOS DE ENTRADA
ID Jira: SC-20
Título: Título de teste
Descrição: '"$fake_editor_text"'
<FIM>'
# A última linha de tech_leader_jira termina em ": " (contexto extra vazio); o espaço vem
# concatenado pra não deixar espaço no fim de linha neste arquivo. As crases de `auto_scrum`
# são texto literal do template, não substituição de comando (SC2016).
# shellcheck disable=SC2016
tdd_ref_tech_leader_jira_on='Você é o tech leader Senior da empresa.

Você recebeu o link de um ticket do Jira que já existe — o pedido não precisa ser
retranscrito à mão.

Antes de montar o plano, busque o ticket via MCP do Jira do projeto alvo, a partir do
link informado: título, ID, descrição e anexos de imagem. Visualize os anexos de imagem
encontrados e considere o conteúdo deles no plano técnico (ex: diagrama de arquitetura,
print de comportamento esperado). Anexos de vídeo estão fora de escopo — não busque nem
trate anexos de vídeo, mesmo que existam no ticket.

Se a busca falhar por qualquer motivo (link inválido, ticket não encontrado, MCP
indisponível ou não configurado neste projeto), avise o analista e peça que ele resolva
o link/acesso antes de continuar. Se não for possível resolver, encerre esse fluxo e
oriente o analista a recomeçar pela opção "Tech Leader - Criar plano" no menu do
`auto_scrum`, digitando ID, título e descrição diretamente por lá — não continue esta
conversa tentando coletar esses dados aqui.

Com o ticket em mãos, analise-o e monte o plano de ação em
docs/plans/<ID-do-ticket>_<resumo-curto>.md — troque <ID-do-ticket> pelo ID do Jira que
você acabou de buscar e <resumo-curto> por um resumo do título da tarefa (poucas
palavras, kebab-case), escolhido por você. Esse resumo serve só para facilitar a leitura
humana do nome do arquivo; o identificador único continua sendo o ID do Jira buscado.

'"$tdd_tl_rules_on"'



Se já existir um arquivo docs/plans/<ID-do-ticket>_*.md para o ID buscado, pergunte antes
de sobrescrever.

---
DADOS DE ENTRADA
Link do ticket no Jira: https://jira.example.com/browse/SC-20
Contexto extra (opcional, complementa o ticket buscado):'" "'
<FIM>'
# shellcheck disable=SC2034
tdd_ref_development_on='Você é desenvolvedor(a) Senior.

Foi te atribuído o serviço descrito em docs/plans/SC-20_*.md. Se o plano tiver alguma
ambiguidade ou faltar informação técnica pra implementar, pergunte antes de prosseguir
— não presuma.

Confirme que está na branch SC-20 antes de começar; se não estiver, avise antes de
prosseguir.
'"$tdd_dev_block_on"'
Não faça git add, git commit ou git push sem a minha autorização expressa.

Faça só isso e espere novos comandos.
<FIM>'
# shellcheck disable=SC2034
tdd_ref_review_on='Você é desenvolvedor(a) Senior.

Foi te atribuído o serviço de revisar o trabalho feito na branch SC-20. Confirme
que está nessa branch antes de revisar; se não estiver, avise antes de prosseguir.

O plano do trabalho está em docs/plans/SC-20_*.md — se houver mais de um arquivo nesse padrão,
ignore docs/plans/SC-20_review.md (é o resultado de um review anterior, não o plano).

Faça um review de código completo, com o mesmo rigor que um(a) senior aplicaria —
estilo, performance (ex.: N+1), legibilidade, segurança, o que mais for relevante. '"$tdd_review_block_on"'

Liste claramente os problemas encontrados (se houver), com sugestão de correção — não
corrija o código você mesmo, quem aplica a correção é o(a) desenvolvedor(a) numa
próxima rodada. Se estiver tudo certo, diga explicitamente que está aprovado.

Grave esse resultado (aprovado, ou a lista de problemas com sugestão de correção) em
docs/plans/SC-20_review.md, sobrescrevendo se já existir.
<FIM>'

# Referências fixas do prompt completo com TDD_ENABLED="false": as de antes da flag com os
# trechos de TDD trocados pelos textos desligados (os dois blocos _TL entram juntos, dentro
# de TECH_LEADER_PLAN_RULES_BLOCK).
# shellcheck disable=SC2034
tdd_ref_tech_leader_off="${tdd_ref_tech_leader_on/"$tdd_tl_rules_on"/"$tdd_tl_rules_off"}"
# shellcheck disable=SC2034
tdd_ref_tech_leader_jira_off="${tdd_ref_tech_leader_jira_on/"$tdd_tl_rules_on"/"$tdd_tl_rules_off"}"
# shellcheck disable=SC2034
tdd_ref_development_off="${tdd_ref_development_on/"$tdd_dev_block_on"/"$tdd_dev_block_off"}"
# shellcheck disable=SC2034
tdd_ref_review_off="${tdd_ref_review_on/"$tdd_review_block_on"/"$tdd_review_block_off"}"

# Um template que referencie variável de outro TYPE passaria no teste de consistência
# genérico, mas seria substituído silenciosamente por vazio.
assert_eq "JIRA_ID PLAN_PATH STACK_BLOCK_DEV TDD_BLOCK_DEV" "$(template_vars "$SCRIPT_DIR/../templates/development.md")" "development.md referencia exatamente as variáveis preenchidas pra esse TYPE"
assert_eq "JIRA_ID PLAN_PATH REVIEW_PATH STACK_BLOCK_REVIEW TDD_BLOCK_REVIEW" "$(template_vars "$SCRIPT_DIR/../templates/review.md")" "review.md referencia exatamente as variáveis preenchidas pra esse TYPE"

# tdd_log_text <logs_dir> <type>: imprime o único log <type>_<timestamp>.md de <logs_dir>
# seguido do marcador <FIM> (só o marcador, se não houver log).
tdd_log_text() {
  local logs_dir="$1" type="$2" logs
  logs="$(logs_for_type "$logs_dir" "$type")"
  cat "$(head -n1 <<< "$logs")" 2>/dev/null || true
  printf '<FIM>'
}

# check_tdd_e2e <type> <variant> <extra_conf_line> <stdin> [setup]: roda main() com
# run_main, num case_dir próprio, e compara o prompt no log (o texto exato enviado ao
# claude) com a referência fixa tdd_ref_<type>_on (ou _off, na variante false). O
# .nick.conf tem as outras chaves vazias (config_keys_blank TDD_ENABLED), depois a
# <extra_conf_line> e, conforme <variant>: true → TDD_ENABLED="true"; vazio →
# TDD_ENABLED=""; TRUE → TDD_ENABLED="TRUE"; sem_linha → nada; false → TDD_ENABLED="false".
check_tdd_e2e() {
  local type="$1" variant="$2" extra_line="$3" input="$4" setup="${5:-:}"
  local case_dir="$TEST_TMP/main_e2e_tdd/${type}_$variant"
  local conf_line label rc logs ref_var log_text
  conf_line="$(config_keys_blank TDD_ENABLED)"
  if [ -n "$extra_line" ]; then
    conf_line="$conf_line"$'\n'"$extra_line"
  fi
  case "$variant" in
    true) conf_line="$conf_line"$'\n''TDD_ENABLED="true"' ;;
    vazio) conf_line="$conf_line"$'\n''TDD_ENABLED=""' ;;
    TRUE) conf_line="$conf_line"$'\n''TDD_ENABLED="TRUE"' ;;
    false) conf_line="$conf_line"$'\n''TDD_ENABLED="false"' ;;
  esac
  label="$type, TDD_ENABLED $variant"
  rc="$(run_main "$case_dir" "$conf_line" "$input" "$setup")"
  logs="$(logs_for_type "$case_dir/logs" "$type")"
  log_text="$(tdd_log_text "$case_dir/logs" "$type")"
  assert_eq "0" "$rc" "$label: main() termina com código 0"
  assert_eq "1" "$(grep -c . <<< "$logs" || true)" "$label: gera exatamente um log ${type}_<timestamp>.md"
  if [ "$variant" = "false" ]; then
    ref_var="tdd_ref_${type}_off"
    assert_eq "${!ref_var}" "$log_text" "$label: o prompt é idêntico à referência fixa do texto desligado"
    assert_not_contains "$log_text" "TDD" "$label: o prompt não menciona TDD"
    # shellcheck disable=SC2016
    # Aspas simples intencionais: '${' é o texto literal buscado no prompt.
    assert_not_contains "$log_text" '${' "$label: nenhuma variável \${...} sobra sem substituir no prompt"
    case "$type" in
      development)
        assert_contains "$log_text" "se o projeto tiver uma suíte de testes automatizados" "$label: só manda rodar a suíte se o projeto tiver uma"
        assert_not_contains "$log_text" "rode a suíte completa de testes (não só os novos)" "$label: não manda rodar a suíte completa incondicionalmente"
        ;;
      review)
        assert_contains "$log_text" "pela forma de verificação descrita" "$label: valida cada critério pela forma de verificação descrita no plano"
        ;;
      *)
        assert_contains "$log_text" "diga como ele será verificado" "$label: a regra de plano exige dizer como cada critério será verificado"
        ;;
    esac
  else
    ref_var="tdd_ref_${type}_on"
    assert_eq "${!ref_var}" "$log_text" "$label: o prompt é idêntico à referência fixa (o texto de antes da flag)"
  fi
  case "$variant" in
    sem_linha)
      assert_eq "1" "$(count_warnings "$case_dir/output")" "$label: exatamente um aviso na saída"
      assert_contains "$(cat "$case_dir/output" 2>/dev/null || true)" "$(missing_warning "$case_dir/cwd/.nick.conf" "$tdd_note_line" 'TDD_ENABLED="true"')" "$label: o aviso de chave ausente mostra o .nick.conf e só TDD_ENABLED, sugerindo \"true\""
      ;;
    TRUE)
      assert_eq "1" "$(count_warnings "$case_dir/output")" "$label: exatamente um aviso na saída"
      assert_contains "$(cat "$case_dir/output" 2>/dev/null || true)" "$(value_warning "$case_dir/cwd/.nick.conf:$(wc -l < "$case_dir/cwd/.nick.conf")" TDD_ENABLED TRUE "$tdd_note")" "$label: o aviso de valor mostra o .nick.conf, a linha e a nota de TDD_ENABLED"
      ;;
    *)
      assert_eq "0" "$(count_warnings "$case_dir/output")" "$label: nenhum aviso na saída"
      ;;
  esac
}

# stdin de cada TYPE: a opção do menu, as respostas do roteiro e N pra recusar o envio. Com
# Jira desligado, tech_leader é 3, development 4 e review 5; com Jira ligado,
# tech_leader_jira é 5. O plano vazio usa o padrão docs/plans/SC-20_*.md.
tdd_input_tech_leader=$'3\nSC-20\nTítulo de teste\nN'
tdd_input_tech_leader_jira=$'5\nhttps://jira.example.com/browse/SC-20\n\nN'
tdd_input_development=$'4\nSC-20\n\nN'
tdd_input_review=$'5\nSC-20\n\nN'
for tdd_variant in true vazio TRUE sem_linha false; do
  check_tdd_e2e tech_leader "$tdd_variant" "" "$tdd_input_tech_leader" "EDITOR='$fake_editor'"
  check_tdd_e2e tech_leader_jira "$tdd_variant" 'JIRA_ENABLED="true"' "$tdd_input_tech_leader_jira"
  check_tdd_e2e development "$tdd_variant" "" "$tdd_input_development"
  check_tdd_e2e review "$tdd_variant" "" "$tdd_input_review"
done

echo
echo "== main(): projeto 'Nenhum' mantém a TDD ligada nos prompts de Tech Leader, Desenvolvimento e Review =="
# cwd sem .nick.conf e PROJECTS_DIR vazio: o menu de projetos é 1) Nenhum 2) Sair (q). stdin:
# Nenhum (1) e depois o roteiro do TYPE.
for tdd_none_type in tech_leader development review; do
  tdd_none_dir="$TEST_TMP/main_e2e_tdd_none/$tdd_none_type"
  mkdir -p "$tdd_none_dir/cwd" "$tdd_none_dir/projects"
  tdd_none_input_var="tdd_input_$tdd_none_type"
  tdd_none_rc=0
  timeout 5 bash -c "cd '$tdd_none_dir/cwd' || exit 99; source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$tdd_none_dir/logs'; PROJECTS_DIR='$tdd_none_dir/projects'; EDITOR='$fake_editor'; main" <<< "1"$'\n'"${!tdd_none_input_var}" > "$tdd_none_dir/output" 2>&1 || tdd_none_rc=$?
  tdd_none_ref_var="tdd_ref_${tdd_none_type}_on"
  assert_eq "0" "$tdd_none_rc" "Nenhum, $tdd_none_type: main() termina com código 0"
  assert_eq "${!tdd_none_ref_var}" "$(tdd_log_text "$tdd_none_dir/logs" "$tdd_none_type")" "Nenhum, $tdd_none_type: o prompt é idêntico à referência fixa (TDD ligada)"
  assert_eq "0" "$(count_warnings "$tdd_none_dir/output")" "Nenhum, $tdd_none_type: nenhum aviso na saída"
done

# Com "Nenhum", JIRA_ENABLED fica vazio e tech_leader_jira não aparece no menu. Por isso esse
# TYPE é renderizado sem passar pelo menu: dá source, não carrega configuração, chama os
# build_*_blocks na ordem em que main() os chama, exporta e roda o envsubst com a lista lida
# do próprio script — os mesmos passos de main() depois de resolve_stack.
tdd_none_builds="$(grep -E '^  build_[a-z_]+_blocks$' "$AUTO_SCRUM_SH" | tr -d ' ' | paste -sd ';' -)"
tdd_none_envsubst_vars="$(grep -F "envsubst '" "$AUTO_SCRUM_SH" | sed -E "s/.*envsubst '([^']*)'.*/\1/")"
tdd_none_jira_text="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $tdd_none_builds; JIRA_LINK='https://jira.example.com/browse/SC-20'; DESCRIPTION=''; export ${tdd_none_envsubst_vars//\$/}; envsubst '$tdd_none_envsubst_vars' < '$SCRIPT_DIR/../templates/tech_leader_jira.md'; printf '<FIM>'" 2>/dev/null || true)"
assert_eq "$tdd_ref_tech_leader_jira_on" "$tdd_none_jira_text" "Nenhum, tech_leader_jira (renderizado sem o menu): o prompt é idêntico à referência fixa (TDD ligada)"

echo
echo "== build_tdd_blocks: só TDD_ENABLED=\"false\" exato desliga a TDD nos blocos de Tech Leader, Desenvolvimento e Review =="
# tdd_block_exact <setup> <variável>: num bash separado, dá source, roda <setup>, monta os
# blocos na ordem de main() (build_tdd_blocks antes de build_tech_leader_blocks, que embute
# os blocos _TL) e imprime <variável> seguida do marcador <FIM>.
tdd_block_exact() {
  local setup="$1" var="$2"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $setup; build_tdd_blocks; build_tech_leader_blocks; printf '%s<FIM>' \"\${$var}\"" 2>/dev/null || true
}

# Qualquer valor diferente de "false" exato (inclusive o valor global padrão, que é o caso
# "Nenhum") mantém os três blocos idênticos ao texto de antes da flag.
for tdd_value_setup in "TDD_ENABLED='true'" "TDD_ENABLED=''" "TDD_ENABLED='TRUE'" "TDD_ENABLED='True'" "TDD_ENABLED='sim'" "TDD_ENABLED=' false'" "TDD_ENABLED='FALSE'" ":"; do
  tdd_value_label="$tdd_value_setup"
  if [ "$tdd_value_setup" = ":" ]; then
    tdd_value_label="TDD_ENABLED com o valor global padrão"
  fi
  assert_eq "$tdd_tl_rules_on<FIM>" "$(tdd_block_exact "$tdd_value_setup" TECH_LEADER_PLAN_RULES_BLOCK)" "$tdd_value_label: TECH_LEADER_PLAN_RULES_BLOCK idêntico ao texto de antes da flag"
  assert_eq "$tdd_dev_block_on<FIM>" "$(tdd_block_exact "$tdd_value_setup" TDD_BLOCK_DEV)" "$tdd_value_label: TDD_BLOCK_DEV é o texto ligado"
  assert_eq "$tdd_review_block_on<FIM>" "$(tdd_block_exact "$tdd_value_setup" TDD_BLOCK_REVIEW)" "$tdd_value_label: TDD_BLOCK_REVIEW é o texto ligado"
done

for tdd_off_check in "TECH_LEADER_PLAN_RULES_BLOCK|$tdd_tl_rules_off" "TDD_BLOCK_DEV|$tdd_dev_block_off" "TDD_BLOCK_REVIEW|$tdd_review_block_off"; do
  tdd_off_var="${tdd_off_check%%|*}"
  tdd_off_block="$(tdd_block_exact "TDD_ENABLED='false'" "$tdd_off_var")"
  assert_eq "${tdd_off_check#*|}<FIM>" "$tdd_off_block" "TDD_ENABLED=\"false\": $tdd_off_var idêntico à referência fixa do texto desligado"
  assert_not_contains "$tdd_off_block" "TDD" "TDD_ENABLED=\"false\": $tdd_off_var não menciona TDD"
  assert_not_contains "$tdd_off_block" $'\n\n\n' "TDD_ENABLED=\"false\": $tdd_off_var não deixa linha em branco sobrando"
  # shellcheck disable=SC2016
  # Aspas simples intencionais: '${' é o texto literal buscado no bloco.
  assert_not_contains "$tdd_off_block" '${' "TDD_ENABLED=\"false\": $tdd_off_var não tem \${...} literal"
done

echo
echo "== consistência: variáveis dos templates = lista do envsubst = lista do export =="
# Variável de template fora do envsubst sobra como \${...} literal no prompt; variável no
# envsubst/export sem uso em template é código morto; export e envsubst divergentes fazem o
# valor não chegar ao envsubst. Os três conjuntos saem ordenados, um por linha.
# shellcheck disable=SC2016
# Aspas simples intencionais: '${}' é a lista de caracteres pro tr apagar, não expansão.
vars_templates="$(grep -ohE '\$\{?[A-Za-z_][A-Za-z0-9_]*\}?' "$SCRIPT_DIR"/../templates/*.md | tr -d '${}' | LC_ALL=C sort -u)"
vars_envsubst="$(grep -F "envsubst '" "$AUTO_SCRUM_SH" | grep -oE '\$[A-Za-z_][A-Za-z0-9_]*' | tr -d '$' | LC_ALL=C sort -u)"
# Do "  export" até a primeira linha que não termina em "\" (inclusive a própria linha do
# export, se ela for única). Não usa grep -A3: com um export mais curto, pegaria linhas
# seguintes do main().
vars_export="$(awk '/^  export /{found=1} found{print} found && !/\\$/{exit}' "$AUTO_SCRUM_SH" | grep -oE '[A-Za-z_][A-Za-z0-9_]*' | grep -E '^[A-Z_][A-Z0-9_]*$' | LC_ALL=C sort -u)"
assert_eq "$vars_templates" "$vars_envsubst" "variáveis usadas nos templates são exatamente as da lista do envsubst"
assert_eq "$vars_envsubst" "$vars_export" "lista do envsubst é exatamente a lista do export"

echo
echo "== auto_scrum.sh: a única linha source/. é a de lib/config.sh (config nunca é executada) =="
# A configuração nunca é executada: o leitor (config_load) não usa source/eval, e lib/config.sh
# não tem source, . nem eval (teste estrutural em lib/tests/test_config.sh). Aqui, a lista
# inteira de linhas source/. tem de ser só a da biblioteca.
# shellcheck disable=SC2016
# Aspas simples intencionais: '$SCRIPT_DIR' é o texto literal buscado no código-fonte.
source_lines="$(grep -E '^[[:space:]]*(source|\.)[[:space:]]' "$AUTO_SCRUM_SH" || true)"
# shellcheck disable=SC2016
assert_eq 'source "$SCRIPT_DIR/../lib/config.sh"' "$source_lines" "auto_scrum.sh tem uma linha source/. só, a de lib/config.sh"

echo
echo "== auto_scrum.sh: carrega lib/config.sh com a diretiva do shellcheck logo acima =="
# shellcheck disable=SC2016
lib_source_number="$(grep -nxF 'source "$SCRIPT_DIR/../lib/config.sh"' "$AUTO_SCRUM_SH" | cut -d: -f1 || true)"
assert_eq "true" "$([ -n "$lib_source_number" ] && echo true || echo false)" "auto_scrum.sh tem a linha source \"\$SCRIPT_DIR/../lib/config.sh\""
assert_eq "# shellcheck source=SCRIPTDIR/../lib/config.sh" "$( [ -n "$lib_source_number" ] && sed -n "$((lib_source_number - 1))p" "$AUTO_SCRUM_SH" || true)" "a linha acima do source é '# shellcheck source=SCRIPTDIR/../lib/config.sh'"

echo
echo "== lib/config.sh: nenhuma chave concreta do auto_scrum na biblioteca nem nos testes dela =="
# As chaves vêm do próprio registro do auto_scrum, então este teste cobre sozinho as chaves
# futuras. -w não casa um nome mais longo (o _ conta como caractere de palavra).
LIB_CONFIG_SH="$SCRIPT_DIR/../../lib/config.sh"
LIB_TEST_CONFIG_SH="$SCRIPT_DIR/../../lib/tests/test_config.sh"
registered_keys="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; printf '%s\n' \"\${CONFIG_KEYS[@]}\"" 2>/dev/null || true)"
# Só garante que o laço abaixo não roda vazio, sem fixar a quantidade de chaves.
assert_eq "true" "$([ -n "$registered_keys" ] && echo true || echo false)" "o auto_scrum registra pelo menos uma chave"
for registered_key in $registered_keys; do
  assert_eq "" "$(grep -nw "$registered_key" "$LIB_CONFIG_SH" || true)" "lib/config.sh não cita $registered_key"
  assert_eq "" "$(grep -nw "$registered_key" "$LIB_TEST_CONFIG_SH" || true)" "lib/tests/test_config.sh não cita $registered_key"
done

echo
echo "== auto_scrum.sh: o namespace config_/CONFIG_ é só da biblioteca =="
auto_scrum_code="$(grep -vE '^[[:space:]]*#' "$AUTO_SCRUM_SH" || true)"
assert_eq "" "$(grep -nE '^[[:space:]]*(function[[:space:]]+)?config_[A-Za-z0-9_]*[[:space:]]*\(\)' <<< "$auto_scrum_code" || true)" "auto_scrum.sh não define função config_*"
assert_eq "" "$(grep -nE '(^|[^A-Za-z0-9_])CONFIG_[A-Za-z0-9_]*(\[[^]]*\])?\+?=' <<< "$auto_scrum_code" || true)" "auto_scrum.sh não atribui global CONFIG_*"
for old_name in is_toggle_config_key warn_malformed_config_line warn_invalid_toggle_value warn_missing_config_keys; do
  assert_eq "" "$(grep -nE "^[[:space:]]*${old_name}[[:space:]]*\(\)" "$AUTO_SCRUM_SH" || true)" "não sobrou definição de $old_name"
done
for old_name in PROJECT_CONFIG_KEYS CWD_CONFIG_NAME; do
  assert_eq "" "$(grep -nw "$old_name" "$AUTO_SCRUM_SH" || true)" "não sobrou $old_name"
done

echo
echo "== auto_scrum.sh: as chaves são registradas num lugar só, um bloco contíguo no topo =="
# register_block_problems <arquivo> <chaves>: imprime um problema por linha (nada, se o
# registro de <arquivo> está certo), comparando com <chaves> (uma por linha, na ordem de
# CONFIG_KEYS). Não recorta o bloco por linha em branco: o comentário de uma chave pode ter
# linha vazia (o esqueleto a escreve como "#"). Usa os números de linha das próprias chamadas
# "config_register_key" (fora de função, na coluna 0):
#   - toda chamada fora de comentário está na coluna 0 e há uma por chave;
#   - os nomes saem na ordem de <chaves>;
#   - entre a primeira e a última chamada não há definição de função nem atribuição CHAVE=;
#   - a última chamada vem antes da primeira função (o bloco fica no topo).
register_block_problems() {
  local file="$1" expected_keys="$2" code calls first last first_function
  code="$(grep -vE '^[[:space:]]*#' "$file" || true)"
  calls="$(grep -nE '^config_register_key [A-Z0-9_]+' "$file" || true)"
  if [ "$(grep -cw 'config_register_key' <<< "$code" || true)" != "$(grep -c . <<< "$expected_keys" || true)" ]; then
    echo "número de chamadas config_register_key diferente do número de chaves"
  fi
  if [ "$(cut -d' ' -f2 <<< "$calls")" != "$expected_keys" ]; then
    echo "nomes das chamadas fora da ordem de CONFIG_KEYS"
  fi
  first="$(head -n1 <<< "$calls" | cut -d: -f1)"
  last="$(tail -n1 <<< "$calls" | cut -d: -f1)"
  first_function="$(grep -nE '^[a-z_]+\(\) \{' "$file" | head -n1 | cut -d: -f1 || true)"
  if [ -z "$first" ] || [ -z "$first_function" ]; then
    echo "sem chamada de registro ou sem função no arquivo"
    return 0
  fi
  if sed -n "${first},${last}p" "$file" | grep -qE '^([a-z_]+\(\)|[A-Z_]+=)'; then
    echo "código entre a primeira e a última chamada de registro"
  fi
  if [ "$last" -ge "$first_function" ]; then
    echo "chamada de registro depois da primeira função"
  fi
}
assert_eq "" "$(register_block_problems "$AUTO_SCRUM_SH" "$registered_keys")" "o bloco de registro é contíguo, no topo, com uma chamada por chave na ordem de CONFIG_KEYS e nenhuma fora dele"
# Regressão: linha vazia dentro do comentário da primeira chave (depois da 1ª linha do
# comentário) não quebra a checagem.
register_dir="$TEST_TMP/register_block"
mkdir -p "$register_dir"
awk '{print} /^config_register_key /{found++} found == 1 && !done && !/^config_register_key /{print ""; done=1}' "$AUTO_SCRUM_SH" > "$register_dir/blank_comment_line.sh"
assert_eq "1" "$(diff "$AUTO_SCRUM_SH" "$register_dir/blank_comment_line.sh" | grep -c '^> $' || true)" "cópia de regressão: tem uma linha vazia a mais, dentro do bloco de registro"
assert_eq "" "$(register_block_problems "$register_dir/blank_comment_line.sh" "$registered_keys")" "linha vazia no comentário de uma chave: o bloco de registro continua válido"
# Controle negativo: uma chamada a mais no fim do arquivo é pega.
{ cat "$AUTO_SCRUM_SH"; printf '%s\n' "config_register_key LATE_ENABLED \"false\" \"\" 'LATE_ENABLED: tarde.'"; } > "$register_dir/late_key.sh"
assert_contains "$(register_block_problems "$register_dir/late_key.sh" "$registered_keys")" "chamada de registro depois da primeira função" "chave registrada no fim do arquivo: a checagem acusa"
skeleton_body="$(awk '/^print_project_skeleton\(\) \{/{found=1} found{print} found && /^\}/{exit}' "$AUTO_SCRUM_SH")"
assert_contains "$skeleton_body" "print_project_skeleton" "print_project_skeleton() existe"
assert_eq "" "$(grep -E '^[[:space:]]*[A-Z_]+=' <<< "$skeleton_body" || true)" "o corpo de print_project_skeleton não tem nenhuma linha CHAVE="

echo
echo "== auto_scrum: chave nova só no registro aparece no esqueleto e nos dois avisos =="
newkey_dir="$TEST_TMP/new_key"
mkdir -p "$newkey_dir"
newkey_setup="config_register_key NEW_X_ENABLED 'false' '' 'NEW_X_ENABLED: chave extra de teste.'"
newkey_skeleton="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $newkey_setup; print_project_skeleton" 2>/dev/null || true)"
assert_contains "$newkey_skeleton" $'# NEW_X_ENABLED: chave extra de teste.\nNEW_X_ENABLED="false"' "a chave extra aparece no esqueleto, com o comentário e NEW_X_ENABLED=\"false\""
config_keys_blank > "$newkey_dir/without.conf"
run_loader "$newkey_dir/without.conf" "$newkey_dir/without" "$newkey_setup" >/dev/null
assert_eq "$(missing_warning "$newkey_dir/without.conf" 'NEW_X_ENABLED="false"')" "$(cat "$newkey_dir/without.err" 2>/dev/null || true)" "a chave extra entra no aviso de chave ausente"
printf '%s\n' "$(config_keys_blank)" 'NEW_X_ENABLED="sim"' > "$newkey_dir/invalid.conf"
run_loader "$newkey_dir/invalid.conf" "$newkey_dir/invalid" "$newkey_setup" >/dev/null
assert_eq "$(value_warning "$newkey_dir/invalid.conf:7" NEW_X_ENABLED sim "$default_off_note")" "$(cat "$newkey_dir/invalid.err" 2>/dev/null || true)" "a chave extra com \"sim\" recebe o aviso de valor"

echo
echo "== menus: \"Sair (q)\" é o último número; o número, q ou Q encerram com código 0 =="
menu_quit_label="Sair (q)"
menu_quit_message="Saindo. Nada foi feito."
menu_invalid_message="Opção inválida, tente novamente."
menu_dir="$TEST_TMP/menu_quit"
mkdir -p "$menu_dir"

# run_choose_type_rc <setup> <stdin> <out_file>: como run_choose_type, mas grava
# stdout+stderr de choose_type em <out_file> e imprime "<código de saída>|<TYPE>". O TYPE só
# sai se choose_type retornou: quem sai pelo menu encerra o bash antes do printf. <stdin>
# vazio vira < /dev/null.
run_choose_type_rc() {
  local setup="$1" input="$2" out_file="$3" rc=0 type=""
  local cmd="source '$AUTO_SCRUM_SH'; $setup; choose_type >'$out_file' 2>&1; printf '%s' \"\$TYPE\""
  if [ -z "$input" ]; then
    type="$(timeout 2 bash -c "$cmd" < /dev/null 2>/dev/null)" || rc=$?
  else
    type="$(timeout 2 bash -c "$cmd" <<< "$input" 2>/dev/null)" || rc=$?
  fi
  printf '%s|%s' "$rc" "$type"
}

# count_in_file <file> <text>: imprime quantas vezes <text> aparece em <file>. Conta
# ocorrências, não linhas: sem terminal, o prompt "#? " e as mensagens do menu podem cair na
# mesma linha.
count_in_file() {
  local file="$1" text="$2"
  { grep -oF "$text" "$file" 2>/dev/null || true; } | grep -c . || true
}

# Menu principal (choose_type): as quatro combinações de JIRA_ENABLED/SENTRY_ENABLED, com os
# TYPEs na ordem do menu. Cada número antigo continua dando o mesmo TYPE, e o número seguinte
# ao último (o "Sair (q)"), q e Q encerram com código 0 sem TYPE.
ct_combos=(
  ":|po po_discussion tech_leader development review"
  "JIRA_ENABLED=true|po po_discussion po_jira tech_leader tech_leader_jira development review"
  "SENTRY_ENABLED=true|po po_discussion po_sentry tech_leader development review"
  "JIRA_ENABLED=true; SENTRY_ENABLED=true|po po_discussion po_jira po_sentry tech_leader tech_leader_jira development review"
)
for ct_combo in "${ct_combos[@]}"; do
  ct_setup="${ct_combo%%|*}"
  read -r -a ct_types <<< "${ct_combo#*|}"
  ct_n="${#ct_types[@]}"
  ct_quit_number="$((ct_n + 1))"
  for ct_index in "${!ct_types[@]}"; do
    assert_eq "${ct_types[$ct_index]}" "$(run_choose_type "$ct_setup" "$((ct_index + 1))")" "menu principal ($ct_setup): opção $((ct_index + 1)) continua sendo ${ct_types[$ct_index]}"
  done
  for ct_input in "$ct_quit_number" q Q; do
    ct_out="$menu_dir/choose_type_${ct_n}_$ct_input.log"
    assert_eq "0|" "$(run_choose_type_rc "$ct_setup" "$ct_input" "$ct_out")" "menu principal ($ct_setup), entrada '$ct_input': encerra com código 0 e sem TYPE"
    assert_contains "$(cat "$ct_out" 2>/dev/null || true)" "$menu_quit_message" "menu principal ($ct_setup), entrada '$ct_input': mostra a mensagem de saída"
  done
  ct_out="$menu_dir/choose_type_${ct_n}_$ct_quit_number.log"
  assert_contains "$(cat "$ct_out" 2>/dev/null || true)" "$ct_quit_number) $menu_quit_label" "menu principal ($ct_setup): \"$menu_quit_label\" é o item $ct_quit_number, logo depois das opções"
  assert_not_contains "$(cat "$ct_out" 2>/dev/null || true)" "$((ct_n + 2)))" "menu principal ($ct_setup): nada depois do \"$menu_quit_label\""
done

# Zero à esquerda: o select aceita "08" como 8, e o índice não pode vir de $((REPLY - 1)),
# que lê "08" como octal inválido. Com Jira e Sentry: 8 é review e 9 é o Sair.
ct_setup="JIRA_ENABLED=true; SENTRY_ENABLED=true"
assert_eq "review" "$(run_choose_type "$ct_setup" 08)" "menu principal: '08' é lido como 8 (review), sem erro aritmético"
assert_eq "0|" "$(run_choose_type_rc "$ct_setup" 09 "$menu_dir/choose_type_09.log")" "menu principal: '09' é lido como 9 (Sair), encerra com código 0"

# Entradas inválidas, sem flags (Sair é o 6): só q e Q exatos encerram; o resto repete a
# pergunta até uma opção válida.
ct_out="$menu_dir/choose_type_invalid.log"
assert_eq "0|po" "$(run_choose_type_rc ":" $'x\nqq\nsair\n q\n0\n7\n1' "$ct_out")" "menu principal: x, qq, sair, ' q', 0 e 7 são inválidas; o 1 seguinte dá po"
assert_eq "6" "$(count_in_file "$ct_out" "$menu_invalid_message")" "menu principal: cada entrada inválida mostra a mensagem de inválida (6 vezes)"
assert_not_contains "$(cat "$ct_out" 2>/dev/null || true)" "$menu_quit_message" "menu principal: nenhuma entrada inválida encerra pelo menu"

# Ctrl+D (regressão): stdin fechado, com ou sem entrada inválida antes, continua saindo com
# código 1 e sem mensagem de saída.
for ct_input in "" x; do
  ct_out="$menu_dir/choose_type_eof_${#ct_input}.log"
  assert_eq "1|" "$(run_choose_type_rc ":" "$ct_input" "$ct_out")" "menu principal, stdin fecha (entrada: '$ct_input' + EOF): choose_type retorna 1, sem TYPE"
  assert_not_contains "$(cat "$ct_out" 2>/dev/null || true)" "$menu_quit_message" "menu principal, stdin fecha (entrada: '$ct_input' + EOF): sem mensagem de saída"
done

# Menu de projetos (resolve_stack): com alpha.conf e beta.conf o menu é
# 1) alpha 2) beta 3) Nenhum 4) Sair (q). Sair não carrega projeto nenhum (sem aviso) e
# encerra antes de resolve_stack retornar, então o helper imprime vazio.
rs_quit_dir="$menu_dir/resolve_stack"
rs_quit_cwd="$rs_quit_dir/cwd"
rs_quit_projects="$rs_quit_dir/projects"
mkdir -p "$rs_quit_cwd" "$rs_quit_projects"
printf '%s\n' 'STACK_DESCRIPTION="stack-alpha"' > "$rs_quit_projects/alpha.conf"
printf '%s\n' 'STACK_DESCRIPTION="stack-beta"' > "$rs_quit_projects/beta.conf"
for rs_quit_input in 4 q Q; do
  rs_quit_result="$(run_resolve_stack "$rs_quit_cwd" "$rs_quit_projects" "" "$rs_quit_input" "$rs_quit_dir/quit_$rs_quit_input")"
  assert_eq "" "$rs_quit_result" "menu de projetos, entrada '$rs_quit_input': resolve_stack não retorna (o script encerra no menu)"
  assert_eq "0" "$(cat "$rs_quit_dir/quit_$rs_quit_input.rc")" "menu de projetos, entrada '$rs_quit_input': código 0"
  assert_contains "$(cat "$rs_quit_dir/quit_$rs_quit_input.out" 2>/dev/null || true)" "$menu_quit_message" "menu de projetos, entrada '$rs_quit_input': mostra a mensagem de saída no stdout"
  assert_eq "0" "$(count_warnings "$rs_quit_dir/quit_$rs_quit_input.err")" "menu de projetos, entrada '$rs_quit_input': nenhum projeto é carregado (sem aviso)"
done
assert_contains "$(cat "$rs_quit_dir/quit_4.err" 2>/dev/null || true)" "4) $menu_quit_label" "menu de projetos: \"$menu_quit_label\" é o item 4, depois do Nenhum"
assert_contains "$(cat "$rs_quit_dir/quit_4.err" 2>/dev/null || true)" "3) Nenhum" "menu de projetos: Nenhum continua sendo o item 3"

rs_quit_result="$(run_resolve_stack "$rs_quit_cwd" "$rs_quit_projects" "" "3" "$rs_quit_dir/none")"
assert_eq "||||" "$rs_quit_result" "menu de projetos: Nenhum (3) segue sem projeto, separado do Sair"
assert_eq "0" "$(cat "$rs_quit_dir/none.rc")" "menu de projetos: Nenhum (3) retorna com código 0"
assert_not_contains "$(cat "$rs_quit_dir/none.out" 2>/dev/null || true)" "$menu_quit_message" "menu de projetos: Nenhum (3) não mostra a mensagem de saída"

# Sem projetos: o menu é 1) Nenhum 2) Sair (q).
rs_quit_empty="$rs_quit_dir/projects_empty"
mkdir -p "$rs_quit_empty"
rs_quit_result="$(run_resolve_stack "$rs_quit_cwd" "$rs_quit_empty" "" "2" "$rs_quit_dir/empty_quit")"
assert_eq "" "$rs_quit_result" "menu de projetos sem projetos, entrada '2': resolve_stack não retorna"
assert_eq "0" "$(cat "$rs_quit_dir/empty_quit.rc")" "menu de projetos sem projetos, entrada '2': código 0"
assert_contains "$(cat "$rs_quit_dir/empty_quit.err" 2>/dev/null || true)" "2) $menu_quit_label" "menu de projetos sem projetos: \"$menu_quit_label\" é o item 2"
rs_quit_result="$(run_resolve_stack "$rs_quit_cwd" "$rs_quit_empty" "" "1" "$rs_quit_dir/empty_none")"
assert_eq "||||" "$rs_quit_result" "menu de projetos sem projetos: Nenhum (1) segue sem projeto"

# Inválidas: x, 5 (depois do Sair) e qq repetem a pergunta; o 3 seguinte é Nenhum.
rs_quit_result="$(run_resolve_stack "$rs_quit_cwd" "$rs_quit_projects" "" $'x\n5\nqq\n3' "$rs_quit_dir/invalid")"
assert_eq "||||" "$rs_quit_result" "menu de projetos: x, 5 e qq são inválidas; o 3 seguinte dá Nenhum"
assert_eq "3" "$(count_in_file "$rs_quit_dir/invalid.out" "$menu_invalid_message")" "menu de projetos: cada entrada inválida mostra a mensagem de inválida (3 vezes)"

# Ctrl+D (regressão): stdin fechado continua saindo com código 1 e sem mensagem de saída.
rs_quit_result="$(run_resolve_stack "$rs_quit_cwd" "$rs_quit_projects" "" "" "$rs_quit_dir/eof")"
assert_eq "" "$rs_quit_result" "menu de projetos, stdin fecha (EOF): resolve_stack não retorna valor"
assert_eq "1" "$(cat "$rs_quit_dir/eof.rc")" "menu de projetos, stdin fecha (EOF): código 1"
assert_not_contains "$(cat "$rs_quit_dir/eof.out" 2>/dev/null || true)" "$menu_quit_message" "menu de projetos, stdin fecha (EOF): sem mensagem de saída"

# Menu do --init (init_project): 1) diretório atual 2) projects/ 3) Sair (q). Sair encerra
# com código 0 sem criar o .nick.conf nem o PROJECTS_DIR.
init_quit_dir="$menu_dir/init_project"
for init_quit_input in 3 q Q; do
  init_quit_cwd="$init_quit_dir/cwd_$init_quit_input"
  init_quit_projects="$init_quit_dir/projects_$init_quit_input"
  mkdir -p "$init_quit_cwd"
  init_quit_rc="$(run_init_project "$init_quit_cwd" "$init_quit_projects" "$init_quit_input" "$init_quit_dir/quit_$init_quit_input.log")"
  assert_eq "0" "$init_quit_rc" "--init, entrada '$init_quit_input': init_project encerra com código 0"
  assert_contains "$(cat "$init_quit_dir/quit_$init_quit_input.log" 2>/dev/null || true)" "$menu_quit_message" "--init, entrada '$init_quit_input': mostra a mensagem de saída"
  assert_not_exists "$init_quit_cwd/.nick.conf" "--init, entrada '$init_quit_input': nenhum .nick.conf é criado no cwd"
  assert_not_exists "$init_quit_projects" "--init, entrada '$init_quit_input': PROJECTS_DIR não é criado"
done
assert_contains "$(cat "$init_quit_dir/quit_3.log" 2>/dev/null || true)" "3) $menu_quit_label" "--init: \"$menu_quit_label\" é o item 3, depois dos dois destinos"

# Inválidas seguidas de EOF: x, 4 (depois do Sair) e qq repetem a pergunta; o stdin fecha
# e o comportamento de hoje continua (código 1, cancelado, nada criado).
init_quit_cwd="$init_quit_dir/cwd_invalid"
init_quit_projects="$init_quit_dir/projects_invalid"
mkdir -p "$init_quit_cwd"
init_quit_rc="$(run_init_project "$init_quit_cwd" "$init_quit_projects" $'x\n4\nqq' "$init_quit_dir/invalid.log")"
assert_eq "1" "$init_quit_rc" "--init: x, 4 e qq são inválidas; o EOF seguinte sai com código 1"
assert_eq "3" "$(count_in_file "$init_quit_dir/invalid.log" "$menu_invalid_message")" "--init: cada entrada inválida mostra a mensagem de inválida (3 vezes)"
assert_not_contains "$(cat "$init_quit_dir/invalid.log" 2>/dev/null || true)" "$menu_quit_message" "--init: nenhuma entrada inválida encerra pelo menu"
assert_not_exists "$init_quit_cwd/.nick.conf" "--init: entradas inválidas + EOF não criam .nick.conf"

echo
echo "== main(): sair por qualquer menu não cria log, não chama o claude e não cria configuração =="
# claude falso: só toca um marcador. Prova que o exec claude não acontece ao sair pelo menu
# — e o controle positivo abaixo prova que, quando o envio é confirmado, o stub é chamado.
fake_claude_dir="$TEST_TMP/fake_claude"
fake_claude_marker="$fake_claude_dir/called"
mkdir -p "$fake_claude_dir"
printf '%s\n' '#!/usr/bin/env bash' "touch '$fake_claude_marker'" > "$fake_claude_dir/claude"
chmod +x "$fake_claude_dir/claude"
fake_claude_setup="PATH='$fake_claude_dir':\"\$PATH\""

# Controle positivo: po_discussion (2), assunto e s pra confirmar — o marcador tem de existir.
mq_main_dir="$menu_dir/main_positive"
rm -f "$fake_claude_marker"
mq_rc="$(run_main "$mq_main_dir" "$(config_keys_blank)" $'2\nassunto de teste\ns' "$fake_claude_setup")"
assert_eq "0" "$mq_rc" "controle positivo: confirmar o envio termina com código 0 (claude falso)"
assert_exists "$fake_claude_marker" "controle positivo: confirmar o envio chama o claude (marcador criado pelo stub)"

# Menu principal: flags desligadas, 5 opções, então o Sair é o 6.
for mq_input in 6 q Q; do
  mq_main_dir="$menu_dir/main_menu_$mq_input"
  rm -f "$fake_claude_marker"
  mq_rc="$(run_main "$mq_main_dir" "$(config_keys_blank)" "$mq_input" "$fake_claude_setup")"
  mq_output="$(cat "$mq_main_dir/output" 2>/dev/null || true)"
  assert_eq "0" "$mq_rc" "menu principal, entrada '$mq_input': main() termina com código 0"
  assert_contains "$mq_output" "$menu_quit_message" "menu principal, entrada '$mq_input': mostra a mensagem de saída"
  assert_not_exists "$mq_main_dir/logs" "menu principal, entrada '$mq_input': nenhum log é criado"
  assert_not_exists "$fake_claude_marker" "menu principal, entrada '$mq_input': o claude não é chamado"
  assert_not_contains "$mq_output" "Confirma o envio" "menu principal, entrada '$mq_input': não chega à confirmação de envio"
done

# Menu de projetos: cwd sem .nick.conf e projects/alpha.conf, então o menu é
# 1) alpha 2) Nenhum 3) Sair (q).
for mq_input in 3 q Q; do
  mq_proj_dir="$menu_dir/projects_menu_$mq_input"
  mkdir -p "$mq_proj_dir/cwd" "$mq_proj_dir/projects"
  printf '%s\n' 'STACK_DESCRIPTION="x"' > "$mq_proj_dir/projects/alpha.conf"
  rm -f "$fake_claude_marker"
  mq_rc=0
  timeout 5 bash -c "cd '$mq_proj_dir/cwd' || exit 99; source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$mq_proj_dir/logs'; PROJECTS_DIR='$mq_proj_dir/projects'; $fake_claude_setup; main" <<< "$mq_input" > "$mq_proj_dir/output" 2>&1 || mq_rc=$?
  mq_output="$(cat "$mq_proj_dir/output" 2>/dev/null || true)"
  assert_eq "0" "$mq_rc" "menu de projetos, entrada '$mq_input': main() termina com código 0"
  assert_contains "$mq_output" "$menu_quit_message" "menu de projetos, entrada '$mq_input': mostra a mensagem de saída"
  assert_not_contains "$mq_output" "Qual tipo de conversa" "menu de projetos, entrada '$mq_input': não chega ao menu principal"
  assert_not_exists "$mq_proj_dir/logs" "menu de projetos, entrada '$mq_input': nenhum log é criado"
  assert_not_exists "$fake_claude_marker" "menu de projetos, entrada '$mq_input': o claude não é chamado"
done

# --init: cwd vazio e PROJECTS_DIR inexistente; o menu é 1) cwd 2) projects/ 3) Sair (q).
for mq_input in 3 q Q; do
  mq_init_dir="$menu_dir/init_menu_$mq_input"
  mkdir -p "$mq_init_dir/cwd"
  rm -f "$fake_claude_marker"
  mq_rc=0
  timeout 5 bash -c "cd '$mq_init_dir/cwd' || exit 99; source '$AUTO_SCRUM_SH'; check_requirements() { :; }; LOGS_DIR='$mq_init_dir/logs'; PROJECTS_DIR='$mq_init_dir/projects'; $fake_claude_setup; main --init" <<< "$mq_input" > "$mq_init_dir/output" 2>&1 || mq_rc=$?
  mq_output="$(cat "$mq_init_dir/output" 2>/dev/null || true)"
  assert_eq "0" "$mq_rc" "--init, entrada '$mq_input': main --init termina com código 0"
  assert_contains "$mq_output" "$menu_quit_message" "--init, entrada '$mq_input': mostra a mensagem de saída"
  assert_not_exists "$mq_init_dir/cwd/.nick.conf" "--init, entrada '$mq_input': nenhum .nick.conf é criado"
  assert_not_exists "$mq_init_dir/projects" "--init, entrada '$mq_input': PROJECTS_DIR não é criado"
  assert_not_exists "$mq_init_dir/logs" "--init, entrada '$mq_input': nenhum log é criado"
  assert_not_exists "$fake_claude_marker" "--init, entrada '$mq_input': o claude não é chamado"
done
rm -f "$fake_claude_marker"

echo
echo "== auto_scrum.sh: um select só, dentro de select_menu (todo menu ganha o Sair) =="
# Todo menu passa por select_menu, que acrescenta o "Sair (q)": um select direto em outra
# função seria um menu sem saída. A âncora aceita "select" no começo da linha ou depois de
# ";", pra um select escondido na mesma linha de outro comando não escapar.
select_lines="$(grep -nE '(^|;)[[:space:]]*select[[:space:]]' "$AUTO_SCRUM_SH" || true)"
assert_eq "1" "$(grep -c . <<< "$select_lines" || true)" "auto_scrum.sh tem exatamente uma linha select"
select_menu_body="$(awk '/^select_menu\(\) \{/{found=1} found{print} found && /^\}/{exit}' "$AUTO_SCRUM_SH")"
assert_contains "$select_menu_body" "select " "a única linha select fica dentro de select_menu()"
# A saída pelo menu é um exit: dentro de $(...) ele só encerraria o subshell, o script
# seguiria e a mensagem de saída seria engolida pela captura. Nenhum menu (nem função que
# mostra um) pode ser chamado assim.
menu_in_subshell="$(grep -nE '\$\((select_menu|choose_type|resolve_stack|init_project)\b' "$AUTO_SCRUM_SH" || true)"
assert_eq "" "$menu_in_subshell" "nenhum menu é chamado dentro de \$(...)"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
