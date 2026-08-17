#!/usr/bin/env bash
#
# auto_scrum: dispara conversas Claude Code a partir de templates por tipo
# (PO, Tech Leader, Development, Review).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMPLATES_DIR="$SCRIPT_DIR/templates"
LOGS_DIR="$SCRIPT_DIR/logs"
PROJECTS_DIR="$SCRIPT_DIR/projects"

TITLE=""
DESCRIPTION=""
EXTRA=""
JIRA_ID=""
JIRA_LINK=""
PLAN_PATH=""
REVIEW_PATH=""
STACK_DESCRIPTION=""
STACK_BLOCK_DEV=""
STACK_BLOCK_TL=""
STACK_BLOCK_REVIEW=""
SENTRY_ENABLED=""
SENTRY_BLOCK_PO=""
SENTRY_BLOCK_TL=""
SENTRY_LINE_EXTRA=""
PERMISSION_ENABLED=""
PERMISSION_BLOCK_PO=""
JIRA_ENABLED=""
PO_VALIDATION_BLOCK=""
PO_TICKET_FORMAT_BLOCK=""
TECH_LEADER_PLAN_RULES_BLOCK=""
TYPE=""

check_requirements() {
  local missing=()
  local editor_bin="${EDITOR:-nano}"
  editor_bin="${editor_bin%% *}"
  command -v claude >/dev/null 2>&1 || missing+=("claude")
  command -v envsubst >/dev/null 2>&1 || missing+=("envsubst (pacote gettext)")
  command -v "$editor_bin" >/dev/null 2>&1 || missing+=("$editor_bin (defina \$EDITOR ou instale nano)")
  if [ "${#missing[@]}" -gt 0 ]; then
    echo "Erro: comando(s) necessário(s) não encontrado(s) no PATH: ${missing[*]}" >&2
    echo "Instale antes de continuar (ex: sudo pacman -S gettext para envsubst)." >&2
    exit 1
  fi
}

# read_required <prompt> <variable_name>
read_required() {
  local prompt="$1" __resultvar="$2" value=""
  while [ -z "$value" ]; do
    read -r -p "$prompt" value
    [ -z "$value" ] && echo "Esse campo é obrigatório, tente de novo."
  done
  printf -v "$__resultvar" '%s' "$value"
}

# read_optional <prompt> <variable_name>
read_optional() {
  local prompt="$1" __resultvar="$2" value=""
  read -r -p "$prompt" value
  printf -v "$__resultvar" '%s' "$value"
}

# read_via_editor <prompt> <variable_name>
# Opens $EDITOR (fallback: nano) on a temp file so the text can be typed/pasted
# there instead of read line-by-line in the terminal — pasting a large block of
# text straight into read locks up/glitches the terminal.
read_via_editor() {
  local prompt="$1" __resultvar="$2"
  local editor="${EDITOR:-nano}"
  local tmpfile value
  echo "$prompt — abrindo $editor (salve e feche o editor para continuar)."
  while true; do
    tmpfile="$(mktemp)"
    "$editor" "$tmpfile"
    if [ -s "$tmpfile" ]; then
      break
    fi
    rm -f "$tmpfile"
    echo "Esse campo é obrigatório, tente de novo."
  done
  value="$(cat "$tmpfile")"
  rm -f "$tmpfile"
  printf -v "$__resultvar" '%s' "$value"
}

# choose_type: monta o menu principal. As opções "po_jira" e "tech_leader_jira" só entram
# na lista quando JIRA_ENABLED="true" (setado no arquivo do projeto, opt-in — igual
# SENTRY_ENABLED/PERMISSION_ENABLED) — nem todo projeto alvo tem o MCP do Jira
# configurado, então não faz sentido oferecer uma opção que vai falhar de cara pra quem
# não tem. Cada uma entra logo após o par manual do seu papel (po_jira depois de
# po_discussion, tech_leader_jira depois de tech_leader).
choose_type() {
  local options=("PO - Criar ticket" "PO - Conversar para definir ticket")
  local keys=("po" "po_discussion")
  if [ "$JIRA_ENABLED" = "true" ]; then
    options+=("PO - Criar ticket a partir do Jira")
    keys+=("po_jira")
  fi
  options+=("Tech Leader - Criar plano")
  keys+=("tech_leader")
  if [ "$JIRA_ENABLED" = "true" ]; then
    options+=("Tech Leader - Criar plano a partir do Jira")
    keys+=("tech_leader_jira")
  fi
  options+=("Desenvolvimento" "Review")
  keys+=("development" "review")
  local opt
  echo "Qual tipo de conversa você quer iniciar?"
  select opt in "${options[@]}"; do
    if [ -n "${opt:-}" ]; then
      TYPE="${keys[$((REPLY - 1))]}"
      break
    fi
    echo "Opção inválida, tente novamente."
  done
}

