#!/usr/bin/env bash
#
# test_keys: testes em bash puro (sem framework) de lib/config/keys.sh, o registro de todas as
# chaves da configuração do nick (config_register_nick_keys). Cada caso roda num bash -c
# separado, que dá source em lib/config/config.sh e lib/config/keys.sh, nessa ordem. A
# biblioteca genérica (lib/config/config.sh) é testada só com chaves de mentira em
# lib/tests/test_config.sh; as chaves concretas são testadas aqui.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_SH="$SCRIPT_DIR/../config/config.sh"
KEYS_SH="$SCRIPT_DIR/../config/keys.sh"

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

# LOADER_SH: carrega as duas partes da biblioteca e registra as chaves do nick, como fazem as
# ferramentas (auto_scrum.sh, bin/nick-init).
LOADER_SH="$TEST_TMP/loader.sh"
cat > "$LOADER_SH" <<EOF
source '$CONFIG_SH'
source '$KEYS_SH'
config_register_nick_keys
EOF

# run_keys <commands>: num bash separado, com set -euo pipefail, dá source em LOADER_SH e roda
# <commands>. O '|| true' mantém a suíte rodando (e reportando FAIL) mesmo se o bash de dentro
# morrer, ex: função ainda inexistente.
run_keys() {
  timeout 2 bash -c "set -euo pipefail; source '$LOADER_SH'; $1" 2>/dev/null || true
}

# run_loader <config_file> <out_prefix> [setup]: num bash separado, com as chaves do nick
# registradas, roda [setup] (padrão: nada), chama config_load <config_file> com stdout em
# <out_prefix>.out e stderr em <out_prefix>.err e imprime as 5 primeiras chaves separadas por
# '|' (chave não atribuída sai vazia).
run_loader() {
  local config_file="$1" out_prefix="$2" setup="${3:-:}"
  timeout 2 bash -c "source '$LOADER_SH'; $setup; config_load '$config_file' >'$out_prefix.out' 2>'$out_prefix.err'; printf '%s|%s|%s|%s|%s' \"\${STACK_DESCRIPTION-}\" \"\${SENTRY_ENABLED-}\" \"\${PERMISSION_ENABLED-}\" \"\${JIRA_ENABLED-}\" \"\${PRODUCTION_ENABLED-}\"" 2>/dev/null || true
}

# loaded_value <arquivo> <chave>: imprime o valor que config_load atribui a <chave>.
loaded_value() {
  local file="$1" key="$2"
  timeout 2 bash -c "source '$LOADER_SH'; config_load '$file' 2>/dev/null; loaded_key='$key'; printf '%s' \"\${!loaded_key-}\"" 2>/dev/null || true
}

# config_keys_blank [except_key]: imprime uma linha CHAVE="" pra cada chave registrada, menos
# <except_key>. Monta fixtures completas, que não disparam o aviso de chave ausente.
config_keys_blank() {
  local except_key="${1:-}"
  run_keys "for key in \"\${CONFIG_KEYS[@]}\"; do [ \"\$key\" = '$except_key' ] || printf '%s=\"\"\n' \"\$key\"; done"
}

# Nota de PRODUCTION_ENABLED: é uma das duas chaves com texto próprio nos avisos, porque o
# padrão desligado dela tira do prompt do PO uma pergunta que já existia.
production_note='a pergunta de impacto em produção está desligada no prompt do PO. Use "true" para ligá-la ou "false" para mantê-la desligada.'
# Nota de TDD_ENABLED: é a única flag ligada por padrão, então a nota diz que a TDD continua
# ligada (e não "a opção está desligada.").
tdd_note='a metodologia TDD continua ligada nos prompts de Tech Leader, Desenvolvimento e Review. Use "false" só em projeto sem testes automatizados.'
# Fim padrão do aviso de valor inválido, pra chave sem nota própria.
default_off_note="a opção está desligada."

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

# value_warning <arquivo:linha> <chave> <valor> <fim>: imprime, sem \n final, o texto exato
# do aviso de valor inválido.
value_warning() {
  printf '%s' "Aviso: $1: valor \"$3\" inválido para $2 (valores aceitos: \"true\", \"false\" ou \"\") — $4"
}

# count_warnings <file>: imprime quantas linhas de <file> começam com "Aviso:".
count_warnings() {
  grep -c '^Aviso:' "$1" 2>/dev/null || true
}

# Variáveis que o próprio bash cria ou mexe entre os retratos (não são da biblioteca).
clean_bash_vars='_|BASH_.*|COLUMNS|LINES|FUNCNAME|LINENO|PIPESTATUS'

