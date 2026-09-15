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

# assert_eq <expected> <actual> <description>
assert_eq() {
  local expected="$1" actual="$2" description="$3"
  if [ "$expected" = "$actual" ]; then
    echo "  OK: $description"
  else
    echo "  FAIL: $description (esperado: '$expected', obtido: '$actual')"
    failures=$((failures + 1))
  fi
}

# assert_contains <haystack> <needle> <description>
assert_contains() {
  local haystack="$1" needle="$2" description="$3"
  case "$haystack" in
    *"$needle"*)
      echo "  OK: $description"
      ;;
    *)
      echo "  FAIL: $description (esperado conter: '$needle')"
      failures=$((failures + 1))
      ;;
  esac
}

# assert_not_contains <haystack> <needle> <description>
assert_not_contains() {
  local haystack="$1" needle="$2" description="$3"
  case "$haystack" in
    *"$needle"*)
      echo "  FAIL: $description (não deveria conter: '$needle')"
      failures=$((failures + 1))
      ;;
    *)
      echo "  OK: $description"
      ;;
  esac
}

# assert_exists <path> <description>
assert_exists() {
  local path="$1" description="$2"
  if [ -e "$path" ]; then
    echo "  OK: $description"
  else
    echo "  FAIL: $description (não existe: '$path')"
    failures=$((failures + 1))
  fi
}

# assert_not_exists <path> <description>
assert_not_exists() {
  local path="$1" description="$2"
  if [ -e "$path" ]; then
    echo "  FAIL: $description (não deveria existir: '$path')"
    failures=$((failures + 1))
  else
    echo "  OK: $description"
  fi
}

# run_loader <config_file> <out_prefix> [setup]: dá source no auto_scrum.sh num bash
# separado, roda [setup] (padrão: nada), chama load_project_config <config_file> com
# stdout em <out_prefix>.out e stderr em <out_prefix>.err e imprime as 4 chaves conhecidas
# separadas por '|'. O '|| true' mantém a suíte rodando (e reportando FAIL) mesmo se o
# bash de dentro morrer, ex: função ainda inexistente.
run_loader() {
  local config_file="$1" out_prefix="$2" setup="${3:-:}"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $setup; load_project_config '$config_file' >'$out_prefix.out' 2>'$out_prefix.err'; printf '%s|%s|%s|%s' \"\$STACK_DESCRIPTION\" \"\$SENTRY_ENABLED\" \"\$PERMISSION_ENABLED\" \"\$JIRA_ENABLED\"" 2>/dev/null || true
}

