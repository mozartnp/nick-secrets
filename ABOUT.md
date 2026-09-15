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
  falso positivo intencional (ex: aspas simples de propósito numa lista de variáveis),
  suprimir com `# shellcheck disable=SCxxxx` acompanhado do porquê, como já feito no
  `SC2016` da linha do `envsubst` em `main()`.

**Por quê:** essas regras evitam as três classes de bug mais comuns em bash —
variável não citada que quebra com espaço/glob, variável de função vazando pro
escopo global, e falha de comando ignorada silenciosamente por causa do
`set -e` ausente.

## README genérico: sem repetir detalhe que muda com o código

O `README.md` descreve uso e comportamento estável: como rodar, de onde vem a configuração
e em que ordem, a regra de formato, o que é versionado onde, notas de migração. Listas que
crescem a cada ticket — chaves de configuração, quais etapas usam cada flag, opções de
menu — **não** vão pro README: ficam no código, neste `ABOUT.md` ou nos comentários do
esqueleto gerado por `--init` (`print_project_skeleton()`), que é a fonte única da lista de
chaves da configuração do projeto (protegida pelo teste de consistência com
`PROJECT_CONFIG_KEYS`). O README remete a essas fontes em vez de copiá-las.

**Por quê:** o README não tem teste que pegue desatualização, e cada flag nova exigiria
lembrar de editá-lo. O ticket 8 já mostrou isso acontecendo: uma frase do passo 1 de "Uso"
("só é usado de fato por Tech Leader/Desenvolvimento") foi mexida e continuou errada, já que
`JIRA_ENABLED`/`SENTRY_ENABLED`/`PERMISSION_ENABLED` afetam o menu e o prompt do PO.

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

## PO - Conversar para definir ticket (`po_discussion`)

Segunda forma de ativar o PO, além de `po` — mesmo papel/critérios de validade, entrada
diferente: em vez de chegar com título+descrição prontos, começa só com um assunto solto
(`ask_questions_po_discussion`, um `read_required` só). O template
`templates/po_discussion.md` instrui o PO a conversar e explorar antes de aplicar os
critérios de validade, só fechando o formato de ticket quando a conversa convergir.

Não existe encadeamento automático daqui pro Tech Leader — igual ao resto do projeto, o
ticket que sai no final da conversa é copiado manualmente pro Jira, e o Tech Leader é
acionado depois, à parte, com o `JIRA_ID` já existente.

