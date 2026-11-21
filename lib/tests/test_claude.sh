#!/usr/bin/env bash
#
# test_claude: testes em bash puro (sem framework) da biblioteca lib/claude/claude.sh e do
# arquivo de regras lib/claude/permissions.json. Cada caso roda num bash -c separado, que dá
# source só em lib/claude/claude.sh — nunca em lib/config/config.sh nem em lib/jail/jail.sh, o
# que prova que as bibliotecas são independentes. A validação do JSON usa jq e/ou python3 (só
# aqui, nos testes: a biblioteca não depende de nenhum dos dois). A integração com o auto_scrum
# (o argv do claude) fica em auto_scrum/tests/test_auto_scrum.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CLAUDE_SH="$LIB_DIR/claude/claude.sh"
PERMISSIONS_JSON="$LIB_DIR/claude/permissions.json"
AUTO_SCRUM_SH="$LIB_DIR/../auto_scrum/auto_scrum.sh"

failures=0

# Diretório-base temporário único da suíte: cada teste usa um subdiretório dele, e nada é
# escrito dentro do repositório.
TEST_TMP="$(mktemp -d)"
trap 'chmod -R u+rwx "$TEST_TMP" 2>/dev/null || true; rm -rf "$TEST_TMP"' EXIT

# shellcheck source=SCRIPTDIR/assert.sh
source "$SCRIPT_DIR/assert.sh"

# Variáveis que o próprio bash cria ou mexe entre dois retratos do compgen -v (não são da
# biblioteca): um padrão só, usado pelo retrato da claude_conversation_latest e pela carga limpa.
clean_bash_vars='_|BASH_.*|COLUMNS|LINES|FUNCNAME|LINENO|PIPESTATUS'

# code_lines <file>: imprime as linhas de <file> que não são comentário de linha inteira.
code_lines() {
  grep -vE '^[[:space:]]*#' "$1" || true
}

# file_of <path>: imprime o conteúdo de <path>, ou "sem arquivo" se ele não existir.
file_of() {
  cat "$1" 2>/dev/null || printf 'sem arquivo'
}

# new_case <name>: cria (vazio) e imprime o diretório de um caso, com o subdiretório cwd/.
new_case() {
  local dir="$TEST_TMP/$1"
  rm -rf "$dir"
  mkdir -p "$dir/cwd"
  printf '%s' "$dir"
}

# run_claude <case_dir> <commands> [env] [errexit]: num bash separado, com cwd em
# <case_dir>/cwd, roda [env] (atribuições, antes do source), liga set -euo pipefail (a menos
# que [errexit] seja "off"), dá source só em lib/claude/claude.sh e roda <commands>. stdout em
# <case_dir>/out, stderr em <case_dir>/err e o código de saída em <case_dir>/rc. O
# '|| t_rc=$?' mantém a suíte rodando (e reportando FAIL) mesmo se o bash de dentro morrer.
run_claude() {
  local case_dir="$1" commands="$2" env="${3:-:}" errexit="${4:-on}" t_rc=0 strict="set -euo pipefail"
  [ "$errexit" = "off" ] && strict="set +euo pipefail"
  timeout 5 bash -c "cd '$case_dir/cwd' || exit 99; $env; $strict; source '$CLAUDE_SH'; $commands" < /dev/null > "$case_dir/out" 2> "$case_dir/err" || t_rc=$?
  printf '%s' "$t_rc" > "$case_dir/rc"
}

# As regras obrigatórias da política inicial (decisão 8 do plano do ticket 21), conferidas
# pela string exata. São dado de teste: a fonte única das regras é o permissions.json.
REQUIRED_DENY=(
  # Privilégio
  'Bash(sudo *)' 'Bash(su)' 'Bash(su *)' 'Bash(doas *)'
  # Destruição em massa
  'Bash(rm -rf *)' 'Bash(rm -fr *)' 'Bash(rm -Rf *)' 'Bash(rm -fR *)' 'Bash(rm -r -f *)'
  'Bash(rm -f -r *)' 'Bash(rm --recursive --force *)' 'Bash(rm --force --recursive *)'
  # Reescrita do remote
  'Bash(git push --force*)' 'Bash(git push * --force*)' 'Bash(git push -f*)'
  'Bash(git push * -f)' 'Bash(git push * -f *)'
  # Segredos fora do projeto
  'Read(~/.ssh/**)' 'Read(~/.aws/**)' 'Read(~/.gnupg/**)'
)
REQUIRED_ASK=(
  # Versionamento
  'Bash(git add *)' 'Bash(git commit *)' 'Bash(git commit)' 'Bash(git push)' 'Bash(git push *)'
  # Perda de trabalho local
  'Bash(git reset --hard*)' 'Bash(git reset * --hard*)' 'Bash(git clean *)'
  'Bash(git checkout -- *)' 'Bash(git restore *)' 'Bash(git branch -D *)'
  # Segredos do projeto
  'Read(**/.env)' 'Read(**/.env.*)'
)

# Validadores de JSON disponíveis nesta máquina. Cada um disponível valida o arquivo (os dois,
# se houver os dois); sem nenhum, a validação é FAIL, não PULADO: ela é a única defesa contra
# uma regra ignorada em silêncio pelo claude.
JSON_TOOLS=()
command -v jq > /dev/null 2>&1 && JSON_TOOLS+=(jq)
command -v python3 > /dev/null 2>&1 && JSON_TOOLS+=(python3)

# json_valid <tool> <file>: código 0 se <file> é JSON válido, segundo <tool>.
json_valid() {
  case "$1" in
    jq) jq -e . "$2" > /dev/null 2>&1 ;;
    python3) python3 -m json.tool "$2" > /dev/null 2>&1 ;;
  esac
}