# run_resolve_stack <cwd> <projects_dir> <project_arg> <stdin> <out_prefix>: num bash
# separado, faz cd pra <cwd> ANTES do source (resolve_stack procura config no cwd), aponta
# PROJECTS_DIR pra <projects_dir> e roda resolve_stack <project_arg>, com stdout em
# <out_prefix>.out, stderr em <out_prefix>.err e o código de saída em <out_prefix>.rc.
# <stdin> vazio vira < /dev/null. Imprime as 4 chaves conhecidas separadas por '|'.
run_resolve_stack() {
  local cwd="$1" projects_dir="$2" project_arg="$3" input="$4" out_prefix="$5" rc=0 vars=""
  local cmd="cd '$cwd' || exit 99; source '$AUTO_SCRUM_SH'; PROJECTS_DIR='$projects_dir'; resolve_stack '$project_arg' >'$out_prefix.out' 2>'$out_prefix.err'; printf '%s|%s|%s|%s' \"\$STACK_DESCRIPTION\" \"\$SENTRY_ENABLED\" \"\$PERMISSION_ENABLED\" \"\$JIRA_ENABLED\""
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
assert_not_exists "$init_projects_unused" "destino cwd: PROJECTS_DIR não é criado"

init_rc="$(run_init_project "$init_projects_dest" "$init_projects" $'2\nprojeto-teste' "$init_dir/projects_dest.log")"
init_projects_file="$init_projects/projeto-teste.conf"
assert_eq "0" "$init_rc" "destino projects/: init_project termina com código 0"
assert_exists "$init_projects_file" "destino projects/ (opção 2): cria \$PROJECTS_DIR/projeto-teste.conf"
assert_contains "$(cat "$init_projects_file" 2>/dev/null)" 'JIRA_ENABLED="false"' "destino projects/: esqueleto declara JIRA_ENABLED=\"false\" por padrão"
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
assert_eq "|false|false|false" "$skeleton_result" "esqueleto carregado pelo leitor: flags false e STACK_DESCRIPTION vazio"
assert_eq "" "$(cat "$init_dir/skeleton_loader.err" 2>/dev/null || echo 'sem stderr')" "esqueleto carregado pelo leitor não gera aviso (nem pelos comentários que citam CHAVE=\"valor\")"

skeleton_keys="$(grep -oE '^[A-Z_]+=' "$init_cwd_file" 2>/dev/null | tr -d '=' | sort || true)"
config_keys="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; printf '%s\n' \"\${PROJECT_CONFIG_KEYS[@]}\" | sort")"
assert_eq "$config_keys" "$skeleton_keys" "chaves atribuídas no esqueleto são exatamente as de PROJECT_CONFIG_KEYS"

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
tl_rules_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; build_tech_leader_blocks; printf '%s' \"\$TECH_LEADER_PLAN_RULES_BLOCK\"")"
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
  '# Config de exemplo, no formato do esqueleto.' \
  '# Exemplo: STACK_DESCRIPTION="não deve ser lido"' \
  'STACK_DESCRIPTION="Bash (POSIX [ ], set -euo pipefail), pytest e TDD"' \
  '' \
  'SENTRY_ENABLED="true"' \
  'PERMISSION_ENABLED="false"' \
  'JIRA_ENABLED="true"' > "$lpc_dir/skeleton.conf"
lpc_result="$(run_loader "$lpc_dir/skeleton.conf" "$lpc_dir/skeleton")"
assert_eq "Bash (POSIX [ ], set -euo pipefail), pytest e TDD|true|false|true" "$lpc_result" "formato do esqueleto: as 4 chaves carregam com o valor do arquivo (inclusive [ ], vírgula e parênteses)"
assert_eq "" "$(cat "$lpc_dir/skeleton.err" 2>/dev/null || echo 'sem stderr')" "formato do esqueleto: nenhum aviso no stderr (nem pela linha '# Exemplo: STACK_DESCRIPTION=...')"

printf '%s\n' 'echo executou' "touch \"$lpc_dir/marker1\"" > "$lpc_dir/commands.conf"
run_loader "$lpc_dir/commands.conf" "$lpc_dir/commands" >/dev/null
assert_not_contains "$(cat "$lpc_dir/commands.out" "$lpc_dir/commands.err" 2>/dev/null)" "executou" "linha 'echo executou' não é executada"
assert_not_exists "$lpc_dir/marker1" "linha 'touch <marker>' não é executada"

printf '%s\n' "STACK_DESCRIPTION=\"\$(touch $lpc_dir/marker2)\"" > "$lpc_dir/subst.conf"
lpc_result="$(run_loader "$lpc_dir/subst.conf" "$lpc_dir/subst")"
assert_not_exists "$lpc_dir/marker2" "\$(...) dentro do valor não é executado"
assert_eq "\$(touch $lpc_dir/marker2)|||" "$lpc_result" "\$(...) dentro do valor vira texto literal em STACK_DESCRIPTION"

printf '%s\n' '# linha 1' "JIRA_ENABLED=\"true\" && touch \"$lpc_dir/marker3\"" > "$lpc_dir/hybrid.conf"
lpc_result="$(run_loader "$lpc_dir/hybrid.conf" "$lpc_dir/hybrid")"
assert_not_exists "$lpc_dir/marker3" "linha 'CHAVE=\"x\" && touch <marker>' não executa o comando"
assert_eq "|||" "$lpc_result" "linha 'CHAVE=\"x\" && touch <marker>' não altera JIRA_ENABLED"
assert_contains "$(cat "$lpc_dir/hybrid.err" 2>/dev/null)" "Aviso: $lpc_dir/hybrid.conf:2: linha de JIRA_ENABLED ignorada" "linha híbrida gera aviso no stderr com arquivo e número da linha"

