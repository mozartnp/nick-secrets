#!/usr/bin/env bash
#
# test_auto_scrum: testes em bash puro (sem framework) pro auto_scrum.sh. Dá source no
# script (que precisa do guard BASH_SOURCE[0] = $0 pra não disparar main() sozinho) e
# roda asserts simples contra as funções expostas.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AUTO_SCRUM_SH="$SCRIPT_DIR/../auto_scrum.sh"

failures=0

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
echo "== init_project: skeleton de projeto novo inclui JIRA_ENABLED (opt-in, padrão false) =="
init_skeleton_result="$(timeout 2 bash -c "source '$AUTO_SCRUM_SH'; PROJECTS_DIR=\"\$(mktemp -d)\"; printf 'projeto-teste\n' | init_project >/dev/null 2>&1; cat \"\$PROJECTS_DIR/projeto-teste.sh\"")"
assert_contains "$init_skeleton_result" 'JIRA_ENABLED="false"' "skeleton de init_project() declara JIRA_ENABLED=\"false\" por padrão"

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
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