# json_shape_ok <tool> <file>: código 0 se <file> é exatamente {"permissions": {"deny": [...],
# "ask": [...]}, "disableAgentView": true}, com as duas listas de strings e nenhuma outra chave.
# O disableAgentView entrou no NS-50: desliga o segundo plano do claude (/background, o ← no
# prompt vazio), que faz a conversa continuar noutra sessão (ABOUT.md, seção do NS-50).
json_shape_ok() {
  case "$1" in
    jq)
      jq -e '(type == "object") and (keys == ["disableAgentView", "permissions"])
        and (.disableAgentView == true)
        and (.permissions | type == "object") and (.permissions | keys == ["ask", "deny"])
        and (.permissions.deny | type == "array") and (.permissions.ask | type == "array")
        and all(.permissions.deny[], .permissions.ask[]; type == "string")' "$2" > /dev/null 2>&1
      ;;
    python3)
      python3 -I -c '
import json, sys
d = json.load(open(sys.argv[1]))
p = d.get("permissions") if isinstance(d, dict) else None
ok = (isinstance(d, dict) and set(d) == {"permissions", "disableAgentView"} and isinstance(p, dict)
      and d["disableAgentView"] is True
      and set(p) == {"deny", "ask"}
      and all(isinstance(p[k], list) and all(isinstance(r, str) for r in p[k]) for k in p))
sys.exit(0 if ok else 1)' "$2" > /dev/null 2>&1
      ;;
  esac
}

# json_rules <tool> <file> <list>: imprime as regras de permissions.<list>, uma por linha.
json_rules() {
  case "$1" in
    jq) jq -r --arg l "$3" '.permissions[$l][]' "$2" 2>/dev/null || true ;;
    python3) python3 -I -c 'import json, sys; print("\n".join(json.load(open(sys.argv[1]))["permissions"][sys.argv[2]]))' "$2" "$3" 2>/dev/null || true ;;
  esac
}

echo "== permissions.json: existe, é JSON válido e tem só permissions.deny/permissions.ask e disableAgentView =="
assert_exists "$PERMISSIONS_JSON" "lib/claude/permissions.json existe"
if [ "${#JSON_TOOLS[@]}" -eq 0 ]; then
  echo "  FAIL: nenhum validador de JSON (instale jq ou python3)"
  failures=$((failures + 1))
fi
for json_tool in "${JSON_TOOLS[@]}"; do
  assert_eq "0" "$(json_valid "$json_tool" "$PERMISSIONS_JSON" && echo 0 || echo 1)" "$json_tool: permissions.json é JSON válido"
  assert_eq "0" "$(json_shape_ok "$json_tool" "$PERMISSIONS_JSON" && echo 0 || echo 1)" "$json_tool: só permissions.deny e permissions.ask (arrays de strings) e disableAgentView true, sem outras chaves"
done

echo
echo "== validadores do teste: recusam JSON inválido e formato com chave a mais (controle) =="
bad_dir="$(new_case bad_json)"
printf '%s\n' '{"permissions": {"deny": ["Bash(sudo *)"], "ask": []' > "$bad_dir/invalid.json"
printf '%s\n' '{"permissions": {"deny": [], "ask": [], "allow": []}, "disableAgentView": true}' > "$bad_dir/allow.json"
printf '%s\n' '{"permissions": {"deny": [], "ask": []}, "disableAgentView": true, "defaultMode": "auto"}' > "$bad_dir/mode.json"
printf '%s\n' '{"permissions": {"deny": [1], "ask": []}, "disableAgentView": true}' > "$bad_dir/number.json"
printf '%s\n' '{"permissions": {"deny": []}, "disableAgentView": true}' > "$bad_dir/no_ask.json"
printf '%s\n' '{"permissions": {"deny": [], "ask": []}}' > "$bad_dir/no_agent_view.json"
printf '%s\n' '{"permissions": {"deny": [], "ask": []}, "disableAgentView": false}' > "$bad_dir/agent_view_false.json"
printf '%s\n' '{"permissions": {"deny": [], "ask": []}, "disableAgentView": "true"}' > "$bad_dir/agent_view_string.json"
printf '%s\n' '{"permissions": {"deny": [], "ask": []}, "disableAgentView": true}' > "$bad_dir/good.json"
for json_tool in "${JSON_TOOLS[@]}"; do
  assert_eq "1" "$(json_valid "$json_tool" "$bad_dir/invalid.json" && echo 0 || echo 1)" "$json_tool: JSON truncado é recusado"
  assert_eq "0" "$(json_shape_ok "$json_tool" "$bad_dir/good.json" && echo 0 || echo 1)" "$json_tool: controle: o formato certo, com listas vazias, é aceito"
  for bad in allow mode number no_ask no_agent_view agent_view_false agent_view_string; do
    assert_eq "1" "$(json_shape_ok "$json_tool" "$bad_dir/$bad.json" && echo 0 || echo 1)" "$json_tool: formato '$bad' é recusado"
  done
done

echo
echo "== permissions.json: contém cada regra obrigatória da política (string exata) =="
for json_tool in "${JSON_TOOLS[@]}"; do
  deny_rules="$(json_rules "$json_tool" "$PERMISSIONS_JSON" deny)"
  ask_rules="$(json_rules "$json_tool" "$PERMISSIONS_JSON" ask)"
  for rule in "${REQUIRED_DENY[@]}"; do
    assert_eq "1" "$(grep -cxF -- "$rule" <<< "$deny_rules" || true)" "$json_tool: deny contém '$rule' (uma vez)"
  done
  for rule in "${REQUIRED_ASK[@]}"; do
    assert_eq "1" "$(grep -cxF -- "$rule" <<< "$ask_rules" || true)" "$json_tool: ask contém '$rule' (uma vez)"
  done
  # deny vence ask: uma regra nas duas listas ficaria só no deny, e a do ask seria enganosa.
  assert_eq "" "$(comm -12 <(sort <<< "$deny_rules") <(sort <<< "$ask_rules") | grep . || true)" "$json_tool: nenhuma regra aparece em deny e em ask ao mesmo tempo"
done