echo "== carga limpa: source de keys.sh só define config_register_nick_keys, sem registrar nada =="
clean_dir="$TEST_TMP/clean"
mkdir -p "$clean_dir"
clean_rc=0
# config.sh primeiro (keys.sh depende dela), e o retrato só em volta do source de keys.sh. Só
# redirecionamentos entre os retratos, pra o próprio teste não criar variável nenhuma.
timeout 2 bash --norc --noprofile -c "source '$CONFIG_SH'; compgen -v | sort > '$clean_dir/vars_before'; declare -F | sort > '$clean_dir/funcs_before'; source '$KEYS_SH' > '$clean_dir/out' 2>&1; compgen -v | sort > '$clean_dir/vars_after'; declare -F | sort > '$clean_dir/funcs_after'; printf '%s %s %s %s %s' \"\${#CONFIG_KEYS[@]}\" \"\${#CONFIG_KEY_DEFAULTS[@]}\" \"\${#CONFIG_KEY_NOTES[@]}\" \"\${#CONFIG_KEY_COMMENTS[@]}\" \"\${#CONFIG_KEY_PROMPTS[@]}\" > '$clean_dir/registry'" < /dev/null || clean_rc=$?
assert_eq "0" "$clean_rc" "source lib/config/keys.sh (depois de config.sh) num bash limpo termina com código 0"
assert_eq "" "$(cat "$clean_dir/out" 2>/dev/null || echo 'sem saída gravada')" "source lib/config/keys.sh não imprime nada (stdout nem stderr)"
clean_new_vars="$(comm -13 "$clean_dir/vars_before" "$clean_dir/vars_after" 2>/dev/null | grep -vxE "$clean_bash_vars" || true)"
clean_new_funcs="$(comm -13 "$clean_dir/funcs_before" "$clean_dir/funcs_after" 2>/dev/null | sed 's/^declare -f //' || true)"
assert_eq "" "$clean_new_vars" "source lib/config/keys.sh não cria nenhuma variável"
assert_eq "config_register_nick_keys" "$clean_new_funcs" "source lib/config/keys.sh só cria a função config_register_nick_keys"
assert_eq "0 0 0 0 0" "$(cat "$clean_dir/registry" 2>/dev/null || echo 'sem registro gravado')" "a carga de keys.sh não registra nada: os cinco arrays do registro continuam vazios"
# Num terminal com tamanho, o bash atualiza COLUMNS e LINES (checkwinsize) depois de cada comando
# externo: o mesmo retrato, num pseudo-terminal 80x24 (script, do util-linux), não pode mostrar
# nenhuma variável nova da biblioteca.
if script -qec true /dev/null > /dev/null 2>&1; then
  mkdir -p "$clean_dir/tty"
  script -qec "stty cols 80 rows 24; timeout 2 bash --norc --noprofile -c \"source '$CONFIG_SH'; compgen -v | sort > '$clean_dir/tty/vars_before'; source '$KEYS_SH' > '$clean_dir/tty/out' 2>&1; compgen -v | sort > '$clean_dir/tty/vars_after'\"" /dev/null < /dev/null > /dev/null 2>&1 || true
  clean_tty_new_vars="$(comm -13 "$clean_dir/tty/vars_before" "$clean_dir/tty/vars_after" 2>/dev/null | grep -vxE "$clean_bash_vars" || true)"
  assert_eq "true" "$([ -f "$clean_dir/tty/vars_after" ] && echo true || echo false)" "num terminal 80x24: o retrato foi gravado (amostra não vazia)"
  assert_eq "" "$clean_tty_new_vars" "num terminal 80x24: nenhuma variável nova (COLUMNS e LINES são do bash)"
  assert_eq "" "$(cat "$clean_dir/tty/out" 2>/dev/null || echo 'sem saída gravada')" "num terminal 80x24: source não imprime nada"
else
  echo "  PULADO: sem o script do util-linux pra simular um terminal"
fi

echo
echo "== carga limpa: estrutura do arquivo (sem shebang, sem bit de execução, sem set/exit/source/eval) =="
assert_eq "# shellcheck shell=bash" "$(head -n1 "$KEYS_SH" 2>/dev/null || true)" "a 1ª linha é a diretiva '# shellcheck shell=bash' (sem shebang)"
assert_eq "sem bit de execução" "$([ -e "$KEYS_SH" ] && [ ! -x "$KEYS_SH" ] && echo 'sem bit de execução' || echo 'executável ou inexistente')" "lib/config/keys.sh existe e não tem bit de execução"
keys_code="$(code_lines "$KEYS_SH" 2>/dev/null || true)"
assert_eq "" "$(grep -nE '(^|[;&|[:space:]])set[[:space:]]+[-+]' <<< "$keys_code" || true)" "nenhum set -e/set -o (nem outro set) fora de comentário"
assert_eq "" "$(grep -nwE 'exit|source|eval' <<< "$keys_code" || true)" "nenhum exit, source ou eval fora de comentário"
assert_eq "" "$(grep -nE '(^|[;&|])[[:space:]]*\.[[:space:]]+[^[:space:]]' <<< "$keys_code" || true)" "nenhum '. arquivo' fora de comentário"

echo
echo "== registro: config_register_nick_keys registra as chaves do nick, com os padrões e as notas de antes =="
nick_keys="STACK_DESCRIPTION SENTRY_ENABLED PERMISSION_ENABLED JIRA_ENABLED JIRA_URL PRODUCTION_ENABLED TDD_ENABLED JAIL_ENABLED JAIL_CLIPBOARD JAIL_FORWARD_PORTS JAIL_ALLOW_HOSTS"
assert_eq "$nick_keys" "$(run_keys "printf '%s' \"\${CONFIG_KEYS[*]}\"")" "CONFIG_KEYS é exatamente a lista de antes, na mesma ordem"
assert_eq "[][false][false][false][][false][true][false][][][]" "$(run_keys "printf '[%s]' \"\${CONFIG_KEY_DEFAULTS[@]}\"")" "os padrões de cada chave são os de antes, com JIRA_URL \"\" depois de JIRA_ENABLED (só TDD_ENABLED com \"true\")"
assert_eq "[][][][][][$production_note][$tdd_note][][][][]" "$(run_keys "printf '[%s]' \"\${CONFIG_KEY_NOTES[@]}\"")" "as notas são as de antes: só PRODUCTION_ENABLED e TDD_ENABLED têm nota (JIRA_URL sem nota)"
assert_eq "0" "$(run_keys "t_rc=0; config_register_nick_keys 2>/dev/null || t_rc=\$?; [ \"\$t_rc\" -ne 0 ] && printf 0 || printf 1")" "chamar config_register_nick_keys duas vezes retorna ≠ 0 (as chaves já foram registradas)"
assert_eq "1|11|depois" "$(timeout 2 bash -c "source '$LOADER_SH'; config_register_nick_keys 2>/dev/null; t_rc=\$?; printf '%s|%s|depois' \"\$t_rc\" \"\${#CONFIG_KEYS[@]}\"" 2>/dev/null || true)" "duas vezes, sem set -e: retorna 1, sem exit (o shell segue) e sem registrar de novo"
# Uma chamada que falha no meio para o registro ali (|| return 1): as chaves depois dela não
# entram. A JAIL_ENABLED readonly faz a sétima chamada falhar.
assert_eq "1|STACK_DESCRIPTION SENTRY_ENABLED PERMISSION_ENABLED JIRA_ENABLED JIRA_URL PRODUCTION_ENABLED TDD_ENABLED|depois" "$(timeout 2 bash -c "source '$CONFIG_SH'; source '$KEYS_SH'; readonly JAIL_ENABLED=x; config_register_nick_keys 2>/dev/null; t_rc=\$?; printf '%s|%s|depois' \"\$t_rc\" \"\${CONFIG_KEYS[*]}\"" 2>/dev/null || true)" "registro que falha no meio: retorna 1 sem exit e não registra as chaves seguintes"
assert_eq "parou" "$(timeout 2 bash -c "set -e; source '$CONFIG_SH'; source '$KEYS_SH'; readonly JAIL_ENABLED=x; trap 'printf parou' EXIT; config_register_nick_keys 2>/dev/null; trap - EXIT; printf seguiu" 2>/dev/null || true)" "registro que falha no meio, sob set -e: o chamador para na chamada (como o bloco de antes)"