printf '%s\n' 'SENTRY_ENABLED=true' > "$lpc_dir/unquoted.conf"
lpc_result="$(run_loader "$lpc_dir/unquoted.conf" "$lpc_dir/unquoted")"
assert_eq "|||" "$lpc_result" "valor sem aspas (SENTRY_ENABLED=true) é ignorado"
assert_contains "$(cat "$lpc_dir/unquoted.err" 2>/dev/null)" "Aviso: $lpc_dir/unquoted.conf:1: linha de SENTRY_ENABLED ignorada (formato esperado: SENTRY_ENABLED=\"valor\", sem aspas duplas dentro do valor)" "valor sem aspas gera aviso no stderr"

printf '%s\n' 'FOO="bar"' 'TYPE="review"' 'PATH="/nao/existe"' > "$lpc_dir/unknown.conf"
unknown_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; load_project_config '$lpc_dir/unknown.conf' 2>'$lpc_dir/unknown.err'; printf '%s|' \"\$TYPE\"; command -v ls >/dev/null && printf 'ls ok'" 2>/dev/null || true)"
assert_eq "|ls ok" "$unknown_result" "chaves desconhecidas (FOO, TYPE, PATH) não alteram variáveis nem quebram a execução"
assert_eq "" "$(cat "$lpc_dir/unknown.err" 2>/dev/null || echo 'sem stderr')" "chaves desconhecidas não geram aviso"

printf '%s\n' '  PERMISSION_ENABLED="true"' > "$lpc_dir/indented.conf"
lpc_result="$(run_loader "$lpc_dir/indented.conf" "$lpc_dir/indented")"
assert_eq "||true|" "$lpc_result" "linha com espaços no início é aceita"

printf 'JIRA_ENABLED="true"\r' > "$lpc_dir/crlf.conf"
lpc_result="$(run_loader "$lpc_dir/crlf.conf" "$lpc_dir/crlf")"
assert_eq "|||true" "$lpc_result" "última linha com CRLF e sem \\n final é aceita, sem o \\r no valor"

printf '%s\n' 'STACK_DESCRIPTION_EXTRA="x"' > "$lpc_dir/prefix.conf"
lpc_result="$(run_loader "$lpc_dir/prefix.conf" "$lpc_dir/prefix")"
assert_eq "|||" "$lpc_result" "chave com prefixo parecido (STACK_DESCRIPTION_EXTRA) é ignorada"
assert_eq "" "$(cat "$lpc_dir/prefix.err" 2>/dev/null || echo 'sem stderr')" "chave com prefixo parecido não gera aviso"

printf '%s\n' 'STACK_DESCRIPTION=""' > "$lpc_dir/empty.conf"
lpc_result="$(run_loader "$lpc_dir/empty.conf" "$lpc_dir/empty" "STACK_DESCRIPTION=antes")"
assert_eq "|||" "$lpc_result" "CHAVE=\"\" atribui valor vazio"
assert_eq "" "$(cat "$lpc_dir/empty.err" 2>/dev/null || echo 'sem stderr')" "CHAVE=\"\" não gera aviso"