echo
echo "== claude_settings_load: carrega o conteúdo exato do arquivo, com e sem set -euo pipefail =="
load_dir="$(new_case load)"
expected_content="$(cat "$PERMISSIONS_JSON" 2>/dev/null || true)"
run_claude "$load_dir" "claude_settings_load settings; echo \"rc=\$?\" > '$load_dir/rc_in'; printf '%s' \"\$settings\""
assert_eq "rc=0" "$(file_of "$load_dir/rc_in")" "set -euo pipefail: retorna 0"
assert_eq "$expected_content" "$(file_of "$load_dir/out")" "set -euo pipefail: a variável recebe o conteúdo do permissions.json (igual ao cat)"
assert_eq "" "$(file_of "$load_dir/err")" "set -euo pipefail: nada no stderr"
run_claude "$load_dir" "claude_settings_load settings; echo \"rc=\$?\" > '$load_dir/rc_in'; printf '%s' \"\$settings\"" ":" off
assert_eq "rc=0" "$(file_of "$load_dir/rc_in")" "sem set -euo pipefail: retorna 0"
assert_eq "$expected_content" "$(file_of "$load_dir/out")" "sem set -euo pipefail: a variável recebe o conteúdo do permissions.json"
# Um arquivo de outro lugar (o caminho é variável só pra os testes): o conteúdo vem dele.
printf '%s\n' '{"permissions": {"deny": ["Bash(x *)"], "ask": []}}' > "$load_dir/other.json"
run_claude "$load_dir" "NICK_CLAUDE_PERMISSIONS_FILE='$load_dir/other.json'; claude_settings_load settings; printf '%s' \"\$settings\""
assert_eq '{"permissions": {"deny": ["Bash(x *)"], "ask": []}}' "$(file_of "$load_dir/out")" "o arquivo é lido na chamada, a partir de NICK_CLAUDE_PERMISSIONS_FILE"

echo
echo "== claude_settings_load: o nome da variável de destino não é sombreado pelas locais =="
for shadow_name in content file path settings value result var __resultvar; do
  shadow_dir="$(new_case "shadow_$shadow_name")"
  run_claude "$shadow_dir" "$shadow_name=antes; claude_settings_load $shadow_name; printf '%s' \"\$$shadow_name\""
  assert_eq "$expected_content" "$(file_of "$shadow_dir/out")" "destino '$shadow_name': recebe o conteúdo"
done
assert_eq "" "$(grep -nE '^[[:space:]]*local[[:space:]]' "$CLAUDE_SH" 2>/dev/null | grep -vE '^[0-9]+:[[:space:]]*local([[:space:]]+__claude_[a-z_]+(=[^[:space:]]*)?)+[[:space:]]*$' || true)" "toda local de lib/claude/claude.sh tem o prefixo __claude_"

echo
echo "== localização: NICK_CLAUDE_PERMISSIONS_FILE é absoluto, de qualquer cwd e por caminho relativo =="
loc_value() {
  timeout 5 bash -c "cd '$1' || exit 99; source '$2'; printf '%s' \"\$NICK_CLAUDE_PERMISSIONS_FILE\"" < /dev/null 2>/dev/null || true
}
assert_eq "$PERMISSIONS_JSON" "$(loc_value / "$CLAUDE_SH")" "carregada com o cwd em /: aponta para lib/claude/permissions.json"
assert_eq "$PERMISSIONS_JSON" "$(loc_value "$TEST_TMP" "$CLAUDE_SH")" "carregada com o cwd no TEST_TMP: aponta para lib/claude/permissions.json"
assert_eq "$PERMISSIONS_JSON" "$(loc_value "$LIB_DIR/tests" ../claude/claude.sh)" "carregada por caminho relativo (../claude/claude.sh): aponta para lib/claude/permissions.json"
assert_eq "$PERMISSIONS_JSON" "$(loc_value "$LIB_DIR" claude/claude.sh)" "carregada por caminho relativo (claude/claude.sh): aponta para lib/claude/permissions.json"
cdpath_value="$(timeout 5 bash -c "cd '$LIB_DIR' || exit 99; export CDPATH='$TEST_TMP'; source claude/claude.sh; printf '%s' \"\$NICK_CLAUDE_PERMISSIONS_FILE\"" < /dev/null 2>/dev/null || true)"
assert_eq "$PERMISSIONS_JSON" "$cdpath_value" "com CDPATH definido: o valor continua só o caminho (o cd não imprime nada nele)"
rel_load_dir="$(new_case relative_load)"
rel_out="$(timeout 5 bash -c "cd '$LIB_DIR/tests' || exit 99; set -euo pipefail; source ../claude/claude.sh; cd /; claude_settings_load s; printf '%s' \"\$s\"" < /dev/null 2>"$rel_load_dir/err" || true)"
assert_eq "$expected_content" "$rel_out" "carregada por caminho relativo e chamada depois de trocar de cwd: lê o permissions.json"

echo
echo "== erros: arquivo ausente, sem leitura ou vazio — retorna 1, stderr com o caminho, sem exit =="
err_dir="$(new_case errors)"
: > "$err_dir/empty.json"
printf '\n\n' > "$err_dir/blank.json"
printf '  \n\t\n' > "$err_dir/spaces.json"
cp "$PERMISSIONS_JSON" "$err_dir/noread.json" 2>/dev/null || printf '{}' > "$err_dir/noread.json"
chmod 000 "$err_dir/noread.json"
mkdir -p "$err_dir/a dir.json"
err_variants=(missing empty blank spaces directory)
if [ "$(id -u)" -eq 0 ]; then
  echo "  PULADO: arquivo sem permissão de leitura (rodando como root, que lê mesmo com chmod 000)"
else
  err_variants+=(noread)