echo
echo "== registro: o esqueleto (corpo) é idêntico byte a byte ao de antes do ticket NS-30 =="
skel_dir="$TEST_TMP/skeleton"
mkdir -p "$skel_dir"
run_keys "config_print_skeleton nick > '$skel_dir/skeleton.conf'" >/dev/null
# O corpo do esqueleto (tudo menos as três linhas de cabeçalho), fixado à mão: os comentários
# de cada chave, byte a byte, como eram registrados no topo do auto_scrum.sh.
cat > "$skel_dir/body_expected.conf" <<'EOF'
# STACK_DESCRIPTION: stack técnica usada no prompt do Tech Leader/Desenvolvimento
# (texto livre, sem ponto final no fim — o template já adiciona).
# Exemplo: STACK_DESCRIPTION="Django avançado, django-tenants, Django ORM, PostgreSQL, pytest e TDD"
STACK_DESCRIPTION=""
# SENTRY_ENABLED: "true" se esse projeto usa Sentry — a opção "PO - Criar ticket a partir
# do Sentry" aparece no menu principal (o PO busca o issue pelo MCP do Sentry do .mcp.json
# do projeto alvo), e os prompts de PO e Tech Leader passam a mencionar links de erro
# externo. Padrão é opt-in: vazio ou "false" esconde a opção e tira a menção dos prompts.
SENTRY_ENABLED="false"
# PERMISSION_ENABLED: "true" se esse projeto tem um sistema de permissões — o PO passa a
# avaliar, pra cada pedido, se será necessário adicionar uma nova permissão. Padrão é
# opt-in: vazio ou "false" tira essa pergunta do prompt do PO.
PERMISSION_ENABLED="false"
# JIRA_ENABLED: "true" se esse projeto tem o MCP do Jira configurado (.mcp.json do
# projeto alvo) — só então a opção "PO - Criar ticket a partir do Jira" aparece no menu
# principal. Padrão é opt-in: vazio ou "false" esconde a opção do menu.
JIRA_ENABLED="false"
# JIRA_URL: base dos links do Jira (ex: https://empresa.atlassian.net/jira/browse/). O nick
# monta o link de cada ticket e épico com ID do Jira como base + ID e o mostra no painel e
# nos formulários das etapas. Guardada exatamente como digitada. Vazio: sem link.
JIRA_URL=""
# PRODUCTION_ENABLED: "true" se esse projeto já está em produção — o PO passa a avaliar,
# pra cada pedido, se existe impacto em produção. Padrão é opt-in: vazio ou "false" tira
# essa pergunta do prompt do PO. Ligue ("true") quando o projeto entrar em produção.
PRODUCTION_ENABLED="false"
# TDD_ENABLED: "true" mantém a metodologia TDD nos prompts de Tech Leader, Desenvolvimento
# e Review. Use "false" só em projeto sem testes automatizados (ex: site em HTML estático):
# o plano passa a dizer como cada critério de aceite será verificado, e a suíte só roda se
# existir. Ao contrário das outras chaves *_ENABLED, vazio ou valor inválido mantêm a TDD
# ligada — só "false" desliga.
TDD_ENABLED="true"
# JAIL_ENABLED: "true" roda o claude de todas as etapas dentro do sandbox ai-jail: escrita
# só no diretório do projeto, o resto do sistema somente leitura e ~/.ssh, ~/.gnupg, ~/.aws
# e dados de navegador escondidos. Precisa do ai-jail e, no Linux, do bubblewrap (bwrap),
# instalados por você — veja a seção "Sandbox (ai-jail)" do README ou rode
# auto_scrum.sh --requisitos-jail. Se ligada e a jail não funcionar nesta máquina, o
# auto_scrum para antes de qualquer pergunta. A rede da jail é filtrada, e ela libera os
# MCPs remotos: mcp.atlassian.com quando JIRA_ENABLED="true" e mcp.sentry.dev
# quando SENTRY_ENABLED="true". Padrão é opt-in: vazio ou "false" roda o claude direto, como
# antes.
JAIL_ENABLED="false"
# JAIL_CLIPBOARD: só vale com JAIL_ENABLED="true". Quanto do terminal e do clipboard a jail
# libera; o auto_scrum mostra o nível a cada execução:
#   "restrito": não dá pra copiar de dentro do claude;
#   "medio" (ou vazio, o padrão): copia (--terminal-passthrough do ai-jail), mas o agente
#   pode escrever no seu clipboard e mandar sequências de controle ao terminal;
#   "brando": copia e o Ctrl+V do claude cola (--terminal-passthrough --display), mas o
#   agente pode LER o seu clipboard (senhas, tokens) e abrir janelas na sua tela.
# Nos três, cole com o botão direito → Colar. Outro valor faz o auto_scrum parar com erro.
# Veja a seção "Sandbox (ai-jail)" do README.
JAIL_CLIPBOARD=""
# JAIL_FORWARD_PORTS: só vale com JAIL_ENABLED="true". Portas de localhost da sua máquina
# que o claude alcança de dentro da jail (--forward-port do ai-jail), separadas por espaço —
# ex: o PostgreSQL do Docker que os testes usam. Exemplo:
#   JAIL_FORWARD_PORTS="54320 6379"
# Cada porta liberada dá ao agente acesso total ao serviço dela. Este arquivo é commitado
# com o projeto, então o time inteiro herda a lista. Vazio: nenhuma porta (o localhost da
# máquina fica inalcançável). Outro formato faz o auto_scrum parar com erro. Veja a seção
# "Sandbox (ai-jail)" do README.
JAIL_FORWARD_PORTS=""
# JAIL_ALLOW_HOSTS: só vale com JAIL_ENABLED="true". Hosts que o claude alcança de dentro
# da jail (--allow-host do ai-jail), separados por espaço — ex: a documentação da stack do
# projeto. Exemplo:
#   JAIL_ALLOW_HOSTS="docs.djangoproject.com tailwindcss.com"
# Cada host libera também os subdomínios dele. Já são liberados os hosts dos MCPs ligados
# acima e os da lista padrão do nick (lib/jail/allow_hosts.txt, com a documentação do
# Claude, de Python, Django etc.). Este arquivo é commitado com o projeto, então o time
# inteiro herda a lista. Vazio: nenhum host além desses. Outro formato (com https://, porta
# ou caminho) faz o auto_scrum parar com erro. Veja a seção "Sandbox (ai-jail)" do README.
JAIL_ALLOW_HOSTS=""
EOF
tail -n +4 "$skel_dir/skeleton.conf" > "$skel_dir/body.conf" 2>/dev/null || true
assert_eq "igual" "$(cmp -s "$skel_dir/body_expected.conf" "$skel_dir/body.conf" && echo igual || echo diferente)" "o corpo do esqueleto (sem as 3 linhas de cabeçalho) é idêntico byte a byte ao de antes, com a JIRA_URL do NS-53"
assert_eq "3" "$(grep -c '^# .*nick' <(head -n3 "$skel_dir/skeleton.conf" 2>/dev/null) || true)" "as 3 linhas de cabeçalho citam o nome passado a config_print_skeleton (nick)"

