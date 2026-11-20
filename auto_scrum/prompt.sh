# shellcheck shell=bash
#
# prompt.sh: the prompt assembly of the auto_scrum stages (NS-35), loaded with source by
# auto_scrum.sh and by bin/nick-agent (the launcher of the agents of the nick shell), so both
# build the same prompt byte for byte: the answers and the blocks the templates use, the blocks
# built from the project flags (build_*_blocks, run by build_prompt_blocks), the templates
# expanded by envsubst and the exchange block at the end (render_prompt), plus jira_id_valid
# and warn_legacy_plan, which the answers go through, and the notice of a resumed agent whose
# ticket changed its ID (resume_id_notice, NS-39). The functions were moved from
# auto_scrum.sh without changing a line of their bodies.
#
# Not a library of lib/, on purpose (ABOUT.md, "Rust no projeto: etapas do auto_scrum na casca
# `nick` (NS-35)"): the build_*_blocks read and write the globals of the caller (the config keys
# and the blocks), which lib/ forbids, and rewriting them with parameters is the risk the move
# avoids. The rules it follows, in the spirit of lib/: no shebang, no exec bit, no set -euo
# pipefail (the caller's), and on load it only defines (globals, constants and functions),
# printing nothing and running nothing but the $(cd ...; pwd) of BASH_SOURCE; no exit, no source
# and no eval. It needs lib/config/config.sh loaded first (CONFIG_*_NAME), and the config keys
# it reads (STACK_DESCRIPTION, SENTRY_ENABLED, PERMISSION_ENABLED, PRODUCTION_ENABLED and
# TDD_ENABLED) are the caller's globals, written by config_load.

# The answers of the stage questions (ask_questions_* in auto_scrum.sh, or the --render
# arguments of bin/nick-agent).
TITLE=""
DESCRIPTION=""
JIRA_ID=""
JIRA_LINK=""
SENTRY_LINK=""
PLAN_PATH=""
REVIEW_PATH=""
# The blocks built from the project flags (build_prompt_blocks).
STACK_BLOCK_DEV=""
STACK_BLOCK_TL=""
STACK_BLOCK_REVIEW=""
SENTRY_BLOCK_PO=""
SENTRY_BLOCK_TL=""
PERMISSION_BLOCK_PO=""
PRODUCTION_BLOCK_PO=""
TDD_BLOCK_TL=""
TDD_CRITERIA_BLOCK_TL=""
TDD_BLOCK_DEV=""
TDD_BLOCK_REVIEW=""
PO_VALIDATION_BLOCK=""
PO_TICKET_FORMAT_BLOCK=""
TECH_LEADER_PLAN_RULES_BLOCK=""
# The nick dir in the target project (the cwd): the project config (.nick/nick.conf, read by
# lib/config/config.sh), the exchange dir and the per-ticket docs. The names come from the
# library (CONFIG_*_NAME, lib/config/config.sh), the single source; config_dir_prepare creates
# the tree with a single .gitignore that ignores everything but itself and nick.conf.
NICK_DIR="$CONFIG_NICK_DIR_NAME"
# Exchange dir: where the user and the agents trade files (screenshots, attachments), announced
# at the end of every prompt (exchange_block).
EXCHANGE_DIR="$CONFIG_EXCHANGE_DIR_NAME"
# Docs dir: plan, review and validation screenshots of each ticket, in $NICK_DOCS_DIR/<ID>/
# (created by the agent when it writes there).
NICK_DOCS_DIR="$CONFIG_DOCS_DIR_NAME"
# The templates of the stages (auto_scrum/templates/<type>.md) and the dir of the prompt logs
# (auto_scrum/logs/), next to this file, absolute (the cd runs in the subshell of $(...), and its
# output, which exists with CDPATH set, goes to /dev/null). auto_scrum.sh keeps its own
# TEMPLATES_DIR and LOGS_DIR (its tests override LOGS_DIR); bin/nick-agent uses these.
# NICK_PROMPT_LOGS_DIR, when absolute, replaces the logs dir: the tests of bin/nick-agent and of
# the nick shell point it at a temporary dir, so a test never writes in auto_scrum/logs/ (which
# has real data). A relative or empty value is ignored.
# shellcheck disable=SC2034
# Used by bin/nick-agent, which loads this file (shellcheck checks this file alone).
PROMPT_TEMPLATES_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" > /dev/null && pwd)/templates"
PROMPT_LOGS_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" > /dev/null && pwd)/logs"
# shellcheck disable=SC2034
# Used by bin/nick-agent (see above).
case "${NICK_PROMPT_LOGS_DIR:-}" in
  /*) PROMPT_LOGS_DIR="$NICK_PROMPT_LOGS_DIR" ;;
esac

# jira_id_valid <id>: returns 0 when <id> can name the ticket's docs dir ($NICK_DOCS_DIR/<id>/),
# 1 otherwise, with no output. Refuses any /, any .., exactly ".", any whitespace and any
# control character ([:cntrl:] and not [:print:], so an accent isn't refused under LC_ALL=C —
# same reason as config_warn_invalid_toggle). Empty is read_required's job.
jira_id_valid() {
  case "$1" in
    "" | . | */* | *..* | *[[:space:]]* | *[[:cntrl:]]*) return 1 ;;
  esac
  return 0
}