fi
for err_variant in "${err_variants[@]}"; do
  case "$err_variant" in
    missing) err_file="$err_dir/nao existe.json" ;;
    empty) err_file="$err_dir/empty.json" ;;
    blank) err_file="$err_dir/blank.json" ;;
    spaces) err_file="$err_dir/spaces.json" ;;
    directory) err_file="$err_dir/a dir.json" ;;
    noread) err_file="$err_dir/noread.json" ;;
  esac
  for err_mode in on off; do
    case_dir="$(new_case "err_${err_variant}_$err_mode")"
    # O override vem depois do source (a carga atribui a constante).
    run_claude "$case_dir" "NICK_CLAUDE_PERMISSIONS_FILE='$err_file'; settings=intacto; claude_settings_load settings || echo \"rc=\$?\"; echo \"settings=\$settings\"; echo depois" ":" "$err_mode"
    err_label="$err_variant (set -e $err_mode)"
    assert_eq "0" "$(file_of "$case_dir/rc")" "$err_label: o shell segue (nenhum exit)"
    assert_eq "rc=1"$'\n'"settings=intacto"$'\n'"depois" "$(file_of "$case_dir/out")" "$err_label: retorna 1, variável intacta e nada mais no stdout"
    assert_contains "$(file_of "$case_dir/err")" "Erro: regras de permissão do Claude Code não encontradas ou ilegíveis: $err_file" "$err_label: stderr com o caminho"
    assert_contains "$(file_of "$case_dir/err")" "O nick não abre o claude sem essas regras." "$err_label: stderr diz que o claude não é aberto"
  done
done

echo
echo "== claude_print_settings_error: caracteres de controle do caminho viram '?' =="
ctl_dir="$(new_case control)"
run_claude "$ctl_dir" "claude_print_settings_error \$'/x/a\\e[2Jb.json'"
assert_eq "" "$(file_of "$ctl_dir/out")" "nada no stdout"
assert_contains "$(file_of "$ctl_dir/err")" "ilegíveis: /x/a?[2Jb.json" "o ESC do caminho vira '?'"
assert_not_contains "$(file_of "$ctl_dir/err")" $'\e' "nenhum ESC chega ao terminal"
run_claude "$ctl_dir" "claude_print_settings_error '/proj/não.json'" "export LC_ALL=C"
assert_contains "$(file_of "$ctl_dir/err")" "ilegíveis: /proj/não.json" "com LC_ALL=C, o 'não' do caminho sai intacto"

echo
echo "== claude_conversation_file: o arquivo da conversa do claude (NS-39) =="
conv_dir="$(new_case conversation)"
# Um caminho com cada tipo de caractere: só A-Z, a-z e 0-9 ficam; o resto vira "-" (com locale
# UTF-8, um caractere acentuado vira um "-" só, como o claude faz).
run_claude "$conv_dir" "claude_conversation_file /cfg '/home/u/meu_proj.v2/a b é' 0f8e-11 file; printf '%s' \"\$file\"" "export LC_ALL=C.UTF-8"
assert_eq "0" "$(file_of "$conv_dir/rc")" "sai com 0"
assert_eq "/cfg/projects/-home-u-meu-proj-v2-a-b--/0f8e-11.jsonl" "$(file_of "$conv_dir/out")" \
  "<config>/projects/<dir com todo caractere não alfanumérico trocado por ->/<sessão>.jsonl"
assert_eq "" "$(file_of "$conv_dir/err")" "nada no stderr"
run_claude "$conv_dir" "claude_conversation_file /cfg '/x/é' s file; printf '%s' \"\$file\"" "export LC_ALL=C"
assert_eq "/cfg/projects/-x---/s.jsonl" "$(file_of "$conv_dir/out")" \
  "controle: com LC_ALL=C o é são dois bytes e vira \"--\" (por isso quem chama força UTF-8)"
run_claude "$conv_dir" "file=intacto; other=x; claude_conversation_file /c /d s file; printf '%s|%s' \"\$file\" \"\$other\""
assert_eq "/c/projects/-d/s.jsonl|x" "$(file_of "$conv_dir/out")" "grava só na variável pedida (printf -v)"

echo
echo "== claude_conversation_latest: segue os continued-in até a última sessão (NS-50) =="
# chain_uuid <n>: um UUID minúsculo (8-4-4-4-12) diferente para cada <n>.
chain_uuid() {
  printf '%08x-0000-4000-8000-%012x' "$1" "$1"
}
CHAIN_A="$(chain_uuid 10)"
CHAIN_B="$(chain_uuid 11)"
CHAIN_C="$(chain_uuid 12)"
CHAIN_D="$(chain_uuid 13)"
# chain_file <case_dir> <sessão>: o arquivo da conversa <sessão> com o cwd em /proj, sob
# <case_dir>/cfg (o mesmo layout da claude_conversation_file: a pasta é "-proj").
chain_file() {
  printf '%s/cfg/projects/-proj/%s.jsonl' "$1" "$2"
}
# continued_line <sessão> <próxima>: o registro que o claude grava ao trocar de sessão, no formato
# real (claude 2.1.328, seção 1 do plano do NS-50).
continued_line() {
  printf '{"type":"continued-in","timestamp":"2026-11-21T14:41:33.908Z","sessionId":"%s","continuedInSessionId":"%s"}\n' "$1" "$2"
}
# talk_line <sessão> <texto>: uma linha de conversa (mensagem do usuário).
talk_line() {
  printf '{"parentUuid":null,"type":"user","message":{"role":"user","content":"%s"},"sessionId":"%s"}\n' "$2" "$1"
}
# chain_write <case_dir> <sessão> [linhas...]: cria o arquivo de <sessão> com uma linha de conversa
# e depois as [linhas] dadas.
chain_write() {
  local case_dir="$1" session="$2" file
  shift 2
  file="$(chain_file "$case_dir" "$session")"
  mkdir -p "$(dirname "$file")"
  talk_line "$session" "oi" > "$file"
  if [ "$#" -gt 0 ]; then
    printf '%s\n' "$@" >> "$file"
  fi
}
# chain_latest <case_dir> <sessão> [errexit]: roda a função com o cwd do caso e grava a resposta
# em out (rc e err como no run_claude).
chain_latest() {
  run_claude "$1" "claude_conversation_latest '$1/cfg' /proj '$2' latest; printf '%s' \"\$latest\"" ":" "${3:-on}"
}
# chain_check <case_dir> <esperado> <descrição>: a resposta, o código 0 e o stderr vazio.
chain_check() {
  assert_eq "0|$2" "$(file_of "$1/rc")|$(file_of "$1/out")" "$3"
  assert_eq "" "$(file_of "$1/err")" "$3: nada no stderr"
}