echo
echo "== registro: as chamadas ficam num bloco contíguo, dentro de config_register_nick_keys =="
# register_block_problems <arquivo> <chaves>: imprime um problema por linha (nada, se o
# registro de <arquivo> está certo), comparando com <chaves> (uma por linha, na ordem de
# CONFIG_KEYS). Não recorta o bloco por linha em branco: o comentário de uma chave pode ter
# linha vazia (o esqueleto a escreve como "#"). Usa os números de linha das próprias chamadas
# "config_register_key":
#   - há uma chamada por chave fora de comentário, e nenhuma outra;
#   - os nomes saem na ordem de <chaves>;
#   - toda chamada fica dentro do corpo de config_register_nick_keys() (da linha da
#     definição até o primeiro "}" na coluna 0 depois dela);
#   - entre a primeira e a última chamada não há definição de função nem atribuição CHAVE=.
register_block_problems() {
  local file="$1" expected_keys="$2" code calls first last start end
  code="$(grep -vE '^[[:space:]]*#' "$file" || true)"
  calls="$(grep -nE '^[[:space:]]*config_register_key [A-Z0-9_]+' "$file" || true)"
  if [ "$(grep -cw 'config_register_key' <<< "$code" || true)" != "$(grep -c . <<< "$expected_keys" || true)" ]; then
    echo "número de chamadas config_register_key diferente do número de chaves"
  fi
  if [ "$(sed -E 's/^[0-9]+:[[:space:]]*config_register_key ([A-Z0-9_]+).*/\1/' <<< "$calls")" != "$expected_keys" ]; then
    echo "nomes das chamadas fora da ordem de CONFIG_KEYS"
  fi
  first="$(head -n1 <<< "$calls" | cut -d: -f1)"
  last="$(tail -n1 <<< "$calls" | cut -d: -f1)"
  start="$(grep -nE '^config_register_nick_keys\(\) \{' "$file" | head -n1 | cut -d: -f1 || true)"
  if [ -z "$first" ] || [ -z "$start" ]; then
    echo "sem chamada de registro ou sem config_register_nick_keys() no arquivo"
    return 0
  fi
  end="$(awk -v start="$start" 'NR > start && /^\}/{print NR; exit}' "$file")"
  if [ -z "$end" ] || [ "$first" -le "$start" ] || [ "$last" -ge "$end" ]; then
    echo "chamada de registro fora de config_register_nick_keys()"
  fi
  if sed -n "${first},${last}p" "$file" | grep -qE '^([a-z_]+\(\)|[A-Z_]+=)'; then
    echo "código entre a primeira e a última chamada de registro"
  fi
}
registered_keys="$(run_keys "printf '%s\n' \"\${CONFIG_KEYS[@]}\"")"
assert_eq "true" "$([ -n "$registered_keys" ] && echo true || echo false)" "o registro tem pelo menos uma chave (amostra não vazia)"
assert_eq "" "$(register_block_problems "$KEYS_SH" "$registered_keys")" "o bloco de registro é contíguo, dentro de config_register_nick_keys, com uma chamada por chave na ordem de CONFIG_KEYS e nenhuma fora dele"
register_dir="$TEST_TMP/register_block"
mkdir -p "$register_dir"
# Regressão: linha vazia dentro do comentário da primeira chave (depois da 1ª linha do
# comentário) não quebra a checagem.
awk '{print} /config_register_key [A-Z]/{found++} found == 1 && !done && !/config_register_key [A-Z]/{print ""; done=1}' "$KEYS_SH" > "$register_dir/blank_comment_line.sh"
assert_eq "1" "$(diff "$KEYS_SH" "$register_dir/blank_comment_line.sh" | grep -c '^> $' || true)" "cópia de regressão: tem uma linha vazia a mais, dentro do bloco de registro"
assert_eq "" "$(register_block_problems "$register_dir/blank_comment_line.sh" "$registered_keys")" "linha vazia no comentário de uma chave: o bloco de registro continua válido"
# Controles negativos: uma chamada a mais no fim do arquivo (fora da função) e um registro no
# corpo do arquivo são pegos.
{ cat "$KEYS_SH"; printf '%s\n' "config_register_key LATE_ENABLED \"false\" \"\" 'LATE_ENABLED: tarde.'"; } > "$register_dir/late_key.sh"
assert_contains "$(register_block_problems "$register_dir/late_key.sh" "$registered_keys")" "número de chamadas config_register_key diferente do número de chaves" "chave registrada no fim do arquivo: a checagem acusa a chamada a mais"
sed -E 's/^config_register_nick_keys\(\) \{$/config_register_nick_keys_old() {/' "$KEYS_SH" > "$register_dir/no_function.sh"
assert_contains "$(register_block_problems "$register_dir/no_function.sh" "$registered_keys")" "sem chamada de registro ou sem config_register_nick_keys() no arquivo" "sem config_register_nick_keys(): a checagem acusa"

