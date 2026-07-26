#!/usr/bin/env bash
#
# auto_scrum: dispara conversas Claude Code a partir de templates por tipo
# (PO, Tech Leader, Desenvolvimento, Review). Ver incio_projeto.md na raiz
# do repo para o plano completo.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMPLATES_DIR="$SCRIPT_DIR/templates"
LOGS_DIR="$SCRIPT_DIR/logs"
PROJETOS_DIR="$SCRIPT_DIR/projetos"

TITULO=""
DESCRICAO=""
EXTRA=""
ID_JIRA=""
PLANO_PATH=""
STACK_DESC=""
STACK_BLOCK_DEV=""
STACK_BLOCK_TL=""
TIPO=""

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

# read_required <prompt> <nome_da_variavel>
read_required() {
  local prompt="$1" __resultvar="$2" value=""
  while [ -z "$value" ]; do
    read -r -p "$prompt" value
    [ -z "$value" ] && echo "Esse campo é obrigatório, tente de novo."
  done
  printf -v "$__resultvar" '%s' "$value"
}

# read_optional <prompt> <nome_da_variavel>
read_optional() {
  local prompt="$1" __resultvar="$2" value=""
  read -r -p "$prompt" value
  printf -v "$__resultvar" '%s' "$value"
}

# read_via_editor <prompt> <nome_da_variavel>
# Abre $EDITOR (fallback: nano) num arquivo temporário para o texto ser
# digitado/colado lá, em vez de ler linha a linha no terminal — cola grande
# de texto direto no read trava/buga o terminal.
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

choose_tipo() {
  local opcoes=("PO - Criar ticket" "Tech Leader - Criar plano" "Desenvolvimento" "Review")
  local chaves=("po" "tech_leader" "desenvolvimento" "review")
  local opt
  echo "Qual tipo de conversa você quer iniciar?"
  select opt in "${opcoes[@]}"; do
    if [ -n "${opt:-}" ]; then
      TIPO="${chaves[$((REPLY - 1))]}"
      break
    fi
    echo "Opção inválida, tente novamente."
  done
}