# Formas "quase certas" de chave conhecida: continuam não aceitas, mas avisam (uma vez só
# por linha), senão a flag ficaria desligada sem explicação.
lpc_near_case=0
for lpc_near_line in 'export JIRA_ENABLED="true"' 'SENTRY_ENABLED = "true"' 'PERMISSION_ENABLED ="true"'; do
  lpc_near_case=$((lpc_near_case + 1))
  lpc_near_key="${lpc_near_line#export }"
  lpc_near_key="${lpc_near_key%%[ =]*}"
  printf '%s\n' '# linha 1' "$lpc_near_line" > "$lpc_dir/near_$lpc_near_case.conf"
  lpc_result="$(run_loader "$lpc_dir/near_$lpc_near_case.conf" "$lpc_dir/near_$lpc_near_case")"
  lpc_near_err="$(cat "$lpc_dir/near_$lpc_near_case.err" 2>/dev/null || true)"
  assert_eq "|||" "$lpc_result" "linha '$lpc_near_line' não altera $lpc_near_key"
  assert_contains "$lpc_near_err" "Aviso: $lpc_dir/near_$lpc_near_case.conf:2: linha de $lpc_near_key ignorada" "linha '$lpc_near_line' gera aviso no stderr com arquivo e número da linha"
  assert_eq "1" "$(grep -c 'Aviso:' "$lpc_dir/near_$lpc_near_case.err" 2>/dev/null || true)" "linha '$lpc_near_line' gera um aviso só"
done

printf '%s\n' 'STACK_DESCRIPTION_EXTRA = "x"' > "$lpc_dir/prefix_spaced.conf"
lpc_result="$(run_loader "$lpc_dir/prefix_spaced.conf" "$lpc_dir/prefix_spaced")"
assert_eq "|||" "$lpc_result" "chave com prefixo parecido e espaço em volta do = (STACK_DESCRIPTION_EXTRA = \"x\") é ignorada"
assert_eq "" "$(cat "$lpc_dir/prefix_spaced.err" 2>/dev/null || echo 'sem stderr')" "chave com prefixo parecido e espaço em volta do = não gera aviso"

echo
echo "== resolve_stack: projects/ usa extensão .conf e é lido por load_project_config (sem source) =="
rs_dir="$TEST_TMP/resolve_stack_conf"
rs_cwd="$rs_dir/cwd"
rs_projects="$rs_dir/projects"
mkdir -p "$rs_cwd" "$rs_projects"
printf '%s\n' '# alpha' 'STACK_DESCRIPTION="stack-alpha"' 'SENTRY_ENABLED="true"' 'PERMISSION_ENABLED="false"' 'JIRA_ENABLED="true"' > "$rs_projects/alpha.conf"
printf '%s\n' '# beta' 'STACK_DESCRIPTION="stack-beta"' 'SENTRY_ENABLED="false"' 'PERMISSION_ENABLED="true"' 'JIRA_ENABLED="false"' > "$rs_projects/beta.conf"

rs_result="$(run_resolve_stack "$rs_cwd" "$rs_projects" "alpha" "" "$rs_dir/by_arg")"
assert_eq "stack-alpha|true|false|true" "$rs_result" "--projeto=alpha carrega os valores de alpha.conf"

rs_result="$(run_resolve_stack "$rs_cwd" "$rs_projects" "" "1" "$rs_dir/menu")"
assert_eq "stack-alpha|true|false|true" "$rs_result" "menu: opção 1 carrega alpha.conf (glob em ordem alfabética)"
assert_contains "$(cat "$rs_dir/menu.err")" "1) alpha" "menu lista alpha (nome vindo de basename .conf)"
assert_contains "$(cat "$rs_dir/menu.err")" "2) beta" "menu lista beta (nome vindo de basename .conf)"

rs_result="$(run_resolve_stack "$rs_cwd" "$rs_projects" "" "3" "$rs_dir/none")"
assert_eq "|||" "$rs_result" "menu: opção Nenhum (último número) deixa STACK_DESCRIPTION vazio"

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
assert_eq "stack-alpha|||" "$legacy_result" "menu com gamma.sh legado: opção 1 continua sendo alpha"
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
assert_eq "stack-do-cwd|||true" "$cwd_result" "com .nick.conf no cwd, as variáveis vêm dele"
assert_contains "$(cat "$cwd_dir/load.out")" "Configuração do projeto carregada de: $cwd_with/.nick.conf" "com .nick.conf no cwd, stdout mostra de onde a configuração foi carregada"
assert_not_contains "$cwd_output" "Qual projeto" "com .nick.conf no cwd, o menu de projetos não aparece"
assert_not_contains "$cwd_output" "alpha" "com .nick.conf no cwd, os projetos de projects/ não são listados"
assert_not_contains "$cwd_output" "não é mais lido" "com .nick.conf no cwd, não aparece aviso de .sh legado"