echo
echo "== esqueleto: amigável a lint/pre-commit e consistente com config_load =="
skel_file="$skel_dir/skeleton.conf"
skel_first_line="$(head -n1 "$skel_file" 2>/dev/null || true)"
assert_eq "sem shebang" "$(case "$skel_first_line" in '#!'*) echo 'com shebang' ;; *) echo 'sem shebang' ;; esac)" "esqueleto não começa com shebang"
assert_eq "0" "$(grep -c '[[:space:]]$' "$skel_file" 2>/dev/null || true)" "esqueleto não tem espaço (nem \\r) no fim de nenhuma linha"
assert_eq '\n' "$(tail -c1 "$skel_file" 2>/dev/null | od -An -c | tr -d ' ')" "esqueleto termina com \\n"

skeleton_result="$(run_loader "$skel_file" "$skel_dir/skeleton_loader" "STACK_DESCRIPTION=antes")"
assert_eq "|false|false|false|false" "$skeleton_result" "esqueleto carregado pelo leitor: flags false e STACK_DESCRIPTION vazio"
assert_eq "" "$(cat "$skel_dir/skeleton_loader.err" 2>/dev/null || echo 'sem stderr')" "esqueleto carregado pelo leitor não gera aviso (nem pelos comentários que citam CHAVE=\"valor\")"
assert_eq "true" "$(loaded_value "$skel_file" TDD_ENABLED)" "esqueleto carregado pelo leitor: TDD_ENABLED vem \"true\""

skeleton_keys="$(grep -oE '^[A-Z_]+=' "$skel_file" 2>/dev/null | tr -d '=' | sort || true)"
assert_eq "$(sort <<< "$registered_keys")" "$skeleton_keys" "chaves atribuídas no esqueleto são exatamente as de CONFIG_KEYS"

# Testes-guarda da regra liga/desliga (config_is_toggle_key, em lib/config/config.sh): o
# esqueleto é o modelo, então toda chave com "false" ou "true" nele precisa passar pela
# checagem de valor, e toda chave com "" não. skeleton_toggle_lines guarda a linha inteira,
# com o padrão da chave.
skeleton_guard_dir="$skel_dir/guard"
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
done < <(grep -E '^[A-Z_]+=' "$skel_file" 2>/dev/null || true)
assert_eq "" "$skeleton_other_lines" "toda chave do esqueleto é liga/desliga (=\"false\"/=\"true\") ou texto (=\"\")"
assert_eq "true" "$([ "${#skeleton_toggle_lines[@]}" -gt 0 ] && [ "${#skeleton_text_keys[@]}" -gt 0 ] && echo true || echo false)" "o esqueleto tem pelo menos uma chave liga/desliga e uma de texto"

for skeleton_line in "${skeleton_toggle_lines[@]}"; do
  skeleton_key="${skeleton_line%%=*}"
  skeleton_copy="$skeleton_guard_dir/toggle_$skeleton_key.conf"
  sed "s/^$skeleton_line\$/$skeleton_key=\"sim\"/" "$skel_file" > "$skeleton_copy"
  run_loader "$skeleton_copy" "$skeleton_guard_dir/toggle_$skeleton_key" >/dev/null
  assert_eq "true" "$(grep -qxF "$skeleton_key=\"sim\"" "$skeleton_copy" && echo true || echo false)" "esqueleto com $skeleton_line trocado por $skeleton_key=\"sim\": a cópia tem a linha trocada"
  assert_eq "1" "$(count_warnings "$skeleton_guard_dir/toggle_$skeleton_key.err")" "esqueleto com $skeleton_key=\"sim\": exatamente um aviso (toda liga/desliga do modelo é checada)"
  assert_contains "$(cat "$skeleton_guard_dir/toggle_$skeleton_key.err" 2>/dev/null || true)" "valor \"sim\" inválido para $skeleton_key" "esqueleto com $skeleton_key=\"sim\": o aviso é o de valor inválido"
done

for skeleton_key in "${skeleton_text_keys[@]}"; do
  skeleton_copy="$skeleton_guard_dir/text_$skeleton_key.conf"
  sed "s/^$skeleton_key=\"\"\$/$skeleton_key=\"sim\"/" "$skel_file" > "$skeleton_copy"
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
  if ! grep -qxF "$skeleton_line" "$skel_file"; then
    skeleton_unmatched_lines+="$skeleton_line"$'\n'
  fi
