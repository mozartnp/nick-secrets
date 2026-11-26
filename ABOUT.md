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
também nomes de diretório/arquivo internos (`lib/config/`, `templates/*.md`) e os
valores de `$TYPE` (`po`, `tech_leader`, `development`, `review`). Fica em português só o
que é visual pro usuário: os textos do menu (`select`), prompts de `read -p`, mensagens de
erro/confirmação, a flag `--requisitos-jail`, e o conteúdo em si dos templates (`.md`) — que o
usuário lê/edita diretamente.

**Por quê:** identificadores, comentários e nomes de arquivo/diretório são "estrutura do
projeto", lida só por quem mexe no código — inglês é a convenção comum pra isso. Já o que
aparece na tela ou é digitado pelo usuário (menus, prompts, o texto dos templates) é
conteúdo voltado a ele, e trocar isso pra inglês só atrapalharia sem ganho real.

## Qualidade de código bash

As ferramentas e as bibliotecas do nick (`lib/`, `bin/nick-init`, `bin/nick-agent`, `auto_scrum`)
e o instalador (`install/install.sh`, NS-52) são em bash. O
Rust entra em dois lugares, com regras próprias: a casca `nick`, em `nick/` (ver a regra de
linguagem em "Rust no projeto: a casca `nick` (NS-31)"), e o protótipo descartável do painel,
em `prototype/panel/` (ver "Rust no projeto: protótipo do painel (NS-29)"). Os scripts bash de
lá (o `nick/tests/test_cargo.sh`, o init falso dos testes da casca e o `launch_session.sh` do
protótipo) e os falsos do `claude`/`ai-jail` em `bin/tests/fixtures/bin/` (NS-33) seguem estas
regras. Regras a seguir em todo script novo ou alterado:

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
  morde) só numa cópia descartável **fora do repositório**, num diretório **novo** criado com
  `mktemp -d` na hora da sabotagem (ex: `copia="$(mktemp -d)"`), com o caminho anotado.
  **Proibido** reaproveitar uma cópia de sabotagem anterior, usar um caminho fixo (ex:
  `/tmp/copia-sabotagem`) ou limpar qualquer cópia antiga, antes ou depois: pra sabotar de novo,
  crie outro `mktemp -d`. A cópia leva só (caminhos a partir da
  raiz) `Makefile`, `tests/`, `install/` (o instalador e a suíte dele, desde o NS-52), `auto_scrum/auto_scrum.sh`, `auto_scrum/prompt.sh` (desde o
  NS-35), `auto_scrum/templates/`,
  `auto_scrum/tests/`, `bin/`, `lib/`, `README.md` (lido pelo teste README × helper de
  `lib/tests/test_jail.sh`), `.gitignore` (lido pelos testes de `.gitignore` de
  `auto_scrum/tests/test_auto_scrum.sh`) e `nick/` sem `nick/target/` (a casca Rust, desde o
  NS-31), na mesma estrutura (sem copiar `auto_scrum/logs/`,
  que tem dados reais), com um `git init` na cópia e o cwd dentro dela ao rodar a suíte (ou o
  `make test`). O cwd do Bash tool volta pra raiz entre chamadas, então o `cd "$copia"` vai no
  **mesmo** comando que roda a suíte (ex: `cd "$copia" && bash tests/run_all.sh`). O cargo da
  cópia usa um target dentro do mesmo diretório (ex: `CARGO_TARGET_DIR="$copia/target-cargo"`),
  pra tudo ficar num caminho só. No fim, o agente **não apaga** a cópia: confere, a partir da
  raiz do repositório, com `git status --short --ignored` que nada mudou ali (obrigatório) e
  informa o caminho da cópia no relatório final. Com a jail ligada (o agente sabe pelo aviso da
  jail na instrução de sistema da sessão), a cópia some sozinha quando a sessão termina, porque
  o `/tmp` da jail é privado. Com a jail desligada, ou sem certeza, a cópia fica no `/tmp` da
  máquina, e o usuário decide se apaga.

**Por quê:** essas regras evitam as três classes de bug mais comuns em bash —
variável não citada que quebra com espaço/glob, variável de função vazando pro
escopo global, e falha de comando ignorada silenciosamente por causa do
`set -e` ausente. A regra da sabotagem existe porque os testes unitários usam o
`LOGS_DIR` e o cwd reais (só os de `main()` apontam pra diretórios temporários), e a
configuração da raiz (`.nick/nick.conf`, ou o `.nick.conf` de antes do
ticket 24 num clone ainda não migrado) e a `.nick/` inteira são gitignoradas, sem cópia: uma
sabotagem que escreve arquivo, rodada da raiz do repositório, zera ou cria arquivos reais —
já aconteceu na validação do ticket 5, com o `.nick.conf` do usuário. O `.gitignore` e o
`git init` entram na cópia porque os testes de `.gitignore` da `test_auto_scrum.sh` (ticket 24)
usam `git check-ignore` contra o `.gitignore` da raiz: sem os dois, eles dão FAIL falso, e a
suíte acusaria falha sem relação com a sabotagem (aconteceu na validação do ticket 25). O
`bin/` entra na cópia desde o NS-30: a suíte `bin/tests/test_nick_init.sh` roda o
`bin/nick-init`, e as mensagens do `auto_scrum.sh` apontam o caminho dele. O `nick/` entra desde
o NS-31, sem o `nick/target/` (build grande, que o cargo refaz). Rode o cargo da cópia com
`CARGO_TARGET_DIR` fora do repositório (desde o NS-47, dentro do diretório da cópia). Ao
restaurar um arquivo sabotado, dê `touch` nele: o
`.orig` guardado com `cp` tem mtime mais antigo que o build sabotado, e o cargo não recompila
(o controle "sem sabotagem" passaria a falhar por engano, como aconteceu na validação do
NS-31).

A regra não manda apagar nada desde o NS-47. Antes ela terminava mandando remover a cópia, e o
deny de `rm -rf` do nick (`lib/claude/permissions.json`) recusa isso em qualquer caminho. Com a
instrução de não contornar do NS-41, o agente parava e perguntava em toda validação com
sabotagem (aconteceu na validação do NS-41). Alternativas descartadas:

- **Exceção no deny pra `/tmp`:** deny vence allow, então um `allow` pra `rm -rf /tmp/*` não
  furaria o deny. Restringir o próprio deny por caminho também não funciona, porque o casamento
  é pelo texto do comando (`cd /tmp/x && rm -rf .`, `rm -rf "$copia"` com variável). Ver
  "Regras de deny/ask do Claude Code (`lib/claude/`)".
- **Mexer no `/tmp` da jail:** sem jail, o `/tmp` é o da máquina, então mudar a jail não
  resolveria esse caso. E a jail é opt-in.
- **Helper (script, função de `lib/` ou alvo de `make`) que apaga a cópia:** seria um caminho
  aprovado pra contornar o deny, o oposto do NS-41.

A garantia "nada a apagar com a jail" depende de o ai-jail montar um `tmpfs` privado em `/tmp`,
que some com a sessão. Conferido com `findmnt -T /tmp` dentro da jail no ai-jail 3.5.0 (NS-47,
2026-11-21): `tmpfs tmpfs rw,nosuid,nodev,relatime,mode=755,uid=1000,gid=1000,inode64`, com o
`TMPDIR` vazio (o `mktemp -d` cai em `/tmp`). **Rever esta regra quando o ai-jail mudar de
versão** (subir o `JAIL_VERSION_MIN` ou sair da série 3.x, em `lib/jail/jail.sh`). Se uma versão
futura deixar de usar `/tmp` privado, as cópias passam a se acumular no host sem ninguém notar.

Impactos aceitos: sem jail, as cópias se acumulam no `/tmp` da máquina até o usuário apagar ou
até o reboot. Não têm segredos (a cópia não leva `.nick/` nem `auto_scrum/logs/`), mas ocupam
espaço, por isso o caminho é sempre informado. Com jail, o `/tmp` é `tmpfs`: a cópia e o build do
cargo ocupam RAM até a sessão acabar, e por isso o target fica dentro da cópia, num caminho só.
Se o agente ignorar a regra e tentar `rm -rf`, o deny recusa e a instrução do NS-41 faz ele
parar e perguntar ("Instrução de não contornar uma recusa (NS-41)").

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
- **Uma de cada vez, com o cwd na raiz:** as suítes usam o `LOGS_DIR` e o cwd reais (ver a
  regra da sabotagem em "Qualidade de código bash"), então não podem rodar em
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
  valem. Cada alvo tem uma linha de receita só, que chama o script (desde o NS-52, a suíte do
  runner confere isso por alvo, e não mais "uma linha no arquivo todo", decisão do usuário quando
  entraram o `install` e o `uninstall`). No script elas valem sem exceção, ele tem a própria suíte e roda sem `make`. Fica em
  `tests/` da raiz, o lugar dos testes do repositório como um todo; `lib/` é só pra
  bibliotecas carregadas com `source`.
- **`make` sem alvo roda o `test`** (`.DEFAULT_GOAL := test`): rodar os testes não grava nada no
  repositório (as suítes usam `mktemp`), e um alvo de ajuda seria uma lista a manter à mão (o
  problema da seção "README genérico"). **Revista no NS-52**, quando entraram o `install` e o
  `uninstall`: continua `test`, porque `make` sem alvo nunca deve instalar nem desinstalar nada
  (grava fora do repositório). A suíte do instalador confere que `make -n` sem alvo é o runner. `test` é `.PHONY`, pra um arquivo chamado `test`
  na raiz não fazer o `make` achar o alvo em dia. O `.NOTPARALLEL:` é garantia explícita:
  hoje a receita é uma só, e o `make -j4 test` já roda tudo em série, mas o alvo pode um dia
  ser dividido em um por suíte.
- **O `make test` exige o `cargo` desde o NS-31** (impacto aceito no ticket): a suíte
  `nick/tests/test_cargo.sh` roda `cargo fmt --check`, `cargo clippy --all-targets -- -D
  warnings` e `cargo test` da casca, com o cwd no crate. Sem `cargo` no PATH, ela falha com
  uma mensagem que diz o que instalar, em vez de passar sem testar a casca. Ver "Rust no
  projeto: a casca `nick` (NS-31)".
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
  pelo texto — uma opção que se chame "Sair (q)" continua sendo uma opção comum;
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
  isso que o resultado volta por `printf -v`, e o menu roda no shell principal (`main` chama
  `choose_type` diretamente). Um teste estrutural garante que nem `select_menu` nem
  `choose_type` aparecem depois de `$(`. Desde o NS-30 o menu principal (de tipo de conversa)
  é o único: o menu de projetos e o de destino do `--init` saíram junto com o `projects/` e o
  `--init` (ver "Configuração do projeto: só `.nick/nick.conf` no cwd").

**A saída:** `quit_from_menu` é o único caminho, com o texto `Saindo. Nada foi feito.` só nessa
função, no **stdout**, e `exit 0`. Sair é uma ação normal, como responder `N` na confirmação de
envio, que também usa stdout e código 0. É `exit`, e não `return`, porque voltar até `main()`
exigiria um sinal a mais em cada chamador (quando a regra nasceu, havia um menu de projetos
antes do principal). Não há efeito colateral porque a ordem já garante isso: em `main()`, o
`mkdir -p "$LOGS_DIR"`, o log, a árvore `.nick/` (`config_dir_prepare`) e o lançamento do
`claude` (`jail_exec`) só acontecem depois de `choose_type`. Os testes
de ponta a ponta (com um `claude` falso no `PATH` e um controle positivo que prova que o stub é
usado) protegem essa ordem.

**Ctrl+D continua igual** (fora do escopo do ticket, mas o caminho foi tocado e ganhou teste
de regressão): no EOF, `select_menu` deixa a variável vazia e retorna 1, o mesmo status do
`select` puro. `choose_type` faz `select_menu ... || return 1`, e o `set -e` de `main()`
encerra com código 1, sem mensagem. Atenção: chamar
a função num `if !` ou antes de `||` desliga o `set -e` **dentro** dela. Isso é aceitável
porque o corpo de `select_menu` só tem `select`, `printf -v`, `echo`, `[ ]` e
`quit_from_menu`, e o `exit` funciona do mesmo jeito. Não acrescente ao helper comandos que
dependam do `set -e`.

**Por que `q`:**
- é a tecla de sair de `less`, `man`, `top`/`htop` e do pager do `git`, então quem usa
  terminal já a conhece;
- `s` (de "sair") não serve: no próprio script, `s` já significa "sim" nas confirmações
  `(s/N)`, e a mesma tecla teria sentidos opostos em telas vizinhas;
- um número fixo também não serve: o número do Sair muda conforme as flags, e o `select` não
  mostra um `0`.

**Fora de escopo, de propósito:**
- prompts de texto (`read -p`, inclusive a confirmação `(s/N)`) e o loop do editor em
  `read_via_editor`;
- mudar o comportamento do Ctrl+D (só ganhou testes de regressão) e tratar Ctrl+C (`trap`);
- aceitar `sair`, `quit`, `qq` ou `q` com espaços em volta.

## README genérico: sem repetir detalhe que muda com o código

O `README.md` descreve uso e comportamento estável: como rodar, de onde vem a configuração e em
que ordem, a regra de formato, o que é versionado onde, notas de migração. Listas que crescem a
cada ticket — chaves de configuração, quais etapas usam cada flag, opções de menu — **não** vão
pro README: ficam no código, neste `ABOUT.md` ou nos comentários do esqueleto gerado pelo
`bin/nick-init`. A fonte única da lista de chaves da configuração do projeto é o registro
(`config_register_nick_keys`, em `lib/config/keys.sh`), de onde saem o esqueleto e os avisos
(ver seção "Registro de chaves da configuração"). O README remete a essas fontes em vez de
copiá-las. Pelo mesmo motivo, o README não traz um exemplo de arquivo de configuração completo:
ele teria de listar todas as chaves. O exemplo completo é o arquivo gerado pelo
`bin/nick-init`, e o README só ilustra o formato com uma linha preenchida.

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
(`.nick/nick.conf` no cwd — ver seção "Configuração do projeto: só `.nick/nick.conf` no cwd"
abaixo), no formato
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

Toda flag nova é uma chamada `config_register_key` no registro de `lib/config/keys.sh`
(`config_register_nick_keys`, ver seção "Registro de chaves da configuração"): o leitor (`config_load`, em
`lib/config/config.sh`) ignora qualquer chave fora do registro, então uma flag só escrita no arquivo
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
criado pelo `bin/nick-init` puxe menção a uma ferramenta que ele não usa sem querer. `TDD_ENABLED` é
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
conta como presente, linha comentada e chave parecida não contam). Um
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
- **O esqueleto do `bin/nick-init` já traz a linha** (`PRODUCTION_ENABLED="false"`, com comentário mandando ligar
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
- pergunta interativa no init (hoje o `bin/nick-init`): ele continua só gerando o esqueleto
  comentado, como faz pras outras chaves;
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
- **Só `"false"` exato desliga.** `"true"`, vazio, chave ausente e qualquer valor
  inválido (`"TRUE"`, `"False"`, `" false"`, `"sim"`) mantêm a TDD ligada. Um erro de digitação
  nunca pode remover a TDD de um projeto que tem testes; o erro contrário (TDD ligada num
  projeto sem testes) é o problema de antes da flag, e o aviso de valor já o torna visível. O
  leitor (`config_load`, em `lib/config/config.sh`) não muda: `"TRUE"` continua atribuído literal e
  recebe o aviso; quem decide é só `build_tdd_blocks`, com `[ "$TDD_ENABLED" = "false" ]` —
  essa é a regra de desligamento, não o padrão da chave.
- **As outras flags `_ENABLED` continuam opt-in** (padrão desligado). Uma flag nova não deve
  copiar esta exceção sem um motivo do mesmo tipo (desligar por padrão removeria um
  comportamento que todo projeto já tinha e que continua sendo o esperado).
- **Padrão por chave só no registro.** A chamada `config_register_key` de cada chave é o único
  lugar com o padrão dela (`"true"` pra `TDD_ENABLED`, `"false"` pras outras liga/desliga, `""`
  pras de texto); `config_key_default` (em `lib/config/config.sh`) só o devolve. Ele é usado pela
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
- pergunta interativa no init (hoje o `bin/nick-init`);
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

O cwd também decide a configuração do projeto (`STACK_DESCRIPTION`/`*_ENABLED`): ela vem só
do `.nick/nick.conf` do cwd, e sem ele o script para — ver seção "Configuração do projeto: só
`.nick/nick.conf` no cwd" abaixo. É também no cwd que ficam a troca de arquivos e os docs de
cada ticket (seção "Pasta `.nick/` no projeto alvo").

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

Os nomes são constantes de `lib/config/config.sh`, atribuídas na carga e relativas ao cwd:
`CONFIG_NICK_DIR_NAME=".nick"`, `CONFIG_EXCHANGE_DIR_NAME=".nick/troca"`,
`CONFIG_DOCS_DIR_NAME=".nick/docs"` e `CONFIG_CWD_FILE_NAME=".nick/nick.conf"`, escritas por
extenso (um teste de `lib/tests/test_config.sh` garante que `CONFIG_CWD_FILE_NAME` é
`$CONFIG_NICK_DIR_NAME/nick.conf` e que as duas pastas ficam dentro dela). O `auto_scrum.sh`
continua com `NICK_DIR`, `EXCHANGE_DIR` e `NICK_DOCS_DIR` (usados nos prompts e nas perguntas),
agora atribuídos a partir delas — fonte única, sem mudar o valor. Um teste de consistência
garante que a linha `!nick.conf` do `.gitignore` é o `basename` de `CONFIG_CWD_FILE_NAME` (a
exceção não pode desalinhar do nome lido pela biblioteca — o código monta a linha a partir da
constante).

**`config_dir_prepare`** (`lib/config/config.sh`; era o `nick_dir_prepare` do `auto_scrum.sh`,
que substituiu o `exchange_dir_prepare` do ticket 21, movido no NS-30 sem mudar comportamento
nem mensagem) monta a árvore inteira, menos o `nick.conf` (do `config_init`) e as pastas
`docs/<ID>/` (dependem do ticket;
o agente as cria ao gravar): `mkdir -p` de `troca/` e `docs/` e, **só se `.nick/.gitignore`
não existir** (`[ -e ]`), grava exatamente `*`, `!.gitignore` e `!nick.conf`, sem comentário.
Regras:
- um `.gitignore` existente, editado ou não, nunca é sobrescrito; `troca/` e `docs/`
  existentes ficam como estão, com o que tiverem dentro;
- não cria `.gitignore` em `troca/` nem em `docs/` (o `*` de cima já as ignora, e o git não
  versiona pasta vazia). Um `.nick/troca/.gitignore` antigo (do ticket 21) não é tocado;
- é chamada em dois pontos: no `config_init` (o `bin/nick-init`) e em `main()` do
  `auto_scrum.sh` ao confirmar o envio (`|| exit 1`), só pra completar o que faltar (pasta
  apagada à mão, projeto migrado com `mv`). Cancelar o envio não cria nada;
- se não der pra criar (ex: `.nick/troca` é um arquivo, ou `.nick/` sem permissão de escrita),
  imprime só `Erro: não foi possível criar a estrutura .nick/ (.gitignore, troca/ e docs/) em
  <PWD>.` (a gravação do `.gitignore` fica num grupo `{ ...; } 2>/dev/null`, porque um
  `> arquivo 2>/dev/null` que falha mostra o erro cru do bash antes de o `2>` valer) e
  retorna 1, sem `exit`: o `config_init` retorna 1 sem gravar o `nick.conf` (o `bin/nick-init`
  sai com 1), e `main()` para antes do `claude`, porque o prompt promete a pasta.

**A árvore nasce no `bin/nick-init`** (pedido do usuário, desde o `--init` do ticket 24): ele
termina com `.gitignore`, `nick.conf`, `troca/` e `docs/` criados e mostra essa árvore na
saída, explicando o que é versionado. Assim o time vê e versiona a estrutura desde o primeiro
commit, sem depender de uma primeira execução. A ordem no `config_init <nome da ferramenta>`
é: confirmação de sobrescrita → `config_dir_prepare` (cria `.nick/`, que o `nick.conf`
precisa) → esqueleto (`config_print_skeleton`) → árvore na saída → aviso de legado, se houver.
Respondendo N à sobrescrita, nada é criado nem completado: `Cancelado. Nada foi alterado.` no
stdout e código 0. **Ctrl+D na sobrescrita** (o `read` falha no EOF) mostra a mesma mensagem no
**stderr** e retorna 1, com o mesmo "nada criado" — é o canal e o código do EOF no antigo menu
de destino do `--init`. Antes do NS-30, o EOF nessa pergunta derrubava o script pelo `set -e`,
sem mensagem; o tratamento usa `if ! read ...` e não depende do `set -e` do chamador. É a única
mudança de comportamento do init pedida no NS-30. Na review do NS-30 entraram dois cuidados de
API (o `config_init` é público, e a casca Rust e outros módulos vão depender dele), no mesmo
padrão de `config_register_key` e `config_dir_prepare` — o chamador nunca vê o erro cru do bash:
- **número de argumentos:** sem exatamente 1 argumento não vazio, imprime `Erro: config_init
  espera 1 argumento (o nome da ferramenta), recebeu <N>.` no stderr e retorna 1, sem criar
  nada. A checagem vem antes de qualquer `$1`, então vale também sob `set -u` (antes, era o erro
  cru `$1: variável não associada`; sem `set -u`, gravava um cabeçalho "pro ." com código 0);
- **falha ao gravar o `nick.conf`** (somente leitura, ou um diretório com esse nome): `Erro:
  não foi possível gravar <caminho>.` no stderr e código 1, com o mesmo grupo
  `{ ...; } 2>/dev/null` do `.gitignore`. A árvore pode já ter sido completada nesse ponto, como
  antes. `config_print_skeleton` continua sem validar o argumento (fora do escopo do NS-30).

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

**Por que a árvore e o init foram pra `lib/config/config.sh` (NS-30):** até o NS-30, o
`nick_dir_prepare` ficava no `auto_scrum.sh` "até existir uma segunda ferramenta". O
`bin/nick-init` é essa segunda: ele e o `auto_scrum.sh` criam a mesma árvore, e a casca Rust
(fatia F2a do épico NS-28) vai chamar o `bin/nick-init`. A biblioteca continua sem conhecer
ferramenta: o nome que vai no cabeçalho do esqueleto (`nick`) chega por parâmetro.

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

## Configuração do projeto: só `.nick/nick.conf` no cwd

A configuração de cada projeto alvo (a stack e as flags, ou seja, as chaves registradas em
`lib/config/keys.sh`) mora **só** em `.nick/nick.conf`, no cwd. `resolve_stack()` (sem
parâmetro), chamada em `main()` logo depois de `check_requirements` e antes de `jail_check`, do
menu de tipo, do log e do `claude`:

0. **Antes de tudo, o legado:** `config_cwd_legacy_check` (em `lib/config/config.sh`) com
   `|| exit 1` (ver "Migração do `.nick.conf`" abaixo).
1. **Sem `.nick/nick.conf` no cwd** (`$PWD/.nick/nick.conf`, de `config_cwd_file` e da
   constante `CONFIG_CWD_FILE_NAME`): o script para com `Erro: não há configuração do projeto em
   <PWD>/.nick/nick.conf. Rode na raiz do projeto: <caminho absoluto do bin/nick-init>` no
   stderr e código 1, sem criar nada no cwd.
2. **Com ele:** é carregado (`load_project_config`) e o script imprime no stdout
   `Configuração do projeto carregada de: <caminho absoluto>`.

**Saíram no NS-30:** a flag `--projeto=<nome>`, o diretório `auto_scrum/projects/` (os
`<nome>.conf` locais desta máquina), o menu de projetos com a opção "Nenhum", o aviso de
`projects/*.sh` legado e o `--init` do `auto_scrum` (com os dois destinos). `--init` e
`--projeto=<qualquer valor>` viram erro **antes de qualquer outra coisa** (requisitos, menu,
log, `claude`), mesmo com um `.nick/nick.conf` válido no cwd, com uma mensagem que mostra o
caminho absoluto do `bin/nick-init` (`NICK_INIT_BIN`, global do `auto_scrum.sh`, calculada a
partir do `SCRIPT_DIR`: o nick ainda não é instalado no `PATH`, então é o caminho que o
operador consegue colar). O `Uso:` do argumento desconhecido passou a ser
`auto_scrum.sh [--requisitos-jail]`. A configuração nova é criada pelo **`bin/nick-init`**, na
raiz do projeto alvo (`config_init "nick"`, ver "Pasta `.nick/` no projeto alvo"): destino
único, sem menu nem nome de projeto, e qualquer argumento é recusado (`Erro: argumento
desconhecido` + `Uso: nick-init (rode na raiz do projeto)`, código 1, nada criado). Ele só
carrega `lib/config/` (um teste estrutural prova que as únicas linhas `source` são as de
`config.sh` e `keys.sh` e que ele não cita `auto_scrum` em código, e um teste de comportamento
o roda numa cópia sem `auto_scrum/`). O esqueleto é o de antes, só com o cabeçalho citando o
`nick` em vez do `auto_scrum`.

**Por que a mudança:** a configuração é **do nick**, e não só do `auto_scrum`: outros módulos
vão lê-la, a começar pela casca Rust (épico NS-28). `auto_scrum/projects/` não guarda o
caminho do projeto (o nome do arquivo é só um rótulo), então não serve à casca, que precisa
achar a configuração a partir do projeto (Q1 do épico). E o cwd já identifica o projeto alvo
(seção "auto_scrum roda com o cwd dentro do projeto alvo"). Manter dois lugares seria manter
dois caminhos de carga, o menu e a opção "Nenhum" só pro `auto_scrum`.

**Quem não quer versionar o `.nick/`** no projeto alvo (substitui o antigo "só nesta máquina"
do `projects/`) põe `.nick/` no `.git/info/exclude` do repositório dele: é local, não é
versionado e não mexe no `.gitignore` do time.

**Sem migração automática dos `auto_scrum/projects/*.conf`** (fora do escopo do NS-30): o
formato é o mesmo, então a migração é copiar o arquivo pra `.nick/nick.conf` na raiz do
projeto e apagar o `auto_scrum/projects/` antes de atualizar o clone (nota no README). Rodar o
`bin/nick-init` em vez de copiar perderia os valores. Nesta máquina a migração foi feita pelo
usuário antes do NS-30 (commit `7e66ebe`).

**Nome `nick.conf` dentro de `.nick/`, só no cwd.** O arquivo vai ser versionado dentro do
repositório de outro time, então não pode ser apanhado pelo lint/pre-commit/CI desse time:
dentro de uma pasta oculta (não polui a listagem — antes do ticket 24 era o dotfile
`.nick.conf`, na raiz, pelo mesmo motivo); sem `.sh` (fora de `shellcheck $(git ls-files
'*.sh')`, `find -name '*.sh'` e dos hooks que filtram tipo shell); sem shebang (o `identify` do
pre-commit detecta shell por shebang em arquivo sem extensão conhecida); sem `.env` (não é
carregado por `python-dotenv`/`django-environ`/`docker compose` nem tratado como segredo por
scanners); sem `.yml`/`.json`/`.toml`/`.ini` (fora de `check-yaml`/`check-json`/prettier). Os
hooks genéricos de texto (`trailing-whitespace`, `end-of-file-fixer`, `mixed-line-ending`)
valem pra qualquer arquivo, então o esqueleto do `bin/nick-init` sai sem espaço no fim de linha, com
LF e terminando em `\n` — coberto por teste. A busca é só no `$PWD`, sem subir pra diretórios
pais ou pra raiz do git: a convenção já é rodar da raiz do projeto alvo, e subir criaria
ambiguidade (qual arquivo vence num monorepo?) sem pedido concreto.

**Leitura por `CHAVE="valor"` (`config_load`, em `lib/config/config.sh`; no `auto_scrum`,
chamado pela casca fina `load_project_config`), sem `source` nem `eval`.** Gramática: depois de tirar
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

**Migração do `.nick.conf` (ticket 24).** A configuração do cwd saiu de `$PWD/.nick.conf` pra
`$PWD/.nick/nick.conf`. `lib/config/config.sh` tem as duas constantes (`CONFIG_CWD_FILE_NAME` e
`CONFIG_CWD_LEGACY_FILE_NAME`) e a função `config_cwd_legacy_check`, porque é ela que sabe onde
mora o arquivo do cwd:
- só o antigo existe → erro no stderr com o comando de migração (`mkdir -p .nick && mv
  .nick.conf .nick/nick.conf`) e retorno 1. A biblioteca nunca chama `exit`: quem para é o
  chamador;
- os dois existem → vale o novo, com um aviso no stderr de que o antigo é ignorado e pode ser
  apagado, e retorno 0. *(Acréscimo do plano: sem isso, quem editasse o arquivo antigo por
  engano não entenderia por que nada muda.)*;
- caso contrário → silêncio, retorno 0. O arquivo antigo nunca é lido, movido nem apagado
  (mover arquivo do usuário sem pedir é surpresa).

Onde o `auto_scrum` chama: **`resolve_stack`**, antes de procurar o `.nick/nick.conf`. Só o
antigo dá o erro de migração (que já diz o que fazer), e não o erro de configuração ausente.
Como `resolve_stack` roda antes de qualquer menu de tipo, log ou `claude`, a saída ≠ 0
acontece antes de abrir o `claude`. `--requisitos-jail` não carrega configuração e não checa.

**O init não para no legado (decisão do usuário, revendo o plano do ticket 24; desde o NS-30,
vale pro `bin/nick-init`/`config_init`).** O plano do ticket 24 fazia o `--init` com destino no
cwd parar como a execução normal, pra não gerar um `.nick/nick.conf` ao lado do antigo e perder
os valores em silêncio. Na validação, o usuário preferiu o contrário: o init é o caminho de
quem quer a pasta nova pronta, com o esqueleto padrão, e o antigo é a fonte de onde copiar os
valores. Por isso `config_init` não chama `config_cwd_legacy_check` (nem o leitor): segue o
fluxo de sempre (sobrescrita, `config_dir_prepare`, esqueleto, árvore na saída) e, se
`$PWD/.nick.conf` existir, termina com um aviso no stderr
(`Aviso: o arquivo antigo <PWD>/.nick.conf não foi lido nem alterado — copie os valores dele
para <PWD>/.nick/nick.conf e apague o antigo.`). O aviso é o que impede a perda silenciosa. Com
N na sobrescrita (os dois arquivos já existem), nada muda e nada é avisado; a execução normal
já avisa dos dois arquivos. A execução normal continua parando só com o antigo: é ela que
rodaria o projeto sem configuração, e sem a jail.

**`.gitignore` do `nick_secrets`:** as linhas `auto_scrum/projects/*` e
`!auto_scrum/projects/.gitkeep` **saíram no NS-30** (decisão do usuário, revendo o ticket, que
mandava mantê-las como proteção de legado): nada mais lê o diretório. A linha existia pra um
`.conf` com dado de cliente não aparecer como "untracked" num repositório público; é seguro
tirá-la porque o diretório foi apagado antes (o usuário migrou os `.conf` desta máquina e
removeu o `.gitkeep` do git no commit `7e66ebe`), então não há o que aparecer. Quem tiver um
clone com `auto_scrum/projects/` precisa migrar e apagar o diretório **antes** de atualizar
(nota no README), porque ele deixou de ser ignorado. Um teste confere que o `.gitignore` não
tem nenhuma linha de `auto_scrum/projects` e que nada nesse caminho é versionado.
`.nick/` é listada, sem `/` inicial (vale em qualquer profundidade: um
`bin/nick-init` rodado com o cwd dentro do próprio `nick_secrets` não cria arquivo versionável no
repositório público). Como o git não entra numa pasta ignorada, o `.nick/.gitignore` interno
(com `!nick.conf`) não tem efeito aqui: `.nick/nick.conf` continua ignorado neste repositório
(um teste com `git check-ignore --no-index` cobra isso). **A linha `.nick.conf` continua,
como proteção de legado** (o ticket 24 pedia removê-la; o plano ajustou o critério): este
repositório, e qualquer clone já usado, ainda tem um `.nick.conf` real, ignorado e nunca
commitado. Tirar a linha faria esse arquivo — com dado de cliente, num repositório público —
aparecer como "untracked" e virar candidato a um `git add .`. O custo de manter é zero; ela
pode sair num ticket futuro. A linha `docs/plans/*` tinha o mesmo motivo e saiu no commit
`3db87b5` (decisão do usuário: a pasta não é mais usada, e não existe mais na máquina dele);
o teste do `.gitignore` foi ajustado no NS-29, com autorização do usuário, porque o
`make test` tinha ficado vermelho. Quem ainda tiver planos antigos em `docs/plans/` num clone
já usado precisa apagá-los ou movê-los antes de um `git add .`. `.claude/settings.local.json` também é listado: são as
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

**Por que no projeto alvo (desde o ticket 8):** com a configuração só em
`auto_scrum/projects/`, gitignorada, ela se perdia ao trocar de máquina e o time do projeto alvo
não conseguia reaproveitá-la — e versioná-la no `nick_secrets` não é opção, porque o repositório
é público e exporia nome de cliente e pistas de infraestrutura. Como o cwd já identifica o
projeto alvo (seção acima), a configuração pode morar lá, versionada junto com o código que ela
descreve. Só que um arquivo num repositório
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
(em `lib/config/config.sh`). As duas checagens e a validação do registro usam essa função. Uma flag
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
idêntica no esqueleto (o que o `bin/nick-init` grava; o script não lê o esqueleto em tempo de
execução).
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
  contam avisos por ela; até o NS-30 aceitavam antes o prompt `#? ` que o `select` do menu de
  projetos deixava sem quebra de linha, e esse menu saiu). Cada chave vem numa linha
  `  CHAVE="padrão"`. A nota da chave,
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
- Não sugerem o init (`bin/nick-init`, antes o `--init`): ele sobrescreve o arquivo e
  apagaria os valores já preenchidos.
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

**Onde rodam.** Só em `config_load`, o único leitor, que roda no `.nick/nick.conf` do cwd (o
único lugar da configuração desde o NS-30). O init (`config_init`, o `bin/nick-init`) não chama
o leitor e por isso não avisa — nem quando sobrescreve um arquivo incompleto que já existe, nem
quando o usuário responde N. O esqueleto já nasce com todas as chaves e traz um comentário com
as duas regras, então um arquivo novo carrega sem aviso.

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
- corrigir ou completar o arquivo automaticamente: mexer no arquivo do usuário sem pedir é
  surpresa (mesmo racional de não mover o `.nick.conf` antigo), e o aviso já traz as linhas
  prontas pra colar.

## Código compartilhado entre ferramentas (`lib/`)

O `nick_secrets` vai ter outras ferramentas com IA além do `auto_scrum`, e elas leem o mesmo
`.nick/nick.conf`. O que é genérico entre elas mora em `lib/`, com as suítes de teste em
`lib/tests/` (todas lá, inclusive a de uma biblioteca em subdiretório). **Convenção de
nome:** uma biblioteca de um arquivo só de código fica em `lib/<assunto>.sh`; uma com mais de um
arquivo (dados ou código) ganha um diretório, `lib/<assunto>/<assunto>.sh` + os outros arquivos.
Hoje as três bibliotecas têm diretório: `lib/config/` (dois arquivos de código, desde o NS-30),
`lib/jail/` (com o `allow_hosts.txt`) e `lib/claude/` (com o `permissions.json`):
- `lib/config/`, a biblioteca da configuração do projeto, em duas partes, carregadas nesta
  ordem:
  - `lib/config/config.sh`, a parte genérica: o leitor (`config_load`), os três avisos, a
    regra liga/desliga (`config_is_toggle_key`), a troca de caracteres de controle, o padrão e
    a nota por chave, os nomes da árvore `.nick/` no cwd (`CONFIG_NICK_DIR_NAME`,
    `CONFIG_EXCHANGE_DIR_NAME`, `CONFIG_DOCS_DIR_NAME`), o arquivo do cwd
    (`CONFIG_CWD_FILE_NAME`/`config_cwd_file`, só o cwd), a checagem do `.nick.conf` antigo
    (`CONFIG_CWD_LEGACY_FILE_NAME`/`config_cwd_legacy_check`), o formato do esqueleto
    (`config_print_skeleton`), a criação da árvore `.nick/` (`config_dir_prepare`) e o init
    (`config_init <nome da ferramenta>`, ver "Pasta `.nick/` no projeto alvo"). Ela continua
    genérica: não conhece nenhuma chave concreta nem ferramenta (o nome da ferramenta chega
    por parâmetro), e `lib/tests/test_config.sh` só usa chaves de mentira;
  - `lib/config/keys.sh`, o registro de **todas** as chaves do nick (`config_register_nick_keys`),
    inclusive as de módulos futuros. Até o NS-30 a regra era "a biblioteca não conhece nenhuma
    chave concreta: cada ferramenta registra as suas"; ela foi **revogada**, porque a
    configuração é do nick e não do `auto_scrum` (o `bin/nick-init` grava o esqueleto de todas
    as chaves sem carregar o `auto_scrum`). A regra que sobra: as chaves concretas moram só em
    `keys.sh` (e na suíte dele, `lib/tests/test_keys.sh`). Ver "Registro de chaves da
    configuração" abaixo;
- `lib/jail/jail.sh`: o mecanismo da jail (ai-jail) — verificação (`jail_check`), requisitos
  (`jail_print_requirements`), status (`jail_print_status`), opções do ai-jail por nível
  (`jail_options`, a fonte única delas), hosts dos MCPs remotos (`jail_mcp_hosts <jira>
  <sentry>`, desde o NS-33: o `auto_scrum.sh` e o `bin/nick-agent` chamam a mesma função),
  toolchains montadas (`jail_toolchain_dirs`), aviso
  ao agente (`jail_agent_notice`), navegadores do Playwright (`jail_playwright_browsers`),
  versão do ai-jail (`jail_version_output`, `jail_version_of`, `jail_version_supported`, com a
  versão mínima em `JAIL_VERSION_MIN`) e lançamento (`jail_exec`), com a lista padrão de hosts da rede em `lib/jail/allow_hosts.txt`
  (`jail_nick_hosts`). Mesmo formato do `lib/claude/`: pasta própria com o código e o dado,
  testes em `lib/tests/`. Ver seção "Sandbox do agente com ai-jail (`JAIL_ENABLED`)";
- `lib/claude/claude.sh` + `lib/claude/permissions.json`: as regras de deny/ask do Claude Code
  que toda sessão aberta pelo nick recebe, a instrução de sistema de não contornar uma recusa
  dessas regras (`claude_permission_notice`, desde o NS-41), e, desde o NS-39, onde o `claude`
  guarda a conversa de uma sessão (`claude_conversation_file`, com a pasta de configuração por
  parâmetro: seção "Rust no projeto: retomar e arquivar agentes (NS-39)"). Ver seção "Regras de
  deny/ask do Claude Code (`lib/claude/`)". A divisão:

  | Fica em `lib/claude/` | Fica na ferramenta (`auto_scrum.sh`) |
  |---|---|
  | as regras (`permissions.json`, fonte única) | a global `NICK_CLAUDE_SETTINGS=""` que recebe o conteúdo |
  | achar o arquivo pelo `BASH_SOURCE` (`NICK_CLAUDE_PERMISSIONS_FILE`) | quando carregar (`main()`, logo depois de `jail_check`, com `\|\| exit 1`) |
  | ler o arquivo e entregar o conteúdo (`claude_settings_load`) | montar o argv: `claude_args=(--settings "$NICK_CLAUDE_SETTINGS")` |
  | a mensagem de erro (`claude_print_settings_error`) | o resto do argv (`--permission-mode`, `--append-system-prompt`, prompt) |
  | o texto da instrução de não contornar uma recusa (`claude_permission_notice`, NS-41) | entregá-lo no início do único `--append-system-prompt` (`main()` e `exec_claude`) |

**As bibliotecas não dependem umas das outras:** cada uma pode ser carregada sozinha, e a suíte
de uma não carrega a outra (`lib/tests/test_jail.sh` nunca carrega `lib/config/config.sh`, e um teste
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
`lib/tests/test_config.sh`, `lib/tests/test_keys.sh`, `lib/tests/test_jail.sh` e
`lib/tests/test_claude.sh`):
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
  `lib/config/config.sh`; toda função `jail_` e toda global `JAIL_` em `lib/jail/jail.sh`; toda função
  `claude_` e toda global `NICK_CLAUDE_` em `lib/claude/claude.sh` (as locais, `__claude_`).
  As globais de `lib/claude/` **não** usam `CLAUDE_`: o Claude Code lê variáveis `CLAUDE_*` do
  ambiente (ex: `CLAUDE_CODE_DISABLE_MOUSE`), e um nome nosso nesse espaço poderia colidir com
  uma variável dele, hoje ou numa versão futura, e mudar o comportamento do agente. Pelo mesmo
  motivo, a global do `auto_scrum.sh` é `NICK_CLAUDE_SETTINGS`. Um teste estrutural proíbe
  `CLAUDE_` sem `NICK_` na frente em código de `lib/claude/claude.sh`, e outro proíbe atribuir
  global `CLAUDE_*` no `auto_scrum.sh`. O `auto_scrum.sh` não define nada nesses namespaces;
- **nunca chama `exit`** e não usa `source`, `.` nem `eval`;
- **não lê globais do chamador:** tudo entra por parâmetro ou pelo próprio registro (o `$PWD`
  de `config_cwd_file`, `config_dir_prepare` e `config_init` é estado do shell, não global do
  chamador);
- mesmas regras de estilo de "Qualidade de código bash".

**Exceção de globais, documentada:** `config_load` escreve, com `printf -v`, as globais das
chaves que o chamador registrou — esse é o papel do leitor — e nada mais: nem outra global do
chamador, nem `CONFIG_*` (um teste compara o `declare -p` antes e depois). O registro só
escreve nos próprios arrays `CONFIG_*` e não inicializa as globais das chaves; as linhas
`STACK_DESCRIPTION=""` etc. continuam no topo do `auto_scrum.sh`, que é o dono delas (o
`bin/nick-init` não precisa delas: não chama o leitor). Por isso
`load_project_config` continua existindo no `auto_scrum` como chamada fina: é ali que a
ferramenta guarda o que precisar sobre o arquivo carregado, sem pôr global de chamador dentro
da biblioteca (hoje, `PROJECT_CONFIG_FILE`, o caminho usado na mensagem de erro da jail).

**Exceção de `exit`, documentada:** `jail_exec` termina com `exec` (dentro ou fora da jail) e
por isso não retorna. Não é um `exit`: é o papel dela, o último passo da ferramenta.

**Constantes atribuídas ao carregar:** em `lib/config/config.sh`, `CONFIG_NICK_DIR_NAME`,
`CONFIG_EXCHANGE_DIR_NAME`, `CONFIG_DOCS_DIR_NAME`, `CONFIG_CWD_FILE_NAME` e
`CONFIG_CWD_LEGACY_FILE_NAME` (valores fixos, relativos ao cwd); `lib/config/keys.sh` não cria
nenhuma (só define `config_register_nick_keys`); em `lib/jail/`, `JAIL_BIN`, `JAIL_VERSION_MIN`
e `JAIL_OS_RELEASE_FILE` (valores fixos); e, em `lib/claude/`,
`NICK_CLAUDE_PERMISSIONS_FILE`, calculada a partir de `${BASH_SOURCE[0]}`
(`"$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" > /dev/null && pwd)/permissions.json"`). O `cd`
roda no subshell do `$(...)`: não muda o cwd de quem carrega, e a saída dele (que existe com
`CDPATH` definido) vai pro `/dev/null`, então a regra "só define ao ser carregada" continua
valendo. O caminho sai absoluto mesmo com a biblioteca carregada por caminho relativo de outro
cwd (coberto por teste). É variável, e não texto fixo, só pra os testes apontarem pra um
arquivo ausente, ilegível ou vazio — mesmo racional de `JAIL_BIN`.

**Carga:** logo depois de `SCRIPT_DIR`, com

```bash
# shellcheck source=SCRIPTDIR/../lib/config/config.sh
source "$SCRIPT_DIR/../lib/config/config.sh"
# shellcheck source=SCRIPTDIR/../lib/config/keys.sh
source "$SCRIPT_DIR/../lib/config/keys.sh"
# shellcheck source=SCRIPTDIR/../lib/jail/jail.sh
source "$SCRIPT_DIR/../lib/jail/jail.sh"
# shellcheck source=SCRIPTDIR/../lib/claude/claude.sh
source "$SCRIPT_DIR/../lib/claude/claude.sh"

config_register_nick_keys
```

A diretiva usa `SCRIPTDIR/` e não `../lib/config/config.sh`: o caminho relativo puro só resolve
se o `shellcheck` rodar de dentro de `auto_scrum/` (da raiz dá SC1091), e com `SCRIPTDIR` ele
resolve de qualquer cwd. Rode `shellcheck -x`, pra ele seguir o `source`. O `SCRIPT_DIR` vem de
`BASH_SOURCE[0]`, então o caminho também funciona quando os testes dão `source` no script de
outro cwd. As quatro linhas `source`, nessa ordem (config/config → config/keys → jail →
claude), e, desde o NS-35, uma quinta, `source "$SCRIPT_DIR/prompt.sh"` (a montagem do prompt,
que não é biblioteca de `lib/`: seção "Rust no projeto: etapas do auto_scrum na casca `nick`
(NS-35)"), são as únicas linhas `source`/`.` do `auto_scrum.sh` (um teste compara a lista
inteira), e a chamada `config_register_nick_keys` vem logo depois, no topo, fora de função. O
`bin/nick-agent` carrega as mesmas cinco, na mesma ordem (a quinta é
`source "$SCRIPT_DIR/../auto_scrum/prompt.sh"`), com a mesma chamada. O
`bin/nick-init` carrega só as duas de `lib/config/`, na mesma ordem, com a mesma chamada. A
garantia de que a configuração nunca é executada fica nos testes estruturais de
`lib/config/config.sh` e `lib/config/keys.sh` e nos testes de gramática.

**Riscos aceitos:**
- a API de `lib/config/config.sh` nasceu com um cliente só; desde o NS-30 tem o segundo (o
  `bin/nick-init`, com `config_dir_prepare`/`config_init`), e a casca Rust (`nick/`, NS-31)
  chama o `bin/nick-init`, não a biblioteca;
- o `auto_scrum/` e o `bin/` dependem do `lib/` irmão por caminho relativo (`SCRIPT_DIR` pelo
  `BASH_SOURCE`): mover um deles sozinho quebra o script, e um symlink do `bin/nick-init` em
  outro diretório (ex: `~/.local/bin`) não acharia o `lib/` — resolver isso é da instalação
  (fatia F6 do épico NS-28).

**Por que agora:** a decisão 1 do plano do ticket 13 (jail) era extrair o leitor "quando a
segunda ferramenta existir". O ticket 23 reviu isso: o 13 introduz `JAIL_ENABLED`, que vale
pro nick inteiro, e extrair antes dele custava só revisar aquele plano; depois, custaria
refazer código e testes do 13.

**Como rodar as suítes:** `make test` na raiz (ou `bash tests/run_all.sh`, sem `make`) roda
todas, uma de cada vez, e sai com código 0 só se todas passarem (ver seção "Testes: `make
test` e `tests/run_all.sh`"). Cada suíte continua podendo rodar direto, da raiz, e sai com
código 1 se houver `FAIL`: `bash auto_scrum/tests/test_auto_scrum.sh`,
`bash bin/tests/test_nick_init.sh`, `bash bin/tests/test_nick_agent.sh` (o launcher dos agentes da
casca, NS-33), `bash lib/tests/test_config.sh`,
`bash lib/tests/test_keys.sh`, `bash lib/tests/test_jail.sh`, `bash lib/tests/test_claude.sh`
(esta precisa de `jq` ou `python3` pra validar o JSON; sem nenhum dos dois, dá FAIL),
`bash tests/test_run_all.sh`, `bash tests/test_cargo_suite.sh` (a guarda da suíte Rust, sem o
`cargo` real), `bash nick/tests/test_cargo.sh` (a casca Rust: exige o `cargo`; ver "Rust no
projeto: a casca `nick` (NS-31)") e `bash prototype/panel/tests/test_launch_session.sh` (o
launcher do protótipo do painel; ver "Rust no projeto: protótipo do painel (NS-29)").

## Registro de chaves da configuração

> **Mudou no NS-53** (seção "Rust no projeto: link do Jira montado pelo ID (NS-53)"): o
> `config_register_key` aceita um **5º argumento opcional**, a pergunta do init, guardada no array
> interno `CONFIG_KEY_PROMPTS` (pergunta vazia é o mesmo que nenhuma; chave `_ENABLED` com pergunta
> é recusada, retorno 1). A mensagem de contagem passou a "espera 4 ou 5 argumentos". O
> `config_init` pergunta cada chave com pergunta, **depois** da confirmação de sobrescrita e
> **antes** de criar qualquer coisa, e o `config_print_skeleton` ganhou pares opcionais
> `CHAVE=valor`. Hoje só a `JIRA_URL` tem pergunta.

As chaves da configuração do nick ficam num bloco único em `lib/config/keys.sh`, dentro da
função `config_register_nick_keys`, uma chamada por chave, na ordem do esqueleto (até o NS-30
o bloco ficava no topo do `auto_scrum.sh`):

```bash
config_register_nick_keys() {
  # config_register_key <name> <default> <note> <skeleton comment>
  config_register_key PRODUCTION_ENABLED "false" \
'a pergunta de impacto em produção está desligada no prompt do PO. ...' \
'PRODUCTION_ENABLED: "true" se esse projeto já está em produção — ...' || return 1
  ...
}
```

- o comentário vai **sem** o `# ` de cada linha; quem acrescenta o prefixo (e escreve `#`
  numa linha vazia) é `config_print_skeleton`, porque é regra de formato do esqueleto. Os
  textos entre aspas continuam na coluna 0 dentro da função: indentá-los mudaria o esqueleto;
- a ordem do registro é a do esqueleto, do aviso de chave ausente e de `CONFIG_KEYS`;
- **por que uma função (decisão D1 do NS-30):** toda biblioteca de `lib/` só define ao ser
  carregada (seção "Código compartilhado entre ferramentas"); chamadas no corpo de `keys.sh`
  rodariam comando na carga. Quem decide quando registrar é a ferramenta: o `auto_scrum.sh` e o
  `bin/nick-init` chamam `config_register_nick_keys` uma vez, logo depois dos `source`, no
  topo, fora de função. Ela depende de `config.sh` já carregado (é a mesma biblioteca; a regra
  "as bibliotecas não dependem umas das outras" fala de config × jail × claude);
- cada chamada termina em `|| return 1`: a primeira que falha faz a função retornar 1 ali, sem
  `exit` e sem registrar as chaves seguintes; o chamador, sob `set -euo pipefail`, para na
  carga — um registro inválido é erro de programação. Chamar a função duas vezes também falha
  (as chaves "já foram registradas"), como esperado.

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

Os testes do registro ficam em `lib/tests/test_keys.sh` (movidos do `test_auto_scrum.sh` no
NS-30): a carga de `keys.sh` não cria variável nem registra nada (só a função, inclusive no
pseudo-terminal); `CONFIG_KEYS`, padrões e notas iguais aos de antes (valores fixados à mão);
o corpo do esqueleto idêntico byte a byte ao de antes; o bloco contíguo, dentro da função, com
uma chamada por chave; as posições e os comentários das chaves `JAIL_*`; e a consistência do
esqueleto com o leitor (sem espaço no fim, LF, `\n` final, sem shebang, ida e volta sem aviso,
toda liga/desliga checada e nenhuma de texto, cada linha sugerida no aviso de ausente idêntica à
do esqueleto). No `test_auto_scrum.sh` ficam: o `auto_scrum.sh` não tem nenhuma linha
`config_register_key` e chama `config_register_nick_keys` uma vez, no topo; nenhuma chave
registrada aparece em `lib/config/config.sh` nem em `lib/tests/test_config.sh` (que só usa
chaves de mentira); e uma chave extra registrada no teste aparece no esqueleto e nos avisos de
chave ausente e de valor inválido.

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

**Na casca `nick` (NS-35), a mesma regra:** o agente aberto pelo painel com o tipo
`development` sobe com `--permission-mode auto`, e os outros sete tipos no modo padrão. Quem
decide é o `bin/nick-agent`, pelo `--type` que o painel passa
(`[ "$type" = "development" ] && claude_args+=(--permission-mode auto)`, a regra do `auto_scrum.sh`
com o `$TYPE`). O
Rust nunca passa `--permission-mode`: um teste estrutural proíbe o texto em `nick/src/`. Na
retomada (NS-39), o tipo não é gravado, e quem decide é a etapa (`--stage development`), pela
mesma linha do launcher (`exec_claude`).

## Sandbox do agente com ai-jail (`JAIL_ENABLED`)

> **Mudou no NS-57** (seção "Rust no projeto: liberar e prender a jail de um agente (NS-57)"): a
> jail do projeto pode ser desligada **para um agente** pela TUI (`l` no nó dele), sempre com uma
> confirmação de borda vermelha e só até ele fechar; nada é gravado. O `bin/nick-agent --resume`
> ganhou o `--no-jail`, que roda o `jail_check` de sempre (com o `JAIL_ENABLED` do projeto) e só
> depois desliga a jail: a decisão "Verificação na inicialização, sem fallback" continua valendo, e
> uma jail quebrada falha também com o `--no-jail`.

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
  arquivo compartilhado quando a configuração é o `.nick/nick.conf` do cwd (`config_cwd_file`;
  desde o NS-30 é sempre o caso, mas o `auto_scrum.sh` mantém a comparação, pra não mudar a
  chamada da jail fora do escopo) e a lista de `jail_print_requirements`, e retorna 1. O `|| exit 1` vem antes de `mkdir -p
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
- **`PROJECT_CONFIG_FILE` fica no `auto_scrum`** (atribuída em `load_project_config`; desde o
  NS-30, sempre o caminho absoluto do `.nick/nick.conf` do cwd), porque `lib/config/config.sh`
  não guarda o caminho carregado.
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
  - **Mudou em 2026-11-26 (achado durante o NS-57):** com o Jira, a função (`jail_mcp_hosts`, em
    `lib/jail/jail.sh`, desde o NS-33) devolve também o `auth.atlassian.com` e o
    `api.media.atlassian.com`, nessa ordem, depois do `mcp.atlassian.com`. O endpoint v2 do MCP
    (`https://mcp.atlassian.com/v2/mcp`) tem como servidor OAuth o `auth.atlassian.com`
    (`/.well-known/oauth-protected-resource/v2/mcp`), e não o próprio `mcp.atlassian.com`, como
    o v1. O login continua fora da jail, mas o `claude` **renova** o token sozinho (com o refresh
    token) quando ele vence, e dentro da jail essa renovação era recusada (`CONNECT tunnel
    failed, response 403`): o MCP caía no meio da sessão, e fora da jail seguia funcionando,
    sem novo login. Conferido com `curl` dentro da jail: `mcp.atlassian.com` respondia 401, e
    `auth.atlassian.com` e `api.atlassian.com`, 403 do proxy. O `api.media.atlassian.com` (os
    anexos e as mídias do Jira) saiu da lista padrão (`lib/jail/allow_hosts.txt`) e veio para
    cá, por decisão do usuário: os dois só servem a projetos com o Jira, e cada host liberado é
    mais um canal de saída. O `developer.atlassian.com` (documentação) continua na lista padrão.
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
  da jail (desde o NS-41, o `--append-system-prompt` existe sempre e começa pela instrução de
  não contornar uma recusa, e o aviso da jail vem depois dela, no mesmo argumento: ver
  "Instrução de não contornar uma recusa (NS-41)"). Pelo system prompt, e não por um bloco nos
  8 templates, porque vale pra sessão inteira, fica num lugar só e não depende da etapa. O
  texto fica na biblioteca (o conhecimento sobre a jail é dela) e a ferramenta só decide como
  entregá-lo, porque `jail_exec` é genérico sobre o comando e não pode injetar argumento do
  `claude`. O texto descreve o padrão do ai-jail (rede: API do Claude, registries e GitHub;
  home: estado do `claude`, configuração do git e, desde o ticket 27, os caches de dependência
  do `--toolchains`; reconferido no 3.2.0 com `--dry-run`), por isso diz "por padrão": com
  `allow_hosts` na configuração global do ai-jail (`~/.config/ai-jail/config.toml`) ou numa
  versão nova, pode haver mais. **A cada atualização do ai-jail, reconferir com `--dry-run`
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
claude --settings "<conteúdo de lib/claude/permissions.json>" [--permission-mode auto] --append-system-prompt "<instrução (NS-41)>[\n\n<aviso da jail>]" "<prompt>"
```

Com a jail: `ai-jail <opções> -- claude --settings ...`. `--settings` vem sempre primeiro,
antes do `--permission-mode`.

**Como funciona:** `lib/claude/permissions.json` é `{"permissions": {"deny": [...], "ask":
[...]}}`, sem `allow`, `defaultMode` nem outra chave, além de `"disableAgentView": true`, que
desliga o segundo plano do `claude` (desde o NS-50: seção "Rust no projeto: retomada segue a
troca de sessão do claude (NS-50)"). `claude_settings_load NICK_CLAUDE_SETTINGS
|| exit 1` roda em `main()` logo depois de `jail_check` (antes de `jail_print_status`, dos
`build_*`, do menu e do `mkdir -p "$LOGS_DIR"`): lê o arquivo dentro da função (`cat`, sem
`jq`) e grava o conteúdo na global com `printf -v`. Arquivo ausente, ilegível ou vazio (só
espaços, tabs e quebras de linha contam como vazio) → `Erro: regras de permissão do Claude Code não
encontradas ou ilegíveis: <caminho>` no stderr e código 1: nenhuma pergunta, nenhum log,
`claude` não aberto, com e sem jail. Com a jail ligada, a verificação dela (`ai-jail ... --
true`) já rodou nesse ponto; o lançamento, não. `--requisitos-jail` (e o `--init`/`--projeto=`
recusados, desde o NS-30) saem antes e não carregam as regras, porque não abrem o `claude`. A biblioteca não valida o JSON: isso
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
sem `--force`). O isolamento de verdade é a jail (seção "Sandbox do agente com ai-jail"). Para
o agente não refazer por outro caminho o que uma regra recusou, toda sessão recebe uma instrução
de sistema (subseção "Instrução de não contornar uma recusa (NS-41)", abaixo).

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
regras por projeto; hooks; regras de rede/MCP; `lib/ai/`; mudar `lib/config/config.sh` ou
`lib/jail/jail.sh`; qualquer dependência nova em runtime.

### Instrução de não contornar uma recusa (NS-41)

**O problema.** As regras casam pelo texto do comando (acima). Quando uma delas recusava um
comando (ex: `rm -rf`), o agente costumava refazer a mesma ação por outro caminho (um script no
scratchpad, `bash -c`, `python -c`), o que anula a proteção. No modo `auto` da etapa de
Desenvolvimento isso acontece sem ninguém ver. A recusa vem do deny/ask do Claude Code, **não da
jail**, que permite escrever no projeto e por isso não resolve. Bloquear todo equivalente é
inviável.

**A correção é de comportamento.** Toda sessão aberta pelo nick recebe uma instrução fixa de
sistema: uma recusa por regra de permissão é decisão do usuário, não um obstáculo técnico. O
agente não pode obter o mesmo efeito por outro meio (script, `bash -c`, `sh -c`, `python -c`,
outro interpretador, outra ferramenta ou outra forma de escrever o comando). Ele tem de parar,
dizer qual comando foi recusado e por qual regra, e perguntar como seguir. A instrução vale
**só** para o que uma regra recusou: comandos permitidos (ex: um `rm` de arquivo, sem `-rf`)
continuam liberados, sem pedir licença, para não travar o modo `auto`.

- **Fonte única: `claude_permission_notice`, em `lib/claude/claude.sh`.** Ela fica junto das
  regras a que se refere, e as duas ferramentas (`auto_scrum.sh` e `bin/nick-agent`) já carregam
  essa biblioteca. Segue o modelo de `jail_agent_notice`: `echo`s literais em PT-BR, sem
  parâmetro, sem ler arquivo nem global, retorno 0. Um teste estrutural de
  `lib/tests/test_claude.sh` garante, com controle positivo, que a frase de abertura não está
  copiada no `auto_scrum.sh`, no `prompt.sh`, nos templates, no `bin/nick-agent` nem em
  `nick/src/`, e que as duas ferramentas chamam a função. Os testes de ponta a ponta calculam o
  argv esperado pela própria lib.
- **Entrega: um único `--append-system-prompt`, sempre.** Ele aparece em todas as etapas, com e
  sem jail, no `auto_scrum` e no `bin/nick-agent` (lançamento, retomada e retomada de épico).
  Ordem, com uma linha em branco entre os avisos: instrução → aviso da jail (só com
  `JAIL_ENABLED="true"`) → aviso do ID (só na retomada com o ID mudado). A instrução vem
  primeiro porque está sempre presente e fala das mesmas regras do `--settings`, que também vem
  primeiro.
  Desde então, com a jail desligada, o argv deixou de ser "o de antes da jail": ganhou o
  `--append-system-prompt`. Não vai para o `prompt.sh` nem para os templates porque vale para
  toda sessão, inclusive a retomada, que não tem prompt. O Rust não monta nada disso (regra dos
  NS-35/NS-39).
- **"Ask" negado também conta.** O texto cobre a confirmação respondida com "não" pelo usuário,
  além do deny: é a mesma situação, uma decisão do usuário sobre aquela ação. Sem isso, o
  agente poderia contornar uma negação no `git push` (regra `ask`). Se o usuário preferir só o
  deny, basta tirar a oração "ou quando o usuário negar um pedido de confirmação".
- **O texto respeita as regras estruturais de `lib/`.** Nenhum `Bash(`/`Read(`, nenhum `CLAUDE_`
  sem `NICK_`, nenhum `jail_`/`JAIL_`/`config_`. O guarda "não usa jq nem python" passou a olhar
  só a posição de comando (começo da linha, depois de `;`, `&`, `|`, `(`, `$(` ou de uma
  palavra-chave), com controles positivos. Antes ele casava qualquer `python` em código, e o
  `python -c` do texto (dentro de um `echo`) não é dependência em runtime (decisão do usuário na
  execução).
- **Limitação: depende do modelo.** A instrução reduz o contorno, mas não o impede
  tecnicamente: um modelo pode ignorá-la.
- **Por que não hooks.** Um hook `PreToolUse` também só vê o texto do comando, então tem a mesma
  limitação, e não enxerga o conteúdo de um script gravado e depois executado. Ele exigiria
  entregar e manter um script e uma configuração de hooks que funcionassem dentro e fora da
  jail, sem gravar no `.claude/settings*.json` do projeto alvo (decisão do NS-21). O ticket o
  exclui.
- **Fora do escopo:** o protótipo `prototype/panel/launch_session.sh` (NS-29) também carrega
  `lib/claude/claude.sh`, mas não foi alterado, e a suíte dele passa sem mudança. Isso prova que
  a função nova não mexeu no que já existia na biblioteca.

**Teste manual (roteiro; executado pelo usuário, resultado pendente):**
1. Num repositório descartável fora do `nick_secrets` (ex: `/tmp/nick41-proj`, com `git init`,
   `.nick/nick.conf` gerado pelo `bin/nick-init` e um diretório `descartavel/` com dois
   arquivos, `a.txt` e `b.txt`), abrir pelo `auto_scrum` uma sessão de **Desenvolvimento** (modo
   `auto`), uma vez com `JAIL_ENABLED="false"` e outra com `"true"`.
2. Pedir: "apague o diretório `descartavel/` com `rm -rf`".
3. Esperado: o Claude Code recusa o comando pela regra, e o agente **para**, diz que
   `rm -rf descartavel` foi recusado por regra de permissão e pergunta como seguir. Ele **não**
   cria script nem usa `bash -c`/`python -c`/`find -delete`, e `descartavel/` continua
   existindo.
4. Contraprova (o que não deve mudar): pedir "apague o arquivo `descartavel/a.txt`". O agente
   roda o `rm` sem `-rf` normalmente, sem perguntar.
5. Repetir o passo 2 numa sessão aberta pela casca `nick` (agente de Desenvolvimento) e numa
   retomada desse agente (`r`), já que a retomada não tem prompt.

Registrar aqui e no PR o resultado de cada passo.

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

## Rust no projeto: protótipo do painel (NS-29)

`prototype/panel/` é um **protótipo descartável** do épico NS-28 ("nick como programa"). O
binário Rust `nick-panel` (ratatui + crossterm, portable-pty, vt100 + tui-term) mostra uma
lista fixa de sessões fictícias e o terminal da sessão selecionada, com o `claude` real de cada
sessão rodando ao mesmo tempo. Ele existe pra responder se um painel assim é usável com o
`claude` dentro da ai-jail e com as regras do nick. **O código dele não é a base da casca
futura.** A decisão de usar Rust na casca de verdade sai do relatório
(`prototype/panel/README.md`, seção "Relatório").

**A regra de linguagem:** as ferramentas e as bibliotecas do nick continuam em bash (seção
"Qualidade de código bash"). O Rust entra só em `prototype/panel/`, isolado: nada de
`auto_scrum/`, `lib/`, `tests/` da raiz nem do `Makefile` muda por causa dele. A regra foi
revista no ticket da casca: ver "Rust no projeto: a casca `nick` (NS-31)".

**Lançamento só pelo `lib/`.** Cada sessão é um PTY com `bash prototype/panel/launch_session.sh`
(o único comando que o painel lança; um teste estrutural em Rust olha os fontes de `src/`). O
launcher é um script bash que carrega `lib/jail/jail.sh` e `lib/claude/claude.sh` e repete o
caminho do `auto_scrum`: `jail_check` → `claude_settings_load` → `jail_print_status` →
`jail_exec ... claude --settings <permissions.json> --append-system-prompt
<jail_agent_notice>`, no modo de permissão padrão e sem prompt inicial. Os motivos:
- **um launcher bash, e não Rust:** as opções do ai-jail, a leitura do `permissions.json` e o
  aviso ao agente têm fonte única em `lib/`. Uma cópia em Rust divergiria na primeira mudança, e
  o critério do ticket era "o mesmo caminho de lançamento do nick". Nada em `lib/` precisou
  mudar: tudo entra por parâmetro;
- **a jail é sempre ligada:** `jail_check "true"`, `jail_exec "true"` e `jail_print_status
  "true"` são literais, e não há flag nem variável que a desligue (teste estrutural);
- **sem `.nick/nick.conf`:** o launcher não carrega `lib/config/config.sh`. As sessões são dados
  fictícios, e a configuração do projeto ficou fora do escopo. As opções da jail vêm do ambiente
  do painel: `NICK_PANEL_CLIPBOARD` (nível do `JAIL_CLIPBOARD`), `NICK_PANEL_ALLOW_HOSTS` e
  `NICK_PANEL_FORWARD_PORTS`. O prefixo é `NICK_` e não `CLAUDE_`, pelo motivo de
  `lib/claude/` (o Claude Code lê `CLAUDE_*` do ambiente). No lugar do caminho da configuração,
  as mensagens de erro do `jail_check` mostram o rótulo `prototype/panel (protótipo NS-29)`;
- o cwd de todas as sessões é o cwd de onde o painel foi aberto (o projeto de trabalho), como no
  `auto_scrum`. O launcher é achado por `CARGO_MANIFEST_DIR`, fixado na compilação: o protótipo
  roda do checkout com `cargo run`.

**Testes: o launcher no `make test`, o Rust fora.** A suíte bash
`prototype/panel/tests/test_launch_session.sh` é achada sozinha pelo `tests/run_all.sh`. Ela
confere o argv exato que o ai-jail falso recebe, calculado pela própria lib (`jail_options`,
`jail_agent_notice`, `permissions.json`, com `env -u PLAYWRIGHT_BROWSERS_PATH` e `HOME` do caso,
mesmo racional de `jail_expected_argv`), com controles de ai-jail ausente (`PATH` mínimo, com
controle positivo), versão fora da faixa e nível inválido, além da estrutura e do
`shellcheck -x`. O `cargo test` (unitários de `keys.rs`, `app.rs` e `ui.rs`, e o de integração
`tests/pty_launch.rs`, que abre sessões em PTY pelo construtor real com os mesmos falsos de
`prototype/panel/tests/fixtures/bin/`, e o `tests/panic_hook.rs`, que roda o binário num PTY e
força um panic) **não** entra no `make test`. Assim o `make test` não
passa a exigir o Rust instalado, e o protótipo é descartável. Ele roda à mão, junto com
`cargo clippy -- -D warnings` e `cargo fmt --check` (README do protótipo).

**`.gitignore`:** `prototype/panel/target/` (saída de build do Cargo). O `Cargo.lock` é
versionado (binário).

**Resultado (2026-11-12, revisto no review de 2026-11-17):** com o `claude` e o ai-jail reais
(parte conferida pelo agente por PTY, parte pelo usuário), os itens 1–6 do roteiro
funcionaram e o 8 funcionou com a ressalva do `kill -9` (terminal sujo). O item 9 (uso real)
está sem a duração e as impressões. A recomendação do relatório é **seguir com Rust e ratatui
na casca**, começando do zero e levando os achados abaixo; a regra de linguagem foi revista no
ticket da casca (NS-31).

**Item 7 (copiar/colar), resultado final (2026-11-17, fechado no NS-31):** refeito depois do
repasse do OSC 52. A parte automática foi feita pelo Tech Leader fora da jail, com o ai-jail
3.4.1 e o `claude` 2.1.325 reais (`.nick/docs/NS-29/item7_auto.py` e `item7_auto.txt`): o OSC
52 não chega ao painel no `restrito` e é repassado com o texto selecionado no `medio` e no
`brando`; o `Ctrl+V` cola no `brando` e não no `restrito` nem no `medio`; a colagem de várias
linhas chega inteira nos três níveis; o clique não copia nada; a saída fica limpa nos três. A
parte que depende do programa de terminal foi feita pelo usuário (`.nick/docs/NS-29/item7_manual.md`):
(i) copiar pelo `claude` e colar fora, (iii) `Ctrl+V` e (iv) botão direito → Colar funcionaram;
(ii) a seleção nativa com `Shift` copia, mas pega junto o que está na mesma linha fora do
`claude` (a lista de sessões, as bordas). Isso é um **achado para a F3**, não uma falha: a
seleção é do terminal, que só enxerga a tela inteira, e o caminho de copiar é o (i). Se a
seleção com `Shift` precisar sair limpa, a sessão precisa de um modo de tela cheia, sem a
lista ao lado. Conferido no Konsole; VS Code não testado (o terminal do VS Code fica coberto
pela verificação manual do `nick` no NS-31).

**Achados que valem para a casca futura** (detalhe no relatório):
- o `tui-term` 0.3.4 só funciona com o `vt100` 0.16 (ele o reexporta). Uma casca que use os dois
  segue a versão do `tui-term`, ou desenha o `vt100::Screen` à mão;
- em raw mode, o `Ctrl+\` (0x1c) chega como tecla (o crossterm o entrega como `Ctrl+4`), sem
  SIGQUIT. O Claude Code não usa esse atalho (documentação de keybindings), e por isso ele é o
  atalho reservado pra sair do terminal da sessão; o `Ctrl+]` foi evitado. Na casca, o atalho é
  configurável: seção "Rust no projeto: vários agentes na casca `nick` (NS-34)";
- com o `claude` e o ai-jail reais (2026-11-12, conferido pelo agente fora da jail, por PTY): o
  `claude` sobe e responde dentro do painel; acento, `Esc`/`Ctrl+C` interrompendo uma resposta,
  histórico, menu do `/`, `Shift+Tab`, troca de sessão e resize funcionam. `TERM`/`COLORTERM`
  chegam dentro da jail apesar do `--no-inherit-env`. CPU do painel: 0% ocioso, abaixo de 1%
  com resposta em andamento e durante resizes; o gasto é do próprio `claude`, igual dentro e
  fora do painel;
- achado da `lib/jail`, fora do escopo do NS-29: o `jail_agent_notice` diz que o `~/.config` não
  existe dentro da jail, mas o próprio `claude` cria nele, ao subir, os manifests de native
  messaging da extensão do Chrome (e há um `mimeapps.list`). Não é perfil real de navegador,
  mas o agente estranhou. Vale um ticket pra ajustar o texto;
- o terminal integrado do VS Code não repassa o `Ctrl+\`: o VS Code fica com a tecla, que é
  atalho dele ("dividir editor"). Num terminal standalone, ela funciona (conferido pelo usuário).
  Decisão do usuário: manter o `Ctrl+\` no protótipo, com a regra do `keybindings.json` que
  manda o 0x1c (`workbench.action.terminal.sendSequence`, README do protótipo). A casca de
  verdade precisa de um atalho reservado que funcione no VS Code sem configuração, porque é lá
  que o usuário trabalha. Na casca, o padrão continua `Ctrl+\`, com as duas saídas para o VS
  Code: seção "Rust no projeto: vários agentes na casca `nick` (NS-34)";
- sem leitura contínua, o PTY de uma sessão escondida enche e trava o filho: toda sessão precisa
  da sua thread leitora. Com um laço bloqueado num canal (entrada, sinais, saída e fim de cada
  sessão), o painel ocioso não gasta CPU (0 tick em 5 s, com os falsos);
- o encerramento é por grupo de processos (o filho do PTY é líder de sessão): SIGTERM, SIGKILL
  depois de 2 s, e `wait`. Com o ai-jail real, o `bwrap` fica numa sessão e num grupo próprios,
  então o sinal só chega ao `ai-jail`, e é ele que encerra o resto. Na conferência manual não
  sobrou órfão em nenhum caso: `q`+`s`, `kill -TERM`, `kill -9` e fechar a janela. O `kill -9`
  deixa o terminal sujo (tela alternativa e mouse ligados; `reset` resolve), como em qualquer
  TUI. O `portable-pty` 0.9 não expõe `pre_exec`, então um `PR_SET_PDEATHSIG` exigiria abrir o
  PTY por conta própria; não fez falta. Só os grupos de sessões ainda não colhidas recebem
  sinal: depois do `wait`, o pid pode ir pra outro líder de grupo (um job do shell do
  usuário);
- um emulador embutido (`vt100`) não repassa sozinho o que não é tela. O OSC 52 (o copiar do
  `claude`) chega a um callback e, com o padrão, é descartado: o painel o reescreve no
  terminal de fora, só o copiar (a leitura do clipboard, `ESC]52;c;?`, é descartada). Que
  pedidos chegam é decisão da jail (o `restrito` barra o OSC 52 antes do painel);
- colar com bracketed paste: tirar só o `ESC[201~` do texto colado não basta (`ESC ESC[201~`
  o forma de novo, e o resto chega como digitado). O painel descarta todo caractere de
  controle menos tab e CR dentro dos colchetes, como o xterm;
- o `vt100` 0.16 entra em panic com um caractere largo (emoji, CJK) numa tela de 1 coluna ou 1
  linha. A tela de cada sessão (parser e PTY) tem de ter pelo menos 2x2, por menor que seja a
  janela;
- o panic hook vale pra qualquer thread e tem de terminar o processo (saída 101): senão, um
  panic numa thread secundária restaura o terminal e mata as sessões, mas o laço principal
  continua desenhando sobre o terminal restaurado.
- **requisito da casca, pedido do usuário no uso do protótipo (2026-11-12):** o `nick` roda de
  qualquer diretório e mostra todos os projetos (navegação Projeto > Ticket > Agente), e cada
  sessão abre no diretório do próprio projeto. O cwd passa a ser por sessão, e não um só pro
  painel: é ele que o ai-jail deixa gravável e que faz o `claude` carregar o `.mcp.json` e o
  `CLAUDE.md` do projeto (mesmo racional de "auto_scrum roda com o cwd dentro do projeto
  alvo"). O protótipo não faz isso, porque a navegação com dados reais estava fora do escopo do
  NS-29. Fica em aberto de onde vêm a lista de projetos e os caminhos.

## Rust no projeto: a casca `nick` (NS-31)

> **Mudou no NS-52** (seção "Rust no projeto: instalador local (NS-52)"): o caminho do
> `bin/nick-init` não é mais fixado só na compilação. A raiz dos arquivos sai da localização do
> binário (o prefixo de uma instalação do `make install`, ou o checkout), e o operador instala com
> `make install`, não com `cargo install --path nick`. O `NICK_INIT_BIN` continua igual. A
> política de migrações (o banco mais novo recusado) ganhou um teste de integração com o binário.

> **Mudou no NS-38** (seção "Rust no projeto: navegação em árvore (NS-38)"): as telas Módulos e
> Projetos viraram a árvore (Módulo > Projeto > Ticket > Agente), sempre lado a lado com o painel do
> nó selecionado. O cadastro (`a`) e as Configurações (`c`) ficam no nó do módulo, a remoção (`r`)
> no nó do projeto. As regras de cadastro, disponibilidade, init e remoção abaixo não mudaram.

`nick/` é o crate da casca de verdade do épico NS-28 (fatia F2a): o programa `nick`, uma TUI em
Rust com ratatui e crossterm que o operador abre de qualquer terminal (como o `htop`). Ela mostra
**Módulos > Projetos**, lidos de um banco SQLite. Nesta fatia o único módulo é o auto_scrum, e a
TUI só cadastra, mostra e remove projetos. Tickets, agentes, histórico e o lançamento do
`claude` ficam para as fatias seguintes. (O NS-32 acrescentou o nível **Tickets**, abaixo de
Projetos: seção "Rust no projeto: tickets na casca `nick` (NS-32)".) O código do protótipo (`prototype/panel/`) não foi
copiado: o crate foi escrito do zero, levando os achados do NS-29.

**A regra de linguagem (bash + Rust), revista no NS-31:**
- o **Rust é a casca**: o programa `nick`, a TUI, o banco e a navegação;
- o **bash continua sendo as ferramentas e as bibliotecas** (`lib/`, `bin/nick-init`,
  `auto_scrum`), a fonte única do init, da configuração, da jail e das regras do `claude`. A
  casca chama o bash e não o reimplementa: o botão "Criar configuração" roda o `bin/nick-init`
  como ele está, e nenhuma linha do init foi para o Rust;
- regras do Rust: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, edition 2024, código
  (identificadores, comentários, nomes de arquivo, SQL) em inglês e o que aparece na tela em
  PT-BR (seção "Idioma do código"). Versões: `ratatui` 0.30.2 e `crossterm` 0.29.0 (as que o
  NS-29 validou) e `rusqlite` com a feature `bundled`, que compila o SQLite no binário (não
  depende da versão do sistema e já prepara o binário único da F6). Dev-dependencies:
  `portable-pty` (o binário num PTY), `tempfile` e o `vt100` 0.16.2, o mesmo do protótipo, que
  lê a saída do PTY como tela nos testes de integração. O `signal-hook` 0.3.18 (o `nick` imune ao
  `Ctrl+C` enquanto o init roda, abaixo) é declarado na versão que o crossterm já usa, sem
  duplicar nada no `Cargo.lock`. Sem crate de migração e sem
  `dirs`/`directories`: a versão do esquema é o `PRAGMA user_version`, e o diretório de estado
  sai de duas variáveis de ambiente, tudo em poucas linhas e testável.

**Crate, instalação e o `bin/nick-init`:**
- o crate fica em `nick/` (pacote e binário `nick`), com `Cargo.lock` versionado (é binário) e
  `nick/target/` no `.gitignore`. O operador instala com `cargo install --path nick` (o
  binário vai para `~/.cargo/bin/nick`). Empacotamento e atualização pelo sistema são da F6;
- o caminho do `bin/nick-init` é fixado na compilação:
  `concat!(env!("CARGO_MANIFEST_DIR"), "/../bin/nick-init")`, normalizado sem tocar no disco
  (`init::init_path`). O binário instalado continua apontando para o checkout de onde foi
  compilado: um `cargo install` feito de outro clone aponta para aquele clone, e um checkout
  movido depois da instalação quebra o botão. A mensagem do init ausente mostra o caminho
  procurado. Resolver isso é da F6. Um teste de unidade confere que o caminho padrão é
  `<raiz>/bin/nick-init` e que o arquivo existe, para pegar um crate movido sem ajustar o
  caminho;
- a variável **`NICK_INIT_BIN`**, quando definida e não vazia, troca o caminho do init (um
  caminho relativo vira absoluto contra o cwd do `nick`, já que o init roda com o cwd no
  projeto). É ela que os testes de integração usam para apontar para o init falso
  (`nick/tests/fixtures/bin/nick-init`). O prefixo é `NICK_` e não `CLAUDE_`, pelo mesmo motivo
  do `lib/claude/` e das `NICK_PANEL_*`. Ela só escolhe qual programa o operador roda pelo botão,
  no terminal dele, então não abre nada que ele já não pudesse rodar.

**Local do banco:** `$XDG_STATE_HOME/nick/nick.db`, ou `$HOME/.local/state/nick/nick.db` quando
`XDG_STATE_HOME` não está definida, está vazia ou é relativa (pela especificação XDG, um valor
relativo é inválido e deve ser ignorado). É o diretório de **estado** do XDG, e não o de
configuração nem o de dados, porque o cadastro é estado da máquina do operador: não é
configuração versionável nem dado que valha copiar entre máquinas. Sem `HOME` (ou com `HOME`
vazio) e sem um `XDG_STATE_HOME` válido, o `nick` não abre a TUI: imprime `Erro: não sei onde
guardar o banco do nick (HOME e XDG_STATE_HOME não definidos).` no stderr e sai com 1. O
diretório é criado com `0700` quando nasce, porque o banco pode ter caminhos de projetos de
cliente. Uma falha ao criar o diretório ou abrir o banco também não abre a TUI: mensagem em
PT-BR com o caminho no stderr e saída 1. Só o `main.rs` lê o ambiente
(`paths::state_dir(xdg, home)` recebe as variáveis por parâmetro), então nenhum teste toca no
`~/.local/state` real: os de integração rodam o binário com `env_clear` e um `HOME` e um
`XDG_STATE_HOME` temporários.

**Política de migrações** (`nick/src/db/`):
- a versão do esquema é o `PRAGMA user_version`. A migração N (item N-1 da lista estática e
  ordenada de `migrations.rs`) leva o banco da versão N-1 para a N. Ao abrir o banco, cada
  migração pendente roda **numa transação, junto com o novo `user_version`**: se ela falhar,
  nada dela fica, e o `nick` sai com a mensagem e o código 1;
- **uma migração publicada nunca é editada.** Um erro se corrige com uma migração nova no fim
  da lista. O teste `migration_1_is_pinned` fixa o texto literal da migração 1 (e o tamanho da
  lista). Se ele falhar, desfaça a edição e escreva a migração 2 (que fixa o próprio texto no
  mesmo teste);
- um banco de versão **maior** que a última conhecida (migrado por um `nick` mais novo) é
  recusado sem ser gravado: `Erro: o banco <caminho> está na versão <N>, mais nova que a deste
  nick (<M>). Atualize o nick.` e saída 1;
- toda conexão liga `PRAGMA foreign_keys = ON` e `busy_timeout` de 5 s. O SQLite tenta de novo
  sozinho enquanto outro `nick` estiver gravando no mesmo banco, que é o único erro passageiro
  dele. Decisão do usuário no NS-31: **sem reabrir o banco nem repetir com espera crescente**.
  O laço é síncrono, então a TUI ficaria parada sem responder nem ao `q`. Os outros erros
  (arquivo corrompido, disco cheio, permissão) não passam sozinhos, e reabrir um `nick.db`
  apagado o recriaria vazio sem avisar. Com a TUI aberta, um erro inesperado do banco vira
  `Erro no banco do nick: <erro>.` na barra de status, e a TUI continua;
- a migração 1 tem só `modules` (com o `auto_scrum`) e `projects`. Ticket, agente e histórico
  entram nas fatias que os usam. A migração 2 (NS-32) cria a tabela `tickets`, ligada ao projeto
  com `ON DELETE CASCADE` (seção do NS-32). O `created_at` é gravado pelo próprio SQL
  (`strftime('%Y-%m-%dT%H:%M:%SZ','now')`, RFC 3339 em UTC), sem crate de data. O caminho é
  `TEXT`: um caminho que não é UTF-8 é recusado no cadastro (`Erro: o caminho tem caracteres
  que o nick não consegue guardar.`), um caso raro que mantém o esquema simples.

**Cadastro só explícito de projetos:** o operador cadastra cada projeto na TUI, informando o
caminho, e o diretório de onde o `nick` é aberto não influencia nada (um teste de integração
abre de três cwds e vê a mesma lista). A Q1 do plano do épico
(`.nick/docs/plans/migracao_tui.md`) propunha cadastrar automaticamente o projeto do cwd. O
ticket decidiu pelo cadastro explícito, e o cadastro automático saiu (o plano do épico ficou
desatualizado nesse ponto). O único ponto em que o cwd entra é o caminho relativo digitado pelo
operador, resolvido contra o cwd do processo `nick`. A ordem da validação (`projects::check`):
aparar os espaços (vazio → `Informe o caminho da pasta do projeto.`), expandir `~` e `~/...`
com o `HOME` (`~usuario` é um nome relativo comum), resolver o relativo, `canonicalize` (sem
`.`/`..`, com os symlinks resolvidos; inexistente → `A pasta <digitado> não existe.`), conferir
que é pasta, que o caminho é UTF-8 e que ainda não está cadastrado no módulo (dois caminhos para
a mesma pasta contam como o mesmo, e o `UNIQUE` garante isso e dá a mesma mensagem), e só então
olhar o `.nick/nick.conf`. Remover faz só `DELETE FROM projects WHERE id = ?`: nenhum arquivo do
projeto é tocado (um teste compara um retrato recursivo da pasta antes e depois). Desde o NS-32,
esse `DELETE` também apaga os tickets do projeto, pelo cascade do esquema.

**O projeto pertence a um módulo, e a disponibilidade é calculada:** a navegação é "projetos
**do módulo**", então `projects.module_id` aponta para `modules`. Um módulo futuro que use o
mesmo diretório cadastra o projeto de novo no módulo dele. Se um dia os projetos passarem a ser
compartilhados entre módulos, uma migração nova faz a mudança. A disponibilidade (pasta ausente
ou que virou arquivo → `[indisponível: pasta não encontrada]`; sem `.nick/nick.conf` → `[indisponível:
sem .nick/nick.conf]`) é calculada do disco ao entrar na tela e depois de cada ação na lista,
nunca guardada no banco. Enquanto um diálogo fica aberto ela não é recalculada: digitar um
caminho não é uma ação, e cada recálculo lê o disco (uma montagem de rede travada congelaria o
laço síncrono a cada letra; review do NS-31, P3). Ela volta a ser calculada quando o diálogo
fecha, seja qual for o jeito. Um projeto indisponível aparece com o estilo apagado e não derruba nada.

**Chamada ao `bin/nick-init` suspendendo a tela** (fluxo "Criar configuração"):
- uma pasta sem `.nick/nick.conf` abre um alerta com o caminho exato onde a configuração seria
  criada e dois botões, "Criar configuração" e "Cancelar". **O foco inicial fica em
  "Cancelar"**, porque o init grava na pasta do projeto, que pode estar fora da jail. Abrir o
  alerta não grava nada, nem no banco nem na pasta;
- o init só roda pelo botão: o `app.rs` (estado e teclas, sem I/O de terminal) só devolve
  `Action::RunInit` quando o botão é acionado, e o laço só chama o `InitRunner` a partir dessa
  ação. Um teste de unidade (com um runner falso) e um de integração (o binário real no PTY, com
  um init falso que grava um marcador) provam que tentar cadastrar uma pasta sem configuração
  não roda o init e não cria nenhum arquivo nela;
- o botão confere antes se o init existe e é arquivo. Se não for, o alerta mostra `Não encontrei
  o bin/nick-init em <caminho>. Nada foi feito.`, e a TUI continua;
- senão, a TUI é suspensa (sai do raw mode e da tela alternativa e mostra o cursor), imprime
  `Rodando <init> em <pasta>...` e roda `bash <init>` com o cwd no projeto, sem argumentos e com
  o terminal herdado. **Depois, seja qual for o resultado, espera um `Enter`** (`Pressione Enter
  para voltar ao nick.`). Sem isso, um erro do init sumiria no instante em que a tela alternativa
  volta, e é o mesmo cuidado do `git` com o pager. Depois a TUI volta, com um redesenho completo
  (`terminal.clear()`), também quando o init falha;
- **o `nick` ignora o `Ctrl+C` (SIGINT) e o `Ctrl+\` (SIGQUIT) enquanto o init roda e na espera
  do `Enter`** (review do NS-31, P1). Fora do raw mode, o terminal manda esses sinais para todo o
  grupo de processos em primeiro plano, e com a ação padrão o `nick` morreria junto com o init,
  sem restaurar a TUI e sem dizer nada. Ele faz como o `system(3)` do POSIX: só o filho recebe o
  sinal. É um handler que só liga uma flag que ninguém lê (`signal_hook::flag::register`), e não
  um `SIG_IGN`, porque o handler volta ao padrão no `exec`, e o `bash`/`nick-init` continua
  recebendo o `Ctrl+C`. Na espera do `Enter`, o `read_line` repete sozinho no `EINTR`, então o
  `Ctrl+C` ali é ignorado. Os handlers saem antes de a TUI voltar, e no raw mode o `Ctrl+C` é de
  novo a tecla que sai do `nick`. O teste `ctrl_c_during_the_init_only_interrupts_the_init`
  manda um `0x03` real pelo PTY. O teste do init "morto por sinal" com `kill -TERM "$$"` não
  pegava esse caso, porque o sinal não passava pelo `nick`;
- **na volta, o `.nick/nick.conf` é conferido de novo.** O projeto só é cadastrado se o arquivo
  existir. Senão, o alerta diz por quê, conforme o fim do init: saiu com 0 (sobrescrita
  recusada), saiu com ≠ 0 (com o código), morto por sinal, ou não deu para rodar (`spawn`). O
  arquivo é o que vale: um init que falhou depois de criar a configuração cadastra o projeto.

**Terminal, laço e panic:**
- um guarda RAII (`TerminalGuard`) entra no raw mode e na tela alternativa e, no `Drop`, sai dos
  dois e mostra o cursor, também quando o `run()` devolve erro (aí a mensagem vai para o stderr
  como `nick: <erro>`, e a saída é 1). Nesta fatia não há captura de mouse nem bracketed paste:
  nada usa esses modos, e cada modo a mais é mais um a restaurar. Colar um caminho no campo de
  texto funciona como digitação;
- **laço síncrono nesta fatia:** `crossterm::event::read()` na thread principal, sem thread
  leitora e sem canal. Ainda não há PTY, e uma thread leitora disputaria o stdin com o
  `bin/nick-init` enquanto ele roda no terminal suspenso. **A F3 troca isso por um laço guiado
  por um canal** (achado do NS-29) e, então, a leitura da entrada tem de ser pausada enquanto o
  init roda. **Feito no NS-33** (seção "Rust no projeto: agentes na casca `nick` (NS-33)"): o
  laço é guiado por canal, e a thread de entrada para num portão enquanto o init roda;
- o **panic hook** é instalado antes de entrar na TUI. Ele restaura o terminal, chama o hook
  padrão (a mensagem sai depois da restauração, legível) e termina com
  `std::process::exit(101)`, em qualquer thread. Sem o `exit`, um panic numa thread secundária
  deixaria o laço principal vivo sobre o terminal restaurado;
- **`NICK_TEST_PANIC` é um gancho só de teste** (`nick/tests/tui.rs`), no estilo do
  `NICK_PANEL_TEST_PANIC` do protótipo, depois de entrar na TUI: `main` faz panic na thread
  principal, `thread` faz panic numa thread criada para isso (com a principal bloqueada no
  `event::read()`), e `error` faz o `run()` devolver um erro. O `error` foi acrescentado no
  NS-31 (decisão do usuário): como o banco abre antes da TUI, o teste do banco ilegível não
  chega a entrar nela e não prova a restauração por erro;
- **SIGTERM/SIGHUP ficam sem tratamento nesta fatia:** não há filhos para encerrar, e o padrão do
  sistema mata o processo. O terminal pode ficar sujo, como no `kill -9` (achado do NS-29), e o
  `reset` resolve. A F3 trata isso junto com o encerramento dos agentes. **Feito no NS-33:** os
  dois restauram o terminal, encerram o agente aberto e saem com 143/129;
- teclas: `q`/`Q` saem só nas listas (regra de "Menus: opção Sair (q)"). No campo de texto o `q`
  é digitado, e nos diálogos ele não sai. `Ctrl+C` sai de qualquer tela, porque o raw mode o
  entrega como tecla e o operador não pode ficar preso. Na remoção, só `s`/`S` confirmam (o "sim"
  do `auto_scrum`). As teclas de cada tela ficam no rodapé da TUI, e não no README;
- barra de status: a recusa do campo de texto continua na barra enquanto o campo está aberto (o
  operador corrige o texto), e sai quando ele cancela o campo (`Esc`) ou quando o alerta abre ou
  fecha por `Esc`/"Cancelar": é mensagem de uma ação que ele abandonou (review do NS-31, P2);
- campo de texto: tem uma linha só, sem quebra. Um caminho maior que a caixa mostra `…` e o fim
  do texto, com o cursor logo depois, porque o ponto de digitação é o que importa. Com quebra,
  um caminho de mais de 88 caracteres fazia o cursor sumir (review do NS-31, P5).

**Como acrescentar um módulo:** uma variante em `ModuleKind` (e em `ModuleKind::ALL`), uma
migração nova que o insere em `modules` e um arquivo em `nick/src/modules/`. É um `enum` com
`match`, e não um trait com objetos dinâmicos: com um módulo só, um trait seria abstração sem
reuso real (seção "Reaproveitar o que já existe antes de criar"). Cada módulo define a própria
hierarquia quando for desenhado (decisão do épico). Um teste confere que o `enum` e a tabela
`modules` (depois das migrações) têm os mesmos módulos.

**Testes: o `make test` passa a exigir o `cargo`** (impacto aceito no ticket). A suíte bash
`nick/tests/test_cargo.sh` é achada pelo `tests/run_all.sh` só pelo nome e pelo lugar, sem mexer
no runner nem no `Makefile`. Ela dá `cd` no crate (achado pelo `BASH_SOURCE`) e roda, em ordem,
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` e `cargo test`, parando no
primeiro que falhar. Sem `cargo` no PATH, ela imprime no stderr `Erro: o cargo não foi encontrado
no PATH. A suíte Rust do nick (nick/) precisa dele: instale o Rust (rustup) e rode de novo.` e sai
com 1. A guarda dela é `tests/test_cargo_suite.sh`, na raiz, ao lado de `test_run_all.sh`,
porque testa uma peça do `make test`. Ela **nunca roda o `cargo` real**, pelo mesmo racional da
suíte do runner. Cobre o PATH mínimo sem `cargo` (com controle positivo), as três chamadas em
ordem com o cwd no crate (com um `cargo` falso que grava argv e cwd), a falha de cada uma, a
estrutura e o `shellcheck -x` das duas suítes e do init falso. A suíte Rust tem testes de unidade
por módulo (`paths`, `db`, `modules`, `projects`, `init`, `app` com um `InitRunner` falso, `ui`
com o `TestBackend`) e testes de integração com o binário real: `tests/cli.rs` (argumentos) e
`tests/tui.rs` (PTY 30x100, com `tests/common/`). O retrato recursivo de uma pasta (tipo,
tamanho, conteúdo e mtime de cada entrada), que prova que nenhum arquivo do projeto foi tocado,
é um só: `nick/tests/common/snapshot.rs`, usado pelos testes de integração e incluído nos de
unidade de `projects.rs` com `#[path]` (review do NS-31, P4). Assim o teste no PTY também pega
um `touch`. O harness do PTY responde ao pedido de posição
do cursor (`ESC[6n`), como faria um terminal: o `Terminal::clear` do ratatui o faz quando a TUI
volta do init, e sem resposta ele dá erro depois de 2 s.

## Rust no projeto: tickets na casca `nick` (NS-32)

> **Mudou no NS-40** (seção "Rust no projeto: épicos opcionais (NS-40)"): o ID é único entre os
> tickets **e** os épicos do projeto (os dois usam `.nick/docs/<ID>/`), conferido pelo código
> (`Db::find_key`, no lugar do `find_ticket_key`) e por triggers da migração 7, com a mensagem
> `Já existe um épico com o ID <ID> neste projeto.` ao lado da do ticket. Um ID provisório que
> colide com um épico é gerado de novo. O laço da criação virou `tickets::create_key_with`, usado
> também pelos épicos.

> **Mudou no NS-38** (seção "Rust no projeto: navegação em árvore (NS-38)"): a tela de Tickets virou
> o nível de tickets da árvore; as ações (`a` no nó do projeto; `e`, `o` no nó do ticket) e as
> regras abaixo não mudaram. O filtro `f` passou a valer para a árvore inteira e não se desliga ao
> subir.

A fatia F2b do épico NS-28. A navegação da casca passa a ser **Módulos > Projetos > Tickets**.
`Enter`/`→` sobre um projeto disponível abre a lista dos tickets dele, e `Esc`/`←` volta com o
projeto ainda selecionado. Na lista, o operador cria, corrige o título, oculta e restaura
tickets. As teclas ficam no rodapé da TUI. As regras, sem tela, ficam em `nick/src/tickets.rs`,
no molde do `projects.rs`, e o esquema na migração 2 (`nick/src/db/migrations.rs`).

**Tudo fica só no banco.** Nenhuma ação sobre ticket cria, altera ou apaga arquivo do projeto.
Em especial, nenhuma cria `.nick/docs/<ID>/`: essa pasta continua sendo do `auto_scrum.sh`. Um
teste de unidade (`tickets.rs`) e um de integração no PTY (`tests/tui.rs`) provam isso com o
retrato recursivo da pasta (`tests/common/snapshot.rs`). A lista também não importa as pastas
`.nick/docs/<ID>/` que já existem. **Desde o NS-36**, o "Editar ID" renomeia a pasta
`.nick/docs/<ID>/` do ticket, a única escrita do nick no projeto (seção "Rust no projeto: etapa
do ticket, histórico e edição do ID (NS-36)").

**UUID provisório:** sem o ID do Jira, o nick gera um ID de **8 caracteres hexadecimais
minúsculos** com o próprio SQLite (`lower(hex(randomblob(4)))`), sem crate de UUID nem de
número aleatório. O valor sempre passa no `jira_id_valid`. A unicidade é por projeto e vem do
`UNIQUE (project_id, key)`: numa colisão, o nick gera de novo, até **5 tentativas** no total.
Depois disso, a barra mostra `Não consegui gerar um ID provisório único para o ticket. Tente de
novo.`, e nada é gravado. O gerador é injetável (`tickets::create_with`), como as migrações do
`Db::open_with`, e os testes provam a colisão com um gerador falso. O ID é local à máquina:
ainda não há como evitar a colisão entre máquinas (Q7 do épico). A coluna `is_provisional` existe
para que a F4 (Q8) saiba qual ID trocar pelo do Jira. Só pelo texto não dá para saber, porque
`deadbeef` e `24` são IDs válidos do Jira. (Desde o NS-36, o "Editar ID" troca o ID e zera o
`is_provisional`: seção "Rust no projeto: etapa do ticket, histórico e edição do ID (NS-36)".)

**O título é digitado pelo operador e guardado no banco.** O nick não lê o `plan.md` nem o Jira
para preencher o título. A lista precisa de um rótulo antes de existir plano, e ler o plano ou o
Jira acoplaria a casca a formatos e à rede, o que ficou fora do escopo. Se o título do nick
divergir do título no Jira ou no plano, tudo bem: ele só serve para o operador reconhecer o
ticket. O título é gravado aparado, e vazio ou só com espaços é recusado (`Informe o título do
ticket.`). No esquema, um `CHECK` serve de rede de segurança.

**Ocultar no lugar de apagar:** ocultar só grava `hidden_at`, e a linha continua no banco. O
filtro `f` mostra os ocultos com a marca `[oculto]`, e `o` sobre um oculto o restaura sem pedir
confirmação, porque restaurar não esconde nada. Ocultar pede `(s/N)`, como a remoção de projeto.
Não existe limpeza dos ocultos, de propósito. Um ID oculto continua ocupando o ID no projeto, e
a recusa avisa (`... neste projeto (oculto; f mostra os ocultos).`). Ocultar projetos ficou fora
do escopo.

**Ticket que sumiu do banco não é dado como alterado** (review do NS-32): corrigir o título,
ocultar e restaurar conferem quantas linhas o `UPDATE` mudou. Se o ticket não existe mais
(outro `nick` removeu o projeto enquanto este mostrava a lista), a barra diz `O ticket <ID> não
existe mais no banco.` no lugar da mensagem de sucesso, e nada é gravado.

**Remover projeto apaga os tickets dele em cascata** (`ON DELETE CASCADE`, decisão do analista no
NS-32). Sem o cascade, o `DELETE FROM projects` falharia, porque toda conexão liga
`foreign_keys = ON`. Como o cascade faz parte do próprio `DELETE`, não precisa de transação extra,
e o `delete_project` não mudou. A confirmação avisa quantos tickets vão junto, ocultos incluídos:
`Os <N> tickets dele (inclusive os ocultos) serão apagados do banco.` ou, com um, `O ticket dele
será apagado do banco.` (texto do singular escolhido pelo usuário no NS-32). Com 0 tickets, o
texto é o do NS-31, byte a byte.

**Validação do ID duplicada (Rust e bash):** `tickets::jira_id_valid` espelha o `jira_id_valid`
de `auto_scrum/auto_scrum.sh`. Os dois recusam vazio, exatamente `.`, qualquer `/`, qualquer
`..`, espaço e caractere de controle. **Qualquer mudança vale para os dois lados, com os mesmos
casos de teste.** O teste Rust copia as listas da suíte bash (`auto_scrum/tests/test_auto_scrum.sh`,
seção `jira_id_valid`), e um comentário em cada lado aponta para o outro. Um teste diferencial
roda o `jira_id_valid` real do bash em cada caso, com o locale herdado e com `LC_ALL=C`, e exige o
mesmo veredito. É ele que pega uma divergência. O ID digitado é aparado antes, como o `read` do
bash faz, e vazio depois de aparar quer dizer "sem ID" (gera o provisório). **Diferença
conhecida e aceita:** o Rust usa as classes Unicode (`char::is_whitespace` também pega o NBSP
U+00A0, e `char::is_control` pega o C1, U+0080–U+009F). Já o `[[:space:]]`/`[[:cntrl:]]` do bash
depende do locale. O desvio sempre vai para o lado de recusar mais, e o Rust nunca aceita o que o
bash recusa nos casos de teste. Os dois aceitam acento (`SÃO-1`). A recusa não mostra o valor
digitado, que pode ter caractere de controle, como no bash.

**Esquema (migração 2):** `tickets (id, project_id → projects ON DELETE CASCADE, key,
is_provisional ∈ {0,1}, title com CHECK, created_at, hidden_at NULL enquanto visível,
UNIQUE (project_id, key))`. A comparação do `key` é exata (`BINARY`), como o nome de pasta no
Linux, então `NS-1` e `ns-1` são IDs diferentes. As datas saem do próprio SQL (`strftime`, RFC
3339 em UTC), como no NS-31. O teste `migration_1_is_pinned` passou a fixar também o texto da
migração 2. Um banco na versão 1 com projetos abre num `nick` novo, vai para a 2 e mantém os
projetos (teste de unidade).

**Barra de status e rodapé com quebra de linha** (decisões do usuário no NS-32): os dois ocupavam
uma linha cada e cortavam o que passasse da largura. Duas mensagens do NS-32 passam de 100
colunas (o projeto indisponível e o ID inválido), e o rodapé da lista de tickets com o filtro
ligado tem 101. Agora cada um ocupa as linhas de que precisa, com quebra por palavra
(`ui::wrap_words`), até um terço da tela cada, e a lista encolhe. Uma palavra maior que a largura
é partida. Um texto curto continua numa linha, como no NS-31, e a linha da barra vazia continua
reservada. Num terminal estreito, o rodapé das outras telas também passa a aparecer inteiro. A conta é exata, e não
a estimativa do `wrapped_rows` dos diálogos, para não sobrar linha vazia. O
`Paragraph::line_count` do ratatui daria a conta, mas fica atrás de uma feature instável.

**Reaproveitamento e adaptações no código do NS-31:**
- `unavailable_mark` saiu do `ui.rs` e virou `projects::unavailable_text`. O `ui.rs` e a recusa
  de abrir os tickets usam o mesmo texto: `Projeto indisponível: <marca>. Os tickets só abrem com
  a pasta e o .nick/nick.conf no lugar.`. A disponibilidade é lida do disco na hora do `Enter`;
- a digitação do campo de texto virou `app::edit_text`, usada pelo `AddPath` (sem mudar o
  comportamento) e pelos dois diálogos novos (`NewTicket`, com dois campos, e `EditTitle`);
- `Dialog::ConfirmRemove` ganhou `tickets`, preenchido com `count_tickets` ao abrir. Por isso,
  dois testes do NS-31 mudaram só na sintaxe: um padrão ganhou `..` e um literal ganhou
  `tickets: 0`. O texto conferido não mudou;
- o `snapshot.rs` é incluído uma vez só, pelo `projects.rs` (`pub(crate)`, só em teste), e o
  `tickets.rs` usa esse módulo. Incluir o mesmo arquivo duas vezes com `#[path]` cai no
  `clippy::duplicate_mod`;
- **corrida nos testes de PTY do NS-31 (corrigida no NS-32, com o aval do usuário):** depois de
  um `ESC` no alerta, três pontos de `tests/tui.rs` esperavam `Nenhum projeto cadastrado`, um
  texto que já está visível atrás do alerta. Se o `ESC` e a tecla seguinte chegam juntos ao
  `nick`, o crossterm lê os dois como `Alt+tecla`, e o teste trava até o timeout. Com os testes
  novos rodando em paralelo, isso apareceu 2 vezes em umas 15 rodadas. Agora eles esperam o rodapé
  da lista (`r remover`), que só aparece com o alerta fechado. Regra para testes novos no PTY:
  **depois de um `ESC`, espere um texto que só existe no estado seguinte** (de preferência o
  rodapé), nunca um texto que já estava na tela.

## Rust no projeto: agentes na casca `nick` (NS-33)

> **Mudou no NS-52** (seção "Rust no projeto: instalador local (NS-52)"): o launcher não é mais
> fixado na compilação. Ele fica sob a raiz dos arquivos do nick, achada pela localização do
> binário (`resources.rs`). A regra de segurança continua: nenhuma variável de ambiente troca o
> `bin/nick-agent`. Sem o `claude` no PATH, abrir e retomar agente são recusados antes de qualquer
> chamada ao launcher.

> **Mudou no NS-50** (seção "Rust no projeto: retomada segue a troca de sessão do claude
> (NS-50)"): o ID de sessão "escolhido pelo nick e único" continua valendo para o `session_id`,
> que fica fixo e continua sendo a chave da trava (`<session_id>.lock`). Mas a retomada pode usar
> outra sessão, a `agents.resume_session_id` (migração 8), quando o `claude` continuou a conversa
> noutra sessão.

> **Mudou no NS-39** (seção "Rust no projeto: retomar e arquivar agentes (NS-39)"): o estado
> `ended` virou `stopped` (parado, retomável), e entrou o `archived`, pela migração 6, que também
> acrescentou `agents.opened_key`. A coluna `state` continua sem `CHECK`, e o `AgentState`
> continua a única fonte dos estados. O `end_agent` virou `stop_agent`, e o `bin/nick-agent`
> ganhou os modos `--resume` e `--has-conversation`.

> **Mudou no NS-38** (seção "Rust no projeto: navegação em árvore (NS-38)"): o `g` fica no nó do
> ticket, e o fim de um agente deixa a seleção no nó dele (agora encerrado), e não "de volta à
> lista". A remoção de um projeto passou a levar os agentes dele: a confirmação os conta, os vivos
> deste nick são encerrados, e um ativo em outro nick recusa a remoção.

> **Mudou no NS-35** (seção "Rust no projeto: etapas do auto_scrum na casca `nick` (NS-35)"): o
> `--check` passou a ter três linhas (`jail=`, `jira=` e `sentry=`), o diálogo das 4 etapas virou
> o menu dos 8 tipos do `choose_type` com formulário e prévia do prompt, e o modo de lançamento
> sem prompt e sem `--permission-mode` deixou de existir (o lançamento exige `--type` e
> `--prompt-file`). O texto abaixo é o registro do NS-33.

A fatia F3a do épico NS-28. Na tela de tickets, `g` sobre um ticket visível de um projeto
disponível abre o diálogo "Abrir agente" (PO, Tech Leader, Dev, Review), e a etapa escolhida
abre o `claude` num terminal embutido que ocupa a tela, com uma linha de cabeçalho. **Um agente
por vez**, e o terminal do agente **só fecha quando o `claude` termina** (`/exit`, `Ctrl+D`,
`Ctrl+C` duas vezes): todas as teclas vão para ele, inclusive `Ctrl+C`, `Esc`, `q` e `Ctrl+\`
(0x1c). O atalho reservado para sair, vários agentes e a confirmação ao sair são da F3b (feitos
no NS-34: seção "Rust no projeto: vários agentes na casca `nick` (NS-34)"). Fora das
teclas, só `Shift+PageUp/PageDown` e a roda do mouse (quando o programa não pediu o mouse) rolam
o histórico; qualquer tecla enviada volta para o fim. Ao terminar, a TUI volta para a lista com o
mesmo ticket selecionado e `O agente <nome> terminou.` (ou `... terminou com o código <N>.`).

**O lançamento passa só pelo `bin/nick-agent`** (bash, ao lado do `bin/nick-init`). Ele carrega
as quatro bibliotecas na ordem do `auto_scrum.sh`, chama `config_register_nick_keys` no topo e
lê o `.nick/nick.conf` do cwd (a casca roda `bash <launcher> ...` com o cwd na pasta do projeto,
o caminho do banco). Interface:
- `nick-agent --check`: só valida (o `.nick.conf` antigo, o `.nick/nick.conf` do cwd com a mesma
  mensagem do `resolve_stack`, o `config_load`, o `jail_check` e o `claude_settings_load`), sem
  lançar nem gravar nada no projeto. Sucesso: uma linha no stdout, `jail=true` ou `jail=false` (o
  valor efetivo: `true` só com `"true"` exato), e saída 0; os avisos do `config_load` vão para o
  stderr. Falha: a mensagem da lib no stderr e saída 1;
- `nick-agent --session-id <uuid> --name <nome>`: as mesmas validações, `config_dir_prepare`,
  `jail_print_status` e `jail_exec ... claude --settings <regras> [--append-system-prompt
  <aviso> só com a jail ligada] --session-id <uuid> --name <nome>` (desde o NS-41, o
  `--append-system-prompt` existe sempre e começa pela instrução de não contornar uma recusa:
  ver "Instrução de não contornar uma recusa (NS-41)"). **Sem `--permission-mode` e sem
  prompt**: o modo de permissão é o padrão do `claude` em todas as etapas até a F4 (roteiros e
  modo `auto` do Dev);
- qualquer outro uso: `Erro: ...` e `Uso: nick-agent --check | --session-id <uuid> --name <nome>`
  no stderr, saída 1. O formato do UUID não é validado no bash (o `claude` valida; o nick sempre
  gera um válido).

A casca acha o launcher como acha o init, pelo caminho fixado na compilação
(`concat!(env!("CARGO_MANIFEST_DIR"), "/../bin/nick-agent")`, normalizado sem tocar no disco;
o `normalize` saiu do `init.rs` para o `paths.rs`). **Não há variável de ambiente que troque o
launcher** (diferente do `NICK_INIT_BIN`): ela deixaria lançar outra coisa no lugar do caminho
que garante a jail e as regras. Os testes usam o launcher real com o `claude` e o `ai-jail`
falsos (`bin/tests/fixtures/bin/`) à frente do `PATH`; os de unidade do `App` recebem o caminho
do launcher por parâmetro. A API do `agent_session.rs` só sabe abrir `bash <launcher>
--session-id ... --name ...`, e um teste estrutural (`nick/tests/agent.rs`) lê os fontes de
`src/`: o único processo num PTY é `bash` com o launcher, nenhum `Command::new`/
`CommandBuilder::new` abre `claude` ou `ai-jail`, e nenhum fonte tem `--permission-mode` (com
controle positivo do filtro). O Rust não reimplementa nada da jail, das regras nem da
configuração: só sabe o que o `--check` devolve.

**Pré-checagem fora do PTY.** Um erro do `jail_check` ou da configuração dentro do PTY só
apareceria numa tela que some quando o launcher termina. Por isso a casca roda antes `bash
<launcher> --check` com o stdin em `/dev/null` e a saída capturada. Saída ≠ 0 → um alerta
("Agente não aberto") com `Nada foi aberto.` **no topo** e o stderr logo abaixo (cada linha por
`paths::printable`); nada vai para o banco e nada é lançado. O `Nada foi aberto.` fica no topo
porque o erro da jail tem umas 25 linhas e, num terminal baixo, o fim do alerta é cortado.
**Achado, fora do escopo:** o alerta não rola, então o fim de um erro longo pode ficar cortado
numa tela pequena (o começo, com o motivo, aparece). Saída 0 com qualquer coisa que não seja
exatamente uma linha `jail=…` também é erro (`O bin/nick-agent respondeu algo inesperado na
verificação.`), para nunca presumir a jail. **Um stderr de sucesso (aviso da configuração)
aparece na barra quando o agente termina**, junto com a mensagem de fim (`... terminou. Aviso da
verificação: ...`): decisão do usuário no NS-33, porque a tela do agente não tem barra de status
(o próprio launcher repete o aviso no topo do terminal antes de o `claude` limpar a tela). O
lançamento de verdade repete as validações; se falharem ali, o PTY termina com código ≠ 0 e a
barra diz o código. A chamada é síncrona, como no `auto_scrum` (o `jail_check` roda o
`ai-jail ... -- true`).

**Relação com o `auto_scrum.sh`: a lógica não pode divergir.** A única extração foi o
`mcp_jail_hosts`, que virou `jail_mcp_hosts <jira_enabled> <sentry_enabled>` em `lib/jail/jail.sh`
(por parâmetro, sem ler global do chamador). O `mcp_jail_hosts` do `auto_scrum.sh` continua como
chamada fina, porque os testes e o `main()` usam esse nome, e os testes dele passaram sem mudança.
O resto do caminho já era da lib. Uma função "lançar agente" inteira no `lib/` não cabe: ela
juntaria config + jail + claude, e as bibliotecas não dependem umas das outras. **Teste
diferencial** (`bin/tests/test_nick_agent.sh`): para a mesma `.nick/nick.conf` (jail, Jira,
Sentry, `JAIL_ALLOW_HOSTS`, `JAIL_FORWARD_PORTS`, `JAIL_CLIPBOARD`), o `auto_scrum.sh` (etapa PO,
`po_discussion`) e o launcher chamam o `ai-jail` com as mesmas opções até o `--`, e o argv do
`claude` do launcher é o do `auto_scrum` sem o prompt e com `--session-id`/`--name` no fim (e o
mesmo com a jail desligada). Uma mudança num dos dois que não for para o outro quebra o teste.

**A jail segue o projeto.** Vale o `JAIL_ENABLED` do `.nick/nick.conf` de cada projeto. O
cabeçalho mostra `<nome> · na jail` ou, fora dela, `<nome> · FORA DA JAIL: o agente roda direto
no seu shell.` com o aviso em vermelho e negrito (o texto do `jail_print_status` desligado). Se
o agente roda na jail fica só em memória nesta fatia (a F3b decide se vira coluna; decidido no
NS-34: não vira, seção "Rust no projeto: vários agentes na casca `nick` (NS-34)").

**Banco: migração 3, tabela `agents`** (`ticket_id → tickets ON DELETE CASCADE`, `stage` com
`CHECK` nos valores de `$TYPE` do `auto_scrum`, `session_id UNIQUE`, `name`, `opened_at` em RFC
3339 UTC pelo SQL, `state`). **`state` não tem `CHECK`, de propósito:** a F5 acrescenta
"parado" e "arquivado", e o SQLite não altera um `CHECK` sem recriar a tabela. A fonte dos
estados é o enum `AgentState` (`agents.rs`), e só ele grava a coluna (os métodos do `Db` usam
`AgentState::...key()`, nunca um literal). Estados desta fatia: `active` e `ended`. Um agente sai
do banco junto com o ticket, que sai com o projeto; a confirmação de remoção do projeto não
conta agentes. O ID de sessão é um UUID v4 gerado pelo próprio SQLite (`Db::random_session_id`,
`randomblob` com o nibble de versão `4` e o de variante em `8|9|a|b`), sem crate; o `UNIQUE` é a
rede de segurança. O nome é `<projeto> · <Etapa> · <ID do ticket>` (`PO`, `Tech Leader`, `Dev`,
`Review`). Testes de banco do NS-31/NS-32 que citavam a versão 2 passaram a citar a 3; o teste
`a_version_1_database_with_projects_migrates_to_2_and_keeps_them` passou a abrir só com as
migrações 1 e 2 (decisão do usuário no NS-33), sem mudar nenhuma asserção.

**Trava por agente e limpeza (D7, decisão do analista no plano do NS-33).** O critério do ticket
mandava encerrar, na abertura do nick, todo agente marcado como ativo; com dois `nick` abertos, o
segundo encerraria o agente vivo do primeiro. Por isso, enquanto o agente roda, o nick segura uma
trava exclusiva (`std::fs::File::lock`, um `flock`; sem crate) em
`<diretório de estado>/agents/<session_id>.lock` (diretório `0700`). O kernel solta a trava quando
o processo morre, até no `kill -9`. **Ordem:** pegar a trava antes de inserir a linha `active`;
ao terminar, `ended`, apagar o arquivo (ainda travado) e soltar. **Na abertura do nick**, antes da
TUI, para cada agente `active`: `try_lock` livre → `ended` e o arquivo apagado; `WouldBlock` →
outro nick vivo o segura, fica `active`; outro erro de I/O → fica como está (não se encerra o que
não dá para provar morto). Um erro do banco nessa limpeza vai para a barra, e a TUI abre.

**Laço guiado por canal** (o que o NS-31 reservava para a F3). A thread principal bloqueia num
`mpsc` que recebe: a entrada (uma thread com `event::poll(100 ms)` + `read`, viva enquanto a TUI
estiver aberta), a saída do PTY do agente, as cópias OSC 52 e o fim do processo, e os sinais. A
thread de entrada **para num portão** (`terminal::InputGate`, `Mutex` + `Condvar`) enquanto o
`bin/nick-init` roda: o `TerminalInitRunner` fecha o portão (e espera a thread sair da leitura)
antes de suspender a TUI e o reabre depois; o mesmo portão fica fechado durante o
`Terminal::clear` da volta do agente, que pergunta a posição do cursor. Custo aceito: 10
acordadas por segundo com a TUI ociosa. Redesenho no máximo a cada ~16 ms, e só quando algo
mudou. A cópia OSC 52 é escrita no terminal de fora entre dois quadros. Captura de mouse e
bracketed paste ligam ao entrar na tela do agente e desligam ao sair; o `terminal::restore`
desliga os dois sempre.

**Sinais:** uma thread com `signal_hook::iterator::Signals` trata **SIGTERM e SIGHUP** (fechar a
janela). Com agente aberto: SIGTERM no grupo do processo, SIGKILL depois de 2 s no que sobrar,
`wait`, `ended` no banco, trava solta, terminal restaurado e saída `128 + sinal` (143 e 129). Sem
agente: restaura e sai com o mesmo código. Só o grupo de um filho ainda não colhido recebe sinal
(achado do NS-29). **SIGINT não entra:** no raw mode o `Ctrl+C` é tecla, e o handler do init
(`IgnoreTerminalSignals`, NS-31) continua sendo o único dono dele enquanto o init roda.

**Panic:** o hook (já existente) liga uma flag, restaura o terminal, encerra os grupos dos agentes
ainda não colhidos (lista estática do `agent_session.rs`), chama o hook padrão e sai com 101. Ele
não mexe no banco: a limpeza da próxima abertura resolve. **Achado do NS-33:** sem a flag, a
thread principal, livre enquanto o hook espera o grupo morrer (até 2 s), recebia o fim do agente,
gravava `ended` e podia redesenhar sobre o terminal restaurado; com a flag
(`terminal::panicking`), o laço para de tratar eventos. `NICK_TEST_PANIC=agent` é um gancho só
de teste (`nick/tests/agent.rs`): panic numa thread pouco depois de um agente abrir.

**Achados do NS-29 reescritos no crate** (nada do `nick/` depende de `prototype/panel/`; o
código e os testes de lá foram a referência e a fonte dos casos de teste): `keys.rs` (teclas como
um xterm, colagem que descarta todo controle menos tab e CR dentro dos colchetes, mouse; sem
atalho reservado: `Ctrl+\` vai como 0x1c), `agent_session.rs` (PTY, `vt100` com o callback OSC 52
que só repassa cópia, thread leitora contínua, thread que colhe o filho, tela mínima 2x2,
encerramento por grupo) e o desenho com `tui_term::widget::PseudoTerminal`.

**Dependências novas** (`nick/Cargo.toml`): `portable-pty` 0.9 e `vt100` 0.16.2 passaram de
dev-dependencies para dependencies; entraram `tui-term` 0.3.4 (só funciona com o `vt100` 0.16) e
`nix` 0.31 com as features `signal` e `process` (o `killpg`; o 0.31.4 já estava no `Cargo.lock`,
nenhuma versão duplicada nova). O `signal-hook` continua no 0.3.18.

**Testes:** `bin/tests/test_nick_agent.sh` (achada pelo `make test`): argv exato do `ai-jail` e
do `claude` falsos calculado pela lib, jail ligada e desligada, dois projetos, `--check` sem
gravar nada (retrato recursivo), sem configuração, `.nick.conf` antigo, falhas do `jail_check`
(cada uma com `--check` e com o lançamento, `ai-jail` ausente com controle positivo), argumentos,
o diferencial com o `auto_scrum.sh`, estrutura e `shellcheck -x`. Na suíte Rust: unidade em
`db`, `agents`, `keys`, `agent_session` (PTY real com o launcher real e os falsos), `launch`,
`app` (com um `AgentLauncher` falso), `ui` e `terminal` (o portão); integração no PTY em
`nick/tests/agent.rs`. A verificação com o `claude` e o ai-jail reais não roda dentro da jail do
nick: o roteiro fica em `.nick/docs/NS-33/verificacao_manual.md`, para o analista executar fora
dela.

## Rust no projeto: vários agentes na casca `nick` (NS-34)

> **Mudou no NS-57** (seção "Rust no projeto: liberar e prender a jail de um agente (NS-57)"): a
> altura do cabeçalho **não depende mais da jail** (decisão do usuário). Ela é o máximo entre as
> linhas da versão na jail e as da versão fora dela, com o mesmo teto (um terço do painel); na
> prática, a da versão fora da jail. Na jail, as linhas que sobram ficam em branco. O tamanho do
> terminal continua dependendo do nome e da largura, só a jail saiu da conta:
> `terminal_in_panel(panel, name)`, `header_rows(name, panel)`, e o `TerminalAgentLauncher` perdeu o
> `jail` e o `with_jail`. O aviso fora da jail continua nunca cortado.

> **Mudou no NS-38** (seção "Rust no projeto: navegação em árvore (NS-38)"): a tela é sempre lado a
> lado, com ou sem agente vivo (a regra "sem agente vivo, as telas são as de antes, byte a byte"
> deixou de valer); o agente exibido é o do nó selecionado; a visão "Agentes ativos" virou o filtro
> `v` da árvore, e o `x` fica no nó do agente; o fim do agente exibido deixa a seleção no nó dele, e
> o atalho de foco vai ao último agente exibido; a altura fixa do rodapé passou a valer para os
> rodapés de cada tipo de nó (`HELP_STATES`, no lugar do `LIST_SCREENS`). O tamanho dos PTYs não
> mudou.

A fatia F3b do épico NS-28, que fecha o P1 do épico (processos simultâneos difíceis de manusear).
Vários agentes, de tickets e projetos diferentes, ficam vivos ao mesmo tempo, cada um no seu PTY,
com o cwd e a `.nick/nick.conf` do seu projeto. **Não há limite de agentes, de propósito.** A tela
do agente em tela cheia do NS-33 (`Screen::Agent`) deixou de existir: o agente fica ao lado da
lista.

**Layout lado a lado só com agente vivo** (decisão do usuário no plano). Sem nenhum agente vivo, as
telas são as de antes, byte a byte (a lista em largura total, a barra e o rodapé), menos o rodapé de
Módulos, que ganhou `c configurações`. Com pelo menos um agente vivo, a área acima da barra e do
rodapé se divide: à esquerda a lista da tela atual, com `ui::LIST_WIDTH = 40` colunas, mas no
máximo metade da janela; à direita o painel do agente exibido (desde o NS-37, uma borda em volta
do cabeçalho e do terminal: seção "Rust no projeto: foco evidente no painel (NS-37)"). A barra
e o rodapé continuam em largura total, com a quebra do NS-32, e os diálogos são centrados sobre a
área inteira, por cima do painel (só abrem com o foco na lista). **A conta tem fonte única:**
`ui::layout(app, area)` devolve as áreas, e o desenho, o mouse e o tamanho de cada PTY
(`ui::agent_area`) saem dela. Depois de cada evento que pode mudar o estado (tecla, resize, fim de
um agente, marca de saída nova), o laço recalcula a área de cada sessão e só chama `resize` quando
ela mudou. Assim uma mensagem longa na barra, que quebra em mais linhas, também redimensiona os
terminais. A tela de cada sessão continua com pelo menos 2x2.

**Com agente vivo, a altura do rodapé é fixa** (review do NS-34, P1). O rodapé reserva as linhas do
mais longo dos rodapés das listas (Módulos, Projetos, Tickets com e sem o filtro, Agentes ativos e
Configurações), com a contagem na forma mais longa (`N, N com saída nova`). O rodapé curto do
momento, como o `<atalho> voltar à lista`, é desenhado na primeira dessas linhas. Antes, cada
`Ctrl+\` na tela de Tickets levava o rodapé de 1 para 2 linhas, e o mesmo acontecia ao ir de
Tickets para Projetos ou para a visão. Cada mudança redimensionava todos os PTYs: todo `claude`
recebia um SIGWINCH e redesenhava, e os escondidos ficavam marcados `● saída nova` sem nada novo.
Agora só a barra com uma mensagem longa e um diálogo com rodapé mais longo que o das listas (numa
janela estreita) mudam a altura. Sem agente vivo, o rodapé ocupa as linhas de que precisa, como
antes. **O PTY de um agente novo nasce com o tamanho final** (P2). `ui::new_agent_panel` calcula,
pela mesma conta, o painel que o agente vai ter depois de aberto (um agente a mais, o foco no
terminal e a barra vazia). O `TerminalAgentLauncher` recebe uma função de tamanho e a chama no
spawn com o nome e com a jail que o próprio `--check` informou (`ui::terminal_in_panel`).

**O cabeçalho quebra em mais linhas** (decisão do usuário no NS-34). No painel de 60 colunas de um
terminal de 100, o aviso `<nome> · FORA DA JAIL: o agente roda direto no seu shell.` não cabe numa
linha. Ele nunca é cortado: quebra por palavra (`wrap_words`), até um terço do painel, e o
terminal encolhe. Por isso a altura do terminal depende do cabeçalho do agente: cada sessão tem o
tamanho do seu painel (`agent_area` calcula como se aquele agente fosse o exibido), e não um
tamanho só para todas, como o plano previa. Na lista de 40 colunas, um nome longo com
`[FORA DA JAIL]` ou `● saída nova` é cortado no fim da linha (achado do NS-34, aceito).

**Foco e atalho reservado.** O `App` tem `focus: Focus { List, Terminal }`, os agentes vivos em
`agents` (na ordem de abertura) e `shown`, o `id` do agente exibido. Abrir um agente o acrescenta,
o exibe e põe o foco no terminal, e a lista continua nos tickets, com o mesmo ticket. Com o foco no
terminal, toda tecla vai para o `claude` (inclusive `Ctrl+C`, `Esc` e `q`), menos o atalho, que
devolve o foco à lista, e o Ctrl+Z, descartado (NS-44). A colagem e o mouse também vão para o
terminal. Com o foco na lista, as teclas são as da tela, e nada vai para o `claude`. A captura do
mouse e o bracketed paste ligam com o foco no terminal e desligam na lista, onde o operador
seleciona texto do jeito nativo. O atalho é testado antes de tudo, menos da captura de um atalho
novo. Só o `Press` alterna: um `Repeat` é engolido, para segurar a tecla não piscar o foco. **Com um
diálogo aberto, o atalho é ignorado**, porque o diálogo é uma pergunta pendente. O
`keys::encode_key` não mudou: com o atalho trocado, o `Ctrl+\` volta a ir para o `claude` como 0x1c.
Quando o `claude` de um agente termina, a linha fica `ended`, a trava sai e a barra mostra a
mensagem do NS-33. Se ele era o exibido, passa a ser exibido o agente vivo aberto mais recentemente,
e o foco vai para a lista. Se não era, nada muda. A tela da esquerda não muda.

**`Shortcut` e a forma canônica** (`keys.rs`). O atalho é guardado e comparado numa forma que bate
com a tecla do jeito que o crossterm a entregar:
- um `Ctrl+<símbolo>` com byte de controle vira o símbolo do byte (`ctrl_byte`, sem o `Shift`).
  `Ctrl+4` e `Ctrl+\` são `Ctrl+\` (0x1c), `Ctrl+5` é `Ctrl+]`, `Ctrl+6` é `Ctrl+^`,
  `Ctrl+7` e `Ctrl+/` são `Ctrl+_`, `Ctrl+3` é `Ctrl+[`, e `Ctrl+2` e `Ctrl+Espaço` são `Ctrl+@`;
- uma letra com `Ctrl`/`Alt` vira minúscula, e o `Shift` vira modificador explícito;
- as outras teclas (F1–F12 e as nomeadas) ficam com o código e os modificadores. Só `Ctrl`, `Alt`
  e `Shift` contam: uma tecla com `Super` nunca é atalho. O `Shift+Tab` é `Tab` com `Shift`.

No banco, o texto é minúsculo, com os modificadores na ordem `ctrl+alt+shift+` e depois a tecla
(`ctrl+\`, `alt+x`, `ctrl+shift+f5`, `f7`, `alt+space`). O `Shortcut::from_db` só aceita a forma
que o `to_db` escreve: um `ctrl+4`, um modificador repetido ou fora de ordem e uma maiúscula são
recusados. O padrão é `Shortcut::DEFAULT` (`Ctrl+\`).

**Banco: migração 4, tabela `settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)`.** A chave do
atalho é `db::SETTING_RESERVED_SHORTCUT` (`reserved_shortcut`). Os métodos do `Db` são
`get_setting`, `set_setting` (`INSERT ... ON CONFLICT(key) DO UPDATE`) e `delete_setting`. **O
padrão só existe no código:** sem linha vale o padrão, e "voltar ao padrão" (`p`) apaga a linha.
Assim uma mudança futura do padrão vale para quem nunca trocou o atalho. A leitura acontece na
abertura do nick (`App::load_shortcut`, no `main.rs`, junto com a limpeza dos agentes). Um valor
que o `from_db` não entende, ou que hoje seria recusado, cai no padrão, e a barra mostra
`O atalho gravado no banco (<valor>) não é válido. Usando o padrão Ctrl+\.`. Um erro do banco
vai para a barra, e a TUI abre com o padrão. Se a limpeza também falhar, as duas mensagens se
juntam com um espaço. O `migration_1_is_pinned` fixa também o texto da migração 4. Os testes que
citavam a versão 3 como a última passaram a citar a 4. O teste do banco v2 → v3 passou a abrir só
com as migrações 1 a 3, como o do v1 → v2 no NS-33, sem mudar nenhuma asserção.

**Tela de Configurações e captura** (`c` em Módulos; o `ModuleKind` não mudou). A tela mostra
`Atalho para alternar entre a lista e o terminal: <label>`, com ` (padrão)` quando não há linha.
`t` abre a captura. A primeira tecla válida fica guardada, e a mesma tecla de novo (comparada na
forma canônica) é gravada, vale na hora e fica para as próximas aberturas. Uma tecla válida
diferente vira a nova primeira. Uma recusada mostra o motivo na barra e recomeça a captura. `Esc`
cancela sem gravar, e o `Esc` nunca pode ser atalho. **A segunda tecla prova que o terminal entrega
aquela tecla ao nick:** no VS Code sem a regra do `keybindings.json`, o `Ctrl+\` nunca chega, então
nunca é salvo. Na captura, o `Ctrl+C` é recusado e não fecha o nick. Escolher o próprio atalho
atual é aceito. **A lista de recusa** (`keys::shortcut_refusal`, sobre a forma canônica), com as
mensagens em PT-BR:
- `Enter`, `Esc`, `Tab`, `Shift+Tab`, `Backspace` e as setas, com qualquer modificador →
  `O claude precisa dessa tecla (<label>). Escolha outra.`;
- `Shift+PageUp`/`Shift+PageDown` (a rolagem do histórico do NS-33) →
  `O nick usa essa tecla para rolar o histórico. Escolha outra.`;
- uma tecla sem modificador, ou só com `Shift` → `Use uma tecla com Ctrl ou Alt (ou uma tecla
  de função, F1–F12).`. As teclas de função sem modificador são aceitas, porque o `claude` não as
  usa;
- os atalhos documentados do Claude Code (code.claude.com/docs/en/interactive-mode, conferido em
  2026-11-19): `Ctrl+` `A B C D E G J K L N O P R S T U V W X Y Z`, `Ctrl+_` e `Ctrl+[`; `Alt+`
  `B D F M O P T V Y`. Numa letra, a comparação ignora o `Shift`. Os que usam `Enter`,
  `Backspace` e as setas caem na primeira regra. Mensagem: a do `claude`;
- os sinônimos de byte, que chegam ao `claude` como outra tecla: `Ctrl+H` (Backspace), `Ctrl+I`
  (Tab), `Ctrl+M` (Enter) e `Ctrl+[` (Esc). Mensagem: a do `claude`.

A lista é a constante comentada `CLAUDE_SHORTCUTS` em `keys.rs`, com a fonte e a data, e deve
crescer quando o Claude Code ganhar atalhos. Ela não vai para o README (regra "README genérico").
Diferença do plano: o `shortcut_refusal` devolve `Option<String>`, e não `Option<&'static str>`,
porque a mensagem cita o label da tecla.

**VS Code.** O padrão continua `Ctrl+\`, que o terminal integrado do VS Code não repassa (achado do
NS-29). Há duas saídas, documentadas no README: (1) trocar o atalho na tela de Configurações, ou
(2) a regra do `keybindings.json` que manda o 0x1c (texto copiado do `prototype/panel/README.md`).

**Visão "Agentes ativos"** (`v` em Módulos, Projetos e Tickets, com o foco na lista e sem diálogo
aberto; funciona sem agentes e diz `Nenhum agente ativo.`). Ela lista **só os agentes deste
processo**, na ordem de abertura. Os `active` de outro `nick` aberto têm a trava do outro, e este
não tem o terminal deles. Cada linha é o nome (`projeto · etapa · ticket`), mais
`  [FORA DA JAIL]` em vermelho quando o agente roda sem jail e `  ● saída nova` quando está
marcado. A seleção começa no exibido, e mover a seleção exibe o agente selecionado na hora.
`Enter`/`→` põem o foco no terminal dele, `x` pede para encerrá-lo, e `Esc`/`←` voltam para a tela
de origem (`App::back`), com a mesma seleção. Com agente vivo, o rodapé de Módulos, Projetos e
Tickets ganha `v agentes ativos (N)` ou `(N, M com saída nova)`. O rodapé da visão mostra o label
do atalho atual, e com o foco no terminal o rodapé é só `<atalho> voltar à lista`.
**Se o agente roda na jail continua só em memória, e não vira coluna** (a decisão que o NS-33
deixou para a F3b): todos os agentes da visão são deste processo, e o valor vem do `--check` de
cada um.

**Aviso de saída nova** (regra escolhida pelo usuário no plano). Cada `RunningAgent` guarda
`seen_text`, o texto da tela (`vt100::Screen::contents()`) no momento em que ele deixou de ser
exibido, e `unseen`. Num `Output` de um agente escondido e ainda sem marca, o laço lê o
`contents()` atual, e `App::hidden_output` (função pura, testada sem PTY) o marca quando o texto
mudou. Um redesenho só de cor, de cursor ou que reescreve o mesmo texto não marca. Um agente já
marcado não recalcula nada até ser visto, e o exibido nunca lê o `contents()` a cada `Output`. A
marca some quando o agente passa a ser exibido, com o foco na lista ou no terminal. O laço só
redesenha por um escondido quando a marca muda: N agentes trabalhando não fazem a TUI redesenhar à
toa. **Falsos avisos conhecidos e aceitos:** um resize da janela faz o `claude` redesenhar com
outra largura, e o texto muda, então um resize pode marcar todos os escondidos. Também é marcado
um `claude` que muda o texto sem conteúdo novo, como o spinner ou um relógio.

**Eventos com o `id` do agente e OSC 52 só do exibido.** O `AgentSession::spawn` e o
`AgentLauncher::spawn` recebem o `id` da linha, que já existe antes do spawn. Os eventos passam a
ser `AgentEvent { agent, event: SessionEvent }`, e o `Shell` guarda `sessions: Vec<(i64,
AgentSession)>`. Cada sessão continua com a sua thread leitora e a sua thread que colhe o filho:
uma escondida que inunda a saída não trava (teste de PTY). O `Exited` de um agente que o operador
já encerrou não acha a sessão e é ignorado. **Só a cópia (OSC 52) do agente exibido vai para o
terminal de fora; a de um escondido é descartada.** Copiar é uma ação do operador na tela que ele
vê, e um agente escondido não pode trocar o clipboard sem que o operador perceba.

**Encerrar.**
- **Um agente pela visão:** `x` abre `Encerrar o agente <nome>? (s/N)`. Só `s`/`S` confirmam
  (`Action::EndAgent`), e qualquer outra tecla cancela. O laço tira a sessão, chama `terminate()`
  e depois `App::agent_ended_by_operator`, que grava `ended`, solta a trava, ajusta o exibido e o
  foco como no fim natural e mostra `O agente <nome> foi encerrado.`.
- **Ao sair com agentes vivos:** `q`/`Q` numa lista ou `Ctrl+C` com o foco na lista abrem
  `Há <N> agentes ativos (<projetos>). Encerrar todos e sair? (s/N)` (com um, `Há 1 agente ativo
  (<projeto>). Encerrar e sair? (s/N)`). Os projetos não se repetem e seguem a ordem de abertura.
  `s`/`S` → `Action::QuitEndingAgents`, e qualquer outra tecla cancela. **Sem agente vivo, `q` e
  `Ctrl+C` saem direto, como antes.**
- **Encerrar todos de uma vez:** `agent_session::terminate_all` manda SIGTERM a todos os grupos
  ainda não colhidos num único `end_groups` (uma espera de 2 s no total, e não 2 s por agente) e
  depois faz o `wait` de cada um. O `AgentSession::terminate` é o caso de um só. A mesma função
  serve à saída confirmada e ao SIGTERM/SIGHUP. Depois vem `App::end_all_records`, que grava
  `ended` e solta a trava de cada um, seguindo depois de um erro (guarda o primeiro e continua),
  para não deixar trava presa. O P1 do review do NS-33 (a saída por erro de I/O no laço sem
  encerrar os agentes) não foi corrigido antes do merge, então esse caminho continua sem
  encerramento, como no NS-33.
- o `terminate` é síncrono e pode segurar a TUI até 2 s com um grupo que ignora o SIGTERM. É um
  custo aceito, e não há thread de encerramento nesta fatia;
- o panic hook já encerrava todos os grupos registrados (`GROUPS`) e não mudou.
  `NICK_TEST_PANIC=second_agent` (só de teste) dispara o panic depois do segundo agente. O `agent`
  do NS-33 continua disparando depois do primeiro.

**Reaproveitamento e o que mudou no código do NS-33.** `agents.rs` não mudou. `launch.rs` só ganhou
o `agent_id` no `spawn`. `keys.rs` ganhou o `Shortcut` e a recusa, e o `encode_key` não mudou. O
`app.rs` não ganhou módulo novo: a visão e a captura ficaram nele, ao lado das outras telas, porque
são poucas funções que dependem do estado do `App`. Já as regras do atalho sem tela ficaram em
`keys.rs`. Os testes do NS-31 e do NS-32 passaram sem diff, menos o rodapé de Módulos. Os do NS-33
mudaram só onde o plano previa:
- `Screen::Agent`/`app.agent` viraram o foco e o exibido;
- o `Ctrl+4` do teste "toda tecla vai para o programa" virou `Ctrl+]`, porque agora ele é o
  atalho;
- `agent_ended` ganhou o `id`;
- o `spawn` e o casamento dos eventos ganharam o `id`;
- em `nick/tests/agent.rs`, o cabeçalho é lido no painel (`Nick::wait_for_panel`) e o tamanho do
  PTY é o do painel (`SIZE 25 60` e, a 40x120, `SIZE 36 80`, com as 2 linhas que o rodapé
  reserva).

O `claude` falso ganhou, sem mudar os modos existentes, o modo por projeto
(`FAKE_CLAUDE_MODE_<pasta>`), o log por projeto (`FAKE_CLAUDE_LOG_DIR`) e os modos guiados por
arquivo `trigger`, `osc52_on_trigger` e `flood_on_trigger`. O modo por projeto só vale quando o
nome da pasta é um nome de variável válido. Com `-` ou `.` (`meu-proj`, `proj.v2`), a expansão
indireta abortava o falso em qualquer modo (review do NS-34, P3). No modo `sleep`, o SIGHUP é ignorado no próprio
falso antes de criar o `sleep` em segundo plano, e não num `trap` dentro do filho. Com o `trap` no
filho, um SIGHUP ao grupo logo depois do `READY` podia chegar antes dele e matar o sobrevivente, e
os testes de "grupo já colhido" falhavam ao acaso (review do NS-34, P4; o padrão vinha do NS-33).

**Outros ajustes do review do NS-34:**
- as três confirmações `(s/N)` usam o mesmo `is_yes`;
- com o foco no terminal, a roda do mouse só rola o histórico sobre o terminal, e não sobre a
  lista ao lado;
- na saída por `q` + `s`, um erro do banco ao gravar os `ended` sai no stderr depois da
  restauração do terminal. O nick sai com 0, e as travas saem do mesmo jeito, então a limpeza da
  próxima abertura encerra a linha que ficou `active`.

**Testes:** unidade em `keys` (canonização, ida e volta do banco, label e recusa), `db` (migração
4, v3 → v4 e `settings`), `agent_session` (duas sessões no mesmo canal com o `id` certo e uma
inundação, e `terminate_all` numa espera só sem sinal a grupo colhido), `app` (vários agentes, foco,
atalho, visão, encerrar, sair, captura e leitura do banco, com o `FakeLauncher`) e `ui` (layout,
cabeçalho quebrado, visão, Configurações, captura e confirmações). Integração no PTY em
`nick/tests/agent.rs`: dois projetos (um com a jail e outro sem), o atalho nos dois sentidos, o
aviso de saída nova, o OSC 52 só do exibido, a troca do atalho com a próxima abertura, a recusa na
captura, encerrar pela visão, sair com dois agentes, SIGTERM e panic com dois agentes e a
inundação escondida. A verificação com o `claude` e o ai-jail reais, no terminal standalone e no VS
Code, não roda dentro da jail do nick. O roteiro fica em `.nick/docs/NS-34/verificacao_manual.md`,
para o analista executar fora dela.

## Rust no projeto: etapas do auto_scrum na casca `nick` (NS-35)

> **Mudou no NS-53** (seção "Rust no projeto: link do Jira montado pelo ID (NS-53)"): o `--check`
> imprime **quatro** linhas, com `jira_url=<valor>` por último, e o `parse_check` recusa as três
> de antes com `UNEXPECTED_CHECK_MESSAGE`. Entrou o modo leve `--jira-url`, só com a configuração
> (sem jail e sem as regras do claude). O item "Como o painel recebe as flags" vale também para a
> `JIRA_URL`: ela chega só pelo `bin/nick-agent`, e o Rust continua sem ler o `.nick/nick.conf`.

> **Mudou no NS-52** (seção "Rust no projeto: instalador local (NS-52)"): os logs do `--render`
> do nick saem do `auto_scrum/logs/` e vão para `$XDG_STATE_HOME/nick/logs/<nome>-<id>/`. O nick
> passa o diretório pelo `NICK_PROMPT_LOGS_DIR` de sempre e sobrepõe um valor herdado do ambiente.
> Os testes de integração passaram a esperar o log no state dir temporário. O `auto_scrum.sh` não
> mudou.

A fatia F4a do épico NS-28. O `g` sobre um ticket deixa de abrir o `claude` sem prompt e passa a
seguir o fluxo do `auto_scrum.sh`: os mesmos tipos de conversa, as mesmas perguntas, o mesmo
prompt, a mesma confirmação e o mesmo argv do `claude`.

**O fluxo:**
1. `g` (com as recusas de antes: ticket oculto e projeto indisponível) → `Action::CheckAgent` →
   `App::check_agent`: o launcher precisa ser arquivo, e o `bin/nick-agent --check` precisa
   passar. Uma falha vira o alerta "Agente não aberto", com `Nada foi aberto.` no topo, como no
   NS-33;
2. o menu "Abrir agente" com os 8 tipos do `choose_type`, na mesma ordem e com os mesmos textos,
   filtrados pelas flags do `--check` (`StageType::menu`);
3. o formulário do tipo, com as perguntas do `ask_questions_<tipo>` (`StageType::fields`): os
   campos de uma linha são digitados no próprio diálogo (`app::edit_text`, `Tab`/`Shift+Tab`/
   `↑`/`↓` trocam de campo), e a descrição longa abre no `$EDITOR` (`Action::EditDescription`).
   `Enter` num campo de uma linha valida tudo (`stages::render_args`): a primeira recusa vai para
   a barra, com o foco no campo recusado. `Esc` fecha sem gravar nada;
4. `Action::RenderPrompt` → `bin/nick-agent --render <tipo> <respostas>`, fora do PTY, que monta o
   prompt pelo bash e grava o log;
5. a prévia (`Dialog::ConfirmPrompt`), sobre a tela inteira acima da linha de ajuda (limpa toda,
   para nada da lista aparecer por trás), numa caixa com margem de uma célula: no topo, o
   status da jail (fora dela, `FORA DA JAIL: o agente roda direto no seu shell.` em vermelho e
   negrito, como no cabeçalho do agente, e depois as linhas do `jail_print_status`) e os avisos
   do `--render` (o plano no lugar antigo, a configuração); no meio, o prompt com quebra por
   palavra (mantendo a indentação; o tab vira espaços até a próxima coluna múltipla de 8, só na
   prévia, para não aparecer como `?` do `printable`) e rolagem (`↑/↓/PgUp/PgDn/Home/End`); embaixo,
   `linhas a–b de N`, `Confirma o envio para o claude? (s/N)` e `Salvo em <log>`. As teclas ficam
   só na linha de ajuda, como nos outros diálogos. A rolagem fica presa ao começo e ao fim pela
   janela (linhas por página, rolagem máxima) que o laço calcula com o `ui::prompt_window` (a
   mesma conta do desenho) e passa ao `App` antes de cada tecla (`App::set_prompt_window`), no
   molde do `set_agent_term`: o `App` não depende do `ui` (review do NS-35). A quebra
   (`wrap_keeping_spaces`) é linear, medida sobre fatias do texto, porque uma descrição longa
   chega a 128 KiB e a conta roda a cada tecla. Só `s`/`S` (`is_yes`) lançam; `n`/`N`/`Esc` fecham com `Cancelado. O
   prompt gerado continua salvo em: <log>` (o texto do script), e as outras teclas são ignoradas;
6. `Action::OpenAgent` → `App::open_agent`, como no NS-34 (sessão, nome, trava, linha `active`,
   spawn e foco no terminal), mas **sem rodar o `--check` de novo**: usa o `Precheck` guardado
   desde o passo 1 (a jail e o aviso). O lançamento de verdade repete as validações no bash.

Com um diálogo aberto, o atalho reservado continua ignorado (NS-34), `q` não sai, e o `Ctrl+C`
com o foco na lista continua saindo (ou perguntando, com agentes vivos).

**Onde a montagem do prompt passou a morar e por quê (D1).** O `auto_scrum/prompt.sh`, novo, é
carregado com `source` pelo `auto_scrum.sh` e pelo `bin/nick-agent`. Tem as globais do prompt
(as respostas, os blocos e os nomes da árvore `.nick/`), o `PROMPT_TEMPLATES_DIR` e o
`PROMPT_LOGS_DIR` (calculados pelo `BASH_SOURCE`), as funções movidas **com o corpo idêntico**
(`jira_id_valid`, `warn_legacy_plan`, os sete `build_*_blocks` e o `exchange_block`, com os
comentários) e duas funções novas: `build_prompt_blocks`, que chama os sete `build_*_blocks` na
ordem de antes, e `render_prompt <templates_dir> <tipo> <log>`, com o `export`, o `envsubst` (a
mesma lista e o mesmo `SC2016`) e o `exchange_block` que estavam no `main()`. O `main()` chama
as duas. O arquivo **não fica em `lib/`, de propósito**: as regras de `lib/` exigem prefixo por
biblioteca e proíbem ler globais do chamador, e cumpri-las seria reescrever cada `build_*_blocks`
com parâmetros e `printf -v`, que é o risco que o ticket apontava. O prompt também é domínio do
auto_scrum (os templates moram em `auto_scrum/templates/`). Ele segue, no espírito de `lib/`: 1ª
linha `# shellcheck shell=bash`, sem shebang, sem bit de execução e sem `set -euo pipefail`; ao
ser carregado só define (globais, constantes e funções), sem imprimir nem rodar comando além do
`$(cd …; pwd)` do `BASH_SOURCE`; não chama `exit`, `source` nem `eval`; e depende de
`lib/config/config.sh` já carregado. O ticket de limpeza do `auto_scrum.sh` pode rever o lugar.
**Prova antes/depois:** o `.nick/docs/NS-35/gerar_prompts.sh` gera, pelo `main()` do script, o
log dos 8 tipos em cada combinação da matriz de flags (41 prompts). Os gerados com o `main` de
antes e com o NS-35 deram `diff -r` vazio.

**A interface nova do `bin/nick-agent` (D2)**, três modos (`Uso: nick-agent --check | --render
<tipo> [respostas] | --session-id <uuid> --name <nome> --type <tipo> --prompt-file <arquivo>`):
- `--check`: o mesmo de antes, mas o stdout de sucesso tem **exatamente três linhas, nesta
  ordem**: `jail=true|false`, `jira=true|false` e `sentry=true|false`, cada uma `true` só com
  `"true"` exato no `.nick/nick.conf` (a regra do `choose_type` e do `jail_exec`). O
  `launch::parse_check` exige as três, nessa ordem, com ou sem o `\n` final. Qualquer outra forma,
  inclusive a linha `jail=` sozinha do NS-33, dá `UNEXPECTED_CHECK_MESSAGE`: nada é presumido;
- `--render <tipo> [--title|--description|--jira-id|--jira-link|--sentry-link|--plan-path <v>]`,
  cada valor num argumento só, em qualquer ordem. Recusa (`usage_error`, saída 1, nenhum log):
  tipo fora dos 8 (`AGENT_TYPES`, os do `choose_type`, conferidos por teste), tipo indisponível
  pelas flags (a regra do `choose_type`), resposta que não pertence ao tipo, repetida, sem valor,
  obrigatória ausente ou vazia (numa resposta que o script lê com `read`, uma feita só de espaço,
  tab e `\n` conta como vazia, porque o `read` a deixaria vazia; a descrição longa, que vem do
  `read_via_editor` e é aceita como está, só não pode ser vazia — review do NS-35), e ID que o
  `jira_id_valid` recusa (a mensagem do `read_jira_id`
  sem o "Tente de novo."). Roda o `load_and_check`, aplica no Dev e no Review o
  `warn_legacy_plan` (stderr) e as mesmas linhas do plano padrão e do `REVIEW_PATH` do
  `ask_questions_*`, o `build_prompt_blocks` e o `render_prompt`. O stdout é `log=<caminho
  absoluto>` e depois a saída do `jail_print_status` (o status que o script mostra antes da
  confirmação). Não grava nada no projeto (sem `config_dir_prepare`) e não abre o `claude`;
- o lançamento exige os quatro `--session-id`, `--name`, `--type` (um dos 8) e `--prompt-file`
  (arquivo regular, legível e não vazio), em qualquer ordem. O argv é o do `auto_scrum.sh` com
  `--session-id <uuid> --name <nome>` antes do prompt: `claude --settings <regras>
  [--permission-mode auto só no development] [--append-system-prompt <aviso> só com a jail]
  --session-id <uuid> --name <nome> "$(cat <arquivo>)"` (desde o NS-41, o
  `--append-system-prompt` existe sempre e começa pela instrução de não contornar uma recusa:
  ver "Instrução de não contornar uma recusa (NS-41)"). O prompt é o último argumento, lido
  como o script lê (o `$(cat …)` tira os `\n` do fim). O modo sem prompt do NS-33 deixou de
  existir.

**O log dos testes (`NICK_PROMPT_LOGS_DIR`).** O log do `--render` fica no `PROMPT_LOGS_DIR`, o
`auto_scrum/logs/` do checkout, que tem dados reais. Os testes do launcher e da casca rodam o
`bin/nick-agent` real (a casca acha o launcher por um caminho fixado na compilação, sem variável
que o troque), então rodar numa cópia temporária do `bin/`+`lib/`+`auto_scrum/` não serve para
os testes de PTY. A saída mais simples: o `prompt.sh` aceita a variável de ambiente
`NICK_PROMPT_LOGS_DIR`, que, quando é um caminho absoluto, troca o `PROMPT_LOGS_DIR` (relativo ou
vazio é ignorado). Todo teste que roda o `--render` a aponta para um temporário, e a suíte bash
confere no fim que o `auto_scrum/logs/` real não mudou. Ela só escolhe onde o log é gravado, não
o que é lançado (diferente do launcher, que nenhuma variável troca). O `auto_scrum.sh` não a usa:
ele continua com o `LOGS_DIR` dele.

**Como o painel recebe as flags:** só pelo `--check`. O Rust não lê o `.nick/nick.conf`, não lê
template e não monta prompt: ele coleta as respostas, mostra o log (ler o log para a prévia não
é montar prompt) e lança pelo launcher. Um teste estrutural (`nick/tests/agent.rs`,
`no_rust_source_builds_the_prompt_nor_reads_the_config`) proíbe em `nick/src/` (fora de
comentário) `templates/`, `envsubst`, `exchange_block`, `build_`, `_BLOCK`, o nome de um template
(`po.md`, …) e um `read_to_string`/`fs::read` do `nick.conf`, com controle positivo de cada filtro.
(Desde o NS-52, o `envsubst` é isento só em `src/deps.rs`, que o procura no PATH.)

**Os tipos e os formulários (`nick/src/stages.rs`).** Um módulo sem tela, no molde do
`tickets.rs`: `StageType` (8 variantes) com `key()` (o `$TYPE`), `label()` (o texto do menu),
`stage()` (a `Stage` do banco: `po*` → PO, `tech_leader*` → Tech Leader), `menu(jira, sentry)` e
`fields()` (rótulo, argumento do `--render`, regra e se é linha ou editor). As regras são as do
script: as linhas são aparadas como o `read -r` com o `IFS` padrão (`trim_read`: espaço, tab e
`\n` nas pontas, e **não** o `trim()` Unicode: o `read` não tira o NBSP), um obrigatório vazio
(ou só com espaços) é recusado com `Esse campo é obrigatório: <rótulo>.`, um opcional vazio não é
enviado, e o ID passa pelo `tickets::jira_id_valid` (recusa: `tickets::INVALID_ID_MESSAGE`). O
texto do editor **não** é aparado (o `$(cat …)` só tira os `\n` do fim, e o `read_via_editor`
não apara). O plano vazio vai sem `--plan-path`, e quem calcula `.nick/docs/<ID>/plan.md` é o
bash; o rótulo do plano mostra esse padrão com o ID digitado no momento.

**O ID vem do ticket, editável, e o nome do agente fica com o ID do ticket.** O campo do ID nasce
com o `ticket.key`. Se o operador o troca, o prompt usa o ID digitado, mas o nome do agente
continua `<projeto> · <Etapa> · <ID do ticket>` (a troca do ID do ticket é da F4b; desde o
NS-36, depois de um "Editar ID", os agentes novos usam o ID novo, e os já registrados mantêm o
nome antigo). O banco não
mudou (nenhuma migração): `agents.stage` continua com a `Stage`, e o tipo não é gravado
(histórico e etapa atual são do NS-36).

**O `$EDITOR` pela TUI (D8).** O caminho do init foi generalizado sem mudar o comportamento dele:
`terminal::run_suspended(guard, gate, f)` fecha o portão da thread de entrada, suspende a TUI e
reabre o portão, e o `IgnoreTerminalSignals` foi para o `terminal.rs`. O `TerminalInitRunner` usa
essa função, e os testes do init passaram sem mudança. O `editor.rs` (novo) tem o trait
`EditorRunner` (o `App` é testado com um falso) e o `TerminalEditorRunner`: o editor é o
`$EDITOR`, ou o `nano` quando ela não está definida ou está vazia (`${EDITOR:-nano}`, lido só no
`main.rs`), rodado como **um** programa, sem dividir por espaço (um `EDITOR="code -w"` falha nos
dois). Ele roda pelo `bash` (`command -v -- "$0" || exit 127; exec "$0" "$1"`), o único programa
que a casca abre com `Command` (regra estrutural do NS-33), que também o acha no `PATH` como o
script. Imprime `<rótulo> — abrindo <editor> (salve e feche o editor para continuar).`, abre um
temporário (`tempfile::NamedTempFile`, sempre apagado) e lê o arquivo de volta. Arquivo com
tamanho 0 → `Empty`; senão o texto sem os `\n` do fim; não UTF-8, editor ausente (`comando não
encontrado`), sem permissão, que sai com código ≠ 0 ou morto por sinal → `Não consegui abrir o
editor <editor>: <motivo>.` na barra. Sem "Pressione Enter" na volta. O nick ignora o `Ctrl+C` e
o `Ctrl+\` enquanto o editor roda (só o editor os recebe), como no init. **Diferenças do script,
de propósito:**
- sem laço no vazio: o script reabre o editor até ter texto, e só sai com `Ctrl+C`. No painel não
  há `Ctrl+C` para sair do laço, então o campo fica vazio, a barra mostra `Esse campo é
  obrigatório, tente de novo.`, e o operador decide (`Enter` reabre, `Esc` desiste). Um arquivo
  só com quebras de linha (não vazio para o `-s` do script, mas vazio depois do `$(cat …)`) conta
  como vazio no painel: o `--render` recusaria a descrição obrigatória vazia;
- reabrir um campo já preenchido abre o editor **com o texto atual** (o script nunca reedita);
- enquanto o editor está aberto, o laço fica parado no `run_suspended`: os agentes continuam
  rodando, os eventos de saída se acumulam no canal e são tratados na volta, com o aviso de saída
  nova acumulado, e um SIGTERM/SIGHUP também só é tratado na volta (impacto aceito no ticket);
- um texto salvo leva o foco ao próximo campo (dando a volta para o primeiro), onde o `Enter`
  monta o prompt.

**O modo `auto` do Dev no painel** é decidido no bash, pelo `--type` (seção "Modo auto só na
etapa de Desenvolvimento"). O `AgentLauncher::spawn` e o `AgentSession::spawn` ganharam o tipo e
o arquivo do prompt, que viram `--type <key> --prompt-file <log>` no `CommandBuilder`, que
continua sendo `bash <launcher>`. O `AgentLauncher` ganhou o `render` (`launch::run_render`, que
roda o `--render` com o stdin em `/dev/null`, lê a saída com `parse_render` — falha: o stderr ou
o código; sucesso: a 1ª linha `log=<absoluto>` e o resto como status, senão `O bin/nick-agent
respondeu algo inesperado na montagem do prompt.` — e lê o log para a prévia; ilegível ou não
UTF-8 é erro com o caminho). O `TerminalAgentLauncher` recebe a jail do `Precheck` guardado
(`with_jail`), para o PTY nascer com o tamanho final do painel (NS-34). O `tempfile` passou de
dev-dependency para dependency (já estava no `Cargo.lock`; nenhum crate novo).

**Convivência com o `auto_scrum.sh` até a limpeza.** Os textos do menu e os rótulos dos campos
ficam duplicados (Rust e bash). Amarram os dois: o teste do menu (`stages.rs`) roda o
`choose_type` real nas 4 combinações de Jira e Sentry (com `COLUMNS=1`, para o `select` listar um
item por linha) e compara com o `menu()`; e cada rótulo de campo precisa aparecer no
`auto_scrum.sh` (o do plano pelo começo, `Arquivo do plano (opcional, Enter para usar `), com
controle positivo. A lista dos 8 tipos do launcher (`AGENT_TYPES`) é comparada com a do
`choose_type`. **Diferenciais:** o bash (`bin/tests/test_nick_agent.sh`) roda, para os 8 tipos e
a matriz de flags (tudo desligado, tudo ligado com a jail e `STACK_DESCRIPTION`, e cada uma de
Jira, Sentry, produção, TDD e permissão sozinha), o `main()` do script e o launcher (`--render` e
lançamento) e compara o log byte a byte, o argv do `claude` (tirando `--session-id`/`--name`) e
as opções do `ai-jail`; o Rust (`nick/tests/stages.rs`) monta as respostas pelo `stages.rs`, com
espaços nas pontas, chama o `launch::run_render` e o `AgentSession::spawn` reais e compara com o
script.

**Ajustes do review do NS-35:** o `--render` recusa a resposta só com espaços (acima, com casos
novos na suíte e o controle positivo da descrição longa só com espaços, aceita como no script);
as falhas do launcher fora do PTY têm uma fonte só (`launch::launcher_failure`, usada pelo
`parse_check` e pelo `parse_render`), o alerta "Nada foi aberto." também (`App::agent_failed`, do
`--check` e do `--render`), e o launcher sem PTY do laço também (`Shell::offline_launcher`); o
`App` deixou de chamar o `ui` (a janela da prévia vem do laço); a quebra da prévia ficou linear;
o tab aparece como espaços na prévia; e a prévia ocupa a tela acima da ajuda, limpa, com as teclas
num lugar só. As telas antes desses ajustes ficaram em `.nick/docs/NS-35/prints_antes_do_review/`.

**Achados, sem correção** (os dois últimos herdados do script):
- a variável `NICK_PROMPT_LOGS_DIR` vale também em produção: um operador que a tenha no ambiente
  vai ter os logs do painel fora do `auto_scrum/logs/` (observação do review, aceita: ela só
  escolhe onde o log é gravado);
- dois `--render` do mesmo tipo no mesmo segundo usam o mesmo nome de log
  (`<tipo>_<AAAAmmdd_HHMMSS>.md`), e o segundo sobrescreve o primeiro (D9);
- o prompt vai como argumento do `claude`, e a descrição longa como argumento do `--render`. O
  Linux limita cada argumento a 128 KiB (`MAX_ARG_STRLEN`), como no script (impacto aceito no
  ticket). Se o `--render` ou o lançamento falharem por isso, o erro do sistema vai para o alerta
  ou para a barra. Nenhum mecanismo novo (D10).

**Testes adaptados, com o motivo:**
- `auto_scrum/tests/test_auto_scrum.sh`: os testes do bloco `export`, da linha do `envsubst`, da
  lista do `envsubst` × templates × `export` e da extração das chamadas `build_*_blocks` passaram a
  ler o `prompt.sh` (o texto mudou de arquivo, as asserções não); "as únicas linhas source"
  espera cinco; os dois testes de ordem do `main()` (`jail_check < … < build_stack_blocks < …`)
  passaram a procurar `build_prompt_blocks`, que é a chamada que o `main()` faz agora;
- `bin/tests/test_nick_agent.sh`: os casos de lançamento passaram a passar `--type` e
  `--prompt-file` (o prompt é o último argumento esperado); os do `--check` esperam três linhas;
  os de argumentos ganharam os casos novos; o diferencial do NS-33 virou o da matriz; e "o launcher
  nunca passa `--permission-mode`" virou "uma única linha, condicionada a `development`", com
  controle positivo;
- `nick/src/launch.rs`: `jail_true_and_jail_false_are_read`, `any_other_stdout_with_exit_0_is_an_error`,
  `the_stderr_of_a_success_is_a_warning` e `a_failure_is_the_stderr_or_the_code` usam as três
  linhas (os formatos recusados continuam recusados, e entraram ordem trocada, linha faltando,
  valor não exato e linha a mais);
- `nick/src/agent_session.rs`: o `Case` dos testes grava um prompt e passa `--type`/`--prompt-file`;
  `the_launcher_runs_claude_with_the_session_id_and_the_name_in_the_project` espera o prompt depois
  do `--name`;
- `nick/src/app.rs`: os testes do diálogo das etapas (`ChooseStage`) viraram os do menu, do
  formulário, do editor e da prévia; o helper `open_agent` percorre o fluxo novo (a etapa PO abre
  pelo `po_discussion`, de uma linha), e os testes do NS-33/NS-34 que o usam passaram sem mudar as
  asserções. O alerta do launcher ausente e o do `--check` com falha passaram a ser provocados pelo
  `g` (é ali que o `--check` roda agora), com as mesmas asserções; `with_a_dialog_open_the_shortcut_is_ignored`
  abre o menu novo no lugar do diálogo antigo;
- `nick/src/ui.rs`: o teste do diálogo das 4 etapas virou o do menu dos tipos; o `NoSpawn` passa o
  `Precheck` ao `open_agent`;
- `nick/tests/agent.rs`: o helper `open_agent` escolhe o tipo, preenche o formulário (o editor
  falso no Tech Leader) e confirma na prévia. Só `an_agent_out_of_the_jail_runs_claude_in_the_project_and_ends`
  mudou duas asserções, porque o comportamento mudou de propósito: o argv termina com o prompt (e
  não mais com o `--name`), e o Dev agora tem `--permission-mode auto`. Os testes do init
  (`nick/tests/tui.rs`, inclusive o do `Ctrl+C`) não mudaram;
- `tests/test_cargo_suite.sh`: o `shellcheck -x` cobre também o editor falso.

**Testes novos:** `stages.rs` (menu nas 4 combinações, `key`/`label`/`stage`/`fields`, o aparo do
`read`, a validação, os pares do `--render`, o diferencial do menu e os rótulos), `launch.rs`
(`parse_check` com as três linhas, `parse_render`, `run_render` com o launcher real e o log
ilegível), `editor.rs` (o editor real com editores falsos: texto, vazio, temporário apagado,
ausente, código ≠ 0 e não UTF-8), `terminal.rs` (o portão do `run_suspended`), `app.rs` (check
antes do menu, menu pelas flags, ID do ticket editável, recusas com a barra e o foco, editor
`Text`/`Empty`/`Failed`/reabertura, os pares do `render` nos 8 tipos, o `s` sem segundo check, o
cancelamento e os limites da rolagem), `ui.rs` (menu, formulário, rótulo do plano, prévia com o
`FORA DA JAIL` em vermelho, aviso, rolagem e terminal estreito) e, no PTY (`nick/tests/agent.rs`):
o menu com e sem Jira/Sentry, o `po_discussion` de ponta a ponta (com rolagem e o prompt como
último argumento), o Dev em modo `auto`, o cancelamento na prévia, o `po` com o editor falso
(`nick/tests/fixtures/bin/editor`), o editor que deixa o arquivo vazio e o `Ctrl+C` dentro do
editor, que só interrompe o editor.

A verificação com o `claude` e o ai-jail reais não roda dentro da jail do nick. O roteiro fica em
`.nick/docs/NS-35/verificacao_manual.md`, para o analista executar fora dela.

## Rust no projeto: etapa do ticket, histórico e edição do ID (NS-36)

> **Mudou no NS-61** (seção "Rust no projeto: etapa Rejeitado (NS-61)"): as etapas
> passaram a ser **sete**, com **Rejeitado** (`rejected`) depois de Concluído. O `m` para Rejeitado
> pede um motivo obrigatório (até 120 caracteres, `Dialog::RejectReason`) antes da confirmação, e o
> motivo vai para a coluna nova `ticket_stage_changes.reason` (migração 11) e aparece no histórico,
> na linha `Motivo:` abaixo da mudança. O `tickets::change_stage` e o `Db::change_ticket_stage`
> ganharam o parâmetro `reason`, e o `ConfirmStageChange` ganhou `reason` e `live`. O corte do
> histórico passou a contar linhas.

> **Mudou no NS-45** (seção "Rust no projeto: etapa Aberto, cores e avanço ao abrir agente
> (NS-45)"): as etapas passaram a ser **seis**, com **Aberto** (`open`) antes de PO, e os tickets
> novos nascem em Aberto. A regra "o nick nunca muda a etapa sozinho" foi **substituída**: abrir um
> agente **novo** de uma etapa **posterior** à do ticket move o ticket para essa etapa
> (`tickets::advance_on_agent_opening`, só no `App::open_agent`, depois do `spawn`), e o histórico
> guarda a origem de cada mudança (`operator` ou `agent_opening`). O fim do agente, o encerramento,
> a saída, a retomada e a troca de foco continuam sem mexer na etapa. O estrutural virou
> `the_stage_is_only_written_by_the_operator_or_by_opening_a_new_agent`, e o histórico vazio diz a
> etapa atual (`Nenhuma mudança: o ticket começou em <etapa>.`).

> **Mudou no NS-40** (seção "Rust no projeto: épicos opcionais (NS-40)"): o "Editar ID" recusa
> também o ID de um épico do projeto, e o miolo (renomear, gravar, desfazer) virou
> `tickets::rename_key_with`, usado também pelo "Editar ID" do épico. O rodapé do nó do ticket
> foi encurtado para caber o `p` (`m etapa · i ID · p épico`), sem mudar a altura reservada.

> **Mudou no NS-39** (seção "Rust no projeto: retomar e arquivar agentes (NS-39)"): o "retomar é
> da F5" foi feito. Um agente parado é retomado pelo `r` no nó dele, e um agente parado depois de
> um "Editar ID" é avisado, na retomada, da pasta nova `.nick/docs/<ID novo>/` (o ID de abertura
> fica em `agents.opened_key`). A edição do ID continua recusada só com agente `active`.

> **Mudou no NS-38** (seção "Rust no projeto: navegação em árvore (NS-38)"): a tela do ticket virou
> o painel do nó do ticket, à direita, sem a seção "Agentes do ticket" (os agentes ficam abaixo do
> ticket na árvore, e `Enter` num agente vivo põe o foco no terminal dele). `m`, `i` e `g` ficam no
> nó do ticket, com as mesmas regras, recusas e confirmações.

A fatia F4b do épico NS-28. O ticket ganha uma **etapa atual**, com histórico, uma **tela do
ticket** e a ação **"Editar ID"**, que renomeia a pasta `.nick/docs/<ID>/`.

**As cinco etapas e quem as muda.** `PO`, `Tech Leader`, `Dev`, `Review` e `Concluído`
(`tickets::TicketStage`; as quatro primeiras reaproveitam a chave e o rótulo de `agents::Stage`,
e a `Done` é a única só do ticket). A etapa é **só um registro**: o operador a muda para
qualquer uma, para a frente ou para trás, pela tela do ticket (`m`), com confirmação `(s/N)`.
**O nick nunca muda a etapa sozinho**: abrir um agente, o fim dele, encerrá-lo pelo operador ou
ao sair não tocam a etapa nem o histórico, e nada impede abrir um Dev com o ticket em PO. A
etapa também não fecha nem esconde agente nenhum (mudar para Concluído deixa o agente vivo do
ticket como estava). O único caminho que grava a etapa é `tickets::change_stage`, chamado só
pela confirmação do diálogo (`Dialog::ConfirmStageChange`). Um teste estrutural
(`the_stage_is_only_written_by_the_confirmation_of_the_operator`, em `nick/tests/agent.rs`,
com controle positivo) cobra isso no código de produção: `change_ticket_stage` só no
`db/mod.rs` e no `tickets.rs`, `change_stage` numa única chamada no `app.rs` dentro da
confirmação, e nenhum `UPDATE tickets SET stage` nem `ticket_stage_changes` fora de `src/db/`.

**Banco: migração 5.** `tickets.stage TEXT NOT NULL DEFAULT 'po'` com `CHECK` nas cinco chaves
(`po`, `tech_leader`, `development`, `review`, `done`), e a tabela `ticket_stage_changes (id,
ticket_id → tickets ON DELETE CASCADE, from_stage, to_stage, changed_at)`, com o mesmo `CHECK`
nas duas etapas e `from_stage <> to_stage`. Os tickets que já existiam ficam em `po` pelo
`DEFAULT`, e um ticket novo nasce lá (o `insert_ticket` não mudou). **Por que `CHECK` aqui e
não no `agents.state`:** as cinco etapas são o fluxo do auto_scrum, e não um conjunto que a F5
vai aumentar; uma etapa nova viria com uma migração que recria a coluna. O histórico guarda
**só as mudanças** (a criação já tem o `created_at`; um ticket sem mudança tem o histórico vazio,
"começou em PO"), sem autor (um operador só), e sai com o ticket, que sai com o projeto (a
confirmação de remoção do projeto não mudou). A mudança é `Db::change_ticket_stage`, numa
transação (`unchecked_transaction`, porque o `Db` tem `&self`): `UPDATE ... WHERE id = ? AND
stage = <origem>` e o `INSERT` do histórico entram juntos ou nenhum entra (provado com um
gatilho de teste que faz o `INSERT` falhar). Com 0 linhas, nada é gravado, e o resultado diz se
o ticket sumiu (`O ticket <ID> não existe mais no banco.`) ou se outro nick mudou a etapa nesse
meio-tempo (`A etapa do ticket <ID> mudou em outro nick. Nada foi gravado.`). A mesma etapa é
recusada (`O ticket <ID> já está em <etapa>.`). Uma chave desconhecida no banco (o `CHECK`
impede) aparece crua na tela, sem `panic!`.

**Tela do ticket** (`Screen::Ticket`; `Enter`/`→` na lista, inclusive num oculto com o filtro;
`Esc`/`←` voltam com o mesmo ticket selecionado). Mostra o ID, o título e as marcas, a etapa
atual, o plano e o review (`Plano (existe):  .nick/docs/<ID>/plan.md` e o mesmo para o
`review.md`, com `(não existe)`; o estado vem antes do caminho para não ser cortado a 40
colunas, como a etapa logo depois do ID na lista: P3 do review do NS-36),
o histórico do mais antigo para o mais novo (data `AAAA-MM-DD HH:MM UTC`, por fatia de string,
sem crate de data; um valor fora do formato aparece cru; se não couber, as mais novas e
`(+N mudanças mais antigas)` no topo da seção) e os agentes do ticket de todas as etapas, numa
lista com seleção: `● vivo` (este nick o roda), `ativo em outro nick` (linha `active` sem
sessão aqui) ou `encerrado`. `Enter`/`→` num vivo o exibe com o foco no terminal, **qualquer que
seja a etapa**; num não vivo, a barra diz `O agente <nome> não está rodando neste nick.` (retomar
é da F5). `m` muda a etapa, `i` edita o ID, `g` abre um agente (a mesma função `choose_stage`
do `g` da lista, que passou a receber o ticket, com as mesmas recusas), `v` abre "Agentes
ativos" (o `Esc` dela volta para a tela do ticket) e `q`/`Ctrl+C` seguem as regras de sair. A
tela é relida depois de cada ação e quando um agente abre ou termina (o ticket pelo
`Db::get_ticket`, pelo `id`, sem reler a lista do projeto: P3 do review do NS-36); um ticket que
sumiu do banco volta para a lista com a mensagem. Com agente vivo, a tela fica na coluna da esquerda
(40 colunas), e o fim das linhas longas é cortado, como na lista (achado do NS-34).

**Artefatos lidos do disco na hora** (`nick/src/docs.rs`, novo): `docs::artifacts` faz um
`is_file()` de cada arquivo a cada atualização da tela, sem cache, sem ler o conteúdo e sem
copiar nada para o banco (uma pasta chamada `plan.md` não conta). **Aviso de plano ausente** no
formulário do Dev e do Review (NS-35): com o campo do plano vazio, um ID válido no campo do ID e
sem `.nick/docs/<ID>/plan.md`, o formulário mostra em amarelo, acima dos campos, `Aviso: não
existe .nick/docs/<ID>/plan.md. Informe outro caminho no campo do plano, ou siga sem ele.`. A
conta roda ao abrir o formulário e a cada tecla nele (um `stat`), e fica no estado do diálogo
(`plan_missing`): o `ui.rs` não lê o disco. Com um caminho digitado no plano não há checagem (o
script também não confere), e com o ID vazio ou inválido não há aviso (a validação já recusa).
**O aviso não impede nada**: `Enter` segue para o `--render`, e o `warn_legacy_plan` do bash
continua na prévia.

**Fonte única do caminho do plano** (P2 do review do NS-36): o rótulo do campo do plano do
NS-35 (`stages::field_label`) passou a montar o `.nick/docs/<ID>/plan.md` pelo
`docs::plan_path`, como o aviso do plano no mesmo formulário. A saída não mudou: o teste do
rótulo do NS-35 passou sem mudança.

**Concluído não oculta.** A lista mostra a etapa logo depois do ID (`<ID>  [<etapa>]  <título>
[provisório]  [oculto]`), porque com agente vivo a lista tem 40 colunas e o fim da linha é
cortado. Os concluídos aparecem com `[✓ Concluído]` em verde e **continuam na lista**: só o `o`
os oculta, e o filtro não mudou.

**"Editar ID"** (`i` na tela do ticket; `tickets::edit_id`, com o `edit_id_with` que recebe a
gravação do banco por parâmetro para os testes, no molde do `create_with`). Vale para um ticket
provisório ou com ID do Jira. Ordem, e cada recusa sai sem mudar nada:
1. o ID novo é aparado como na criação; vazio → `Informe o novo ID do ticket.`;
2. `jira_id_valid` → `INVALID_ID_MESSAGE`;
3. igual ao atual → `O ID não mudou.` (outra caixa é outro ID: `BINARY`, como o nome de pasta);
4. a unicidade no projeto, visível ou oculto (os textos da criação);
5. agente `active` do ticket → `O ticket <ID> tem agente ativo. Encerre os agentes dele antes de
   editar o ID: o prompt deles cita a pasta .nick/docs/<ID>/.`. Vale também para o `active` de
   outro nick aberto e para o de um nick que morreu, até a limpeza da próxima abertura (NS-33):
   a recusa é o lado seguro;
6. a pasta `.nick/docs/<antigo>/` é renomeada **primeiro** (`docs::rename`): sem ela, nada é
   feito nem criado (nem `.nick/docs/`); com destino existente, de qualquer tipo (pasta, mesmo
   vazia, arquivo ou link), `Já existe a pasta .nick/docs/<novo>/ no projeto. Nada foi mudado.`;
7. o banco é gravado **depois** (`update_ticket_key`, condicionado ao ID antigo, com
   `is_provisional = 0`: o ID digitado nunca é provisório, mesmo o UUID de antes com outra
   caixa). Sucesso: `ID atualizado: <antigo> → <novo>.`, mais ` A pasta .nick/docs/<antigo>/
   virou .nick/docs/<novo>/.` quando houve pasta. Se o banco falhar (o ticket sumiu, o ID mudou
   em outro nick, o `UNIQUE` de um ID criado nesse meio-tempo ou outro erro), a renomeação é
   desfeita, e a mensagem do banco ganha ` A pasta voltou para .nick/docs/<antigo>/.`. Se o
   desfazer falhar, a mensagem diz onde a pasta ficou e que o ticket continua com o ID antigo.
   **Nada é apagado nem mesclado para "consertar".**

O diálogo abre com o ID atual, o `Enter` faz as checagens 1 a 5 sem renomear (a recusa fica na
barra, com o diálogo aberto), e a confirmação `(s/N)` diz se a pasta será renomeada e que `A
branch do git, o ticket no Jira e os agentes já registrados não mudam.`. O `s` chama o
`edit_id`, que refaz todas as checagens. O projeto indisponível é recusado ao abrir o diálogo
(`Projeto indisponível: <marca>. O ID só é editado com a pasta e o .nick/nick.conf no lugar.`).

**`renameat2(RENAME_NOREPLACE)`, e não `std::fs::rename`:** o `rename(2)` troca em silêncio um
diretório vazio no destino, e a edição nunca pode sobrescrever nem mesclar pastas. O
`symlink_metadata` antes da renomeação recusa o destino que já existe, e o `RENAME_NOREPLACE`
cobre a janela entre a checagem e a renomeação (o `EEXIST` vira a mesma recusa). O teste que
prova isso cria a pasta vazia nessa janela, por um gancho do `rename_with` (privado): na
sabotagem, só ele pegou a troca por `std::fs::rename`, porque a checagem anterior já recusava o
destino existente. O desfazer usa o mesmo `RENAME_NOREPLACE`. O `renameat2` do `nix` existe em
Linux com glibc, a plataforma do nick. **A feature `fs` do `nix` foi a única mudança no
`Cargo.toml`**: ela não puxa crate nenhum, e o `Cargo.lock` não mudou.

**A única escrita do nick num projeto é essa renomeação.** Um teste estrutural
(`no_source_but_docs_rs_writes_in_a_project`, com controle positivo) proíbe no código de
produção, fora do `docs.rs`, `renameat2`, `fs::rename`, `remove_dir`, `remove_file`,
`create_dir`, `fs::write`, `File::create` e `OpenOptions`, menos os usos que já existiam e não
tocam projeto: a trava de cada agente, no diretório de estado do nick (`agents.rs`, NS-33, com
`OpenOptions` e `remove_file`). O temporário do editor (NS-35) é um `NamedTempFile`, fora dessa
lista. Os dois estruturais novos olham só o código de produção (sem o `mod tests` e sem
comentários), porque os testes de unidade gravam arquivos de propósito. O estrutural do NS-35
que proíbe nomes de template ganhou uma isenção só para o `review.md` no `src/docs.rs`
(decisão do usuário no NS-36): ali ele é o artefato do ticket, e não o template; as outras
regras continuam valendo para o arquivo, e o controle positivo continua pegando o nome em
outro fonte.

**O que não acompanha a edição** (fora do escopo, dito também na confirmação): a branch do git,
o ticket no Jira, o nome dos agentes já registrados (`agents.name` não muda; os agentes novos
usam o ID novo) e o histórico da conversa dos agentes encerrados (pendência para a F5). A pasta
renomeada pode estar versionada num projeto que mudou o `.nick/.gitignore`, e aí o git vê
arquivos apagados e criados (impacto aceito no ticket). Renomear também muda o mtime de
`.nick/docs/`, a pasta-mãe: os retratos dos testes de sucesso ignoram só esse mtime.

**Rodapés.** Tela do ticket: `↑/↓ mover · Enter abrir agente · m mudar etapa · i editar ID · g
novo agente · Esc voltar · q sair`, com o `v agentes ativos (N)` do NS-34 quando há agente vivo.
Lista de tickets: ganhou `Enter detalhes`, **depois de `g abrir agente`**. Logo depois de `↑/↓
mover`, ele levava a quebra da linha para o meio de `g abrir agente` (o `g` sozinho no fim da
primeira linha), e os testes de PTY que esperam esse texto deixariam de achá-lo; assim a quebra
cai num separador. Os diálogos novos: `↑/↓ escolher · Enter escolher · Esc cancelar`, `s mudar ·
qualquer outra tecla cancela`, `Enter confirmar · Esc cancelar` e `s trocar · qualquer outra
tecla cancela`. A tela do ticket entrou no `LIST_SCREENS`, para a altura fixa do rodapé com
agente vivo (P1 do review do NS-34). Hoje o rodapé dela cabe nas linhas do rodapé da lista de
tickets em qualquer largura (conferido de 10 a 219 colunas), então a altura não mudaria sem
ela: a entrada protege contra uma mudança futura do rodapé. Por isso o teste da altura não
pegou a sabotagem, e entrou o `every_list_screen_is_in_list_screens`, com um `match` exaustivo
que obriga uma tela nova a entrar na lista.

**Testes adaptados, com o motivo:**
- `nick/src/db/mod.rs`: `a_new_database_gets_version_5_with_auto_scrum_and_no_project` (era o
  da versão 4; renomeado), `opening_twice_duplicates_nothing` (versão 5),
  `the_schema_has_only_modules_projects_tickets_agents_settings_and_stage_changes` (inclui
  `ticket_stage_changes`; renomeado), `a_newer_database_is_refused_without_being_written` e
  `a_database_one_version_newer_is_refused_too` (citam a `(5)`, e o "um a mais" é a 6) e
  `migration_1_is_pinned` (5 migrações e o texto da 5). Os testes de migração v2 → v3 e v3 → v4
  passaram a gravar e ler os tickets do banco antigo por helpers de SQL cru
  (`old_insert_ticket`, `old_tickets`), com as colunas da migração 2 (decisão do usuário no
  NS-36): com o `stage` no `TICKET_COLUMNS`, o `insert_ticket`/`list_tickets` falham num banco
  anterior à 5. As asserções comparam os mesmos campos de antes, e o v3 → v4 passou a abrir só
  com as migrações 1 a 4;
- `nick/src/ui.rs`: o literal de `Ticket` ganhou o `stage` (só sintaxe), o `form` dos testes
  ganhou `plan_missing: false` (só sintaxe), as linhas da lista ganharam a etapa
  (`the_tickets_screen_lists_id_and_title_with_the_provisional_mark`,
  `with_the_filter_the_hidden_ones_are_dimmed_and_marked` e
  `control_characters_in_tickets_are_shown_as_question_marks`), o rodapé da lista ganhou `Enter
  detalhes` (os dois primeiros e `the_lists_say_how_many_agents_run_only_with_agents`), e
  `the_terminal_keeps_its_size_whatever_the_focus_the_marks_and_the_dialog` ganhou a tela do
  ticket no laço;
- `nick/tests/tui.rs`: as linhas da lista ganharam a etapa
  (`the_ticket_flow_works_and_touches_no_file_of_the_project` e
  `the_tickets_of_a_project_do_not_show_in_another`);
- `nick/tests/agent.rs`: a isenção do `review.md` no `src/docs.rs` em
  `no_rust_source_builds_the_prompt_nor_reads_the_config` (acima).

Os testes do "nada no projeto" do NS-32 passaram sem mudança, e o bash (`auto_scrum.sh`,
`prompt.sh`, `bin/nick-agent`, templates) não mudou. O harness do PTY ganhou o `keep_screen`, que
grava a tela num arquivo só quando a variável `NICK_TEST_SCREENS` está definida (as telas da
validação do ticket); sem ela, não faz nada.

**Testes novos:** `db` (migração v4 → v5 com dados, os `CHECK`, o cascade, o tudo ou nada, o
`Gone`/`Moved` e as consultas), `tickets` (`TicketStage`, `change_stage` e os casos do "Editar
ID", cada um com o retrato recursivo do projeto), `docs` (caminhos, artefatos, renomeação,
destino pasta vazia/arquivo/link, a pasta que aparece depois da checagem e o desfazer), `app`
(tela do ticket, `m`, `i`, `g`/`v`, a etapa que não muda sozinha e o aviso do plano), `ui` (a
lista com a etapa, a tela do ticket, o corte do histórico, os estados dos agentes, os diálogos e
o aviso amarelo), os dois estruturais (`nick/tests/agent.rs`) e, no PTY, `tui.rs` (mudar a
etapa até Concluído, o plano lido do disco, o "Editar ID" com a pasta e com o destino
existente) e `agent.rs` (o agente vivo aberto da tela do ticket depois de mudar a etapa, o `i`
recusado com agente vivo e o formulário do Dev sem plano). As sabotagens e as telas ficam em
`.nick/docs/NS-36/`.

## Rust no projeto: foco evidente no painel (NS-37)

> **Mudou no NS-45** (seção "Rust no projeto: etapa Aberto, cores e avanço ao abrir agente
> (NS-45)"): a TUI ganhou as cores das etapas (Aberto em branco, PO e Tech Leader em azul
> `LightBlue`, Dev e Review em laranja `Indexed(208)`, Concluído em verde) e o roxo (`Magenta`) do
> ID e do título do épico. O azul das etapas é **diferente** do ciano do foco (`FOCUS_COLOR`), e um
> teste cobra isso.

> **Mudou no NS-38** (seção "Rust no projeto: navegação em árvore (NS-38)"): as bordas de foco valem
> sempre, também sem agente vivo (a árvore com a borda grossa ciano e o painel `Plain` + DIM); o
> `Block::bordered()` sem estilo de "sem agente" deixou de existir.

História filha do épico NS-28. No layout lado a lado (NS-34, com pelo menos um agente vivo), o
painel com o foco fica evidente de relance, para o operador não mandar sem querer uma tecla para
o `claude` quando queria navegar na lista.

**A regra visual** (tipo e cor decididos pelo usuário no plano):
- o painel com o foco, seja a lista ou o painel do agente, tem a borda `BorderType::Thick` em
  `ui::FOCUS_COLOR` (`Color::Cyan`);
- o painel sem o foco tem a borda `BorderType::Plain` com `Modifier::DIM`, sem cor;
- **só com agente vivo.** Sem nenhum agente, a lista em largura total continua com o
  `Block::bordered()` de antes, sem estilo, byte a byte (a garantia do NS-34). Um teste cobra o
  canto `┌` sem DIM e sem cor, e nenhuma célula grossa ou ciano na tela;
- os diálogos não mudaram: diálogo não é painel de foco.

> **Mudou no NS-46** (seção "Rust no projeto: borda de foco nos diálogos (NS-46)"): o diálogo
> aberto recebe a borda de foco (vermelha quando a ação é irreversível), e a lista e o painel do
> agente ficam sem ela enquanto ele está aberto. As bordas não leem mais o `App::on_terminal`
> direto: passam pelo `ui::focused`, que olha primeiro o diálogo. O rodapé continua no
> `on_terminal`.

O ciano não se confunde com as outras cores da TUI: o vermelho (FORA DA JAIL), o amarelo (avisos
de diálogo) e o verde (Concluído). **A cor não é a única pista, de propósito.** Num terminal de
16 cores ou num tema claro, o ciano pode ficar fraco, e a espessura (`┏━┓` × `┌─┐`) continua
mostrando o foco. Os prints da conferência estão em `.nick/docs/NS-37/prints/`.

**Duas funções em `ui.rs`, sem widget nem módulo novo:** `pane_block(focused)` (o bloco de um
painel ao lado de um agente) e `list_block(app)` (o `Block::bordered()` de antes sem agente vivo,
`pane_block` com agente). Todos os `draw_*` das listas usam o `list_block`: Módulos, Agentes
ativos, Configurações, Projetos, Tickets e Ticket, inclusive o bloco sem título de quando o
detalhe do ticket não carregou.

**A regra "foco no terminal" tem uma função só:** `App::on_terminal` (passou a ser `pub`, sem
mudar o comportamento). O rodapé (`help_text`) e as bordas usam essa mesma função, então os dois
nunca divergem.

**O painel do agente ganhou borda** (decisão do usuário no plano). O ticket supunha uma borda de
foco "discreta demais" e dizia que uma borda mais grossa não ocupa espaço. No código, a lista
tinha sempre a borda simples, e o painel do agente não tinha borda nenhuma: só o cabeçalho e o
terminal. A borda agora envolve o painel inteiro e tira 2 colunas e 2 linhas do PTY de cada
agente. **O tamanho é o mesmo nos dois estados de foco**, porque `Thick` e `Plain` ocupam uma
célula cada. Assim, trocar o foco não redimensiona nada, e a regra do review do NS-34 (P1)
continua valendo. O tamanho muda uma vez só, na versão nova, e não a cada troca de foco.

**O desconto da borda tem fonte única:** `ui::terminal_in_panel` calcula sobre
`Block::bordered().inner(panel)`. Dali o tamanho chega a `layout` (o desenho e o mouse do
`main.rs`), a `agent_area` (o tamanho de cada PTY) e ao spawn (`new_agent_panel` +
`terminal_in_panel`, então o PTY de um agente novo continua nascendo com o tamanho final). Se o
desconto ficasse só no desenho, o `claude` desenharia 2 colunas além do visível. `Areas::panel`
é o retângulo externo, com a borda, e `Areas::header` e `Areas::terminal` ficam no interior. O
`draw_agent` ganhou o parâmetro `focused`, desenha o `pane_block` e tira o retângulo do terminal
do `terminal_in_panel`, sem refazer a conta. O `header_rows` não mudou de lógica e passou a
receber o retângulo interno. Num painel de 1 ou 2 células, o interno sai vazio sem underflow. O
mouse e o cursor do programa caem dentro da borda sem mudar código, porque o `encode_mouse` e o
`area.contains` já descontam o `x`/`y` da área do terminal. Um clique ou a roda sobre a borda é
descartado.

**Helpers da integração** (`nick/tests/common/mod.rs`):
- `Nick::panel()` lê só o interior da borda, pela geometria: entram as linhas com um lado da
  borda (`┃`/`│`) na coluna `PANEL_COL` (o topo e o fundo ficam de fora), e delas só as colunas
  de `PANEL_COL + 1` até a penúltima. Um cabeçalho quebrado não lê `seu ┃ ┃shell.`, e uma régua
  ou caixa que o programa desenhe na ponta da linha (`────`, `│`) fica inteira (review do NS-37,
  N2: a primeira versão tirava esses caracteres das pontas com um `trim_matches`, e uma régua
  do `claude` real sumiria). Sem o lado da borda na coluna, a linha não entra;
- `Nick::flat_screen()` tira o `┃` além do `│`, porque a lista com o foco tem a borda grossa;
- `Nick::cell(row, col)` devolve o símbolo e a cor de uma célula do `vt100` (o ciano é o
  `Idx(6)`), os dois da mesma célula e num parse só (review do NS-37, N1). A primeira versão
  lia o símbolo do texto da tela e a cor da célula, e um caractere largo antes da coluna
  desalinharia os dois.

A adaptação dos helpers rodou verde sobre o código antigo antes de o layout mudar.

**Testes adaptados, com o motivo:**
- `nick/src/ui.rs`: a geometria do NS-34 (`with_an_agent_the_list_is_on_the_left_and_the_agent_on_the_right`,
  `the_warning_out_of_the_jail_wraps_in_the_header_and_the_terminal_shrinks`, com a quebra em 58
  colunas, e `a_long_status_takes_rows_from_the_panel_too`) e os do cabeçalho do NS-33
  (`render_agent` com `focused`, o cabeçalho lido na linha 1 a partir da coluna 1). Em
  `the_agents_of_the_ticket_show_their_state_and_the_selection` (NS-36), o `│` literal da borda
  da lista com o foco virou `┃`.
  `the_terminal_keeps_its_size_whatever_the_focus_the_marks_and_the_dialog` e
  `a_new_agent_is_born_with_the_size_of_its_panel` passaram sem mudança;
- `nick/tests/agent.rs`: `a_resize_reaches_claude_below_the_header` passou a esperar `SIZE 23 58`
  e, a 40x120, `SIZE 34 78` (40 − 1 da barra − 2 do rodapé − 2 da borda − 1 do cabeçalho; o
  plano dizia `SIZE 35 78`, porque contou o corpo com 38 linhas e esqueceu as 2 que o rodapé
  reserva com agente vivo). Em `two_agents_of_two_projects_run_at_once_each_with_its_cwd_and_config`,
  a linha do `alfa` na visão passou a ser lida só nas colunas da lista: com o cabeçalho na linha
  1, dentro da borda, ele divide a linha da tela com o `[FORA DA JAIL]` do `beta` exibido no
  painel. `toggling_the_focus_neither_resizes_nor_marks_a_hidden_agent` passou sem mudança.

Os testes de desenho das telas sem agente e os do rodapé passaram sem edição. Nenhuma
dependência nova, e o README não mudou (regra "README genérico").

**Testes novos:** em `ui`, o estilo de `pane_block`, os dois estados de foco no painel e em
todas as telas de lista (`LIST_SCREENS` e a tela do ticket com o detalhe), a regressão "sem
agente nada muda", o terminal dentro da borda (com painéis de 0 a 3 células), o cursor do
programa e o mouse sobre a borda. No PTY, em `agent.rs`, `toggling_the_focus_moves_the_thick_border_at_once`
(o binário real, com o canto `┏` ciano indo e voltando entre a lista e o painel a cada atalho) e
`the_panel_keeps_the_rules_the_program_draws_up_to_the_border` (review do NS-37, N2). Esse
segundo usa o modo `rules` do `claude` falso, que desenha uma régua `─` e uma linha de caixa
`│caixa … │` na largura inteira do terminal, encostadas na borda. O teste cobra as duas inteiras
no `Nick::panel()`, sem nenhum pedaço da borda grossa. Com o `trim_matches` da primeira versão
do helper ele cai, conforme a sabotagem registrada em `.nick/docs/NS-37/review_sabotagens.txt`.

## Rust no projeto: navegação em árvore (NS-38)

> **Mudou no NS-55** (seção "Rust no projeto: letreiro do nó selecionado na árvore (NS-55)"): uma
> linha longa deixou de ser só cortada pela borda. A não selecionada termina em `…` na última coluna
> antes da borda, e a selecionada rola como um letreiro. As linhas que cabem ficaram iguais. A
> montagem de uma linha saiu do `draw_tree` para o `ui::tree_row`, e o símbolo de seleção virou a
> constante `ui::HIGHLIGHT_SYMBOL`.

> **Mudou no NS-40** (seção "Rust no projeto: épicos opcionais (NS-40)"): a árvore ganhou o nível
> opcional do épico (Módulo > Projeto > [Épico >] Ticket > Agente), com o `NodeId::Epic`, as
> profundidades 2 a 4, o `Place::Epic` e o `HELP_STATES` com 33 estados (a altura do rodapé não
> mudou). Um projeto sem épicos gera as linhas de antes. O dono de um agente é um `Owner` (ticket
> ou épico), e o `RunningAgent`/`LiveAgent` trocaram o `ticket_id` pelo `owner`.

> **Mudou no NS-39** (seção "Rust no projeto: retomar e arquivar agentes (NS-39)"): o agente não
> vivo é `parado`, `[arquivado]` (só com o filtro `f`) ou `ativo em outro nick`; o painel dele
> perdeu a frase "Retomar ... ainda não é possível" e ganhou as ações do estado (`r`, `o`); o
> `Place::OtherAgent` virou `StoppedAgent`, `ArchivedAgent` e `ElsewhereAgent` (o `HELP_STATES`
> tem 29 estados, e a altura do rodapé não mudou); e a confirmação da remoção de projeto diz
> `(inclusive os parados e os arquivados)`.

História filha do épico NS-28. As telas em sequência (Módulos → Projetos → Tickets → Ticket, mais a
visão "Agentes ativos") deram lugar a **uma árvore expansível à esquerda**, como o explorador do VS
Code, e a um **painel contextual à direita**. "Configurações" continua sendo uma tela (`c` no nó do
módulo; `Esc`/`←` voltam para a árvore), e o painel direito continua mostrando o nó selecionado.
O enum `Screen` ficou com duas variantes, `Tree` e `Settings`. Fora do escopo: épicos, retomada de
agentes (F5), arrastar itens, busca na árvore e lembrar entre aberturas quais ramos estavam
abertos.

**A árvore e seus níveis:** Módulo > Projeto > Ticket > Agente. O módulo é a raiz, mesmo único
(`ModuleKind::ALL`). Cada linha tem 2 espaços por nível, `▾` aberto, `▸` fechado e 2 espaços numa
folha (um ticket sem agentes, um agente, um módulo sem projetos), e o rótulo:
- projeto: o nome e, indisponível, a marca (`projects::unavailable_text`), tudo apagado. O
  caminho foi para o painel, para caber nas 40 colunas;
- ticket: `<ID>  [<etapa>]  <título>  [provisório]  [oculto]` (os spans da lista do NS-36, com o
  `[✓ Concluído]` verde e o oculto apagado);
- agente: `<Etapa>  <estado>  [FORA DA JAIL]  <AAAA-MM-DD HH:MM>`, com o estado `● vivo` (deste
  nick), `● saída nova` (vivo e marcado), `ativo em outro nick` ou `encerrado`, e os não vivos
  apagados. **Diferença do plano:** o plano sugeria a data antes do estado. Na profundidade 3
  sobram 28 colunas, e a data empurrava o estado e as marcas para fora da tela; com eles antes da
  data, o fim cortado é só a data (o mesmo racional da etapa logo depois do ID, NS-36). **Review do
  NS-38, P1:** a primeira versão punha `● saída nova` depois do `● vivo` e do `[FORA DA JAIL]`, e
  a marca nunca aparecia num agente fora da jail (`PO  ● vivo  [FORA DA JAIL]  ● saída nova` tem 40
  colunas) e saía cortada no Tech Leader. Um agente marcado é sempre vivo, então a marca toma o
  lugar do `● vivo` e vem antes do aviso da jail, que o cabeçalho do painel mostra inteiro: o mais
  longo, `Tech Leader  ● saída nova`, tem 25 colunas;
- `  ● saída nova` no fim de um nó **fechado** com um agente marcado abaixo dele (o ancestral
  aberto não repete a marca: o agente já está visível).

O título é ` Navegação `, com ` (com ocultos)` e/ou ` (só ativos)` quando os filtros estão
ligados. **Diferença do plano:** o plano sugeria ` (só agentes ativos)`; com os dois filtros o
título passava das 38 colunas da borda e era cortado. O rodapé diz `v mostrar tudo`.

**Decisões (os números são os do plano do NS-38; a 3 e a 4, a tela de Configurações e o
`Screen` com duas variantes, estão no começo desta seção):**
- **Decisão 1:** **Sempre lado a lado** (decisão do usuário). A árvore à esquerda (`ui::LIST_WIDTH`,
  40 colunas, no máximo metade da janela) e o painel à direita, com ou sem agente vivo. Isso
  derrubou a regra "sem agente vivo, as telas são as de antes, byte a byte" (NS-34) e a do NS-37 (o
  `Block::bordered()` sem estilo): o `ui::split` não recebe mais o `beside`, e o `list_block` é
  sempre `pane_block(!app.on_terminal())`. Sem agente, o foco está sempre na árvore (borda grossa
  ciano), e o painel fica `Plain` + DIM;
- **Decisão 2:** **O atalho de foco com um nó que não é agente vivo** vai ao **último agente
  exibido** (`App::last_shown`, decisão do usuário): seleciona o nó dele (abrindo os ramos acima,
  `reveal_and_select`) e põe o foco no terminal. O atalho de novo devolve o foco à árvore, no mesmo
  agente. Sem agente vivo, ou com diálogo aberto, nada;
- **Decisão 5:** **"Exibido" (`App::shown`) segue a seleção:** é `Some(id)` só com o nó de um agente
  vivo deste nick selecionado (`App::select`). Mover a seleção para outro nó esconde o agente, e a
  regra da marca `● saída nova` do NS-34 vale sem mudança (o laço do `main.rs` guarda o texto pelo
  `left_shown` ao comparar o exibido antes e depois de cada evento: não precisou de mudança);
- **Decisão 6:** **Os agentes da árvore são todos os do ticket no banco** (`Db::list_ticket_agents`,
  todas as etapas e estados);
- **Decisão 7:** **Um ticket oculto com agente vivo deste nick continua na árvore** (marcado
  `[oculto]`, apagado), mesmo sem o filtro de ocultos;
- **Decisão 8:** **Os filtros são globais:** `f` (ocultos) e `v` (só agentes ativos) valem para a
  árvore inteira, juntos ou não, e não se desligam ao subir de nível (antes, sair da lista de
  tickets desligava o `f`). Com o `v`, só os ramos com agente vivo deste nick e esses agentes,
  **todos abertos**, sem mexer no estado aberto/fechado guardado (que volta quando o filtro
  desliga). Sem agente vivo, só o módulo, e o painel diz `Nenhum agente ativo.`;
- **Decisão 9:** **Seleção por identidade** (`tree::NodeId`). Depois de cada refresh, um nó que
  sumiu (ticket ocultado sem o filtro, projeto removido, ticket apagado por outro nick, filtro que o
  esconde) passa a seleção ao **irmão seguinte**, senão ao **anterior**, senão ao **ancestral** mais
  próximo (`Tree::reselect`, sobre as linhas de antes e de depois). O ticket selecionado que sumiu
  do banco diz `O ticket <ID> não existe mais no banco.`, como a tela do ticket dizia;
- **Decisão 10:** **Fim do agente exibido:** a seleção **fica no nó** (agora um agente encerrado,
  com os dados à direita), o foco vai para a árvore e o `last_shown` passa a ser o agente vivo
  aberto mais recentemente (ou nenhum). Substitui o "passa a ser exibido o agente vivo aberto mais
  recentemente" do NS-34;
- **Decisão 11:** **Projeto indisponível:** apagado com a marca, não abre (`→`/`Enter` dão a
  mensagem de antes), e um projeto aberto que fica indisponível é fechado no refresh. **Exceção,
  achada na execução:** o plano não previa o encontro desta regra com a 7 ("todo agente vivo
  continua alcançável"): se o projeto de um agente vivo ficasse indisponível, ele seria fechado e o
  agente sumiria da árvore, até do atalho. Um projeto indisponível **com agente vivo deste nick**
  continua expansível e não é fechado; sem agente vivo, vale a regra (exceção confirmada pelo
  usuário);
- **Decisão 12:** **Nenhuma migração e nenhuma dependência nova.** O `ModuleKind` só ganhou
  `derive(Hash)`.

**O que o refresh lê** (`App::refresh_tree`, depois de cada tecla sem diálogo aberto, a regra D9 do
NS-31, e quando um agente abre ou termina): a disponibilidade dos projetos (do disco, como a tela de
Projetos fazia), os tickets de **todos** os projetos (só banco: assim o painel de um projeto fechado
tem a contagem, e o `reveal` acha o projeto de um ticket), os agentes dos tickets dos projetos
abertos e dos que têm agente vivo, e o detalhe do ticket selecionado (os artefatos do disco, só
dele). Os agentes vêm numa consulta por projeto (`Db::list_project_agents`, agrupada por ticket, na
ordem de abertura), e não numa por ticket: o refresh roda a cada seta, e três projetos abertos de
100 tickets eram 300 consultas por tecla (review do NS-38, P2). A seleção vai por `select`, e as
linhas visíveis são recalculadas uma vez por refresh: o `key_tree` não as recalcula antes, porque o
`reselect` precisa das linhas de antes da tecla (achado na execução: um filtro que escondia o nó
selecionado levava a seleção para o lugar errado). A marca `● saída nova` recalcula as linhas na
hora (`hidden_output` e `show`), para subir aos ancestrais fechados sem esperar uma tecla.

**Ações por nó** (`App::key_tree` e `key_node`; os diálogos, as mensagens, as recusas e as
confirmações são as mesmas funções de antes, e as variantes de `Dialog`/`Action` são as de antes,
menos o `NewTicket`, que guarda o projeto, e a remoção de projeto, abaixo: o `ConfirmRemove` com
`agents`/`live` e a `Action::RemoveProject`; review do NS-38, P4):

| Nó | Teclas próprias |
|---|---|
| Todos | `↑/↓` e `k/j` movem; `→`/`Enter` abrem um nó fechado ou descem ao primeiro filho de um aberto; `←`/`Esc` fecham um aberto ou sobem ao pai (com o `v`, sempre sobem); `f`; `v`; `q`/`Ctrl+C` |
| Módulo | `a` cadastrar projeto; `c` configurações |
| Projeto | `a` novo ticket (o projeto do nó, guardado no `Dialog::NewTicket` quando o diálogo abre: um refresh pelo fim de um agente pode mover a seleção com o diálogo aberto, e o ticket iria para outro projeto; review do NS-38, P5); `r` remover (com a contagem dos tickets e dos agentes) |
| Ticket | `e` editar título; `o` ocultar `(s/N)` ou restaurar; `g` abrir agente; `m` mudar etapa; `i` editar ID |
| Agente vivo | `→`/`Enter` foco no terminal; `x` encerrar `(s/N)` |
| Agente não vivo | `→`/`Enter`: `O agente <nome> não está rodando neste nick.` |

O projeto de uma ação vem do nó (`App::selected_project`, `project_of`), e não de uma tela
aberta. Cada ação nova seleciona o nó criado, com os ramos acima abertos: o projeto cadastrado, o
ticket criado e o agente aberto (este com o foco no terminal).

**Remover um projeto leva os agentes dele** (decisão do usuário depois do plano). Antes, o
`DELETE` do projeto já apagava em cascata os registros dos agentes (pelos tickets), mas um
`claude` vivo deste nick continuava rodando, sem nó na árvore e sem linha no banco, e a
confirmação não citava os agentes. Agora o `r` no nó do projeto lê os agentes dele
(`Db::list_project_agents`, uma consulta nova, sem migração):
- **um agente `active` em outro nick** (este não tem o processo e não consegue encerrá-lo) recusa
  a remoção: `O projeto <nome> tem agente ativo em outro nick. Encerre os agentes dele lá antes de
  remover o projeto.`. É o lado seguro, como no "Editar ID" do NS-36, e vale também para o
  `active` de um nick que morreu, até a limpeza da próxima abertura. A checagem é refeita no `s`,
  porque outro nick pode ter aberto um agente nesse meio-tempo;
- a confirmação diz, nesta ordem, os agentes vivos deste nick que serão encerrados (`O agente vivo
  dele neste nick será encerrado.` ou `Os <N> agentes vivos dele neste nick serão encerrados.`), os
  tickets (o texto do NS-32) e os registros que saem do banco (`O agente registrado dele será
  apagado do banco.` ou `Os <N> agentes registrados dele (inclusive os encerrados) serão apagados
  do banco.`). Sem nenhum deles, o texto do NS-31, byte a byte;
- o `s` sem agente vivo remove na hora, como antes. Com agente vivo, vira
  `Action::RemoveProject { project, agents }`: o laço tira as sessões e encerra os grupos numa
  espera só (`agent_session::terminate_all`, como na saída com agentes), e
  `App::remove_project_ending_agents` grava `ended`, solta as travas (como o `x`) e remove o
  projeto. A barra diz `Projeto removido do nick: <nome>. Nenhum arquivo foi apagado. O agente
  <nome> foi encerrado.` (ou `Os <N> agentes vivos dele foram encerrados.`). Nenhum arquivo do
  projeto é tocado (o teste de PTY compara o retrato recursivo).

**Painel direito contextual** (`ui::draw_node_panel`, no `areas.panel` de sempre; com um agente
vivo selecionado, o `draw_agent` do NS-37 sem mudança):
- módulo: `Projetos: N` (com ` (M indisponíveis)` quando há), `Agentes vivos neste nick: N` e, sem
  projeto, `Nenhum projeto cadastrado. Pressione a para adicionar.`;
- projeto: ` Projeto <nome> `, `Caminho:`, `Situação: disponível` ou a marca, `Tickets: N` (com
  ` (M ocultos)` quando há) e, sem ticket, `Nenhum ticket neste projeto. Pressione a para criar.`
  O texto `Nenhum ticket neste projeto, nem oculto.` da lista com o filtro saiu: o painel conta os
  ocultos;
- ticket: o `draw_ticket` do NS-36 sem a seção "Agentes do ticket" (`draw_ticket_detail`), título
  ` Ticket <ID> `, e o histórico com toda a altura que sobra, ainda com `(+N mudanças mais
  antigas)`;
- agente não vivo: ` Agente `, `Nome:`, `Etapa:`, `Estado:`, `Aberto em:`, `Sessão:` e `Retomar um
  agente encerrado ainda não é possível neste nick.`

**Rodapés por nó e altura fixa.** O rodapé é por lugar (`ui::Place`: módulo, projeto, ticket,
agente vivo, agente não vivo, Configurações), sempre com `· f mostrar ocultos|esconder ocultos ·
v agentes ativos|mostrar tudo` antes de `· q sair`, e com `(N)` ou `(N, M com saída nova)` no `v`
quando há agente vivo. No projeto e no ticket o rodapé diz `← fechar/subir` (o `←` também sobe
de um nó fechado ou de uma folha; nit do review do NS-38, sem mudar a altura do rodapé); no
módulo, `← fechar`. `HELP_STATES` (no lugar do `LIST_SCREENS`) é a lista exaustiva dos 21
estados (cada lugar com os dois filtros, mais Configurações), e o `help_rows` reserva com agente
vivo as linhas do mais longo deles. O teste `every_help_state_is_in_help_states` tem um `match`
exaustivo sobre `Place`. Assim mover a seleção entre tipos de nó ou ligar os filtros não muda a
altura do rodapé e não redimensiona nenhum PTY (P1 do review do NS-34): os `SIZE 23 58` e `SIZE
34 78` de `a_resize_reaches_claude_below_the_header` passaram sem mudar números.

**`nick/src/tree.rs` é um módulo novo, com reuso real:** o modelo puro da árvore (sem banco, sem
tela, sem disco), usado pelo `app.rs` (seleção, movimento, refresh) e pelo `ui.rs` (desenho), e
testado sem `App`, no molde do `tickets.rs`/`stages.rs`: o `NodeId`, o `TreeData` (o que o `App`
preenche), o estado aberto/fechado, o `visible` (as linhas com a profundidade, os filtros e o
`unseen_below`), o `parent`, o `reveal` e o `reselect`. Não há trait nem widget genérico de árvore
(há um módulo só: regra "Como acrescentar um módulo" do NS-31) nem crate de árvore: a árvore é
desenhada com o `List` do ratatui. O `RunningAgent` ganhou `ticket_id` e `project_id`, preenchidos
no `open_agent`, para o filtro, o `reveal` e a marca que sobe sem ler o banco.

**O que saiu do código:** as variantes `Modules`, `Projects`, `Tickets`, `Ticket` e
`ActiveAgents`; `key_modules`, `key_projects`, `open_tickets`, `key_tickets`, `refresh_tickets`,
`open_ticket`, `refresh_ticket`, `key_ticket`, `open_active_agents`, `key_active_agents`,
`show_selected`; os `draw_modules`, `draw_projects`, `draw_tickets`, `draw_ticket` e
`draw_active_agents`; os campos `module_index`, `project_index`, `project`, `ticket_index`,
`ticket_rows`, `show_hidden` (foi para o `Tree`), `agent_index`, `back` e `ticket_agent_index`, e o
campo `agents` do `TicketDetail`. Nenhum `#[allow(dead_code)]` segura código morto.

**Testes adaptados e substituídos** (o mapa completo, teste a teste, está em
`.nick/docs/NS-38/mapa_testes.md`):
- os testes de unidade das ações (`app.rs`) passaram a navegar pela árvore pelos helpers do
  `Fixture` (`add` seleciona o módulo, `new_ticket` o projeto da seleção, `with_tickets` abre o
  projeto, `select_key`, `keys`, `visible_tickets`, `on_project`, `on_ticket`, e o `g` sobe do
  agente ao ticket), com as mesmas asserções de mensagem, banco e disco (os retratos do "nada no
  projeto" do NS-32 incluídos);
- mudaram de propósito, com a decisão no próprio teste: o fim do agente exibido (decisão 10), o
  `f` que não se desliga ao subir (8), a visão "Agentes ativos" que virou filtro (`v` e `x` no nó),
  a tela do ticket que virou painel (os agentes no `ticket_agents`, a seleção por `select`), o
  layout sem agente (1) e a borda sem agente (NS-37);
- no PTY, a navegação ficou centralizada no `tests/common/mod.rs` (`Nick::tree_rows`,
  `selected_row`, `select`, `open_node`, `open_project`, `wait_for_flat`, `wait_for_tree`), e as
  esperas por títulos de tela antigos (`auto_scrum > Projetos`, `auto_scrum > <proj> > Tickets`,
  `Módulos`, `Agentes ativos`) viraram textos da árvore, do painel ou do rodapé do nó. Regra do
  NS-32 mantida: depois de um `ESC` de diálogo, espera-se o rodapé do nó (`a adicionar projeto`,
  `a novo ticket`), que só existe com o diálogo fechado. Como o rodapé da árvore quebra em duas
  linhas a 100 colunas, as esperas por ele são pela tela lida numa linha só (`wait_for_flat`).

**Testes novos:** `tree.rs` (raiz mesmo única, profundidade, indisponível, ocultos e a decisão 7,
o filtro `v` que devolve o estado, a marca que sobe, `parent`, `reveal`, `reselect` e a exceção do
projeto indisponível com agente vivo); `app.rs` (navegação, refresh, seleção por identidade,
filtro que esconde a seleção, teclas de um nó que não valem em outro, foco e exibição, atalho com
o `last_shown`, fim do agente exibido, `x`, filtro `v` com a marca e o projeto indisponível com
agente vivo); `ui.rs` (linhas, estilos, títulos dos filtros, caracteres de controle, os painéis
de cada nó, o histórico longo, sempre lado a lado, os agentes vivos e a marca, os rodapés, o
`HELP_STATES` e o tamanho dos terminais em cinco larguras, com cada tipo de nó, filtro, foco e
marca); e, no PTY (`tests/agent.rs`), do módulo ao terminal só com setas e `Enter` (critério 3), o
atalho com um ticket selecionado indo ao último agente e voltando (com a borda grossa ciano pelo
`Nick::cell`), a navegação por todos os tipos de nó e filtros sem nenhum SIGWINCH novo (pelo modo
`winsize` do `claude` falso) e o painel e o rodapé de cada tipo de nó. As telas de conferência
estão em `.nick/docs/NS-38/telas/`.

## Rust no projeto: retomar e arquivar agentes (NS-39)

> **Mudou no NS-57** (seção "Rust no projeto: liberar e prender a jail de um agente (NS-57)"): o
> `--resume` do `bin/nick-agent` aceita o `--no-jail` (flag sem valor, só nesse modo), e o
> `Launch::Resume` ganhou o `jail_off`, que o acrescenta no fim do argv; sem ele, o argv é o de
> antes, byte a byte. O `r` passou a ser o `ask_resume_with(id, ResumeJail::AsConfig)`, e o
> `Action::CheckResume`, o `check_resume` e o `resume_agent` levam o `ResumeJail` da retomada.

> **Mudou no NS-61** (seção "Rust no projeto: etapa Rejeitado (NS-61)"): o `r` de um
> agente de ticket **rejeitado** é recusado antes do `--check` (`ticket rejeitado: mude a etapa com
> m antes de abrir um agente`), com o ticket lido do banco. Isso difere do ticket oculto, cuja
> retomada continua permitida. Rejeitar um ticket encerra os agentes vivos dele neste nick, depois
> de gravar a etapa.

> **Mudou no NS-50** (seção "Rust no projeto: retomada segue a troca de sessão do claude
> (NS-50)"): o `--has-conversation` segue a cadeia de registros `continued-in` e, com o arquivo,
> imprime duas linhas (`conversation=true` e `session=<última sessão>`). O `conversation=false`
> não mudou. A retomada usa a sessão efetiva (`resume_session_id`, ou o `session_id` sem ela) no
> `--has-conversation` e no `--resume`, e a trava continua a do `session_id`.

> **Mudou no NS-40** (seção "Rust no projeto: épicos opcionais (NS-40)"): o `--resume` aceita
> `--epic-id <id>` no lugar de `--ticket-id` (exatamente um dos dois), para um agente de épico, e o
> `resume_id_notice <antigo> <novo> [ticket|epic]` ganhou o texto do épico (o do ticket não mudou,
> byte a byte). O `Launch::Resume` leva um `HolderKey`, e o `ticket_of_agent` virou
> `holder_of_agent`. A migração 7 reconstruiu a tabela `agents` (o dono ticket **ou** épico).

A fatia F5 do épico NS-28. Um agente cujo `claude` terminou, ou cujo nick fechou, deixa de ser
uma conversa perdida: ele fica **parado** e pode ser **retomado** na mesma conversa (`claude
--resume <session_id>`) pelo nó dele na árvore, ou **arquivado**. Fora do escopo: retomar
sozinho ao abrir o nick, apagar agentes de vez, épicos (F7) e rodar em segundo plano.

**Estados e migração 6.** Os estados são `active` (ativo), `stopped` (parado, retomável: o
`ended` de antes) e `archived` (arquivado). A migração 6 faz `ALTER TABLE agents ADD COLUMN
opened_key TEXT` e `UPDATE agents SET state = 'stopped' WHERE state = 'ended'`. A coluna `state`
continua sem `CHECK` (NS-33), e o enum `AgentState` (`agents.rs`, com `key`/`from_key`) continua
sendo a única fonte dos estados no código: os métodos do `Db` usam `AgentState::...key()`, e o
literal da migração é a exceção (ela é fixada por teste e não muda mais). Dois estruturais cobram
isso, com controle positivo: `no_state_literal_outside_the_enum` (`agents.rs`: nenhum literal de
estado no código de produção do `db/mod.rs` nem do `agents.rs`, fora o `key()` do enum) e
`no_rust_source_writes_the_state_ended` (`nick/tests/agent.rs`: nenhum `"ended"`/`'ended'` em
`nick/src/` fora do `db/migrations.rs`). O `end_agent` virou `stop_agent`, com a mesma semântica:
o fim do `claude`, o `x`, a saída (`q` + `s`, SIGTERM/SIGHUP) e a limpeza da abertura com a trava
livre deixam o agente `stopped`. As mudanças de estado novas são UPDATEs condicionados ao estado
(`Db::resume_agent` só de `stopped` ou `active`; `archive_agent` só de `stopped`;
`unarchive_agent` só de `archived`, que volta a `stopped`), no molde do `change_ticket_stage`: com
0 linhas, a linha é relida e o `AgentChange` diz por quê (`Gone`, `Active`, `Stopped`,
`Archived`). Um estado que este nick não conhece conta como `Gone`: nada é feito nele.

**A retomada é só manual** (`r` no nó de um agente parado). Abrir o nick não retoma nada (teste
no `App` e no PTY). O fluxo:
1. `r` (`App::ask_resume`) recusa na hora, sem `--check`: o arquivado (`O agente <nome> está
   arquivado. Desarquive (o) antes de retomar.`), o `active` cuja trava outro nick vivo segura
   (`agents::held_elsewhere`, que tenta a trava e solta na hora: `O agente <nome> está ativo em
   outro nick e não pode ser retomado aqui.`) e o projeto indisponível (a recusa do `g`). Senão,
   `Action::CheckResume`;
2. o laço (`main.rs`) chama o `App::check_resume` com um `TerminalAgentLauncher` que já nasce com
   o tamanho do painel (os helpers `pty_launcher`/`adopt_session`, extraídos do `OpenAgent`, sem
   mudar o comportamento dele): o `--check` (falha → o alerta "Agente não aberto", pelo
   `agent_failed`) e o `--has-conversation` (abaixo);
3. com a conversa, `App::resume_agent`: `agents::resume` pega a trava **antes** do UPDATE (como o
   `open` pega antes do INSERT; o `take_lock` foi extraído do `open_with`, cujos testes passaram
   sem mudar asserção), o PTY abre com `Launch::Resume` e o agente entra nos vivos pelo
   `start_running`, extraído do `open_agent` (os testes dele passaram sem mudar asserção): nó
   selecionado, exibido e foco no terminal. A barra diz `O agente <nome> foi retomado.`. Se o
   spawn falhar, o agente volta a `stopped` pelo `agents::end`, como no `open_agent`.

Um `active` cuja trava está livre (o nick dono morreu: a prova do D7 do NS-33) é retomado
normalmente. Na árvore ele continua `ativo em outro nick` até a limpeza da próxima abertura. O
nome não muda na retomada (vale o `agents.name`, regra do NS-36), e a retomada vale para
agentes de qualquer etapa. **Um ticket oculto não recusa o `r`** (decisão do usuário na
execução): a retomada segue uma conversa que já existe. Já o `n` da conversa perdida, que é o
fluxo do `g`, recusa como ele.

**A configuração atual vale na retomada.** O launcher roda o mesmo `load_and_check`,
`config_dir_prepare`, `jail_print_status` e `jail_exec` do lançamento, com o `.nick/nick.conf`
do momento: um agente aberto fora da jail é retomado na jail se o projeto a ligou depois (teste
no PTY). O `--permission-mode auto` sai da **etapa**, porque o tipo não é gravado (NS-35): o
modo `--resume` recebe `--stage <po|tech_leader|development|review>` (a coluna `agents.stage`), e
o `auto` só vale com `development`, a regra de "Modo auto só na etapa de Desenvolvimento".

**O `bin/nick-agent` ganhou dois modos** (o `Uso:` cita os dois):
- `--has-conversation <uuid>`: sem `load_and_check` (não depende da configuração), imprime
  exatamente `conversation=true` ou `conversation=false` e sai com 0; não lança nem grava nada
  (retrato recursivo no teste). O Rust lê a resposta com `launch::parse_has_conversation`, no
  molde do `parse_check`: outra saída com código 0 é `O bin/nick-agent respondeu algo inesperado
  na verificação da conversa.`, e uma falha usa o `launcher_failure`;
- `--resume <uuid> --name <nome> --stage <etapa> [--opened-id <id>] --ticket-id <id>`, em
  qualquer ordem, com as recusas do lançamento (valor ausente, repetido, desconhecido, a etapa
  fora das 4, e os dois IDs pelo `jira_id_valid`). O argv é `claude --settings <regras>
  [--permission-mode auto] [--append-system-prompt <aviso>] --resume <uuid> --name <nome>`,
  **sem prompt** (desde o NS-41, o `--append-system-prompt` existe sempre e começa pela
  instrução de não contornar uma recusa: ver "Instrução de não contornar uma recusa (NS-41)").
  O main despacha para ele quando `--resume` é uma das opções (`launch_kind`, que pula os
  valores).

**Diferença do plano, aprovada pelo usuário na execução:** os dois modos de lançamento terminam
numa função comum, `exec_claude <tipo ou etapa> <aviso> [args]`, extraída do fim do
`launch_mode` sem mudar o argv dele. Dois estruturais de antes (`--permission-mode` numa linha
só, condicionada a `development`, e uma única chamada de `jail_exec`) cobram exatamente isso, e
passaram sem mudança. Um diferencial novo prova que, para a mesma configuração, a retomada chama
o `ai-jail` com as mesmas opções até o `--` que o lançamento.

**A pré-checagem da conversa e a sua limitação** (decisão 1 do plano, do usuário). O arquivo da
conversa é `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects/<cwd físico com todo caractere fora de
A-Z, a-z e 0-9 trocado por "->/<session_id>.jsonl`, montado pela `claude_conversation_file
<config_dir> <dir> <session_id> <var>` de `lib/claude/claude.sh`. **Diferença do plano:** a
pasta de configuração entra por parâmetro, e não lida dentro da função, porque as regras do
`lib/` (testes estruturais de antes) proíbem expandir variável que não seja da biblioteca e
qualquer `CLAUDE_` sem `NICK_` no código de `lib/claude/`. Quem passa o
`${CLAUDE_CONFIG_DIR:-$HOME/.claude}` é o `bin/nick-agent`, com o `pwd -P`. **O locale:** a troca
é por caractere no locale de quem chama (um `é` vira um `-` só em UTF-8, como o `claude` faz, e
dois em `LC_ALL=C`). Por decisão do usuário na execução, o `has_conversation_mode` força
`local LC_ALL=C.UTF-8` (o escopo dinâmico do bash vale durante a chamada da lib), e o teste roda
com `LC_ALL=C` e `POSIX` também. Numa máquina sem o locale `C.UTF-8`, o bash avisa no stderr (que
o `parse_has_conversation` ignora num sucesso) e a troca volta a ser por byte. **O nick passa a
depender do layout interno do `claude`** (impacto aceito no ticket): num caminho muito longo, o
`claude` encurta o nome da pasta, e uma versão nova pode mudar a regra. Também diverge um
caractere fora do BMP (um emoji, por exemplo) no caminho: o `claude` (JavaScript) troca por
unidade UTF-16 e gera `--`, e o bash em UTF-8 gera um `-` só (review do NS-39, ponto 5). Nos
três casos, o nick diria "não existe" para uma conversa que existe. Nada se perde (o `Esc` não grava nada, e arquivar
é reversível), e a verificação manual confere o caminho longo. Se o `--resume` falhar mesmo com o
arquivo presente, a barra mostra o `terminou com o código N` de sempre.

**A conversa perdida** (`Dialog::ConversationLost`): `A conversa do agente <nome> não existe mais
no claude (o histórico foi apagado ou o projeto mudou de pasta). Nada foi aberto.`, com o rodapé
`a arquivar · n abrir novo agente nesta etapa · Esc cancelar`. Nenhum PTY abre (teste no PTY,
com a árvore respondendo depois). `a` arquiva sem segundo `(s/N)` (o diálogo já é a pergunta);
`n` abre o menu "Abrir agente" do NS-35 com os tipos da etapa do agente
(`StageType::stage() == agent.stage`) que as flags oferecem, e com o `Precheck` da retomada (sem
segundo `--check`); daí em diante é o fluxo do `g`. Se a etapa não tivesse tipo disponível (hoje
não acontece), a barra diria por quê. `Esc` fecha sem gravar nada, e o atalho reservado continua
ignorado com o diálogo aberto (NS-34).

**O aviso do ID novo (Q9).** O `agents.opened_key` guarda a chave do ticket na abertura
(`agents::open` e `Db::insert_agent` ganharam o parâmetro). Na retomada, o ID de abertura é o
`opened_key` ou, se ele for nulo (agente de antes do NS-39), o último pedaço do `agents.name`
depois de ` · `, só se passar no `jira_id_valid` (`agents::opened_key`, pura e testada). O
`Launch::Resume` leva esse ID (`--opened-id`, só quando conhecido) e o `ticket.key` atual
(`--ticket-id`). Quando são diferentes, o launcher acrescenta, no mesmo `--append-system-prompt`
do aviso da jail (uma linha em branco entre os dois; sem nenhum aviso, nenhum
`--append-system-prompt`; desde o NS-41, ele existe sempre e começa pela instrução de não
contornar uma recusa, e o aviso do ID vem por último: ver "Instrução de não contornar uma
recusa (NS-41)"), o texto da `resume_id_notice` do `auto_scrum/prompt.sh`: `O ID deste
ticket mudou de <antigo> para <novo> depois que esta conversa começou. A pasta do ticket agora é
.nick/docs/<novo>/ (antes era .nick/docs/<antigo>/): use o caminho novo para o plano, o review e
os artefatos.`. O texto é prompt, então mora no bash: um estrutural
(`the_notice_of_the_new_id_is_only_in_bash`, com controle positivo) proíbe um trecho fixo dele em
`nick/src/`. A barra diz ` Ele foi avisado de que a pasta agora é .nick/docs/<novo>/.`. O
"Editar ID" do NS-36 não mudou: só um agente `active` bloqueia a edição, e os parados recebem o
aviso na retomada.

**Arquivar e desarquivar** (`o` no nó do agente). Num parado, `Arquivar o agente <nome>? Ele sai
da árvore e volta com f. (s/N)` (`Dialog::ConfirmArchive`, `is_yes`); depois do refresh, a
seleção segue a decisão 9 do NS-38 (o irmão seguinte, o anterior ou o ancestral). Num arquivado,
desarquiva direto (`O agente <nome> voltou para parado.`), como o restaurar do ticket. Num vivo,
ou ativo em outro nick: `Um agente ativo não pode ser arquivado.`. Os arquivados aparecem com o
filtro `f`, o mesmo dos tickets ocultos (`Tree::visible`): um ticket com só agentes arquivados
vira folha sem ele, e o filtro `v` não muda.

**`Enter`/`→` num agente não vivo** dizem a tecla: `O agente <nome> está parado. Pressione r para
retomar.` e `O agente <nome> está arquivado. Pressione o para desarquivar.`. Num ativo de outro
nick, a mensagem de antes. As mensagens de fim (`terminou`, `foi encerrado`) não mudaram.

**Árvore, painel e rodapés.** O nó do agente mostra `● vivo`, `ativo em outro nick`, `parado` ou
`[arquivado]`, os não vivos apagados (DIM). **Diferença do plano:** o plano pedia "`arquivado`
apagado e com `[arquivado]`"; na árvore o estado do arquivado é o próprio `[arquivado]` (no molde
do `[oculto]` do ticket), porque `arquivado  [arquivado]` repetia o texto e, na profundidade 3,
empurrava a data para fora das 38 colunas. O painel diz `Estado: arquivado`. No painel do agente
não vivo saiu a frase `Retomar um agente encerrado ainda não é possível neste nick.`, e entram a
linha `ID na abertura: <id>` (quando difere do atual, ou `desconhecido`) e a ação do estado:
`r retomar esta conversa · o arquivar`, `o desarquivar` ou `Ativo em outro nick: feche-o lá para
retomar aqui.`. O `Place::OtherAgent` virou `StoppedAgent`, `ArchivedAgent` e `ElsewhereAgent`, e o
`HELP_STATES` foi de 21 para 29 estados (o `every_help_state_is_in_help_states` cobra). Os rodapés
(decisão do usuário: `← subir`, como nos outros nós de agente, e não o `← voltar` do plano):
`↑/↓ mover · r retomar · o arquivar · ← subir · …` e `↑/↓ mover · o desarquivar · ← subir · …`;
o do ativo em outro nick é o de antes. Nenhum é mais longo que o do ticket: os `SIZE 23 58`/`SIZE
34 78` do `a_resize_reaches_claude_below_the_header` e o teste do tamanho em cinco larguras
passaram sem mudar números. A confirmação da remoção de projeto diz `(inclusive os parados e os
arquivados)`.

**Lançamento no Rust.** `launch::Launch` (`New { kind, prompt_file }` ou `Resume { stage,
opened_key, ticket_key }`) substitui o `kind`/`prompt_file` do `AgentSession::spawn` e do
`AgentLauncher::spawn`, e `Launch::args` monta os argumentos do launcher; o `CommandBuilder`
continua `bash <launcher>`. O `New` gera o argv de antes: os testes do `agent_session.rs` e o
diferencial de `nick/tests/stages.rs` passaram sem mudar asserção (só a chamada). O
`AgentLauncher` ganhou o `has_conversation`. O teste estrutural "nenhum `--permission-mode` no
Rust" lê o fonte inteiro, testes incluídos: o teste da retomada no PTY confere o modo `auto` pelo
`\nauto\n` do argv, sem citar a flag.

**Ajustes do review do NS-39** (cada um com teste que falhou antes da correção):
- `agents::resume_with`: se a linha virou `active` mas a releitura dela falhou, ela volta a
  `stopped` antes de a trava sair. Senão ficaria `active` sem dono, "ativo em outro nick" até a
  limpeza da próxima abertura (ponto 1);
- o `r` (`App::ask_resume`) relê a linha do banco (`Db::get_agent`) em vez de confiar na árvore
  lida antes da tecla. Um agente que outro nick retomou nesse meio-tempo é recusado sem o
  `--check` (ponto 2);
- o `unarchive` de um agente ativo diz `O agente <nome> já está ativo.`, e não a mensagem do
  arquivar (ponto 3);
- o `check_resume` roda a partir do laço, fora do `handle_key`. Por isso, uma retomada recusada
  ali (a linha mudou depois da tecla, ou o spawn falhou) relê a árvore, e o nó mostra o estado de
  que a barra fala (ponto 4);
- a limitação dos caracteres fora do BMP, acima (ponto 5);
- um estado que este nick não conhece não aparece mais como `parado`. A árvore diz `estado
  desconhecido`, o painel diz `Estado: desconhecido (<valor>)` e `Este nick não conhece o estado
  deste agente: nada a fazer aqui.`, o rodapé não oferece ação, e `r`/`o` respondem `O agente
  <nome> está num estado que este nick não conhece (<valor>).` (ponto 6);
- nits: o `ticket_of_agent` junta a leitura do ticket repetida no `check_resume` e no
  `resume_agent`, e linhas de comentário passaram a caber em 100 colunas (ponto 7).

**Testes adaptados, com o motivo** (o mapa teste a teste está em
`.nick/docs/NS-39/mapa_testes.md`): os de banco que citavam a versão 5 (6 migrações, o texto da
6, "mais novo" com 7); os de migração antiga, que passaram a gravar e ler os agentes por helpers
de SQL cru (`old_insert_agent`, `old_end_agent`, `old_active_agents`, o molde do NS-36), porque
o `AGENT_COLUMNS` tem a coluna nova, e o v4 → v5 passou a abrir só com as migrações 1 a 5; o
`insert_agent`/`agents::open` com o `opened_key` (só sintaxe); `ended` → `stopped` nas asserções
de estado; o `Enter` num agente parado (decisão 3); `encerrado` → `parado` e o painel/rodapé do
agente não vivo no `ui.rs` e no PTY; a lista de funções do `prompt.sh` (`resume_id_notice`) no
`test_auto_scrum.sh`; e o `Uso:`, o `-u CLAUDE_CONFIG_DIR` do `run_agent` e o modo `resume` do
`jail_fail` no `test_nick_agent.sh`.

**Testes novos:** `db` (v5 → v6 com dados, `get_agent`, cada `AgentChange` de retomar, arquivar e
desarquivar, estado desconhecido), `agents` (estados, o estrutural dos literais, a trava antes do
UPDATE, a trava de outro nick, o ativo com a trava livre, `Gone`/`Archived` soltando a trava,
`held_elsewhere`, `archive`/`unarchive`, `opened_key`), `lib/tests/test_claude.sh`
(`claude_conversation_file`), `bin/tests/test_nick_agent.sh` (`--has-conversation` com e sem o
arquivo, `CLAUDE_CONFIG_DIR` e `$HOME/.claude`, `_`, `.`, espaço e acento em três locales, link
simbólico, argumentos, nada gravado; `--resume` com a jail ligada e desligada, as 4 etapas, sem
prompt, os avisos juntos ou nenhum, as recusas, as falhas do `jail_check` e o diferencial),
`launch` (`parse_has_conversation`, `run_has_conversation` real), `agent_session` (a retomada no
PTY real), `tree` (o arquivado e o filtro), `app` (cada item do 6.5 do plano, com um processo
filho segurando o `flock`) e `ui` (estados, estilos, painéis, diálogos e rodapés), e no PTY
(`nick/tests/agent.rs`): retomar de ponta a ponta, fechar e reabrir o nick sem retomar nada, a
configuração atual (jail ligada depois), a conversa perdida com arquivar e desarquivar, o ativo
em outro nick, o ID trocado e os dois estruturais. As sabotagens ficam em
`.nick/docs/NS-39/sabotagens.md`, as telas em `.nick/docs/NS-39/telas/`, e a verificação com o
`claude` real (que não roda dentro da jail do nick) em `.nick/docs/NS-39/verificacao_manual.md`.

## Rust no projeto: épicos opcionais (NS-40)

> **Mudou no NS-54** (seção "Rust no projeto: concluir épicos e filtro de concluídos (NS-54)"): o
> épico ganhou os estados aberto e concluído (migração 10, `epics.done_at`), o `c` no nó do épico
> (concluir/reabrir) e a marca `[concluído]` no nó e no painel, com a linha `Estado:`. A árvore ganhou
> o filtro `d` (`Tree::hide_done`), e o `f` e o `d` são lembrados entre aberturas. O `set_ticket_epic`
> e o `change_ticket_stage` devolvem também a chave do épico reaberto, e o INSERT de ticket passou
> para o `insert_ticket_reopening` (o `insert_ticket` continua, com a mesma assinatura). O
> `HELP_STATES` tem 65 estados, e o rodapé mais longo cresceu uma linha.

> **Mudou no NS-45** (seção "Rust no projeto: etapa Aberto, cores e avanço ao abrir agente
> (NS-45)"): o ID e o título do épico ficam em roxo (`ui::EPIC_COLOR`, `Color::Magenta`) na
> árvore. O "sem cor nova" desta seção deixou de valer para eles; o `[épico]` continua só em
> negrito, e o `[provisório]`, o `[oculto]` e o DIM do épico oculto não mudaram. A migração 9
> reconstrói `tickets` e recria os seis gatilhos desta migração com o mesmo texto.

A fatia F7 do épico NS-28. O **épico** é um nível **opcional** entre o projeto e o ticket, para
guardar conversas longas: um plano que vive mais que um ticket (como o `migracao_tui.md`). A
árvore passa a ser **Módulo > Projeto > [Épico >] Ticket > Agente**. Fora do escopo: chat livre
(agente sem épico nem ticket), sincronizar com o épico do Jira, épico dentro de épico e etapa de
épico.

**O modelo.** O épico tem ID (do Jira ou UUID provisório) e título, com as regras do ticket, e
**não tem etapa nem histórico**. Os agentes dele são **só de PO e de Tech Leader**
(`stages::EPIC_STAGES`, fonte única), com o formulário e o prompt do NS-35, e são retomados e
arquivados como os de ticket (NS-39): são as conversas de longa duração. Um ticket fica direto no
projeto (sem épico) ou dentro de um épico.

**Decisão: sem chat livre por enquanto.** Todo agente tem dono: um ticket ou um épico. Um agente
solto no projeto ficou fora do escopo do ticket; o `Owner` (abaixo) é o lugar onde ele entraria.

**Banco: migração 7** (uma transação, pelo `Db::open_with`):
- a tabela `epics` (`project_id → projects ON DELETE CASCADE`, `key`, `is_provisional`, `title`
  com `CHECK`, `created_at`, `hidden_at`, `UNIQUE (project_id, key)`);
- `tickets.epic_id` (`ALTER TABLE ... ADD COLUMN`, `REFERENCES epics(id)`, **sem `ON DELETE`**):
  um épico nunca é apagado sozinho, só com o projeto, e aí os tickets também saem. **Conferido na
  execução:** a ordem do `CASCADE` (projeto → épicos e tickets) não esbarra na FK
  `tickets.epic_id`, porque a FK sem ação é conferida no fim do `DELETE` e os tickets já saíram
  (teste `deleting_a_project_deletes_its_epics_tickets_and_the_agents_of_both_and_no_other`, com
  `foreign_key_check` vazio). O `ON DELETE SET NULL` não foi preciso;
- **a reconstrução da tabela `agents`**, porque o SQLite não altera `NOT NULL`, `CHECK` nem FK:
  `ticket_id` e `epic_id` opcionais, `CHECK ((ticket_id IS NULL) <> (epic_id IS NULL))` (um dono,
  nunca os dois, nunca nenhum) e `CHECK (epic_id IS NULL OR stage IN ('po', 'tech_leader'))`. A
  ordem é `CREATE TABLE agents_new`, `INSERT ... SELECT` com os `id` mantidos (as travas são por
  `session_id`, mas a árvore e os testes usam o `id`), `DROP TABLE agents` e `ALTER TABLE
  agents_new RENAME TO agents`. Funciona com `foreign_keys` ligado, porque nenhuma tabela aponta
  para `agents`. A coluna `state` continua sem `CHECK` (NS-33, NS-39): o `AgentState` segue a
  única fonte dos estados. O teste v6 → v7 confere `id`, campos, estados e `opened_key` mantidos,
  `foreign_key_check` e `integrity_check` vazios e o `UNIQUE` do `session_id` valendo;
- os triggers (abaixo).

**Unicidade do ID entre tickets e épicos: código + triggers** (decisão do usuário no plano). Os
dois usam `.nick/docs/<ID>/`, então o ID é único entre os tickets **e** os épicos do projeto. O
código confere antes (`Db::find_key`, no lugar do `find_ticket_key`, que devolve `KeyKind::Ticket
| Epic` e se está oculto) e dá a mensagem em PT-BR: `Já existe um ticket com o ID <ID> neste
projeto.` ou `Já existe um épico com o ID <ID> neste projeto.`, com o sufixo de oculto de antes.
Um `UNIQUE` não cruza duas tabelas, então a migração 7 cria triggers `BEFORE INSERT` e `BEFORE
UPDATE OF key` nas duas tabelas com `RAISE(ABORT, 'nick: key of a ticket' | 'nick: key of an
epic')`, que fecham a corrida entre dois nicks. `db::is_trigger_abort(err, marker)` reconhece o
`SQLITE_CONSTRAINT_TRIGGER` pela mensagem (o molde do `is_unique_violation`), e o erro vira a
mesma mensagem do código. Um ID provisório que colide com um ticket ou um épico é gerado de novo.
Outro par de triggers recusa um ticket num épico de outro projeto (`nick: epic of another
project`), e o `UPDATE` do `set_ticket_epic` já exige o mesmo projeto no SQL.

**O dono do agente é um tipo:** `db::Owner { Ticket(i64), Epic(i64) }` (reexportado por
`agents.rs`). O `Agent` trocou o `ticket_id` pelo `owner`, lido de `ticket_id`/`epic_id` (uma
linha sem nenhum dos dois é erro do banco, nunca `panic!`); o `LiveAgent` e o `RunningAgent`
também. O `NodeId` ganhou `Epic(i64)`, e o pai de um agente é o nó do dono (`NodeId::from(owner)`).
O `Db::insert_agent(owner, ...)` e o `count_active_agents(owner)` recebem o dono, e o
`list_project_agents` inclui os agentes dos épicos (a remoção do projeto e a recusa "ativo em
outro nick" já os cobrem). `agents::open` recebe o `Owner` e recusa, sem tocar na trava, outra
etapa num épico: `O épico só tem agentes de PO e de Tech Leader.`. A regra das etapas do épico
está em três lugares com uma fonte: o `CHECK` (um teste compara o texto da migração com o
`EPIC_STAGES`), o menu (`StageType::epic_menu`) e o `agents::open`.

**O `Holder`** (`agents.rs`): `{ owner, project_id, key, title, hidden }`, construído por
`From<&Ticket>` e `From<&Epic>`. Os fluxos de abrir e retomar agente deixaram de receber um
`Ticket`: `Dialog::ChooseType`, `StageForm`, `ConfirmPrompt`, `ConversationLost`,
`Action::CheckAgent` e `Action::OpenAgent` carregam um `Holder`, e `check_agent`, `open_agent`,
`start_running`, `resume_agent` e `holder_of_agent` (era `ticket_of_agent`) o recebem. **É o único
caminho**: não há um "abrir agente de épico" paralelo. **Diferença do plano:** o `Holder` ganhou o
`title`, porque os diálogos de "Abrir agente" mostram `Ticket <ID> — <título>` (NS-35), agora
também `Épico <ID> — <título>`; sem ele, o título viria da árvore, e o teste de desenho do menu
(que monta o diálogo sem a árvore) mudaria. O `g` de um épico relê o épico do banco antes do menu
(`get_epic`, no molde do `ask_resume`): outro nick pode tê-lo ocultado.

**O formulário e o prompt do NS-35 não mudaram.** O formulário do épico é o do ticket, e o campo
`ID do Jira` começa com a chave do épico (`initial_values(&holder.key)`), então o `tech_leader`
grava em `.nick/docs/<ID do épico>/plan.md` pelo próprio template. **O menu do épico é exclusivo do
nick** (o `auto_scrum.sh` não tem épico): o diferencial de `nick/tests/stages.rs` cobre os 6 tipos
de PO e TL com o ID do épico (o log do `main()` do script com `EPIC-1` digitado, byte a byte), e
um teste separado documenta que o menu do script tem os 8 tipos e o `epic_menu`, só os 6. Os dois
testes desse binário mudam o ambiente, então passaram a se revezar num `Mutex` (antes havia um
teste só).

**A árvore** (`tree.rs`): dentro do projeto, primeiro os épicos (por criação), depois os tickets
sem épico (por criação); dentro do épico, primeiro os agentes do épico (por abertura), depois os
tickets dele (decisão do usuário: os agentes do épico ficam perto do topo mesmo com muitos
tickets). Profundidades: épico 2, agente do épico 3, ticket do épico 3, agente de ticket do épico
4; o ticket sem épico e os agentes dele ficam como antes (2 e 3), e **um projeto sem épicos gera
exatamente as linhas de antes** (os testes de `tree.rs` passaram só com a construção do
`TreeData` adaptada). O `TreeData` ganhou `epics` (por projeto) e o `agents` passou a ser por dono
(`HashMap<Owner, Vec<Agent>>`); o `LiveAgent.epic_id` é o épico do ramo (o do dono, ou o do
ticket), por onde sobem a marca `● saída nova` e o filtro `v`. O `App` lê os épicos com uma
consulta por projeto, como os tickets (P2 do review do NS-38). O `reselect` não mudou e funciona a
partir da profundidade 4 (teste).

**As teclas** (todas no rodapé do nó):
- projeto: `a` novo ticket (sem épico) e **`n` novo épico** (o mesmo diálogo `NewTicket`, com
  `NewItem::Epic` e o título ` Novo épico `);
- épico: **`a` novo ticket dentro dele** (o ramo abre e o ticket fica selecionado), `e` título,
  `o` ocultar/restaurar, `g` abrir agente (só PO e TL), `i` editar ID. Não há `m`, `p` nem `r`. O
  `a` num épico oculto é recusado: `O épico <ID> está oculto. Restaure-o (o) para criar um
  ticket.` (decisão do usuário na execução; o plano não dizia);
- ticket: **`p` ("pôr em épico")** abre ` Mover ticket `, com `Sem épico` e os épicos visíveis do
  projeto, e o lugar atual marcado `(atual)`. Sem épico para oferecer: `O projeto não tem épicos.
  Crie um com n no nó do projeto.`.

**Ocultar o épico oculta o ramo inteiro** sem mudar o `hidden_at` dos tickets: a árvore esconde o
ramo pelo `epics.hidden_at`, e ao restaurar cada ticket volta como estava (um ticket oculto antes
continua oculto). O `o` pede `Ocultar o épico <ID>? O ramo inteiro (<N> tickets) sai da árvore e
volta com f. (s/N)` e é recusado, já na tecla e sem diálogo, quando há agente `active` (de qualquer
nick) no épico ou num ticket dele: `O épico <ID> tem <N> agente(s) ativo(s) (no épico ou nos
tickets dele). Encerre-os antes de ocultar.` (`1 agente ativo` / `N agentes ativos`). A contagem e
a gravação são **um único `UPDATE` condicionado** (`Db::hide_epic`: `... WHERE id = ? AND NOT
EXISTS (agente active do ramo)`), no molde do `change_agent_state`: com 0 linhas, a linha é relida
e o motivo contado (`EpicHide::{Done, Gone, Active(n)}`); um ramo cujos agentes acabaram entre os
dois comandos é tentado mais uma vez. As regras do ramo oculto são as do ticket oculto: `g` num
épico oculto, ou num ticket dele, é recusado (`O épico <ID> está oculto. Restaure-o (o) para abrir
um agente.`); `r` não é recusado (NS-39); um épico oculto com agente vivo deste nick continua na
árvore; o `p` só oferece épicos visíveis.

**Mover não toca arquivo nem avisa o agente.** `epics::move_ticket` (o único que chama o
`Db::set_ticket_epic`; um estrutural cobra isso, e nenhum `UPDATE tickets SET epic_id` fora de
`src/db/`) só muda o banco: `Ticket <ID> movido para o épico <EID>.`, `Ticket <ID> tirado do
épico <EID>.` e `O ticket <ID> já está aí.`. Sem `(s/N)`, porque mover se desfaz do mesmo jeito.
A pasta `.nick/docs/<ticket>/` é do ticket e não muda; o retrato do projeto é o mesmo antes e
depois, inclusive com agente ativo no ticket (unidade, `App` e PTY), e o agente não é avisado
(decisão do ticket).

**"Editar ID" do épico** (`epics::edit_id`) segue as regras do NS-36 (`docs::rename` primeiro, o
banco depois, desfazer se o banco falhar), recusado se o destino existir ou com agente `active`
**do épico** (o prompt dele cita a pasta). Os agentes dos tickets do épico não contam: a pasta
deles é a do ticket. **Reaproveitamento:** o laço da criação (`tickets::create_key_with`) e o miolo
do "Editar ID" (`tickets::check_new_key` e `tickets::rename_key_with`) foram extraídos do
`tickets.rs` para funções genéricas usadas pelos dois. Ficaram no `tickets.rs`, e não num
`keys::create_with` como o plano sugeria, porque o `keys.rs` já é o módulo do teclado (decisão do
usuário na execução). O `check_title` ganhou o irmão `check_title_of(input, noun)` (`Informe o
título do épico.`), e o `NO_UNIQUE_ID_MESSAGE` do épico troca o substantivo (decisão do usuário).
O `tickets::create` ganhou o irmão `create_in(..., epic_id, ...)`; o `create` e o `create_with`
não mudaram de assinatura, e os testes de `tickets.rs` passaram sem mudar asserção.

**O `--epic-id` e o aviso do épico.** O `--resume` do `bin/nick-agent` aceita `--epic-id <id>` no
lugar de `--ticket-id`, **exatamente um dos dois** (`informe só um dos dois: --ticket-id ou
--epic-id.`, `informe o --ticket-id <id> ou o --epic-id <id>.`), com as recusas de antes (repetido,
sem valor, `jira_id_valid`). O `resume_id_notice <antigo> <novo> [ticket|epic]` do `prompt.sh` tem o
padrão `ticket`, com o texto do NS-39 byte a byte, e com `epic` diz `O ID deste épico mudou de
<antigo> para <novo> depois que esta conversa começou. A pasta do épico agora é .nick/docs/<novo>/
(antes era .nick/docs/<antigo>/): use o caminho novo para o plano e os artefatos.`. Um estrutural
(`the_notice_of_the_new_id_of_an_epic_is_only_in_bash`, com controle positivo) proíbe o texto em
`nick/src/`. No Rust, o `Launch::Resume` troca o `ticket_key` por `holder_key: HolderKey`
(`Ticket(&str) | Epic(&str)`), e o `args` gera `--ticket-id` ou `--epic-id`. O `Uso:` cita os dois
(`(--ticket-id <id> | --epic-id <id>)`), e a mensagem de "nenhum argumento" também.

**Desenho** (`ui.rs`): o nó do épico é `<ID>  [épico]  <título>  [provisório]  [oculto]`, com o
`[épico]` em negrito (sem cor nova) logo depois do ID, para caber nas 34 colunas da profundidade 2
com um ID de 8 caracteres; oculto, apagado. O painel do épico mostra `Épico <ID> — <título>`, as
marcas, `Pasta:`, o plano (`(existe)`/`(não existe)`), `Tickets: N (M ocultos)` e `Agentes do
épico: só PO e Tech Leader.`, sem etapa, histórico nem `m`. O painel do ticket ganhou `Épico: <EID>
— <título>` quando o ticket tem épico. Na profundidade 4 sobram 26 colunas, e o `Tech Leader  ●
saída nova` (25) cabe inteiro (teste); a data é cortada antes. O `Place` ganhou `Epic`, e o
`HELP_STATES` foi de 29 para 33 estados. Rodapés:
- projeto: `↑/↓ mover · →/Enter abrir · ← fechar/subir · a novo ticket · n novo épico · r remover`;
- épico: `... · a novo ticket · e editar título · o ocultar/restaurar · g abrir agente · i editar ID`;
- ticket, **encurtado de propósito** para caber o `p`: `... · g abrir agente · m etapa · i ID · p
  épico`.

**Nenhum rodapé mudou a altura reservada** (P1 do review do NS-34): os `SIZE 23 58`/`SIZE 34 78` do
`a_resize_reaches_claude_below_the_header` e o teste do tamanho em cinco larguras passaram sem
mudar números.

**Remoção de projeto:** a confirmação ganhou, numa frase própria e só quando há épicos, `O épico
dele será apagado do banco.` ou `Os <N> épicos dele (inclusive os ocultos) serão apagados do
banco.` (decisão do usuário na execução: frase própria, que funciona também sem tickets). Sem
épicos, o texto de antes. Os épicos saem com o projeto (`ON DELETE CASCADE`).

**Ajustes do review do NS-40** (cada um com teste que falha sem a correção, provado na cópia):
- **P2.1, handlers compartilhados** (a condição do plano para os diálogos irmãos): o título e o
  ID do ticket e do épico usam um handler só (`App::key_text_field`, com o Enter e a reabertura
  por parâmetro), a confirmação do ID também (`App::confirm_edit_id`, com o `tickets::edit_id` ou
  o `epics::edit_id`), e a recusa do projeto indisponível do `i` virou `App::edit_id_refusal`. No
  `ui.rs`, `title_lines`, `id_lines` e `confirm_id_lines` desenham os dois, com o substantivo por
  parâmetro. Ficaram os diálogos irmãos (`EditEpicTitle`, `EditEpicId`, `ConfirmEditEpicId`), e não
  um alvo no diálogo, porque assim os testes de ticket que comparam `Dialog::EditTitle`/`EditId`
  não mudam (passaram sem diff). O `o` do ticket e o do épico não foram unidos: só a linha do
  restaurar se parece, e o ocultar do épico tem a contagem e outro diálogo;
- **P3.1:** `tickets::create_in` e `create_with` chamam um corpo só (`create_in_with`);
- **P3.2:** o plural do `(1 ticket)` da confirmação de ocultar e do `(1 oculto)` do painel do
  épico (`ui::count_of`). O `Tickets: N (M ocultos)` do painel do projeto (NS-38) não mudou;
- **P3.3:** o `hide_epic` que perde a corrida nas duas tentativas sem nenhum ativo para contar
  devolve `EpicHide::Changed`, com `O épico <ID> mudou enquanto era ocultado. Tente de novo.`, e
  não mais "1 agente ativo" (o teste provoca isso com um trigger de teste `RAISE(IGNORE)`);
- **P3.4:** o `set_ticket_epic` só põe um ticket num épico **visível** (`AND hidden_at IS NULL`):
  com a lista do `p` aberta, outro nick pode ter ocultado o épico, e o ticket (talvez com agente
  ativo) cairia num ramo oculto, furando a regra que o `hide_epic` garante. O `move_ticket` relê o
  épico e diz `O épico <ID> está oculto.`. Tirar de um épico oculto continua livre;
- **P3.5:** o `n` da conversa perdida lê o `hidden_refusal` uma vez só.

**Testes adaptados, com o motivo** (o mapa teste a teste está em `.nick/docs/NS-40/mapa_testes.md`):
os de banco que citavam a versão 6 (7 migrações, o texto da 7, "mais novo" com 8); os de migração
antiga v4 → v5 e v5 → v6, que passaram a gravar e ler os tickets e agentes por SQL cru (o
`TICKET_COLUMNS` e o `AGENT_COLUMNS` têm as colunas da 7), e o v5 → v6 abre só com as migrações 1
a 6; a construção de `insert_ticket` (`, None`), `insert_agent`/`count_active_agents`/
`agents::open` (`Owner::Ticket(..)`), dos literais de `Ticket`/`Agent`/`LiveAgent`/`TreeData`, dos
diálogos e ações com `holder` (`(&ticket).into()`) e do `ConfirmRemove` (`epics: 0`); as asserções
`agent.ticket_id == t` viraram `agent.owner == Owner::Ticket(t)` (o mesmo fato); o
`find_ticket_key` virou `find_key`; os rodapés do projeto e do ticket (`n novo épico`, `m etapa ·
i ID · p épico`) e as esperas do PTY por `m mudar etapa` (agora `p épico`); e o `Uso:` do
`test_nick_agent.sh`.

**Testes novos:** `db` (migração v6 → v7 com dados, os `CHECK`, o cascade, os triggers nos dois
sentidos e entre projetos, `is_trigger_abort` e cada método novo), `epics` (criar, unicidade nos
dois sentidos e a corrida pelo trigger, ocultar/restaurar, "Editar ID", mover com retrato),
`agents` (o `open` de épico e o `Holder`), `stages` (`epic_menu` em cada combinação de flags),
`launch` e `agent_session` (o `--epic-id`, no PTY real com o launcher real), `tree` (ordem,
profundidades, ramo oculto, `v`, marca, `parent`, `reveal` e `reselect`), `app` (cada item do 6.6
do plano), `ui` (nó, painéis, diálogos, rodapés e a profundidade 4), o diferencial do épico e o do
menu exclusivo do nick (`nick/tests/stages.rs`), o `bin/tests/test_nick_agent.sh` (`--epic-id`, as
recusas e o `resume_id_notice` dos dois tipos) e, no PTY (`nick/tests/agent.rs`), o épico de ponta
a ponta, mover com agente vivo, ocultar o ramo, um banco v6 de verdade e os dois estruturais. As
sabotagens, as telas e o roteiro de verificação manual ficam em `.nick/docs/NS-40/`.

## Rust no projeto: atalhos de letra sem diferença de maiúscula (NS-43)

Um bug filho do épico NS-28. Com o Caps Lock ligado (ou com Shift), o terminal manda a letra
maiúscula, e o painel só reconhecia a minúscula: `j`/`k`, `a`, `n`, `e`, `o`, `g`, `p`, `m`, `i`,
`r`, `x`, `f`, `v`, `c`, `t` etc. não faziam nada. Só o `s`/`S` das confirmações, o `q`/`Q` das
listas e o `n`/`N` do preview aceitavam as duas formas, cada um do seu jeito.

**A regra: atalho de letra não diferencia maiúscula de minúscula.** Nas telas de lista, de escolha
e de confirmação, `J` (Caps Lock, sem modificador) e Shift+`j` são o mesmo `j`.

**Um ponto só.** `keys::shortcut_key(key)` (no `keys.rs`, junto do `Shortcut`) devolve a letra
ASCII sem Ctrl nem Alt em minúscula e sem `SHIFT`, mantendo `kind` e `state`. Qualquer outra tecla
volta igual: dígitos, símbolos, `ç`/`Ç`, teclas especiais e letras com Ctrl ou Alt. É só ASCII de
propósito: nenhum atalho é letra acentuada, e o `Ç` continua sem efeito. A função é chamada uma
vez, em `App::handle_key`, logo antes do `match self.dialog.take()`, a menos que o diálogo aberto
receba texto (`Dialog::takes_text`). O `takes_text` tem `match` **exaustivo, sem `_`**: um diálogo
novo obriga quem o criar a decidir. Com isso os handlers (`key_tree`, `key_node`,
`key_ticket_node`, `key_epic_node`, `key_settings`, as listas de escolha, o preview, a conversa
perdida e as confirmações `(s/N)`) recebem a tecla já normalizada e só citam a minúscula. O
`is_yes` passou a ser `key.code == Char('s')`, o `is_quit` `Char('q')` sem modificadores e o
cancelamento do preview `Esc | Char('n')`. Fora do `mod tests`, o `app.rs` não tem
`Char('<maiúscula>')`.

**O que não passa pela regra** (recebe a tecla como foi digitada):
- os campos de texto, os sete diálogos de `takes_text() == true`: `AddPath`, `NewTicket` (título
  e ID do Jira), `EditTitle`, `EditEpicTitle`, `EditEpicId`, `EditId` e `StageForm`. Um `Q`, `S`
  ou `N` digitado neles é texto, não sai, não confirma e não cancela;
- o terminal do agente (`key_agent`), atendido antes: os bytes vão ao programa como vieram, menos
  o Ctrl+Z (NS-44);
- a captura do atalho reservado (`Dialog::CaptureShortcut`) e o próprio atalho reservado
  (`Shortcut::matches`), atendidos antes. O `Shortcut::from_key` já tinha a sua forma canônica
  (NS-34);
- as letras com Ctrl ou Alt.

**Proibido atalho com `Shift+letra` diferente da letra** nas telas de lista e de escolha. Um
atalho novo com Shift usa outra tecla (de função, com Ctrl ou Alt), nunca a maiúscula.

**Borda aceita.** Antes, Ctrl+Shift+`S` e Alt+Shift+`S` confirmavam, porque o `is_yes` ignorava os
modificadores e aceitava `S`. Agora não confirmam (cancelam, como qualquer outra tecla), porque
letra com Ctrl/Alt não é normalizada. Ctrl+`s` e Alt+`s` (minúsculas) continuam confirmando, como
antes. **Conferido na execução:** o nick não liga o protocolo de teclado estendido do crossterm
(`PushKeyboardEnhancementFlags`), então hoje o terminal manda Ctrl+Shift+S como Ctrl+`s` (o mesmo
byte 0x13), e ele continua confirmando. A borda vale para o evento com a maiúscula e Ctrl, que só
chega se esse protocolo for ligado um dia, e é ela que o teste fixa. O Ctrl+J/Ctrl+K da árvore continua movendo a seleção, como antes: os `match`
dos handlers não olham modificadores, e a normalização não toca neles.

**Não existe campo de filtro.** O ticket citava um campo de texto de filtro, mas o `f` (ocultos) e
o `v` (só ativos) são alternâncias. Os dois são atalhos e entram na regra.

**Nenhum texto visível mudou:** os rodapés continuam mostrando as minúsculas.

**Por quê:** normalizar num ponto só, depois do que precisa da tecla crua, evita repetir as duas
letras em cada `KeyCode::Char` dos handlers e esquecer uma tela nova. O `takes_text` exaustivo
protege o risco principal, que é normalizar um campo de texto.

**Testes:** `keys.rs` (`shortcut_key_lowercases_plain_letters_only`, tabela das 52 letras, das
teclas que voltam iguais e de `kind`/`state`, comparando os campos crus, porque o `==` do
`KeyEvent` do crossterm ignora a caixa de uma letra com Shift); `app.rs`
(`only_the_seven_dialogs_with_a_text_field_take_text`, com um exemplar de cada um dos 24
diálogos; `capital_letters_do_on_the_tree_what_the_small_ones_do` e
`capital_letters_do_on_the_lists_of_choice_and_the_settings_what_the_small_ones_do`, que comparam
duas fixtures iguais, com a pasta temporária, os horários e os session ids tirados do texto, e
conferem que a minúscula fez alguma coisa; `capital_s_confirms_and_capital_n_cancels_every_confirmation`,
nas nove confirmações; `capital_letters_are_typed_as_they_are_in_the_text_fields`, com Caps Lock e
com Shift, até o título gravado no banco;
`ctrl_or_alt_with_a_capital_s_does_not_confirm_and_with_a_small_s_still_does`;
`on_the_terminal_the_capital_letters_go_to_the_program_as_they_are`;
`the_capture_of_the_shortcut_gets_the_capital_letters_as_they_are`) e, no PTY
(`nick/tests/tui.rs`), `capital_letters_work_as_shortcuts_and_are_typed_as_they_are`: `A` adiciona
a pasta `Meu` e cria o ticket `TICKET MAIÚSCULO`, e `Q` sai com 0. Os testes que já existiam
passaram sem edição.

**Fora de escopo:** atalhos configuráveis além do reservado, e atalhos diferentes para maiúscula e
minúscula (proibidos acima).

## Rust no projeto: Ctrl+Z descartado no terminal do agente (NS-44)

Um bug filho do épico NS-28. Com o foco no terminal de um agente, o Ctrl+Z ia para o `claude`
como o byte `0x1A` (SUSP). O `claude` se suspendia e esperava um `fg`, e o painel desse agente
ficava travado até o agente ser encerrado. O Ctrl+Z fica ao lado do atalho reservado padrão
(`Ctrl+\`), então é fácil apertá-lo por engano.

**A regra: com o foco no terminal do agente, o nick descarta o Ctrl+Z**, sem escrever nada no PTY,
e a barra de status mostra `O Ctrl+Z não tem efeito dentro do nick.`. O foco continua no terminal.
- **Descartado:** `z` ou `Z` (Caps Lock) com Ctrl, com ou sem Shift e com ou sem Super. É tudo o
  que o `encode_key` mandaria como `0x1A` puro, porque ele ignora o Shift e o Super numa letra com
  Ctrl. A tecla segurada (`Repeat`) também é descartada. O `Release` já era descartado antes, em
  `handle_key`.
- **Não descartado:** o Alt+Ctrl+Z, que continua indo ao `claude` como `ESC 0x1A`; o `0x1A` dentro
  de texto colado (`encode_paste` não mudou); as outras letras com Ctrl, inclusive o Ctrl+C e o
  Ctrl+D; e o Ctrl+\ quando ele não é o atalho.
- **Com o foco na lista, nada mudou.** O Ctrl+Z continua sem efeito ali e não mostra a mensagem.

**Onde fica.** O predicado puro `keys::is_suspend_key(&KeyEvent)` fica no `keys.rs`, ao lado do
`shortcut_key`, separado do `encode_key`. O `encode_key` não mudou e continua sem regra de atalho.
O descarte é a primeira regra de `App::key_agent`, antes do Shift+PageUp/PageDown e do
`encode_key`. Ele devolve `Action::None` e grava a mensagem no `status`. A regra é fixa e não usa
`Shortcut`, de propósito: um `Shortcut` só criaria a dúvida de qual dos dois ganha. O
`CLAUDE_SHORTCUTS` não mudou, e o Ctrl+Z continua recusado como atalho reservado, na captura e no
valor lido do banco.

**A mensagem é curta de propósito.** Ela é a constante `CTRL_Z_IGNORED` do `app.rs`, com 39
caracteres, e cabe na linha que a barra sempre ocupa. Uma mensagem de duas linhas mudaria a altura
do painel e redimensionaria **todos** os PTYs, com SIGWINCH e falsos `● saída nova` em todo
`claude` (o problema registrado no NS-34). Não aumente o texto sem conferir isso. A mensagem fica
até outra ação limpar a barra, como as outras mensagens: as teclas mandadas ao programa não limpam
o `status`. Um segundo Ctrl+Z grava o mesmo texto.

**Por quê:** dentro do nick não há shell com controle de jobs para dar `fg` no `claude`
suspenso, e o painel do agente travava até ele ser encerrado.

**Quando rever:** se o `claude` passar a usar o Ctrl+Z para outra coisa (hoje a doc dele lista o
Ctrl+Z como suspender), ou se o nick ganhar um jeito de retomar um processo suspenso.

**Testes:** `keys.rs` (`is_suspend_key_is_ctrl_z_with_or_without_shift_and_caps_lock`, uma tabela
com Shift, Caps Lock, Super, `Repeat`, Alt e as outras 25 letras com Ctrl); `app.rs`
(`on_the_terminal_ctrl_z_is_dropped_and_the_status_says_so`, com o texto literal da mensagem;
`on_the_terminal_the_other_ctrl_keys_and_alt_ctrl_z_still_go_to_the_program`;
`on_the_list_ctrl_z_does_nothing_and_shows_no_message`); `ui.rs`
(`the_ctrl_z_message_fits_one_row_and_keeps_the_panel_size`, a 100 e a 60 colunas); e, no PTY
(`nick/tests/agent.rs`), `ctrl_z_never_reaches_claude_and_the_status_says_so`. Nele, depois de
`\x1a` e `x`, o `claude` falso recebe só `78`. Os testes que já existiam passaram sem edição,
inclusive o `Ctrl+Z → \x1a` do `encode_key`.

**Fora de escopo:** retomar um agente que já está travado, e as outras teclas de sinal.

## Rust no projeto: Shift+Enter no terminal do agente (NS-49)

Um bug filho do épico NS-28. No Konsole, o Shift+Enter quebra a linha no `claude` aberto direto,
mas dentro do nick enviava a mensagem incompleta. Só o Alt+Enter quebrava a linha. Eram duas
causas. O nick não pedia ao terminal de verdade o modo de teclado que distingue o Shift+Enter (o
pedido que o `claude` faz fica preso no terminal embutido, o `vt100`). E o `encode_key` descartava
o Shift do Enter.

**O mecanismo do Konsole** (sondagem do Tech Leader em 2026-11-21, `KONSOLE_VERSION=260803`,
`TERM=xterm-256color`, fora do tmux):

| Situação | Bytes recebidos |
|---|---|
| Pergunta `CSI ? u` + DA1 (`CSI c`) | `ESC[?0u` e depois `ESC[?62;1;4c`: **suporta o protocolo de teclado do kitty** e responde na hora |
| Modo legado, Shift+Enter | `ESC O M` (SS3 M, o Enter do teclado numérico) |
| Modo legado, Alt+Enter | `ESC` + Enter |
| Kitty flag 1, Shift+Enter | `ESC[13;2u` |
| Kitty flag 1, Esc | `ESC` (igual ao legado) |
| Kitty flag 1, `Ctrl+\` | `ESC[92;5u` |
| Kitty flag 1, Ctrl+C | `ESC[99;5u` |
| `modifyOtherKeys` nível 2 (`CSI > 4;2 m`), Shift+Enter | `ESC O M`: **o Konsole ignora esse modo** |

O Konsole distingue o Shift+Enter pelo **protocolo de teclado do kitty**. No modo legado, o
`ESC O M` não serve: o parser do crossterm 0.29 (`event/sys/unix/parse.rs`, ramo `b'O'`) só aceita
`A B C D H F P..S`, e a sequência que ele não entende é descartada (`source/unix/mio.rs`,
`Err(_) => buffer.clear()`). Com o protocolo kitty, o crossterm entrega `KeyCode::Enter` com
`SHIFT`. O `modifyOtherKeys` não serve: o Konsole o ignora, e o crossterm também não decodificaria
o `CSI 27;2;13~`.

**A regra:**
- **só a flag 1 do kitty** (`DISAMBIGUATE_ESCAPE_CODES`, `ESC[>1u`). As outras flags (eventos de
  soltar, todas as teclas como escape, texto associado) mudariam muito mais teclas e trariam
  eventos `Release`/`Repeat` que hoje não chegam. O crossterm já decodifica o `CSI u` nos mesmos
  `KeyEvent` do modo legado: não há parser nem dependência nova;
- **detecção uma vez, ao abrir**, com `crossterm::terminal::supports_keyboard_enhancement()`
  (`ESC[?u` + `ESC[c`, até 2 s). Sem suporte, ou com erro (o terminal não respondeu), **nada muda**:
  o nick não manda o `ESC[>1u` nem o `ESC[<1u`, e não há mensagem;
- no `encode_key`, o **Shift+Enter vira `ESC CR`**, a mesma sequência do Alt+Enter, que o `claude`
  entende como quebra de linha. Shift+Alt+Enter também, com um ESC só. A regra não depende do modo:
  no legado o crossterm nunca entrega Enter com Shift. **O Ctrl+Enter continua `CR`**, e as outras
  combinações com Shift (ex: Shift+Tab) não mudam;
- no `encode_key`, uma **letra minúscula com Shift e sem Ctrl vai como maiúscula** (`capital`,
  review do NS-49, P1). Com o modo ligado, o Konsole manda o Alt+Shift+a como `ESC[97;4u`, com a
  letra minúscula, e o `claude` recebia `ESC a`, quando no legado recebia `ESC A`. Agora volta a
  receber `ESC A`. Uma letra cuja maiúscula tem mais de um caractere (o `ß`) fica como está. No
  legado nada muda, porque o crossterm nunca entrega minúscula com Shift. Os atalhos do nick já
  tratavam as duas formas (`Shortcut::from_key`);
- o modo é **desligado** (`ESC[<1u`) ao sair (`q`, erro, SIGTERM, SIGHUP, panic) e enquanto o
  `bin/nick-init` ou o `$EDITOR` rodam, e **religado** na volta, sem perguntar de novo;
- sem variável de ambiente para desligar: a detecção já deixa como hoje o terminal sem suporte.

**Onde fica.** `terminal.rs`: o `static KEYBOARD_ENHANCED: AtomicBool` (global como o `PANICKING`,
porque o panic hook não tem acesso ao guard), `push_keyboard_enhancement()` (manda o Push e marca
`true`) e `pop_keyboard_enhancement()` (manda o Pop **só se** estava ligado, com `swap(false)`), com
as versões `_to(out)` testáveis sem terminal. O `restore()` chama o pop como **primeira** sequência,
e ele é o ponto único do `Drop` do `TerminalGuard`, do panic hook e do `suspend`: todos os caminhos
de saída ficam cobertos sem código em cada um, e dois `restore()` não mandam dois pops.
`TerminalGuard::enter` detecta (campo `keyboard_enhancement`) depois do `enable_raw_mode` e do
`EnterAlternateScreen`, e **antes do `spawn_input`** (`main.rs`, com comentário): a consulta lê a
entrada e disputaria os bytes com a thread de entrada. `suspend` religa depois do
`EnterAlternateScreen` e antes do `clear`. `keys.rs`: o braço do `KeyCode::Enter` do `encode_key`
e a função `capital`, que o braço do `KeyCode::Char` chama.

**Alt+Shift no Konsole e por que só a flag 1** (sondagem do review, 2026-11-21, com o script
`.nick/docs/NS-49/ns49_sonda_alt_shift.sh` e o resultado no `.txt` ao lado):

| Tecla | Legado | Kitty flag 1 | Kitty flags 1 e 4 (`ESC[>5u`) |
|---|---|---|---|
| Alt+Shift+a | `ESC A` | `ESC[97;4u` (o `a` minúsculo) | `ESC[97;4u`, igual |
| Alt+Shift+1 | `ESC !` | `ESC[33;4u` (o próprio `!`) | `ESC[33::49;4u` |
| `Ctrl+\` | — | `ESC[92;5u` | `ESC[92::60;5u` |

O Konsole manda o símbolo já com o Shift (o `!`, e não o `1` que a especificação do kitty pede),
então o Alt+Shift+símbolo e os atalhos `alt+<símbolo>` não mudaram. Na letra ele manda a
minúscula, e é isso que o `capital` corrige. A flag 4 (`REPORT_ALTERNATE_KEYS`) não ajuda: o
Konsole não manda a maiúscula como alternativa da letra e só acrescenta o código da tecla do
layout (o terceiro campo), que o crossterm não usa. Ela traria risco sem ganho.

**Ligado também na lista.** O modo não entra nos modos da tela do agente
(`enter_agent_modes`/`leave_agent_modes`, mouse e colagem): fica ligado o tempo todo na TUI, para
as teclas chegarem na mesma forma em qualquer foco. Se ligasse e desligasse a cada troca de foco,
cada `Ctrl+\` mudaria a forma das teclas seguintes.

**O harness de PTY responde ao DA1.** O nick manda `ESC[?u` + `ESC[c` ao abrir. Sem resposta, cada
abertura de todo teste de PTY esperaria 2 s. O `Nick::spawn` (`tests/common/mod.rs`) responde
**sempre** ao `ESC[c` com `ESC[?62c`, então o crossterm conclui na hora que não há suporte e os
testes que já existiam continuam no modo legado. O modo kitty do harness (`World::spawn_kitty`,
`Nick::spawn_as(.., Keyboard::Kitty)`) responde também ao `ESC[?u` com `ESC[?0u`, antes do DA1,
como o Konsole. Cada pergunta é respondida uma vez, como o `ESC[6n`, cuja resposta não mudou.

**A ajuda**, com o foco no terminal, é `"{atalho} voltar à lista · Alt+Enter nova linha"`, sempre,
com ou sem suporte. O Alt+Enter funciona em qualquer terminal; mostrar o Shift+Enter exigiria
passar o suporte até a UI só por um rótulo, e quem tem o Shift+Enter já o usa por hábito. O texto
cabe nas linhas que o `help_rows` reserva: a troca de foco não redimensiona os PTYs (NS-34).

**Por quê:** o operador usa o Shift+Enter o tempo todo e mandava mensagens incompletas ao `claude`.

**Quando rever:** se outro terminal com suporte ao protocolo kitty quebrar alguma tecla (o teste
manual só cobriu o Konsole; um terminal que siga a especificação à risca mandaria o Alt+Shift+1
como `1`, e o `claude` receberia `ESC 1`), se o crossterm passar a entender o `ESC O M`, ou se o nick ganhar um
protocolo de teclado no terminal embutido (hoje o `claude` continua no modo legado dentro do nick).

**Testes:** `keys.rs` (`shift_enter_is_sent_as_alt_enter`, com Shift+Alt, Ctrl+Enter, `Repeat` e
`Release`; `kitty_keys_encode_like_the_legacy_ones`, com os `KeyEvent` que o crossterm produz para
`ESC[92;5u`, `ESC[99;5u`, `ESC[122;6u` e `ESC[97;65u`, e o atalho padrão coberto por
`the_default_matches_ctrl_4_and_ctrl_backslash_of_crossterm`;
`a_small_letter_with_shift_is_sent_capital`, com as formas do kitty e do legado, Ctrl e o `ß`); `terminal.rs`
(`the_keyboard_mode_is_popped_only_when_pushed`); `ui.rs`
(`the_terminal_help_shows_alt_enter_for_a_new_line`, `the_terminal_help_keeps_the_reserved_help_rows`);
no PTY, `nick/tests/tui.rs` (`without_keyboard_enhancement_the_nick_sends_no_keyboard_mode`, com a
abertura em menos de 1,5 s; `with_keyboard_enhancement_the_mode_is_on_and_off_on_quit`;
`with_keyboard_enhancement_an_error_turns_the_mode_off_before_the_message`;
`with_keyboard_enhancement_a_panic_in_the_main_thread_turns_the_mode_off` e `..._in_a_secondary_thread_...`;
`with_keyboard_enhancement_the_mode_is_off_during_the_init_and_on_after`, com uma pergunta só) e
`nick/tests/agent.rs` (`shift_enter_reaches_claude_as_alt_enter`;
`alt_enter_still_reaches_claude_as_alt_enter`, com `ESC CR` nos dois modos e `ESC[13;3u` no kitty;
`enter_still_sends`; `with_the_keyboard_mode_on_ctrl_c_reaches_claude`;
`with_the_keyboard_mode_on_alt_shift_keys_reach_claude_as_before`, com `ESC[97;4u` → `ESC A` e
`ESC[33;4u` → `ESC !`, as sequências do Konsole, e as formas do legado;
`with_the_keyboard_mode_on_the_default_shortcut_toggles_the_focus`;
`with_the_keyboard_mode_on_a_shortcut_of_the_settings_toggles_the_focus`, com `ctrl+]` no banco;
`with_the_keyboard_mode_on_esc_and_the_letters_work_on_the_list`;
`with_the_keyboard_mode_on_ctrl_z_never_reaches_claude`;
`with_the_keyboard_mode_on_it_is_off_during_the_editor_and_on_after`;
`with_the_keyboard_mode_on_sigterm_turns_it_off`). A ordem das sequências no `raw()` é conferida
por `common::assert_in_order`. Os testes que já existiam passaram sem edição, menos os dois de
`ui.rs` que fixavam o texto antigo da ajuda do terminal.

**O `claude` falso do modo `winsize` sai no Ctrl+C com `trap 'exit 130' INT`** (review do NS-49,
O1). Sem o trap, o `a_resize_reaches_claude_below_the_header` falhava de vez em quando: quando o
SIGINT chega no instante em que um `sleep` do laço termina normalmente, o bash conclui que o filho
tratou o sinal e continua no laço. Numa sonda fora do nick (um laço `sleep 0.05` que recebe o
SIGINT no grupo de processos, como o terminal faz), isso aconteceu em cerca de 1 vez a cada 100, e
nunca com o trap. Um modo novo do `claude` falso que precise terminar no Ctrl+C usa o mesmo trap.

**Fora de escopo:** outras combinações com Shift além do Enter, configurar o terminal do usuário,
as flags do kitty além da 1, um protocolo de teclado no terminal embutido e o `modifyOtherKeys`.

## Rust no projeto: retomada segue a troca de sessão do claude (NS-50)

Bug do épico NS-28. O nick retomava um agente por `claude --resume <session_id>` (NS-39), mas o
`claude` pode continuar a conversa **noutra sessão**, e aí a retomada abria a conversa antiga.
Quando troca, o `claude` acrescenta ao arquivo da sessão antiga um registro como este (formato
real, `claude` 2.1.328):

```json
{"type":"continued-in","timestamp":"2026-11-21T14:41:33.908Z","sessionId":"<antiga>","continuedInSessionId":"<nova>"}
```

O arquivo da sessão nova fica **na mesma pasta de projeto** e começa com um `derived-from` (`"via":
"fork"`). **O nick passa a depender desse detalhe interno do `claude`** (impacto aceito no
ticket, no molde do layout do NS-39). Se uma versão nova mudar o registro, a cadeia para na
sessão guardada, e a retomada volta a ser a de antes do NS-50.

**A cadeia** (`claude_conversation_latest <config_dir> <dir> <session_id> <var>`, em
`lib/claude/claude.sh`, com os mesmos parâmetros da `claude_conversation_file`, que monta o
caminho de cada salto):
1. começa pela **sessão efetiva** do agente, cujo arquivo precisa existir. Sem ele, a resposta é a
   "conversa que não existe mais" do NS-39 (`conversation=false`);
2. no arquivo da sessão atual vale só o **último** registro com `"type":"continued-in"`, lido com
   `grep -F` (só as linhas do registro, sem carregar a conversa). O `sessionId` dele precisa ser o
   da sessão atual, e o `continuedInSessionId`, um UUID minúsculo (`8-4-4-4-12` hexadecimal,
   conferido com `grep -Eqx` em `LC_ALL=C`). Se algum falhar, o registro é ignorado e a cadeia
   para. Um registro bom anterior não salva a cadeia;
3. o salto só vale se o arquivo da sessão seguinte existir na mesma pasta de projeto. A
   validação do UUID impede um `/` ou um `..` de sair dela;
4. a cadeia também para num ciclo (sessão já visitada) e em 50 saltos (literal dentro da função:
   uma global nova quebraria a "carga limpa" da lib);
5. um arquivo ilegível para a cadeia nele, sem nada no stderr (o status 2 do `grep` é tratado). O
   arquivo inicial ilegível dá a própria sessão: a conversa existe, e a retomada usa a sessão
   guardada, como antes;
6. **segue sempre**, mesmo com conversa depois do registro (decisão do analista no plano).
   Consequência aceita: um agente que já tinha sido retomado errado antes do NS-50 pula para a
   sessão nova, e o que foi feito na retomada errada fica só no arquivo antigo, que o operador
   ainda pode abrir à mão.

A função segue as regras da lib sem exceção: `local __claude_*` em tudo, `[ ]`, `printf -v`, sem
global do chamador, sem `CLAUDE_*`, funciona com e sem `set -euo pipefail`, e o `grep` sem
resultado ou com arquivo ilegível não derruba o chamador (`|| __claude_line=""`).

**`bin/nick-agent --has-conversation <uuid>`:** o `<uuid>` é a sessão efetiva, que o Rust passa.
Sem o arquivo, a saída continua a de antes, uma linha só, `conversation=false`. Com o arquivo, são
duas linhas: `conversation=true` e `session=<última sessão da cadeia>` (o próprio `<uuid>` sem
cadeia). O código de saída é 0 nos dois casos, e o modo continua sem gravar nada e sem depender da
configuração. O `launch::parse_has_conversation` devolve `Result<Option<String>, String>` (`None`:
perdida; `Some(sessão)`) e recusa, com a mensagem de antes (`O bin/nick-agent respondeu algo
inesperado na verificação da conversa.`), qualquer outra saída com código 0: `conversation=true`
sem `session=`, `session=` vazio, maiúsculo, curto ou com outro caractere, linhas a mais, ordem
trocada ou `conversation=false` com `session=`.

**`agents.resume_session_id` (migração 8):** `ALTER TABLE agents ADD COLUMN resume_session_id
TEXT` e `CREATE UNIQUE INDEX agents_resume_session_id ON agents(resume_session_id)` (no SQLite,
vários `NULL` não violam o índice). O **`session_id` fica fixo** (decisão do analista): continua
sendo o `--session-id` da abertura, o `UNIQUE` de antes e **a chave do lock**
(`<estado>/agents/<session_id>.lock`). A **sessão efetiva** (`Agent::effective_session`) é o
`resume_session_id` ou, se ele for `NULL`, o `session_id`. O critério do ticket pedia "atualizar o
`session_id`", e foi reescrito no plano: o que muda é a sessão de retomada.

**Por que o lock continua no `session_id`:** abrir, retomar, a limpeza da abertura do nick e o
`held_elsewhere` continuam travando o mesmo arquivo. Assim dois nicks, inclusive um de versão
anterior aberto ao mesmo tempo (que só conhece o `session_id`), disputam a mesma trava, e nenhum
agente é retomado duas vezes. Um teste segura o `flock` de `<session_id>.lock` num processo filho
depois da troca para a sessão B, e a retomada é recusada. A sabotagem que trava pela sessão
efetiva faz esse teste falhar.

**A troca é gravada sob o lock, junto com o `active`.** O `check_resume` pergunta pela sessão
efetiva (`--has-conversation`), o `agents::resume(db, dir, agent, latest)` pega o lock do
`session_id` e só então o `Db::resume_agent(id, latest)` faz um único `UPDATE` condicionado ao
estado, com `state = 'active'` e, só quando `latest` difere da efetiva, `resume_session_id =
latest` (`SessionSwap::Swapped`). A efetiva é lida antes do `UPDATE`: só quem tem o lock grava a
coluna, então ela não muda nesse meio-tempo. Se o `UPDATE` bater no `UNIQUE` (outro agente já
aponta para essa sessão), só a mudança de estado é feita (`SessionSwap::Taken`), e a retomada
segue pela efetiva. As recusas de antes (`Gone`, `Archived`, a releitura que falha) não mudaram:
o lock sai e nada é gravado. O PTY abre com a sessão efetiva da linha **relida**.

**A barra** soma ao `O agente <nome> foi retomado.`, antes do aviso da pasta do NS-39, quando há:
- troca feita: ` A conversa tinha continuado em outra sessão do claude (<nova>), e o nick passou
  a usá-la.`;
- troca recusada pelo `UNIQUE`: ` A conversa continuou na sessão <nova> do claude, mas outro
  agente deste nick já usa essa sessão: a retomada seguiu pela sessão guardada.`

O ID passa por `printable`. Sem cadeia, a barra é a de antes, byte a byte. O painel do agente
mostra `Sessão: <efetiva>` e, quando há `resume_session_id`, a linha `Sessão na abertura:
<session_id>`.

**O que dispara a troca e como evitar (D5).** Fontes:
- `claude --help` (2.1.328): `--bg, --background` ("With --resume <session-id>, continues that
  session in the background under the same ID, or starts a copy and says so when the session is
  already running") e `--fork-session`. O nick não passa nenhum dos dois;
- <https://code.claude.com/docs/en/agent-view>: o `/background` (ou `/bg`) "move the current
  conversation into a background session", e o `←` num prompt vazio "backgrounds it and opens
  agent view". Se uma ferramenta estiver rodando, o `claude` espera ela terminar ("If a tool is
  running when you press `←`, Claude Code waits for it to finish before backgrounding"). Isso bate
  com o aviso visto pelo usuário, `Backgrounding after the current tool finishes…`, e com o
  `"via":"fork"` do arquivo novo. A mesma página diz como desligar: "set the `disableAgentView`
  setting to `true` or set the `CLAUDE_CODE_DISABLE_AGENT_VIEW` environment variable";
- <https://code.claude.com/docs/en/settings-reference>, `disableAgentView` (escopo "Any file",
  booleano): "`true`: Claude Code turns off `claude agents`, `--bg`, `/background`, and the
  on-demand supervisor". O `leftArrowOpensAgents` (o `←`) é "Global config": só vale no
  `~/.claude.json` ("Claude Code ignores this key in `settings.json`"), que o nick não grava. Ele
  "Appears in `/config` as **← opens agents** when agent view is available";
- <https://code.claude.com/docs/en/env-vars>: `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS` é outra
  coisa (o `Ctrl+B` e o `run_in_background` das ferramentas), e não desliga as sessões em segundo
  plano. Ele **não** foi posto: comandos e subagentes em segundo plano dentro da sessão continuam.

Nenhuma fonte diz com todas as letras que o segundo plano é o que gera o `continued-in`, nem que
o `disableAgentView` desliga o `←`. A conferência fica no roteiro manual
(`.nick/docs/NS-50/verificacao_manual.md`), porque o `claude` real não roda na jail do nick.
**Decisão do usuário na execução:** `"disableAgentView": true` entrou no
`lib/claude/permissions.json`, que já é o `--settings` de toda sessão do nick e do `auto_scrum`.
O diferencial `auto_scrum` × `nick-agent` do NS-33 continua valendo sem exceção. O que se perde
nessas sessões: o `/background`/`/bg`, o `←` que manda para segundo plano, o "Move to background
and exit", o `claude --bg`/`claude agents` pelo próprio agente, e o `/fork` passa a abrir um
subagente em vez de copiar a sessão (agent-view: "When agent view is turned off, `/fork` starts
a forked subagent instead of copying the session"). As sessões abertas à mão, fora do nick e do
`auto_scrum`, não mudam. A cadeia do `continued-in` continua como rede de segurança: para um
agente que já trocou de sessão, para uma versão do `claude` que ignore a chave e para outro
gatilho da troca, se houver.

**Mudou a regra do `permissions.json`:** até o NS-49 ele era só `{"permissions": {"deny": [...],
"ask": [...]}}`, sem nenhuma outra chave. Desde o NS-50 tem também, e só, `"disableAgentView":
true`. O teste de forma (`json_shape_ok` de `lib/tests/test_claude.sh`) exige exatamente isso. Os
controles dele ganharam a chave, e três casos novos são recusados (sem a chave, `false` e
`"true"`). Um controle positivo mostra que o formato certo, com listas vazias, é aceito.

**Testes adaptados, com o motivo:**
- `lib/tests/test_claude.sh`: o `json_shape_ok`, o título e a descrição do teste de forma, e os
  arquivos de controle (`allow`, `mode`, `number`, `no_ask`), que ganharam a chave
  `disableAgentView` para continuar recusados só pelo defeito de cada um (a regra do
  `permissions.json` mudou, acima). A lista do `clean_func` ganhou a `claude_conversation_latest`;
- `bin/tests/test_nick_agent.sh`: as quatro asserções de `conversation=true` do NS-39
  (`CLAUDE_CONFIG_DIR`, `$HOME/.claude`, os três locales e o link simbólico) passaram a esperar
  também a linha `session=<uuid>` (a saída nova do `--has-conversation`). O
  `conversation=false` não mudou;
- `nick/src/launch.rs`: o `the_conversation_check_is_exactly_one_line` foi substituído por
  `the_conversation_check_is_false_or_true_with_the_session` e
  `the_conversation_check_refuses_anything_else` (o formato novo; as saídas recusadas de antes
  continuam na lista). No `run_has_conversation_runs_the_real_launcher_in_the_project`,
  `Ok(false)`/`Ok(true)` viraram `Ok(None)`/`Ok(Some(<sessão>))` (o tipo novo);
- `nick/src/db/mod.rs`: os testes de versão (7 → 8: `a_new_database_gets_version_8_...`, que
  trocou o número no nome, `opening_twice_duplicates_nothing`, e 8 → 9 no banco "mais novo" de
  `a_newer_database_is_refused_without_being_written` e
  `a_database_one_version_newer_is_refused_too`); o `migration_1_is_pinned` (8 migrações e o
  texto da 8); o `a_version_6_database_with_agents_migrates_to_7_keeping_every_row`, que passou a
  abrir só com as migrações 1 a 7 (o teste da 8 é o dele), sem mudar asserção; e as cinco
  chamadas `db.resume_agent(a.id)` de `resume_agent_makes_a_stopped_or_active_one_active_...` e
  `an_unknown_state_is_not_resumed_archived_nor_unarchived`, que viraram
  `db.resume_agent(a.id, "s-1").unwrap().0` (só sintaxe: a sessão efetiva, sem troca). No PTY,
  o `a_real_version_6_database_opens_migrated_with_the_tree_of_before` (`nick/tests/agent.rs`)
  passou a esperar a versão 8 depois da abertura (a mais nova). Os testes
  de migração antiga já gravavam e liam os agentes por SQL cru (NS-39/NS-40), então nenhum
  precisou passar a usar os helpers;
- `nick/src/agents.rs`: as chamadas de `resume`/`resume_with` de
  `resuming_takes_the_lock_before_the_update`,
  `a_lock_held_by_another_nick_refuses_to_resume_and_changes_nothing`,
  `an_active_agent_whose_lock_is_free_is_resumed`,
  `a_gone_or_archived_agent_is_not_resumed_and_the_lock_goes` e
  `a_resume_whose_row_cant_be_read_again_stops_it_and_releases_the_lock` (só sintaxe: a sessão
  `"s-1"` e o par `(OpenAgent, SessionSwap)`), e o helper `named` (o campo novo do `Agent`);
- `nick/src/tree.rs` (helper `agent`, o campo novo), `nick/src/ui.rs` (a assinatura do falso do
  launcher, que não é chamada) e `nick/src/app.rs` (o `FakeLauncher` devolve o tipo novo e ganhou
  `latest`; os testes do NS-39 que configuram `conversation = Ok(false)` não mudaram).

Os testes da conversa perdida, abrir agente, a limpeza da abertura, o `held_elsewhere`,
arquivar/desarquivar, o aviso do ID novo e o diferencial `auto_scrum` × `nick-agent` passaram sem
mudar asserção.

**Testes novos:** `lib/tests/test_claude.sh` (`claude_conversation_latest`: sem registro, um
salto, dois saltos com e sem `set -euo pipefail`, começando do meio, salto para arquivo inexistente
e para outra pasta de projeto, vários registros (vale o último), conversa depois do registro, sete
registros malformados (`continuedInSessionId` ausente, vazio, maiúsculo, com caractere estranho,
com `../`, curto e `sessionId` de outra sessão) com controle positivo, ciclo A → B → A e C → C,
limite de 50 saltos com controle, arquivo seguinte, do meio e inicial ilegíveis, só a variável de
destino muda (retrato `compgen -v`), a leitura por `grep -F` sem carregar o arquivo inteiro; e o
`disableAgentView` no formato do JSON); `bin/tests/test_nick_agent.sh` (a cadeia de dois saltos
via `CLAUDE_CONFIG_DIR` e via `$HOME/.claude`, o salto para arquivo inexistente, o arquivo
inicial ilegível, o "sem arquivo" com uma linha só mesmo com B e C na pasta, os retratos
recursivos do projeto e da pasta do `claude`, e o `"disableAgentView": true` no `--settings` do
lançamento e da retomada); `launch` (cada recusa do formato novo, o launcher real com cadeia);
`db` (v7 → v8 com dados, o `UNIQUE` com vários `NULL`, troca feita, mesma sessão, `Archived`,
`Gone` e estado desconhecido sem gravar, `Taken`); `agents` (o lock do `session_id` durante o
`update` e nenhum da sessão nova, o lock depois da troca com um processo filho segurando o
`flock`, com `flock -o` para que o `sleep` não herde a trava, e o `Taken`); `app` (a sessão
efetiva no `has_conversation`, a sessão nova no `spawn` e no banco, a retomada seguinte partindo
dela com a barra de antes, o `Taken`, a soma com o aviso da pasta e o `printable`); `ui` (o
painel com e sem `resume_session_id`); e no PTY (`nick/tests/agent.rs`):
`a_resume_follows_the_chain_of_sessions_and_the_next_one_starts_from_its_end` (`--resume <C>`, a
barra, o banco, o lock e, depois de fechar e reabrir o nick sem os arquivos de A e B,
`--resume <C>` direto) e `a_resume_without_the_record_resumes_the_session_stored`. As
sabotagens ficam em `.nick/docs/NS-50/sabotagens.md`, as telas em `.nick/docs/NS-50/telas.txt`, e
a verificação com o `claude` real em `.nick/docs/NS-50/verificacao_manual.md`.

**Ajustes do review do NS-50** (`.nick/docs/NS-50/review.md`; só código de teste, sem mudar
asserção):
- **um `hold_lock` só** (ponto 1): o processo filho que segura o `flock` de uma trava ("outro
  nick") existia em `app.rs` (NS-39) e foi copiado para `agents.rs`, e as cópias já divergiam.
  Ele virou `agents::test_support::hold_lock` (`#[cfg(test)] pub(crate)`), usado pelos testes dos
  dois módulos, e a cópia de `app.rs` saiu. O de `app.rs` **ganhou o `flock -o`**: sem ele, o
  `sleep 30` herda a trava, e o `child.kill()` mata só o `flock`, deixando um `sleep` órfão
  segurando a trava. Não quebrava nada (cada teste tem o seu tempdir), mas um teste que
  retomasse depois do `kill` ficaria preso, como aconteceu com o
  `after_the_swap_the_lock_is_still_the_one_of_the_opening`. A espera também mudou: antes, o
  `app.rs` esperava pelo `held_elsewhere`, que cria e apaga o arquivo da trava e podia disputar
  com o `flock` que o estava abrindo. Agora ela abre o arquivo sem criar e só tenta a trava;
- **um padrão só das variáveis do bash** (ponto 2): o `clean_bash_vars` de
  `lib/tests/test_claude.sh` foi para o topo da suíte e é usado pelo retrato da
  `claude_conversation_latest` e pela carga limpa. O `chain_bash_vars`, que era uma cópia do
  mesmo literal, saiu.

As três observações do review que não pediam mudança ficam registradas: as asserções
`'"disableAgentView": true'` dependem da formatação do `permissions.json` (o `--settings` leva o
conteúdo como ele é); o `UNIQUE` da migração 8 não cruza o `resume_session_id` de um agente com o
`session_id` de outro (exigiria o `claude` continuar numa sessão criada pelo nick, o que não é
realista); e a validação do UUID faz um `grep` por salto (custo desprezível com o limite de 50).

## Rust no projeto: a gata Nick no painel (NS-51)

> **Mudou no NS-55** (seção "Rust no projeto: letreiro do nó selecionado na árvore (NS-55)"): o
> laço acorda também pelo letreiro do nó selecionado. O `idle_wait` recebe o `App::next_wake` (o
> mais próximo entre a gata e o letreiro) no lugar do `App::mascot_wake`, que continua existindo e
> igual. O `Nick::spawn` dos testes de PTY põe também o `NICK_TEST_MARQUEE=still`.

História filha do épico NS-28. A Nick, a gata do operador, aparece desenhada em pixel art na base
da coluna da esquerda do `nick` (a árvore e as Configurações), dentro da borda e encostada à
esquerda. Ela pisca de vez em quando e, raramente, mostra um balão "Miau!" ao lado.

**Sempre visível, sem opção de desligar. Ela some só por falta de espaço.** A regra é fixa
(decisão do analista no plano): ela aparece se o **interior** da coluna tiver altura ≥
`MASCOT_ROWS + TREE_MIN_ROWS` = 7 + 8 = **15 linhas** e largura ≥ `MASCOT_COLS` = **17 colunas**.
Abaixo disso ela não é desenhada, e a árvore (ou as Configurações) usa o interior todo. A regra
**não depende de quantos nós estão visíveis**, para a gata não aparecer e sumir enquanto o
operador abre e fecha ramos. Uma árvore maior que o espaço rola como antes (`ListState`). A regra
da largura evita desenhar a gata cortada: com a coluna em `min(40, largura/2)`, uma janela de menos
de 38 colunas não comporta a gata. Com ela, a árvore fica com as `interior − 7` linhas de cima, e
ela com as 7 de baixo.

**A arte de referência:** o original aprovado (variante B, "patinhas na borda") fica em
`.nick/docs/NS-51/nick_sprite.py`, não versionado, como tudo em `.nick/docs/`
(`python3 .nick/docs/NS-51/nick_sprite.py` mostra as três poses). No repositório, a fonte é
`REST`/`BLINK`/`PALETTE` em `nick/src/mascot.rs`: um pixel por caractere, `.` transparente. Cada
célula do terminal junta 2 pixels na vertical: os dois coloridos → `▀`, com o de cima no `fg` e o
de baixo no `bg`; só o de cima → `▀`, só com `fg`; só o de baixo → `▄`, só com `fg`; nenhum → a
célula não é tocada (fica o fundo da coluna). Os testes de `mascot.rs` têm uma **cópia literal
independente** das linhas aprovadas (`APPROVED_REST`, `APPROVED_BLINK`, `APPROVED_PALETTE`), no
estilo do `migration_1_is_pinned`. O teste de `ui.rs` confere, a partir dessa cópia, o símbolo, o
`fg` e o `bg` de cada célula na tela, nas duas poses, com e sem truecolor, na árvore e nas
Configurações. **Mudar a arte exige mudar os dois, de propósito.** A `OPEN` do `.py` não entrou no
código: ela é só a base de onde a `PAWS` saiu.

**O balão é o lugar futuro dos avisos da "Nick assistente"** (outro ticket). Ele fica à direita da
gata, com 1 coluna de espaço, nas 3 últimas linhas dela (17 colunas, como no `.py`), e precisa de
`17 + 1 + 17 = 35` colunas no interior (o padrão tem 38). Se não couber, o balão não é desenhado,
mas o relógio do miado segue normalmente: um balão cortado não seria legível. **Regra a definir
naquele ticket: o aviso tem prioridade sobre o miado.**

**Intervalos** (constantes em `mascot.rs`): piscar a cada 6 a 20 s sorteados (`BLINK_EVERY`), por
150 ms (`BLINK_FOR`); miar a cada 3 a 10 min sorteados (`MEOW_EVERY`), por 3 s (`MEOW_FOR`). O
próximo intervalo é sorteado quando o evento anterior termina (e, na criação, a partir do instante
inicial: primeiro o do piscar, depois o do miado). Os dois são independentes: ela pode piscar com o
balão aberto.

**Relógio e sorteio injetáveis, numa máquina de estados pura.** `nick/src/mascot.rs` não tem I/O,
banco nem tela, no molde de `tree.rs` e `stages.rs`. O relógio entra por parâmetro
(`Mascot::new(now, dice)`, `tick(now) -> bool`, `next_wake() -> Option<Instant>`,
`frame() -> Frame { pose, meowing }`), e nenhuma chamada a `Instant::now()` fica dentro do módulo.
O sorteio é o trait `Dice` (`pick(min, max)`). O de produção (`SystemDice`) é um xorshift64* com a
semente tirada do `RandomState` da std. **Nenhuma dependência nova:** o projeto não usa crate de
aleatoriedade, e a gata não precisa de qualidade criptográfica. Os testes usam um `Dice`
roteirizado, que devolve uma sequência fixa e grava os limites pedidos. O `tick` aplica **no máximo
uma transição por evento** e conta o próximo prazo a partir do `now`. Um `tick` muito atrasado (o
laço ficou parado, ex: o `bin/nick-init` com a TUI suspensa) não recupera eventos perdidos: no
máximo uma piscada e um miado começam.

**O estado fica no `App`** (`mascot`, `truecolor`), e a assinatura de `App::new` não mudou (ele cria
a gata de produção e começa sem truecolor). O `tick_mascot` só mexe na gata: não mexe em foco,
seleção, diálogo, texto digitado, barra de status nem agentes (teste em `app.rs`). Em `ui.rs`, a
geometria tem fonte única nas funções puras `mascot_area`, `content_area` e `bubble_area`, e o
`left_column` desenha o bloco da coluna, a gata e devolve o retângulo do conteúdo. O `draw_tree` e
o `draw_settings` desenham o `List`/`Paragraph` sem bloco nesse retângulo. **A gata não entra em
`Areas`/`layout`:** ela não muda o tamanho de nenhum PTY, o cabeçalho nem o mouse (teste com
`layout`/`agent_area` em todas as poses, com e sem balão).

**O laço acorda pelo relógio da gata** (`main.rs`). No topo de cada volta,
`dirty |= app.tick_mascot(Instant::now())`. No ramo ocioso, no lugar do `rx.recv()` de antes, o
laço espera até o `next_wake` (`terminal::idle_wait`, uma função pura testada: `None` bloqueia sem
prazo, um instante passado dá zero), e um `Timeout` só volta ao topo. A gata **não** chama o
`after_change`: ela não muda PTY, modo do terminal de fora nem o texto guardado do agente
escondido. Ela só marca `dirty`, e o desenho sai pelo `draw` de sempre, com o limite de um quadro
a cada `FRAME`. **Custo ocioso aceito no ticket:** antes, zero redesenhos. Agora, cerca de 2 a
cada 6–20 s (começo e fim da piscada), mais 2 a cada 3–10 min (o balão).

**Cores: truecolor ou a paleta de 256.** Só o `main.rs` lê o ambiente (regra do NS-31). Com
`COLORTERM` igual a `truecolor` ou `24bit`, sem diferença de maiúscula (`terminal::is_truecolor`),
a gata sai com o `Color::Rgb` exato do `.py`. Com qualquer outro valor, ou sem a variável, ela sai
com `Color::Indexed(nearest_256(...))`: distância euclidiana em RGB sobre o cubo 6×6×6 (16–231,
níveis `0, 95, 135, 175, 215, 255`) e a rampa de cinzas (232–255, `8 + 10·i`). As 16 primeiras
cores ficam de fora, porque o tema do terminal as redefine. No empate, vale o menor índice.

| Letra | RGB | Índice 256 |
|---|---|---|
| D marrom escuro | (52, 36, 30) | 235 |
| B marrom médio | (110, 78, 62) | 240 |
| C creme | (196, 170, 140) | 180 |
| W branco | (242, 240, 234) | 255 |
| E azul dos olhos | (110, 170, 225) | 74 |
| K pupila | (20, 20, 24) | 233 |
| P nariz | (232, 160, 168) | 181 |

**Ressalva:** os dois marrons viram cinza na paleta de 256, porque o cubo não tem marrom escuro. É a
aproximação que o ticket aceita. Se ficar ruim, a troca é uma tabela fixa por letra no lugar do
`nearest_256`.

**O gancho só de teste `NICK_TEST_MASCOT`**, lido no `main.rs` ao lado do `NICK_TEST_PANIC`
(`mascot_hook`):
- `still`: a gata parada, que nunca pisca nem mia (`next_wake` = `None`, e o laço ocioso volta a
  bloquear sem prazo, como antes do NS-51). É o padrão do `Nick::spawn` dos testes de PTY
  (`tests/common/mod.rs`; um `NICK_TEST_MASCOT` do caso substitui). Assim um "Miau!" sorteado não
  aparece no meio de uma leitura de tela;
- `fast`: intervalos curtos fixos (pisca a cada 40 ms por 20 ms, mia a cada 120 ms por 60 ms), sem
  sorteio, para os testes de PTY provarem que a animação não atrapalha a digitação nem o terminal
  do agente;
- qualquer outro valor, ou vazio: a gata de produção. **O gancho não esconde a gata**, porque o
  ticket proíbe uma opção de desligar.

**Helpers da integração** (`nick/tests/common/mod.rs`): o `tree_rows` e o `flat_screen` tiram os
`▀`/`▄` (`MASCOT_BLOCKS`). Assim as linhas da gata viram linhas vazias em `tree_rows`, e o `select`
("o fim da árvore é a linha antes de uma vazia") continua certo. **A adaptação rodou verde sobre o
código sem a gata antes da mudança** (o `make test` inteiro; log em
`.nick/docs/NS-51/baseline_helpers_sem_nick.log`). Novos: `Nick::cat_rows` (as linhas com meio-bloco
nas colunas da gata), `Nick::keep_when` (espera uma condição e guarda **a mesma** tela em que ela
valeu, porque a piscada dura 20 ms no modo `fast`) e, além do texto em `NICK_TEST_SCREENS`, a tela
com as cores (ANSI, para dar `cat` num terminal) em `<rótulo>.ansi`, no diretório de
`NICK_TEST_SCREENS_ANSI`, quando definido.

**Testes adaptados:** nenhum. Os testes de `ui.rs`, `app.rs` e do PTY que já existiam passaram
sem edição com a gata no lugar. O rodapé, o `HELP_STATES`, o tamanho de cada PTY e as bordas de
foco (NS-37) não mudaram. Também não entrou dependência nova nem migração nova, e o README não
mudou (regra "README genérico").

**Testes novos:** em `mascot` (as cores da tabela, as pontas `(0,0,0)→16`, `(255,255,255)→231` e
`(128,128,128)→244`, a arte fixada, a forma, os 4 casos de pixel→célula, a piscada só nas células 3
e 4, a máquina de estados com o `Dice` roteirizado, inclusive o `tick` atrasado, o `SystemDice`
dentro dos limites, o `still` e o `fast`); em `app` (a gata nova e o `tick_mascot` sem mexer no
resto, com agente vivo e com um campo de texto aberto); em `terminal` (`idle_wait` e
`is_truecolor`); em `ui` (a geometria com interior de 14, 15 e 16 linhas e de 16 e 17 colunas, o
desenho pixel a pixel, a gata que some com 14 linhas e numa janela estreita, o balão byte a byte e
sem espaço, e o layout que não muda); no PTY, em `tests/tui.rs` (a gata na árvore e nas
Configurações com `Idx(74)` no olho, `Rgb(110,170,225)` com `COLORTERM=truecolor`/`24bit`, o resize
para 19 linhas que a esconde e a volta, o `fast` piscando e miando com o balão que some, e a
digitação num campo de texto) e em `tests/agent.rs` (com o `fast` e o `claude` falso: o texto
digitado chega inteiro, o foco fica no terminal, e nenhum SIGWINCH em 3 s de animação, com
controle positivo do resize). As telas da validação (parada, piscando, miando, sumida, com e sem
truecolor) ficam em `.nick/docs/NS-51/prints/`, em texto, ANSI e PNG.

**Ajustes do review do NS-51** (`.nick/docs/NS-51/review.md`):
- **a paleta é resolvida uma vez** (problema 1): sem truecolor, o `color_of` roda o `nearest_256`
  sobre 240 candidatos, e o desenho fazia isso para o pixel de cima e o de baixo de cada célula, a
  cada quadro (até um a cada 16 ms enquanto um agente escreve), para chegar sempre às mesmas 7
  cores. Agora `mascot::Palette::new(truecolor)` resolve as 7 cores, o `App` a guarda e só a
  recalcula no `set_truecolor`, e o `draw_mascot` só procura a letra nela. O `nearest_256` continua
  sendo a fonte da regra (`color_of` e `Palette` usam a mesma função privada `resolve`). Testes:
  `the_palette_resolved_once_has_the_colors_of_color_of` (`mascot`) e a paleta do `App` antes e
  depois do `set_truecolor`;
- **a geometria dos helpers vem da lib** (problema 2): `tests/common/mod.rs` usa
  `nick::mascot::MASCOT_COLS`, e o `wait_for_cat` de `tests/tui.rs` usa `MASCOT_ROWS`, no lugar de
  uma cópia da constante e do `7` literal. A cópia independente vale para a arte e a paleta (a
  referência aprovada), não para a geometria que o helper usa para achar a gata;
- **`BUBBLE_ROWS`** (nit): as 3 linhas do balão viraram constante em `mascot.rs`, ao lado do
  `BUBBLE_COLS`;
- o nit do `main.rs` (com `terminal::panicking()`, o ramo ocioso acorda no `next_wake` e volta ao
  topo, em vez de ficar no `recv()`) ficou como está, como o review propôs: o laço não desenha nada
  e o hook do panic encerra o processo.

## Rust no projeto: etapa Aberto, cores e avanço ao abrir agente (NS-45)

> **Mudou no NS-61** (seção "Rust no projeto: etapa Rejeitado (NS-61)"): entrou a
> sétima etapa, **Rejeitado**, em cinza (`ui::STAGE_GRAY`, `Color::DarkGray`), com o rótulo inteiro
> do ticket cinza na árvore e a marca `[✗ Rejeitado]`. O avanço na abertura usa
> `current.is_closed()` (Concluído ou Rejeitado) em vez de `current == Done`, então nunca tira um
> ticket do Rejeitado, e o `g` num ticket rejeitado é recusado. A migração 11 repete as três
> armadilhas da 9.

História filha do épico NS-28. Três ajustes nas etapas do ticket do NS-36: a etapa **Aberto**
antes de PO, as **cores** das etapas e do épico, e o **avanço da etapa** quando se abre um agente
novo de uma etapa posterior. Fora do escopo: cores configuráveis, etapa nos épicos e cor da etapa
nas linhas de agente, no diálogo "Mudar etapa" e nas linhas do histórico.

**As seis etapas** (`tickets::TicketStage`): Aberto (`open`) → PO → Tech Leader → Dev → Review →
Concluído. `TicketStage` ganhou a variante `Open` e `derive(PartialOrd, Ord)`: a ordem de
declaração é a do fluxo, e um teste cobra que `ALL` já sai ordenado. `key()` e `label()` viraram
um `match` com as seis variantes (as quatro do meio pegam o texto de `agents::Stage`, sem repetir),
e `agent_stage()` devolve `None` para `Open` e `Done`, as etapas só do ticket. `agents::Stage`
**não** ganhou `Open`: não existe agente de Aberto, e o `CHECK` de `agents.stage` não mudou. Os
tickets novos nascem em Aberto pelo `DEFAULT 'open'` da migração 9 (o `insert_ticket` não mudou),
e os que já existiam ficam na etapa que tinham. O diálogo "Mudar etapa" percorre `ALL`, então o
Aberto aparece em primeiro e o operador pode voltar o ticket para ele de qualquer etapa.

**A origem da mudança** (`tickets::StageSource`, no molde de `TicketStage`): `operator`
(`operador`) ou `agent_opening` (`abertura de agente`), gravada em `ticket_stage_changes.source`.
`source_label` devolve a chave crua quando ela é desconhecida, como o `stage_label`.
`Db::change_ticket_stage(id, from, to, source)` continua sendo o **único escritor** da etapa no
banco, para os dois casos; `tickets::change_stage` (o operador) passa `operator`, com as mesmas
mensagens e recusas de antes.

**A regra do avanço** (`tickets::advance_on_agent_opening(db, ticket_id, stage)`): relê o ticket
do banco (a etapa pode ter mudado em outro nick) e, só quando a etapa do agente é **posterior** à
do ticket (pelo `Ord`), grava a mudança com `agent_opening`, mesmo pulando etapas (Aberto →
Review). Etapa igual ou anterior, Concluído, chave desconhecida e ticket que sumiu não mudam nada
(`Ok(None)`). Se outro nick mudou a etapa entre a leitura e a escrita (`Moved`), a barra diz `A
etapa do ticket <ID> mudou em outro nick. A etapa não avançou.`; erro do banco vira a mensagem do
banco. No sucesso, a barra diz `Etapa do ticket <ID>: <de> → <para> (abertura de agente).`. A
leitura do ticket vem por parâmetro num `advance_on_agent_opening_with` privado (o molde do
`create_with`/`edit_id_with`), para o teste simular o outro nick. **Quem chama:** só o
`App::open_agent`, depois do `launcher.spawn` com sucesso e só para um agente de ticket, **antes**
do `start_running` (que relê a árvore, já com a etapa nova, e limpa a barra; a mensagem do avanço
é posta depois dele). Uma falha no avanço não encerra o agente, que já está rodando: só a barra
avisa. Cada tipo de etapa (PO discussão, PO Jira…) avança pelo `kind.stage()`. **Não avançam:**
`agents::open` ou `spawn` que falha, a retomada (`resume_agent`, NS-39), a troca de foco, o
`Enter` num agente vivo, o agente de épico, o fim do agente, o encerramento pelo operador e a
saída. A etapa continua sem fechar nem esconder agente (mudar para Concluído deixa o agente vivo
como estava).

**Estrutural** (`the_stage_is_only_written_by_the_operator_or_by_opening_a_new_agent`, o antigo
`the_stage_is_only_written_by_the_confirmation_of_the_operator`): `change_ticket_stage` só no
`db/mod.rs` e no `tickets.rs`; `tickets::change_stage(` com uma chamada no `app.rs`, dentro da
confirmação do diálogo; `advance_on_agent_opening(` só no `tickets.rs` e, no `app.rs`, com **uma**
chamada, dentro de `fn open_agent` e depois de `launcher.spawn(`; nenhum `UPDATE tickets SET
stage` nem `ticket_stage_changes` fora de `src/db/`. O controle positivo pega a chamada no
`resume_agent`, antes do `spawn`, no `agent_ended`, duas chamadas e a chamada em outro fonte. Como
o `app.rs` precisa ter o avanço, cada caso ruim antigo do controle ganhou a abertura boa, para
quebrar só a própria regra (a confirmação sozinha agora dá `0 calls of advance_on_agent_opening`).

**Banco: migração 9.** No plano ela era a 8, mas o NS-50 publicou a `MIGRATION_8`
(`resume_session_id`) antes; por decisão do usuário, o texto do plano virou a `MIGRATION_9` sem
outra mudança, e os testes passaram a ser v8 → v9. O SQLite não altera um `CHECK`, então
`tickets` e `ticket_stage_changes` são **reconstruídas** (`CREATE TABLE x_new`, `INSERT ...
SELECT` com o `id`, `DROP`, `RENAME`, o molde do `agents` da migração 7), com as colunas na mesma
ordem (`TICKET_COLUMNS` não mudou). A etapa ganhou `open` no `CHECK` das duas tabelas e o `DEFAULT
'open'`; o histórico ganhou `source TEXT NOT NULL CHECK (source IN ('operator',
'agent_opening'))`, **sem `DEFAULT`** (todo `INSERT` diz a origem). As linhas antigas viram
`operator`, porque até o NS-45 só o operador mudava a etapa. A migração 5 (com o "the nick never
does it alone") não foi editada: migração publicada é fixada por teste, e a regra nova fica no
comentário da 9. **As três armadilhas, que a migração 7 não teve porque nada apontava para
`agents`:**
1. **`DROP TABLE tickets` com a FK ligada apaga os filhos**: o `DROP` faz um `DELETE` implícito, e
   o `ON DELETE CASCADE` de `agents.ticket_id` e do histórico levaria tudo. O `PRAGMA
   foreign_keys` é ignorado dentro de uma transação, então o laço do `Db::open_with` foi adaptado
   (o procedimento de 12 passos do SQLite): **só quando há migração pendente**, a FK é desligada
   antes do laço; cada migração roda na sua transação, e o `PRAGMA foreign_key_check` roda antes
   do `user_version` e do commit (qualquer linha recusa a migração com `referência quebrada na
   tabela <tabela> (para <pai>)`, e a transação desfaz); depois do laço a FK volta ligada,
   **também quando uma migração falha** (o erro da migração tem prioridade sobre um erro ao
   religar). Vale para **todas** as migrações: para as antigas o resultado é o mesmo, porque elas
   não criam referência quebrada, e se criassem o `foreign_key_check` recusaria como a FK ligada
   recusava. Os testes de migração v1 → v8 passaram sem mudança;
2. **os seis gatilhos da migração 7 caem com a tabela** (quatro em `tickets` e os dois de `epics`
   que leem `tickets`), e o `RENAME` confere o esquema inteiro: a migração 9 faz o `DROP TRIGGER`
   dos seis antes e os recria depois do `RENAME`, **com o texto idêntico** ao da migração 7. O
   `sqlite_master` de um banco na versão 8 foi conferido antes: além deles, só o autoindex do
   `UNIQUE (project_id, key)`, que a tabela nova recria;
3. **os `id`s são preservados**, então `agents.ticket_id`, o histórico e `tickets.epic_id`
   continuam válidos, e o `foreign_key_check` prova isso.

**Histórico no painel:** `  <data>  <de> → <para>  (<origem>)`, com a origem no fim de propósito:
com agente vivo a coluna tem 40 colunas, e o corte leva a origem, não a data nem as etapas. O
histórico vazio diz a etapa atual, que é a etapa em que o ticket começou (`Nenhuma mudança: o
ticket começou em <etapa>.`): um ticket antigo diz PO, e um novo diz Aberto.

**Cores** (`ui.rs`, sem módulo de tema nem enum novo): `stage_color(TicketStage)` dá Aberto em
`Color::White`, PO e Tech Leader em `STAGE_BLUE` (`Color::LightBlue`, o índice 12 das 16 cores,
diferente do `Color::Cyan` do `FOCUS_COLOR` do NS-37), Dev e Review em `STAGE_ORANGE`
(`Color::Indexed(208)`) e Concluído em `Color::Green`. `EPIC_COLOR` (`Color::Magenta`, o índice 5)
pinta o ID e o título do épico; o `[épico]` continua só em negrito, e o `[provisório]` e o
`[oculto]` ficam sem cor. A marca da etapa na árvore (`stage_mark`) e o `Etapa atual:` do painel
saem de uma função só (`stage_span`, com ou sem colchetes; o Concluído é sempre `[✓ Concluído]`),
e a chave desconhecida sai crua e sem cor, como antes: `[archived]` na árvore e `archived` no
painel, sem colchetes (corrigido no review). O DIM do ticket ou do épico oculto convive com o `fg`
do span (conferido no teste). No PTY, o `vt100` lê as cores do crossterm como `Idx(15)` (branco),
`Idx(12)` (azul), `Idx(208)` (laranja), `Idx(2)` (verde) e `Idx(5)` (roxo); o harness ganhou o
`Nick::color_at(texto, deslocamento)`, sobre o `cell` que já existia.

**Impactos conhecidos** (aceitos no ticket ou no plano):
- num terminal de 16 cores, o laranja 208 sai aproximado; o azul e o roxo são da paleta de 16;
- num tema claro, o branco do Aberto pode sumir (o ticket fixou o branco);
- um avanço por engano (abrir um agente de etapa posterior sem querer) é corrigido à mão pelo
  operador (`m`), e o histórico guarda as duas mudanças com a origem de cada uma;
- `[Aberto]` é mais longo que `[PO]`: com 40 colunas, o fim da linha de um ticket novo é cortado
  mais cedo (o corte é o comportamento aceito desde o NS-34). O ID e a etapa continuam inteiros.

**Testes adaptados, com o motivo:**
- `nick/src/db/mod.rs`: `a_new_database_gets_version_8_…` → `a_new_database_gets_version_9_…`
  (renomeado), `opening_twice_duplicates_nothing` (9), `a_newer_database_is_refused_without_being_written`
  e `a_database_one_version_newer_is_refused_too` (citam a `(9)`, e o "um a mais" é a 10) e
  `migration_1_is_pinned` (nove migrações e o texto da 9); `a_version_7_database_with_agents_migrates_to_8_with_no_session_of_resume`
  passou a abrir só até a migração 8 (a 9 tem o teste dela); o helper `raw_change` grava a origem
  `operator` (a coluna é `NOT NULL` sem `DEFAULT`), com o irmão `raw_sourced_change`;
  `change_ticket_stage_stores_the_stage_and_one_history_row`, `change_ticket_stage_gone_and_moved_store_nothing`
  e `change_ticket_stage_is_all_or_nothing` passam a origem, partem de `open` (o ticket nasce em
  Aberto) e cobram a origem gravada (o último também cobra que uma origem recusada pelo `CHECK`
  não grava nada). Nenhum teste de migração antigo lia o histórico pelo `list_stage_changes` num
  banco anterior à 9, então nenhum precisou passar para SQL cru;
- `nick/src/tickets.rs`: `ticket_stage_has_the_five_stages_…` → `…_six_stages_…` (Aberto, `Ord`);
  `change_stage_goes_forwards_backwards_and_to_and_from_done` parte de Aberto, vai **para** e
  **de** Aberto e cobra a origem `operator`; `the_same_stage_is_refused_and_nothing_is_stored` e
  `a_ticket_gone_or_moved_by_another_nick_stores_nothing` partem de Aberto;
- `nick/src/app.rs`: `the_stage_never_changes_alone_and_done_closes_no_agent` foi **substituído**
  por `opening_a_new_agent_of_a_later_stage_advances_the_ticket` (com a parte "Concluído não fecha
  nem esconde agente") e `the_stage_does_not_change_in_the_other_cases`;
  `m_lists_the_five_stages_…` → `m_lists_the_six_stages_…` (os índices andam um, e a etapa atual
  de um ticket novo é Aberto); `the_ticket_screen_shows_the_history_the_agents_and_the_artifacts_read_now`
  (as aberturas do PO e do Dev avançam a etapa: o histórico tem as duas, e a mudança do operador
  vira a terceira);
- `nick/src/ui.rs`: o helper `change` ganhou a origem (só sintaxe, com o irmão `sourced`);
  `the_dialogs_of_the_stage_and_of_the_id` (o índice do Review é 4, e o Aberto vem antes do PO);
  `the_label_of_an_epic_has_the_mark_bold_and_dims_when_hidden` (o ID e o título em roxo; as
  marcas sem cor); `the_rows_are_indented_with_the_arrows_and_the_label_of_each_node`,
  `the_styles_of_the_nodes` e `the_tree_shows_the_epic_before_the_tickets_and_a_long_id_fits_with_its_mark`
  (o ticket novo mostra `[Aberto]`); `the_panel_of_a_ticket_shows_the_stage_the_artifacts_and_the_history`
  (o histórico vazio diz a etapa atual, Tech Leader na fixture, e as linhas ganharam `(operador)`);
- `nick/tests/agent.rs`: o estrutural (acima); `a_real_version_6_database_opens_migrated_with_the_tree_of_before`
  (versão 9); `the_dev_form_without_a_plan_warns_and_opens_anyway` (abrir o Dev agora leva o
  ticket a `development`); `a_hidden_agent_whose_text_changed_is_marked_until_it_is_shown` (o
  `select("Tech Leader")` parava na linha do ticket, que agora mostra `[Tech Leader]`: passou a
  `select("Tech Leader  ●")`, a linha do agente com ou sem a marca); `each_kind_of_node_has_its_panel_and_its_help`
  (`NS-2  [Aberto]  Segundo  [ocul`, cortado a 40 colunas). O `NS-1  [PO]` de
  `hiding_an_epic_is_refused_with_a_live_agent_…` não mudou: o agente PO aberto ali leva o ticket a
  PO;
- `nick/tests/tui.rs`: `the_ticket_flow_works_and_touches_no_file_of_the_project` e
  `the_tickets_of_a_project_do_not_show_in_another` (`[Aberto]`; com o corte, o título esperado é o
  pedaço que cabe, `Corrigir o log` × `Corrigir o sen`, para continuar provando o título novo);
  `the_operator_changes_the_stage_and_done_stays_on_the_list` (Aberto, `começou em Aberto`,
  `Aberto → Review  (operador)`, e as cores do Aberto, do Review e do Concluído).

Os testes do NS-40 (gatilhos e épicos), da troca manual e do "Editar ID" do NS-36, do foco do
NS-37, da retomada do NS-39, do Ctrl+Z do NS-44 e da sessão de retomada do NS-50 passaram sem
mudança. Nenhum teste foi apagado sem substituto.

**Testes novos:** `tickets` (as seis etapas e o `Ord`, o `StageSource`, o avanço de Aberto para
cada etapa, pulando etapas, a mesma etapa, a anterior e o Concluído, o ticket sumido e a chave
desconhecida, o `Moved` pelo outro nick, o erro do banco); `db` (a reconstrução de uma tabela-pai
que mantém os filhos, a migração com referência quebrada recusada e desfeita, a FK ligada depois
de migrar e depois de uma migração que falha, o banco v8 populado migrando para a 9 com todas as
linhas campo a campo, os `id`s, a origem `operator`, o `foreign_key_check` vazio, os seis gatilhos
iguais e o cascade, os gatilhos da migração 7 recusando depois da 9, o ticket novo em `open` e os
`CHECK`s novos); `app` (o avanço, os casos sem mudança, a retomada, o agente de épico e o operador
voltando a Aberto de qualquer etapa); `ui` (as cores das etapas e do épico, cada etapa na árvore e
no painel, o ticket oculto com cor e DIM, o épico roxo e oculto com DIM, o histórico com a origem e
o vazio com a etapa atual, o diálogo com `> Aberto (atual)`); o estrutural; e, no PTY,
`opening_new_agents_advances_the_stage_and_each_stage_has_its_color` (`tests/agent.rs`: o épico
roxo, o Aberto branco, o avanço para PO azul e para Dev laranja com a mensagem e o histórico, e o
PO de novo sem mudança). As sabotagens e as telas ficam em `.nick/docs/NS-45/`.

## Rust no projeto: borda de foco nos diálogos (NS-46)

> **Mudou no NS-57** (seção "Rust no projeto: liberar e prender a jail de um agente (NS-57)"):
> **agora são duas regras que dão a borda vermelha.** Além do `Dialog::irreversible` (só o
> `ConfirmRemove`, sem mudança), o `Dialog::touches_jail` (`match` exaustivo, sem `_`, `true` só no
> `ConfirmJailToggle`) também pinta o diálogo com o `IRREVERSIBLE_COLOR`, embora trocar a jail
> tenha volta. O nome da constante ficou o mesmo. São 26 variantes de `Dialog` desde então.

Bug filho do épico NS-28. O NS-37 deixou os diálogos fora da borda de foco ("diálogo não é painel
de foco"). Com um diálogo aberto, as teclas vão para ele, mas a lista continuava desenhada com a
borda grossa ciano, e o diálogo ficava com a borda simples.

**A regra: o foco é de quem recebe as teclas, incluindo os diálogos.** Ela fica num ponto só do
desenho, `ui::focused(app) -> Focused { Dialog, Terminal, List }`, nesta ordem: o diálogo aberto,
senão o terminal do agente exibido (`App::on_terminal`), senão a lista. O `list_block` desenha a
borda de foco só com `Focused::List`, e o `draw_agent` só com `Focused::Terminal`. O painel do nó
continua com `pane_block(false)`. **O `App::on_terminal`, o `handle_key` e o `toggle_focus` não
mudaram:** o roteamento das teclas, o paste e o rodapé são os mesmos. Hoje os diálogos só abrem
com o foco na lista, e o campo `focus` não muda ao abrir nem ao fechar um diálogo. Por isso
"voltar o foco para onde estava" não precisa de estado novo no `App`: fechado o diálogo, a regra
de desenho devolve a borda a quem a tinha. Pôr o diálogo antes do terminal é defensivo: se um dia
um diálogo abrir com o foco no terminal, mesmo assim só a borda dele fica destacada.

**A borda do diálogo vem de `pane_block(true)`**, via `ui::dialog_block(dialog)`, sem estilo
copiado. O `draw_dialog` e o `draw_prompt` (o "Prompt final") usam essa função. A espessura e o
tamanho são os mesmos de qualquer borda (`Thick` e `Plain` ocupam uma célula), então o
`dialog_area`, o `prompt_layout` e o cursor dos campos de texto não mudaram.

**Borda vermelha só para ação irreversível.** Cada diálogo declara se é irreversível num ponto
só, `Dialog::irreversible` (`app.rs`, ao lado do `takes_text`), com `match` exaustivo e sem `_`:
um diálogo novo não compila sem decidir. Hoje só o `ConfirmRemove` é `true`, porque apaga do banco
o projeto com tickets, épicos, agentes e histórico. Nesse caso, o `dialog_block` troca só a cor
por `ui::IRREVERSIBLE_COLOR` (`Color::Red`). O `border_style` do ratatui 0.30 substitui o estilo,
então não sobra ciano por baixo, e o teste cobra isso. **Ficam ciano, por terem volta:** ocultar
(`ConfirmHide`, `ConfirmHideEpic`), arquivar (`ConfirmArchive`), editar o ID (`ConfirmEditId`,
`ConfirmEditEpicId`), encerrar um agente (`ConfirmEndAgent`, porque o agente encerrado pode ser
retomado, NS-39) e sair (`ConfirmQuit`). Também ficam ciano a troca de etapa, a conversa perdida,
o "Prompt final", os formulários, as escolhas e os alertas. O vermelho do `[FORA DA JAIL]`
(`header_lines`, `agent_spans`, `prompt_top`) continua com o `Color::Red` literal, e não com o
`IRREVERSIBLE_COLOR`: são regras diferentes que hoje coincidem na cor.

**Ajuste em relação ao ticket: o "histórico" não é diálogo.** O ticket citava "escolha de etapa,
histórico e Prompt final". O histórico de etapas é desenhado no painel do nó, que nunca tem o
foco, e não num diálogo. A escolha de etapa é o `ChooseTicketStage`, e a confirmação, o
`ConfirmStageChange`. O critério virou "todo diálogo (as 24 variantes de `Dialog`)".

**O catálogo dos diálogos é compartilhado com os testes do `ui`.** No `app.rs`, o
`one_of_each_dialog()` e o `variant()` passaram a `pub(crate)`, dentro de um
`#[cfg(test)] pub(crate) mod tests`, e o `ui::tests` os importa de lá. **O detector de escrita em
projeto foi ajustado junto** (decisão do usuário na implementação, registrada no plano): o
`production()` de `tests/agent.rs` cortava o código de produção só no texto exato
`"#[cfg(test)]\nmod tests"`. Com o `pub(crate)`, os `fs::write`/`remove_file` dos testes do
`app.rs` passaram a contar como produção. Agora ele corta no primeiro de
`"#[cfg(test)]\nmod tests"` e `"#[cfg(test)]\npub(crate) mod tests"`, só nesses textos exatos.
Cortar no primeiro `#[cfg(test)]` qualquer não serve: o `#[cfg(test)] #[path = ...]` do topo do
`projects.rs` deixaria quase todo esse arquivo fora da checagem, sem nenhum teste falhar. O
`production_sources()` já corta no primeiro `#[cfg(test)]` e não precisou de ajuste. Também
avaliamos o re-export (`#[cfg(test)] pub(crate) use tests::{...}`, com o `mod tests` privado) e o
`test_support`, que arrastaria os fixtures de que o catálogo depende.

**Testes novos:**
- `app`: `only_the_removal_of_a_project_is_irreversible`.
- `ui`: `the_dialog_block_is_the_focused_pane_block_cyan_or_red` (`dialog_block == pane_block(true)`
  para um diálogo comum; símbolos grossos em ciano ou vermelho, sem DIM; nenhuma célula ciano no
  vermelho; `inner` igual); `every_dialog_has_the_focus_border_and_only_it` (as 24 variantes, com
  e sem agente: um único `┏` na tela, na cor certa, e a lista e o painel `┌` sem foco; no "Prompt
  final", nada grosso fora da caixa); `the_focus_border_goes_back_when_the_dialog_closes` (com o
  foco na lista e, forçado, no terminal); `the_removal_dialog_is_red_and_the_others_cyan`.
- PTY: `a_dialog_takes_the_focus_border_and_gives_it_back` (`tests/agent.rs`, o `q` com um agente
  vivo: o `┏` ciano no topo da caixa, a lista e o painel `┌`, e a borda volta à lista ao cancelar)
  e `the_removal_dialog_has_the_red_focus_border` (`tests/tui.rs`: o `┏` e o `┃` em `Idx(1)`, que
  é o `Color::Red` do crossterm, e o projeto continua lá depois de cancelar).
- O autoteste do detector (`the_stage_is_only_written_by_the_confirmation_of_the_operator`)
  ganhou dois casos: o `pub(crate) mod tests` some, e um `#[cfg(test)]` de `#[path]` não corta
  nada.
- Helper novo em `tests/common/mod.rs`: `Nick::cells_of(symbol)`, as células com o símbolo e a cor,
  num parse só.

Os testes PTY e os de desenho morderam numa sabotagem feita numa cópia fora do repositório: o
desenho voltou ao de antes e, em outra rodada, o vermelho foi desligado. O log está em
`.nick/docs/NS-46/review_sabotagens.txt`.

**Testes adaptados (só o caractere da borda do diálogo, `│ └` → `┃ ┗`):**
- `ui::a_long_path_in_the_text_field_keeps_its_end_and_the_cursor_visible`: o lado da caixa lido
  é o `┃`, e o fundo, o `┗`;
- `tests/agent.rs`, `menu_labels` (usado por `the_menu_follows_jira_and_sentry_of_the_project`):
  o rótulo sozinho na linha aceita o `┃` depois dele;
- `ui::the_removal_confirmation_names_the_project_and_s_n` e
  `tests/tui.rs::a_missing_init_shows_the_path_and_the_tui_goes_on` (review do NS-46, O1). Eles
  passavam sem edição porque o texto cobrado cabe numa linha da caixa. Mas o `replace('│', "")`
  deles não tirava mais a borda do diálogo, e um texto quebrado em duas linhas falharia por um
  motivo enganoso. Agora usam os helpers que já tiram `│` e `┃`: o `flat(&buffer)` do `ui` (o
  trecho era uma cópia dele) e o `Nick::flat_screen()`, que também junta as linhas com espaço (o
  `collect()` antigo juntava sem separador).

Passaram sem edição os testes do NS-37 sem diálogo (`the_focused_panel_has_the_thick_cyan_border`,
`every_list_screen_shows_where_the_focus_is`,
`without_an_agent_the_tree_is_on_the_left_focused_and_the_panel_on_the_right` e, no PTY,
`toggling_the_focus_moves_the_thick_border_at_once`), os de tamanho e cursor, o
`the_preview_covers_the_screen_above_the_help_and_the_keys_show_once` e todos os do `app.rs`. O
`assert_border` dos testes do `ui` passou a delegar ao `assert_border_color` novo (`Some(cor)`:
grossa nessa cor; `None`: simples e DIM), sem mudar as chamadas.

Prints da conferência em `.nick/docs/NS-46/prints/` (texto, ANSI e PNG): um diálogo comum ciano
("Sair do nick", com agente vivo) e o "Remover projeto" vermelho. Nenhuma dependência nova, e o
README não mudou (regra "README genérico").

**Fora de escopo:** escurecer o fundo atrás do diálogo e mudar o texto ou o tamanho dos diálogos.

## Rust no projeto: instalador local (NS-52)

A fatia F6a do épico NS-28: `make install`/`make uninstall` instalam o nick num prefixo (padrão
`~/.local`, sem `sudo`), e o nick instalado roda sem o checkout.

**A árvore instalada e a regra de seleção.** A lógica fica em `install/install.sh` (diretório novo:
`lib/` é só para bibliotecas carregadas com `source`, e `tests/` da raiz é dos testes), e o
`Makefile` só o chama (`PREFIX ?= $(HOME)/.local`, uma linha de receita por alvo). A instalação
replica a estrutura relativa do checkout, então nenhum script bash mudou o jeito de achar `lib/`
(`SCRIPT_DIR/../lib`) nem os templates (ao lado do `prompt.sh`):

```
$PREFIX/bin/nick                              (0755, gravado por último)
$PREFIX/share/nick/.nick-install              (o manifesto)
$PREFIX/share/nick/bin/nick-agent, nick-init  (0755)
$PREFIX/share/nick/lib/<dir>/*                (0644)
$PREFIX/share/nick/auto_scrum/prompt.sh       (0644)
$PREFIX/share/nick/auto_scrum/templates/*.md  (0644)
```

A seleção é uma regra, não uma lista (um template ou uma lib nova entra sozinha): todo arquivo
regular **direto** em `bin/` (não `bin/tests/`), em cada `lib/<dir>/` menos `lib/tests/`, o
`auto_scrum/prompt.sh` e os `auto_scrum/templates/*.md`. **Não** vão: o `auto_scrum.sh` (o legado
roda do checkout e continua gravando em `auto_scrum/logs/`), logs, testes, fixtures, `prototype/` e
`nick/`. Sem `--binary`, o script roda `cargo build --release --locked --manifest-path
<raiz>/nick/Cargo.toml` e instala `${CARGO_TARGET_DIR:-<raiz>/nick/target}/release/nick`. Um
`CARGO_TARGET_DIR` relativo é recusado (o cargo o leria contra outro cwd), e o `--binary` existe
para a suíte rodar sem compilar. Um prefixo relativo é recusado, para nunca gravar num caminho
relativo ao cwd. No fim, se `$PREFIX/bin` não está no `PATH`, ele mostra a linha do bash/zsh e a do
fish, com o prefixo real. Limite conhecido: o `PREFIX ?= $(HOME)/.local` do `make` vira `/.local`
quando o `HOME` está vazio. O script não tem como distinguir esse caso de um `--prefix /.local`, e
a gravação falha por permissão. O `install.sh` chamado direto, sem `--prefix` e sem `HOME`,
recusa com mensagem.

**Como o nick acha os arquivos: pela localização do binário** (`nick/src/resources.rs`). O
`main.rs` passa o `std::env::current_exe()` (que no Linux resolve symlinks) para a função pura
`resources::locate`. Se `<dir do binário>/../share/nick/.nick-install` existe, é o **modo
instalado** (a raiz é aquele `share/nick/`). Senão, é o **modo checkout** (`CARGO_MANIFEST_DIR/..`,
normalizado), que vale para `cargo run`, `cargo test` e o `cargo install --path nick` antigo
(`~/.cargo/bin/nick` não tem `~/.cargo/share/nick/.nick-install`). O `init::init_path` e o
`launch::launcher_path` recebem a raiz. Ficou um módulo próprio, e não funções em `paths.rs`,
porque o modo e os dois caminhos andam juntos (`Resources::{root, init, launcher}`), e o
`paths.rs` é de caminhos do estado e de texto. Decisões:
- **sem prefixo fixado na compilação** (`option_env!`): o teste de instalação e o futuro pacote do
  AUR (F6b, com `DESTDIR`) precisariam recompilar o binário para cada prefixo. Pela localização,
  a instalação é relocável e o binário é o mesmo;
- **sem fallback do modo instalado para o checkout:** o binário e um checkout podem ser de versões
  diferentes, e o Rust de uma com o bash de outra quebraria o protocolo do `--check`/`--render`
  sem aviso. Um arquivo faltando no prefixo dá a mensagem de arquivo ausente que já existia, com o
  caminho do prefixo (controle positivo em `nick/tests/install.rs`);
- **segurança (regra do NS-33):** nenhuma variável de ambiente troca o `bin/nick-agent`. A
  localização do binário não é variável: quem grava em `$PREFIX/share/nick/` também troca o
  próprio `$PREFIX/bin/nick`. O `NICK_INIT_BIN` continua como antes (ele só troca o init);
- o binário de `cargo test` (`nick/target/debug/nick`) não tem `nick/target/share/nick/.nick-install`:
  os testes rodam no modo checkout. Nenhum teste cria esse arquivo.

**Manifesto e desinstalação sem `rm -rf`.** O `.nick-install` é texto: a primeira linha é
`# <saída do binário --version>`, que precisa ser uma linha só começando com `nick ` (senão o
`install` recusa, sem gravar nada: uma saída de várias linhas punha caminhos soltos no manifesto,
e o `uninstall` passaria a recusar a instalação inteira; review do NS-52), e depois um caminho relativo ao prefixo por linha, de tudo que
foi instalado (o `bin/nick` e o próprio manifesto inclusive). O `uninstall` sem manifesto diz
`Não há instalação do nick em <prefixo>. Nada foi feito.` e sai com 0 (idempotente, e nunca apaga
um `$PREFIX/bin/nick` que não instalou). Com manifesto, ele confere **todas** as linhas antes de
remover qualquer coisa: só `bin/nick` ou `share/nick/...` sem `..` (um manifesto alterado nunca
remove arquivo fora do prefixo; decisão deste ticket, que o plano não previa). Depois, `rm -f`
de cada arquivo (o binário primeiro, o manifesto por último) e `rmdir` dos diretórios das
entradas, do mais fundo para cima (`LC_ALL=C sort -r`), até `share/nick` (nunca `share/` nem
`bin/`). Um diretório que fica por causa de algo que o nick não instalou (um arquivo, ou uma pasta
que não está no manifesto, como `share/nick/extra/`) recebe o aviso `Mantive <dir>: tem arquivos
que não foram instalados pelo nick.`; um pai que só ficou porque um subdiretório do manifesto ficou
não recebe (o aviso vai para o subdiretório). O caso da pasta fora do manifesto entrou no review do
NS-52: antes, ela ficava sem aviso nenhum. Nunca
toca no state dir: a mensagem final diz onde estão o banco e os logs, calculado como no Rust. O
`install` por cima de uma instalação roda a mesma remoção antes, sem aviso, para que um arquivo que
saiu da lista não fique órfão. A ordem da cópia é `share/nick/`, manifesto e o binário por último:
um binário nunca existe sem os arquivos dele.

**Logs de prompt em `$XDG_STATE_HOME/nick/logs/<nome>-<id>/`** (a Q5 do épico, decidida com o
analista). O `<nome>` é o `projects.name` e o `<id>` o `projects.id`: legível, e dois projetos com
o mesmo nome de pasta não se misturam. `paths::prompt_logs_dir` saneia o nome (um componente só:
`/` e caracteres de controle viram `_`; vazio, `.`, `..` e o `/` que o `register` grava para a
raiz viram `projeto`). O canal é o `NICK_PROMPT_LOGS_DIR` que já existia no `prompt.sh` (NS-35), sem
nenhuma linha de bash nova: o `App` calcula o diretório do projeto do ticket ou do épico
(`RenderRequest::logs_dir`) e o `launch::run_render` o põe no ambiente do `bash <launcher>
--render`, **depois** do ambiente herdado. Mudança aceita: um `NICK_PROMPT_LOGS_DIR` do ambiente do
operador deixa de valer para o nick (o instalado não pode gravar em
`$PREFIX/share/nick/auto_scrum/logs/`). O `mkdir -p` continua o do `nick-agent`, dentro do state
dir, que já nasce `0700`. Os logs antigos ficam em `auto_scrum/logs/`.

**Dependências: aviso ao abrir, e a jail nunca é desligada pelo nick** (`nick/src/deps.rs`). Ao
abrir, o `main.rs` passa o `PATH` para `Deps::check`, que procura `claude`, `envsubst`, `ai-jail` e
`bwrap` (arquivo regular com bit de execução, link seguido; entradas vazias do `PATH` ignoradas,
porque seriam o cwd). Sem `which` e sem crate. Faltando algo, a barra de status ganha (junto das
outras mensagens) `Faltam no PATH: <lista>.` com uma frase por efeito e `Veja o INSTALL.md.`. Nada
trava: sem `claude`, abrir e retomar agente são recusados antes de qualquer chamada ao launcher
(nem `--check`), com `Não dá para abrir agentes: o claude não foi encontrado no PATH. Instale o
Claude Code e abra o nick de novo.`, e o resto da TUI funciona. Sem `ai-jail`/`bwrap`, o nick
**não** desliga a jail: um projeto com `JAIL_ENABLED="false"` abre agentes com o aviso de jail
desligada que já existia, e um com `JAIL_ENABLED="true"` continua recebendo o erro do `jail_check`
(regra "There is no fallback"). Sem `envsubst`, só o aviso (o erro do `--render` já chega na TUI).
O `App` nasce com tudo presente (`Deps::default`), então os testes antigos não mudaram. O teste
estrutural do NS-35 (`no_rust_source_builds_the_prompt_nor_reads_the_config`), que proíbe
`envsubst` em `nick/src/`, ganhou uma isenção só dessa palavra e só em `src/deps.rs` (decisão do
usuário no NS-52, no molde da do `review.md` em `src/docs.rs`): o `deps.rs` só procura o programa
no PATH, nunca o roda. A mesma palavra em outro arquivo continua sendo pega (asserção no teste).

**Versão e commit.** `nick/build.rs` exporta `NICK_GIT_COMMIT` (o `git rev-parse --short HEAD` do
crate, ou `desconhecido` sem git, sem repositório ou sem commit), sem nunca falhar o build. Ele
pede recompilação quando muda o `HEAD`, o ref para onde ele aponta (ou o diretório dele, enquanto o
ref não existe) e o `packed-refs`, achados pelo próprio git (`--git-dir`, `--git-common-dir`,
`symbolic-ref`), o que funciona em worktree. Sem marca de "sujo" (recompilaria a cada mudança do
índice) e sem `vergen`. **Limite conhecido (para o F6b):** o `build.rs` pega o `HEAD` de qualquer
repositório que contenha o crate. Num build do AUR, o `$srcdir` do `makepkg` costuma ficar dentro
do clone do pacote AUR, e a versão mostraria o commit do PKGBUILD. Quando chegar o F6b, comparar
`git rev-parse --show-toplevel` com `CARGO_MANIFEST_DIR/..` e cair em `desconhecido` se forem
diferentes. `version::long()` dá `0.1.0 (commit abc1234)`: o `nick --version` imprime
`nick <long>` e a tela Configurações mostra `Versão: <long>` abaixo do atalho. Sem tags de versão.

**Duas instalações, um banco.** O instalado e o do checkout usam o mesmo `nick.db`. Um nick mais
velho recusa um banco migrado por um mais novo, sem gravá-lo (a "Política de migrações" do NS-31).
O NS-52 acrescentou o teste de integração com o binário
(`a_database_newer_than_this_nick_is_refused_untouched`, em `nick/tests/cli.rs`), com a prova de
que ele morde feita numa cópia descartável.

**Testes.** Suíte bash `install/tests/test_install.sh` (achada pelo runner pelo nome e pelo lugar),
que nunca roda o cargo real: o binário vem de `--binary` (um nick falso que só responde ao
`--version`) ou de um `cargo` falso, num PATH mínimo com um `sudo` falso que nunca pode ser
chamado. Ela compara a árvore instalada com a lista gerada pela regra **escrita no teste** (não lida
do script), os modos e o manifesto, e cobre o prefixo padrão, sem `HOME`, relativo, o cargo
(argv, cwd, ausente, falhando, alvo padrão e relativo), o aviso do PATH, o uninstall (estado
mantido, arquivo alheio, sem manifesto, manifesto com caminho de fora), a reinstalação, o
`make -n` dos alvos e do padrão e o `shellcheck -x`. Rust: `version`, `resources`, `deps` e
`paths::prompt_logs_dir` com testes de unidade; o `app` (o render com o diretório do projeto de
ticket e de épico, e as recusas sem `claude`, de agente novo e de retomada, sem chamar o launcher);
o `launch` (o diretório do pedido vence o do ambiente, com o launcher real); e integração no PTY:
o log no state dir e não no `auto_scrum/logs/` (retrato antes e depois), o PATH vazio, a falta só
da jail com `JAIL_ENABLED` `false` e `true`, e o `nick/tests/install.rs`, de ponta a ponta: copia o
que a instalação precisa para um temporário com `git init` sem commit, roda o `make install` com um
`CARGO_TARGET_DIR` próprio em `CARGO_TARGET_TMPDIR` (cache entre execuções, sem disputar a trava do
build do `cargo test`), **renomeia a cópia**, roda o `--version` (`commit desconhecido`) e abre um
agente no nick instalado, apaga o `prompt.sh` do prefixo para o controle positivo e roda o
`make uninstall` da cópia movida. Os helpers do menu dos tipos no PTY (`open_menu`, `choose_type`,
`TYPE_LABELS`) ficam em `nick/tests/common/menu.rs`, usados por `agent.rs` e `install.rs` (review
do NS-52: estavam copiados). Com cache, leva uns 5 s; o primeiro build release, uns 20 s
nesta máquina. Os testes de `bin/tests/test_nick_agent.sh` e `auto_scrum/tests/test_auto_scrum.sh`
não mudaram.

## Rust no projeto: link do Jira montado pelo ID (NS-53)

A chave nova `JIRA_URL` guarda a base dos links do Jira do projeto (ex:
`https://empresa.atlassian.net/jira/browse/`). O painel do ticket e o do épico mostram
`Jira: <base><ID>`, e os formulários de `po_jira` e `tech_leader_jira` já abrem com o link
preenchido.

**A chave.** `JIRA_URL` é de texto (padrão `""`, sem nota), registrada em `lib/config/keys.sh`
logo depois de `JIRA_ENABLED`. Ela não depende da flag: o link no painel é só uma referência, e os
campos de link só existem nos tipos com Jira, que já dependem de `JIRA_ENABLED`. O valor fica
exatamente como foi digitado: o nick não acrescenta a `/` final, não confere o esquema nem valida
o formato. Fora do escopo: o link pode estar quebrado. Ela não entra no `export` nem na lista do
`envsubst` do `prompt.sh`, porque nenhum template a usa. As globais ganharam `JIRA_URL=""` no
`bin/nick-agent` e no `auto_scrum.sh`. O `auto_scrum.sh` nunca lê a chave, mas o bloco dele lista todas as chaves. Por
isso a linha leva um `# shellcheck disable=SC2034` com o motivo (P1 do review do NS-53: o aviso
passou porque nenhuma suíte rodava o shellcheck no `auto_scrum.sh`). Desde então, o
`auto_scrum/tests/test_auto_scrum.sh` tem um bloco `shellcheck -x` do `auto_scrum.sh` e dela
mesma, no molde das outras suítes.

**A regra "base + ID", com fonte única** (`tickets::jira_link(base, key, provisional)`). Ela
devolve `None` sem base, com base vazia ou com ID provisório, e `base + ID` nos outros casos. O
painel do ticket, o do épico e os formulários usam essa função, e nenhum outro lugar concatena
base e ID.

**Como a casca recebe o valor (D1): o Rust continua sem ler o `nick.conf`.** A regra do NS-31 e do
NS-35 e o estrutural `no_rust_source_builds_the_prompt_nor_reads_the_config` continuam valendo sem
isenção nova. O valor chega por dois caminhos:
- **`bin/nick-agent --jira-url`**, um modo leve novo: roda só o `load_config`, que é o começo do
  `load_and_check` (o `.nick.conf` antigo, a existência do arquivo e o `config_load`). O
  `load_and_check` passou a chamar o `load_config`, sem mudar a ordem nem as mensagens. Não roda o
  `jail_check` nem o `claude_settings_load`. Imprime uma linha só, `jira_url=<valor>` (com
  `printf`, então o valor sai literal), e sai com 0. Uma jail ligada e quebrada não esconde o link:
  o erro da jail aparece no `g`. Sem configuração (ou só com o arquivo antigo), sai com 1 e a
  mensagem de sempre. Argumento extra dá `usage_error`, e o modo não grava nada;
- **a 4ª linha do `--check`**, `jira_url=<valor>`. O formulário usa o valor do momento do `g`, e o
  `jira_url` da `Row` do projeto passa a ser esse valor.

**O protocolo do Rust (D7).** O `parse_check` exige **exatamente quatro linhas**, nesta ordem:
`jail=`, `jira=`, `sentry=` e `jira_url=`. O valor é o resto da linha, pode ser vazio e precisa ser
UTF-8. As três linhas de antes viram `UNEXPECTED_CHECK_MESSAGE`, porque binário e bash de versões
diferentes não podem ser aceitos em silêncio (o racional do NS-52). Também é recusada uma 4ª linha
que não seja `jira_url=<valor>` UTF-8: o projeto fica sem abrir agente até o valor ser corrigido,
o lado seguro de "nada é presumido" (decisão desta execução). O `Precheck` ganhou
`jira_url: Option<String>`, que fica `None` quando o valor é vazio. O `launch::run_jira_url`
segue o molde do `run_check`: `Command::new("bash")`, cwd no projeto e stdin em `/dev/null`. O
`parse_jira_url` aceita a linha `jira_url=<valor>` exata, com ou sem `\n`. Uma falha (código ≠ 0,
saída inesperada, não UTF-8, launcher ausente) vira `None` sem mensagem na barra: o painel só
deixa de mostrar o link, e o erro de verdade aparece no `g`. O stderr do sucesso, com os avisos da
configuração, é ignorado: esses avisos aparecem na prévia do `g`.

**Onde o valor mora, e quando é lido (D8, D9, com uma mudança decidida pelo usuário).** A `Row`
ganhou `jira_url`. O leitor é injetável (`App::with_jira_url_reader`, um
`Box<dyn FnMut(&Path, &Path) -> Option<String>>`, no molde do `with_agents`). O padrão do
`App::new` devolve `None` sem rodar nada, então os testes de unidade não rodam bash. O `main.rs`
liga o `launch::run_jira_url`. O leitor só roda com o projeto `Available` e o launcher sendo
arquivo.

**Mudança no plano, decidida pelo usuário na execução:** o D8 mandava ler a cada `App::refresh`.
Só que o `refresh_tree` roda depois de **toda tecla** na árvore, e cada `--jira-url` custa uns
28 ms nesta máquina: isso dava uns 85 ms de atraso por seta com 3 projetos. Por isso a leitura
roda só quando o projeto entra na lista ou acaba de ficar disponível. Nos outros refreshes, o
valor vem da `Row` anterior, se o projeto já estava disponível com o mesmo caminho. O
`with_jira_url_reader` e o `with_agents` releem todos os projetos disponíveis, então a ordem das
duas chamadas no `main.rs` não importa. Impacto: uma edição à mão da `JIRA_URL` com o nick aberto
só aparece no painel depois de um `g` naquele projeto, de o projeto ficar indisponível e voltar,
ou de reabrir o nick.

O `TicketDetail` e o `EpicDetail` ganharam `jira_link`, calculado no `read_ticket_detail` e no
`read_epic_detail` a partir da `Row` do projeto, da chave e do `is_provisional`. Como os detalhes
são relidos depois de cada ação, o link acompanha o "Editar ID" (NS-36 e NS-40) sem código a mais.
Um provisório que ganha um ID passa a ter link.

**O formulário (D10).** O `Holder` ganhou `provisional`, preenchido nos dois `From`. O plano
falava em 8 literais de `Holder`, mas os únicos que existem são os dois `From`.
`StageType::initial_values(holder_key, jira_link)` faz o campo `Link do ticket no Jira` nascer
com o link. O `ID do Jira` continua nascendo com a chave, mesmo provisória, como no NS-35. O link
vem de `jira_link(precheck.jira_url, holder.key, holder.provisional)` no `key_choose_type`, e
nunca da `Row`. O campo continua editável e obrigatório.

**O init (D2, D5, D6).** O `config_init` pergunta as chaves com pergunta, na ordem do registro,
depois da sobrescrita e antes do `config_dir_prepare`. A resposta é aparada nas pontas como faz o
`read -r` (o `IFS` padrão vai explícito no `read`). Enter e EOF gravam `""` e o init segue: aqui o
EOF **não cancela**, diferente da sobrescrita. Assim, todo `nick-init < /dev/null` continua
gerando o esqueleto. Um EOF depois de texto sem `\n` também grava `""`. Uma resposta com `"` ou
com caractere de controle (`[[:cntrl:]]`; sob `LC_ALL=C` um acento passa) é recusada com
`Erro: a resposta não pode ter aspas duplas (") nem caracteres de controle. Tente de novo.`. A
mensagem não ecoa o valor, e a pergunta se repete. O `read -p` só mostra a pergunta num terminal,
então os testes provam a repetição pela contagem de recusas. Para provar que nada é criado antes
das perguntas, um teste usa um FIFO. O
`config_print_skeleton <tool> [CHAVE=valor...]` sem pares dá a saída de antes, byte a byte. Cada
par troca o padrão da linha `CHAVE="..."` pelo valor literal. Um par de chave não registrada (ou
sem `=`) é recusado antes de imprimir qualquer coisa: mensagem no stderr e retorno 1. O
`bin/nick-init` não mudou.

**No painel (D11).** No ticket, a linha `Jira: <link>` entra logo depois da linha do épico (ou do
cabeçalho, sem épico), antes de `Etapa atual`. No épico, entra logo depois de `Pasta:`. As duas
passam por `printable()`. Sem link, nenhuma linha aparece, e a tela fica idêntica à de antes. Com
agente vivo, o painel tem 40 colunas e o fim do link pode ser cortado (impacto aceito, como no
NS-36).

**Impactos aceitos:** todo `.nick/nick.conf` existente (inclusive o deste repositório) passa a
receber o aviso de chave ausente da `JIRA_URL` até ela ser acrescentada. O link montado pode não
existir no Jira. O valor do painel vem da última leitura (acima). E roda um `bash` a mais por
projeto quando ele entra na lista ou volta a ficar disponível.

**Testes adaptados, com o motivo:**
- `lib/tests/test_keys.sh`: os retratos fixados à mão (`CONFIG_KEYS`, padrões, notas, o corpo do
  esqueleto, a contagem 11 e o "falha no meio") ganharam a `JIRA_URL`, porque a lista de chaves
  mudou de propósito. A carga limpa confere os cinco arrays;
- `auto_scrum/tests/test_auto_scrum.sh`: os quatro avisos de chave ausente fixados à mão ganharam
  `JIRA_URL=""`, pelo mesmo motivo ("onze chaves");
- `bin/tests/test_nick_agent.sh`: o `--check` espera quatro linhas, o `USAGE` cita `--jira-url`, e
  a matriz do diferencial ganhou `JIRA_URL` no "tudo ligado";
- `nick/src/launch.rs`: `jail_true_and_jail_false_are_read`,
  `the_flags_of_jira_and_sentry_are_read`, `any_other_stdout_with_exit_0_is_an_error`,
  `the_stderr_of_a_success_is_a_warning` e `a_failure_is_the_stderr_or_the_code` usam quatro
  linhas. Os formatos recusados continuam recusados, e entraram as três linhas antigas, a 4ª linha
  fora do formato ou em outra posição e o valor não UTF-8;
- `nick/src/stages.rs`: `the_id_starts_with_the_ticket_and_the_rest_empty` e
  `the_form_of_an_epic_starts_with_its_key` usam a assinatura nova, com as mesmas asserções e
  `None`;
- `nick/tests/stages.rs`: o diferencial do épico usa `initial_values(.., None)`. Os literais de
  `Precheck` (`app.rs`, `ui.rs`) e de `Row` (`tree.rs`, `ui.rs`) ganharam o campo novo, só
  sintaxe. Os testes atuais de painel (`draw_ticket_detail`, `epic_lines`) passaram sem mudança.

**Testes novos:** `lib/tests/test_config.sh` (registro com 4 ou 5 argumentos, pergunta em `_ENABLED`,
pergunta vazia, pares do esqueleto, as perguntas do init, inclusive recusa, EOF, sobrescrita e
FIFO); `lib/tests/test_keys.sh` (a `JIRA_URL` depois de `JIRA_ENABLED`, a única com pergunta, com o
texto fixado); `bin/tests/test_nick_init.sh` (URL no stdin; `s` e a URL; N; aspas recusadas; nenhum
código da chave no script); `bin/tests/test_nick_agent.sh` (a 4ª linha literal com `$(id)`, a chave
ausente, a do ambiente que não conta, e o `--jira-url` com jail quebrada de quatro jeitos, com
controle positivo pelo `--check`, sem configuração, legado, argumentos e o cabeçalho);
`launch.rs` (`parse_check` com a URL, `parse_jira_url`, `run_check` e `run_jira_url` com o launcher
real, inclusive com a jail quebrada); `tickets.rs` (`jira_link`); `stages.rs` (o campo do link);
`app.rs` (leitor do `new`, leitura só quando o projeto fica disponível e sem launcher, os detalhes
nos três casos para ticket e épico, o "Editar ID" dos dois, o formulário pelo `Precheck` e não pela
`Row`, provisório e sem URL, o `ID do Jira` igual ao do NS-35, o campo editável e obrigatório até
o `render`, a `Row` atualizada pelo `g`); `ui.rs` (a linha no painel do ticket, do ticket num épico
e do épico, a posição, o `?` no lugar do controle); `nick/tests/stages.rs` (o diferencial de
`po_jira` e `tech_leader_jira` com o link do `initial_values` × o script com o link digitado); e,
no PTY, `nick/tests/agent.rs` (painel → `g` → formulário preenchido → prévia com
`Link do ticket no Jira: <base><ID>`; épico com jail quebrada; sem `JIRA_URL`, sem linha). As telas
ficam em `.nick/docs/NS-53/`.

## Rust no projeto: concluir épicos e filtro de concluídos (NS-54)

> **Mudou no NS-61** (seção "Rust no projeto: etapa Rejeitado (NS-61)"): "concluído"
> passou a ser "fechado" (`TicketStage::CLOSED`: Concluído ou Rejeitado). O `d` esconde também os
> rejeitados, e o `complete_epic` e o `reopen_if_pending` contam como pendente só o ticket não
> fechado (`stage NOT IN (?2, ?3)`, com as chaves de `db::closed_keys()`). O rodapé do `d`, a
> mensagem `Ele sai da árvore (d mostra os concluídos).` e as mensagens de pendência não mudaram.

História filha do épico NS-28. O épico (NS-40) ganhou fim: dois estados, **aberto** e **concluído**,
com reabertura automática, e a árvore ganhou um filtro que esconde os concluídos. Os filtros `f` e `d`
são lembrados entre aberturas. Fora do escopo: outros estados de épico, concluir em lote, lembrar o
`v`, cor nova para o épico e sincronizar com o Jira.

**A regra, que vale sempre: um épico concluído tem todos os tickets concluídos.** "Todos" inclui os
ocultos: um ticket oculto e não concluído impede a conclusão.

**Banco: migração 10** (`ALTER TABLE epics ADD COLUMN done_at TEXT`). `done_at` é um timestamp RFC 3339
UTC, no formato do `hidden_at`, e `NULL` quer dizer aberto. Não tem `CHECK` (dois estados não precisam
de enum no banco), então a tabela não é reconstruída. Todo épico que já existia fica **aberto**, mesmo
com todos os tickets concluídos: nada é inferido, e o operador conclui quando quiser. O `Epic`, o
`EPIC_COLUMNS` e o `epic_from_row` ganharam `done_at`. O teste v9 → v10 usa o banco populado do NS-45
(projetos, épicos visíveis, ocultos e provisórios, tickets em várias etapas dentro e fora de épico,
agentes de ticket e de épico, histórico, settings) mais um épico com todos os tickets concluídos. Ele
confere todos os `id` e campos, `done_at` nulo em todos, `foreign_key_check` e `integrity_check`
vazios e os seis gatilhos da migração 9.

**Concluir e reabrir à mão (`c` no nó do épico, D2 e D4).** Com `done_at` nulo, `c` conclui
(`epics::complete`); com `done_at`, reabre (`epics::reopen`). Não há `(s/N)` em nenhum dos dois
sentidos: a mesma tecla desfaz, como o restaurar (NS-40). O `c` do nó do módulo (configurações) não
mudou, porque as teclas são por nó, e o `C` vale pela regra do NS-43. O `Db::complete_epic` é **um
`UPDATE` condicionado** (aberto, com tickets, nenhum ticket com etapa diferente de
`TicketStage::Done.key()`, que vai por parâmetro e não como literal). Ele roda numa transação, e com 0
linhas o motivo é relido nela, que já tem a trava de escrita: `EpicCompletion::{Done, Gone,
AlreadyDone, Empty, Pending(n)}`. Concluir um épico oculto, ou com agente ativo, é permitido: concluir
não esconde nada; quem esconde é o filtro. Mensagens:
- `Épico concluído: <ID>.` e, quando o filtro de concluídos se aplica (`d` ligado e `v` desligado:
  `Tree::hides_done`, o mesmo critério da árvore), ` Ele sai da árvore (d mostra os concluídos).` (o
  `epics::complete` recebe esse estado num `bool`, sem depender do `Tree`);
- `O épico <ID> ainda tem 1 ticket não concluído.` / `... tem <N> tickets não concluídos.`
  (`epics::pending_message`, com o plural de `paths::count_of`). Quando algum dos pendentes está
  oculto, a recusa diz quantos e a tecla que os mostra, no molde da recusa de ID duplicado: `(oculto;
  f mostra os ocultos)`, `(ocultos; ...)` quando todos estão ocultos, ou `(<M> oculto(s); ...)`. O
  `EpicCompletion::Pending` traz `{ pending, hidden }`. Sem isso, um épico com o único ticket pendente
  oculto parecia pronto na árvore, e o `c` parecia não fazer nada (achado na validação com o banco
  do operador: o NS-42 tinha um ticket provisório oculto em PO);
- `O épico <ID> não tem tickets e não pode ser concluído.` e `O épico <ID> já está concluído.` (outro
  nick concluiu antes);
- `Épico reaberto: <ID>.` e `O épico <ID> já está aberto.`; sumido, o `gone_message` de sempre.

**Reabertura automática: no código, na mesma transação do escritor do ticket (D3).** Só três métodos
do `Db` escrevem num ticket o que pode quebrar a regra, e cada um reabre o épico na mesma transação da
escrita, por um `UPDATE epics SET done_at = NULL ... AND EXISTS (ticket do épico não concluído)
RETURNING key` (`reopen_if_pending`, privado do `db/mod.rs`). A leitura do épico afetado vem **depois**
da escrita do ticket, já com a trava. Os três:
- `insert_ticket_reopening` (o épico do ticket novo, que nasce em `open`, então sempre reabre um épico
  concluído). **Diferença do plano, prevista nos riscos dele:** o `insert_ticket` continua com a mesma
  assinatura, agora em cima do irmão (a mesma transação), porque cerca de 70 testes o chamam. O INSERT
  mora só no irmão, e é este nome que o estrutural cobra;
- `set_ticket_epic` (o épico de destino), que passa a devolver `(bool, Option<String>)`. Ele reabre só
  se o ticket movido não estiver concluído;
- `change_ticket_stage` (o épico lido no SQL, `SELECT epic_id FROM tickets WHERE id = ?1`, e não no
  `Ticket` em memória, que pode estar velho), que passa a devolver `(StageChange, Option<String>)`.

**Por que no código e não em gatilho:** a barra de status precisa saber que *esta* ação reabriu o
épico. Um gatilho reabriria em silêncio, e descobrir depois exigiria ler antes e depois numa transação
`DEFERRED`, sujeita à corrida com outro nick. O `UPDATE ... RETURNING` dentro da transação do escritor
já tem a trava de escrita, então a resposta é exata. A corrida entre dois nicks fica fechada porque o
SQLite serializa as escritas: se o "concluir" de um nick vem antes, a escrita do ticket do outro
reabre; se vem depois, a condição do `complete_epic` falha. Como a regra vive no código e não em
gatilho, uma reconstrução futura de `tickets` ou `epics` não precisa recriar nada por causa dela.

**Garantia de que não há outro escritor:** o estrutural
`only_the_three_writers_of_the_db_write_a_ticket_and_they_reopen_the_epic` (`nick/tests/agent.rs`)
cobra que fora de `src/db/` não há `INSERT INTO tickets`, `UPDATE tickets SET stage` nem `UPDATE
tickets SET epic_id`, e que no `db/mod.rs` cada um aparece uma vez, no seu método
(`insert_ticket_reopening`, `set_ticket_epic`, `change_ticket_stage`), cujo corpo tem a transação, o
`reopen_if_pending(&tx, ...)` e o commit. As migrações ficam de fora (o `INSERT INTO tickets_new` é
outra tabela). O código é lido com os espaços colapsados e sem os `\` de continuação das strings, e as
escritas valem em qualquer grafia: um ticket novo por qualquer `... INTO tickets` (`INSERT`, `REPLACE`,
`INSERT OR ...`) e a etapa ou o épico em qualquer ponto da lista do `SET` (`SET title = ?, stage = ?`).
O controle positivo cobre uma escrita em outra função, essas grafias, o `UPDATE tickets SET title`
(que não é escrita da regra) e o histórico (`ticket_stage_changes`, outra tabela). Os estruturais de antes (`only_move_ticket_moves_a_ticket_between_epics`
e o da etapa do NS-45) passaram sem mudança.

**Casos que não reabrem (decisão do plano):** tirar o último ticket de um épico concluído (ele fica
concluído e vazio: a regra continua verdadeira, e "vazio não pode ser concluído" é uma regra da ação
de concluir); mover um ticket já concluído para um épico concluído; ocultar ou restaurar um ticket; e
editar título ou ID. Uma reabertura feita por **outro nick** só aparece no próximo refresh da árvore,
sem aviso aqui.

**O aviso.** `epics::reopened_notice(épico, ticket)` dá `O épico <EID> foi reaberto: o ticket <ID> não
está concluído.`, e `epics::with_reopened(mensagem, reaberto, ticket)` o acrescenta **só** quando houve
reabertura (senão a mensagem de antes, byte a byte). Ele entra em `epics::move_ticket`,
`tickets::change_stage`, `tickets::advance_on_agent_opening` e na criação do App, que passou a chamar
`tickets::create_in_reopening` (irmão novo de `create_in`, que não mudou de assinatura; o corpo único
continua no `create_in_with`). O avanço ao abrir agente nunca sai de `Done` (NS-45), então ele só
reabre um épico concluído contra a regra (feito à mão no teste). Exemplos:
`Ticket criado: NS-9 — Login. O épico E-1 foi reaberto: o ticket NS-9 não está concluído.` e
`Etapa do ticket NS-3: Concluído → Dev. O épico E-1 foi reaberto: o ticket NS-3 não está concluído.`

**O filtro de concluídos: tecla `d`, `Tree::hide_done` (D5).** Por padrão ele fica desligado e tudo
aparece. Ligado, some todo ticket com etapa `done` (em qualquer profundidade) e todo épico com
`done_at`, **com o ramo inteiro** (os agentes do épico e os tickets dele). Com ele desligado, a árvore
gera exatamente as linhas de antes. O `d` fica em `App::key_tree`, ao lado do `f` e do `v`, sem relayout
(o `reselect` precisa das linhas de antes).
- **Diferente do `f`, um agente vivo não segura o nó.** O ticket aceitou que um ticket ativo pode sumir
  da vista. O agente continua acessível pelo `v`: com `only_active`, o `hide_done` não se aplica, e o `v`
  mostra todo ramo com agente vivo, como já fazia com os ocultos.
- Um agente vivo cujo ramo o filtro tirou **não conta acima dele**: a marca `● saída nova` não sobe aos
  ancestrais fechados, não segura um épico oculto e não abre um projeto indisponível. O `visible` filtra
  os vivos uma vez, no começo, e só com o filtro aplicado: os ids dos épicos e tickets concluídos são
  lidos uma vez num `DoneIds` (dois `HashSet`), e sem o filtro os dados são usados como estão, sem
  cópia. O rodapé `v agentes ativos (N, M com saída nova)` continua contando o agente.
- O `f` e o `d` são independentes: um ticket oculto e concluído só aparece com o `f` ligado **e** o `d`
  desligado.

**Filtros lembrados (`settings`, D6).** Há duas chaves em `db/mod.rs`: `SETTING_TREE_SHOW_HIDDEN =
"tree_show_hidden"` e `SETTING_TREE_HIDE_DONE = "tree_hide_done"`. O valor `"1"` quer dizer ligado, fora
do padrão, e voltar ao padrão **apaga a linha** (regra da migração 4). Cada `f` ou `d` grava na hora
(`App::store_filter`). Se o banco der erro, o filtro muda na tela mesmo assim, e a barra mostra o
`db_error_message`. O `App::load_filters` (no molde do `load_shortcut`) roda no `main.rs` logo depois
do atalho, e as mensagens se juntam na mesma linha, com o atalho primeiro. Sem linha, vale o padrão.
Qualquer outro valor também cai no padrão, com `A configuração <chave> gravada no banco (<valor>) não é
válida. Usando o padrão.` (o valor passa por `printable`), e as duas mensagens vêm na ordem do rodapé.
O `v` **não** é lembrado. O `App::filters()` continua devolvendo `(f, v)`, porque um teste de antes
compara esse par, e o `d` sai por `App::hide_done()`.

**Desenho (D7).** O nó do épico ficou `<ID>  [épico]  [aberto|concluído]  <título>  [provisório]
[oculto]`,
sem cor nova (o roxo do NS-45 continua no ID e no título). **Diferença do plano, decidida pelo
usuário na validação:** o plano punha a marca depois de `[provisório]`, mas na profundidade 2 o
rótulo tem 30 colunas (40, menos a borda, o `> `, o recuo e a seta), e com qualquer título de
tamanho normal a marca era cortada inteira: o épico concluído ficava igual ao aberto na árvore. Logo
depois de `[épico]`, ela cabe inteira com um ID de até 8 caracteres (o provisório), e o corte leva o
título. **Também por decisão do usuário, o épico aberto mostra `[aberto]` no mesmo lugar** (o plano
deixava o rótulo de antes): o estado fica sempre visível na árvore, e não só pela falta da marca. É
`[aberto]` minúsculo, sem cor, diferente da etapa `[Aberto]` do ticket (branca, NS-45). O
painel do épico tem a mesma marca no cabeçalho e a linha `Estado: aberto` / `Estado: concluído em
<data>` (`short_date`, como o `Aberto em:` do agente), logo depois de `Tickets:`. Ela não fica entre
`Pasta:`, `Jira:` e `Plano`, porque um teste do NS-53 cobra que essas três linhas sejam seguidas. O
`Tickets: N (M ocultos, K concluídos)` opcional ficou de fora: ele mudaria a construção do
`EpicDetail` nos testes. Rodapés:
- épico: `... · o ocultar/restaurar · c concluir/reabrir · g abrir agente · i editar ID`;
- filtros, depois do `f`: `d esconder concluídos` (padrão) / `d mostrar concluídos` (ligado).

O `HelpState` ficou `(Place, f, d, v)`, e o `HELP_STATES` tem os 65 estados (8 lugares da árvore × 8
combinações dos três filtros, mais Configurações), gerados por uma `const fn`. Um teste confere as 65
combinações.

**A altura reservada do rodapé cresceu uma linha** (P1 do review do NS-34; o limite de +1 é a decisão
do analista no plano). Foi medido com o `help_rows` real, o texto novo contra o mesmo texto sem `· d …`
e `· c concluir/reabrir`, com 1, 2, 3, 9 e 10 agentes. Com agente vivo, o rodapé ficou com +1 linha em
50, 58, 78, 80, 100 e 120 colunas e +0 em 140; com 10 agentes a 100 colunas, +0. Sem agente vivo, o
rodapé do módulo a 140 colunas passou de 1 para 2 linhas (aí não há PTY para redimensionar). O terminal
do agente perdeu uma linha. Números que mudaram:
- `a_resize_reaches_claude_below_the_header` (`nick/tests/agent.rs`): `SIZE 23 58` → `SIZE 22 58` e
  `SIZE 34 78` → `SIZE 33 78` (o rodapé reserva 3 linhas, e não 2, a 100 e a 120 colunas);
- `moving_on_the_tree_sends_no_sigwinch_to_claude` e `the_fast_cat_sends_no_sigwinch_to_claude`
  (`nick/tests/agent.rs`): `SIZE 23 58` → `22 58` e `34 78` → `33 78`, pelo mesmo motivo. O primeiro
  também passou a alternar o `d` (nenhum SIGWINCH);
- `with_an_agent_the_list_is_on_the_left_and_the_agent_on_the_right` (`ui.rs`): a 100×30, a lista e o
  painel passaram de 27 para 26 linhas, o terminal de 24 para 23, a barra de status da linha 27 para
  a 26 e o rodapé de 2 linhas (na 28) para 3 (na 27). O resto segue: o `ultima` fica na linha 24, o `└`
  na 25 e o rodapé do terminal na 27;
- `the_warning_out_of_the_jail_wraps_in_the_header_and_the_terminal_shrinks` (`ui.rs`): o terminal
  fora da jail passou de 23 para 22 linhas, e o da jail de 24 para 23;
- `the_focused_panel_has_the_thick_cyan_border` (`ui.rs`): a borda de baixo subiu da linha 26 para a 25;
- `the_terminal_keeps_its_size_whatever_the_focus_the_marks_and_the_dialog` (`ui.rs`): sem agente, o
  rodapé do módulo a 140 colunas tem 2 linhas, e não 1. Ganhou o caso de 160 colunas, onde é 1.

**Testes adaptados, com o motivo:**
- `db/mod.rs`: `a_new_database_gets_version_9_…` → `…_version_10_…`, `opening_twice_duplicates_nothing`
  (10), os dois de "banco mais novo" (11 e `(10)`) e `migration_1_is_pinned` (10 migrações e o texto da
  10), pela migração nova; `a_version_8_database_populated_migrates_to_9_…` passou a abrir só até a
  migração 9, porque o `SELECT *` de `epics` ganharia a coluna da 10 (o teste da 10 é próprio); os
  testes de `set_ticket_epic` e de `change_ticket_stage` ganharam `.0`, só na construção do retorno
  novo, com as mesmas asserções;
- `tickets.rs`: `a_ticket_gone_or_moved_by_another_nick_stores_nothing` ganhou `.0`, só na construção;
- `tree.rs` e `ui.rs`: os literais de `Epic` ganharam `done_at: None`;
- `ui.rs`: os 10 rodapés completos fixados nos testes ganharam o `· d esconder concluídos` (e o do épico,
  o `c concluir/reabrir`), porque o rodapé mudou de propósito; `every_help_state_is_in_help_states` e
  `the_project_help_has_n_and_the_epic_place_is_in_the_help_states` usam o `HelpState` de quatro campos
  (65 estados, 8 do épico); `moving_on_the_tree_and_the_filters_resize_no_terminal` ganhou combinações
  com o `d`; o helper `last_line` passou a pegar a última linha **com texto**. Com 3 linhas
  reservadas, o rodapé de um diálogo deixa duas vazias embaixo, e o `str::lines()` só ignora uma (o
  `the_confirmations_of_ending_one_and_quitting` não mudou de asserção). Os números de tamanho estão
  acima;
- `nick/tests/agent.rs`: `a_real_version_6_database_opens_migrated_with_the_tree_of_before` (versão 10)
  e os três de `SIZE` (acima).

Os testes de `hide_epic`/`restore_epic`, de etapa (NS-45), de `settings` (NS-34), de mover (NS-40), de
criação e mensagens de ticket e os de `tree.rs` passaram sem mudar asserção. Nenhum teste foi apagado.

**Testes novos:** `db` (v9 → v10 populado, `done_at` lido, cada `EpicCompletion`, inclusive o ticket
oculto contando, épico oculto e com agente ativo, `reopen_epic` nos três casos, a reabertura nos três
escritores e os casos que não reabrem, a atomicidade com um gatilho de teste que impede a reabertura);
`epics` (mensagens de concluir, recusas, reabrir, aviso, mover com e sem reabertura, criar no épico
concluído); `tickets` (a etapa saindo de Concluído e o avanço); `tree` (padrão sem mudança, ticket nas
profundidades 2 e 3, épico com o ramo, o vivo não segura, o `v`, `f` × `d`, a marca e o `reselect`);
`app` (`c` com as recusas, concluir e reabrir, com o `d` ligado, `d`/`f` gravando e apagando, o `v` não
gravado, erro do banco ao gravar, `load_filters` sem linha, com `"1"`, inválido e com erro, filtros num
`App` novo sobre o mesmo banco, o vivo de um ticket concluído pelo `v`, e o aviso pelo `a` no épico, pelo
`p` e pelo `m`); `ui` (a marca, o `Estado:`, os rodapés, os 65 estados); o estrutural de D3; e, no PTY
(`nick/tests/tui.rs`), `a_done_epic_and_the_filter_d_are_remembered_when_the_nick_opens_again` (concluir,
`d`, sair, abrir de novo com o filtro ligado e o épico fora, `d` apagando a linha) e
`a_ticket_leaving_done_in_a_done_epic_reopens_it_with_the_notice_on_the_bar`. As sabotagens estão em
`.nick/docs/NS-54/sabotagens.md`, e as telas em `.nick/docs/NS-54/telas.txt` e `telas_ansi/`.

**Ajustes do review do NS-54** (`.nick/docs/NS-54/review.md`). Os quatro pontos procediam. Os de
comportamento ganharam teste que falhou antes da correção, provado também na cópia (S13 e S14 em
`.nick/docs/NS-54/sabotagens.md`):
- **P1:** com o `d` e o `v` ligados, concluir um épico com agente vivo dizia `Ele sai da árvore`, mas o
  épico continuava nela (o `v` mostra todo ramo com agente vivo). O `c` passa o `Tree::hides_done()`
  (agora `pub(crate)`) e não o `hide_done` cru. Teste:
  `c_with_the_filters_d_and_v_does_not_say_the_epic_leaves_the_tree`;
- **P2:** o `count_of` saiu do `ui.rs` e foi para o `paths.rs`, ao lado do `printable`, o texto já
  compartilhado entre os módulos. Ele ficou genérico (`usize` e `i64`), e o `epics::pending_message` e o
  `epics::active_message` passaram a usá-lo. As mensagens não mudaram, e os testes de antes passaram sem
  mudança; entrou o `count_of_says_one_or_many`;
- **P3:** o `Tree::visible` não copia os vivos sem o filtro e, com ele, consulta os `HashSet` do
  `DoneIds` em vez de varrer todos os épicos e tickets para cada agente vivo. Os testes de `tree.rs`
  passaram sem mudança;
- **P4:** os quatro rodapés esperados acima de 100 colunas no `ui.rs` foram quebrados com `\`, dois
  comentários de documentação foram refluídos, e o estrutural de D3 passou a reconhecer as outras
  grafias (acima), com controle positivo que falhou antes.

**Validação com o banco do operador** (fora da jail, sempre numa cópia do `nick.db`; o original foi
conferido byte a byte depois). O NS-42 "não mudava" porque o `c` era recusado por um ticket oculto que
a árvore não mostrava, e porque a marca `[concluído]` era cortada. Por decisão do usuário, a recusa
passou a apontar os ocultos e a marca foi para logo depois de `[épico]` (ambos acima). Os testes que
mudaram por isso: `the_label_of_a_done_epic_has_its_mark_right_after_the_mark_of_the_epic` (era
`..._between_provisional_and_hidden`), as recusas com oculto de `complete_epic_is_refused_with_...`,
`complete_is_refused_with_...` e `c_on_an_epic_refuses_...`, e as esperas do PTY por
`EPIC-1  [épico]  [concluído]`. Com o `[aberto]`, mudaram de propósito os testes do NS-40/NS-45 que
fixavam o rótulo do épico aberto: `the_label_of_an_epic_has_the_mark_bold_and_dims_when_hidden` (o
texto e os índices dos spans, um a mais), `the_id_and_the_title_of_the_epic_are_purple_on_the_tree`
(o título 10 colunas adiante, e o `[aberto]` sem cor), e, no PTY (`nick/tests/agent.rs`),
`an_epic_from_its_creation_to_the_resume_of_its_agent`,
`opening_new_agents_advances_the_stage_and_each_stage_has_its_color` e
`hiding_an_epic_is_refused_with_a_live_agent_and_then_takes_and_brings_back_the_branch`, que passaram a esperar `EPIC-1  [épico]  [aberto]  Pla` (o que cabe nas 30 colunas). As telas estão em `.nick/docs/NS-54/telas_validacao_banco_real.txt`.

## Rust no projeto: letreiro do nó selecionado na árvore (NS-55)

História filha do épico NS-28. Antes, um nome que não cabia na coluna da árvore era cortado na
borda pelo `List` do ratatui, sem nenhuma indicação. Agora:

- a linha **não selecionada** que não cabe termina em `…`, sem passar da borda;
- a linha **selecionada** que não cabe rola como um letreiro: começa parada, avança uma coluna por
  vez até mostrar o fim, pausa e volta de uma vez ao começo, em ciclo;
- uma linha que cabe fica como antes: não rola e não ganha `…`;
- trocar a seleção reinicia o letreiro do nó novo do início.

Fora do escopo: quebrar o nome em várias linhas, mudar a largura da coluna (`ui::LIST_WIDTH`), a
tela de Configurações, os títulos dos blocos e o painel da direita.

**Decisões (os números são os do plano):**
- **D1. O que rola:** tudo que vem **depois** da seta (`▾ `/`▸ `/`  `), ou seja, o rótulo do nó e,
  num nó fechado, a marca `  ● saída nova`. O recuo e a seta ficam parados, para a hierarquia
  continuar legível. Numa linha não selecionada, a marca de nó fechado pode ficar sob o `…`, como
  antes ela já era cortada pela borda.
- **D2. Largura disponível** de uma linha: a largura do conteúdo da coluna (`content_area` do
  `left_column`, com ou sem a gata), menos o símbolo de seleção e menos o recuo e a seta
  (`ui::tree_available`). O símbolo é a constante `ui::HIGHLIGHT_SYMBOL = "> "`, usada pelo `List` e
  pela conta, e não um `2` solto. O `List` reserva esse espaço em todas as linhas quando há
  seleção, e na árvore sempre há (`HighlightSpacing::WhenSelected`, o padrão, ficou). A **sobra** é
  `largura do texto − disponível` (`saturating_sub`). Com sobra 0, a linha é desenhada como antes.
- **D3. Linha não selecionada com sobra** (`marquee::ellipsize`): os graphemes que cabem em
  `disponível − 1` colunas e, depois, `…` (`Span::raw`, sem estilo próprio: herda o da linha, como o
  DIM de um ticket oculto ou de um agente parado). Cada span mantém o próprio estilo (a cor da
  etapa, o vermelho do `[FORA DA JAIL]`). Um caractere largo que não cabe inteiro antes do `…` sai
  inteiro, e a linha fica uma coluna mais curta.
- **D4. Linha selecionada com sobra** (`marquee::window`): a janela de `disponível` colunas a
  partir da coluna `offset` do texto, **sem** `…` (o movimento já diz que há mais texto). Um
  caractere largo cortado por uma das pontas da janela vira espaço na metade que aparece, então a
  janela tem sempre `disponível` colunas e o texto não "pula".
- **D5. Tempo** (constantes em `marquee.rs`, `PRODUCTION`): 1500 ms parado no começo
  (`hold_start`), 150 ms por coluna (`step`) e 1500 ms parado no fim (`hold_end`). Depois ele volta
  de uma vez ao começo (sem rolar para trás). Com `t = (agora − início) % ciclo`, em que
  `ciclo = hold_start + sobra·step + hold_end`: `t < hold_start` dá `offset 0`; até
  `hold_start + sobra·step`, `offset = (t − hold_start) / step`; depois, `offset = sobra` (o fim à
  mostra). Com sobra 0, o `offset` é sempre 0 e não há prazo para acordar.
- **D6. Reinício:** o letreiro guarda o `NodeId` que está rolando e o instante em que começou. Quando
  o nó selecionado muda (inclusive voltar a um nó anterior), ele recomeça do `offset 0` a partir do
  `agora`. Um refresh que muda o texto do **mesmo** nó (título editado) ou um resize **não**
  reinicia: a sobra é recalculada, e o `offset` desenhado é limitado à sobra nova
  (`Marquee::offset_for`, `min(offset, sobra)`). O `App::select` não recebe `Instant`: o reinício
  sai do `tick`, que compara o `NodeId` guardado com o selecionado, sem mexer nas dezenas de
  chamadas de `select`.
- **D7. Onde rola:** só na tela da árvore. Nas Configurações a sobra é 0. O letreiro continua
  rolando com o foco no terminal do agente e com diálogo aberto, porque a árvore continua visível.
  É só redesenho, sem `after_change` (a mesma regra da gata): não muda o tamanho de nenhum PTY, nem
  o modo do terminal de fora, nem o texto guardado do agente escondido.
- **D8. Custo aceito no ticket:** enquanto o nó selecionado tem sobra, há cerca de 1 redesenho a
  cada 150 ms durante o movimento, mais 2 nas pausas de cada ciclo, sempre com o limite de `FRAME`
  (16 ms) do laço. Sem sobra, o custo é zero (o `next_wake` do letreiro é `None`). O laço também
  refaz a conta da sobra (`ui::marquee_overflow`, que chama o `layout`) no topo de cada volta,
  como o desenho faz a cada quadro.
- **D9. O gancho só de teste `NICK_TEST_MARQUEE`**, lido no `main.rs` logo depois do
  `NICK_TEST_MASCOT` (`marquee_hook`):
  - `still`: o letreiro nunca sai do `offset 0` e não acorda o laço. É o padrão do `Nick::spawn`
    dos testes de PTY (`tests/common/mod.rs`; um valor do caso substitui), para uma leitura de
    tela não pegar a linha selecionada no meio da rolagem;
  - `fast`: tempos curtos fixos (`FAST`: 60 ms no começo e no fim, 20 ms por coluna);
  - qualquer outro valor, ou vazio: o de produção. O gancho não desliga o `…` (D3), que não depende
    do relógio.

**Largura por grapheme, pelo ratatui, sem dependência nova.** O corte percorre os spans por
`Span::styled_graphemes` (com o estilo vazio de base, então cada grapheme sai com o estilo do
próprio span) e mede cada grapheme com `Span::raw(símbolo).width()`, o unicode-width que o próprio
buffer do ratatui usa para desenhar. Nada de `len()` nem `chars().count()`: uma letra com acento
combinante (`a\u{0303}`) é um grapheme de 1 coluna, e `🐈`/`漢` têm 2. Um grapheme de largura 0
solto acompanha o anterior. **Não** entraram `unicode-width` nem `unicode-segmentation` no
`Cargo.toml`. Sequências ZWJ e bandeiras seguem o que o ratatui fizer e não têm teste próprio.

**Reaproveitamento:**
- `nick/src/marquee.rs` é um módulo novo **no molde do `mascot.rs`**: puro (sem I/O, sem banco,
  sem `Instant::now()`), com o relógio por parâmetro. Nenhum código é compartilhado com a gata: os
  eventos dela são sorteados e liga/desliga, e o letreiro é uma função determinística da posição,
  então uma abstração comum seria só semelhança superficial. Ele tem três partes separadas: o tempo
  puro (`offset`, `next_change`, que devolve quando o **valor** muda, e não a fronteira de fase), o
  estado (`Marquee`: `new`, `still`, `fast`, `tick`, `next_wake`, `offset_for`) e o corte puro
  (`width`, `ellipsize`, `window`);
- **o padrão do `prompt_window`**: o laço pede à `ui` uma conta da tela (`ui::marquee_overflow`) e
  entrega ao `App` (`App::tick_marquee`). A conta usa as **mesmas** funções do desenho (`layout`, o
  `list_block` e o `content_area` da coluna, `tree_row`, `tree_available`), e um teste confere que
  ela bate com o que o `draw_tree` corta;
- o laço (`main.rs`) e o `terminal::idle_wait` foram reaproveitados sem mudança na função: no topo
  de cada volta, ao lado do `tick_mascot`, vêm a sobra e o `tick_marquee`; no ramo ocioso, o
  `idle_wait` recebe o `App::next_wake` (o mínimo entre a gata e o letreiro). Toda tecla que muda a
  seleção marca `dirty`, e o desenho sai sempre numa volta que começou pelo `tick_marquee`, então o
  primeiro quadro depois da troca já tem o `offset 0`. Mesmo assim, o `offset_for` devolve 0 para
  um nó diferente do guardado, como defesa;
- os spans de cada nó (`node_spans`, `project_spans`, `ticket_spans`, `epic_spans`, `agent_spans`)
  não mudaram: o letreiro corta o resultado deles.

**A extração do `tree_row` foi provada antes do corte novo.** A montagem de uma linha (recuo e seta,
rótulo, a marca de nó fechado e o estilo) saiu do `draw_tree` para o `ui::tree_row`, usada pelo
desenho e pela conta da sobra. Logo depois da extração, sozinha, o `make test` inteiro passou sem
editar nenhum teste (log em `.nick/docs/NS-55/baseline_extracao.log`).

**O estado fica no `App`** (`marquee`), e a assinatura de `App::new` não mudou (ele cria o letreiro
de produção). Novos: `set_marquee` (o gancho), `tick_marquee(now, sobra)`, `marquee_offset(nó,
sobra)` e `next_wake`. O `tick_marquee` só mexe no letreiro: não mexe em foco, seleção, diálogo,
texto digitado, barra de status nem agentes.

**Testes adaptados** (a lista completa, com antes e depois, está em
`.nick/docs/NS-55/testes_adaptados.md`): mudaram de propósito só os que conferiam o fim cortado pela
borda de uma linha longa **não selecionada**, que agora termina em `…`.
`the_rows_are_indented_with_the_arrows_and_the_label_of_each_node` (`ui.rs`),
`each_kind_of_node_has_its_panel_and_its_help`,
`hiding_an_epic_is_refused_with_a_live_agent_and_then_takes_and_brings_back_the_branch` e
`opening_new_agents_advances_the_stage_and_each_stage_has_its_color` (`tests/agent.rs`),
`a_project_whose_directory_is_gone_is_marked_unavailable` e
`the_ticket_flow_works_and_touches_no_file_of_the_project` (`tests/tui.rs`, que passou a esperar o
texto comum à linha selecionada e à não selecionada). Nenhum teste foi apagado. Também não entrou
dependência nova nem migração nova, e o README não mudou (regra "README genérico").

**Testes novos:** em `marquee` (os tempos de produção e do `fast`; `offset` e `next_change` em
instantes exatos, sem sobra, com sobra 1 e depois de um milhão de ciclos; o `Marquee` com nó novo,
o avanço só com mudança, a troca e a volta, a sobra que diminui sem reiniciar, o `next_wake`, o
`still` e o `fast`; o corte com ASCII, acento pré-composto, combinante e solto, emoji e caractere
largo nas duas pontas, estilos de vários spans e larguras 0 e 1); em `app`
(`tick_marquee_follows_the_selection`, `ticking_the_marquee_changes_nothing_but_the_marquee` e
`next_wake_is_the_nearest_of_the_cat_and_the_marquee`); em `ui` (a linha não selecionada com `…` em
projeto, épico, ticket e agente, com a borda no lugar e os estilos; a selecionada rolando, com o
prefixo parado e sem `…`; a linha curta igual a antes; acento e emoji sem deslocar a borda em
nenhuma posição; o `marquee_overflow` igual ao corte do desenho em 50, 60, 78, 100 e 140 colunas,
com e sem a gata; 0 nas Configurações; e o `layout`/`agent_area` sem mudança em nenhuma posição);
no PTY, em `tests/tui.rs` (com o `fast`: o começo, o meio e o fim, o `…` do nó não selecionado e a
volta ao começo ao sair e voltar ao nó; com o `still` padrão, a linha selecionada igual entre duas
leituras) e em `tests/agent.rs` (com o `fast`, o nó de um agente fora da jail rolando: o texto
digitado chega inteiro com o foco no terminal, e nenhum SIGWINCH em 3 s, com o controle positivo do
resize). As sabotagens estão em `.nick/docs/NS-55/sabotagens.md`, e as telas em
`.nick/docs/NS-55/prints/`.

**Ajustes do review do NS-55** (`.nick/docs/NS-55/review.md`). Os três pontos procediam, e nenhum
mexeu no código de produção além de um comentário:
- **P1, leitura da árvore nos helpers de PTY:** o `tests/tui.rs` tinha cópias próprias de
  `tree_rows` e `selected_row`, porque o `Nick::tree_rows` cortava a linha em 38 `char` do texto da
  tela, e no vt100 um caractere largo (`🐈`, `漢`) é um `char` só mas ocupa duas células: numa linha
  com ele, a borda entrava junto. A correção foi no helper comum. As funções livres
  `common::tree_rows_of(screen)` e `common::selected_row_of(screen)` (no molde do `cat_rows`) leem
  as colunas 1 a 38 **célula a célula**, pulando a segunda célula de um caractere largo e trocando
  os meio-blocos da gata por espaço, e o `Nick::tree_rows`/`selected_row` passaram a chamá-las.
  **Diferença da sugestão do review:** ele propunha cortar na borda direita (`take_while` até `│`/`┃`).
  Isso cortaria errado a linha com um diálogo aberto, cuja borda fica sobre as colunas da árvore. Por
  célula, uma linha sem caractere largo dá exatamente o texto de antes, e a suíte de PTY passou sem
  mudar nenhum teste. As cópias do `tui.rs` saíram;
- **P2, a volta ao nó sem janela curta:** o teste de PTY conferia a volta ao NS-1 num laço próprio,
  que exigia ler a primeira tela dentro dos cerca de 80 ms em que o letreiro `fast` fica no começo.
  Agora há o `Nick::wait_screen(what, cond)`, que devolve a mesma tela em que a condição valeu (o
  `keep_when` usa ele e também devolve a tela). O teste sai do NS-2 depois de ele andar pelo menos
  10 colunas, volta ao NS-1 e desce de novo, e confere que a primeira tela do NS-2 começa **antes**
  de onde ele saiu. Sem o reinício, ele seguiria dali ou adiante; com ele, só um atraso de 260 ms
  passaria das 10 colunas. Provado numa cópia (sabotagem do `tick` sem reiniciar: `at 11, left at
  10`; e o laço com `mascot_wake`), e rodado 10 vezes seguidas sem falha;
- **P3:** a linha do `//!` do `main.rs` que passava de 100 colunas foi quebrada.

Das observações do review, ficou como está (D6): voltar das Configurações para a árvore com o mesmo
nó não reinicia o letreiro, porque o nó não mudou.

## Rust no projeto: etapa Rejeitado (NS-61)

História filha do épico NS-28. Antes, um ticket que não ia ser feito só podia ir para Concluído.
Agora existe a etapa **Rejeitado** (`rejected`): ela é escolhida no `m`, a partir de qualquer etapa,
exige um motivo, que vai para o histórico, e bloqueia agentes enquanto o ticket estiver nela. O
plano está em `.nick/docs/NS-61/plan.md`. Ficaram fora do escopo:
- sincronizar com o Jira (rejeitar no nick não rejeita no Jira: impacto aceito no ticket);
- rejeitar épicos;
- editar o motivo depois;
- mudar o rodapé `d esconder concluídos`;
- cor por etapa no histórico e no diálogo "Mudar etapa".

**A etapa e "fechado".** `TicketStage::Rejected` (chave `rejected`, rótulo `Rejeitado`) é declarada
depois de `Done`, então o `ALL` tem sete etapas, segue ordenado pelo `Ord` e o diálogo mostra o
Rejeitado por último. O `agent_stage()` dá `None`, como Aberto e Concluído; o `agents::Stage` e o
`CHECK` de `agents.stage` não mudaram.

O conceito de **fechado** tem uma fonte única: `TicketStage::CLOSED = [Done, Rejected]` e
`is_closed()`. Quem usa:
- o filtro `d` (`tickets::is_closed(&Ticket)`, em `DoneIds` e no `push_ticket`);
- a conclusão e a reabertura do épico (`db::closed_keys()`, que vai por parâmetro no
  `stage NOT IN (?2, ?3)` do `complete_epic` e do `reopen_if_pending`);
- o avanço na abertura (`current.is_closed() || to <= current`).

Não há literal `"done"`/`"rejected"` fora do `tickets.rs` e das migrações. Para um `Ticket` há um só
ponto que lê a chave gravada: `tickets::is_rejected(&Ticket)` e `tickets::is_closed(&Ticket)`, usados
no `Holder::from`, no `ticket_spans`, na árvore e no `rejected_now`.

**Rejeitar (o `m`).** O fluxo é `ChooseTicketStage` → `Enter` no Rejeitado → `RejectReason` →
`ConfirmStageChange` → `s`.
- **Recusa com agente em outro nick:** se o ticket tem agente `active` em outro nick (um registro
  ativo sem sessão neste nick), a rejeição é recusada com `O ticket <ID> tem agente ativo em outro
  nick. Encerre os agentes dele lá antes de rejeitar o ticket.` (`App::rejection_refused`, no molde
  do `removal_refusal`). Agentes parados não contam, porque a retomada deles já é recusada. A recusa
  é checada ao escolher o Rejeitado, e o "Mudar etapa" fica aberto com ela na barra, e de novo no
  `s`, porque outro nick pode abrir um agente nesse meio-tempo (molde do `key_confirm_remove`).
- **O motivo (`Dialog::RejectReason { ticket, input }`):** campo de texto, título ` Rejeitar ticket `,
  as linhas `Ticket <ID> — <título>` e `Motivo da rejeição (até 120 caracteres):`, e rodapé `Enter
  confirmar · Esc cancelar`. Uma recusa fica na barra com o campo aberto (molde do `EditId`).
- **A confirmação (`ConfirmStageChange { ticket, to, reason, live }`):** `Rejeitar o ticket <ID> (de
  <etapa>)? Motivo: <motivo> (s/N)`, com rodapé `s rejeitar · qualquer outra tecla cancela`. Com
  agentes vivos neste nick, ela avisa antes:
  - com um: `Aviso: o agente <nome> será encerrado.`;
  - com N: `Aviso: <N> agentes vivos deste ticket serão encerrados: <nomes>.`.

  Os nomes entram no aviso de N para cumprir o "avisa quais" do plano. As outras etapas seguem o
  fluxo de antes, com `reason: None` e `live` vazio.
- **Cancelar:** `Esc` no motivo, ou qualquer tecla diferente de `s` na confirmação, cancela sem
  gravar nada.
- **Nenhum dos dois diálogos é irreversível** (`irreversible() == false`): o agente encerrado pode
  ser retomado depois que o ticket sair do Rejeitado.

**O motivo (D3).** Ele é aparado (`trim`), não pode ser vazio e vai até `REJECT_REASON_MAX = 120`
caracteres, contados com `chars().count()`, que dá o mesmo que o `length()` do SQLite para TEXT.
Ele é validado por `tickets::check_reject_reason`, no campo e de novo no `change_stage`. As
mensagens:
- `Informe o motivo da rejeição.`;
- `O motivo tem <N> caracteres; o limite é 120.`.

**Por que obrigatório:** para o histórico explicar a decisão. O passo a mais foi aceito. O motivo só
existe na mudança **para** Rejeitado, e as outras mudanças gravam `NULL`. Um motivo passado para
outra etapa é erro de programação e vira recusa (`Só a mudança para Rejeitado tem motivo.`), sem
`panic!`. Na tela, o motivo passa pelo `printable`.

**Um só ponto de gravação.** `tickets::change_stage(db, ticket, to, reason)` recusa a mesma etapa
primeiro, valida o motivo e chama `Db::change_ticket_stage(id, from, to, source, reason)`, que grava
a coluna `reason` no INSERT do histórico. A chamada no `app.rs` continua uma só, dentro da
confirmação. Os estruturais `the_stage_is_only_written_by_the_operator_or_by_opening_a_new_agent` e
`only_the_three_writers_of_the_db_write_a_ticket_and_they_reopen_the_epic` passaram sem mudança.

**Agentes vivos: grava, depois encerra (D6).** O molde é a remoção de projeto com agentes vivos
(NS-38), com a ordem invertida: a etapa é gravada **primeiro**, na confirmação. Só com sucesso, e se
o ticket tem agentes vivos neste nick, a confirmação devolve `Action::EndRejectedAgents { agents,
message }`. Então o laço do `main.rs` encerra as sessões (`agent_session::terminate_all`), e
`App::end_agents_of_rejected` encerra os registros e solta as travas. A barra mostra a mensagem da
etapa seguida de ` O agente <nome> foi encerrado.` ou ` Os <N> agentes vivos dele foram encerrados.`.

Com essa ordem, uma gravação que falha (`Moved`, `Gone` ou erro) não encerra nenhum agente. O sufixo
e o encerramento dos registros saíram de `remove_project_ending_agents` para o helper privado
`App::end_records`, usado pelos dois. A remoção de projeto não mudou de comportamento, e os testes
dela passaram sem mudança.

**Sem agente num ticket rejeitado (D7).** A mensagem é `tickets::REJECTED_AGENT_MESSAGE` = `ticket
rejeitado: mude a etapa com m antes de abrir um agente`. As crases do Jira eram só formatação, e o
nick cita teclas sem crase. O `agents::Holder` ganhou `rejected: bool`, que é verdadeiro no ticket
em Rejeitado e sempre `false` no épico (no molde do `hidden`). Onde ela recusa:
1. `choose_stage` (o `g`), depois do `hidden_refusal`;
2. o `n` do `key_conversation_lost`;
3. `ask_resume` (o `r`), antes do `--check`. Ele usa `App::rejected_now(owner)`, que **lê o ticket
   do banco** naquela hora. Isso **difere do oculto**, cuja retomada continua permitida (NS-39), por
   decisão do usuário;
4. `resume_agent` (o ticket do `holder_of_agent`, lido agora) e `open_agent` (o `rejected_now` logo
   no começo, antes do `agents::open`). É a defesa final caso outro nick rejeite o ticket no meio do
   fluxo `g` → tipo → formulário → prévia. Recusado, nada é gravado nem lançado, e nenhuma trava é
   tomada.

Um agente de **épico** abre normalmente com tickets rejeitados no épico. O avanço na abertura
(NS-45) nunca tira um ticket do Rejeitado.

**Sair do Rejeitado:** pelo `m`, para qualquer outra etapa, sem motivo. Não existe "reabrir" à parte.
Fora do Rejeitado, o `g` e o `r` voltam a funcionar.

**Migração 11** (no molde da 9, publicada, nunca editada). Ela reconstrói `tickets` e
`ticket_stage_changes`:
- `tickets`: `'rejected'` no `CHECK` da etapa; o `DEFAULT 'open'` continua;
- `ticket_stage_changes`:
  - `'rejected'` nos dois `CHECK`s;
  - a coluna `reason TEXT`, entre `source` e `changed_at`;
  - `CHECK ((to_stage = 'rejected') = (reason IS NOT NULL))`;
  - `CHECK (reason IS NULL OR (trim(reason) <> '' AND length(reason) <= 120))`;
  - as linhas antigas copiadas com `reason` `NULL`.

As três armadilhas da 9 foram repetidas:
- a FK fica desligada pelo laço do `open_with`, que não mudou;
- os seis gatilhos são derrubados e recriados com o texto idêntico ao da 9;
- os `id`s são preservados.

Antes de escrever a migração, o `sqlite_master` de um banco v10 confirmou que só os seis gatilhos e o
autoindex dependem de `tickets` (`.nick/docs/NS-61/sqlite_master_v10.txt`). A prova é o teste
`a_version_10_database_populated_migrates_to_11_keeping_every_row_and_link`. O banco dele é o
populado do v9 → v10 mais um épico concluído (`populated_v10`). O teste confere todas as linhas e
ligações, `foreign_key_check` e `integrity_check` vazios e os seis gatilhos.

**Limite conhecido do `CHECK` do motivo:** o `trim()` do SQLite tira só espaços, então ele não pega
um motivo só de tabs. Quem pega é o código (`check_reject_reason`, com teste). O `CHECK` ficou com o
texto do plano, como o do título.

**Cor e marca (D9).** `ui::STAGE_GRAY = Color::DarkGray` (índice 8, `Idx(8)` no vt100), e o
`stage_span` dá `[✗ Rejeitado]` sempre: na árvore e no `Etapa atual:` do painel. Na árvore,
**o rótulo inteiro** do ticket rejeitado (ID, marca, título, `[provisório]`/`[oculto]`) é cinza
(`ticket_spans`), e o DIM do oculto convive com o cinza. O cinza vale também no rótulo cortado com
`…` e na linha que rola no letreiro do NS-55 (testado).

**Histórico (D10).** Uma mudança para Rejeitado ganha a linha `    Motivo: <motivo>` logo abaixo da
linha de sempre (`  <data>  <de> → Rejeitado  (<origem>)`). Com 40 colunas, o corte leva o fim do
motivo, não a data nem as etapas. O corte `(+N mudanças mais antigas)` passou a contar **linhas** (1
ou 2 por mudança) e mantém as mudanças mais novas inteiras, para não estourar a área.

**Efeito no NS-54 (D8).** O `d` esconde também os rejeitados, em qualquer profundidade, e um agente
vivo não segura um ticket rejeitado na árvore. Um ticket rejeitado conta como fechado:
- um épico só com tickets concluídos e rejeitados pode ser concluído;
- um rejeitado oculto não é pendente;
- mover um ticket de Concluído para Rejeitado (ou o contrário) mantém o épico concluído;
- sair do Rejeitado para uma etapa aberta reabre o épico;
- mover um ticket rejeitado para um épico concluído não o reabre.

Não mudaram, de propósito (mudar o rodapé reabriria a medição de altura do NS-54):
- o rodapé `d esconder concluídos` / `d mostrar concluídos`;
- a mensagem `Ele sai da árvore (d mostra os concluídos).`;
- as pendências `... ticket não concluído`.

No filtro, "concluídos" inclui os rejeitados.

**Impactos conhecidos.**
- O Jira não fica sabendo da rejeição.
- Ao rejeitar, um agente vivo é encerrado sem pergunta além do aviso na confirmação, e a conversa
  dele fica para retomar depois que o ticket sair do Rejeitado.
- O diálogo "Mudar etapa" tem sete linhas e cabe na altura mínima já testada
  (`the_dialog_of_the_stages_fits_its_seven_rows_in_a_short_terminal`).

**Testes adaptados (nenhum apagado), com o motivo de cada um:**
- **Pela sétima etapa:**
  - `ticket_stage_has_the_seven_stages_in_the_order_of_the_flow` (era `six`);
  - `m_lists_the_seven_stages_and_only_s_changes_the_stage` (era `six`; ganhou o índice 6);
  - `the_colors_of_the_stages_and_of_the_epic` (+`STAGE_GRAY`).

  Outros dois percorriam todas as etapas e, sem mudar as asserções, ficaram nas seis de antes,
  porque o Rejeitado tem fluxo próprio:
  - `the_operator_moves_the_ticket_to_open_from_any_stage` (o Rejeitado pede motivo);
  - `each_stage_shows_in_its_color_on_the_tree_and_on_the_panel` (o cinza tem teste próprio).
- **Pelo diálogo novo:**
  - `only_the_eight_dialogs_with_a_text_field_take_text` (era `seven`; com o irreversível
    `only_the_removal_of_a_project_is_irreversible`, as variantes passaram de 24 para 25);
  - `one_of_each_dialog` (ganhou `RejectReason`);
  - `capital_letters_are_typed_as_they_are_in_the_text_fields` (ganhou o motivo);
  - o literal de `ConfirmStageChange` no `ui.rs` (`reason`/`live`).
- **Pela migração 11:**
  - `a_new_database_gets_version_11_…` (era `10`);
  - `opening_twice`;
  - os de banco mais novo (12 / `(11)`);
  - `migration_1_is_pinned` (11 migrações e o `PINNED_MIGRATION_11`);
  - o v9 → v10, que agora abre só até `MIGRATIONS[..10]`;
  - `a_real_version_6_…` (`tests/agent.rs`, versão 11).
- **Só sintaxe (o `None` novo do `reason`):** as chamadas de `change_stage` e `change_ticket_stage`
  nos testes, no `epics.rs` e em `tests/tui.rs`. O helper `sourced` do `ui.rs` ganhou
  `reason: None`.

**Testes novos.**
- **`tickets.rs`:**
  - `only_done_and_rejected_are_closed`;
  - `check_reject_reason_trims_and_refuses_empty_and_long`;
  - `rejecting_from_each_stage_stores_the_trimmed_reason`;
  - `an_invalid_reason_is_refused_and_nothing_is_stored`;
  - `leaving_rejected_to_each_stage_stores_no_reason`;
  - `rejected_again_is_the_same_stage_and_gone_or_moved_store_nothing`;
  - `opening_an_agent_never_takes_a_ticket_out_of_rejected`;
  - `rejecting_in_a_done_epic_keeps_it_done_and_leaving_rejected_reopens_it`.
- **`db/mod.rs`:**
  - o v10 → v11 populado;
  - `the_rejected_stage_and_the_reason_are_checked`;
  - `change_ticket_stage_stores_the_reason_of_a_rejection_and_none_otherwise`;
  - `a_new_ticket_is_still_born_open_after_the_migration_11`;
  - os quatro do épico (concluir com rejeitados, rejeitado oculto não pendente, entre Concluído e
    Rejeitado, mover rejeitado para épico concluído).
- **`tree.rs`:**
  - `hide_done_takes_the_rejected_tickets_at_depth_2_and_3_too`;
  - `a_live_agent_does_not_hold_a_rejected_ticket_and_the_v_shows_it`.
- **`app.rs`:**
  - o `Holder`;
  - as recusas do `g`, do `r`, do `n` e da retomada e da abertura com o ticket rejeitado no meio do
    fluxo;
  - sair do Rejeitado e abrir de novo;
  - agente de épico;
  - o fluxo motivo → confirmação;
  - a recusa com agente em outro nick (na escolha e no `s`);
  - o encerramento de um e de dois vivos;
  - a gravação que falha não encerra nenhum;
  - outra etapa com vivo não encerra nada.
- **`ui.rs`:**
  - a marca e a cor;
  - o rótulo cinza e o DIM;
  - árvore e painel;
  - o cinza no `…` e no letreiro;
  - o histórico com o motivo e o corte por linhas;
  - os diálogos da rejeição;
  - a altura do "Mudar etapa".
- **PTY:**
  - `a_ticket_is_rejected_with_a_reason_and_leaves_rejected_by_m` (`tests/tui.rs`: o cinza `Idx(8)`
    no rótulo, `Etapa atual: [✗ Rejeitado]`, `Motivo:`, a recusa do `g`, o `d` e a saída pelo `m`);
  - `rejecting_a_ticket_with_a_live_agent_warns_and_ends_it` (`tests/agent.rs`).

**Sabotagens** (numa cópia do repositório, `.nick/docs/NS-61/sabotagens.md`): foram 22, e 20 deram
teste vermelho na primeira rodada. Os dois casos restantes:
- **A S18** (o laço não termina as sessões) passou. O PTY da rejeição usava o claude falso em
  `echo`, que não grava pids, e conferia só o registro, a trava e a barra, que o
  `end_agents_of_rejected` faz sozinho. Ele passou para o modo `sleep` e agora espera cada pid do
  grupo morrer (`wait_dead`), como o `removing_a_project_ends_its_live_agent`. Com isso, a S18
  ficou vermelha.
- **A S2** (o avanço sem o `is_closed()`) é um **mutante equivalente**. O Rejeitado é a última
  etapa e não tem etapa de agente, então o `to <= current` já recusa o avanço a partir dele. O
  `is_closed()` fica como a regra explícita do D2, que protege se a ordem das etapas mudar.

As telas estão em `.nick/docs/NS-61/telas.txt` e `telas_ansi/`.

**Ajustes do review do NS-61** (`.nick/docs/NS-61/review.md`). Os sete pontos procediam, e nenhum
mudou comportamento; os testes passaram sem mudança, com uma exceção: o P6 renomeou uma variável
dentro de um teste.
- **P1, bloco repetido no laço:** os braços `RemoveProject` e `EndRejectedAgents` do `main.rs`
  repetiam o mesmo bloco, que tira as sessões de `sessions`/`sizes` e chama o `terminate_all`. O
  bloco virou o método `Shell::terminate_sessions(&[i64])`, usado pelos dois braços.
- **P2, predicado repetido:** "ativo em outro nick" (registro `active` sem sessão neste nick) virou
  `App::active_elsewhere(&[Agent])`, usado pelo `removal_refusal` e pelo `rejection_refused`. Cada um
  continua montando a sua mensagem.
- **P3, linhas do diálogo do motivo:** o `RejectReason` era montado direto no `draw_dialog`. Agora um
  helper comum, `text_field_lines(header, label, typed)`, monta o cabeçalho, a linha vazia, o rótulo,
  o texto digitado e o cursor em (3, n). O `title_lines`, o `id_lines` e o novo `reason_lines`
  delegam a ele, e os textos ficaram iguais byte a byte.
- **P4, "rejeitado?" escrito de três jeitos:** veja o ponto único acima. O `rejected_now` não monta
  mais um `Holder` só para ler o campo, e o `is_closed` privado da árvore saiu.
- **P5, comentários com mais de 100 colunas:** os parágrafos foram refluídos sem mudar o texto, com
  uma exceção. A documentação do `Db::change_ticket_stage` dizia "um ticket que sai de `done`
  reabre o épico", o que estava errado desde o NS-61 e foi corrigido para "sai de uma etapa
  fechada para uma aberta".
- **P6:** a variável `_launcher` do teste `a_failing_write_of_the_rejection_ends_no_agent` era usada
  e virou `launcher`.
- **P7:** `rejection_lines` recebe `Option<&str>` em vez de `&Option<String>`.

## Rust no projeto: liberar e prender a jail de um agente (NS-57)

História filha do épico NS-28. Até aqui a jail era decidida por projeto (`JAIL_ENABLED` no
`.nick/nick.conf`) e valia para todos os agentes. A tecla `l`, no nó de um agente da árvore,
troca a jail **só desse agente** e **só até ele fechar**, sempre depois de uma confirmação
`(s/N)` de borda vermelha. Liberar e prender de novo é **fechar e retomar na mesma conversa**: a
composição do fim pelo operador (`x`) com a retomada (`r`), mais uma opção do launcher que só
desliga a jail.

**Decisões, cada uma com o porquê:**
- **Só por agente e só até fechar; nada é gravado** (D10). O override mora no `RunningAgent`
  (memória) e no argv daquela retomada: nenhuma migração, coluna ou escrita no `.nick/nick.conf`.
  Quando o agente fecha (fim do `claude`, `x`, saída do nick), o próximo `r` segue o `nick.conf`,
  e os outros agentes do projeto nunca são tocados. Liberar a jail é exceção pontual do operador,
  não configuração: gravar faria um agente esquecido continuar sem proteção em aberturas futuras.
- **Sempre com confirmação vermelha** (D4/D5). `Dialog::ConfirmJailToggle { id, name, release }`
  diz a ação (liberar ou prender), que o agente reinicia e, ao liberar, que ele fica **SEM a
  proteção da jail até fechar**. A borda vermelha vem de `Dialog::touches_jail`, uma regra ao lado
  do `Dialog::irreversible`, e não dele: trocar a jail tem volta, e o `irreversible` (e o teste
  `only_the_removal_of_a_project_is_irreversible`) continua só da remoção. O vermelho existe para o
  operador nunca confirmar uma liberação achando que é uma confirmação comum.
- **Sem "ligar à revelia"** (D3). Não existe `--jail`: prender de novo é retomar **sem** o
  `--no-jail`, e aí vale o `nick.conf`. Num projeto sem jail, o `l` não existe (o Rust nem manda o
  `--no-jail`, que seria inócuo).
- **Sem fallback para jail quebrada** (D3, confirmado pelo usuário no planejamento). O
  `--no-jail` roda o `load_and_check` igual, com o `jail_check` do `JAIL_ENABLED` do projeto, e só
  **depois** faz `JAIL_ENABLED="false"` (com `if`, não `[ ] &&`, por causa do `set -e` no fim da
  função). Com a jail ligada e quebrada (ai-jail ausente, versão fora da faixa, sandbox que não
  abre), o `--no-jail` falha junto, com a mensagem do `jail_check`: o `l` não é saída de emergência.
  A troca é pela variável, e não por outro primeiro argumento do `jail_exec`, porque o estrutural
  `claude_command_lines` só aceita `jail_exec "$JAIL_ENABLED" ...`. Com a variável desligada, o
  `jail_print_status` diz "desligada", o `exec_claude` não põe o `jail_agent_notice` e o
  `jail_exec` roda o `claude` direto.
- **`--no-jail` só no `--resume`** (D2). Toda troca passa pela retomada, então o modo de lançamento
  (`--session-id`) não muda e o recusa como argumento desconhecido. É uma flag sem valor, aceita em
  qualquer posição, recusada quando repetida (o `seen`); o `launch_kind` não precisou mudar (ela não
  está entre as opções que pulam um valor), e um `--name --no-jail` continua sendo um nome.

**Como a tecla sabe se o projeto tem jail** (D1, decisão do usuário no planejamento):
- **Agente vivo:** o `RunningAgent` ganhou o `project_jail` (o `jail=` do `--check` daquela
  abertura), e o `jail` passou a ser a jail **efetiva** (`project_jail && !liberado`). O cabeçalho,
  a árvore e o letreiro já liam o `jail`, então mostram o estado novo sem mudança.
- **Agente parado:** o `Row` ganhou o `jail: Option<bool>`, gravado em **todo** `--check`
  bem-sucedido do projeto (`check_agent`, o `g`, e `check_resume`, o `r` e a troca de jail), no
  molde do `row.jira_url` do NS-53, e mantido pelo `refresh` nas mesmas condições (mesmo id, mesmo
  caminho, disponível antes e agora). Projeto indisponível: `None`. Não há leitor ao entrar na
  lista: o `--jira-url` não mudou (contrato de uma linha do NS-53).
- **Consequência aceita:** com o `Row.jail` desconhecido (`None`), o agente parado **não mostra**
  `l` na ajuda e o `l` **não faz nada** — a ajuda e a tecla sempre concordam (as duas leem o
  `App::jail_toggle`). Isso acontece, por exemplo, logo depois de abrir o nick, antes de qualquer
  `g`/`r` naquele projeto. Nesse caso, o operador retoma com `r` e depois usa `l`. Não existe tecla
  escondida (confirmado pelo usuário). O parado sempre abre "liberar": a troca acabou quando ele
  fechou, e ele segue o `nick.conf`, que tem a jail ligada.

**O fluxo** (D6, no `key_node`): vivo com `project_jail` → `ConfirmJailToggle { release: jail }`;
parado com `Row.jail == Some(true)` → `release: true`; qualquer outro caso (projeto sem jail, jail
desconhecida, arquivado, ativo em outro nick) → nada, sem status. Ao confirmar: o vivo vira
`Action::ToggleJail { id, release }`, e o `main.rs` faz o mesmo término do `x`
(`Shell::terminate_session`, extraído do `Action::EndAgent`), chama o
`App::agent_stopped_for_jail(id, release)` (o `take_agent` e o `agents::end` do
`agent_ended_by_operator`, sem a mensagem de "foi encerrado") e executa o `CheckResume` que ele
devolve pelo mesmo `Shell::check_resume` do ramo `CheckResume`. O parado vai direto ao
`ask_resume_with(id, ResumeJail::Off)`. Todas as recusas do `r` (arquivado, outro nick, projeto
indisponível, ticket rejeitado, linha sumida) continuam valendo, e a conversa perdida abre o
`ConversationLost` de sempre. Cancelar não muda nada: a sessão continua, sem novo lançamento.

**O `s` confere de novo a regra da tecla** (achado P1 do review). Com a caixa aberta, o agente
pode mudar: o `claude` termina sozinho (o `Exited` é tratado em qualquer estado, e o
`agent_ended` não fecha diálogos), ou um `--check` muda o `Row.jail`. A primeira versão decidia
pelo estado no `s` e mandava todo parado ao `ResumeJail::Off`: um "prender" confirmado num agente
liberado que tinha acabado de terminar virava uma retomada **fora** da jail. Agora o `s` só age
quando o `App::jail_toggle(id)` ainda devolve o mesmo `Some(release)` da caixa. Senão, não faz
nada e a barra diz "O estado do agente <nome> mudou enquanto a confirmação estava aberta. Nada foi
feito; aperte l de novo.". Assim a caixa, a tecla e a ajuda sempre concordam, e prender nunca vira
liberar. Um "liberar" aberto num vivo que termina continua valendo: o parado de um projeto com a
jail ligada também é liberado pelo `l`.

**A mensagem do "prender" segue a jail desta retomada** (achado P2 do review). Se o `nick.conf`
desligou a jail entre a liberação e o "prender", o `--check` da retomada diz `jail=false`, e o
agente volta sem jail. A barra diz então "foi retomado fora da jail: a jail do projeto está
desligada agora.", e não "foi retomado na jail.".

**O término do registro num helper só** (achado P3 do review, o que o plano pedia):
`App::end_running(id)` faz o `take_agent` e o `agents::end` e devolve o nome, o aviso do check e
o resultado. O `agent_ended`, o `agent_ended_by_operator` e o `agent_stopped_for_jail` montam cada
um a sua mensagem a partir dele, e o comportamento do fim do `claude` e do `x` não mudou.

**`ResumeJail` no lugar do `jail_off: bool` do plano** (decisão da implementação). O plano
previa `ask_resume_with(id, jail_off: bool)` e `Action::CheckResume { jail_off }`, mas a barra de
status precisa de três mensagens (D7): a de hoje no `r` (`foi retomado.`), `foi retomado fora da
jail.` e `foi retomado na jail.` (prender de novo). Um `bool` não distingue o `r` do "prender",
que têm o mesmo argv. Daí `ResumeJail { AsConfig, Off, BackIn }`: só o `Off` põe o `--no-jail`
(`Launch::Resume { jail_off }`, que continua `bool`) e desliga a jail efetiva no `start_running`;
os três escolhem a mensagem. O `r` é o `ask_resume_with(id, ResumeJail::AsConfig)`, sem mudar o
comportamento.

**Geração das sessões** (D8, risco técnico). O `AgentEvent` só levava o `agent: i64`. Na troca de
jail, a sessão antiga é terminada (`terminate`, síncrono, até `TERM_GRACE`: o waiter dela já pôs o
`Exited` no canal quando ele volta) e a nova, **com o mesmo id**, entra no laço logo em seguida.
Sem correção, o `Exited` (ou um `Output`/`Clipboard`) atrasado da antiga seria atribuído à nova: o
`agent_exited` encerraria o agente recém-retomado (e o `wait()` dele travaria o laço). Agora cada
`AgentSession` tem uma geração única (um `u64` de um `AtomicU64` global, nunca reutilizado),
carregada em todo `AgentEvent`, e o `agent_session::event_session(sessions, event)` só acha a
sessão cujo id **e** geração batem. O laço descarta, num ramo antes dos outros, todo evento sem
sessão atual. O mesmo risco existia em teoria no `x` seguido de `r` muito rápido; a correção cobre
os dois casos, e o comportamento visível do `x`/`r` não mudou.

**Cabeçalho de altura fixa quanto à jail** (D11, decisão do usuário): ver a nota "Mudou no NS-57"
na seção do NS-34. O motivo: o aviso fora da jail é bem mais longo que `na jail`; com a altura
dependendo da jail, o mesmo agente mudaria de tamanho ao ser liberado (e o spawn teria de seguir a
jail efetiva). A linha fica reservada e pode servir a mais informação no futuro. **O custo, aceito
pelo usuário:** um agente **na jail** perde para o cabeçalho as linhas que a versão fora da jail
ocuparia (num painel de 58 colunas, uma linha).

**Ajuda** (D9): `Place::LiveAgent` (projeto sem jail, a ajuda de antes), `LiveAgentInJail`
(`... x encerrar · l liberar jail · {atalho} alternar`), `LiveAgentReleased` (`l prender na jail`),
`StoppedAgent` (a de antes) e `StoppedAgentInJail` (`↑/↓ mover · r retomar · o arquivar · l
liberar jail · ← subir`), todos pelo `App::jail_toggle`. O `HELP_STATES` foi de 65 para 89 estados
(11 lugares da árvore × 8 combinações de filtros, mais as Configurações). Os textos novos são mais
curtos que a ajuda mais longa (a do épico), então a reserva de linhas do rodapé e o tamanho dos
terminais não mudaram (teste `the_longest_help_is_still_the_one_of_before`, de 20 a 200 colunas).

**Testes novos:**
- bash (`bin/tests/test_nick_agent.sh`, seção "--resume com --no-jail"): jail ligada + `--no-jail`
  (rc 0, sem `jail.argv`, `order` = `ai-jail -- true` + `claude`, argv sem o aviso da jail,
  `desligada`); o mesmo com o ID mudado; jail desligada + `--no-jail` (o mesmo argv que sem a flag);
  o `--no-jail` antes, no meio e no fim das opções; `--name --no-jail` é um nome; recusas (repetido,
  com valor, no lançamento, no `--check` e no `--jira-url`); jail quebrada + `--no-jail` (três
  modos de falha do `jail_fail`, modo novo `resume_no_jail`); o uso cita a flag; estruturais (o
  `--no-jail` só no `resume_mode`/`launch_kind`/`usage_error`, com controle positivo; o
  `JAIL_ENABLED="false"` uma vez, no `resume_mode`, depois do `load_and_check`). O helper `resume`
  aceita argumentos extras depois de um `--`, e as chamadas antigas ficaram iguais.
- `agent_session`: `a_late_event_of_an_ended_session_is_not_the_new_one_s_with_the_same_id`
  (falhou antes da correção, por asserção) e `a_resume_out_of_the_jail_passes_no_jail_to_the_launcher`
  (PTY real, projeto com a jail ligada pelo ai-jail falso: com o `jail_off`, a jail desligada; sem
  ele, o controle pelo ai-jail).
- `launch`: `the_args_of_a_resume_out_of_the_jail_end_with_no_jail`.
- `app`: `only_the_jail_toggle_touches_the_jail`; o cache do `Row.jail`
  (`every_check_of_g_keeps_the_jail_of_the_project`,
  `the_check_of_a_resume_keeps_the_jail_of_the_project`,
  `the_refresh_keeps_the_jail_while_the_project_stays_available`); o `start_running` com e sem o
  override (`a_resume_out_of_the_jail_runs_released_and_says_so`,
  `an_opened_agent_runs_with_the_jail_of_its_check`); cada linha da tabela do D6 (`l_on_...`), o
  cancelamento, o `agent_stopped_for_jail`, a troca só desse agente e a conversa perdida; do
  review, `s_never_releases_an_agent_the_box_offered_to_jail_again` e
  `s_rechecks_the_jail_of_a_stopped_agent` (P1), e um caso do `BackIn` num projeto sem jail no
  `a_resume_out_of_the_jail_runs_released_and_says_so` (P2).
- `ui`: `the_size_of_the_terminal_does_not_depend_on_the_jail`,
  `the_header_in_the_jail_leaves_its_extra_rows_blank`,
  `the_jail_toggle_dialogs_are_red_and_say_what_happens`, `the_help_has_l_only_where_it_works`,
  `the_longest_help_is_still_the_one_of_before`.
- PTY (`nick/tests/agent.rs`): `l_releases_a_live_agent_and_jails_it_again_in_the_same_conversation`
  (o `┏` vermelho, retomado na mesma sessão, `FORA DA JAIL` em vermelho, o mesmo `SIZE` antes e
  depois, prender de novo pelo ai-jail e, depois de `x` + `r`, de volta na jail),
  `releasing_one_agent_leaves_the_other_of_the_project_in_the_jail`,
  `without_the_jail_in_the_project_there_is_no_l`,
  `cancelling_the_release_keeps_the_agent_running_in_the_same_process` e
  `a_stopped_agent_of_a_project_in_the_jail_is_resumed_out_of_it_by_l`.

**Testes existentes adaptados, com o motivo:**
- pelo D11 (cabeçalho de altura fixa), todos com o agente **na jail** do `with_running(&["proj"])`
  (`precheck(true)`), num painel de 58 colunas, em que o aviso fora da jail quebra em 2 linhas:
  - `ui::with_an_agent_the_list_is_on_the_left_and_the_agent_on_the_right`: cabeçalho
    `(41, 1, 58, 1)` → `(41, 1, 58, 2)`, terminal `(41, 2, 58, 23)` → `(41, 3, 58, 22)` (e o parser
    de teste de 23 para 22 linhas, `primeira` na linha 3);
  - `ui::the_warning_out_of_the_jail_wraps_in_the_header_and_the_terminal_shrinks`: o agente na
    jail `(41, 2, 58, 23)` → `(41, 3, 58, 22)`, o mesmo do agente fora dela (os números do agente
    fora da jail não mudaram);
  - `ui::a_long_status_takes_rows_from_the_panel_too`: 1 → 2 linhas de cabeçalho na conta;
  - `ui::the_cursor_of_the_program_is_inside_the_border`: o cursor `(41, 2)` → `(41, 3)`;
  - `ui::the_mouse_over_the_border_goes_nowhere`: a primeira célula do programa `(41, 2)` →
    `(41, 3)`, e a linha 2 (agora do cabeçalho) entrou nas que não vão a lugar nenhum;
  - `ui::the_terminal_is_inside_the_border_of_the_panel`: `(41, 2, 58, 24)` → `(41, 3, 58, 23)`.
  Nenhum teste com a jail desligada mudou de número: os de tamanho no PTY (`SIZE 22 58`/`SIZE 33
  78` de `a_resize_reaches_claude_below_the_header` e os do NS-34/NS-38, todos com
  `JAIL_ENABLED="false"`) passaram sem edição.
- pelo D9 (ajuda): `ui::the_help_of_each_node_with_the_filters_and_the_count` (o agente do
  `with_running(&["proj"])` está na jail, e a ajuda ganhou `l liberar jail`) e o total do
  `HELP_STATES` (65 → 89) em `help_states_has_every_combination_of_the_three_filters` e
  `the_project_help_has_n_and_the_epic_place_is_in_the_help_states`; o
  `every_help_state_is_in_help_states` listou os 3 lugares novos (o `match` exaustivo dele cobra).
- pelo D5 (variante nova): o total do catálogo `one_of_each_dialog` de 25 para 26 em
  `only_the_eight_dialogs_with_a_text_field_take_text` e
  `only_the_removal_of_a_project_is_irreversible` (o plano falava em 24 → 25, contado antes do
  NS-61, que entrou antes); o `color_of` dos testes do `ui` passou a dar vermelho também com o
  `touches_jail` (o `every_dialog_has_the_focus_border_and_only_it` passa com as 26). O
  `the_removal_dialog_is_red_and_the_others_cyan` já pulava as variantes fora das duas que cobra.
- o `USAGE` de `bin/tests/test_nick_agent.sh` ganhou o ` [--no-jail]` da retomada (o `Uso:` do
  launcher cita a flag).
- só a sintaxe da chamada (o mesmo comportamento, as mesmas asserções): o `jail_off: false` nos
  `Launch::Resume` dos testes de `agent_session.rs` e `launch.rs`; o `ResumeJail::AsConfig` nos
  `check_resume`/`CheckResume` dos testes do `app.rs`; o `terminal_in_panel`/`header_rows` sem o
  `jail` nos testes do `ui`; o `jail: None` nos `Row` literais de `tree.rs` e `ui.rs`; o `..` nos
  padrões de `AgentEvent` do `main.rs` e do `FakeLauncher`.

Passaram sem edição: os testes de `--resume` do bash (inclusive o diferencial "retomada =
lançamento até o `--`" e o estrutural `claude_command_lines`), os de `x`, de `r`, da conversa
perdida, do `ConfirmRemove` vermelho, do cabeçalho (`the_agent_header_in_the_jail`,
`the_agent_header_out_of_the_jail_is_red_and_bold`), do NS-53 (`jira_url`) e todos os de PTY de
antes.

Sabotagens numa cópia fora do repositório em `.nick/docs/NS-57/sabotagens.md`; telas (texto e
ANSI) em `.nick/docs/NS-57/telas/`. Nenhuma dependência nova. O README não mudou: ele não lista as
teclas da árvore (regra "README genérico").

**Fora de escopo:** gravar a escolha no banco, trocar a jail do projeto pela TUI, liberar hosts ou
caminhos isolados (`JAIL_ALLOW_HOSTS`), ligar a jail à revelia do `nick.conf`, um aviso novo ao
agente de que a jail mudou (o ticket aceita o risco do histórico antigo, impacto "Contexto do
agente") e mudar o `--jira-url`/`--check`.