listar_projetos() {
  local f encontrou=0
  echo "Projetos disponíveis em $PROJETOS_DIR/:"
  for f in "$PROJETOS_DIR"/*.sh; do
    [ -e "$f" ] || continue
    encontrou=1
    echo "  - $(basename "$f" .sh)"
  done
  [ "$encontrou" -eq 0 ] && echo "  (nenhum)"
}

# resolve_stack <nome_projeto_via_argv_ou_vazio>
# Define STACK_DESC a partir de auto_scrum/projetos/<nome>.sh (source). Se um nome vier
# por argumento, usa direto (erro se não existir). Senão, mostra um select com os
# projetos encontrados + uma opção "Nenhum" (deixa STACK_DESC vazio — quem monta o texto
# condicional pra sumir a menção de stack quando vazio é build_stack_blocks(), no main).
resolve_stack() {
  local projeto_arg="$1"
  local arquivo

  if [ -n "$projeto_arg" ]; then
    arquivo="$PROJETOS_DIR/$projeto_arg.sh"
    if [ ! -f "$arquivo" ]; then
      echo "Erro: projeto '$projeto_arg' não encontrado." >&2
      listar_projetos >&2
      exit 1
    fi
    # shellcheck disable=SC1090
    source "$arquivo"
    return
  fi

  local nomes=() f opt
  for f in "$PROJETOS_DIR"/*.sh; do
    [ -e "$f" ] || continue
    nomes+=("$(basename "$f" .sh)")
  done
  nomes+=("Nenhum")

  echo "Qual projeto (define a stack usada no prompt)?"
  select opt in "${nomes[@]}"; do
    if [ -z "${opt:-}" ]; then
      echo "Opção inválida, tente novamente."
      continue
    fi
    if [ "$opt" = "Nenhum" ]; then
      STACK_DESC=""
    else
      # shellcheck disable=SC1090
      source "$PROJETOS_DIR/$opt.sh"
    fi
    break
  done
}

# build_stack_blocks: monta STACK_BLOCK_DEV/STACK_BLOCK_TL a partir de STACK_DESC.
# Se STACK_DESC estiver vazio (nenhum projeto escolhido), os blocos viram só um ponto
# final — a frase de stack some inteira do prompt, em vez de aparecer com texto
# genérico. Feito aqui (bash), não no template, porque envsubst não tem condicional.
build_stack_blocks() {
  if [ -n "$STACK_DESC" ]; then
    printf -v STACK_BLOCK_DEV ', com habilidades principais em:\n%s.' "$STACK_DESC"
    printf -v STACK_BLOCK_TL ', especialista em %s.' "$STACK_DESC"
  else
    STACK_BLOCK_DEV="."
    STACK_BLOCK_TL="."
  fi
}

# init_projeto: pergunta o nome do projeto e cria um esqueleto em
# auto_scrum/projetos/<nome>.sh, só com as variáveis (vazias) que os templates esperam.
# O usuário abre e preenche depois.
init_projeto() {
  mkdir -p "$PROJETOS_DIR"
  local nome arquivo confirm
  read_required "Nome do projeto: " nome
  arquivo="$PROJETOS_DIR/$nome.sh"

  if [ -f "$arquivo" ]; then
    read -r -p "Já existe um projeto '$nome'. Sobrescrever? (s/N): " confirm
    case "$confirm" in
      s|S|sim|Sim|SIM) ;;
      *)
        echo "Cancelado. Nada foi alterado."
        return
        ;;
    esac
  fi

  cat > "$arquivo" <<'EOF'
# Config do projeto pro auto_scrum. Preencha as variáveis abaixo e salve.
# STACK_DESC: stack técnica usada no prompt do Tech Leader/Desenvolvimento
# (texto livre, sem ponto final no fim — o template já adiciona).
# Exemplo: STACK_DESC="Django avançado, django-tenants, Django ORM, PostgreSQL, pytest e TDD"
STACK_DESC=""
EOF

  echo "Projeto criado em $arquivo — edite antes de usar."
}

ask_questions_po() {
  read_required "Título do pedido: " TITULO
  read_via_editor "Descrição do pedido" DESCRICAO
  read_optional "Link do Sentry / erro externo (opcional, Enter para pular): " EXTRA
}

ask_questions_tech_leader() {
  read_required "ID do Jira (ex: SC-123): " ID_JIRA
  read_required "Título da tarefa: " TITULO
  read_via_editor "Descrição / contexto" DESCRICAO
}

ask_questions_desenvolvimento() {
  read_required "ID do Jira / branch (ex: SC-123): " ID_JIRA
  read_optional "Arquivo do plano (opcional, Enter para usar @docs/plans/${ID_JIRA}.md): " PLANO_PATH
  PLANO_PATH="${PLANO_PATH:-@docs/plans/${ID_JIRA}.md}"
}

ask_questions_review() {
  read_required "ID do Jira / branch (ex: SC-123): " ID_JIRA
  read_optional "Arquivo do plano (opcional, Enter para usar @docs/plans/${ID_JIRA}.md): " PLANO_PATH
  PLANO_PATH="${PLANO_PATH:-@docs/plans/${ID_JIRA}.md}"
}

main() {
  local projeto_arg="" arg init_flag=0
  for arg in "$@"; do
    case "$arg" in
      --projeto=*) projeto_arg="${arg#--projeto=}" ;;
      --init) init_flag=1 ;;
      *)
        echo "Erro: argumento desconhecido: '$arg'" >&2
        echo "Uso: $(basename "$0") [--projeto=<nome>] [--init]" >&2
        exit 1
        ;;
    esac
  done

  if [ "$init_flag" -eq 1 ]; then
    init_projeto
    exit 0
  fi

  check_requirements
  resolve_stack "$projeto_arg"
  build_stack_blocks
  choose_tipo

  case "$TIPO" in
    po) ask_questions_po ;;
    tech_leader) ask_questions_tech_leader ;;
    desenvolvimento) ask_questions_desenvolvimento ;;
    review) ask_questions_review ;;
  esac

  export TITULO DESCRICAO EXTRA ID_JIRA PLANO_PATH STACK_BLOCK_DEV STACK_BLOCK_TL

  mkdir -p "$LOGS_DIR"
  local timestamp log_file
  timestamp="$(date +%Y%m%d_%H%M%S)"
  log_file="$LOGS_DIR/${TIPO}_${timestamp}.md"

  envsubst '$TITULO $DESCRICAO $EXTRA $ID_JIRA $PLANO_PATH $STACK_BLOCK_DEV $STACK_BLOCK_TL' \
    < "$TEMPLATES_DIR/$TIPO.md" > "$log_file"

  echo
  echo "----- Prompt final (salvo em $log_file) -----"
  cat "$log_file"
  echo "----------------------------------------------"
  echo

  local confirm
  read -r -p "Confirma o envio para o claude? (s/N): " confirm
  case "$confirm" in
    s|S|sim|Sim|SIM)
      exec claude "$(cat "$log_file")"
      ;;
    *)
      echo "Cancelado. O prompt gerado continua salvo em: $log_file"
      exit 0
      ;;
  esac
}

main "$@"