# build_stack_blocks: assembles STACK_BLOCK_DEV/STACK_BLOCK_TL/STACK_BLOCK_REVIEW from
# STACK_DESCRIPTION. If STACK_DESCRIPTION is empty (the project didn't fill it in), the
# blocks become just a period — the stack sentence disappears entirely from the prompt,
# instead of showing generic text. Done here (bash), not in the template, because envsubst
# has no conditionals.
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
# prompts, instead of showing generic text.
#   - SENTRY_BLOCK_PO only tells the PO to reference a Sentry/external error link in the
#     ticket. It is embedded in PO_VALIDATION_BLOCK (build_po_blocks), so it reaches every
#     PO entry point — which is why it must not carry anything that only makes sense when a
#     Sentry link was given.
#   - SENTRY_BLOCK_TL tells the Tech Leader to carry that reference into the plan.
# The instructions to fetch the issue via the Sentry MCP and the fallback when that fails
# live as fixed text in po_sentry.md, the only template that has a Sentry link as input.
# SENTRY_ENABLED's other effect, showing that entry point in the menu, is read directly in
# choose_type().
build_sentry_blocks() {
  if [ "$SENTRY_ENABLED" = "true" ]; then
    SENTRY_BLOCK_PO='- Se o pedido tiver relação com Sentry ou outro link de erro externo, referencie-o no
  ticket (ou na explicação de rejeição, se for o caso).'
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


# build_production_blocks: assembles PRODUCTION_BLOCK_PO from PRODUCTION_ENABLED (set in
# the project file, opt-in — empty/"false" is the default), same mechanism as
# build_permission_blocks(). A project that isn't in production would make the model
# invent a production impact analysis that doesn't exist, so the question only shows up
# in the PO prompt when the project opts in. The block is embedded in PO_VALIDATION_BLOCK
# (build_po_blocks), so it is neither exported nor passed to envsubst.
build_production_blocks() {
  if [ "$PRODUCTION_ENABLED" = "true" ]; then
    printf -v PRODUCTION_BLOCK_PO '\n- Existe impacto em produção?'
  else
    PRODUCTION_BLOCK_PO=""
  fi
}


# build_tdd_blocks: assembles TDD_BLOCK_TL/TDD_CRITERIA_BLOCK_TL/TDD_BLOCK_DEV/
# TDD_BLOCK_REVIEW from TDD_ENABLED (set in the project file), same mechanism as
# build_production_blocks(). TDD_ENABLED is the only flag that is on by default: only "false"
# (exact) turns TDD off, so a typo ("TRUE", "sim", " false") never removes TDD from a project
# that has tests — any other value, empty included, keeps the text of before the flag, byte
# for byte. A project without automated tests (e.g. a static HTML site) uses
# "false": the plan then says how each acceptance criterion will be verified, and Dev and
# Review only run a test suite if the project has one.
#   - TDD_BLOCK_TL and TDD_CRITERIA_BLOCK_TL are embedded in TECH_LEADER_PLAN_RULES_BLOCK
#     (build_tech_leader_blocks, which must run after this), so they are neither exported
#     nor passed to envsubst. TDD_CRITERIA_BLOCK_TL starts with \n, so it leaves no blank
#     line behind when empty;
#   - TDD_BLOCK_DEV and TDD_BLOCK_REVIEW are referenced by development.md and review.md.
build_tdd_blocks() {
  if [ "$TDD_ENABLED" = "false" ]; then
    TDD_BLOCK_TL='Quem vai executar o plano é um(a) desenvolvedor(a) IA em outra sessão — não é você. O
plano não deve exigir testes automatizados nem planejar infraestrutura de testes que o
ticket não peça.'
    printf -v TDD_CRITERIA_BLOCK_TL '\n%s' 'Para cada critério, diga como ele será verificado (por exemplo, verificação manual ou
visual, com o passo a passo) — é por essa descrição que o review vai validá-lo.'
    TDD_BLOCK_DEV='Garanta que todos os critérios de aceite do plano sejam atendidos, verificando cada um
pela forma descrita no plano.

Antes de finalizar, se o projeto tiver uma suíte de testes automatizados, rode-a completa
e confirme que passa. Se não tiver, não crie testes nem infraestrutura de testes que o
plano não peça.

No final, traga um resumo do que foi feito e, para cada critério de aceite, como ele foi
verificado.'
    TDD_BLOCK_REVIEW='Isso
inclui, no mínimo: rodar a suíte de testes, se o projeto tiver uma, e confirmar que
passam; e validar cada critério de aceite do plano pela forma de verificação descrita
nele.'
  else
    TDD_BLOCK_TL='O plano deve ser pensado para ser executado usando a metodologia TDD. Quem vai executar o
plano é um(a) desenvolvedor(a) IA em outra sessão — não é você.'
    TDD_CRITERIA_BLOCK_TL=""
    TDD_BLOCK_DEV='Siga a metodologia TDD e garanta que todos os critérios de aceite do plano sejam
atendidos.

Antes de finalizar, rode a suíte completa de testes (não só os novos) e confirme que
passam.

No final, traga um resumo do que foi feito, incluindo os testes criados/alterados.'
    TDD_BLOCK_REVIEW='Isso
inclui, no mínimo: rodar a suíte de testes e confirmar que passam, e validar se os
critérios de aceite do plano foram cumpridos.'
  fi
}


