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

- `set -euo pipefail` no topo do **script**. Uma biblioteca em `lib/` (carregada com
  `source`) não tem: quem decide isso é o script que a carrega, e ela funciona com ou sem
  (ver seção "Código compartilhado entre ferramentas (`lib/`)").
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
- Sabotagem temporária (alterar o script de propósito pra provar que um teste-guarda
  morde) só numa cópia descartável **fora do repositório**, com só (caminhos a partir da
  raiz) `Makefile`, `tests/`, `auto_scrum/auto_scrum.sh`, `auto_scrum/templates/`,
  `auto_scrum/tests/`, `lib/`, `README.md` (lido pelo teste README × helper de
  `lib/tests/test_jail.sh`) e `.gitignore` (lido pelos testes de `.gitignore` de
  `auto_scrum/tests/test_auto_scrum.sh`), na mesma estrutura (sem copiar
  `auto_scrum/projects/` nem `auto_scrum/logs/`, que têm dados reais), com um `git init` na
  cópia e o cwd dentro dela ao rodar a suíte (ou o `make test`). No fim, apagar a cópia e
  conferir com `git status --short --ignored` que nada mudou no repositório.

**Por quê:** essas regras evitam as três classes de bug mais comuns em bash —
variável não citada que quebra com espaço/glob, variável de função vazando pro
escopo global, e falha de comando ignorada silenciosamente por causa do
`set -e` ausente. A regra da sabotagem existe porque os testes unitários usam o
`LOGS_DIR`, o `PROJECTS_DIR` e o cwd reais (só os de `main()` apontam pra diretórios
temporários), e a configuração da raiz (`.nick/nick.conf`, ou o `.nick.conf` de antes do
ticket 24 num clone ainda não migrado) e a `.nick/` inteira são gitignoradas, sem cópia: uma
sabotagem que escreve arquivo, rodada da raiz do repositório, zera ou cria arquivos reais —
já aconteceu na validação do ticket 5, com o `.nick.conf` do usuário. O `.gitignore` e o
`git init` entram na cópia porque os testes de `.gitignore` da `test_auto_scrum.sh` (ticket 24)
usam `git check-ignore` contra o `.gitignore` da raiz: sem os dois, eles dão FAIL falso, e a
suíte acusaria falha sem relação com a sabotagem (aconteceu na validação do ticket 25).

## Testes: `make test` e `tests/run_all.sh`

`make test` na raiz roda todas as suítes do repositório. A lógica fica em `tests/run_all.sh`,
e o `Makefile` só o chama. As regras:

- **Descoberta por padrão de nome:** é suíte todo `test_*.sh` **direto** num diretório chamado
  `tests` (sem descer em subdiretórios dele), fora de diretórios ocultos (`.git/`, `.nick/`,
  `.claude/`, inclusive um `tests/` lá dentro). Não há lista de suítes nem de exclusões: uma
  suíte nova entra só por ter esse nome nesse lugar, sem mexer no `Makefile` nem no runner, e
  um helper como `lib/tests/assert.sh` fica de fora por não casar com o padrão. O runner usa
  `find -prune` pros ocultos e, dentro de cada `tests/`, um glob `test_*.sh` com `[ -f ]`. Não
  usa `find -path '*/tests/test_*.sh'`: o `*` do `-path` casa `/`, e `tests/test_dir/x.sh`
  entraria. A ordem é a do `LC_ALL=C sort -z`, a mesma em qualquer máquina, e os caminhos
  aparecem relativos à raiz (`./lib/tests/test_config.sh`).
- **Uma de cada vez, com o cwd na raiz:** as suítes usam o `LOGS_DIR`, o `PROJECTS_DIR` e o cwd
  reais (ver a regra da sabotagem em "Qualidade de código bash"), então não podem rodar em
  paralelo nem de outro diretório. O runner dá `cd` na raiz (achada pelo `BASH_SOURCE`), então
  `bash <raiz>/tests/run_all.sh` funciona de qualquer cwd. Cada suíte roda com `bash <suíte>`
  (não `./`, que exigiria bit de execução) e herda o stdin, como rodada à mão.
- **Nenhuma parada na primeira falha:** todas rodam, e o resumo no fim lista o caminho (e o
  código) de cada uma que falhou, pra mostrar tudo de uma vez. Qualquer código ≠ 0 é falha,
  inclusive o de uma suíte que morreu no meio. Com falha, o runner sai com 1, e o `make` sai
  com 2 (`make: *** [Makefile:N: test] Error 1`).
- **Nenhuma suíte achada é falha**, com a mensagem no stderr, e o mesmo vale pra uma busca
  incompleta (o `find` saiu com erro, ex: diretório ilegível): aí nenhuma suíte roda. Sem isso,
  uma descoberta quebrada (diretório renomeado, permissão) diria que passou sem rodar tudo. O
  código de saída do `find` sai de `wait "$!"` depois do `mapfile ... < <(find_suites)`, já
  que o `set -e` não enxerga falha dentro de uma process substitution.