chain_dir="$(new_case chain_none)"
chain_write "$chain_dir" "$CHAIN_A"
chain_latest "$chain_dir" "$CHAIN_A"
chain_check "$chain_dir" "$CHAIN_A" "arquivo sem continued-in: a própria sessão"

chain_dir="$(new_case chain_one)"
chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
chain_write "$chain_dir" "$CHAIN_B"
chain_latest "$chain_dir" "$CHAIN_A"
chain_check "$chain_dir" "$CHAIN_B" "um salto (A → B)"

chain_dir="$(new_case chain_two)"
chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
chain_write "$chain_dir" "$CHAIN_B" "$(continued_line "$CHAIN_B" "$CHAIN_C")"
chain_write "$chain_dir" "$CHAIN_C"
for chain_mode in on off; do
  chain_latest "$chain_dir" "$CHAIN_A" "$chain_mode"
  chain_check "$chain_dir" "$CHAIN_C" "dois saltos (A → B → C), set -euo pipefail $chain_mode"
done
chain_latest "$chain_dir" "$CHAIN_B"
chain_check "$chain_dir" "$CHAIN_C" "começando do meio (B → C)"

chain_dir="$(new_case chain_missing)"
chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
chain_write "$chain_dir" "$CHAIN_B" "$(continued_line "$CHAIN_B" "$CHAIN_C")"
chain_latest "$chain_dir" "$CHAIN_A"
chain_check "$chain_dir" "$CHAIN_B" "o salto para um arquivo que não existe (C): para antes dele, em B"
# O arquivo seguinte numa outra pasta de projeto não conta: só a mesma pasta.
mkdir -p "$chain_dir/cfg/projects/-outro"
talk_line "$CHAIN_C" "oi" > "$chain_dir/cfg/projects/-outro/$CHAIN_C.jsonl"
chain_latest "$chain_dir" "$CHAIN_A"
chain_check "$chain_dir" "$CHAIN_B" "o arquivo seguinte em outra pasta de projeto não conta"

chain_dir="$(new_case chain_last)"
chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")" "$(talk_line "$CHAIN_A" "mais")" \
  "$(continued_line "$CHAIN_A" "$CHAIN_C")"
chain_write "$chain_dir" "$CHAIN_B"
chain_write "$chain_dir" "$CHAIN_C"
chain_latest "$chain_dir" "$CHAIN_A"
chain_check "$chain_dir" "$CHAIN_C" "vários continued-in no mesmo arquivo: vale o último (C, não B)"

chain_dir="$(new_case chain_after)"
chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")" "$(talk_line "$CHAIN_A" "Deu certo")" \
  "$(talk_line "$CHAIN_A" "mais uma")"
chain_write "$chain_dir" "$CHAIN_B"
chain_latest "$chain_dir" "$CHAIN_A"
chain_check "$chain_dir" "$CHAIN_B" "conversa depois do continued-in: segue mesmo assim (D2, item 6)"

# Registros malformados: o último registro é ignorado e a cadeia para em A (o B existe, então
# seguir seria o erro). O B com "/" tenta sair da pasta.
chain_dir="$(new_case chain_bad)"
chain_write "$chain_dir" "$CHAIN_B"
mkdir -p "$chain_dir/cfg/projects/x"
talk_line "$CHAIN_B" "oi" > "$chain_dir/cfg/projects/x/$CHAIN_B.jsonl"
chain_upper="$(printf '%s' "$CHAIN_B" | tr 'a-f' 'A-F')"
chain_write "$chain_dir" "$chain_upper"
for chain_bad in absent empty upper strange slash short other_session; do
  case "$chain_bad" in
    absent) chain_record="{\"type\":\"continued-in\",\"timestamp\":\"2026-11-21T14:41:33.908Z\",\"sessionId\":\"$CHAIN_A\"}" ;;
    empty) chain_record="$(continued_line "$CHAIN_A" "")" ;;
    upper) chain_record="$(continued_line "$CHAIN_A" "$chain_upper")" ;;
    strange) chain_record="$(continued_line "$CHAIN_A" "${CHAIN_B%?}g")" ;;
    slash) chain_record="$(continued_line "$CHAIN_A" "../x/$CHAIN_B")" ;;
    short) chain_record="$(continued_line "$CHAIN_A" "${CHAIN_B%?}")" ;;
    other_session) chain_record="$(continued_line "$CHAIN_D" "$CHAIN_B")" ;;
  esac
  # Antes do registro ruim, um bom: o último é o que vale, então o bom não salva a cadeia.
  chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")" "$chain_record"
  chain_latest "$chain_dir" "$CHAIN_A"
  chain_check "$chain_dir" "$CHAIN_A" "último registro malformado ($chain_bad): ignorado, a cadeia para em A"
done
# Controle positivo: o mesmo arquivo com o registro bom por último segue para B.
chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
chain_latest "$chain_dir" "$CHAIN_A"
chain_check "$chain_dir" "$CHAIN_B" "controle: com o registro bom por último, segue para B"

chain_dir="$(new_case chain_cycle)"
chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
chain_write "$chain_dir" "$CHAIN_B" "$(continued_line "$CHAIN_B" "$CHAIN_A")"
chain_latest "$chain_dir" "$CHAIN_A"
chain_check "$chain_dir" "$CHAIN_B" "ciclo A → B → A: para em B, sem voltar a A"
chain_write "$chain_dir" "$CHAIN_C" "$(continued_line "$CHAIN_C" "$CHAIN_C")"
chain_latest "$chain_dir" "$CHAIN_C"
chain_check "$chain_dir" "$CHAIN_C" "ciclo C → C: a própria sessão"