cwd_result="$(run_resolve_stack "$cwd_with" "$cwd_projects" "alpha" "" "$cwd_dir/arg_existing")"
assert_eq "stack-do-cwd|||true" "$cwd_result" "com .nick.conf no cwd, --projeto=alpha é ignorado e valem os valores do cwd"
assert_contains "$(cat "$cwd_dir/arg_existing.err")" "Aviso: --projeto=alpha ignorado — a configuração do diretório atual ($cwd_with/.nick.conf) tem precedência." "com .nick.conf no cwd, stderr avisa que --projeto=alpha foi ignorado"

cwd_result="$(run_resolve_stack "$cwd_with" "$cwd_projects" "inexistente" "" "$cwd_dir/arg_missing")"
assert_eq "0" "$(cat "$cwd_dir/arg_missing.rc")" "com .nick.conf no cwd, --projeto=inexistente não dá exit 1"
assert_eq "stack-do-cwd|||true" "$cwd_result" "com .nick.conf no cwd, --projeto=inexistente usa os valores do cwd"
assert_not_contains "$(cat "$cwd_dir/arg_missing.out" "$cwd_dir/arg_missing.err")" "não encontrado" "com .nick.conf no cwd, --projeto=inexistente não mostra erro de projeto não encontrado"
assert_contains "$(cat "$cwd_dir/arg_missing.err")" "Aviso: --projeto=inexistente ignorado" "com .nick.conf no cwd, stderr avisa que --projeto=inexistente foi ignorado"

cwd_result="$(run_resolve_stack "$cwd_without" "$cwd_projects" "" "1" "$cwd_dir/fallback_menu")"
cwd_output="$(cat "$cwd_dir/fallback_menu.out" "$cwd_dir/fallback_menu.err")"
assert_eq "stack-alpha|true||" "$cwd_result" "sem .nick.conf no cwd, menu opção 1 carrega alpha.conf"
assert_contains "$cwd_output" "Qual projeto" "sem .nick.conf no cwd, o menu de projetos aparece"
assert_not_contains "$cwd_output" "Configuração do projeto carregada de" "sem .nick.conf no cwd, não aparece a linha 'carregada de'"

cwd_result="$(run_resolve_stack "$cwd_without" "$cwd_projects" "alpha" "" "$cwd_dir/fallback_arg")"
cwd_output="$(cat "$cwd_dir/fallback_arg.out" "$cwd_dir/fallback_arg.err")"
assert_eq "stack-alpha|true||" "$cwd_result" "sem .nick.conf no cwd, --projeto=alpha carrega alpha.conf"
assert_not_contains "$cwd_output" "ignorado" "sem .nick.conf no cwd, --projeto=alpha não gera aviso de flag ignorada"
assert_not_contains "$cwd_output" "Configuração do projeto carregada de" "sem .nick.conf no cwd, --projeto=alpha não imprime a linha 'carregada de'"

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

# po_validation_block <setup>: PO_VALIDATION_BLOCK real, montado na mesma ordem de main().
po_validation_block() {
  local setup="$1"
  timeout 2 bash -c "source '$AUTO_SCRUM_SH'; $setup; build_sentry_blocks; build_permission_blocks; build_po_blocks; printf '%s' \"\$PO_VALIDATION_BLOCK\"" 2>/dev/null || true
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
echo "== auto_scrum.sh: nenhuma linha source/. (config lida só por load_project_config) =="
source_lines="$(grep -nE '^[[:space:]]*(source|\.)[[:space:]]' "$AUTO_SCRUM_SH" || true)"
assert_eq "" "$source_lines" "auto_scrum.sh não tem nenhuma linha source/."

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