list_projects() {
  local f found=0
  echo "Projetos disponíveis em $PROJECTS_DIR/:"
  for f in "$PROJECTS_DIR"/*.sh; do
    [ -e "$f" ] || continue
    found=1
    echo "  - $(basename "$f" .sh)"
  done
  [ "$found" -eq 0 ] && echo "  (nenhum)"
}

# resolve_stack <project_name_via_argv_or_empty>
# Sets STACK_DESCRIPTION from auto_scrum/projects/<name>.sh (source). If a name is
# passed as an argument, uses it directly (error if it doesn't exist). Otherwise, shows
# a select with the projects found + a "Nenhum" option (leaves STACK_DESCRIPTION empty —
# build_stack_blocks(), in main, is what assembles the conditional text that makes the
# stack mention disappear when empty).
resolve_stack() {
  local project_arg="$1"
  local file

  if [ -n "$project_arg" ]; then
    file="$PROJECTS_DIR/$project_arg.sh"
    if [ ! -f "$file" ]; then
      echo "Erro: projeto '$project_arg' não encontrado." >&2
      list_projects >&2
      exit 1
    fi
    # shellcheck disable=SC1090
    source "$file"
    return
  fi

  local names=() f opt
  for f in "$PROJECTS_DIR"/*.sh; do
    [ -e "$f" ] || continue
    names+=("$(basename "$f" .sh)")
  done
  names+=("Nenhum")

  echo "Qual projeto (define a stack usada no prompt)?"
  select opt in "${names[@]}"; do
    if [ -z "${opt:-}" ]; then
      echo "Opção inválida, tente novamente."
      continue
    fi
    if [ "$opt" = "Nenhum" ]; then
      STACK_DESCRIPTION=""
    else
      # shellcheck disable=SC1090
      source "$PROJECTS_DIR/$opt.sh"
    fi
    break
  done
}

# build_stack_blocks: assembles STACK_BLOCK_DEV/STACK_BLOCK_TL/STACK_BLOCK_REVIEW from
# STACK_DESCRIPTION. If STACK_DESCRIPTION is empty (no project chosen), the blocks become
# just a period — the stack sentence disappears entirely from the prompt, instead of
# showing generic text. Done here (bash), not in the template, because envsubst has no
# conditionals.
build_stack_blocks() {
  if [ -n "$STACK_DESCRIPTION" ]; then
    printf -v STACK_BLOCK_DEV ', com habilidades principais em:\n%s.' "$STACK_DESCRIPTION"
    printf -v STACK_BLOCK_TL ', especialista em %s.' "$STACK_DESCRIPTION"
    printf -v STACK_BLOCK_REVIEW ', especialista em %s.' "$STACK_DESCRIPTION"
  else
    STACK_BLOCK_DEV="."
    STACK_BLOCK_TL="."
    STACK_BLOCK_REVIEW="."
  fi
}

# build_sentry_blocks: assembles SENTRY_BLOCK_PO/SENTRY_BLOCK_TL from SENTRY_ENABLED
# (set in the project file, opt-in — empty/"false" is the default). When the project
# doesn't use Sentry, the mention disappears entirely from the PO and Tech Leader
# prompts, instead of showing generic text. SENTRY_LINE_EXTRA (the line with the link
# itself) is assembled separately, in ask_questions_po, since it depends on the EXTRA
# value typed by the user.
#
# SENTRY_BLOCK_PO also instructs the PO to use the Sentry MCP tool (the target project's
# own .mcp.json — auto_scrum doesn't configure or attach anything) to fetch the issue
# itself when a link is given instead of a typed description — the analyst reporting a
# Sentry error often doesn't know which flow produced it, so asking them to narrate it is
# asking for info they don't have. If the fetch fails, the instruction tells the PO to
# fall back to asking the analyst for a manually pasted traceback (treated from then on
# like a normal ticket).
build_sentry_blocks() {
  if [ "$SENTRY_ENABLED" = "true" ]; then
    SENTRY_BLOCK_PO='- Se o pedido tiver relação com Sentry ou outro link de erro externo, referencie-o no
  ticket (ou na explicação de rejeição, se for o caso).
- Se um link do Sentry foi informado e não há descrição escrita pelo analista, use a
  tool do Sentry (MCP) disponível para buscar os detalhes do issue (erro, stack trace,
  frequência, usuários afetados) e baseie sua análise e a descrição do ticket nisso.
- Se não conseguir acessar o Sentry (falha de autenticação, issue não encontrado, MCP
  indisponível), avise o analista e peça que ele descreva o problema manualmente,
  colando o traceback, para seguir como um pedido normal.'
    SENTRY_BLOCK_TL='Se o ticket tiver relação com Sentry ou outro link de erro externo, inclua a referência
no plano.'
  else
    SENTRY_BLOCK_PO=""
    SENTRY_BLOCK_TL=""
  fi
}

# build_permission_blocks: assembles PERMISSION_BLOCK_PO from PERMISSION_ENABLED (set in
# the project file, opt-in — empty/"false" is the default), same mechanism as
# build_sentry_blocks(). Not every target project has a permission system, so the
# question only shows up in the PO prompt when the project opts in.
build_permission_blocks() {
  if [ "$PERMISSION_ENABLED" = "true" ]; then
    printf -v PERMISSION_BLOCK_PO '\n- É necessário adicionar uma nova permissão para esse fluxo?'
  else
    PERMISSION_BLOCK_PO=""
  fi
}

# build_po_blocks: assembles PO_VALIDATION_BLOCK/PO_TICKET_FORMAT_BLOCK — the parts of
# po.md and po_discussion.md that must stay identical no matter which entry point
# triggered the PO (same role, same validity criteria), built once here instead of
# duplicated in both templates. Must run after build_sentry_blocks/build_permission_blocks,
# since the already-resolved SENTRY_BLOCK_PO/PERMISSION_BLOCK_PO values get embedded
# inside PO_VALIDATION_BLOCK.
build_po_blocks() {
  printf -v PO_VALIDATION_BLOCK '%s' "Considere o pedido válido quando, ao mesmo tempo:
- resolve um problema real de usuário ou de negócio;
- está dentro do propósito/escopo atual do sistema;
- o esforço e o risco envolvidos parecem proporcionais ao benefício.

Se faltar informação essencial para decidir (pedido vago, sem contexto suficiente),
pergunte antes de concluir — não presuma.

Análise obrigatória, independente do resultado:${PERMISSION_BLOCK_PO}
- Existe impacto em produção?
- Quais são os impactos negativos possíveis?
${SENTRY_BLOCK_PO}"

  printf -v PO_TICKET_FORMAT_BLOCK '%s' '## Título
## Descrição
## Critérios de aceite
- [ ] ...'
}

# build_tech_leader_blocks: assembles TECH_LEADER_PLAN_RULES_BLOCK — a parte de
# tech_leader.md e tech_leader_jira.md que precisa ficar idêntica não importa qual ponto
# de entrada acionou o Tech Leader (mesmo papel, mesmas regras de plano), montada uma vez
# aqui em vez de duplicada nos dois templates. Mesmo mecanismo de build_po_blocks()/
# PO_VALIDATION_BLOCK. Só entra aqui o que é igual nos dois templates — partes que
# dependem do dado de entrada específico (ex: a seção "DADOS DE ENTRADA" de cada um, ou a
# frase que cita o nome do arquivo do plano, que em tech_leader_jira.md não pode usar
# ${JIRA_ID} — ver comentário em tech_leader_jira.md) continuam com texto próprio em cada
# arquivo.
build_tech_leader_blocks() {
  printf -v TECH_LEADER_PLAN_RULES_BLOCK '%s' 'Se faltar informação técnica essencial para montar o plano, pergunte antes de concluir —
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
}

# init_project: asks for the project name and creates a skeleton in
# auto_scrum/projects/<name>.sh, with just the (empty) variables the templates expect.
# The user opens it and fills it in afterwards.
init_project() {
  mkdir -p "$PROJECTS_DIR"
  local name file confirm
  read_required "Nome do projeto: " name
  file="$PROJECTS_DIR/$name.sh"

  if [ -f "$file" ]; then
    read -r -p "Já existe um projeto '$name'. Sobrescrever? (s/N): " confirm
    case "$confirm" in
      s|S|sim|Sim|SIM) ;;
      *)
        echo "Cancelado. Nada foi alterado."
        return
        ;;
    esac
  fi

  cat > "$file" <<'EOF'
# Config do projeto pro auto_scrum. Preencha as variáveis abaixo e salve.
# STACK_DESCRIPTION: stack técnica usada no prompt do Tech Leader/Desenvolvimento
# (texto livre, sem ponto final no fim — o template já adiciona).
# Exemplo: STACK_DESCRIPTION="Django avançado, django-tenants, Django ORM, PostgreSQL, pytest e TDD"
STACK_DESCRIPTION=""
# SENTRY_ENABLED: "true" se esse projeto usa Sentry (ou outro link de erro externo) nos
# tickets/planos — os prompts de PO e Tech Leader passam a perguntar e mencionar isso.
# Padrão é opt-in: vazio ou "false" faz a menção a Sentry sumir dos prompts.
SENTRY_ENABLED="false"
# PERMISSION_ENABLED: "true" se esse projeto tem um sistema de permissões — o PO passa a
# avaliar, pra cada pedido, se será necessário adicionar uma nova permissão. Padrão é
# opt-in: vazio ou "false" tira essa pergunta do prompt do PO.
PERMISSION_ENABLED="false"
# JIRA_ENABLED: "true" se esse projeto tem o MCP do Jira configurado (.mcp.json do
# projeto alvo) — só então a opção "PO - Criar ticket a partir do Jira" aparece no menu
# principal. Padrão é opt-in: vazio ou "false" esconde a opção do menu.
JIRA_ENABLED="false"
EOF

  echo "Projeto criado em $file — edite antes de usar."
}

# ask_questions_po: pergunta o link do Sentry antes da descrição (quando o projeto usa
# Sentry) porque a resposta muda o que se pede depois — com link, a descrição vira
# opcional e curta, já que o PO vai buscar os detalhes reais via MCP (ver
# build_sentry_blocks); sem link, continua exigindo a descrição completa via editor.
ask_questions_po() {
  read_required "Título do pedido: " TITLE
  if [ "$SENTRY_ENABLED" = "true" ]; then
    read_optional "Link do Sentry / erro externo (opcional, Enter para pular): " EXTRA
  fi
  if [ -n "$EXTRA" ]; then
    SENTRY_LINE_EXTRA="Link do Sentry / erro externo: ${EXTRA}"
    read_optional "Descrição adicional (opcional, Enter para pular — o PO vai buscar os detalhes no Sentry): " DESCRIPTION
  else
    read_via_editor "Descrição do pedido" DESCRIPTION
  fi
}

# ask_questions_po_discussion: sem título/descrição prontos — só um assunto solto pra
# puxar a conversa. O template (po_discussion.md) instrui o PO a discutir antes de
# aplicar os critérios de validade, então não faz sentido exigir aqui o que a conversa
# ainda vai construir.
ask_questions_po_discussion() {
  read_required "Assunto para discutir: " TITLE
}

# ask_questions_po_jira: o pedido já existe como ticket no Jira — pede só o link
# (obrigatório) e uma descrição extra opcional. O PO (po_jira.md) é quem busca título,
# ID, descrição e anexos via MCP do Jira a partir do link; a descrição extra aqui serve
# de complemento manual do analista, não substitui a busca.
ask_questions_po_jira() {
  read_required "Link do ticket no Jira: " JIRA_LINK
  read_optional "Descrição extra (opcional, Enter para pular): " DESCRIPTION
}

ask_questions_tech_leader() {
  read_required "ID do Jira (ex: SC-123): " JIRA_ID
  read_required "Título da tarefa: " TITLE
  read_via_editor "Descrição / contexto" DESCRIPTION
}

# ask_questions_tech_leader_jira: o ticket já existe no Jira — pede só o link
# (obrigatório) e um contexto extra opcional. Reaproveita JIRA_LINK/DESCRIPTION (mesmas
# variáveis de ask_questions_po_jira) em vez de criar variáveis novas: JIRA_LINK já tem a
# semântica certa de URL de entrada, e DESCRIPTION já serve de complemento opcional nesse
# padrão. O Tech Leader (tech_leader_jira.md) é quem busca título, ID, descrição e anexos
# via MCP do Jira a partir do link — JIRA_ID não é pedido aqui, só é conhecido depois,
# dentro da própria sessão do Claude.
ask_questions_tech_leader_jira() {
  read_required "Link do ticket no Jira: " JIRA_LINK
  read_optional "Contexto extra (opcional, Enter para pular): " DESCRIPTION
}

ask_questions_development() {
  read_required "ID do Jira / branch (ex: SC-123): " JIRA_ID
  read_optional "Arquivo do plano (opcional, Enter para usar docs/plans/${JIRA_ID}_*.md): " PLAN_PATH
  PLAN_PATH="${PLAN_PATH:-docs/plans/${JIRA_ID}_*.md}"
}

# ask_questions_review: besides the plan (same glob pattern as Dev, since the exact name
# has the summary chosen by the Tech Leader), computes REVIEW_PATH — a fixed path, not
# asked of the user, since it's always the same kind of content (review result).
ask_questions_review() {
  read_required "ID do Jira / branch (ex: SC-123): " JIRA_ID
  read_optional "Arquivo do plano (opcional, Enter para usar docs/plans/${JIRA_ID}_*.md): " PLAN_PATH
  PLAN_PATH="${PLAN_PATH:-docs/plans/${JIRA_ID}_*.md}"
  REVIEW_PATH="docs/plans/${JIRA_ID}_review.md"
}

main() {
  local project_arg="" arg init_flag=0
  for arg in "$@"; do
    case "$arg" in
      --projeto=*) project_arg="${arg#--projeto=}" ;;
      --init) init_flag=1 ;;
      *)
        echo "Erro: argumento desconhecido: '$arg'" >&2
        echo "Uso: $(basename "$0") [--projeto=<nome>] [--init]" >&2
        exit 1
        ;;
    esac
  done

  if [ "$init_flag" -eq 1 ]; then
    init_project
    exit 0
  fi

  check_requirements
  resolve_stack "$project_arg"
  build_stack_blocks
  build_sentry_blocks
  build_permission_blocks
  build_po_blocks
  build_tech_leader_blocks
  choose_type

  case "$TYPE" in
    po) ask_questions_po ;;
    po_discussion) ask_questions_po_discussion ;;
    po_jira) ask_questions_po_jira ;;
    tech_leader) ask_questions_tech_leader ;;
    tech_leader_jira) ask_questions_tech_leader_jira ;;
    development) ask_questions_development ;;
    review) ask_questions_review ;;
  esac

  export TITLE DESCRIPTION EXTRA JIRA_ID JIRA_LINK PLAN_PATH REVIEW_PATH STACK_BLOCK_DEV \
    STACK_BLOCK_TL STACK_BLOCK_REVIEW SENTRY_BLOCK_PO SENTRY_BLOCK_TL SENTRY_LINE_EXTRA \
    PERMISSION_BLOCK_PO PO_VALIDATION_BLOCK PO_TICKET_FORMAT_BLOCK TECH_LEADER_PLAN_RULES_BLOCK

  mkdir -p "$LOGS_DIR"
  local timestamp log_file
  timestamp="$(date +%Y%m%d_%H%M%S)"
  log_file="$LOGS_DIR/${TYPE}_${timestamp}.md"

  # shellcheck disable=SC2016
  # Intentional single quotes: this is the variable list for envsubst to expand, we
  # don't want bash expanding it first.
  envsubst '$TITLE $DESCRIPTION $EXTRA $JIRA_ID $JIRA_LINK $PLAN_PATH $REVIEW_PATH $STACK_BLOCK_DEV $STACK_BLOCK_TL $STACK_BLOCK_REVIEW $SENTRY_BLOCK_PO $SENTRY_BLOCK_TL $SENTRY_LINE_EXTRA $PERMISSION_BLOCK_PO $PO_VALIDATION_BLOCK $PO_TICKET_FORMAT_BLOCK $TECH_LEADER_PLAN_RULES_BLOCK' \
    < "$TEMPLATES_DIR/$TYPE.md" > "$log_file"

  echo
  echo "----- Prompt final (salvo em $log_file) -----"
  cat "$log_file"
  echo "----------------------------------------------"
  echo

  local confirm
  read -r -p "Confirma o envio para o claude? (s/N): " confirm
  case "$confirm" in
    s|S|sim|Sim|SIM)
      local claude_args=()
      # Only the Development stage runs in auto mode — it's the only one where the
      # agent actually edits code/runs commands without stepping in at every turn;
      # PO/Tech Leader/Review are text conversations, no reason to let go of the wheel.
      [ "$TYPE" = "development" ] && claude_args+=(--permission-mode auto)
      # Nothing to wire here for Sentry: the MCP server itself lives in the target
      # project's own .mcp.json (Claude Code auto-loads it from the cwd), not in
      # auto_scrum. SENTRY_ENABLED only gates the prompt instruction (build_sentry_blocks)
      # telling the PO to use that tool when a link is given.
      exec claude "${claude_args[@]}" "$(cat "$log_file")"
      ;;
    *)
      echo "Cancelado. O prompt gerado continua salvo em: $log_file"
      exit 0
      ;;
  esac
}

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
  main "$@"
fi