- **Script bash e `Makefile` fino:** numa receita de `make` (que roda em `/bin/sh`, com `$$` e
  `\`), o `shellcheck` não enxerga o código e as regras de "Qualidade de código bash" não
  valem. No script elas valem sem exceção, ele tem a própria suíte e roda sem `make`. Fica em
  `tests/` da raiz, o lugar dos testes do repositório como um todo; `lib/` é só pra
  bibliotecas carregadas com `source`.
- **`make` sem alvo roda o `test`** (`.DEFAULT_GOAL := test`): é o único alvo, rodar os testes
  não grava nada no repositório (as suítes usam `mktemp`), e um alvo de ajuda seria uma lista
  a manter à mão (o problema da seção "README genérico"). Quando entrar um segundo alvo (ex:
  `shellcheck`), a escolha deve ser revista. `test` é `.PHONY`, pra um arquivo chamado `test`
  na raiz não fazer o `make` achar o alvo em dia. O `.NOTPARALLEL:` é garantia explícita:
  hoje a receita é uma só, e o `make -j4 test` já roda tudo em série, mas o alvo pode um dia
  ser dividido em um por suíte.
- **A suíte do runner (`tests/test_run_all.sh`) nunca roda o `make test` real:** seria
  recursão, já que ela mesma é achada por ele. Cada caso monta, no `TEST_TMP`, um repositório
  de mentira com cópias do `Makefile` e do `tests/run_all.sh` reais e suítes falsas de poucas
  linhas, sem bit de execução, que gravam o próprio caminho num log. Todo `make` dos casos roda
  com `env -u MAKEFLAGS -u MFLAGS -u MAKELEVEL`: rodando dentro de um `make -j4 test`, a suíte
  herdaria o `-j` e o jobserver, e o caso não mediria só o que ele mesmo passou (mesmo
  racional do `env -u PLAYWRIGHT_BROWSERS_PATH` de `jail_expected_argv`). A serialidade do
  `-j4` é provada com uma trava (`mkdir`) nas suítes falsas, e cada caso que prova uma exclusão
  ou uma proteção tem controle positivo (rodados à mão, os arquivos excluídos gravam no log;
  sem o `.PHONY`, o arquivo `test` impede o `make test`; em paralelo, a trava acusa).

## Menus: opção "Sair (q)"

**Regra:** todo menu do `auto_scrum.sh` passa por `select_menu <variável> <opção>...`;
nunca um `select` direto. O helper acrescenta "Sair (q)" como último item numerado, então
um menu novo ganha a saída só por usar o helper. Um teste estrutural garante que o script
tem uma única linha `select`, dentro de `select_menu()`.

**Contrato de `select_menu`:**
- mostra as opções do chamador e depois "Sair (q)", numerados pelo próprio `select`, sem
  mudar o número das opções que já existem;
- grava em `<variável>` o **índice base 0** da opção escolhida, via `printf -v` (mesmo
  padrão de `read_required`);
- o "Sair" é decidido pela **posição** (o índice logo depois das opções do chamador), não
  pelo texto — um projeto chamado `Sair (q).conf` continua sendo um projeto comum;
- só `q` e `Q` **exatos** encerram; ` q`, `qq`, `quit` e `sair` são inválidos, como
  qualquer outra entrada que o `select` não aceite (mostram `Opção inválida, tente
  novamente.` e repetem a pergunta);
- o índice sai de `[ "$REPLY" -eq N ]`, nunca de `$((REPLY - 1))`: o `select` aceita `08`
  como 8, e o `[ -eq ]` também, mas a aritmética do bash lê `08`/`09` como octal inválido e
  derrubaria o script. O `-eq` só roda depois de conferir que a opção veio preenchida,
  senão `REPLY=x` geraria "integer expression expected";
- as locais usam o prefixo `__menu_`, pra não sombrear a variável do chamador no
  `printf -v`, e `REPLY` também é local: o que o usuário digitou não vaza pro escopo global
  (o `select` continua preenchendo o `REPLY` local normalmente);
- **nunca** chame um menu dentro de `$(...)`: a saída é por `exit`, que só encerraria o
  subshell — o script seguiria e a mensagem de saída seria engolida pela captura. É por
  isso que o resultado volta por `printf -v`, e os três menus rodam no shell principal
  (`main` chama `resolve_stack`, `choose_type` e `init_project` diretamente). Um teste
  estrutural garante que nem `select_menu` nem as três funções de menu aparecem depois de
  `$(`.

**A saída:** `quit_from_menu` é o único caminho, com o texto `Saindo. Nada foi feito.` só nessa
função, no **stdout**, e `exit 0`. Sair é uma ação normal, como responder `N` na confirmação de
envio, que também usa stdout e código 0. É `exit`, e não `return`, porque o menu de projetos
roda antes do principal, e voltar até `main()` exigiria um sinal a mais em cada chamador. Não
há efeito colateral porque a ordem já garante isso: em `main()`, o `mkdir -p "$LOGS_DIR"`, o
log e o lançamento do `claude` (`jail_exec`) só acontecem depois de `choose_type`; no `--init`,
o arquivo e o `mkdir -p "$PROJECTS_DIR"` só acontecem depois da escolha do destino. Os testes
de ponta a ponta (com um `claude` falso no `PATH` e um controle positivo que prova que o stub é
usado) protegem essa ordem.

**Ctrl+D continua igual** (fora do escopo do ticket, mas o caminho foi tocado e ganhou teste
de regressão): no EOF, `select_menu` deixa a variável vazia e retorna 1, o mesmo status do
`select` puro. `choose_type` e `resolve_stack` fazem `select_menu ... || return 1`, e o
`set -e` de `main()` encerra com código 1, sem mensagem; `init_project` usa
`if ! select_menu ...` e mantém o `Cancelado. Nada foi alterado.` no stderr. Atenção: chamar
a função num `if !` ou antes de `||` desliga o `set -e` **dentro** dela. Isso é aceitável
porque o corpo de `select_menu` só tem `select`, `printf -v`, `echo`, `[ ]` e
`quit_from_menu`, e o `exit` funciona do mesmo jeito. Não acrescente ao helper comandos que
dependam do `set -e`.

**Por que `q`:**
- é a tecla de sair de `less`, `man`, `top`/`htop` e do pager do `git`, então quem usa
  terminal já a conhece;
- `s` (de "sair") não serve: no próprio script, `s` já significa "sim" nas confirmações
  `(s/N)`, e a mesma tecla teria sentidos opostos em telas vizinhas;
- um número fixo também não serve: o número do Sair muda conforme as flags e a quantidade
  de projetos, e o `select` não mostra um `0`.

**"Nenhum" continua comparado pelo nome** no menu de projetos
(`"${names[$choice]}" = "Nenhum"`), como antes. Trocar pra posição mudaria o comportamento
de um projeto chamado `Nenhum.conf`, e esse caso não faz parte do ticket.

**Fora de escopo, de propósito:**
- prompts de texto (`read -p`, inclusive o "Nome do projeto" do `--init` e a confirmação
  `(s/N)`) e o loop do editor em `read_via_editor`;
- mudar o comportamento do Ctrl+D (só ganhou testes de regressão) e tratar Ctrl+C (`trap`);
- aceitar `sair`, `quit`, `qq` ou `q` com espaços em volta;
- um projeto chamado `Nenhum.conf`, que continua se confundindo com a opção "Nenhum".

## README genérico: sem repetir detalhe que muda com o código

O `README.md` descreve uso e comportamento estável: como rodar, de onde vem a configuração e em
que ordem, a regra de formato, o que é versionado onde, notas de migração. Listas que crescem a
cada ticket — chaves de configuração, quais etapas usam cada flag, opções de menu — **não** vão
pro README: ficam no código, neste `ABOUT.md` ou nos comentários do esqueleto gerado por
`--init`. A fonte única da lista de chaves da configuração do projeto é o bloco de registro
(`config_register_key`) no topo do `auto_scrum.sh`, de onde saem o esqueleto e os avisos (ver
seção "Registro de chaves da configuração"). O README remete a essas fontes em vez de
copiá-las. Pelo mesmo motivo, o README não traz um exemplo de arquivo de configuração completo:
ele teria de listar todas as chaves. O exemplo completo é o arquivo gerado por `--init`, e o
README só ilustra o formato com uma linha preenchida.

**Por quê:** o README não tem teste que pegue desatualização, e cada flag nova exigiria
lembrar de editá-lo. O ticket 8 já mostrou isso acontecendo: uma frase do passo 1 de "Uso"
("só é usado de fato por Tech Leader/Desenvolvimento") foi mexida e continuou errada, já que
`JIRA_ENABLED`/`SENTRY_ENABLED`/`PERMISSION_ENABLED` afetam o menu e o prompt do PO.

## Convenção do arquivo de plano (Tech Leader → Dev/Review)

Tudo o que é de um ticket fica em `.nick/docs/${JIRA_ID}/` no projeto alvo (`NICK_DOCS_DIR`,
em `auto_scrum.sh`), com nome fixo:
- `plan.md`: o plano do Tech Leader (`tech_leader.md` manda gravar em
  `.nick/docs/${JIRA_ID}/plan.md`; `tech_leader_jira.md`, em `.nick/docs/<ID-do-ticket>/plan.md`,
  com texto literal — ver seção `tech_leader_jira`). É o padrão de `PLAN_PATH` em
  `ask_questions_development`/`ask_questions_review`, então normalmente não precisa ser
  informado. O plano também precisa terminar com duas seções fixas: `## Critérios de aceite`
  (obrigatória — herdada do ticket ou definida pelo Tech Leader) e `## Habilidades
  necessárias`;
- `review.md`: o resultado do Review (aprovado, ou lista de problemas), em `REVIEW_PATH`,
  calculado a partir de `JIRA_ID` em `ask_questions_review` (não é perguntado, pois é sempre
  o mesmo tipo de conteúdo);
- os prints de validação e outros artefatos que comprovem o trabalho: `development.md` e
  `review.md` têm, cada um, um parágrafo literal mandando salvá-los ali (papéis diferentes,
  então não é bloco compartilhado — ver "DRY entre templates do mesmo papel"). Usa só
  `${JIRA_ID}`: nenhuma variável nova no `envsubst`.

Ninguém pré-cria `.nick/docs/<ID>/`: quem grava é o agente (o `Write` do Claude cria o
diretório), e o `.nick/.gitignore` que ignora a pasta já existe antes (ver "Pasta `.nick/` no
projeto alvo").

**Por que nome fixo, sem `<resumo-curto>`:** antes do ticket 24, o plano ia em
`docs/plans/${JIRA_ID}_<resumo-curto>.md`, e Dev/Review o achavam por um glob
(`${JIRA_ID}_*.md`), que exigia mandar o Review ignorar o `${JIRA_ID}_review.md` da mesma
pasta. Com uma pasta por ticket, o ID já identifica tudo, o caminho é exato e o review mora
ao lado do plano com outro nome — o glob e a frase de "ignore o review" saíram.

**Validação do ID (`jira_id_valid`/`read_jira_id`).** O ID vira nome de pasta, então é
validado antes de virar caminho, nas três etapas que o pedem (Tech Leader, Dev e Review),
com a pergunta `ID do Jira (ex: SC-123): ` (saiu o "/ branch"). `read_jira_id` repete
`read_required` até `jira_id_valid` aceitar, gravando via `printf -v` (locais com prefixo
`__jira_`, como em `select_menu`). Recusa: vazio (do `read_required`), qualquer `/`, qualquer
`..`, exatamente `.` (`.nick/docs/./plan.md` cairia em `.nick/docs/plan.md` — acréscimo do
plano do ticket 24), qualquer `[[:space:]]` e qualquer `[[:cntrl:]]` (via `case`;
`[:cntrl:]` e não `[:print:]`, pelo mesmo motivo de `config_warn_invalid_toggle`: não
recusar acento sob `LC_ALL=C`). A mensagem não ecoa o valor digitado, que pode ter caractere
de controle. `tech_leader_jira` não pede o ID (o agente busca no Jira), então o nick não
tem como validar ali. A frase "Confirme que está na branch ${JIRA_ID}" de
`development.md`/`review.md` continua como está. **Fora de escopo, de propósito:** trocar a
lista negra por lista branca (ex: `[A-Za-z0-9._-]` mais acentos, sem começar com `-` nem `.`).
O review do ticket 24 apontou que IDs como `$(id)` ou `*` passam e entram literais nos
caminhos do prompt. Não há travessia de caminho, e o ID é digitado pelo próprio usuário, então
o risco é baixo. Fica pra um ticket futuro, porque muda D4.

**Plano no lugar antigo (`warn_legacy_plan`).** Os planos de `docs/plans/` não são migrados.
No Dev e no Review, depois de ler o ID e **antes** de perguntar o arquivo do plano (assim o
usuário já digita o caminho na pergunta seguinte, sem rodar de novo), se
`.nick/docs/${JIRA_ID}/plan.md` não existe, cada `docs/plans/"$JIRA_ID"_*.md` do cwd (fora o
`_review.md`, e com o glob sem match pulado) gera uma linha de aviso no stderr com o caminho
achado. É o único lugar do `auto_scrum.sh` que cita `docs/plans/` (um teste estrutural cobra
isso, e outro que nenhum template o cita).

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
regras de plano (metodologia TDD — trecho que depende de `TDD_ENABLED`, ver seção
"Metodologia TDD por projeto" —, seções finais obrigatórias `## Critérios de aceite`/
`## Habilidades necessárias`, convenção `.nick/docs/<ID>/plan.md`), entrada
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
(`.nick/nick.conf` no cwd ou `auto_scrum/projects/<name>.conf` — ver seção "Configuração do
projeto: cwd primeiro, projects/ como fallback" abaixo), no formato
`SENTRY_ENABLED="true"/"false"` (opt-in — vazio conta como `"false"`; só `"true"`, `"false"` e
`""` são aceitos, e qualquer outro valor desliga a opção com aviso; a única exceção é
`TDD_ENABLED`, ligada por padrão — ver seção "Metodologia TDD por projeto"), lida por uma função
`build_*_blocks()` em `auto_scrum.sh` (mesmo padrão de `build_stack_blocks()`/
`STACK_DESCRIPTION`) que monta o bloco de texto correspondente só quando a flag está ativa. O
template usa a variável de bloco (ex: `${SENTRY_BLOCK_TL}`) no lugar do texto fixo — nunca
um `if` dentro do `.md`, porque `envsubst` não suporta condicional. Quando o bloco precisa
valer pra vários templates de um mesmo papel, o bash o embute dentro de um bloco
compartilhado em vez de o template referenciá-lo direto: `SENTRY_BLOCK_PO`,
`PERMISSION_BLOCK_PO` e `PRODUCTION_BLOCK_PO` entram em `PO_VALIDATION_BLOCK` via
`build_po_blocks()`, e só o bloco de fora é exportado e passado ao `envsubst` (que não
expande recursivamente).

Toda variável do `envsubst`/`export` em `main()` precisa ser usada em algum template, e toda
variável usada em template precisa estar nas duas listas — um teste estrutural compara os
três conjuntos. Isso vale pra qualquer variável nova: uma variável de template fora do
`envsubst` sobraria como `${...}` literal no prompt, e uma variável exportada sem uso em
template é código morto. O teste genérico não pega uma variável de outro `TYPE` usada num
template cujo `TYPE` não a preenche: ela está nas listas, então seria substituída
silenciosamente por vazio. Pra isso existe um teste de conjunto exato por template (via
`template_vars` em `test_auto_scrum.sh`), mas hoje só `po.md`, `po_sentry.md`,
`development.md` e `review.md` o têm.
Template novo, ou que ganhe variável nova, deve ganhar o seu.

Toda flag nova é uma chamada `config_register_key` no bloco de registro do topo do
`auto_scrum.sh` (ver seção "Registro de chaves da configuração"): o leitor (`config_load`, em
`lib/config.sh`) ignora qualquer chave fora do registro, então uma flag só escrita no arquivo
nunca chegaria ao script. A mesma chamada já a põe no esqueleto e nos avisos, e um teste
compara as chaves do esqueleto com `CONFIG_KEYS`. A flag nova também precisa terminar em
`_ENABLED` e ser registrada com padrão `"false"` (chave de texto entra com `""`; a exceção é
`TDD_ENABLED`, que entra com `"true"`): é o sufixo que a põe na checagem de valor
(`config_is_toggle_key`), o padrão vem do registro (`config_key_default`), e o próprio
`config_register_key` recusa um padrão incompatível com o tipo, além do teste de consistência
do esqueleto — ver seção "Validação da configuração do projeto: chave ausente e valor
inválido".

**Por quê:** manter esse tipo de decisão condicional em bash, não no template, é o mesmo
racional de `build_stack_blocks()` — e opt-in (padrão desligado) evita que um projeto novo
criado via `--init` puxe menção a uma ferramenta que ele não usa sem querer. `TDD_ENABLED` é
a exceção, porque ali o padrão é o comportamento que todo projeto já tinha (ver seção
"Metodologia TDD por projeto"); uma flag nova só copia essa exceção por um motivo do mesmo
tipo.

## Análise de impacto em produção no PO (`PRODUCTION_ENABLED`)

`PRODUCTION_ENABLED` (opt-in, no mesmo formato das flags da seção acima) controla se a
pergunta "Existe impacto em produção?" entra na análise obrigatória do PO.
`build_production_blocks()` monta `PRODUCTION_BLOCK_PO` só quando o valor é `"true"` exato.
Vazio, `"false"`, `"TRUE"`, `"sim"` ou qualquer outro valor desliga; os que não são `"true"`,
`"false"` nem vazio (ex: `"TRUE"`, `"sim"`) desligam com o aviso de valor.
`build_po_blocks()` o embute em `PO_VALIDATION_BLOCK` logo depois do bloco de permissão
(`PERMISSION_BLOCK_PO`). Por estar nesse bloco compartilhado, vale pros quatro pontos de
entrada do PO (`po`, `po_discussion`, `po_jira`, `po_sentry`) sem mexer em template, e,
como os outros blocos embutidos, não é exportado nem passado ao `envsubst`. Com `"true"`,
`PO_VALIDATION_BLOCK` fica byte a byte igual ao de antes da flag, inclusive a ordem
permissão → produção (coberto por teste com referência fixa). Desligada, só a linha da
pergunta sai, sem deixar linha em branco.

**Aviso de migração.** O aviso específico de `PRODUCTION_ENABLED` (a função
`warn_missing_production_line`, que não existe mais) virou parte do aviso geral de chaves
ausentes — ver seção "Validação da configuração do projeto: chave ausente e valor inválido",
que também guarda as regras gerais (roda só no leitor, `config_load`, linha mal formatada
conta como presente, linha comentada e chave parecida não contam, "Nenhum" não avisa). Um
arquivo sem a linha `PRODUCTION_ENABLED` recebe esse aviso, que sugere
`PRODUCTION_ENABLED="false"` precedida de uma nota da chave (registrada em
`config_register_key` e lida por `config_key_note`). Regras
que continuam específicas desta chave, cada uma com o seu motivo:

- **Ela tem nota própria nos avisos**, dizendo que a pergunta de impacto em produção está
  desligada no prompt do PO. É a primeira flag cujo padrão desligado *remove* um
  comportamento que já existia (ver "Por quê" abaixo); nas outras flags opt-in, ficar
  desligada só deixa de acrescentar algo. (`TDD_ENABLED` também tem nota, por outro motivo:
  ver seção "Metodologia TDD por projeto".)
- **A nota mostra as duas saídas, não só a de ligar** (`"true"` pra ligar a pergunta,
  `"false"` pra mantê-la desligada). O aviso aparece em toda execução, e quem mais precisa
  silenciá-lo é justamente o projeto sem produção. Se o texto só ensinasse
  `PRODUCTION_ENABLED="true"`, essa pessoa teria que ir ao README pra descobrir que `"false"`
  também silencia, e poderia acabar ligando a pergunta que a flag existe pra tirar.
- **A nota não repete a pergunta literal** ("Existe impacto em produção?"). As checagens de
  ponta a ponta juntam stdout e stderr e procuram essa frase pra saber se ela chegou ao
  prompt, então um aviso com a mesma frase daria falso positivo.
- **O aviso é permanente.** Não tem data de expiração nem flag pra desligar, porque o script
  não tem como saber quando todas as configurações antigas foram migradas (elas ficam em
  outras máquinas e em repositórios de outros times). A linha com `"true"`, `"false"` ou `""`
  registra a decisão do projeto e tira a chave do aviso de ausentes. Uma linha com valor
  inválido (`"TRUE"`, `"sim"`) também tira a chave desse aviso, mas recebe o aviso de valor,
  que termina com a mesma nota e também diz que a pergunta está desligada. Um projeto sem
  produção usa `PRODUCTION_ENABLED="false"`.
- **O `--init` já traz a linha** (`PRODUCTION_ENABLED="false"`, com comentário mandando ligar
  quando o projeto entrar em produção), então um arquivo novo já nasce com a decisão
  explícita e não dispara o aviso.

**Por quê:** num projeto que ainda não está em produção, a pergunta fixa fazia o modelo
inventar uma análise de impacto em produção sem valor nenhum. O aviso existe porque esta é
a primeira flag cujo padrão desligado *remove* um comportamento que já existia: as outras
(`SENTRY_ENABLED`, `PERMISSION_ENABLED`, `JIRA_ENABLED`) só acrescentam algo quando ligadas.
Sem o aviso, toda configuração criada antes da chave, inclusive os `.nick/nick.conf` já
versionados em projetos alvo, perderia a pergunta em silêncio.

**Fora de escopo, de propósito:**
- staging, homologação e outros ambientes: hoje nenhum prompt mudaria por causa deles, então
  viram ticket próprio quando houver um efeito concreto definido;
- pergunta interativa no `--init`: ele continua só gerando o esqueleto comentado, como faz
  pras outras chaves;
- prompts de Tech Leader, Desenvolvimento e Review: não falam de produção, então não há o
  que desligar neles.

## Metodologia TDD por projeto (`TDD_ENABLED`, ligada por padrão)

Os prompts de Tech Leader, Desenvolvimento e Review exigiam TDD e a execução da suíte de
testes em qualquer projeto. Num projeto sem testes automatizados (ex: site em HTML
estático), o modelo inventava testes, montava infraestrutura de testes que ninguém pediu ou
travava por não ter suíte pra rodar. `TDD_ENABLED="false"` desliga isso por projeto.

`build_tdd_blocks()` monta quatro blocos (mesmo mecanismo das outras flags; o `.md` só
referencia a variável):

| Bloco | Onde entra | Desligada (`"false"`) |
|---|---|---|
| `TDD_BLOCK_TL` | embutido em `TECH_LEADER_PLAN_RULES_BLOCK` (`tech_leader`, `tech_leader_jira`) | sai a frase da metodologia TDD; o plano não deve exigir testes automatizados nem planejar infraestrutura de testes que o ticket não peça |
| `TDD_CRITERIA_BLOCK_TL` | embutido em `TECH_LEADER_PLAN_RULES_BLOCK`, no fim do texto de `## Critérios de aceite` | o plano diz como cada critério será verificado (ligada, o bloco é vazio) |
| `TDD_BLOCK_DEV` | `development.md` | cada critério é verificado pela forma descrita no plano; a suíte só roda se existir; o resumo final diz como cada critério foi verificado |
| `TDD_BLOCK_REVIEW` | `review.md` | a suíte só roda se existir; cada critério é validado pela forma de verificação descrita no plano |

Os dois blocos `_TL` são embutidos (não exportados nem passados ao `envsubst`), por isso
`build_tdd_blocks` roda antes de `build_tech_leader_blocks` em `main()`.
`TDD_CRITERIA_BLOCK_TL` começa com `\n`, pra não deixar linha em branco quando vazio (mesma
técnica de `PERMISSION_BLOCK_PO`).

- **Ligada por padrão, ao contrário das outras flags.** TDD é o comportamento que todo
  projeto já tinha e que se espera; desligar é a exceção (projeto sem testes). Uma flag
  opt-in tiraria a TDD de toda configuração existente, inclusive dos `.nick/nick.conf`
  versionados em repositórios de outros times. Ligada, cada prompt sai byte a byte igual ao
  de antes da flag (coberto por testes de caracterização com referência fixa, que passaram
  com o código antigo). Depois, no ticket 26, essas referências fixas (`tdd_tl_rules_*`,
  `tdd_ref_*`) incorporaram o parágrafo de reaproveitamento de cada papel, montado a partir
  das constantes `reuse_*_text` — ver seção "Reaproveitar o que já existe antes de criar".
- **Só `"false"` exato desliga.** `"true"`, vazio, chave ausente, "Nenhum" e qualquer valor
  inválido (`"TRUE"`, `"False"`, `" false"`, `"sim"`) mantêm a TDD ligada. Um erro de digitação
  nunca pode remover a TDD de um projeto que tem testes; o erro contrário (TDD ligada num
  projeto sem testes) é o problema de antes da flag, e o aviso de valor já o torna visível. O
  leitor (`config_load`, em `lib/config.sh`) não muda: `"TRUE"` continua atribuído literal e
  recebe o aviso; quem decide é só `build_tdd_blocks`, com `[ "$TDD_ENABLED" = "false" ]` —
  essa é a regra de desligamento, não o padrão da chave.
- **As outras flags `_ENABLED` continuam opt-in** (padrão desligado). Uma flag nova não deve
  copiar esta exceção sem um motivo do mesmo tipo (desligar por padrão removeria um
  comportamento que todo projeto já tinha e que continua sendo o esperado).
- **Padrão por chave só no registro.** A chamada `config_register_key` de cada chave é o único
  lugar com o padrão dela (`"true"` pra `TDD_ENABLED`, `"false"` pras outras liga/desliga, `""`
  pras de texto); `config_key_default` (em `lib/config.sh`) só o devolve. Ele é usado pela
  linha sugerida do aviso de ausente e pelo esqueleto (`TDD_ENABLED="true"`). Não
  há outro `case` por chave.
- **Nota da chave no registro** (lida por `config_key_note`). Diz que a TDD continua ligada e
  que `"false"` é só pra projeto sem testes automatizados. Aparece antes de
  `TDD_ENABLED="true"` no aviso de chave ausente e no fim do aviso de valor inválido, no lugar
  de "a opção está desligada.".
- **"Nenhum prompt menciona TDD" vale pro texto do `auto_scrum`.** Uma `STACK_DESCRIPTION`
  escrita pelo usuário com "TDD" continua aparecendo — é dado do projeto.

**Fora de escopo, de propósito:**
- mudar o padrão das outras flags `_ENABLED` (continuam opt-in);
- separar "usa a metodologia TDD" de "tem suíte de testes" em duas flags;
- pergunta interativa no `--init`;
- mudanças nos prompts do PO;
- filtrar "TDD" de uma `STACK_DESCRIPTION` escrita pelo usuário.

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
regra de reaproveitar o que já existe antes de planejar algo novo — ver seção
"Reaproveitar o que já existe antes de criar" —, metodologia TDD — trecho que depende de
`TDD_ENABLED`, embutido via `TDD_BLOCK_TL`/`TDD_CRITERIA_BLOCK_TL` —, as duas seções finais
obrigatórias) é montada em `build_tech_leader_blocks()` e usada tanto em `tech_leader.md`
quanto em `tech_leader_jira.md` — antes da extração, esse parágrafo existia duplicado,
palavra por palavra, nos dois arquivos.

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

## Reaproveitar o que já existe antes de criar (Tech Leader, Dev e Review)

**A regra.** Antes de criar algo novo, os três papéis procuram o que já existe no projeto
alvo e preferem reaproveitar. Conta como existente: o código do projeto (classes, funções,
componentes e afins), as bibliotecas que o projeto já usa como dependência (não se adiciona
uma nova que faça o mesmo) e os padrões e convenções da base (o código novo é escrito do
mesmo jeito que o código parecido que já existe). Se o existente quase serve, a preferência é
adaptá-lo, e a adaptação entra no escopo do ticket, mas não pode mudar o comportamento do
que já funciona. Só vale com reuso real: semelhança superficial não justifica forçar uma
abstração comum. O que cabe a cada papel:
- **Tech Leader:** mapeia o reaproveitamento e o põe no plano: o que será reaproveitado, qual
  adaptação no código existente é necessária e como verificar que o comportamento anterior se
  manteve, com essa verificação nos critérios de aceite;
- **Desenvolvimento:** segue o reaproveitamento do plano; pro que o plano não previu, procura
  antes de criar; e avisa o analista antes de adaptar código existente de um jeito que o plano
  não previu;
- **Review:** trata como problema o código novo que duplica algo existente e a dependência
  nova que faz o mesmo que uma já presente, dizendo na sugestão de correção o que
  reaproveitar, e confere que as adaptações em código existente mantiveram o comportamento.

**Onde mora o texto.** O do Tech Leader fica em `TECH_LEADER_PLAN_RULES_BLOCK`
(`build_tech_leader_blocks()`), porque vale pros dois templates do papel (`tech_leader.md`,
`tech_leader_jira.md` — ver "DRY entre templates do mesmo papel"); entra logo depois da regra
de perguntar se faltar informação e antes do trecho de TDD. O de Desenvolvimento e o de Review
são literais em `development.md` e `review.md` (um template cada, papéis diferentes, textos
diferentes). Nenhuma variável nova no `envsubst`, nenhuma função nova. Os testes guardam o
texto exato de cada papel (`reuse_tl_text`, `reuse_dev_text`, `reuse_review_text` em
`test_auto_scrum.sh`), as duas frases-chave comuns aos três ("não pode mudar o comportamento
do que já funciona" e "semelhança superficial não justifica forçar uma abstração comum",
comparadas com as quebras de linha trocadas por espaço) e a ausência da regra nos prompts do
PO.

**Por quê:** num projeto que usa o nick, o Dev precisava criar uma página muito parecida com
uma que já existia e, em vez de extrair uma base comum, duplicou boa parte do código. Nenhum
prompt mandava procurar o que já existe: o Tech Leader planejava sem mapear o reaproveitamento,
o Dev seguia o plano e o Review não tratava duplicação como problema.

**Por que não tem flag.** É padrão do nick pra todo projeto. `TDD_ENABLED` existe porque nem
todo projeto tem suíte de testes; não existe projeto em que duplicar código seja o
comportamento desejado.

**Como a verificação segue `TDD_ENABLED` sem texto condicional.** O texto da regra é uma
constante, o mesmo byte a byte com a TDD ligada ou desligada e com qualquer
`STACK_DESCRIPTION`. A verificação de que o comportamento anterior se manteve não ganha texto
próprio por flag: o Tech Leader a põe nos critérios de aceite, e os blocos de TDD já decidem
como cada critério é verificado (ligada, pela suíte; desligada, pela forma descrita no plano,
via `TDD_CRITERIA_BLOCK_TL`/`TDD_BLOCK_DEV`/`TDD_BLOCK_REVIEW`). `build_tdd_blocks()` não muda.
Por isso o texto da regra não pode citar "TDD" (a variante desligada cobra que o prompt não
menciona TDD) nem ter `${`.

**Fora de escopo, de propósito:**
- prompts do PO (o PO não escreve código);
- uma flag pra desligar a regra;
- refatorar código existente sem relação com o ticket: a regra só manda reaproveitar e adaptar
  o que o próprio ticket precisa.

## auto_scrum roda com o cwd dentro do projeto alvo

`auto_scrum.sh` não recebe (nem precisa de) um path do projeto alvo como argumento — ele espera
ser executado com o diretório de trabalho já dentro do repositório desse projeto (ex: `cd
~/Projetos/git/siga-construcao && /caminho/pra/auto_scrum.sh`). O script não verifica nem avisa
se isso não for respeitado; rodar de outro lugar simplesmente faz o `claude` (lançado por
`jail_exec` no fim do script, dentro ou fora da jail) subir sem o `.mcp.json`, `CLAUDE.md` e
demais config do projeto errado — ou sem nenhuma, se rodado de um diretório qualquer.

**Por quê:** é o cwd que faz o Claude Code carregar sozinho o `.mcp.json` do projeto alvo
(é assim que a etapa de PO enxerga o MCP do Sentry — ver seção "Sentry via MCP na etapa
de PO" abaixo — sem o `auto_scrum` precisar saber nada sobre ele) e o `CLAUDE.md`/
`ABOUT.md` daquele projeto. Também é o que permite a etapa de Desenvolvimento editar o
código de verdade: ela roda `claude --permission-mode auto` no mesmo cwd, então as
mudanças caem no repositório certo.

O cwd também decide a configuração do projeto (`STACK_DESCRIPTION`/`*_ENABLED`) quando
existe um `.nick/nick.conf` nele: nesse caso `--projeto=<nome>` é ignorado (com aviso) e o
menu de projetos não aparece — ver seção "Configuração do projeto: cwd primeiro, projects/
como fallback" abaixo. É também no cwd que ficam a troca de arquivos e os docs de cada ticket
(seção "Pasta `.nick/` no projeto alvo"). Sem `.nick/nick.conf` no cwd, `--projeto=<nome>` (flag do
`auto_scrum.sh`) escolhe a configuração em `auto_scrum/projects/<name>.conf`, e aí sim não
tem relação com em qual diretório o comando roda — são coisas independentes que
coincidentemente usam o mesmo nome de projeto.

## Pasta `.nick/` no projeto alvo (configuração, troca e docs)

Tudo o que o nick guarda no projeto alvo fica em `.nick/`, no cwd:

```
.nick/
├── .gitignore        # exatamente: *  !.gitignore  !nick.conf   (versionado)
├── nick.conf         # configuração do projeto (versionada)
├── troca/            # troca de arquivos com o usuário (ignorada)
└── docs/
    └── <JIRA_ID>/    # plan.md, review.md, prints de validação (ignorada por padrão)
```

Em `auto_scrum.sh`: `NICK_DIR=".nick"`, `EXCHANGE_DIR="$NICK_DIR/troca"` e
`NICK_DOCS_DIR="$NICK_DIR/docs"`. O nome do `nick.conf` é de `lib/config.sh`
(`CONFIG_CWD_FILE_NAME`), e um teste de consistência garante que `$NICK_DIR/nick.conf` é igual
a ele e que a linha `!nick.conf` do `.gitignore` é o `basename` dele (a exceção não pode
desalinhar do nome lido pela biblioteca — o código monta a linha a partir da constante).

**`nick_dir_prepare`** (substituiu `exchange_dir_prepare`, do ticket 21) monta a árvore
inteira, menos o `nick.conf` (do `init_project`) e as pastas `docs/<ID>/` (dependem do ticket;
o agente as cria ao gravar): `mkdir -p` de `troca/` e `docs/` e, **só se `.nick/.gitignore`
não existir** (`[ -e ]`), grava exatamente `*`, `!.gitignore` e `!nick.conf`, sem comentário.
Regras:
- um `.gitignore` existente, editado ou não, nunca é sobrescrito; `troca/` e `docs/`
  existentes ficam como estão, com o que tiverem dentro;
- não cria `.gitignore` em `troca/` nem em `docs/` (o `*` de cima já as ignora, e o git não
  versiona pasta vazia). Um `.nick/troca/.gitignore` antigo (do ticket 21) não é tocado;
- é chamada em dois pontos: no `init_project` com destino no cwd e em `main()` ao confirmar o
  envio (`|| exit 1`), só pra completar o que faltar (projeto configurado por `projects/`,
  pasta apagada à mão, projeto migrado com `mv`). Cancelar o envio não cria nada;
- se não der pra criar (ex: `.nick/troca` é um arquivo, ou `.nick/` sem permissão de escrita),
  imprime só `Erro: não foi possível criar a estrutura .nick/ (.gitignore, troca/ e docs/) em
  <PWD>.` (a gravação do `.gitignore` fica num grupo `{ ...; } 2>/dev/null`, porque um
  `> arquivo 2>/dev/null` que falha mostra o erro cru do bash antes de o `2>` valer): o `init_project` retorna 1 sem
  gravar o `nick.conf`, e `main()` para antes do `claude`, porque o prompt promete a pasta.

**A árvore nasce no `--init`** (pedido do usuário): com destino no projeto, o `--init`
termina com `.gitignore`, `nick.conf`, `troca/` e `docs/` criados e mostra essa árvore na
saída, explicando o que é versionado. Assim o time vê e versiona a estrutura desde o primeiro
commit, sem depender de uma primeira execução. A ordem no `init_project` é: confirmação de
sobrescrita → `nick_dir_prepare` (cria `.nick/`, que o `nick.conf` precisa) → esqueleto → aviso
de legado, se houver. Respondendo N à sobrescrita, nada é criado nem completado.

**Por que um `.gitignore` só, em `.nick/`:** o ticket 21 punha um `.gitignore` com `*` dentro
de `.nick/troca/`. Com a configuração (versionada) e os docs (ignorados por padrão) na mesma
pasta, um arquivo na raiz de `.nick/` com exceções resolve tudo sem tocar no `.gitignore` do
projeto alvo. **Como versionar os docs** (decisão do time, vale pra todos porque o
`.gitignore` é versionado): acrescentar `!docs/` e `!docs/**`. `*` sem barra casa em qualquer
nível; `!docs/` reabre a pasta e `!docs/**` reabre tudo dentro dela, inclusive as subpastas
`docs/<ID>/`, que o `*` voltaria a ignorar. Um teste com repositório git temporário prova o
padrão (só `.gitignore` e `nick.conf` aparecem no `git status`) e a reabertura (o `plan.md`
passa a aparecer, `troca/` não). Os testes de git rodam por `git_isolated`
(`GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1`): um `core.excludesFile` global que ignore
`.nick/`, plausível em quem usa o nick, faria esses testes passarem ou falharem à toa (achado
do review do ticket 24). Pelo mesmo motivo, o teste do `.gitignore` deste repositório confere
com `check-ignore -v` que a regra vem do `.gitignore` da raiz, e não de `.git/info/exclude`.

**Por que `nick_dir_prepare` fica em `auto_scrum.sh` e não em `lib/`:** hoje só o
`auto_scrum` cria a pasta. Extrair pra biblioteca é assunto de quando existir uma segunda
ferramenta (mesmo racional dos riscos aceitos de `lib/config.sh`).

**Troca de arquivos (`.nick/troca/`):** onde o usuário põe arquivos pro agente ver (prints,
anexos) e onde o agente salva o que entrega pra conferência (ex: relatórios).
`exchange_block` é a seção que fecha o prompt de toda etapa (anexada ao log depois do
`envsubst`, então aparece no prompt final mostrado antes da confirmação). Desde o ticket 24, o
bloco diz que a pasta recebe o que o agente entrega "menos o que o prompt mandar gravar em
outro lugar (ex: os prints de validação de um ticket)" — sem isso, ele contradiria o parágrafo
de `development.md`/`review.md` que manda os prints pra `.nick/docs/<ID>/` — e que o
`.gitignore` é o de `.nick/`. As referências fixas de prompt dos testes de TDD (`tdd_ref_*`)
são do template: `tdd_log_text` tira o bloco do fim, e o bloco tem os testes próprios
(`exchange_expected_block`).

**Por quê dentro do projeto:** com a jail ligada, só o projeto é visível e gravável. Uma
pasta fora (`~/Downloads`, `~/nick-troca`) exigiria um `--rw-map`, abrindo exceção na regra
de que a jail só grava no projeto, e seria compartilhada entre projetos. Dentro do projeto
não muda nada na jail, cada projeto tem a sua, e o caminho é o mesmo com ou sem jail. Por isso
o bloco vai no prompt sempre, e não no aviso da jail (`jail_agent_notice`).

**Verificação manual (2026-11-02, ticket 21):** conferido pelo usuário num projeto real: a
pasta de troca é criada e o agente lê e grava arquivos nela. A verificação do ticket 24 segue
o roteiro `.nick/troca/24_teste_manual.md`.

## Configuração do projeto: cwd primeiro, projects/ como fallback

A configuração de cada projeto alvo (a stack e as flags, ou seja, as chaves registradas
com `config_register_key`) pode morar em dois lugares, e `resolve_stack()` procura nesta ordem:

0. **Antes de tudo, o legado:** `config_cwd_legacy_check` (em `lib/config.sh`) na primeira
   linha útil de `resolve_stack`, com `|| exit 1` (ver "Migração do `.nick.conf`" abaixo).
1. **`.nick/nick.conf` no cwd** (`$PWD/.nick/nick.conf`, de `config_cwd_file` e da constante
   `CONFIG_CWD_FILE_NAME`, em `lib/config.sh`). Se
   existir, vence tudo: é carregado, o script imprime no stdout
   `Configuração do projeto carregada de: <caminho absoluto>` e retorna — sem menu de
   projetos e sem aviso de `.sh` legado. Se `--projeto=<nome>` também foi passado, sai no
   stderr `Aviso: --projeto=<nome> ignorado — a configuração do diretório atual (<caminho>)
   tem precedência.`, e o nome **não é validado** (`--projeto=inexistente` não dá `exit 1`,
   já que a flag nem é usada).
2. **`--projeto=<nome>`** → `auto_scrum/projects/<nome>.conf` (erro se não existir).
3. **Menu** com `auto_scrum/projects/*.conf` + "Nenhum" + "Sair (q)" (ver seção "Menus:
   opção Sair (q)" — o Sair encerra o script sem carregar nada).

A linha "carregada de" só existe no caminho do cwd; nos caminhos 2 e 3 a saída é a de
sempre, exceto pelo aviso de `.sh` legado (abaixo).

**Nome `nick.conf` dentro de `.nick/`, só no cwd.** O arquivo vai ser versionado dentro do
repositório de outro time, então não pode ser apanhado pelo lint/pre-commit/CI desse time:
dentro de uma pasta oculta (não polui a listagem — antes do ticket 24 era o dotfile
`.nick.conf`, na raiz, pelo mesmo motivo); sem `.sh` (fora de `shellcheck $(git ls-files
'*.sh')`, `find -name '*.sh'` e dos hooks que filtram tipo shell); sem shebang (o `identify` do
pre-commit detecta shell por shebang em arquivo sem extensão conhecida); sem `.env` (não é
carregado por `python-dotenv`/`django-environ`/`docker compose` nem tratado como segredo por
scanners); sem `.yml`/`.json`/`.toml`/`.ini` (fora de `check-yaml`/`check-json`/prettier). Os
hooks genéricos de texto (`trailing-whitespace`, `end-of-file-fixer`, `mixed-line-ending`)
valem pra qualquer arquivo, então o esqueleto do `--init` sai sem espaço no fim de linha, com
LF e terminando em `\n` — coberto por teste. A busca é só no `$PWD`, sem subir pra diretórios
pais ou pra raiz do git: a convenção já é rodar da raiz do projeto alvo, e subir criaria
ambiguidade (qual arquivo vence num monorepo?) sem pedido concreto.

**Leitura por `CHAVE="valor"` (`config_load`, em `lib/config.sh`; no `auto_scrum`, chamado
pela casca fina `load_project_config`), sem `source` nem `eval`.** O mesmo
leitor vale pro cwd e pra `projects/` — um mecanismo de carga só. Gramática: depois de tirar
espaços/tabs do começo e do fim da linha (o que também remove o `\r` de CRLF), só é aplicada
uma linha `CHAVE="valor"` com `CHAVE` em `CONFIG_KEYS` (comparação exata) e `valor`
sem aspas duplas dentro. O valor é atribuído **literalmente** com `printf -v` — sem expansão
de `$VAR`, `$(...)`, crase ou escape —, e o nome da variável vem sempre de
`CONFIG_KEYS`, nunca do arquivo. Todo o resto é ignorado em silêncio: comentários,
linhas em branco, comandos e chaves desconhecidas (inclusive globais do script como `PATH` e
`TYPE`, que um `source` sobrescreveria). A exceção é uma linha de chave conhecida que foge do
formato: tanto a que começa com `CHAVE=` (`JIRA_ENABLED=true`, `JIRA_ENABLED="true" && touch
x`, `JIRA_ENABLED="true" # comentário`) quanto as formas "quase certas" `export CHAVE=...` e
`CHAVE = ...`/`CHAVE ="..."` (espaço logo depois do nome — `STACK_DESCRIPTION_EXTRA = "x"`
continua sendo chave desconhecida, sem aviso). Ela é ignorada com um único aviso no stderr,
com arquivo e número da linha (texto único em `config_warn_malformed_line`); essas formas
continuam **não** sendo aceitas, só deixam de ser descartadas em silêncio. Além do aviso de
formato, existem o de chave ausente e o de valor inválido (ver seção "Validação da
configuração do projeto: chave ausente e valor inválido"). Aspas simples, valores sem aspas
e comentário no fim da linha ficam de fora de propósito, pra gramática ser pequena e fácil
de testar. Variáveis locais do leitor ficam em minúsculo — uma local com o nome de uma chave
faria o `printf -v` alterar a local em vez da global —, e isso agora é garantido pela
estrutura: `config_register_key` só aceita nome de chave em maiúsculo (`^[A-Z][A-Z0-9_]*$`).

**`projects/` passou de `.sh` pra `.conf`.** Os arquivos deixaram de ser executados, e manter
`.sh` passaria a ideia errada de que são `source`ados (e editores/`shellcheck` os tratariam
como script). O conteúdo não mudou: já era `CHAVE="valor"` com comentários. `.sh` **não** é
lido como fallback (seriam dois mecanismos de novo) nem renomeado pelo script (mover arquivo
do usuário sem pedir é surpresa): `warn_legacy_project_files` só avisa no stderr, pra cada
`projects/*.sh`, com o `mv` sugerido — e só nos caminhos 2 e 3, nunca quando a configuração
vem do cwd.

**`--init` pergunta o destino** (`.nick/nick.conf` no cwd primeiro, por ser o local
recomendado, ou `auto_scrum/projects/<nome>.conf`), gera o mesmo conteúdo nos dois casos
(`print_project_skeleton`, que chama `config_print_skeleton "auto_scrum"`) e pergunta antes
de sobrescrever em ambos. Com destino no cwd, cria também o resto da árvore `.nick/` e a
mostra na saída (ver "Pasta `.nick/` no projeto alvo").

**Migração do `.nick.conf` (ticket 24).** A configuração do cwd saiu de `$PWD/.nick.conf` pra
`$PWD/.nick/nick.conf`. `lib/config.sh` tem as duas constantes (`CONFIG_CWD_FILE_NAME` e
`CONFIG_CWD_LEGACY_FILE_NAME`) e a função `config_cwd_legacy_check`, porque é ela que sabe onde
mora o arquivo do cwd:
- só o antigo existe → erro no stderr com o comando de migração (`mkdir -p .nick && mv
  .nick.conf .nick/nick.conf`) e retorno 1. A biblioteca nunca chama `exit`: quem para é o
  chamador;
- os dois existem → vale o novo, com um aviso no stderr de que o antigo é ignorado e pode ser
  apagado, e retorno 0. *(Acréscimo do plano: sem isso, quem editasse o arquivo antigo por
  engano não entenderia por que nada muda.)*;
- caso contrário → silêncio, retorno 0. O arquivo antigo nunca é lido, movido nem apagado
  (mover arquivo do usuário sem pedir é surpresa — mesmo racional de `projects/*.sh`).

Onde o `auto_scrum` chama: **`resolve_stack`**, antes do passo 1, vale pros três caminhos
(cwd, `--projeto=`, menu). Antes do ticket 24, um `.nick.conf` no cwd tinha precedência sobre
`--projeto=`; seguir com `projects/` ou "Nenhum" rodaria o projeto sem a configuração dele —
principalmente sem a jail. Como `resolve_stack` roda antes de qualquer menu de tipo, log ou
`claude`, a saída ≠ 0 acontece antes de abrir o `claude`. `--requisitos-jail` não carrega
configuração e não checa.

**O `--init` não para no legado (decisão do usuário, revendo o plano).** O plano do ticket 24
fazia o `--init` com destino no cwd parar como a execução normal, pra não gerar um
`.nick/nick.conf` ao lado do antigo e perder os valores em silêncio. Na validação, o usuário
preferiu o contrário: o `--init` é o caminho de quem quer a pasta nova pronta, com o
esqueleto padrão, e o antigo é a fonte de onde copiar os valores. Por isso `init_project` não
chama `config_cwd_legacy_check`: segue o fluxo de sempre (sobrescrita, `nick_dir_prepare`,
esqueleto, árvore na saída) e, se `$PWD/.nick.conf` existir, termina com um aviso no stderr
(`Aviso: o arquivo antigo <PWD>/.nick.conf não foi lido nem alterado — copie os valores dele
para <PWD>/.nick/nick.conf e apague o antigo.`). O aviso é o que impede a perda silenciosa. Com
N na sobrescrita (os dois arquivos já existem), nada muda e nada é avisado; a execução normal
já avisa dos dois arquivos. O destino `projects/` não olha o legado. A execução normal continua
parando só com o antigo: é ela que rodaria o projeto sem configuração, e sem a jail.

**`.gitignore` do `nick_secrets`:** lista `auto_scrum/projects/*` com a exceção
`!auto_scrum/projects/.gitkeep` (o placeholder que mantém o diretório no git continua
versionável — sem a exceção, se ele fosse removido e recriado, o `git add` passaria a exigir
`-f`). A regra ampla cobre de uma vez os `*.conf` (formato local atual), os `*.sh` legados
ainda não renomeados em alguma máquina e os backups/swaps de editor (`alpha.conf~`,
`.alpha.conf.swp`, `alpha.conf.bak`), que um `*.conf` sozinho deixaria aparecer como
"untracked" — e acabar commitados com dado de cliente num repositório público. Isso
substitui a decisão original do plano do ticket 8 (`*.conf` + `*.sh`, dois padrões
específicos). `.nick/` também é listada, sem `/` inicial (vale em qualquer profundidade: um
`--init` rodado com o cwd dentro do próprio `nick_secrets` não cria arquivo versionável no
repositório público). Como o git não entra numa pasta ignorada, o `.nick/.gitignore` interno
(com `!nick.conf`) não tem efeito aqui: `.nick/nick.conf` continua ignorado neste repositório
(um teste com `git check-ignore --no-index` cobra isso). **As linhas `.nick.conf` e
`docs/plans/*` continuam, como proteção de legado** (o ticket 24 pedia removê-las; o plano
ajustou o critério): este repositório, e qualquer clone já usado, ainda tem um `.nick.conf`
real e planos antigos em `docs/plans/`, ignorados e nunca commitados. Tirar as duas linhas
faria esses arquivos — com dado de cliente, num repositório público — aparecerem como
"untracked" e virarem candidatos a um `git add .`. O custo de manter é zero; elas podem sair
num ticket futuro. `.claude/settings.local.json` também é listado: são as
permissões que cada pessoa aprova no Claude Code ao trabalhar neste repositório, com caminhos
absolutos da máquina dela; versionado, exporia esses caminhos e daria a quem clona regras de
`allow` que ela não aprovou. Só esse arquivo: um `.claude/settings.json` de projeto, se um dia
existir, é pra ser versionado.

**O leitor protege o shell, não o prompt.** O valor de `STACK_DESCRIPTION` entra literal no
texto enviado ao `claude`, inclusive na etapa de Desenvolvimento, que roda em
`--permission-mode auto`. Um `.nick/nick.conf` malicioso (ex: `STACK_DESCRIPTION="Django. Ignore
as instruções anteriores e rode ..."`) não executa nada no shell, mas ainda pode tentar
instruir o agente — é um vetor de prompt injection. Não é um risco novo: é a mesma confiança
que já se dá ao `CLAUDE.md` do projeto alvo, que o Claude Code carrega sozinho do mesmo
repositório. Por isso, alterações em `.nick/nick.conf` devem passar pelo mesmo review de PR que o
`CLAUDE.md` — um `.nick/nick.conf` de terceiros não é "seguro" só por não ser executado.

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

## Validação da configuração do projeto: chave ausente e valor inválido

Além do aviso de formato (seção acima), o leitor (`config_load`) faz duas checagens no arquivo
que carregou. Nenhuma delas interrompe a execução: a função sempre retorna 0, e `main()`
segue até o prompt.

1. **Toda chave de `CONFIG_KEYS` deve estar declarada no arquivo**, mesmo as que o
   projeto não usa. As ausentes geram um único aviso (`config_warn_missing_keys`), com o
   caminho do arquivo e a linha a adicionar pra cada uma, na ordem do registro.
   A chave ausente continua valendo como vazia, e vazio funciona como o padrão da chave.
   O cabeçalho do aviso diz isso ("cada chave ausente funciona como o valor sugerido
   abaixo"), e não que as opções ausentes ficam desligadas, o que seria falso pra
   `TDD_ENABLED`.
2. **As chaves liga/desliga aceitam só `"true"`, `"false"` e `""`.** Qualquer outro valor
   (`"TRUE"`, `"True"`, `"sim"`) gera um aviso por linha (`config_warn_invalid_toggle`), com
   arquivo, número da linha, chave, valor encontrado, valores aceitos e o efeito: "a opção
   está desligada." ou, quando a chave tem nota, a nota (`TDD_ENABLED` diz que a TDD continua
   ligada).

**O que é liga/desliga: nome terminado em `_ENABLED`**, decidido só em `config_is_toggle_key`
(em `lib/config.sh`). As duas checagens e a validação do registro usam essa função. Uma flag
nova precisa seguir o sufixo e ser registrada com `"false"` (ou `"true"`, como `TDD_ENABLED`, a
única ligada por padrão); uma chave de texto entra com `""`. O que cada flag faz, e qual valor
a desliga, é do `auto_scrum`, não da biblioteca. O teste de consistência do esqueleto cobra
isso nos dois sentidos: cada chave com `"false"` ou `"true"` no esqueleto, trocada por `"sim"`,
tem de gerar o aviso de valor, e cada chave com `""` não. Não existe um array separado de
chaves liga/desliga porque ele seria mais um lugar a atualizar a cada flag nova, além da
chamada de registro — e todas as flags já seguem o sufixo.

**Valor padrão da linha sugerida.** Vem do registro (`config_key_default`), o único lugar com
padrão por chave: `"true"` pra `TDD_ENABLED`, `"false"` pras outras liga/desliga e `""` pra
texto. É exatamente a linha do esqueleto, e um teste confere que cada linha sugerida aparece
idêntica no arquivo gerado por `--init` (o script não lê o esqueleto em tempo de execução).
Esse padrão mantém o comportamento de antes: nas flags opt-in, vazio e `"false"` desligam
igual; em `TDD_ENABLED`, vazio e `"true"` mantêm a TDD ligada.

**Presença.** Conta como presente a linha válida com qualquer valor, inclusive inválido, e a
linha fora do formato: as duas já recebem um aviso próprio, que aponta arquivo e linha, e
somar o aviso de chave ausente seria ruído pro mesmo problema. Conta como ausente a linha
comentada, a chave de nome parecido (`JIRA_ENABLED_OLD`), o nome digitado errado
(`JIRA_ENABLE`) e o arquivo vazio: o leitor não vê a chave em nenhum deles, então ela de fato
vale como vazia. É por esse lado que o nome digitado errado aparece: `JIRA_ENABLE="true"`
continua sendo uma chave desconhecida, ignorada em silêncio, mas `JIRA_ENABLED` entra no
aviso de ausentes.

**Forma dos avisos.**
- O de chave ausente é um bloco só, e só a primeira linha começa com `Aviso:` (os testes
  contam avisos por ela, aceitando antes o prompt `#? ` que o `select` do menu de projetos
  deixa sem quebra de linha — sem isso, um aviso logo depois do menu passaria sem ser
  contado). Cada chave vem numa linha `  CHAVE="padrão"`. A nota da chave,
  quando existe, vai numa linha de comentário antes dela, e não no fim da linha: assim o
  bloco pode ser colado no arquivo como está, porque o leitor ignora comentários e tira os
  espaços do começo, enquanto um comentário no fim da linha geraria aviso de formato.
- O de valor inválido é um por linha, no padrão do aviso de formato, e só vale pra linhas que
  o leitor aceitou: uma linha fora do formato recebe só o aviso de formato, mesmo que o valor
  também seja ruim (`JIRA_ENABLED=TRUE`).
- O valor inválido continua sendo atribuído (a variável guarda `"TRUE"` literal): nas flags
  opt-in só `"true"` liga, e `TDD_ENABLED` só desliga com `"false"`. O comportamento do
  leitor não muda: o aviso só explica o efeito do valor.

**Nota por chave, só no registro** (o 3º argumento de `config_register_key`, lido por
`config_key_note`). É o único lugar com texto específico de chave, usado pelos dois avisos.
Hoje duas chaves têm nota: `PRODUCTION_ENABLED`, porque é a única flag cujo padrão desligado
remove algo que o prompt já tinha (ver seção "Análise de impacto em produção no PO"), e
`TDD_ENABLED`, porque é a única ligada por padrão e "a opção está desligada." diria o contrário
do comportamento (ver seção "Metodologia TDD por projeto"). Nas outras, o aviso de valor
termina com "a opção está desligada.". O nome era `config_key_off_note`; virou
`config_key_note` porque a nota de `TDD_ENABLED` não fala de opção desligada.

**O que os avisos não fazem, e por quê.**
- Não sugerem `--init`: ele sobrescreve o arquivo e apagaria os valores já preenchidos.
- Não mostram o valor de `STACK_DESCRIPTION`. Ele é texto livre, pode ser longo e pode trazer
  nome de cliente ou pista de infraestrutura, que é o motivo de essas configurações não
  ficarem neste repositório público. Como nunca é validado, não há por que mostrá-lo.
- Trocam os caracteres de controle do valor por `?` antes de imprimir: o valor vem de um
  arquivo que outro time pode editar e vai direto pro terminal. A troca usa `[[:cntrl:]]`, e
  não `[![:print:]]`, que com locale C trocaria também os bytes de `"não"` (um teste sob
  `LC_ALL=C` protege essa escolha). Ela vale só pro texto do aviso: a variável continua com o
  valor bruto. Limite aceito: os controles C1 (U+0080–U+009F, ex: `C2 9B`) só são
  reconhecidos com locale UTF-8; com locale C, ou como byte solto (`9B`), passam intactos.
  Filtrar os bytes `0x80`–`0x9F` quebraria letras acentuadas em UTF-8 (`É` é `C3 89`), e o
  risco é pequeno perto do que quem edita o arquivo já consegue — mesmo racional do
  parágrafo "O leitor protege o shell, não o prompt".
- Não interrompem a execução: a configuração continua utilizável, só com a chave no padrão
  dela (opção desligada, TDD ligada) ou vazia, que é o comportamento de antes das checagens.

**Onde rodam.** Só em `config_load`, o único leitor, então valem nos três caminhos
(cwd, `--projeto=` e menu) sem repetir código. Só o arquivo carregado é verificado: outros
`projects/*.conf` incompletos ou inválidos não geram aviso, porque não são eles que definem
essa execução. "Nenhum" no menu e o `--init` não chamam o leitor e por isso não avisam — o
`--init` nem quando sobrescreve um arquivo incompleto que já existe. O esqueleto já nasce com
todas as chaves e traz um comentário com as duas regras, então um arquivo novo carrega sem
aviso.

**Por quê:** uma chave nova nunca chega sozinha às configurações que já existem, inclusive
aos `.nick/nick.conf` versionados em repositórios de outros times, e um valor como `"TRUE"` ou um
nome digitado errado desligavam a opção sem nenhuma explicação. O aviso de migração do ticket
14 resolvia isso só pra `PRODUCTION_ENABLED`. Generalizá-lo evita um aviso novo a cada flag:
basta a flag ser registrada com `config_register_key` pra entrar no esqueleto e nas duas
checagens.

**Fora de escopo, de propósito:**
- aceitar `"TRUE"`, `"sim"` e parecidos como ligado: o leitor continua exigindo `"true"`
  exato, e o aviso já tira o silêncio, que era o problema;
- validar `STACK_DESCRIPTION`: é texto livre, sem valor inválido a definir;
- avisar chave desconhecida ou sugerir o nome certo: o aviso de chave ausente já pega o nome
  digitado errado pelo lado da chave certa;
- avisar quando não há arquivo ("Nenhum"): não há arquivo onde adicionar as linhas;
- corrigir ou completar o arquivo automaticamente: mexer no arquivo do usuário sem pedir é
  surpresa (mesmo racional de não renomear os `.sh` legados), e o aviso já traz as linhas
  prontas pra colar.

## Código compartilhado entre ferramentas (`lib/`)

O `nick_secrets` vai ter outras ferramentas com IA além do `auto_scrum`, e elas leem o mesmo
`.nick/nick.conf`. O que é genérico entre elas mora em `lib/`, com as suítes de teste em
`lib/tests/` (todas lá, inclusive a de uma biblioteca em subdiretório). **Convenção de
nome:** uma biblioteca que é só código fica num arquivo, `lib/<assunto>.sh`; uma que tem
arquivo de dados junto ganha um diretório, `lib/<assunto>/<assunto>.sh` + os dados (hoje só
`lib/claude/`, com o `permissions.json`). Hoje existem três bibliotecas:
- `lib/config.sh`: o leitor da configuração do projeto (`config_load`), os três avisos, a
  regra liga/desliga (`config_is_toggle_key`), a troca de caracteres de controle, o padrão e
  a nota por chave, o arquivo do cwd (`CONFIG_CWD_FILE_NAME`/`config_cwd_file`, só o cwd), a
  checagem do `.nick.conf` antigo (`CONFIG_CWD_LEGACY_FILE_NAME`/`config_cwd_legacy_check`) e o
  formato do esqueleto (`config_print_skeleton`). A biblioteca não conhece nenhuma chave
  concreta: cada ferramenta registra as suas (ver "Registro de chaves da configuração"
  abaixo);
- `lib/jail/jail.sh`: o mecanismo da jail (ai-jail) — verificação (`jail_check`), requisitos
  (`jail_print_requirements`), status (`jail_print_status`), opções do ai-jail por nível
  (`jail_options`, a fonte única delas), toolchains montadas (`jail_toolchain_dirs`), aviso
  ao agente (`jail_agent_notice`), navegadores do Playwright (`jail_playwright_browsers`),
  versão do ai-jail (`jail_version_output`, `jail_version_of`, `jail_version_supported`, com a
  versão mínima em `JAIL_VERSION_MIN`) e lançamento (`jail_exec`), com a lista padrão de hosts da rede em `lib/jail/allow_hosts.txt`
  (`jail_nick_hosts`). Mesmo formato do `lib/claude/`: pasta própria com o código e o dado,
  testes em `lib/tests/`. Ver seção "Sandbox do agente com ai-jail (`JAIL_ENABLED`)";
- `lib/claude/claude.sh` + `lib/claude/permissions.json`: as regras de deny/ask do Claude
  Code que toda sessão aberta pelo nick recebe. Ver seção "Regras de deny/ask do Claude Code
  (`lib/claude/`)". A divisão:

  | Fica em `lib/claude/` | Fica na ferramenta (`auto_scrum.sh`) |
  |---|---|
  | as regras (`permissions.json`, fonte única) | a global `NICK_CLAUDE_SETTINGS=""` que recebe o conteúdo |
  | achar o arquivo pelo `BASH_SOURCE` (`NICK_CLAUDE_PERMISSIONS_FILE`) | quando carregar (`main()`, logo depois de `jail_check`, com `\|\| exit 1`) |
  | ler o arquivo e entregar o conteúdo (`claude_settings_load`) | montar o argv: `claude_args=(--settings "$NICK_CLAUDE_SETTINGS")` |
  | a mensagem de erro (`claude_print_settings_error`) | o resto do argv (`--permission-mode`, `--append-system-prompt`, prompt) |

**As bibliotecas não dependem umas das outras:** cada uma pode ser carregada sozinha, e a suíte
de uma não carrega a outra (`lib/tests/test_jail.sh` nunca carrega `lib/config.sh`, e um teste
estrutural proíbe `config_`/`CONFIG_` fora de comentário em `lib/jail/jail.sh`;
`lib/tests/test_claude.sh` só carrega `lib/claude/claude.sh`, que não cita `config_`, `CONFIG_`,
`jail_` nem `JAIL_` em código). A jail recebe os
valores de `JAIL_ENABLED`, `JAIL_CLIPBOARD` e `JAIL_FORWARD_PORTS` (e os hosts extras) por
parâmetro em vez de ler o `.nick/nick.conf`, e a única coisa em comum, a troca de caracteres de
controle por `?`, é uma expansão de uma linha, não uma função que valha a pena compartilhar.
Assim uma ferramenta pode usar só uma das bibliotecas, e os testes de uma não quebram por
mudança na outra.

Os `assert_*` dos testes ficam em `lib/tests/assert.sh`, carregado por todas as suítes;
nenhuma suíte define os seus.

**Regras de toda biblioteca em `lib/`** (cobertas por testes estruturais em
`lib/tests/test_config.sh`, `lib/tests/test_jail.sh` e `lib/tests/test_claude.sh`):
- **só define ao ser carregada:** atribui as constantes, cria as globais vazias e define
  funções; não imprime nada e não roda comando. Sem shebang e sem bit de execução; a 1ª linha
  é `# shellcheck shell=bash`, seguida do cabeçalho em inglês. O teste de "carga limpa" de
  cada suíte compara o `compgen -v` antes e depois do `source` e ignora as variáveis que o
  próprio bash cria ou mexe entre os retratos (`clean_bash_vars`: `_`, `BASH_*`, `COLUMNS`,
  `LINES`, `FUNCNAME`, `LINENO`, `PIPESTATUS`). `COLUMNS` e `LINES` entraram no ticket 25: num
  terminal com tamanho, o bash as preenche (`checkwinsize`, ligado por padrão no bash 5)
  depois de cada comando externo, como o `sort` do 1º retrato, e as três suítes de `lib/tests/`
  falhavam quando rodadas com a saída num terminal (com a saída num arquivo, passavam). Um caso
  em cada suíte repete o retrato num pseudo-terminal 80x24 (`script`, pulado se não houver),
  pra o `make test` sem terminal também pegar uma volta do problema;
- não ativa `set -euo pipefail` (é de quem a carrega), mas funciona com ou sem ele no chamador;
- **prefixo por biblioteca:** toda função `config_` e toda global `CONFIG_` em
  `lib/config.sh`; toda função `jail_` e toda global `JAIL_` em `lib/jail/jail.sh`; toda função
  `claude_` e toda global `NICK_CLAUDE_` em `lib/claude/claude.sh` (as locais, `__claude_`).
  As globais de `lib/claude/` **não** usam `CLAUDE_`: o Claude Code lê variáveis `CLAUDE_*` do
  ambiente (ex: `CLAUDE_CODE_DISABLE_MOUSE`), e um nome nosso nesse espaço poderia colidir com
  uma variável dele, hoje ou numa versão futura, e mudar o comportamento do agente. Pelo mesmo
  motivo, a global do `auto_scrum.sh` é `NICK_CLAUDE_SETTINGS`. Um teste estrutural proíbe
  `CLAUDE_` sem `NICK_` na frente em código de `lib/claude/claude.sh`, e outro proíbe atribuir
  global `CLAUDE_*` no `auto_scrum.sh`. O `auto_scrum.sh` não define nada nesses namespaces;
- **nunca chama `exit`** e não usa `source`, `.` nem `eval`;
- **não lê globais do chamador:** tudo entra por parâmetro ou pelo próprio registro (o `$PWD`
  de `config_cwd_file` é estado do shell, não global do chamador);
- mesmas regras de estilo de "Qualidade de código bash".

**Exceção de globais, documentada:** `config_load` escreve, com `printf -v`, as globais das
chaves que o chamador registrou — esse é o papel do leitor — e nada mais: nem outra global do
chamador, nem `CONFIG_*` (um teste compara o `declare -p` antes e depois). O registro só
escreve nos próprios arrays `CONFIG_*` e não inicializa as globais das chaves; as linhas
`STACK_DESCRIPTION=""` etc. continuam no topo do `auto_scrum.sh`, que é o dono delas. Por isso
`load_project_config` continua existindo no `auto_scrum` como chamada fina: é ali que a
ferramenta guarda o que precisar sobre o arquivo carregado, sem pôr global de chamador dentro
da biblioteca (hoje, `PROJECT_CONFIG_FILE`, o caminho usado na mensagem de erro da jail).

**Exceção de `exit`, documentada:** `jail_exec` termina com `exec` (dentro ou fora da jail) e
por isso não retorna. Não é um `exit`: é o papel dela, o último passo da ferramenta.

**Constantes atribuídas ao carregar:** `JAIL_BIN`, `JAIL_VERSION_MIN` e `JAIL_OS_RELEASE_FILE`
(valores fixos) e
`NICK_CLAUDE_PERMISSIONS_FILE`, calculada a partir de `${BASH_SOURCE[0]}`
(`"$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" > /dev/null && pwd)/permissions.json"`). O `cd`
roda no subshell do `$(...)`: não muda o cwd de quem carrega, e a saída dele (que existe com
`CDPATH` definido) vai pro `/dev/null`, então a regra "só define ao ser carregada" continua
valendo. O caminho sai absoluto mesmo com a biblioteca carregada por caminho relativo de outro
cwd (coberto por teste). É variável, e não texto fixo, só pra os testes apontarem pra um
arquivo ausente, ilegível ou vazio — mesmo racional de `JAIL_BIN`.

**Carga:** logo depois de `SCRIPT_DIR`, com

```bash
# shellcheck source=SCRIPTDIR/../lib/config.sh
source "$SCRIPT_DIR/../lib/config.sh"
# shellcheck source=SCRIPTDIR/../lib/jail/jail.sh
source "$SCRIPT_DIR/../lib/jail/jail.sh"
# shellcheck source=SCRIPTDIR/../lib/claude/claude.sh
source "$SCRIPT_DIR/../lib/claude/claude.sh"
```

A diretiva usa `SCRIPTDIR/` e não `../lib/config.sh`: o caminho relativo puro só resolve se o
`shellcheck` rodar de dentro de `auto_scrum/` (da raiz dá SC1091), e com `SCRIPTDIR` ele
resolve de qualquer cwd. Rode `shellcheck -x`, pra ele seguir o `source`. O `SCRIPT_DIR` vem de
`BASH_SOURCE[0]`, então o caminho também funciona quando os testes dão `source` no script de
outro cwd. As três linhas `source` das bibliotecas, nessa ordem (config → jail → claude),
são as únicas linhas `source`/`.` do `auto_scrum.sh` (um teste compara a lista inteira); a
garantia de que a configuração nunca é executada fica no teste estrutural de `lib/config.sh`
e nos testes de gramática.

**Riscos aceitos:**
- a API de `lib/config.sh` nasce com um cliente só e pode mudar quando a segunda ferramenta
  existir;
- o `auto_scrum/` depende do `lib/` irmão por caminho relativo: mover o `auto_scrum/` sozinho
  quebra o script.

**Por que agora:** a decisão 1 do plano do ticket 13 (jail) era extrair o leitor "quando a
segunda ferramenta existir". O ticket 23 reviu isso: o 13 introduz `JAIL_ENABLED`, que vale
pro nick inteiro, e extrair antes dele custava só revisar aquele plano; depois, custaria
refazer código e testes do 13.

**Como rodar as suítes:** `make test` na raiz (ou `bash tests/run_all.sh`, sem `make`) roda
todas, uma de cada vez, e sai com código 0 só se todas passarem (ver seção "Testes: `make
test` e `tests/run_all.sh`"). Cada suíte continua podendo rodar direto, da raiz, e sai com
código 1 se houver `FAIL`: `bash auto_scrum/tests/test_auto_scrum.sh`,
`bash lib/tests/test_config.sh`, `bash lib/tests/test_jail.sh`, `bash lib/tests/test_claude.sh`
(esta precisa de `jq` ou `python3` pra validar o JSON; sem nenhum dos dois, dá FAIL) e
`bash tests/test_run_all.sh`.

## Registro de chaves da configuração

As chaves da configuração do `auto_scrum` ficam num bloco único no topo do `auto_scrum.sh`,
logo depois do `source` da biblioteca, uma chamada por chave, na ordem do esqueleto:

```bash
# config_register_key <name> <default> <note> <skeleton comment>
config_register_key PRODUCTION_ENABLED "false" \
'a pergunta de impacto em produção está desligada no prompt do PO. ...' \
'PRODUCTION_ENABLED: "true" se esse projeto já está em produção — ...'
```

- o comentário vai **sem** o `# ` de cada linha; quem acrescenta o prefixo (e escreve `#`
  numa linha vazia) é `config_print_skeleton`, porque é regra de formato do esqueleto;
- a ordem do registro é a do esqueleto, do aviso de chave ausente e de `CONFIG_KEYS`;
- as chamadas rodam ao carregar o script (inclusive no `source` dos testes), sob o
  `set -euo pipefail` dele: um registro inválido derruba o script na carga, de propósito, por
  ser erro de programação.

`config_register_key` valida antes de registrar e, se uma regra falhar, imprime uma mensagem
no stderr e retorna 1, sem registrar e sem `exit`: nome em `^[A-Z][A-Z0-9_]*$` (garante a
regra das locais em minúsculo e fecha o `printf -v` pra nomes como `a[$(cmd)]`), sem o
prefixo `CONFIG_`, ainda não registrado; padrão `"true"`/`"false"` pra chave `_ENABLED` e `""`
pra chave de texto; comentário não vazio e sem linha terminando em espaço, tab ou `\r` (que
quebraria os hooks de texto do time alvo — ver "Nome `nick.conf` dentro de `.nick/`").

O nome também não pode ser uma variável especial do shell (`BASH*`, `PATH`, `IFS`, `ENV`,
`HOME`, `CDPATH`, `PS[0-9]`, `SHELLOPTS`, `LD_*`, `LC_*`, `LANG`, `TERM`, `EDITOR`, `SHELL`,
`GLOBIGNORE`, `PROMPT_COMMAND` e outras da lista em `config_register_key`) nem uma variável que
já seja readonly no momento do registro (ex: `UID`). **Por quê:** o `.nick/nick.conf` pode ser
editado por outro time, e o leitor atribui o valor do arquivo à global de mesmo nome. Uma
chave chamada `PATH`, `BASH_ENV` ou `LD_PRELOAD` deixaria esse arquivo mudar como o shell de
quem roda a ferramenta executa comandos, e um `printf -v` numa readonly derrubaria o chamador
sob `set -e` (o leitor nunca deve sair). Hoje nenhuma chave registrada é assim; a regra existe
porque a biblioteca nasce pra ser reaproveitada por outras ferramentas. Consultar o atributo
readonly lê estado do shell, como o `$PWD` de `config_cwd_file`, e não uma global do chamador.

Testes de integração no `test_auto_scrum.sh` cobram o formato: o bloco é contíguo, no topo,
fora de função e com uma chamada por chave; `print_project_skeleton` não tem linha `CHAVE=`;
nenhuma chave registrada aparece em `lib/config.sh` nem em `lib/tests/test_config.sh` (que só
usa chaves de mentira); e uma chave extra registrada no teste aparece no esqueleto e nos
avisos de chave ausente e de valor inválido.

**Por quê:** cada chave fica numa chamada só, com as quatro informações (nome, padrão, nota e
comentário) lado a lado, e acrescentar uma chave é acrescentar uma chamada. Some o trio que
antes precisava ficar sincronizado à mão (a lista `PROJECT_CONFIG_KEYS`, o `case` de padrão e
nota por chave e o heredoc do esqueleto), e a regra "flag com `"false"`/`"true"`, texto com
`""`", que antes só um teste cobrava, passa a valer também no registro. O armazenamento é em
arrays indexados paralelos (`CONFIG_KEYS`, a lista pública, só pra leitura, e
`CONFIG_KEY_DEFAULTS`/`CONFIG_KEY_NOTES`/`CONFIG_KEY_COMMENTS`, internos), não em array
associativo: um `declare -A` exigiria `-g` se a biblioteca fosse carregada dentro de uma
função.

## Modo auto só na etapa de Desenvolvimento

`auto_scrum.sh` sobe o `claude` com `--permission-mode auto` (aprova a maioria das
chamadas de ferramenta sozinho, exceto o que estiver em deny/ask do `settings.json`)
somente quando `$TYPE = development`. PO, Tech Leader e Review sobem no modo padrão
(interativo, pedindo confirmação).

**Por quê:** Desenvolvimento é a única etapa em que o agente de fato edita código e roda
comandos — as outras três são conversas de avaliação/planejamento/revisão em texto, sem
motivo pra soltar a supervisão. Regras de `deny`/`ask` continuam valendo em qualquer modo
(deny > ask > allow > modo da sessão), então `auto` não contorna o que estiver bloqueado ou
marcado pra confirmação. Toda sessão aberta pelo nick, em todas as etapas, recebe as regras
de `lib/claude/permissions.json` (`--settings`): `sudo`, `rm -rf`, `git push --force` e a
leitura de `~/.ssh`/`~/.aws`/`~/.gnupg` são negados, e `git add`/`commit`/`push`, o que perde
trabalho local e a leitura de `.env` pedem confirmação (ver seção "Regras de deny/ask do
Claude Code (`lib/claude/`)"). As regras casam pelo texto do comando, então são uma grade de
proteção contra erro, não um isolamento. O isolamento, opcional, é a jail: com
`JAIL_ENABLED="true"`, o agente roda dentro do sandbox ai-jail, que limita a escrita ao
projeto e esconde `~/.ssh`, `~/.gnupg` etc. (ver seção "Sandbox do agente com ai-jail
(`JAIL_ENABLED`)").

## Sandbox do agente com ai-jail (`JAIL_ENABLED`)

Com `--permission-mode auto` (etapa de Desenvolvimento), o agente aprova sozinho a maioria
das ações e, sem jail, enxerga a máquina inteira. O prompt carrega conteúdo do projeto alvo
(`.nick/nick.conf`, `CLAUDE.md`, tickets), então uma prompt injection ou um erro do agente poderia
ler segredos ou alterar arquivos fora do repositório. `JAIL_ENABLED="true"` faz o `claude` de
todas as etapas rodar dentro do [ai-jail](https://github.com/akitaonrails/ai-jail).

**Como funciona** (`lib/jail/jail.sh`; no `auto_scrum`, chamado em `main()`):
- `jail_check "$JAIL_ENABLED" "$JAIL_CLIPBOARD" "$jail_hosts" "$JAIL_FORWARD_PORTS"
  "$PROJECT_CONFIG_FILE" <compartilhado>` (`jail_hosts` vem de `mcp_jail_hosts`, ver "Hosts dos
  MCPs remotos" abaixo) roda logo depois de `resolve_stack`, antes de qualquer `build_*` e do
  menu principal, com `|| exit 1`. Desligada, não faz nada (nem `command -v`, nem `--version`).
  Ligada, depois de validar os valores e do `command -v`, confere a versão (`ai-jail
  --version`; fora de `>= 3.2.0` e `< 4.0.0`, ou ilegível, para com a versão encontrada, a
  suportada e "atualize o ai-jail" como saída 1, sem rodar nada em sandbox — ticket 27) e roda
  `ai-jail <opções> -- true` (as mesmas opções do lançamento) com o cwd num diretório
  temporário, removido em seguida. Se falhar (ai-jail ausente, `mktemp` falhando, bwrap
  ausente, user namespaces bloqueados, sistema sem suporte), imprime no stderr o caminho da
  configuração, a saída do ai-jail (controles trocados por `?`, linha a linha), as duas saídas
  (instalar ou `JAIL_ENABLED="false"`; quando a causa vem da saída do ai-jail, a primeira é
  "corrija o problema acima", com remissão a "Problemas comuns" do README, porque a lista pode
  vir toda `[ok]` — user namespaces bloqueados, bwrap reprovado na validação de binário
  confiável; achado do review, em que a mensagem mandava instalar com tudo `[ok]`), o aviso de
  arquivo compartilhado quando a configuração é o `.nick/nick.conf` do cwd (`config_cwd_file`) e a
  lista de `jail_print_requirements`, e retorna 1. O `|| exit 1` vem antes de `mkdir -p
  "$LOGS_DIR"`: nenhum log é gravado.
- `jail_print_status "$JAIL_ENABLED" "$JAIL_CLIPBOARD"` imprime uma linha: `Jail (ai-jail):
  desligada — ...` ou `Jail (ai-jail): ativa | Landlock: <estado> | modo de clipboard <nível>`,
  sempre com `⚠️ ` na frente quando a jail está ativa. Roda duas vezes: logo depois de
  `jail_check`, antes do menu principal e de qualquer pergunta (pedido do usuário: o aviso tem
  de aparecer no começo de tudo), e de novo logo antes de "Confirma o envio para o claude?",
  perto do lançamento.
- `jail_exec "$JAIL_ENABLED" "$JAIL_CLIPBOARD" "$jail_hosts" "$JAIL_FORWARD_PORTS" claude
  <args>` substitui o `exec claude`: com `"true"`, `exec ai-jail --no-save-config
  --no-status-bar --clean --agent-state --toolchains --no-github --no-ssh --no-aws --no-kube
  --no-gcloud --no-docker-config --no-docker --no-inherit-env [opções do nível] [--map
  <toolchain>...] [--allow-host <host>...]
  [--forward-port <porta>...] -- claude <args>`; senão, `exec claude <args>`, com o argv
  idêntico ao de antes (coberto por teste de caracterização, que passou com o código antigo).
- `--requisitos-jail` chama `jail_print_requirements` e sai com 0, antes de
  `check_requirements` e de `resolve_stack` (não carrega configuração nem roda o ai-jail em
  sandbox; só o `ai-jail --version`, desde o ticket 27).

**Decisões, cada uma com o motivo:**
- **Integrar o ai-jail em vez de reimplementar.** Isolamento de sistema operacional é difícil
  de acertar, e a ferramenta já cobre Linux (bubblewrap + Landlock) e macOS (`sandbox-exec`).
- **A jail é do nick inteiro.** `JAIL_ENABLED` é convenção de todas as ferramentas com IA:
  cada uma registra a chave com `config_register_key` e passa o valor pra
  `lib/jail/jail.sh`, que aplica a mesma regra (só `"true"` exato liga) e cita `JAIL_ENABLED`
  nas mensagens. O mecanismo mora em `lib/`, não no `auto_scrum`, porque toda ferramenta
  futura vai precisar dele.
- **Opt-in, como as outras flags** (padrão `"false"`, sem nota): exige instalar dependência
  por máquina, e ligar por padrão quebraria toda configuração existente. Valor inválido
  desliga com o aviso de valor ("a opção está desligada.", que descreve o efeito certo).
- **Verificação na inicialização, sem fallback.** Quem pede jail nunca roda sem: não existe
  pergunta "seguir sem jail?" nem variável de ambiente que pule a verificação. Parar logo
  depois de carregar a configuração evita responder o roteiro à toa. O menu de projetos vem
  antes por necessidade (é ele que escolhe a configuração).
- **Verificação com `true`, num diretório temporário.** Testa só a jail, serve pra qualquer
  ferramenta e não toca no estado do `claude` (`~/.claude` é montado com escrita). O diretório
  temporário é uma segunda proteção contra o auto-save (ver `--no-save-config` abaixo): a
  verificação nunca deve gravar nada no projeto alvo. Se o `claude` não for encontrado dentro
  da jail, o erro aparece no lançamento real (README, "Problemas comuns").
- **Sem `--bootstrap` e sem `--init` do ai-jail** (o `--bootstrap` sobrescreveria o
  `~/.claude/settings.json` do usuário; o `--init` grava configuração), e sem repassar
  `--rw-map`, `--overlay-map`, `--network`, `--no-network`, `--no-private-home`, `--exec`, as
  opções que abrem credencial ou autoridade do host (`--github`, `--ssh`, `--aws`, `--kube`,
  `--gcloud`, `--docker-config`, `--docker`, `--inherit-env`), `--no-agent-state` ou qualquer
  outra opção fora de `jail_options` (teste estrutural: toda chamada de `$JAIL_BIN` é
  `"$JAIL_BIN" "${options[@]}" -- ...`, fora a única `"$JAIL_BIN" --version 2>/dev/null` de
  `jail_version_output`, que não abre sandbox; e `options` só vem de `jail_options`, a fonte
  única das opções: `--no-save-config`, `--no-status-bar` e as do ticket 27 (`--clean
  --agent-state --toolchains` e os oito `--no-*`) sempre, cada uma só no corpo de
  `jail_options`, mais as do nível de `JAIL_CLIPBOARD`,
  um `--map` por toolchain de `jail_toolchain_dirs`, o `--map` e o `--env` dos navegadores do
  Playwright, um `--allow-host` por host e um `--forward-port` por porta que o chamador passar
  — os únicos lugares onde `--map`, `--env`, `--allow-host` e `--forward-port` aparecem,
  também cobertos por teste estrutural).
  Todas vão na linha de comando, pra nenhuma depender do `~/.ai-jail` de cada pessoa.
- **`--no-save-config` na verificação e no lançamento** — uma das exceções à regra acima (as
  outras: `--no-status-bar`, as do ticket 27 — `--clean`, `--agent-state`, `--toolchains`,
  `--no-github`, `--no-ssh`, `--no-aws`, `--no-kube`, `--no-gcloud`, `--no-docker-config`,
  `--no-docker`, `--no-inherit-env`, ver abaixo —, as opções de `JAIL_CLIPBOARD`, o `--map` das
  toolchains, o `--allow-host` dos MCPs e o `--forward-port` de `JAIL_FORWARD_PORTS`), decidida no passo 8 do
  ticket 13, depois da conferência com o ai-jail real (o plano dizia "nenhuma opção"). Sem ela,
  num projeto ainda sem `.ai-jail`, o ai-jail grava no cwd (a raiz do projeto alvo) um
  `.ai-jail` com o argv **inteiro** — o prompt do `auto_scrum` incluído, com o conteúdo de
  tickets do Jira e erros do Sentry —, que pode ir pro git; e um `ai-jail` sem comando, rodado
  ali depois, relança o `command` salvo (`claude --permission-mode auto` com o prompt velho). A
  opção só impede a gravação: o `.ai-jail` do projeto continua lido (conferido com `mask`, no
  2.14.0; no 3.x, com o `--clean` do ticket 27, o perfil do projeto, salvo ou legado, não vale
  pelo nick). Não
  perde nada pro nick, que não passa outras opções pra serem lembradas; quem quiser um
  `.ai-jail` de modelo gera com `ai-jail --init`, de propósito. A alternativa (`no_save_config
  = true` no `~/.ai-jail` global) deixaria a proteção a cargo de cada pessoa do time. Ela vai
  também na verificação pra um ai-jail que não aceite a opção falhar ali, com a mensagem de
  erro, e não no lançamento.
- **`--no-status-bar` na verificação e no lançamento** — a segunda exceção, também do passo 8.
  Na primeira execução real (`po_discussion`, terminal do VS Code), a barra de status do
  ai-jail, que mostra `<cwd> | <comando>` (o prompt inteiro de novo) e desenha nas últimas
  linhas da tela, brigou com a interface do `claude`: a tela encheu de cópias da linha do
  comando. Com `ai-jail --no-save-config --no-status-bar -- claude "diga oi"` a tela ficou
  normal (conferido pelo usuário). O próprio README do ai-jail indica `--no-status-bar` pra
  sobreposição na parte de baixo da tela. Não usamos `--exec` (que tira também o proxy de
  terminal do ai-jail): a documentação não diz que proteção o proxy dá, então a mudança
  mínima é só tirar a barra. Perde-se só o indicador visual "Jail Active"; a linha
  `Jail (ai-jail): ativa` do nick, antes da confirmação, continua dizendo que a jail está
  ligada. Vai também na verificação pelo mesmo motivo da opção acima.
- **O nick não instala, não baixa nem embute o binário** (mesma regra de `claude`/`envsubst`).
  `JAIL_BIN="ai-jail"` é o nome no `PATH`, sem caminho absoluto; ele é variável só pra os
  testes apontarem pra um nome inexistente no caso "ai-jail ausente", sem depender da máquina.
  `JAIL_OS_RELEASE_FILE` existe pelo mesmo motivo.
- **`jail_print_requirements` é a fonte única de "o que instalar e como"**: a mensagem de
  erro e o `--requisitos-jail` usam a mesma função. Ela detecta a plataforma (`uname -s` e
  `ID`/`ID_LIKE` do `/etc/os-release`, lido linha a linha, sem `source` e sem `config_load`,
  cujo formato é outro), mostra `[ok]`/`[falta]` por `command -v` e o comando de cada item que
  falta. Só mostra os comandos, nunca os roda (teste estrutural: nenhuma linha de código de
  `lib/jail/jail.sh` cita um instalador fora de string). O README repete os comandos, e um teste
  confere que cada comando do helper aparece literal no README — README genérico não tem
  teste próprio, e esta lista muda quando o upstream muda. Pelo mesmo motivo, outro teste
  confere que os três níveis do `JAIL_CLIPBOARD` e cada opção de `jail_options` aparecem
  literal no README (que repete as flags de cada nível na tabela), e que existem as seções
  "Sandbox (ai-jail)" e "Problemas comuns", citadas na mensagem de erro.
- **`PROJECT_CONFIG_FILE` fica no `auto_scrum`** (atribuída em `load_project_config`, pelos
  três caminhos; vazia com "Nenhum"), porque `lib/config.sh` não guarda o caminho carregado.
  Quem decide se o arquivo é compartilhado com o time também é o `auto_scrum`
  (`config_cwd_file`), não a biblioteca da jail.
- **Personalização só pela configuração global do ai-jail** (`~/.config/ai-jail/config.toml`,
  de cada pessoa, que o nick não lê nem escreve), no que o nick não define na linha de
  comando (ex: `ro_maps`, `mask`; chaves conferidas no README do ai-jail 3.2.0 instalado). Até
  o ticket 27 era o `.ai-jail` do projeto, com o `trust_project_config` do `~/.ai-jail` global
  pra ampliar acesso; no 3.x esses dois são formatos legados (o ai-jail os migra e apaga numa
  execução com escrita, sem o nick), e com o `--clean` o perfil do projeto, salvo ou legado,
  não vale pelo nick (ajuda do 3.2.0: "Ignore saved and legacy project policy"). O README não
  ensina mais esse caminho (teste: não cita `trust_project_config = `).
- **`--` antes do comando** (`ai-jail <opções> -- claude ...`): é o fim de opções documentado
  pelo ai-jail (`ai-jail [OPTIONS] [--] [COMMAND [ARGS...]]`), e garante que nenhum argumento
  do `claude` (o prompt, `--permission-mode`) seja lido como opção do ai-jail. O comando
  continua sendo `claude`, a primeira palavra depois do `--`, que o ai-jail usa pra achar o
  `agent_state`.
- **Conferência manual com o ai-jail real** (ai-jail 2.14.0, Arch, 2026-11-01, em diretórios
  temporários fora do repositório; histórico: o 2.x deixou de ser suportado no ticket 27, ver
  a conferência com o 3.2.0 logo abaixo):
  - `ai-jail -- true` sai com 0 (Landlock ativo);
  - o auto-save existe (`--save-config` é o padrão): sem `.ai-jail`, grava um com
    `command = [<argv inteiro>]`; com `.ai-jail` existente, só o reformata e mantém o
    `command` dele; `--dry-run` não grava nada; com `--no-save-config`, nada é gravado;
  - o comando da linha de comando vence o `command` de um `.ai-jail` existente; `ai-jail`
    sem comando roda o `command` salvo;
  - o argv depois do `--` chega intacto (espaço, `;`, `$HOME` e aspas no mesmo argumento);
  - a rede padrão é filtrada (`--allow-host` de `api.anthropic.com`, registries de pacotes e
    GitHub, conforme o comando: com `curl`, o `api.anthropic.com` fica de fora); um
    `--allow-host` na linha de comando soma à lista padrão, que segue valendo (`--dry-run`);
  - primeira execução real do `auto_scrum` (`po_discussion`, `JAIL_ENABLED="true"`): o
    `claude` subiu e rodou comandos dentro da jail, mas a barra de status bagunçou o terminal
    (daí o `--no-status-bar`, acima);
  - roteiro manual com a versão final (`po_discussion`, terminal do VS Code), feito pelo
    usuário: `ls ~/.ssh` não teve acesso; `touch ~/Projetos/fora-do-projeto` "funcionou" só no
    home privado da jail (tmpfs) — o arquivo não existe na máquina; `touch` na raiz do
    projeto funcionou; `curl` pra fora foi bloqueado; nenhum `.ai-jail` ficou no projeto;
  - copiar, colar e rolar dentro da jail (testes do usuário, com o `claude` fora da jail
    copiando e colando normalmente):
    - modo do nick: rola; não copia (o filtro de terminal do ai-jail barra o OSC 52 do
      `claude`, e a seleção do terminal não pega porque o `claude` controla o mouse); o
      Ctrl+V do `claude` não cola, mas botão direito → Colar cola; o Shift+Insert colou num
      teste e não em outro, provavelmente porque no terminal do VS Code no Linux ele cola a
      seleção primária (o último texto selecionado com o mouse), e não o clipboard — por
      isso as mensagens e o README só indicam o botão direito;
    - `--terminal-passthrough`: copia; o Ctrl+V continua sem colar;
    - `--exec`: o ai-jail exige `--terminal-passthrough` junto e o resultado é o mesmo;
    - `CLAUDE_CODE_DISABLE_MOUSE=1` (variável achada no binário do `claude`, não
      documentada): a seleção do terminal copia, mas a rolagem some;
      `CLAUDE_CODE_DISABLE_MOUSE_CLICKS=1`: nada seleciona;
    - `--terminal-passthrough --display`: copia, e o Ctrl+V cola normal — o `claude` lê o
      clipboard pelo socket do Wayland/X11, que a jail esconde sem `--display`, e não pelo
      terminal; colar imagem o usuário não faz nem fora da jail (passa o caminho).
  - MCP (Jira, Sentry): o usuário não conseguiu ler o Jira nem o Sentry pelo MCP dentro da
    jail. Os dois são remotos (HTTP com OAuth, `.mcp.json` dos projetos alvo):
    `mcp.atlassian.com` e `mcp.sentry.dev`. Com `curl` dentro da jail, os dois deram
    `CONNECT tunnel failed, response 403` (o proxy da jail recusou); com `--allow-host` de
    cada um, 401 (passou; falta só o token, que o `claude` manda). Daí "Hosts dos MCPs
    remotos", abaixo. Com essa mudança, o usuário confirmou pelo `auto_scrum` que o MCP do
    Jira e o do Sentry funcionam dentro da jail (autenticados antes, fora dela);
  - rodar os testes do projeto (`make verify` no `siga-construcao`, pelo agente, dentro da
    jail): o front passou, mas o pytest não rodou — o `venv/bin/python` aponta pro
    `~/.pyenv/versions/3.12.12`, que o home privado da jail esconde. O agente, sem saber que
    estava na jail, concluiu que o `~/.pyenv` "não existe mais" (existia), disse ter
    conferido "fora do sandbox" (não consegue) e começou a propor um ticket de deploy. Daí
    "Toolchains do home" e "Aviso ao agente", abaixo. `--map ~/.pyenv` conferido à mão: dentro
    da jail o `python -V` deu 3.12.12 e o `touch` deu "somente leitura"; um `--map` de
    caminho inexistente só avisa e segue (o nick nem o passa). Um serviço da máquina em
    `localhost` (porta 5050, 302 fora) não responde de dentro da jail (`curl` rc 7);
  - de novo, com o `~/.pyenv` montado e o aviso ao agente: o front passou (2099 testes do
    Vitest), e o pytest falhou na conexão com o PostgreSQL do Docker (`localhost:54320`,
    "Connection refused"). Desta vez o agente reconheceu a limitação da jail, interrompeu a
    suíte em vez de esperar 3284 erros e não diagnosticou a máquina — o aviso funcionou. Com
    `--forward-port 5050 --forward-port 54320`, os dois responderam de dentro da jail, e uma
    porta não liberada (5051) foi recusada. Daí `JAIL_FORWARD_PORTS`, abaixo;
  - de novo, com `JAIL_FORWARD_PORTS="54320"`: o banco conectou, o pytest rodou sem falha
    (interrompido em 23%), o front passou; o E2E não rodou porque o Chromium do Playwright
    fica em `~/.cache/ms-playwright`, que a jail não monta. Conferido à mão: com `--map` nesse
    caminho, o Chromium headless roda (com `--no-sandbox`; sem ele, "No usable sandbox"), mas
    o ai-jail avisa "ignoring rw-map that overlaps a read-only map destination" e deixa de
    montar o `~/.cache` gravável dele (`--dry-run`). Montando em `~/.ms-playwright` com
    `--env PLAYWRIGHT_BROWSERS_PATH`, o Chromium roda e o `~/.cache` continua gravável. Daí
    "Navegadores do Playwright", abaixo;
  - de novo, com os navegadores montados: rodou e passou tudo o que o agente rodou dentro da
    jail — front (lint, tsc, 2099 testes do Vitest), testes com banco, de concorrência
    (`transaction=True`), de integração (schema OpenAPI, PDF com WeasyPrint), E2E com
    Playwright + `live_server` e os do Storybook — sem falha e sem teste pulado. Com isso, o
    critério "consegue rodar os testes dele" está cumprido;
  - as suítes do próprio nick rodadas pelo agente dentro da jail: 13 falhas no
    `test_auto_scrum.sh`, todas do aviso esperado com a linha do Playwright. Era isolamento
    dos testes: dentro da jail, `PLAYWRIGHT_BROWSERS_PATH` vem definida (pelo próprio nick), e
    o helper `jail_expected_argv` herdava a variável, enquanto o `main()` do caso rodava sem
    ela. O helper passou a rodar com `env -u PLAYWRIGHT_BROWSERS_PATH`, e as três suítes
    passaram dentro do ai-jail real, com as opções que o nick gera;
  - `ro_maps` com `trust_project_config` (o caminho alternativo do README), testado sem tocar
    no `~/.ai-jail` do usuário: `HOME` falso com o próprio `.ai-jail` global, dois projetos de
    teste e as opções de `jail_options`. Projeto A confiado, com `ro_maps =
    ["~/jail-teste-ro"]`: o arquivo aparece, e a escrita é recusada; projeto B, sem `.ai-jail`:
    não aparece; projeto A sem `trust_project_config`: não aparece, e o ai-jail avisa "project
    .ai-jail map ... outside project ignored"; `trust_project_config` com `~/` também vale. O
    `.ai-jail` do projeto continuou intacto (o `--no-save-config` não o regrava). O usuário
    decidiu manter esse caminho no README como alternativo, pra necessidades que o nick não
    cobre sozinho.
  - etapa de Desenvolvimento (a única em `--permission-mode auto`) com a versão final, pelo
    usuário: o cabeçalho mostrou `Landlock: completo` no começo e antes da confirmação, o
    `claude` abriu com `--permission-mode auto` + `--append-system-prompt` + prompt sem erro,
    e, perguntado sobre o ambiente, o agente descreveu certo o que o aviso diz (projeto
    gravável, home privado com toolchains e Playwright, rede restrita, `localhost`
    inalcançável). Ele também atribuiu à jail a falha do conector "Claude Docs" da conta
    claude.ai (erro "Proxy"): conector da conta, não MCP do projeto, nenhuma etapa do nick o
    usa — fica como limitação conhecida;
  - `--requisitos-jail` "antes" (simulado: `PATH` igual ao real, só sem o `ai-jail`, porque
    ele já estava instalado): `[falta] ai-jail  →  yay -S ai-jail-bin`, `[ok] bwrap`, rc 0;
    "depois" (`PATH` real): `[ok]` nos dois, rc 0.
  - rede para documentação (`lib/jail/allow_hosts.txt` e `JAIL_ALLOW_HOSTS`), 2026-11-02:
    `curl` dentro do ai-jail real, com as opções de `jail_options`, num diretório temporário
    fora do repositório. (a) `docs.claude.com` e `code.claude.com` responderam (301/302),
    onde antes o WebFetch tinha dado "proxy refused the connection"; (b) `docs.celeryq.dev`,
    fora da lista, foi recusado (o `curl` sai com 7, conexão recusada); (c) o mesmo host
    respondeu (302) passado como host extra, o caminho de `JAIL_ALLOW_HOSTS`. Os 36 hosts da
    lista daquele momento, testados um a um na jail, responderam, menos o apex
    `django-rest-framework.org`, que não responde nem fora da jail: o site fica em `www.`, que
    passa por ser subdomínio (o arquivo comenta isso). Numa etapa real, pelo usuário, o
    agente seguiu o aviso diante do host recusado: o WebFetch em `docs.celeryq.dev` deu
    "proxy refused the connection", e ele disse que a jail do nick recusou, que o host não
    está em `lib/jail/allow_hosts.txt` nem no projeto, que não ia contornar e como liberar
    (`JAIL_ALLOW_HOSTS="docs.celeryq.dev"` no `.nick/nick.conf`);
- **Conferência manual com o ai-jail real (ai-jail 3.2.0, Arch, 2026-11-08, ticket 27)**, em
  diretórios temporários fora do repositório, com um terminal simulado pelo `script` do
  util-linux e um `claude` de sonda (script chamado `claude`, fora do `PATH`, que imprime o que
  enxerga), `GH_TOKEN`/`GITHUB_TOKEN` falsos e uma variável qualquer no ambiente do host. A
  configuração do ai-jail ficou num `XDG_CONFIG_HOME` temporário, menos onde indicado. Roteiro,
  saídas e prints em `.nick/docs/27/` (`teste_manual.md`):
  - `--dry-run` com as opções de `jail_options`: todas aceitas; lista padrão de hosts do
    ai-jail (`api.anthropic.com`, os registries, `github.com`, `codeload.github.com`,
    `objects.githubusercontent.com`) mais a do nick; `~/.claude` e `~/.claude.json` montados;
    nenhum `~/.ssh` ou `~/.config/gh`. O `--toolchains` monta também `~/.rustup` e
    `~/.cargo/bin` (somente leitura) e caches do próprio ai-jail (`~/.cache`, `~/.npm`,
    `~/.cargo/registry`), graváveis: daí a menção aos caches no aviso ao agente;
  - verificação `-- true`: rc 0, `▸ Landlock: fully enforced` e a linha nova `⚠ Mask: ... rule
    not applied`, que `jail_landlock_state` ignora ("completo"). O binário traz linhas de
    detalhe novas (`Landlock: system paths ro`, `docker socket rw`, `disabled by config/flag`
    etc.) que não aparecem na saída normal; se aparecessem sem a linha de estado, dariam "não
    identificado" (o lado seguro) — fica pra um ticket se um dia aparecerem;
  - controle, com as opções de antes do ticket e um terminal: a tela "Configure claude for
    this project" abriu (o processo ficou parado nela);
  - (a) projeto sem perfil, opções novas, terminal: nenhuma tela; `~/.claude` presente; sem
    `GH_TOKEN`, `GITHUB_TOKEN`, `~/.config/gh`, `~/.ssh`, `SSH_AUTH_SOCK`, socket do Docker e
    sem a variável do host (`--no-inherit-env`); `api.anthropic.com` e `github.com` responderam,
    `example.com` foi recusado (rc 7). Igual com a configuração real do usuário. Observação: o
    ai-jail regrava o `~/.config/ai-jail/config.toml` real a cada lançamento com escrita (mtime
    novo, conteúdo igual), também pelo nick — comportamento dele, não gravação do nick;
  - (b) perfil salvo pela tela: `ai-jail configure claude` no modo sequencial do próprio ai-jail
    (`TERM=dumb`, "as mesmas decisões de segurança" da tela, segundo o README dele), preset
    "For developers" aceito, `OPEN` confirmado, salvo: `[grants] github = true`. Com as opções
    de antes do ticket, o `GH_TOKEN` entrou na jail; com as do nick, não. O perfil não foi
    alterado pelo lançamento;
  - (c) `config.toml` global com `github = true` / `ssh = true` (pelo usuário, fora da jail,
    com `.nick/docs/27/teste_9_2c.sh`): no controle (`--clean` sem os `--no-*`), o `github =
    true` pôs o `GH_TOKEN` na jail e o `ssh = true` deu acesso a um agente SSH com chave; com as
    opções do nick, token, `~/.config/gh`, `~/.ssh` e agente ficaram de fora nos dois casos — os
    `--no-*` explícitos fecham o que a configuração global abre, mesmo com `--clean`;
  - (d) `config.toml` global com `network = true`: a verificação do nick falha com "--network
    and --allow-host/--allow-private-host are mutually exclusive ... cannot be combined"
    (README, "Problemas comuns");
  - versão fora da faixa (um `ai-jail` falso que responde `ai-jail 2.14.0`): o `jail_check`
    para antes da verificação, com "encontrada 2.14.0; suportada: 3.2.0 ou mais nova da série
    3.x", "atualize o ai-jail" e `[atualizar] ai-jail 2.14.0  →  yay -S ai-jail-bin ...`; o
    `--requisitos-jail` mostra `[atualizar]` (com o falso) e `[ok]    ai-jail 3.2.0` (com o
    real);
  - git por HTTPS dentro da jail (`credential.helper=cache` no `.gitconfig` do usuário, cujo
    socket fica no `~/.cache` da máquina, que a jail troca pelo dela; sem `~/.git-credentials`):
    `ls-remote` e `fetch` de repositório público funcionam; de repositório inexistente (o
    GitHub responde como a um privado) e `push --dry-run` pedem credencial. Num terminal, o git
    **trava** em `Username for 'https://github.com':` (cortado por timeout de 20 s); sem
    terminal (`setsid`), falha na hora com "could not read Username" (rc 128). `status`,
    `commit` e `log` funcionam. Mudar `GIT_TERMINAL_PROMPT` ficou fora do escopo; o README e o
    aviso ao agente dizem isso;
  - num projeto real (`siga-construcao`, `JAIL_ENABLED="true"`), pelo `auto_scrum`, com o
    `claude` de verdade, seguindo o roteiro de `.nick/docs/27/teste_manual.md` (abrir direto e
    logado, sem tela; git local; token e SSH ausentes; fetch/push por HTTPS): o usuário
    confirmou que está tudo ok, sem anexar a saída de cada comando;
  - as quatro suítes do nick rodadas dentro do ai-jail real com as opções novas (cwd no
    repositório, sem nada gravado nele): mesmo resultado de fora da jail.
  Duas tentativas foram negadas pelo ambiente da sessão e não foram feitas: conferir na
  prática que o `mask` de um `.ai-jail` legado do projeto deixa de valer com `--clean` (a
  afirmação vem da ajuda do 3.2.0) e rodar as suítes numa cópia do repositório.
- **Terminal e clipboard da jail: `JAIL_CLIPBOARD`, em três níveis** (as opções além de
  `--no-save-config` e `--no-status-bar`). Sem passthrough, não dá pra copiar de dentro do
  `claude`, e o Ctrl+V só cola com o display aberto (testes acima; com `--display`, o usuário
  confirmou que o Ctrl+V colou). O caminho até aqui, decidido com o usuário:
  - a primeira versão deixava a liberação no `~/.ai-jail` global de cada pessoa; descartada
    porque um dev novo que não editasse o arquivo ficaria sem copiar e tenderia a desligar a
    jail inteira (`JAIL_ENABLED="false"`), o que é muito pior que o risco do passthrough;
  - depois, `JAIL_TERMINAL_PASSTHROUGH_ENABLED` (liga/desliga, ligada por padrão); a proposta
    de uma segunda flag pro display (`JAIL_DISPLAY_ENABLED`) foi trocada por uma chave só,
    porque os níveis são cumulativos (display sem passthrough colaria sem copiar, combinação
    sem uso) e uma chave com níveis é mais fácil de entender. Nenhuma das duas chegou à `main`.
  Os níveis (`jail_options`, fonte única das opções):
  - `"restrito"`: nada além da base. Não copia; cola-se pelo botão direito → Colar;
  - `"medio"` (e vazio, o padrão): `--terminal-passthrough`. Copia. O agente pode escrever no
    clipboard (trocar o que o usuário copiou, por exemplo por um comando) e mandar sequências
    de controle direto pro terminal;
  - `"brando"`: `--terminal-passthrough --display`. Copia, e o Ctrl+V do `claude` cola. O
    `--display` monta só o socket do Wayland (`--dry-run` no Arch, sessão Wayland com
    XWayland: `/run/user/1000/wayland-1`, `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`; nem
    `/tmp/.X11-unix` nem `DISPLAY`). O agente pode **ler** o clipboard (senhas, tokens),
    abrir janelas na tela e, em compositores com o protocolo de teclado virtual, digitar em
    outras janelas.
  Decisões da chave:
  - `jail_print_status` mostra, a **cada** execução (pedido do usuário), o nível numa linha
    curta — no começo, antes do menu principal, e de novo antes da confirmação. A primeira
    versão tinha um parágrafo de risco por nível; o usuário achou texto demais e pediu o
    formato `⚠️ Jail (ai-jail): ativa | modo de clipboard médio`, com o ⚠️ em todo nível
    (a primeira versão deixava o restrito sem; o usuário pediu o ⚠️ sempre que a jail estiver
    ativa) e o brando em vermelho. O risco de cada nível
    ficou no README e no comentário do esqueleto. O vermelho (`\e[31m`) só sai com stdout
    num terminal (`[ -t 1 ]`) e `NO_COLOR` vazia (convenção no-color.org), pra pipes e logs
    ficarem limpos; o teste simula o terminal com o `script` do util-linux e se declara
    PULADO sem ele;
  - chave de texto, e não `*_ENABLED`, porque tem três valores. Por isso o padrão registrado
    é `""` (regra da `config.sh` pra chave de texto), e vazio vale `"medio"`; e a `config.sh`
    não valida o valor (não pode ser alterada neste ticket);
  - quem valida é `jail_check`, só com `JAIL_ENABLED="true"` (desligada, a chave não tem
    efeito e um valor errado passa quieto): outro valor (inclusive `"MEDIO"` ou com espaço)
    para a ferramenta com `jail_print_clipboard_error` — o valor, o arquivo e os valores
    aceitos, sem a lista de requisitos, porque não é problema de instalação. Um erro de
    digitação nunca escolhe um nível sozinho, no mesmo espírito do "sem fallback" da jail;
  - o nome cita o clipboard, e não "modo da jail", porque arquivos, rede e `~/.ssh` são iguais
    nos três níveis; os valores são os níveis que o usuário propôs (escolhidos entre esses,
    valores descritivos como `"copiar"`, e `JAIL_MODE`);
  - sendo opção de linha de comando, a chave vence o `~/.ai-jail`.
  Descartados: `--exec` (o ai-jail exige `--terminal-passthrough` junto e o resultado é o
  mesmo) e `CLAUDE_CODE_DISABLE_MOUSE` (não é documentada e troca o copiar pela rolagem).
- **Hosts dos MCPs remotos: liberados pelas chaves que já existem** (decisão do usuário,
  depois do teste acima). `mcp_jail_hosts`, no `auto_scrum`, devolve `mcp.atlassian.com` com
  `JIRA_ENABLED="true"` e `mcp.sentry.dev` com `SENTRY_ENABLED="true"` (só `"true"` exato,
  nessa ordem), e `main()` passa a lista, separada por espaço, a `jail_check` e `jail_exec`,
  que a repassam a `jail_options`: um `--allow-host` por host, depois das opções do nível,
  na verificação e no lançamento. Motivos:
  - mesma razão do `JAIL_CLIPBOARD`: liberação na linha de comando, sem depender do
    `~/.ai-jail` de cada pessoa — um dev novo que ligasse a jail ficaria sem Jira e Sentry e
    tenderia a desligá-la;
  - as chaves já dizem que o projeto usa aquele MCP (é o que mostra as opções no menu), então
    não precisa de chave nova; alternativas descartadas: uma chave `JAIL_ALLOW_HOSTS` (cada
    projeto teria de preenchê-la) e só documentar;
  - a lista fica no `auto_scrum` (é ele que sabe o que as chaves significam); a biblioteca só
    recebe hosts por parâmetro, sem ler global do chamador. O `--allow-host` libera também os
    subdomínios e soma à lista padrão do ai-jail;
  - limite: são os servidores hospedados oficiais. Jira ou Sentry self-hosted vão em
    `JAIL_ALLOW_HOSTS`; os caches de um MCP local, no `config.toml` global do ai-jail (README). O login OAuth do MCP é feito fora da jail (lá
    dentro o navegador não abre); o token fica em `~/.claude`, que a jail monta.
  Mais um canal de saída por host liberado, como o README avisa em "O que a jail não
  protege".
- **Toolchains do home: lista fixa no nick, somente leitura** (decisão do usuário: nada no
  `~/.ai-jail` de cada pessoa — se alguém esquece, o problema demora a ser descoberto; o
  nick mantém e controla). `jail_toolchain_dirs` devolve, dos diretórios `.pyenv`, `.nvm`,
  `.rbenv`, `.asdf`, `.local/share/mise`, `.local/share/uv/python`, `.sdkman` e `.volta` do
  `$HOME`, os que existem, e `jail_options` passa um `--map` (somente leitura) por diretório.
  Só gerenciadores de versão: nunca diretório de configuração ou credencial. Do uv, só os
  interpretadores (`uv/python`, pra onde o `venv/bin/python` aponta), e não o
  `~/.local/share/uv` inteiro: lá o `uv auth login` guarda credenciais de índice em texto puro
  (`credentials/`, sem keyring) — achado do review; leitura é justamente o risco. Risco aceito,
  menor: um `npmrc` global dentro de uma instalação do node (`<prefixo>/etc/npmrc`, gravado
  por `npm config set -g`) com token fica legível; o comum é o `~/.npmrc` do usuário, que não
  é montado. Descartados:
  `--no-private-home` (o `--dry-run` mostrou que expõe o home quase inteiro: `~/.config` e
  `~/.cargo` com escrita, `~/.copilot`, `~/.terraform.d`), uma chave `JAIL_RO_MAPS` no
  `.nick/nick.conf` (cada projeto teria de preenchê-la) e `ro_maps` no `~/.ai-jail`. Um gerenciador
  novo entra editando a lista (e o teste e o README). O `$HOME` é estado do shell, como o
  `$TMPDIR`; os testes usam um home falso por caso, pra não depender da máquina.
- **Navegadores do Playwright: montados fora do `~/.cache`** (extensão da lista fixa acima,
  pelo mesmo motivo: o nick mantém, ninguém configura). `jail_playwright_browsers` acha o
  diretório (`PLAYWRIGHT_BROWSERS_PATH` do ambiente do usuário, se definida; senão
  `~/.cache/ms-playwright`, o padrão do Linux; nada se não for diretório, se não for
  absoluto — `"0"`, os navegadores dentro do projeto, ou um caminho relativo, ambíguo pro
  ai-jail e já dentro do projeto visível — ou se tiver `:`, que quebraria o
  `--map ORIGEM:DESTINO`), e
  `jail_options` passa `--map <origem>:~/.ms-playwright` e `--env
  PLAYWRIGHT_BROWSERS_PATH=~/.ms-playwright`. Não vai pra `jail_toolchain_dirs` porque não
  pode ser montado no mesmo caminho: o ai-jail monta um `~/.cache` gravável próprio e o
  descarta quando um mapa somente leitura cai dentro dele, e os caches de pip/uv/npm se
  perderiam. O sandbox do Chromium não funciona dentro da jail; o Playwright já lança com
  `--no-sandbox` (`chromiumSandbox: false` é o padrão), e o aviso ao agente diz isso.
- **Estado do Landlock na linha da jail** (pedido do usuário: o ai-jail mostra
  "▸ Landlock: fully enforced" ao abrir, mas o `claude` limpa a tela em menos de um segundo).
  A verificação (`ai-jail ... -- true`) imprime a mesma linha no stderr, que `jail_check` já
  captura; ao passar, ela guarda em `JAIL_LANDLOCK` (global da própria biblioteca, vazia ao
  carregar e zerada a cada `jail_check`) o estado resumido por `jail_landlock_state`:
  `completo` (fully enforced), `parcial` (partially enforced), `inativo` (not enforced,
  failed to apply ou nenhuma linha — `--no-landlock`) e `não identificado` (outro texto, pra
  um texto novo do ai-jail nunca virar um falso "completo"; as linhas de detalhe, como
  "Landlock: ro map ...", são ignoradas — textos tirados do binário 2.14.0; no 3.2.0, a linha
  nova "⚠ Mask: ... rule not applied" da verificação não tem "Landlock: " e é ignorada, coberto
  por teste).
  `jail_print_status` mostra `| Landlock: <estado> |` entre o "ativa" e o modo de clipboard
  quando `JAIL_LANDLOCK` tem valor, sem cor: o ⚠️ já está na linha.
- **Aviso ao agente de que ele está na jail** (sugestão do usuário, depois do caso do
  `~/.pyenv` acima). `jail_agent_notice <hosts> [ports]` gera o texto (PT-BR, como os prompts):
  só o projeto aceita escrita, home privado (com as toolchains montadas e os hosts extras
  listados), rede restrita, `localhost` da máquina inalcançável, nada fora da jail é
  conferível, e, se faltar algo, a primeira hipótese é a jail — parar e avisar o usuário com o
  caminho, serviço ou host, sem diagnosticar a máquina. O `auto_scrum` o passa ao `claude` por
  `--append-system-prompt`, só com `JAIL_ENABLED="true"`: desligada, o argv continua o de antes
  da jail. Pelo system prompt, e não por um bloco nos 8 templates, porque vale pra sessão
  inteira, fica num lugar só e não depende da etapa. O texto fica na biblioteca (o conhecimento
  sobre a jail é dela) e a ferramenta só decide como entregá-lo, porque `jail_exec` é genérico
  sobre o comando e não pode injetar argumento do `claude`. O texto descreve o padrão do
  ai-jail (rede: API do Claude, registries e GitHub; home: estado do `claude`, configuração do
  git e, desde o ticket 27, os caches de dependência do `--toolchains`; reconferido no 3.2.0 com
  `--dry-run`), por isso diz "por padrão": com `allow_hosts` na configuração global do
  ai-jail (`~/.config/ai-jail/config.toml`) ou numa versão nova, pode haver mais. **A cada atualização do ai-jail, reconferir com `--dry-run`
  esse texto e as strings de `jail_landlock_state`** (achado do review: o agente confia no
  aviso).
- **Portas de `localhost`: `JAIL_FORWARD_PORTS` no `.nick/nick.conf`** (decisão do usuário). A
  jail tem a própria rede, então serviços da máquina (o banco do Docker dos testes) ficam
  inalcançáveis. A porta varia por projeto (`54320` vem do `.env` do `siga-construcao`), então
  não dá pra ser automática como as toolchains e os hosts dos MCPs; a chave fica na
  configuração do projeto, que é do nick e commitada — o time herda a lista, e ninguém mexe
  na configuração do ai-jail. Chave de texto (portas separadas por espaço, padrão `""`, registrada por
  último); `jail_check` valida, só com a jail ligada, que cada porta é um número de 1 a 65535,
  sem zero à esquerda e sem repetir (com `case`, sem `[[ =~ ]]`, pela convenção do projeto),
  e para com `jail_print_ports_error` (valor, arquivo e formato, sem
  lista de requisitos) — um erro de digitação nunca abre outra porta nem chega ao ai-jail
  como opção. `jail_options` passa um `--forward-port` por porta, depois dos hosts, na
  verificação e no lançamento, e `jail_agent_notice` diz ao agente quais portas de
  `localhost` ele alcança. Descartados: ler as portas do `.env` (nomes e formatos variam, e o
  nick passaria a ler um arquivo cheio de segredos) e não liberar (os testes de backend não
  rodariam na jail). Cada porta dá ao agente acesso total ao serviço dela; o comentário do
  esqueleto e o README dizem isso.
- **Documentação e outros hosts: lista fixa do nick + `JAIL_ALLOW_HOSTS`** (decisão do
  usuário, depois de um agente na jail não conseguir ler `docs.claude.com`: "proxy refused the
  connection"). O WebFetch roda na máquina, então passa pelo proxy filtrado da jail; a
  WebSearch não (roda no servidor da Anthropic). Duas partes:
  - a lista padrão do nick, `lib/jail/allow_hosts.txt` (lida por `jail_nick_hosts`, caminho em
    `JAIL_HOSTS_FILE`): sempre liberada com a jail ligada, por qualquer ferramenta. É um
    arquivo de dados, e não código, a pedido do usuário: um dev inclui um host sem chamar o
    agente. Um host por linha; linha em branco, `#` e espaços em volta são ignorados; arquivo
    ausente é lista vazia. Os testes leem o próprio arquivo (incluir um host não quebra
    nada), conferem que todo host passa na validação e que a documentação do Claude está
    lá. `jail_check` valida a lista antes dos hosts do chamador, e um host errado nela para
    com erro citando o arquivo, não a configuração do projeto. Conteúdo inicial, escolhido
    com o usuário: documentação do Claude, AWS, Google Cloud, Python, Rust, PostgreSQL,
    Django/DRF, OpenTofu e a stack do `siga-construcao` (pytest, uv, boto3, psycopg,
    gunicorn, drf-spectacular, MDN, Node, TypeScript, React, TanStack, Vite, Vitest, Tailwind,
    Storybook, Playwright, ESLint, Prettier, Docker, git, Sentry, Atlassian).
    `cloud.google.com` libera também os subdomínios, `console.cloud.google.com` incluído.
    `jail_options` põe esses `--allow-host` logo depois dos hosts do chamador;
  - `JAIL_ALLOW_HOSTS` (chave de texto, padrão `""`, registrada por último): os hosts do
    projeto (a documentação da stack, um Jira/Sentry self-hosted). Fica no `.nick/nick.conf`
    commitado, como o `JAIL_FORWARD_PORTS`: o time herda e o PR revisa. O `main()` passa à jail
    os hosts dos MCPs e depois os da chave. `jail_check` valida todos, só com a jail ligada:
    só nome de host (minúsculas, números, ponto e hífen, com ponto; sem `https://`, porta,
    caminho ou curinga), e para com `jail_print_hosts_error`, que cita só o host errado (os
    dos MCPs são sempre válidos, então o errado veio da chave). Host repetido é aceito (um
    MCP também listado na chave não é erro).
  - `jail_agent_notice` cita os hosts da lista padrão e diz ao agente que outro site é
    recusado com "proxy refused the connection": não contornar, e avisar o usuário qual host
    faltou, que ele pode liberar em `JAIL_ALLOW_HOSTS`.
  Descartado: `--prompt-hosts` do ai-jail (notificação no desktop pedindo Permitir/Negar):
  depende do desktop, é fácil aprovar sem ler e abre saída pra qualquer host aprovado.
- **ai-jail 3.x: sem tela de aprovação, credenciais fechadas (ticket 27).** Desde o 3.x, a
  primeira execução de um harness conhecido (`claude` como primeira palavra depois do `--`)
  num terminal abre a tela "Configure claude for this project", cujo preset sugerido ("For
  developers") abre o token do GitHub (OPEN). A linha de comando do nick de antes não a
  evitava. Decisões, com o motivo:
  - **versão suportada: `>= 3.2.0` e `< 4.0.0`**, numa constante só (`JAIL_VERSION_MIN="3.2.0"`
    no topo de `lib/jail/jail.sh`), de onde saem a checagem, as mensagens ("3.2.0 ou mais nova
    da série 3.x", com a série calculada da própria constante) e o teste README × helper. O
    2.14 deixou de ser suportado: o comportamento que o nick usa só foi conferido no 3.2.0, a
    semântica do `--clean` e o onboarding mudaram dentro do 3.x, e suportar as duas exigiria
    opções diferentes por versão sem como testá-las. O teto existe porque a tela veio numa
    major: uma major nova pode trazer outra pergunta; quando sair o 4.x, um ticket confere e
    sobe o teto;
  - **checagem de versão no `jail_check`**, só com a jail ligada, depois das validações de
    valor e do `command -v`, antes do `mktemp`: `ai-jail --version` (só o stdout;
    `jail_version_output`). O formato aceito é exatamente `ai-jail X.Y.Z` (uma quebra de linha
    no fim, partes numéricas sem zero à esquerda e com até 9 dígitos, sem sufixo como `-rc1`;
    `jail_version_of`), e a comparação é numérica (`3.10.0` > `3.2.0`;
    `jail_version_supported`). Fora da faixa, ilegível ou com `--version` falhando, para com
    `jail_print_check_error`: a versão encontrada (ou "não identificada", com a saída, controles
    trocados por `?`), a suportada, "atualize o ai-jail" no lugar da saída 1 (o 5º parâmetro
    opcional `[way_out]`; sem ele, o texto é byte a byte o de antes) e a lista de requisitos.
    Isso revoga o "detectar a versão do ai-jail" do fora de escopo;
  - **`--requisitos-jail` mostra a versão**: `[ok]    ai-jail 3.2.0` na faixa, `[atualizar]
    ai-jail <versão ou (versão não identificada)>  →  <comando>` fora dela (o de instalação,
    menos no macOS: `brew upgrade ai-jail`), e sempre a linha "Versão suportada do ai-jail:
    ...". Com `[falta]`, igual a antes, sem `--version`;
  - **o perfil salvo pela tela é ignorado (`--clean`)**: o lançamento sai sempre com o GitHub
    fechado, haja ou não perfil (conferido). O nick não lê, não apaga nem altera o perfil; o
    README diz onde ele fica e como removê-lo, pra quem usa o ai-jail sem o nick;
  - **opções novas em `jail_options`**, logo depois de `--no-save-config --no-status-bar` e
    antes das do `JAIL_CLIPBOARD` (a ordem das demais não mudou): `--clean` (sem tela e sem
    perfil do projeto, salvo ou legado), `--agent-state` (login do `claude` liberado pelo nick,
    não pela tela), `--toolchains` (o "Dependency tools" da tela; as toolchains do home seguem
    no `--map` de `jail_toolchain_dirs`) e `--no-github
    --no-ssh --no-aws --no-kube --no-gcloud --no-docker-config --no-docker --no-inherit-env`.
    Os `--no-*` são explícitos porque a configuração global (`~/.config/ai-jail/config.toml`) é
    confiável pro ai-jail e continua valendo com `--clean`: um `github = true` global abriu o
    token mesmo com `--clean` (conferência do Tech Leader); o socket do Docker dá root no host,
    e a herança do ambiente levaria um `GH_TOKEN` do shell pra dentro;
  - **trade-off do `--toolchains`** (achado do review): ele monta, graváveis, os caches do
    próprio ai-jail (`~/.local/share/ai-jail/cache/` da máquina, vistos como `~/.cache`,
    `~/.npm`, `~/.cargo/registry`, `~/.cargo/git`) e, somente leitura, `~/.rustup` e
    `~/.cargo/bin` da máquina (`--dry-run` do 3.2.0). Antes o home da jail era um tmpfs e nada
    escrito fora do projeto sobrevivia à sessão; agora esses caches persistem e são
    compartilhados entre projetos e sessões, então um agente sob prompt injection num projeto
    pode deixar um pacote adulterado que outro projeto instala depois. Risco aceito: sem os
    caches, cada sessão baixaria tudo de novo (era o "Dependency tools" que a tela já oferecia
    ligado), e o ai-jail os separa dos caches reais da máquina. O README diz isso em "O que a
    jail não protege" e como zerar (apagar o diretório fora da jail). O `jail_agent_notice`
    cita esses caches e toolchains (`~/.cargo/bin`, `~/.rustup`) como exemplo, sem tentar
    listar tudo o que o `--toolchains` monta: a lista é interna do ai-jail (cargo, go, maven,
    pnpm, bun, mise...) e só sai rodando ele; por isso o aviso não diz mais que `~/.local`
    inteiro "não existe";
  - **aviso ao agente**: uma linha diz que o token do GitHub e o SSH não estão na jail, que o
    git local e o fetch de repositório público funcionam e que o resto (gh, push, PR, fetch de
    repositório privado) fica com o usuário — inclusive que o git pode ficar esperando usuário
    e senha. Mesmo racional do aviso: o agente não gasta tokens diagnosticando um push que
    falha;
  - **`git fetch`/`push` por HTTPS**: conferido (ver a conferência com o 3.2.0): a rede alcança o
    `github.com`, mas não há credencial; público faz fetch, privado e push falham pedindo
    usuário e senha — travando num terminal. Só documentado (README e aviso), sem mexer em
    `GIT_TERMINAL_PROMPT`.
  Descartados: `--no-network` (com `--allow-host`, derruba a lista padrão do ai-jail; nem a API
  do Claude respondeu); `--network` (não combina com `--allow-host`; um `network = true` global
  faz a verificação falhar, e o README diz pra tirar a linha); `--harness claude`,
  `--claude-dir` e `-- /usr/bin/claude` (não evitam a tela e não são necessários); `-- env
  claude`/`-- bash -c 'claude ...'` (evitam a tela porque o ai-jail não reconhece o harness, mas
  perdem a detecção do estado do `claude`); `--no-agent-state` (o `claude` subiria deslogado).
  Achado no caminho: o teste "README × `jail_options`" nunca conferia nada (a suíte não carrega
  a biblioteca, então `jail_options` dava "comando não encontrado" dentro do `< <(...)`); passou
  a rodar num bash que carrega a biblioteca, com uma guarda que falha se o laço conferir menos
  opções que as da base.
- **Review de PR.** Mudanças nas chaves da jail do `.nick/nick.conf` (`JAIL_ENABLED`,
  `JAIL_ALLOW_HOSTS`, `JAIL_FORWARD_PORTS`) passam pelo mesmo review de PR que o `CLAUDE.md`:
  desligar a flag num PR tira a jail de todo o time, e cada host ou porta amplia o acesso do
  agente. (O `.ai-jail` do projeto saiu daqui no ticket 27: pelo nick, no 3.x, ele não vale.)

**Fora de escopo, de propósito:** as regras de deny/ask do Claude Code, que não são da jail
(ficam em `lib/claude/`, ver seção "Regras de deny/ask do Claude Code (`lib/claude/`)");
`--lockdown`; flags de rede além do `--allow-host` (MCPs, `JAIL_ALLOW_HOSTS` e
`lib/jail/allow_hosts.txt`) e do `--forward-port` de `JAIL_FORWARD_PORTS` (ex: `--network`,
`--allow-private-host`, `--no-network`); forçar a jail numa máquina independente da
configuração do projeto; ler, gerar, validar ou apagar o `.ai-jail`, o `~/.ai-jail`, o perfil
do projeto (`~/.config/ai-jail/projects/`) e o `config.toml` global; rodar a verificação do
`jail_check` no `--requisitos-jail` (ele só checa presença no `PATH` e a versão do ai-jail);
detectar a versão do bubblewrap; verificar se o `claude` existe dentro da jail antes do
lançamento; um executável wrapper (`bin/nick-jail`) enquanto não houver segunda ferramenta;
acesso ao GitHub dentro da jail (se um dia precisar, vira uma chave opt-in separada, com
padrão fechado, em outro ticket — inclui `ai-jail forge setup`); o contrato de wrapper do
ai-jail (`--harness`, `--request-file`, `status --json`); mudar `GIT_TERMINAL_PROMPT` ou o
credential helper do git. A detecção da versão do ai-jail saiu desta lista no ticket 27 (ver
"ai-jail 3.x" acima).

## Regras de deny/ask do Claude Code (`lib/claude/`)

A jail isola a máquina, mas não protege o próprio repositório (`git reset --hard`, `git
clean`), o remote (`git push --force`) nem impede commit ou push sem revisão; e é opt-in.
Toda sessão do `claude` aberta pelo nick, em todas as etapas, com ou sem jail, recebe um
conjunto fixo de regras de permissão do Claude Code:

```
claude --settings "<conteúdo de lib/claude/permissions.json>" [--permission-mode auto] [--append-system-prompt <aviso da jail>] "<prompt>"
```

Com a jail: `ai-jail <opções> -- claude --settings ...`. `--settings` vem sempre primeiro,
antes do `--permission-mode`.

**Como funciona:** `lib/claude/permissions.json` é `{"permissions": {"deny": [...], "ask":
[...]}}`, sem `allow`, `defaultMode` nem outra chave. `claude_settings_load NICK_CLAUDE_SETTINGS
|| exit 1` roda em `main()` logo depois de `jail_check` (antes de `jail_print_status`, dos
`build_*`, do menu e do `mkdir -p "$LOGS_DIR"`): lê o arquivo dentro da função (`cat`, sem
`jq`) e grava o conteúdo na global com `printf -v`. Arquivo ausente, ilegível ou vazio (só
espaços, tabs e quebras de linha contam como vazio) → `Erro: regras de permissão do Claude Code não
encontradas ou ilegíveis: <caminho>` no stderr e código 1: nenhuma pergunta, nenhum log,
`claude` não aberto, com e sem jail. Com a jail ligada, a verificação dela (`ai-jail ... --
true`) já rodou nesse ponto; o lançamento, não. `--init` e `--requisitos-jail` saem antes e
não carregam as regras, porque não abrem o `claude`. A biblioteca não valida o JSON: isso
fica na suíte (`jq` e/ou `python3 -m json.tool`, mais a checagem das chaves e dos tipos), que
dá FAIL, e não PULADO, sem nenhum dos dois.

**Decisões, cada uma com o motivo:**
- **Regras num JSON separado, fonte única.** Nenhum `Bash(`/`Read(` em código de `claude.sh`
  ou do `auto_scrum.sh` (teste). O JSON é o formato do próprio Claude Code, então pode ser
  revisado, validado e copiado sem traduzir; e uma ferramenta futura do nick reaproveita o
  mesmo arquivo pela mesma biblioteca.
- **`--settings` inline, com o conteúdo, e não um caminho nem um arquivo de settings.** Um
  caminho fora do projeto pode não existir dentro da jail (o home é privado). Gravar em
  `.claude/settings*.json` do projeto alvo mexeria no repositório de outro time (e entraria no
  git dele), e em `~/.claude/settings.json` mudaria o `claude` do usuário fora do nick. O
  nick não cria nem altera nenhum desses (teste com cwd e `HOME` temporários). As regras de
  `--settings` se somam às dos settings do usuário e do projeto, e deny/ask vencem allow.
- **Sempre ativo, sem flag e sem fallback.** Nenhuma chave de configuração nova (`CONFIG_KEYS`
  igual à de antes; teste), nenhuma pergunta "seguir sem as regras?". É o mesmo espírito da
  jail: quem roda o nick nunca abre o agente sem a proteção por engano. Um projeto que precise
  de uma ação bloqueada roda o `claude` fora do nick.
- **deny = irreversível ou fora do projeto; ask = versionamento e perda de trabalho local.**
  deny: privilégio (`sudo`, `su`, `doas`), destruição em massa (`rm -rf` e variantes de ordem e
  de opção longa), reescrita do remote (`git push --force`/`-f`; `--force*` cobre
  `--force-with-lease`) e segredos fora do projeto (`~/.ssh`, `~/.aws`, `~/.gnupg`). ask:
  `git add`/`commit`/`push` (reforça a regra dos prompts de não versionar sem autorização —
  `development.md` continua dizendo isso), `git reset --hard`, `git clean`, `git checkout --`,
  `git restore`, `git branch -D` e a leitura de `.env`/`.env.*` do projeto (o `.env.example`
  também pergunta; aceito). deny vence ask: `git push --force` é negado mesmo casando
  `git push *`, e um teste garante que nenhuma regra está nas duas listas.
- **Sintaxe atual do Claude Code**, conferida contra a versão instalada (2.1.312, 2026-11-02):
  `Bash(<padrão>)` com `*` como curinga em qualquer posição e `Read(<glob>)` com `~/` pro home e
  `**` recursivo. A documentação online (docs.claude.com) não foi alcançável de dentro da jail
  (antes da lista padrão de hosts, `lib/jail/allow_hosts.txt`, que hoje a libera); a
  conferência foi no próprio binário: o `--help` documenta `--settings <file-or-json>` ("Path
  to a settings JSON file or a JSON string") e dá `"Bash(git *)"` como exemplo; o validador de
  settings diz "Use * for wildcards", com exemplos `Read(~/.zshrc)` e `Edit(docs/**)`, e trata o
  `:*` legado como prefixo só no fim do padrão ("has a :* that is not at the end, so it is
  matched as a * wildcard"). Uma regra com sintaxe inválida pode ser ignorada sem aviso, então
  **a cada atualização do Claude Code, reconferir a sintaxe** (e o teste manual abaixo).
- **A biblioteca não devolve array.** Bash só faria isso com `eval` ou nameref, e imprimir um
  argumento por linha quebraria com o JSON multilinha. A lib entrega o conteúdo; quem monta
  `claude_args=(--settings "$NICK_CLAUDE_SETTINGS")` é o `auto_scrum.sh`, e o argv final é
  coberto por teste em cada tipo de etapa.
- **Sem abstração multi-agente** (`lib/ai/`, regras genéricas traduzidas pra cada agente): só
  existe o Claude Code; o nome `lib/claude/` deixa claro de quem é o formato.

**Limitação: o casamento é pelo texto do comando.** As regras são uma grade de proteção contra
erro do agente, não um isolamento — quem quer contornar contorna. Exemplos que passam:
`bash -c "sudo ..."`, `python -c 'import os; os.system("rm -rf ...")'`, `git -C . push -f`
(o `-C` muda o começo do comando), `cat .env` (a regra `Read(...)` vale pra ferramenta de
leitura, não pro `cat` no Bash) e o refspec com `+` (`git push origin +main`, um force push
sem `--force`). O isolamento de verdade é a jail (seção "Sandbox do agente com ai-jail").

**Riscos conhecidos:**
- uma versão nova do Claude Code pode mudar a sintaxe ou a semântica das regras, e uma regra
  inválida pode ser ignorada em silêncio — reconferir a cada atualização;
- as regras do usuário ou do projeto alvo continuam valendo junto (um `deny` deles soma); um
  `allow` deles não fura o deny/ask do nick, pela precedência;
- `.env.example` (e qualquer `.env.*`) pede confirmação pra ser lido, o que pode irritar;
- casamento por texto (acima).

**Teste manual (roteiro; executado pelo usuário — resultado pendente):** num repositório
descartável fora do nick_secrets, com o remote bare **dentro** do próprio projeto e escondido
do git (`git init /tmp/nick21-proj && cd /tmp/nick21-proj`; `git init --bare .remoto.git`;
`echo .remoto.git >> .git/info/exclude`; `git remote add origin ./.remoto.git`; um commit
inicial e `git push -u origin HEAD` feitos à mão), abrir pelo nick uma sessão de
Desenvolvimento (`--permission-mode auto`), uma vez com `JAIL_ENABLED="false"` e outra com
`"true"`, e pedir ao agente: `sudo true` → bloqueado; `git push --force` → bloqueado; `git
commit --allow-empty -m teste` → pede confirmação. O remote fica dentro do projeto porque, com
a jail, só o projeto é visível: um remote fora dele faria todo `git push` falhar por erro do
git, e o "bloqueado" passaria mesmo com a regra ignorada (achado do review). **Bloqueado**
quer dizer recusa do Claude Code (permissão negada por regra, sem o comando rodar), não um
erro do `sudo` ou do `git`; a contraprova é um `git push` comum, aprovado no ask, que tem de
publicar no remote. Registrar aqui e no PR o resultado de cada um.

**Fora de escopo, de propósito:** flag pra desligar; gravar em `.claude/settings*.json`;
regras por projeto; hooks; regras de rede/MCP; `lib/ai/`; mudar `lib/config.sh` ou
`lib/jail/jail.sh`; qualquer dependência nova em runtime.

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
entrada, não chave de configuração: não é registrada com `config_register_key`.

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