# O limite de saltos (50): uma cadeia de 60 sessões para na 50ª depois da inicial.
chain_dir="$(new_case chain_limit)"
for chain_n in $(seq 100 159); do
  chain_write "$chain_dir" "$(chain_uuid "$chain_n")" "$(continued_line "$(chain_uuid "$chain_n")" "$(chain_uuid $((chain_n + 1)))")"
done
chain_write "$chain_dir" "$(chain_uuid 160)"
chain_latest "$chain_dir" "$(chain_uuid 100)"
chain_check "$chain_dir" "$(chain_uuid 150)" "uma cadeia de 60 saltos para no limite de 50"
chain_latest "$chain_dir" "$(chain_uuid 120)"
chain_check "$chain_dir" "$(chain_uuid 160)" "controle: uma cadeia de 40 saltos chega ao fim"

if [ "$(id -u)" -eq 0 ]; then
  echo "  PULADO: arquivos ilegíveis (rodando como root, que lê mesmo com chmod 000)"
else
  chain_dir="$(new_case chain_unreadable_next)"
  chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
  chain_write "$chain_dir" "$CHAIN_B"
  chmod 000 "$(chain_file "$chain_dir" "$CHAIN_B")"
  for chain_mode in on off; do
    chain_latest "$chain_dir" "$CHAIN_A" "$chain_mode"
    chain_check "$chain_dir" "$CHAIN_B" "arquivo seguinte ilegível: a resposta é ele (o arquivo existe), set -e $chain_mode"
  done
  chain_dir="$(new_case chain_unreadable_middle)"
  chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
  chain_write "$chain_dir" "$CHAIN_B" "$(continued_line "$CHAIN_B" "$CHAIN_C")"
  chain_write "$chain_dir" "$CHAIN_C"
  chmod 000 "$(chain_file "$chain_dir" "$CHAIN_B")"
  chain_latest "$chain_dir" "$CHAIN_A"
  chain_check "$chain_dir" "$CHAIN_B" "arquivo do meio ilegível com um continued-in: a cadeia para nele (B, não C)"
  chain_dir="$(new_case chain_unreadable_first)"
  chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
  chain_write "$chain_dir" "$CHAIN_B"
  chmod 000 "$(chain_file "$chain_dir" "$CHAIN_A")"
  chain_latest "$chain_dir" "$CHAIN_A"
  chain_check "$chain_dir" "$CHAIN_A" "arquivo inicial ilegível: a própria sessão"
fi

chain_dir="$(new_case chain_vars)"
chain_write "$chain_dir" "$CHAIN_A" "$(continued_line "$CHAIN_A" "$CHAIN_B")"
chain_write "$chain_dir" "$CHAIN_B"
run_claude "$chain_dir" "file=intacto; other=x; compgen -v | grep -vxE '$clean_bash_vars' | sort > '$chain_dir/before'; claude_conversation_latest '$chain_dir/cfg' /proj '$CHAIN_A' file; compgen -v | grep -vxE '$clean_bash_vars' | sort > '$chain_dir/after'; printf '%s|%s' \"\$file\" \"\$other\""
assert_eq "$CHAIN_B|x" "$(file_of "$chain_dir/out")" "grava só na variável pedida (printf -v)"
assert_eq "" "$(comm -3 "$chain_dir/before" "$chain_dir/after" 2> /dev/null || echo 'sem retrato')" "nenhuma variável nova nem apagada no chamador"
# A leitura é só das linhas do registro (grep), sem jq nem carregar o arquivo inteiro.
chain_body="$(sed -n '/^claude_conversation_latest()/,/^}/p' "$CLAUDE_SH" 2> /dev/null || true)"
assert_contains "$chain_body" "grep -F" "a função filtra as linhas do registro com grep -F"
assert_eq "" "$(grep -nE '\$\(cat|mapfile|readarray|while[^#]*read' <<< "$chain_body" || true)" "a função não carrega o arquivo inteiro (sem cat, mapfile nem laço de read)"

echo
echo "== claude_permission_notice: a instrução de não contornar uma recusa (NS-41) =="
notice_dir="$(new_case notice)"
for notice_mode in on off; do
  run_claude "$notice_dir" "claude_permission_notice" ":" "$notice_mode"
  assert_eq "0" "$(file_of "$notice_dir/rc")" "set -e $notice_mode: retorna 0"
  assert_eq "" "$(file_of "$notice_dir/err")" "set -e $notice_mode: nada no stderr"
  assert_eq "com texto" "$([ -s "$notice_dir/out" ] && echo 'com texto' || echo 'vazio')" "set -e $notice_mode: imprime o texto no stdout"
done
notice_text="$(file_of "$notice_dir/out")"
# Não lê o JSON: funciona com as regras ausentes.
run_claude "$notice_dir" "NICK_CLAUDE_PERMISSIONS_FILE='$notice_dir/nao existe.json'; claude_permission_notice"
assert_eq "0" "$(file_of "$notice_dir/rc")" "com NICK_CLAUDE_PERMISSIONS_FILE inexistente: retorna 0"
assert_eq "$notice_text" "$(file_of "$notice_dir/out")" "com NICK_CLAUDE_PERMISSIONS_FILE inexistente: o mesmo texto (não lê o JSON)"
assert_eq "" "$(file_of "$notice_dir/err")" "com NICK_CLAUDE_PERMISSIONS_FILE inexistente: nada no stderr"
# Os pontos obrigatórios (D4 do plano), por trechos curtos.
for notice_part in \
  "Regras de permissão: esta sessão foi aberta pelo nick" \
  "regras de permissão do Claude Code (deny e ask)" \
  "quando o usuário negar um pedido de confirmação" \
  "a recusa é uma decisão do usuário" \
  "não obtenha o mesmo efeito por outro meio" \
  "com um script" \
  "bash -c" \
  "python -c" \
  "outro interpretador" \
  "é contornar a regra" \
  "pare" \
  "qual comando foi recusado" \
  "pergunte como ele quer seguir" \
  "vale só para o que uma regra recusou" \
  "comandos permitidos continuam liberados" \
  "não precisa evitá-los nem pedir licença"; do
  assert_contains "$notice_text" "$notice_part" "a instrução diz: $notice_part"