done <<< "$(grep -E '^  [A-Z_]+=' "$skeleton_guard_dir/empty.err" 2>/dev/null || true)"
assert_eq "" "$skeleton_unmatched_lines" "cada linha sugerida pelo aviso de chave ausente é idêntica à linha do esqueleto"
assert_eq "$registered_keys" "$(printf '%s' "$skeleton_suggested_keys")" "as chaves sugeridas pelo aviso de chave ausente são exatamente as de CONFIG_KEYS, na mesma ordem"

# Ida e volta: o bloco sugerido (o aviso menos a 1ª linha, inclusive as linhas de nota),
# colado como está num arquivo novo, carrega sem nenhum aviso.
tail -n +2 "$skeleton_guard_dir/empty.err" > "$skeleton_guard_dir/pasted.conf" 2>/dev/null || true
skeleton_pasted_result="$(run_loader "$skeleton_guard_dir/pasted.conf" "$skeleton_guard_dir/pasted" "STACK_DESCRIPTION=antes")"
assert_eq "|false|false|false|false" "$skeleton_pasted_result" "bloco do aviso de chave ausente colado num arquivo: as chaves carregam com o padrão do esqueleto"
assert_eq "" "$(cat "$skeleton_guard_dir/pasted.err" 2>/dev/null || echo 'sem stderr')" "bloco do aviso de chave ausente colado num arquivo: carrega sem nenhum aviso"

# Chave nova só no registro (config_register_key): entra nas duas checagens sem mudar a lógica.
skeleton_new_setup="config_register_key NEW_FLAG_ENABLED 'false' '' 'NEW_FLAG_ENABLED: flag nova.'; config_register_key NEW_TEXT '' '' 'NEW_TEXT: texto novo.'"
skeleton_new_file="$skeleton_guard_dir/new_keys.conf"
{ cat "$skel_file"; printf '%s\n' 'NEW_FLAG_ENABLED="sim"' 'NEW_TEXT="sim"'; } > "$skeleton_new_file"
skeleton_new_line="$(grep -nxF 'NEW_FLAG_ENABLED="sim"' "$skeleton_new_file" | cut -d: -f1)"
run_loader "$skeleton_new_file" "$skeleton_guard_dir/new_keys" "$skeleton_new_setup" >/dev/null
assert_eq "1" "$(count_warnings "$skeleton_guard_dir/new_keys.err")" "chaves novas NEW_FLAG_ENABLED/NEW_TEXT com \"sim\": exatamente um aviso"
assert_eq "$(value_warning "$skeleton_new_file:$skeleton_new_line" NEW_FLAG_ENABLED sim "$default_off_note")" "$(cat "$skeleton_guard_dir/new_keys.err" 2>/dev/null || true)" "chaves novas com \"sim\": só NEW_FLAG_ENABLED recebe o aviso de valor"

run_loader "$skel_file" "$skeleton_guard_dir/new_keys_missing" "$skeleton_new_setup" >/dev/null
assert_eq "1" "$(count_warnings "$skeleton_guard_dir/new_keys_missing.err")" "chaves novas ausentes do esqueleto: exatamente um aviso"
assert_eq "$(missing_warning "$skel_file" 'NEW_FLAG_ENABLED="false"' 'NEW_TEXT=""')" "$(cat "$skeleton_guard_dir/new_keys_missing.err" 2>/dev/null || true)" "chaves novas ausentes do esqueleto: o aviso de chave ausente lista as duas, com o padrão de cada tipo"

config_keys_blank > "$skeleton_guard_dir/blank.conf"
run_loader "$skeleton_guard_dir/blank.conf" "$skeleton_guard_dir/blank" >/dev/null
assert_eq "" "$(cat "$skeleton_guard_dir/blank.err" 2>/dev/null || echo 'sem stderr')" "config_keys_blank monta uma fixture completa: carrega sem nenhum aviso"

echo
echo "== JAIL_ENABLED: registrada na quarta posição a partir do fim, padrão \"false\", com comentário no esqueleto =="
jail_skeleton="$(cat "$skel_file" 2>/dev/null || true)"
assert_eq "JAIL_ENABLED"$'\n'"JAIL_CLIPBOARD"$'\n'"JAIL_FORWARD_PORTS"$'\n'"JAIL_ALLOW_HOSTS" "$(tail -n4 <<< "$registered_keys")" "JAIL_ENABLED, JAIL_CLIPBOARD, JAIL_FORWARD_PORTS e JAIL_ALLOW_HOSTS são as quatro últimas chaves de CONFIG_KEYS"
assert_contains "$jail_skeleton" 'JAIL_ENABLED="false"' "o esqueleto tem JAIL_ENABLED=\"false\""
jail_skeleton_comment="$(awk '/^# JAIL_ENABLED:/{found=1} /^JAIL_ENABLED=/{found=0} found' <<< "$jail_skeleton")"
for jail_word in ai-jail bubblewrap README --requisitos-jail JIRA_ENABLED SENTRY_ENABLED mcp.atlassian.com mcp.sentry.dev; do
  assert_contains "$jail_skeleton_comment" "$jail_word" "o comentário de JAIL_ENABLED no esqueleto cita $jail_word"
done
assert_eq "" "$(run_keys "config_key_note JAIL_ENABLED")" "JAIL_ENABLED não tem nota (o aviso de valor termina em \"a opção está desligada.\")"
jail_reg_dir="$TEST_TMP/jail_register"
mkdir -p "$jail_reg_dir"
printf '%s\n' "$(config_keys_blank JAIL_ENABLED)" 'JAIL_ENABLED="sim"' > "$jail_reg_dir/invalid.conf"
run_loader "$jail_reg_dir/invalid.conf" "$jail_reg_dir/invalid" >/dev/null
assert_eq "$(value_warning "$jail_reg_dir/invalid.conf:$(wc -l < "$jail_reg_dir/invalid.conf")" JAIL_ENABLED sim "$default_off_note")" "$(cat "$jail_reg_dir/invalid.err" 2>/dev/null || true)" "JAIL_ENABLED=\"sim\": aviso de valor, terminando em \"a opção está desligada.\""
config_keys_blank JAIL_ENABLED > "$jail_reg_dir/missing.conf"
run_loader "$jail_reg_dir/missing.conf" "$jail_reg_dir/missing" >/dev/null
assert_eq "$(missing_warning "$jail_reg_dir/missing.conf" 'JAIL_ENABLED="false"')" "$(cat "$jail_reg_dir/missing.err" 2>/dev/null || true)" "sem a linha JAIL_ENABLED: aviso de chave ausente sugerindo JAIL_ENABLED=\"false\""