# build_po_blocks: assembles PO_VALIDATION_BLOCK/PO_TICKET_FORMAT_BLOCK — the parts of
# the four PO templates that must stay identical no matter which entry point triggered
# the PO (same role, same validity criteria), built once here instead of duplicated in
# each template. Must run after build_sentry_blocks/build_permission_blocks/
# build_production_blocks, since the already-resolved SENTRY_BLOCK_PO/PERMISSION_BLOCK_PO/
# PRODUCTION_BLOCK_PO values get embedded inside PO_VALIDATION_BLOCK.
build_po_blocks() {
  printf -v PO_VALIDATION_BLOCK '%s' "Considere o pedido válido quando, ao mesmo tempo:
- resolve um problema real de usuário ou de negócio;
- está dentro do propósito/escopo atual do sistema;
- o esforço e o risco envolvidos parecem proporcionais ao benefício.

Se faltar informação essencial para decidir (pedido vago, sem contexto suficiente),
pergunte antes de concluir — não presuma.

Análise obrigatória, independente do resultado:${PERMISSION_BLOCK_PO}${PRODUCTION_BLOCK_PO}
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
# arquivo. Precisa rodar depois de build_tdd_blocks, porque embute TDD_BLOCK_TL e
# TDD_CRITERIA_BLOCK_TL já resolvidos (o trecho de TDD depende de TDD_ENABLED).
build_tech_leader_blocks() {
  printf -v TECH_LEADER_PLAN_RULES_BLOCK '%s' "Se faltar informação técnica essencial para montar o plano, pergunte antes de concluir —
não presuma.

Antes de planejar, procure no projeto o que já existe e pode ser reaproveitado: classes,
funções, componentes e afins; bibliotecas que o projeto já usa como dependência (não planeje
uma nova que faça o mesmo); e os padrões e convenções da base (o código novo deve ser escrito
do mesmo jeito que o código parecido que já existe). Prefira reaproveitar a criar algo novo.
Se o existente quase serve, prefira adaptá-lo: a adaptação entra no escopo do ticket, mas não
pode mudar o comportamento do que já funciona. Reaproveite só quando houver reuso real — a
semelhança superficial não justifica forçar uma abstração comum. O plano deve dizer o que
será reaproveitado, qual adaptação no código existente é necessária e como verificar que o
comportamento anterior se manteve; inclua essa verificação nos critérios de aceite.

${TDD_BLOCK_TL}

O plano deve terminar com estas duas seções:
## Critérios de aceite
Se o ticket já trouxer critérios de aceite na descrição, não copie cegamente: avalie
cada um à luz do plano técnico. Se algum não fizer mais sentido, remova e explique o
porquê; se faltar algo que o plano exige, adicione. Essa seção é obrigatória — a etapa
de review usa ela para validar o trabalho feito.${TDD_CRITERIA_BLOCK_TL}
## Habilidades necessárias
As habilidades que o(a) desenvolvedor(a) precisa para executar o serviço."
}


# build_prompt_blocks: builds every block of the prompt from the project flags, in the order
# main() of auto_scrum.sh always called them: build_po_blocks after the sentry, permission and
# production blocks it embeds, and build_tech_leader_blocks after build_tdd_blocks.
build_prompt_blocks() {
  build_stack_blocks
  build_sentry_blocks
  build_permission_blocks
  build_production_blocks
  build_po_blocks
  build_tdd_blocks
  build_tech_leader_blocks
}

# exchange_block: the section that closes every stage's prompt, telling the agent where to
# receive and deliver files. Outside the project the agent may see nothing (the jail), so files
# the user mentions elsewhere (~/Downloads) are to be copied into the exchange dir.
exchange_block() {
  cat <<EOF
## Troca de arquivos com o usuário

Use a pasta \`$EXCHANGE_DIR/\`, na raiz do projeto, para trocar arquivos com o usuário:
- é lá que ele coloca o que quer que você veja (prints, anexos, exemplos); quando ele citar um
  arquivo de fora do projeto (ex: em ~/Downloads), peça para ele copiar para essa pasta, porque
  fora do projeto você pode não ter acesso (ex: com a jail do nick ligada);
- é lá que você salva o que entregar para ele conferir (ex: relatórios), menos o que o prompt
  mandar gravar em outro lugar (ex: os prints de validação de um ticket).
O conteúdo dessa pasta não é versionado (o nick cria em \`$NICK_DIR/\` um .gitignore que ignora tudo
ali, menos a configuração).
EOF
}


# render_prompt <templates_dir> <type> <log_file>: writes to <log_file> the prompt of <type>:
# <templates_dir>/<type>.md expanded by envsubst with the answers and the blocks (exported for
# it), then a blank line and the exchange block. Run after build_prompt_blocks and the answers.
render_prompt() {
  local templates_dir="$1" type="$2" log_file="$3"
  export TITLE DESCRIPTION JIRA_ID JIRA_LINK SENTRY_LINK PLAN_PATH REVIEW_PATH STACK_BLOCK_DEV \
    STACK_BLOCK_TL STACK_BLOCK_REVIEW SENTRY_BLOCK_TL PO_VALIDATION_BLOCK PO_TICKET_FORMAT_BLOCK \
    TECH_LEADER_PLAN_RULES_BLOCK TDD_BLOCK_DEV TDD_BLOCK_REVIEW

  # shellcheck disable=SC2016
  # Intentional single quotes: this is the variable list for envsubst to expand, we
  # don't want bash expanding it first.
  envsubst '$TITLE $DESCRIPTION $JIRA_ID $JIRA_LINK $SENTRY_LINK $PLAN_PATH $REVIEW_PATH $STACK_BLOCK_DEV $STACK_BLOCK_TL $STACK_BLOCK_REVIEW $SENTRY_BLOCK_TL $PO_VALIDATION_BLOCK $PO_TICKET_FORMAT_BLOCK $TECH_LEADER_PLAN_RULES_BLOCK $TDD_BLOCK_DEV $TDD_BLOCK_REVIEW' \
    < "$templates_dir/$type.md" > "$log_file"
  { echo; exchange_block; } >> "$log_file"
}

# resume_id_notice <old> <new>: prints the notice a resumed agent gets (NS-39, through
# --append-system-prompt of bin/nick-agent --resume) when the ID of its ticket changed after its
# conversation started ("Editar ID", NS-36): the folder of the ticket is now $NICK_DOCS_DIR/<new>/.
# Here, and not in the nick shell, because the text is prompt, and no Rust builds a prompt.
resume_id_notice() {
  echo "O ID deste ticket mudou de $1 para $2 depois que esta conversa começou. A pasta do ticket agora é $NICK_DOCS_DIR/$2/ (antes era $NICK_DOCS_DIR/$1/): use o caminho novo para o plano, o review e os artefatos."
}

# warn_legacy_plan: before this ticket, plans lived in docs/plans/<ID>_<summary>.md. When
# $NICK_DOCS_DIR/$JIRA_ID/plan.md doesn't exist, warns on stderr (one line each) about every
# docs/plans/<ID>_*.md in the cwd but <ID>_review.md (a review, not a plan), so the user can
# type that path in the plan question that follows. Nothing is migrated. The ID is quoted in
# the glob, and an unmatched glob is skipped.
warn_legacy_plan() {
  local new_plan="$NICK_DOCS_DIR/$JIRA_ID/plan.md" f
  [ -e "$new_plan" ] && return 0
  for f in docs/plans/"$JIRA_ID"_*.md; do
    [ -e "$f" ] || continue
    [ "$f" = "docs/plans/${JIRA_ID}_review.md" ] && continue
    echo "Aviso: $new_plan não existe, mas há um plano no lugar antigo: $f — para usá-lo, informe esse caminho na pergunta abaixo." >&2
  done
}