**Por quê:** é o mesmo PO, não uma persona nova — a diferença está em quando a validação
acontece (antes da conversa, com pedido já formado, vs. depois, quando ela converge), não
em quem avalia nem nos critérios usados. Por isso é um `TYPE` novo com template próprio
(a abertura muda de "avalie o pedido abaixo" pra "descubra o pedido junto com o
analista"), mas a parte que precisa ficar idêntica nos dois — critérios de validade,
análise obrigatória, bloco de Sentry, formato do ticket — vem de `build_po_blocks()`
(`PO_VALIDATION_BLOCK`/`PO_TICKET_FORMAT_BLOCK`, mesmo mecanismo de `build_sentry_blocks`/
`build_stack_blocks`), não de texto duplicado nos dois `.md`. Editar um critério em
`build_po_blocks()` já vale pros dois pontos de entrada — não tem como os dois divergirem
por esquecimento.

## PO - Criar ticket a partir do Jira (`po_jira`)

Terceiro ponto de entrada do PO, além de `po` e `po_discussion` — mesmo papel, mesmos
critérios de validade (`PO_VALIDATION_BLOCK`/`PO_TICKET_FORMAT_BLOCK` de
`build_po_blocks()`, sem duplicar texto), entrada diferente: em vez de título+descrição
prontos ou uma conversa, o analista informa só o link de um ticket que já existe no Jira
(`ask_questions_po_jira`, `read_required` pro link) mais uma descrição extra opcional
(`read_optional`). A variável é `JIRA_LINK`, não `JIRA_ID` — `JIRA_ID` já é usado por
`tech_leader`/`development`/`review` com semântica de chave curta (`SC-123`, nome de
branch, path de plano), enquanto aqui o dado de entrada é uma URL e o ID real só é
conhecido depois que o PO busca o ticket.

O template `po_jira.md` instrui o PO a buscar o ticket via MCP do Jira do projeto alvo, a
partir do link: título, ID, descrição e anexos de imagem — considerando o conteúdo desses
anexos na análise. Anexos de vídeo ficam fora de escopo, não são buscados nem tratados. Se
a busca falhar por qualquer motivo (link inválido, ticket não encontrado, MCP indisponível
ou não configurado no projeto alvo), o PO avisa o analista e pede que ele resolva o
link/acesso antes de continuar; só se isso não for possível, o PO encerra esse fluxo e
orienta o analista a recomeçar pela opção "PO - Criar ticket" (`po`) — não tenta coletar a
descrição completa dentro desta mesma conversa, nem presume que a descrição extra digitada
(se houver — é só um complemento opcional) seja suficiente sozinha, já que o analista pode
não ter preenchido esse campo por contar que tudo viria do Jira. Diferente do fallback do
`po_sentry`, que recupera dentro da própria sessão pedindo o traceback colado na conversa
(ver seção "Sentry via MCP na etapa de PO"), aqui o fallback redireciona pro entry point
certo em vez de tentar reconstituir o fluxo de título+descrição dentro de uma sessão
pensada só pro caminho via Jira. Igual ao Sentry, `auto_scrum` não configura nem anexa o
MCP do Jira: isso vive no `.mcp.json` do próprio projeto alvo, carregado sozinho pelo
Claude Code a partir do cwd.

`JIRA_ENABLED` ("true"/"false", opt-in — igual `SENTRY_ENABLED`/`PERMISSION_ENABLED`)
controla se a opção "PO - Criar ticket a partir do Jira" aparece no menu de
`choose_type()`: só entra na lista quando o projeto tem o MCP do Jira configurado. Foge um
pouco do mecanismo padrão dessas flags (`build_*_blocks()` montando um bloco de texto pro
`envsubst`, ver seção "Flags booleanas por projeto" abaixo) porque aqui o efeito não é
texto num prompt — é a própria opção do menu sumir; por isso é lida direto dentro de
`choose_type()`, sem passar por um `build_jira_blocks()`. Igual aos outros dois pontos de
entrada do PO, não existe encadeamento automático daqui pro Tech Leader.

**Por quê:** um ticket que já existe no Jira não deveria precisar ser reescrito à mão pelo
analista, perdendo os anexos de imagem (prints de bug) que não têm como entrar no fluxo
via texto digitado. `JIRA_LINK` fica separada de `JIRA_ID` porque misturar as duas
criaria ambiguidade entre uma URL de entrada e uma chave curta usada depois, no plano e no
código. `JIRA_ENABLED` existe porque nem todo projeto alvo tem o MCP do Jira configurado —
sem a flag, a opção apareceria pra todo mundo no menu e falharia de cara pra quem não tem
o `.mcp.json` certo; escondê-la por padrão (opt-in) evita oferecer um caminho que não vai
funcionar. O fallback pede correção antes de seguir porque a falha mais comum de busca
(link errado, MCP sem autenticar) é algo que o próprio analista consegue resolver na hora
— pedir isso primeiro evita abrir mão do ticket original por um problema temporário e
resolvível. Quando não dá pra resolver (MCP indisponível/não configurado no projeto), o PO
não tenta virar um `po` improvisado dentro da mesma sessão — encerra e manda o analista
recomeçar pelo fluxo certo, que já existe pronto pra coletar título+descrição via editor.
Isso evita duplicar, dentro do template do `po_jira`, a lógica de coleta de descrição que
o `po` já resolve, e evita presumir que o campo de descrição extra (pensado como
complemento, não como pedido
autossuficiente) supre a falta do ticket — se o analista contava com a busca funcionando,
pode muito bem ter deixado esse campo vazio, e nesse caso não haveria nada pra analisar.

## Tech Leader - Criar plano a partir do Jira (`tech_leader_jira`)

Segundo ponto de entrada do Tech Leader, além de `tech_leader` — mesmo papel, mesmas
regras de plano (metodologia TDD, seções finais obrigatórias `## Critérios de aceite`/
`## Habilidades necessárias`, convenção `docs/plans/<ID>_<resumo-curto>.md`), entrada
diferente: em vez de `JIRA_ID`/título/descrição digitados manualmente, o analista informa
só o link de um ticket que já existe no Jira (`ask_questions_tech_leader_jira`,
`read_required` pro link) mais um contexto extra opcional (`read_optional`), reaproveitando
`JIRA_LINK`/`DESCRIPTION` — as mesmas variáveis já usadas por `po_jira`, sem criar
`EXTRA_CONTEXT` nem equivalente.

O template `tech_leader_jira.md` instrui o Tech Leader a buscar o ticket via MCP do Jira
do projeto alvo, a partir do link: título, ID, descrição e anexos de imagem — visualizando
e considerando o conteúdo desses anexos no plano técnico (ex: diagrama de arquitetura,
print de comportamento esperado). Anexos de vídeo ficam fora de escopo, mesma regra do
`po_jira.md`. O fallback também segue o mesmo padrão em duas etapas: se a busca falhar
(link inválido, ticket não encontrado, MCP indisponível ou não configurado), avisa o
analista e pede que resolva o link/acesso; só se isso não for possível, encerra o fluxo e
orienta a recomeçar pela opção "Tech Leader - Criar plano" (`tech_leader`, manual) — sem
tentar coletar ID/título/descrição dentro desta mesma sessão, pensada só pro caminho via
Jira.

**`JIRA_ID` não é usado no template — nem como `${JIRA_ID}`.** Diferente de `tech_leader`
(onde `JIRA_ID` é digitado antes e exportado com valor real), aqui o ID do ticket só é
conhecido *depois* que o Tech Leader busca via MCP, dentro da própria sessão do Claude. O
script continua exportando `JIRA_ID` (fica vazio, já que `ask_questions_tech_leader_jira`
não a define) e continua na lista de variáveis do `envsubst` — outros `TYPE`s dependem
disso. O risco é usar `${JIRA_ID}` dentro de `tech_leader_jira.md`: como a variável está
na lista do `envsubst`, isso não geraria erro nem sobra de `${...}` — só substituiria
silenciosamente por uma string vazia, quebrando a instrução de nome de arquivo sem deixar
rastro. Por isso o template usa texto literal (`<ID-do-ticket>`, "o ID do Jira que você
acabou de buscar") em vez de `${JIRA_ID}`, e um teste estrutural (grep) garante que essa
string nunca entre no arquivo.

`JIRA_ENABLED` (a mesma flag que já controla `po_jira`) também controla se "Tech Leader -
Criar plano a partir do Jira" aparece no menu de `choose_type()`, posicionada logo após
"Tech Leader - Criar plano" e antes de "Desenvolvimento" — mesmo racional de
posicionamento de `po_jira` (logo após o par manual do seu papel). Igual ao restante do
projeto, não existe encadeamento automático daqui pro `development`.

**Por quê:** mesmo racional de `po_jira` — um ticket que já existe no Jira não deveria
precisar ser retranscrito à mão pelo Tech Leader, perdendo anexos de imagem (diagramas,
prints) que informariam o plano técnico. Reaproveitar `JIRA_LINK`/`DESCRIPTION` em vez de
criar variáveis novas evita duplicar um mecanismo que `po_jira` já resolveu pro mesmo tipo
de entrada (URL + complemento opcional). `JIRA_ID` fica de fora do template porque, nesse
`TYPE`, ele nunca tem valor real vindo do script — usá-lo seria uma falha silenciosa
(string vazia), não um erro visível, daí a decisão explícita de texto literal e o teste
estrutural que protege contra reintrodução do problema.

## Flags booleanas por projeto (ex: SENTRY_ENABLED)

Nem toda referência de prompt vale para todo projeto (ex: nem todo projeto usa Sentry).
Esse tipo de coisa vira uma variável booleana no arquivo de configuração do projeto
(`.nick.conf` no cwd ou `auto_scrum/projects/<name>.conf` — ver seção "Configuração do
projeto: cwd primeiro, projects/ como fallback" abaixo), no formato
`SENTRY_ENABLED="true"/"false"` (opt-in — vazio conta como `"false"`), lida por uma função
`build_*_blocks()` em `auto_scrum.sh` (mesmo padrão de `build_stack_blocks()`/
`STACK_DESCRIPTION`) que monta o bloco de texto correspondente só quando a flag está ativa. O
template usa a variável de bloco (ex: `${SENTRY_BLOCK_TL}`) no lugar do texto fixo — nunca
um `if` dentro do `.md`, porque `envsubst` não suporta condicional. Quando o bloco precisa
valer pra vários templates de um mesmo papel, o bash o embute dentro de um bloco
compartilhado em vez de o template referenciá-lo direto: `SENTRY_BLOCK_PO` e
`PERMISSION_BLOCK_PO` entram em `PO_VALIDATION_BLOCK` via `build_po_blocks()`, e só o bloco
de fora é exportado e passado ao `envsubst` (que não expande recursivamente).

Toda variável do `envsubst`/`export` em `main()` precisa ser usada em algum template, e toda
variável usada em template precisa estar nas duas listas — um teste estrutural compara os
três conjuntos. Isso vale pra qualquer variável nova: uma variável de template fora do
`envsubst` sobraria como `${...}` literal no prompt, e uma variável exportada sem uso em
template é código morto. O teste genérico não pega uma variável de outro `TYPE` usada num
template cujo `TYPE` não a preenche: ela está nas listas, então seria substituída
silenciosamente por vazio. Pra isso existe um teste de conjunto exato por template (via
`template_vars` em `test_auto_scrum.sh`), mas hoje só `po.md` e `po_sentry.md` o têm.
Template novo, ou que ganhe variável nova, deve ganhar o seu.

Toda flag nova precisa entrar também em `PROJECT_CONFIG_KEYS` (topo do `auto_scrum.sh`) e no
esqueleto de `print_project_skeleton()`: o leitor `load_project_config` ignora qualquer chave
fora dessa lista, então uma flag só escrita no arquivo nunca chegaria ao script. Um teste
compara as chaves do esqueleto com `PROJECT_CONFIG_KEYS` pra pegar o esquecimento de um dos
dois lados.

**Por quê:** manter esse tipo de decisão condicional em bash, não no template, é o mesmo
racional de `build_stack_blocks()` — e opt-in (padrão desligado) evita que um projeto novo
criado via `--init` puxe menção a uma ferramenta que ele não usa sem querer.

## DRY entre templates do mesmo papel (texto compartilhado vira `build_*_blocks()`)

Variação do mecanismo acima ("Flags booleanas por projeto"), pro caso em que o texto não é
condicional — é sempre montado, só não pode ficar duplicado literalmente em mais de um
`.md`. Quando dois ou mais templates do mesmo papel (pontos de entrada diferentes, ex:
`tech_leader`/`tech_leader_jira`, ou `po`/`po_discussion`/`po_jira`/`po_sentry`)
compartilham um trecho que precisa ficar idêntico entre eles, esse trecho vira uma
variável de bloco (`${PO_VALIDATION_BLOCK}`, `${TECH_LEADER_PLAN_RULES_BLOCK}`) montada
uma vez, numa função `build_*_blocks()` em `auto_scrum.sh` — `build_po_blocks()` pro PO,
`build_tech_leader_blocks()` pro Tech Leader —, exportada e adicionada à lista do
`envsubst`. O template referencia a variável; nunca repete o texto.

Exemplo: `TECH_LEADER_PLAN_RULES_BLOCK` (regra de perguntar se faltar informação técnica,
metodologia TDD, as duas seções finais obrigatórias) é montada em
`build_tech_leader_blocks()` e usada tanto em `tech_leader.md` quanto em
`tech_leader_jira.md` — antes da extração, esse parágrafo existia duplicado, palavra por
palavra, nos dois arquivos.

**O que não entra num bloco compartilhado:** partes que dependem do dado de entrada
específico daquele ponto de entrada, mesmo que pareçam parecidas à primeira vista — ex: a
seção "DADOS DE ENTRADA" de cada template (campos diferentes: `JIRA_ID`/`TITLE` em
`tech_leader.md` vs `JIRA_LINK`/`DESCRIPTION` em `tech_leader_jira.md`), ou a frase que
cita o nome do arquivo do plano, que em `tech_leader.md` usa `${JIRA_ID}` mas em
`tech_leader_jira.md` precisa ser texto literal (`<ID-do-ticket>`, ver seção
`tech_leader_jira` acima) — forçar essas partes a compartilhar uma variável recriaria, por
outro caminho, o mesmo risco de `${JIRA_ID}` vazando como string vazia que a decisão
anterior evitou.

**Por quê:** editar a regra compartilhada em um lugar só (a função) já vale pra todos os
templates que a usam — texto duplicado diverge com o tempo, quando alguém edita um `.md` e
esquece do outro. É o mesmo racional de `build_po_blocks()`/`PO_VALIDATION_BLOCK`
(documentado em "PO - Conversar para definir ticket" acima), generalizado pra qualquer
papel com mais de um ponto de entrada.

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
mudanças caem no repositório certo.

O cwd também decide a configuração do projeto (`STACK_DESCRIPTION`/`*_ENABLED`) quando
existe um `.nick.conf` na raiz dele: nesse caso `--projeto=<nome>` é ignorado (com aviso) e o
menu de projetos não aparece — ver seção "Configuração do projeto: cwd primeiro, projects/
como fallback" abaixo. Sem `.nick.conf` no cwd, `--projeto=<nome>` (flag do
`auto_scrum.sh`) escolhe a configuração em `auto_scrum/projects/<name>.conf`, e aí sim não
tem relação com em qual diretório o comando roda — são coisas independentes que
coincidentemente usam o mesmo nome de projeto.

## Configuração do projeto: cwd primeiro, projects/ como fallback

A configuração de cada projeto alvo (`STACK_DESCRIPTION`, `SENTRY_ENABLED`,
`PERMISSION_ENABLED`, `JIRA_ENABLED`) pode morar em dois lugares, e `resolve_stack()`
procura nesta ordem:

1. **`.nick.conf` na raiz do cwd** (`$PWD/.nick.conf`, constante `CWD_CONFIG_NAME`). Se
   existir, vence tudo: é carregado, o script imprime no stdout
   `Configuração do projeto carregada de: <caminho absoluto>` e retorna — sem menu de
   projetos e sem aviso de `.sh` legado. Se `--projeto=<nome>` também foi passado, sai no
   stderr `Aviso: --projeto=<nome> ignorado — a configuração do diretório atual (<caminho>)
   tem precedência.`, e o nome **não é validado** (`--projeto=inexistente` não dá `exit 1`,
   já que a flag nem é usada).
2. **`--projeto=<nome>`** → `auto_scrum/projects/<nome>.conf` (erro se não existir).
3. **Menu** com `auto_scrum/projects/*.conf` + "Nenhum".

A linha "carregada de" só existe no caminho do cwd; nos caminhos 2 e 3 a saída é a de
sempre, exceto pelo aviso de `.sh` legado (abaixo).

**Nome `.nick.conf`, só no cwd.** O arquivo vai ser versionado dentro do repositório de outro
time, então não pode ser apanhado pelo lint/pre-commit/CI desse time: dotfile (não polui a
listagem); sem `.sh` (fora de `shellcheck $(git ls-files '*.sh')`, `find -name '*.sh'` e dos
hooks que filtram tipo shell); sem shebang (o `identify` do pre-commit detecta shell por
shebang em arquivo sem extensão conhecida); sem `.env` (não é carregado por
`python-dotenv`/`django-environ`/`docker compose` nem tratado como segredo por scanners); sem
`.yml`/`.json`/`.toml`/`.ini` (fora de `check-yaml`/`check-json`/prettier). Os hooks genéricos
de texto (`trailing-whitespace`, `end-of-file-fixer`, `mixed-line-ending`) valem pra qualquer
arquivo, então o esqueleto do `--init` sai sem espaço no fim de linha, com LF e terminando em
`\n` — coberto por teste. A busca é só no `$PWD`, sem subir pra diretórios pais ou pra raiz
do git: a convenção já é rodar da raiz do projeto alvo, e subir criaria ambiguidade (qual
arquivo vence num monorepo?) sem pedido concreto.

**Leitura por `CHAVE="valor"` (`load_project_config`), sem `source` nem `eval`.** O mesmo
leitor vale pro cwd e pra `projects/` — um mecanismo de carga só. Gramática: depois de tirar
espaços/tabs do começo e do fim da linha (o que também remove o `\r` de CRLF), só é aplicada
uma linha `CHAVE="valor"` com `CHAVE` em `PROJECT_CONFIG_KEYS` (comparação exata) e `valor`
sem aspas duplas dentro. O valor é atribuído **literalmente** com `printf -v` — sem expansão
de `$VAR`, `$(...)`, crase ou escape —, e o nome da variável vem sempre de
`PROJECT_CONFIG_KEYS`, nunca do arquivo. Todo o resto é ignorado em silêncio: comentários,
linhas em branco, comandos e chaves desconhecidas (inclusive globais do script como `PATH` e
`TYPE`, que um `source` sobrescreveria). A exceção é uma linha de chave conhecida que foge do
formato: tanto a que começa com `CHAVE=` (`JIRA_ENABLED=true`, `JIRA_ENABLED="true" && touch
x`, `JIRA_ENABLED="true" # comentário`) quanto as formas "quase certas" `export CHAVE=...` e
`CHAVE = ...`/`CHAVE ="..."` (espaço logo depois do nome — `STACK_DESCRIPTION_EXTRA = "x"`
continua sendo chave desconhecida, sem aviso). Ela é ignorada com um único aviso no stderr,
com arquivo e número da linha (texto único em `warn_malformed_config_line`); essas formas
continuam **não** sendo aceitas, só deixam de ser descartadas em silêncio. Aspas simples, valores sem aspas e comentário no fim da linha ficam de fora de
propósito, pra gramática ser pequena e fácil de testar. Variáveis locais da função precisam
ficar em minúsculo — uma local com o nome de uma chave faria o `printf -v` alterar a local
em vez da global.

**`projects/` passou de `.sh` pra `.conf`.** Os arquivos deixaram de ser executados, e manter
`.sh` passaria a ideia errada de que são `source`ados (e editores/`shellcheck` os tratariam
como script). O conteúdo não mudou: já era `CHAVE="valor"` com comentários. `.sh` **não** é
lido como fallback (seriam dois mecanismos de novo) nem renomeado pelo script (mover arquivo
do usuário sem pedir é surpresa): `warn_legacy_project_files` só avisa no stderr, pra cada
`projects/*.sh`, com o `mv` sugerido — e só nos caminhos 2 e 3, nunca quando a configuração
vem do cwd.

**`--init` pergunta o destino** (`.nick.conf` no cwd primeiro, por ser o local recomendado,
ou `auto_scrum/projects/<nome>.conf`), gera o mesmo conteúdo nos dois casos
(`print_project_skeleton`) e pergunta antes de sobrescrever em ambos.

**`.gitignore` do `nick_secrets`:** lista `auto_scrum/projects/*` com a exceção
`!auto_scrum/projects/.gitkeep` (o placeholder que mantém o diretório no git continua
versionável — sem a exceção, se ele fosse removido e recriado, o `git add` passaria a exigir
`-f`). A regra ampla cobre de uma vez os `*.conf` (formato local atual), os `*.sh` legados
ainda não renomeados em alguma máquina e os backups/swaps de editor (`alpha.conf~`,
`.alpha.conf.swp`, `alpha.conf.bak`), que um `*.conf` sozinho deixaria aparecer como
"untracked" — e acabar commitados com dado de cliente num repositório público. Isso
substitui a decisão original do plano do ticket 8 (`*.conf` + `*.sh`, dois padrões
específicos). `.nick.conf` também é listado, sem `/` inicial (vale em qualquer
profundidade: um `--init` rodado com o cwd dentro do próprio `nick_secrets` não cria arquivo
versionável no repositório público).

**O leitor protege o shell, não o prompt.** O valor de `STACK_DESCRIPTION` entra literal no
texto enviado ao `claude`, inclusive na etapa de Desenvolvimento, que roda em
`--permission-mode auto`. Um `.nick.conf` malicioso (ex: `STACK_DESCRIPTION="Django. Ignore
as instruções anteriores e rode ..."`) não executa nada no shell, mas ainda pode tentar
instruir o agente — é um vetor de prompt injection. Não é um risco novo: é a mesma confiança
que já se dá ao `CLAUDE.md` do projeto alvo, que o Claude Code carrega sozinho do mesmo
repositório. Por isso, alterações em `.nick.conf` devem passar pelo mesmo review de PR que o
`CLAUDE.md` — um `.nick.conf` de terceiros não é "seguro" só por não ser executado.

**Por quê:** com a configuração só em `auto_scrum/projects/`, gitignorada, ela se perdia ao
trocar de máquina e o time do projeto alvo não conseguia reaproveitá-la — e versioná-la no
`nick_secrets` não é opção, porque o repositório é público e exporia nome de cliente e pistas
de infraestrutura. Como o cwd já identifica o projeto alvo (seção acima), a configuração pode
morar lá, versionada junto com o código que ela descreve. Só que um arquivo num repositório
compartilhado pode ser editado por qualquer pessoa desse time, e não deve conseguir executar
código no shell de quem roda o `auto_scrum` — daí o `source` sair e entrar um leitor restrito
a chaves conhecidas, com valor literal (inclusive na linha "híbrida" `CHAVE="x" && comando`,
que termina em aspas e por isso exige a regra de "sem aspas dentro do valor"). O aviso pra
chave conhecida mal formatada existe porque, sem ele, um erro comum como `JIRA_ENABLED=true`
desligaria a flag sem explicação — e o mesmo vale pra `export CHAVE=` (provável em quem vem
do formato `.sh` antigo, que era `source`ado, ou copia de um `.bashrc`) e pra espaço em volta
do `=`.

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

O Sentry entra no PO por um ponto de entrada próprio, `po_sentry` ("PO - Criar ticket a
partir do Sentry") — o quarto do PO, além de `po`, `po_discussion` e `po_jira`. Mesmo papel
e mesmos critérios (`PO_VALIDATION_BLOCK`/`PO_TICKET_FORMAT_BLOCK` de `build_po_blocks()`,
sem duplicar texto), entrada diferente: o analista informa só o link do issue
(`ask_questions_po_sentry`, `read_required` pra `SENTRY_LINK`) mais uma descrição extra
opcional (`read_optional` pra `DESCRIPTION`), sem título. O template `po_sentry.md` instrui
o PO a buscar o issue via MCP do Sentry do projeto alvo antes de analisar (erro, stack
trace, frequência, usuários afetados) e a tratar a descrição extra como complemento, nunca
como substituta da busca. Igual aos outros pontos de entrada do PO, não existe
encadeamento automático daqui pro Tech Leader. O `po` voltou a ter um comportamento só:
título obrigatório e descrição pelo editor, com ou sem `SENTRY_ENABLED`.

A variável é `SENTRY_LINK`, não `JIRA_LINK` reaproveitada — mesmo racional que separou
`JIRA_LINK` de `JIRA_ID`: cada variável com uma semântica só, e uma URL do Sentry numa
variável chamada `JIRA_LINK` seria ambígua. `DESCRIPTION` é reaproveitada porque já faz o
papel de complemento opcional em `po_jira`/`tech_leader_jira`. `SENTRY_LINK` é dado de
entrada, não chave de configuração: não entra em `PROJECT_CONFIG_KEYS`.

`SENTRY_ENABLED` tem dois efeitos:
1. **Menu:** a opção `po_sentry` só aparece com `"true"`, lida direto em `choose_type()`
   (igual `JIRA_ENABLED`), logo depois dos outros pontos de entrada do PO (depois de
   `po_jira` quando ele existe, senão depois de `po_discussion`) e antes de "Tech Leader -
   Criar plano".
2. **Texto nos prompts:** `build_sentry_blocks()` monta `SENTRY_BLOCK_PO` e
   `SENTRY_BLOCK_TL`. `SENTRY_BLOCK_PO` só instrui a referenciar no ticket um link do Sentry
   ou de erro externo — ele entra em `PO_VALIDATION_BLOCK`, então vale pros quatro pontos
   de entrada do PO (inclusive o `po`, já que o analista pode colar um link na descrição).
   As instruções de buscar via MCP e de fallback **não** ficam nesse bloco: são texto fixo
   em `po_sentry.md`, o único template com um link do Sentry como entrada. Deixá-las no
   bloco compartilhado mantinha um caminho condicional do Sentry dentro de `po`,
   `po_discussion` e `po_jira`, que nunca recebem esse link.

O fallback tem duas etapas: se a busca falhar (link inválido, issue não encontrado, falha
de autenticação, MCP indisponível ou não configurado), o PO avisa o analista e pede que ele
corrija o link ou o acesso; se não der, pede o traceback ou os detalhes do erro colados na
própria conversa e segue com eles como um pedido normal, **sem** mandar recomeçar por outra
opção do menu. Isso diverge de propósito do `po_jira`, que encerra e redireciona pro `po`:
no Jira, o que faltaria (descrição completa, anexos de imagem) não se reconstitui colando
texto, e o `po` já existe pra coletar título e descrição pelo editor; no Sentry, o dado
essencial é o traceback, que é texto e basta pra análise. A colagem acontece na conversa do
Claude Code, não num `read` do `auto_scrum`, então não esbarra no problema de colar texto
grande no terminal que motivou o `read_via_editor`.

O que não mudou: o `auto_scrum` não configura nem anexa o MCP do Sentry — isso vive no
`.mcp.json` do próprio projeto alvo (ex: `siga-construcao/.mcp.json`), que o Claude Code já
carrega sozinho a partir do diretório onde `auto_scrum.sh` é executado. Não precisa (nem
deve) existir nenhum `--mcp-config` ou config de MCP dentro do `nick_secrets`. Só o PO
acessa o Sentry — o Tech Leader só recebe o ticket já escrito pelo PO, e `SENTRY_BLOCK_TL`
continua só carregando a referência adiante.

**Por quê:** um analista reportando um erro do Sentry muitas vezes só tem o link — não sabe
qual fluxo técnico gerou o erro nem como dar título e descrição a ele, e o próprio Sentry já
carrega isso (stack trace, frequência, usuários afetados). Antes, esse caso era um desvio
dentro do `po`: um campo opcional de link que, se preenchido, mudava a pergunta seguinte
(descrição curta em vez do editor) e o texto do prompt. Um fluxo único que muda de
comportamento conforme um campo opcional é confuso pra quem usa e pra quem mantém — é o
mesmo racional de tratar o Jira como ponto de entrada próprio. A configuração de acesso ao
Sentry (servidor MCP, host, credenciais) é dado do projeto alvo, não do `auto_scrum` — mesma
lógica de `STACK_DESCRIPTION`/`SENTRY_ENABLED` ficarem no arquivo de configuração do
projeto, só que aqui o "arquivo do projeto" nem é do `nick_secrets`, é o `.mcp.json` que já
mora no repositório do projeto alvo. O MCP fica restrito ao PO porque é ali que a análise
do pedido acontece; o Tech Leader trabalha em cima do texto já produzido.