done
# Guarda: a instrução não manda evitar comandos permitidos (um rm comum, por exemplo).
for notice_bad in "evite rm" "não use rm" "evite o rm" "nunca use rm"; do
  assert_not_contains "$notice_text" "$notice_bad" "a instrução não diz: $notice_bad"
done
# Texto fixo: igual em duas chamadas e sem depender do cwd nem do HOME.
run_claude "$notice_dir" "claude_permission_notice; claude_permission_notice > '$notice_dir/second'"
assert_eq "$notice_text" "$(file_of "$notice_dir/second")" "duas chamadas: o mesmo texto"
run_claude "$notice_dir" "cd /; claude_permission_notice" "export HOME='$notice_dir/outro home'"
assert_eq "$notice_text" "$(file_of "$notice_dir/out")" "com outro cwd e outro HOME: o mesmo texto"

echo
echo "== fonte única da instrução: só lib/claude/claude.sh tem o texto, e as ferramentas chamam a função =="
NICK_ROOT="$(cd "$LIB_DIR/.." && pwd)"
notice_opening="Regras de permissão: esta sessão foi aberta pelo nick"
# Controle positivo: a frase está na lib, então o teste morde.
assert_eq "1" "$(grep -cF -- "$notice_opening" "$CLAUDE_SH" || true)" "controle: a frase de abertura está em lib/claude/claude.sh (uma vez)"
notice_copies="$(grep -rlF -- "$notice_opening" "$NICK_ROOT/auto_scrum/auto_scrum.sh" "$NICK_ROOT/auto_scrum/prompt.sh" \
  "$NICK_ROOT/auto_scrum/templates" "$NICK_ROOT/bin/nick-agent" "$NICK_ROOT/nick/src" 2>/dev/null || true)"
assert_eq "" "$notice_copies" "a frase não está copiada no auto_scrum.sh, no prompt.sh, nos templates, no bin/nick-agent nem em nick/src/"
for notice_tool in auto_scrum/auto_scrum.sh bin/nick-agent; do
  assert_contains "$(code_lines "$NICK_ROOT/$notice_tool" 2>/dev/null | grep -wF claude_permission_notice || true)" "claude_permission_notice" "$notice_tool chama claude_permission_notice"
done

echo
echo "== carga limpa: source só define nomes claude_/NICK_CLAUDE_, sem imprimir nada =="
clean_dir="$TEST_TMP/clean"
mkdir -p "$clean_dir"
clean_rc=0
# Só redirecionamentos entre os retratos, pra o próprio teste não criar variável nenhuma.
timeout 2 bash --norc --noprofile -c "compgen -v | sort > '$clean_dir/vars_before'; declare -F | sort > '$clean_dir/funcs_before'; source '$CLAUDE_SH' > '$clean_dir/out' 2>&1; compgen -v | sort > '$clean_dir/vars_after'; declare -F | sort > '$clean_dir/funcs_after'" < /dev/null || clean_rc=$?
assert_eq "0" "$clean_rc" "source lib/claude/claude.sh num bash limpo termina com código 0"
assert_eq "" "$(cat "$clean_dir/out" 2>/dev/null || echo 'sem saída gravada')" "source lib/claude/claude.sh não imprime nada (stdout nem stderr)"
clean_new_vars="$(comm -13 "$clean_dir/vars_before" "$clean_dir/vars_after" 2>/dev/null | grep -vxE "$clean_bash_vars" || true)"
clean_new_funcs="$(comm -13 "$clean_dir/funcs_before" "$clean_dir/funcs_after" 2>/dev/null | sed 's/^declare -f //' || true)"
# Num terminal com tamanho, o bash atualiza COLUMNS e LINES (checkwinsize) depois de cada comando
# externo, como o sort do 1º retrato: elas apareceriam como novas sem a biblioteca criar nada.
# O mesmo retrato, num pseudo-terminal 80x24 (script, do util-linux), dá as mesmas variáveis novas.
if script -qec true /dev/null > /dev/null 2>&1; then
  mkdir -p "$clean_dir/tty"
  script -qec "stty cols 80 rows 24; timeout 2 bash --norc --noprofile -c \"compgen -v | sort > '$clean_dir/tty/vars_before'; source '$CLAUDE_SH' > /dev/null 2>&1; compgen -v | sort > '$clean_dir/tty/vars_after'\"" /dev/null < /dev/null > /dev/null 2>&1 || true
  clean_tty_new_vars="$(comm -13 "$clean_dir/tty/vars_before" "$clean_dir/tty/vars_after" 2>/dev/null | grep -vxE "$clean_bash_vars" || true)"
  assert_eq "$clean_new_vars" "$clean_tty_new_vars" "num terminal 80x24: as mesmas variáveis novas (COLUMNS e LINES são do bash)"
else
  echo "  PULADO: sem o script do util-linux pra simular um terminal"
fi
assert_eq "NICK_CLAUDE_PERMISSIONS_FILE" "$clean_new_vars" "a única variável nova é NICK_CLAUDE_PERMISSIONS_FILE"
assert_eq "" "$(grep -vE '^claude_' <<< "$clean_new_funcs" | grep . || true)" "toda função nova começa com claude_"
for clean_func in claude_settings_load claude_print_settings_error claude_conversation_file claude_conversation_latest claude_permission_notice; do
  assert_contains "$clean_new_funcs" "$clean_func" "source lib/claude/claude.sh define $clean_func"
done