echo
echo "== JAIL_CLIPBOARD: registrada na penúltima posição, chave de texto (padrão \"\"), sem nota, com comentário no esqueleto =="
assert_contains "$jail_skeleton" 'JAIL_CLIPBOARD=""' "o esqueleto tem JAIL_CLIPBOARD=\"\""
clip_skeleton_comment="$(awk '/^# JAIL_CLIPBOARD:/{found=1} /^JAIL_CLIPBOARD=/{found=0} found' <<< "$jail_skeleton")"
for clip_word in 'JAIL_ENABLED="true"' '"restrito"' '"medio"' '"brando"' --terminal-passthrough --display clipboard 'botão direito → Colar' Ctrl+V README; do
  assert_contains "$clip_skeleton_comment" "$clip_word" "o comentário de JAIL_CLIPBOARD no esqueleto cita $clip_word"
done
# Shift+Insert no terminal do VS Code (Linux) cola a seleção primária, não o clipboard.
assert_not_contains "$clip_skeleton_comment" "Shift+Insert" "o comentário de JAIL_CLIPBOARD não manda colar com Shift+Insert"
# jail_print_status mostra só o nível; o risco fica no comentário e no README.
assert_not_contains "$clip_skeleton_comment" "e o risco" "o comentário de JAIL_CLIPBOARD não promete mostrar o risco a cada execução"
assert_contains "$clip_skeleton_comment" "mostra o nível a cada execução" "o comentário de JAIL_CLIPBOARD diz que o auto_scrum mostra o nível a cada execução"
assert_eq "" "$(run_keys "config_key_note JAIL_CLIPBOARD")" "JAIL_CLIPBOARD não tem nota"
# Chave de texto: a config.sh não valida o valor (quem valida é jail_check, com a jail ligada).
printf '%s\n' "$(config_keys_blank JAIL_CLIPBOARD)" 'JAIL_CLIPBOARD="total"' > "$jail_reg_dir/clip_invalid.conf"
run_loader "$jail_reg_dir/clip_invalid.conf" "$jail_reg_dir/clip_invalid" >/dev/null
assert_eq "" "$(cat "$jail_reg_dir/clip_invalid.err" 2>/dev/null || true)" "JAIL_CLIPBOARD=\"total\": a carga da configuração não avisa (a validação é da jail)"
config_keys_blank JAIL_CLIPBOARD > "$jail_reg_dir/clip_missing.conf"
run_loader "$jail_reg_dir/clip_missing.conf" "$jail_reg_dir/clip_missing" >/dev/null
assert_eq "$(missing_warning "$jail_reg_dir/clip_missing.conf" 'JAIL_CLIPBOARD=""')" "$(cat "$jail_reg_dir/clip_missing.err" 2>/dev/null || true)" "sem a linha JAIL_CLIPBOARD: aviso de chave ausente sugerindo JAIL_CLIPBOARD=\"\""

echo
echo "== JAIL_FORWARD_PORTS: chave de texto (padrão \"\"), sem nota, com comentário no esqueleto =="
assert_contains "$jail_skeleton" 'JAIL_FORWARD_PORTS=""' "o esqueleto tem JAIL_FORWARD_PORTS=\"\""
ports_skeleton_comment="$(awk '/^# JAIL_FORWARD_PORTS:/{found=1} /^JAIL_FORWARD_PORTS=/{found=0} found' <<< "$jail_skeleton")"
for ports_word in 'JAIL_ENABLED="true"' localhost --forward-port 'JAIL_FORWARD_PORTS="54320 6379"' "separadas por espaço" "commitado" README; do
  assert_contains "$ports_skeleton_comment" "$ports_word" "o comentário de JAIL_FORWARD_PORTS no esqueleto cita $ports_word"
done
assert_eq "" "$(run_keys "config_key_note JAIL_FORWARD_PORTS")" "JAIL_FORWARD_PORTS não tem nota"
printf '%s\n' "$(config_keys_blank JAIL_FORWARD_PORTS)" 'JAIL_FORWARD_PORTS="abc"' > "$jail_reg_dir/ports_invalid.conf"
run_loader "$jail_reg_dir/ports_invalid.conf" "$jail_reg_dir/ports_invalid" >/dev/null
assert_eq "" "$(cat "$jail_reg_dir/ports_invalid.err" 2>/dev/null || true)" "JAIL_FORWARD_PORTS=\"abc\": a carga da configuração não avisa (a validação é da jail)"
config_keys_blank JAIL_FORWARD_PORTS > "$jail_reg_dir/ports_missing.conf"
run_loader "$jail_reg_dir/ports_missing.conf" "$jail_reg_dir/ports_missing" >/dev/null
assert_eq "$(missing_warning "$jail_reg_dir/ports_missing.conf" 'JAIL_FORWARD_PORTS=""')" "$(cat "$jail_reg_dir/ports_missing.err" 2>/dev/null || true)" "sem a linha JAIL_FORWARD_PORTS: aviso de chave ausente sugerindo JAIL_FORWARD_PORTS=\"\""

