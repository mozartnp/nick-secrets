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

## Qualidade de código bash

O projeto é 100% bash. Regras a seguir em todo script novo ou alterado:

- `set -euo pipefail` no topo do script.
- Aspas em toda expansão de variável (`"$var"`, `"$@"`) — nunca `$var` solto.
- `local` em toda variável declarada dentro de função.
- `[ ]` para testes condicionais (não `[[ ]]`) — é o que já está em uso no
  `auto_scrum.sh`, então mantemos o padrão em vez de misturar os dois estilos.
- `$(...)` em vez de crase para comandos aninhados.
- Nomenclatura: `MAIUSCULO` para variáveis globais/exportadas (ex: `TITULO`,
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

O plano gerado pelo template `tech_leader.md` sempre vai em `@docs/plans/${ID_JIRA}.md`
— sem sufixo de título. É o mesmo path que `desenvolvimento.md`/`review.md` usam como
default de `PLANO_PATH` (`ask_questions_desenvolvimento`/`ask_questions_review` no
`auto_scrum.sh`). O plano também precisa terminar com duas seções fixas: `## Critérios
de aceite` (obrigatória — herdada do ticket ou definida pelo Tech Leader) e `##
Habilidades necessárias`.

**Por quê:** as quatro etapas rodam em sessões `claude` separadas, sem estado
compartilhado além do que é escrito em disco — o path precisa ser previsível pra Dev e
Review acharem o plano sem o usuário precisar informar manualmente toda vez. Os
critérios de aceite são obrigatórios porque `review.md` sempre valida o trabalho contra
eles; se o Tech Leader os deixasse de fora "quando não necessário", a etapa de review
ficaria sem base pra validar. "Habilidades necessárias" vira seção do próprio plano (em
vez de só aparecer na resposta do chat) porque o `log_file` do `auto_scrum.sh` grava
apenas o prompt de entrada — a resposta do Claude não é persistida em lugar nenhum além
do plano que ele mesmo escreve.

## Modo auto só na etapa de Desenvolvimento

`auto_scrum.sh` sobe o `claude` com `--permission-mode auto` (aprova a maioria das
chamadas de ferramenta sozinho, exceto o que estiver em deny/ask do `settings.json`)
somente quando `$TIPO = desenvolvimento`. PO, Tech Leader e Review sobem no modo padrão
(interativo, pedindo confirmação).

**Por quê:** Desenvolvimento é a única etapa em que o agente de fato edita código e roda
comandos — as outras três são conversas de avaliação/planejamento/revisão em texto, sem
motivo pra soltar a supervisão. Regras de `deny` no `settings.json` continuam valendo em
qualquer modo (deny > ask > allow > modo da sessão), então `auto` não contorna o que já
estiver bloqueado ali — mas hoje o projeto não tem nenhuma regra de `deny` configurada,
só `allow`. Vale definir uma deny list antes de confiar demais no modo `auto`.