echo
echo "== carga limpa: estrutura do arquivo (sem shebang, sem bit de execução, sem set/exit/source/eval) =="
assert_eq "# shellcheck shell=bash" "$(head -n1 "$CLAUDE_SH" 2>/dev/null || true)" "a 1ª linha é a diretiva '# shellcheck shell=bash' (sem shebang)"
assert_eq "sem bit de execução" "$([ -e "$CLAUDE_SH" ] && [ ! -x "$CLAUDE_SH" ] && echo 'sem bit de execução' || echo 'executável ou inexistente')" "lib/claude/claude.sh existe e não tem bit de execução"
claude_code="$(code_lines "$CLAUDE_SH" 2>/dev/null || true)"
assert_eq "" "$(grep -nE '(^|[;&|[:space:]])set[[:space:]]+[-+]' <<< "$claude_code" || true)" "nenhum set -e/set -o (nem outro set) fora de comentário"
assert_eq "" "$(grep -nwE 'exit|source|eval' <<< "$claude_code" || true)" "nenhum exit, source ou eval fora de comentário"
assert_eq "" "$(grep -nE '\[\[[[:space:]]' <<< "$claude_code" || true)" "nenhum teste com [[ ]] (a convenção do projeto é [ ])"
assert_eq "" "$(grep -nE '(^|[;&|])[[:space:]]*\.[[:space:]]+[^[:space:]]' <<< "$claude_code" || true)" "nenhum '. arquivo' fora de comentário"
assert_eq "" "$(grep -nE 'local[[:space:]]+-n|declare[[:space:]]+-n' <<< "$claude_code" || true)" "nenhum nameref (local -n/declare -n)"

echo
echo "== prefixo: funções claude_, globais NICK_CLAUDE_, nunca CLAUDE_ sozinho =="
assert_eq "" "$(grep -nE '^[[:space:]]*(function[[:space:]]+)?[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\(\)' <<< "$claude_code" | grep -vE '^[0-9]+:[[:space:]]*(function[[:space:]]+)?claude_' || true)" "toda função definida começa com claude_"
assert_eq "" "$(grep -nE '^[A-Za-z_][A-Za-z0-9_]*=' <<< "$claude_code" | grep -vE '^[0-9]+:NICK_CLAUDE_' || true)" "toda global atribuída no nível de arquivo começa com NICK_CLAUDE_"
assert_eq "" "$(grep -nF 'CLAUDE_' <<< "${claude_code//NICK_CLAUDE_/}" || true)" "nenhum CLAUDE_ sem NICK_ na frente (o Claude Code lê CLAUDE_* do ambiente)"
# Não lê globais do chamador: as únicas variáveis expandidas são as da própria biblioteca
# (NICK_CLAUDE_*, as locais __claude_*), os parâmetros posicionais e BASH_SOURCE.
claude_refs="$(grep -oE '\$\{?[A-Za-z_][A-Za-z0-9_]*' <<< "$claude_code" | sed -E 's/^\$\{?//' | sort -u || true)"
assert_eq "" "$(grep -vE '^(NICK_CLAUDE_[A-Z0-9_]+|__claude_[a-z_]+|BASH_SOURCE)$' <<< "$claude_refs" | grep . || true)" "só expande variáveis da própria biblioteca (nenhuma global do chamador)"

echo
echo "== independência: lib/claude/claude.sh não depende de lib/config/config.sh nem de lib/jail/jail.sh =="
assert_eq "" "$(grep -nE 'config_|CONFIG_|jail_|JAIL_' <<< "$claude_code" || true)" "lib/claude/claude.sh não cita config_, CONFIG_, jail_ nem JAIL_ fora de comentário"
assert_eq "" "$(grep -nE 'config\.sh|jail\.sh' <<< "$claude_code" || true)" "lib/claude/claude.sh não cita config.sh nem jail.sh fora de comentário"
# O padrão não casa com a própria linha: o [^;]* para no ';' do colchete, e a descrição não
# usa as palavras.
assert_eq "" "$(grep -nE 'source[^;]*(config|jail)' <<< "$(code_lines "$0")" || true)" "esta suíte nunca carrega as outras bibliotecas"

echo
echo "== fonte única: nenhuma regra (Bash(/Read() no código de claude.sh nem do auto_scrum.sh =="
assert_eq "" "$(grep -nE '(Bash|Read)\(' <<< "$claude_code" || true)" "lib/claude/claude.sh não tem regra fora de comentário"
assert_eq "" "$(grep -nE '(Bash|Read)\(' <<< "$(code_lines "$AUTO_SCRUM_SH")" || true)" "auto_scrum/auto_scrum.sh não tem regra fora de comentário"
# jq/python só contam em posição de comando (começo da linha, depois de ; & | ( ou $(, ou de uma
# palavra-chave): o texto da claude_permission_notice (NS-41) cita "python -c" como exemplo de
# contorno, dentro de um echo, e isso não é dependência em runtime.
runtime_tool_uses() {
  grep -nE '(^|[;&|(]|\$\(|(^|[[:space:]])(if|then|do|else|elif|while|until|!|exec|command)[[:space:]])[[:space:]]*(jq|python3?)([[:space:]]|\)|$)' || true
}
assert_eq "" "$(runtime_tool_uses <<< "$claude_code")" "lib/claude/claude.sh não usa jq nem python (sem dependência nova em runtime)"
for tool_use in "x=\"\$(python3 -c 'print(1)')\"" 'cat f | jq .' '  python3 -m json.tool f' 'if jq -e . f; then' 'a && python -c x' '  command jq .' "y=\$(jq .)"; do
  assert_contains "$(runtime_tool_uses <<< "$tool_use")" "1:" "controle: o guarda acha o uso em runtime: $tool_use"
done
assert_eq "" "$(runtime_tool_uses <<< '  echo "- com um script, bash -c, sh -c, python -c, outro interpretador;"')" "controle: \"python -c\" dentro do texto de um echo não conta como uso"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures teste(s) falharam."
  exit 1
fi
echo "Todos os testes passaram."