echo
echo "== JAIL_ALLOW_HOSTS: registrada por último, chave de texto (padrão \"\"), sem nota, com comentário no esqueleto =="
assert_eq 'JAIL_ALLOW_HOSTS=""' "$(tail -n1 <<< "$jail_skeleton")" "o esqueleto termina com JAIL_ALLOW_HOSTS=\"\""
hosts_skeleton_comment="$(awk '/^# JAIL_ALLOW_HOSTS:/{found=1} /^JAIL_ALLOW_HOSTS=/{found=0} found' <<< "$jail_skeleton")"
for hosts_word in 'JAIL_ENABLED="true"' --allow-host 'JAIL_ALLOW_HOSTS="docs.djangoproject.com tailwindcss.com"' "separados por espaço" "subdomínios" "lista padrão do nick" "lib/jail/allow_hosts.txt" "commitado" README; do
  assert_contains "$hosts_skeleton_comment" "$hosts_word" "o comentário de JAIL_ALLOW_HOSTS no esqueleto cita $hosts_word"
done
assert_eq "" "$(run_keys "config_key_note JAIL_ALLOW_HOSTS")" "JAIL_ALLOW_HOSTS não tem nota"
printf '%s\n' "$(config_keys_blank JAIL_ALLOW_HOSTS)" 'JAIL_ALLOW_HOSTS="https://x"' > "$jail_reg_dir/hosts_invalid.conf"
run_loader "$jail_reg_dir/hosts_invalid.conf" "$jail_reg_dir/hosts_invalid" >/dev/null
assert_eq "" "$(cat "$jail_reg_dir/hosts_invalid.err" 2>/dev/null || true)" "JAIL_ALLOW_HOSTS=\"https://x\": a carga da configuração não avisa (a validação é da jail)"
config_keys_blank JAIL_ALLOW_HOSTS > "$jail_reg_dir/hosts_missing.conf"
run_loader "$jail_reg_dir/hosts_missing.conf" "$jail_reg_dir/hosts_missing" >/dev/null
assert_eq "$(missing_warning "$jail_reg_dir/hosts_missing.conf" 'JAIL_ALLOW_HOSTS=""')" "$(cat "$jail_reg_dir/hosts_missing.err" 2>/dev/null || true)" "sem a linha JAIL_ALLOW_HOSTS: aviso de chave ausente sugerindo JAIL_ALLOW_HOSTS=\"\""

echo
echo "== JIRA_URL (NS-53): chave de texto logo depois de JIRA_ENABLED, a única com pergunta do init =="
jira_url_prompt='URL base dos links do Jira (opcional, Enter para pular; ex: https://empresa.atlassian.net/jira/browse/): '
assert_eq "JIRA_ENABLED JIRA_URL" "$(grep -oE '^(JIRA_ENABLED|JIRA_URL)$' <<< "$registered_keys" | paste -sd ' ' -)" "JIRA_URL vem logo depois de JIRA_ENABLED em CONFIG_KEYS"
assert_eq "$(($(grep -nx JIRA_ENABLED <<< "$registered_keys" | cut -d: -f1) + 1))" "$(grep -nx JIRA_URL <<< "$registered_keys" | cut -d: -f1)" "JIRA_URL está na posição seguinte à de JIRA_ENABLED"
assert_eq "JIRA_URL|$jira_url_prompt" "$(run_keys "for i in \"\${!CONFIG_KEYS[@]}\"; do [ -z \"\${CONFIG_KEY_PROMPTS[\$i]}\" ] || printf '%s|%s\n' \"\${CONFIG_KEYS[\$i]}\" \"\${CONFIG_KEY_PROMPTS[\$i]}\"; done")" "JIRA_URL é a única chave com pergunta do init, com o texto fixado"
assert_eq "" "$(run_keys "config_key_note JIRA_URL")" "JIRA_URL não tem nota"
assert_contains "$jail_skeleton" 'JIRA_URL=""' "o esqueleto tem JIRA_URL=\"\""
printf '%s\n' "$(config_keys_blank JIRA_URL)" 'JIRA_URL="qualquer coisa, sem validar"' > "$jail_reg_dir/jira_url_any.conf"
run_loader "$jail_reg_dir/jira_url_any.conf" "$jail_reg_dir/jira_url_any" >/dev/null
assert_eq "" "$(cat "$jail_reg_dir/jira_url_any.err" 2>/dev/null || true)" "JIRA_URL com qualquer texto: a carga não avisa (o formato não é validado)"
assert_eq "qualquer coisa, sem validar" "$(loaded_value "$jail_reg_dir/jira_url_any.conf" JIRA_URL)" "JIRA_URL é lida literalmente"
config_keys_blank JIRA_URL > "$jail_reg_dir/jira_url_missing.conf"
run_loader "$jail_reg_dir/jira_url_missing.conf" "$jail_reg_dir/jira_url_missing" >/dev/null
assert_eq "$(missing_warning "$jail_reg_dir/jira_url_missing.conf" 'JIRA_URL=""')" "$(cat "$jail_reg_dir/jira_url_missing.err" 2>/dev/null || true)" "sem a linha JIRA_URL: aviso de chave ausente sugerindo JIRA_URL=\"\""

echo
echo "== nenhuma chave de configuração nova: CONFIG_KEYS igual à de antes do ticket 21 =="
assert_eq "$nick_keys" "$(run_keys "printf '%s' \"\${CONFIG_KEYS[*]}\"")" "CONFIG_KEYS não ganhou chave além da JIRA_URL do NS-53 (as regras do claude não têm flag para desligar)"

echo
echo "== shellcheck -x em lib/config/keys.sh e nesta suíte =="
if command -v shellcheck > /dev/null 2>&1; then
  for checked in "$KEYS_SH" "$SCRIPT_DIR/test_keys.sh"; do
    sc_rc=0
    sc_out="$(shellcheck -x "$checked" 2>&1)" || sc_rc=$?
    assert_eq "0" "$sc_rc" "shellcheck -x limpo: $(basename "$checked")${sc_out:+ — $sc_out}"
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
