# Sobre o projeto

Decisões e convenções que não são óbvias só de ler o código.

## Idioma dos prompts

Os templates do `auto_scrum` (`auto_scrum/templates/*.md`) ficam em português (PT-BR),
tanto as instruções quanto os dados de entrada — não em inglês.

**Por quê:** para as tarefas desses templates (avaliar ticket, montar plano, revisar
código), a vantagem de seguir instruções em inglês é pequena o suficiente pra não
compensar. O que pesa mais é a manutenção: quem edita esses templates é o próprio
usuário, e ter tudo em PT-BR evita a fricção de misturar instrução em inglês com dado em
português. Termos técnicos (TDD, ORM, CBV etc.) continuam em inglês naturalmente dentro
do texto, que é o que importa pro modelo entender contexto de código.

## Idioma do código

Por convenção, todo o código fica em inglês — o oposto do que vale pros templates (seção
acima). Isso inclui não só nomes de variável/função e comentários do `auto_scrum.sh`, mas
também nomes de diretório/arquivo internos (`auto_scrum/projects/`, `templates/*.md`) e os
valores de `$TYPE` (`po`, `tech_leader`, `development`, `review`). Fica em português só o
que é visual pro usuário: os textos do menu (`select`), prompts de `read -p`, mensagens de
erro/confirmação, a flag `--projeto=`, e o conteúdo em si dos templates (`.md`) — que o
usuário lê/edita diretamente.

**Por quê:** identificadores, comentários e nomes de arquivo/diretório são "estrutura do
projeto", lida só por quem mexe no código — inglês é a convenção comum pra isso. Já o que
aparece na tela ou é digitado pelo usuário (menus, prompts, o texto dos templates) é
conteúdo voltado a ele, e trocar isso pra inglês só atrapalharia sem ganho real.

## Qualidade de código bash

O projeto é 100% bash. Regras a seguir em todo script novo ou alterado:

- `set -euo pipefail` no topo do script.
- Aspas em toda expansão de variável (`"$var"`, `"$@"`) — nunca `$var` solto.
- `local` em toda variável declarada dentro de função.
- `[ ]` para testes condicionais (não `[[ ]]`) — é o que já está em uso no
  `auto_scrum.sh`, então mantemos o padrão em vez de misturar os dois estilos.
- `$(...)` em vez de crase para comandos aninhados.
- Nomenclatura: `MAIUSCULO` para variáveis globais/exportadas (ex: `TITLE`,
  `LOGS_DIR`), `minusculo` para variáveis locais e nomes de função.
- Antes de considerar um script pronto, rodar `shellcheck` nele manualmente
  (`sudo pacman -S shellcheck` se ainda não instalado). Quando um aviso for
  falso positivo intencional (ex: `source` de arquivo dinâmico), suprimir com
  `# shellcheck disable=SCxxxx` acompanhado do porquê, como já feito em
  `resolve_stack()`.

**Por quê:** essas regras evitam as três classes de bug mais comuns em bash —
variável não citada que quebra com espaço/glob, variável de função vazando pro
escopo global, e falha de comando ignorada silenciosamente por causa do
`set -e` ausente.

## Convenção do arquivo de plano (Tech Leader → Dev/Review)

O plano gerado pelo template `tech_leader.md` vai em
`docs/plans/${JIRA_ID}_<resumo-curto>.md` — o resumo curto (kebab-case) é escolhido pelo
próprio Tech Leader a partir do título da tarefa, só para facilitar a leitura humana do
nome do arquivo; o identificador único continua sendo o `JIRA_ID`. `development.md`
acha o plano pelo padrão `docs/plans/${JIRA_ID}_*.md` — é o default de `PLAN_PATH` em
`ask_questions_development`/`ask_questions_review` no `auto_scrum.sh`, então
normalmente não precisa ser informado manualmente. O plano também precisa terminar com
duas seções fixas: `## Critérios de aceite` (obrigatória — herdada do ticket ou definida
pelo Tech Leader) e `## Habilidades necessárias`.

A etapa de Review grava o próprio resultado (aprovado, ou lista de problemas) em
`docs/plans/${JIRA_ID}_review.md` — path fixo, calculado a partir de `JIRA_ID` em
`ask_questions_review` (não é perguntado ao usuário, pois é sempre o mesmo tipo de
conteúdo). Esse mesmo path é o que `review.md` instrui a ignorar ao procurar o plano
pelo padrão `${JIRA_ID}_*.md` numa segunda rodada — senão o review anterior seria
confundido com o plano.

## Flags booleanas por projeto (ex: SENTRY_ENABLED)

Nem toda referência de prompt vale para todo projeto (ex: nem todo projeto usa Sentry).
Esse tipo de coisa vira uma variável booleana no arquivo `auto_scrum/projects/<name>.sh`
(`SENTRY_ENABLED="true"/"false"`, opt-in — vazio conta como `"false"`), lida por uma função
`build_*_blocks()` em `auto_scrum.sh` (mesmo padrão de `build_stack_blocks()`/
`STACK_DESCRIPTION`) que monta o bloco de texto correspondente só quando a flag está ativa. O
template usa a variável de bloco (`${SENTRY_BLOCK_PO}`, `${SENTRY_BLOCK_TL}`) no lugar do
texto fixo — nunca um `if` dentro do `.md`, porque `envsubst` não suporta condicional.

**Por quê:** manter esse tipo de decisão condicional em bash, não no template, é o mesmo
racional de `build_stack_blocks()` — e opt-in (padrão desligado) evita que um projeto novo
criado via `--init` puxe menção a uma ferramenta que ele não usa sem querer.

## auto_scrum roda com o cwd dentro do projeto alvo

`auto_scrum.sh` não recebe (nem precisa de) um path do projeto alvo como argumento — ele
espera ser executado com o diretório de trabalho já dentro do repositório desse projeto
(ex: `cd ~/Projetos/git/siga-construcao && /caminho/pra/auto_scrum.sh`). O script não
verifica nem avisa se isso não for respeitado; rodar de outro lugar simplesmente faz o
`claude` (chamado via `exec claude ...` no fim do script) subir sem o `.mcp.json`,
`CLAUDE.md` e demais config do projeto errado — ou sem nenhuma, se rodado de um
diretório qualquer.

**Por quê:** é o cwd que faz o Claude Code carregar sozinho o `.mcp.json` do projeto alvo
(é assim que a etapa de PO enxerga o MCP do Sentry — ver seção "Sentry via MCP na etapa
de PO" abaixo — sem o `auto_scrum` precisar saber nada sobre ele) e o `CLAUDE.md`/
`ABOUT.md` daquele projeto. Também é o que permite a etapa de Desenvolvimento editar o
código de verdade: ela roda `claude --permission-mode auto` no mesmo cwd, então as
mudanças caem no repositório certo. `--projeto=<nome>` (flag do `auto_scrum.sh`) escolhe
só a *stack descrita no prompt* (`STACK_DESCRIPTION`/`SENTRY_ENABLED` de
`projects/<name>.sh`) — não tem relação com em qual diretório o comando roda, são coisas
independentes que coincidentemente usam o mesmo nome de projeto.

## Modo auto só na etapa de Desenvolvimento

`auto_scrum.sh` sobe o `claude` com `--permission-mode auto` (aprova a maioria das
chamadas de ferramenta sozinho, exceto o que estiver em deny/ask do `settings.json`)
somente quando `$TYPE = development`. PO, Tech Leader e Review sobem no modo padrão
(interativo, pedindo confirmação).

**Por quê:** Desenvolvimento é a única etapa em que o agente de fato edita código e roda
comandos — as outras três são conversas de avaliação/planejamento/revisão em texto, sem
motivo pra soltar a supervisão. Regras de `deny` no `settings.json` continuam valendo em
qualquer modo (deny > ask > allow > modo da sessão), então `auto` não contorna o que já
estiver bloqueado ali — mas hoje o projeto não tem nenhuma regra de `deny` configurada,
só `allow`. Vale definir uma deny list antes de confiar demais no modo `auto`.

## Sentry via MCP na etapa de PO

Quando `SENTRY_ENABLED="true"`, `ask_questions_po` pergunta o link do Sentry antes da
descrição. Se um link for informado, a descrição vira opcional (campo curto, não
editor) — o `po.md` (bloco `SENTRY_BLOCK_PO`, montado em `build_sentry_blocks()`)
instrui o PO a buscar o issue via MCP em vez de depender de descrição digitada; se a
busca falhar, instrui a pedir ao analista que cole o traceback manualmente, seguindo
como um pedido normal.

O `auto_scrum` não configura nem anexa o MCP do Sentry — isso vive no `.mcp.json` do
próprio projeto alvo (ex: `siga-construcao/.mcp.json`), que o Claude Code já carrega
sozinho a partir do diretório onde `auto_scrum.sh` é executado. `SENTRY_ENABLED` só liga
a instrução no prompt; não precisa (nem deve) existir nenhum `--mcp-config` ou config de
MCP dentro do `nick_secrets`. Só a etapa de PO recebe essa instrução — o Tech Leader não
acessa o Sentry diretamente, só recebe o ticket já escrito pelo PO (`SENTRY_BLOCK_TL`
continua igual, só carrega a referência adiante).

**Por quê:** um analista reportando um erro do Sentry muitas vezes não sabe qual fluxo
técnico gerou o erro — exigir uma descrição narrada nesse caso é pedir uma informação que
ele não tem, quando o próprio Sentry já carrega isso (stack trace, frequência, usuários
afetados). A configuração de acesso ao Sentry (servidor MCP, host, credenciais) é dado do
projeto alvo, não do `auto_scrum` — mesma lógica de `STACK_DESCRIPTION`/`SENTRY_ENABLED`
ficarem em `projects/<name>.sh`, só que aqui o "arquivo do projeto" nem é do
`nick_secrets`, é o `.mcp.json` que já mora no repositório do projeto alvo. O MCP fica
restrito à etapa de PO porque é ali que a análise do pedido acontece; o Tech Leader
trabalha em cima do texto já produzido, sem necessidade de acesso próprio ao Sentry.
