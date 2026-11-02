# nick_secrets

Ferramentas pessoais de automação do dia a dia.

## Módulos

### auto_scrum

Script bash que automatiza a abertura de conversas no Claude Code para as 4 etapas de
um fluxo de trabalho: **PO** (avaliar/criar ticket), **Tech Leader** (criar plano),
**Desenvolvimento** e **Review**. Em vez de copiar/colar manualmente um prompt de
template e preencher os dados na mão toda vez, o script pergunta os dados daquela
etapa, monta o prompt final e já abre o `claude` com ele.

#### Requisitos

- [`claude`](https://claude.ai/code) no PATH.
- `envsubst` (pacote `gettext` — geralmente já vem instalado; em Arch: `sudo pacman -S gettext`).
- Um editor de texto no `$EDITOR` (fallback: `nano`).
- Opcional, só para rodar o agente dentro do sandbox (`JAIL_ENABLED="true"`): ai-jail e, no
  Linux, bubblewrap — veja a seção [Sandbox (ai-jail)](#sandbox-ai-jail).

#### Uso

```bash
./auto_scrum/auto_scrum.sh [--projeto=<nome>]
```

1. Configuração do projeto (stack e flags): vem do `.nick/nick.conf` do diretório atual, se
   existir (o script mostra de onde carregou); senão o script pergunta qual projeto usar.
   Veja "Stack por projeto" abaixo.
2. Escolha o tipo de conversa no menu (PO, Tech Leader, Desenvolvimento ou Review).
3. Responda o roteiro de perguntas daquela etapa. Campos de descrição longa abrem
   `$EDITOR` num arquivo temporário (salve e feche para continuar) — colar texto grande
   direto no terminal buga a exibição, por isso a descrição sempre passa pelo editor.
4. O prompt final é mostrado na tela e salvo em `auto_scrum/logs/<tipo>_<timestamp>.md`,
   seguido de uma linha que diz se a jail está ativa ou desligada.
5. Confirme (`s`/`N`) para abrir o `claude` com esse prompt em uma sessão nova, ou
   cancele — o prompt gerado continua salvo no log.

Toda sessão do `claude` aberta pelo nick recebe um conjunto fixo de regras de permissão
(ações bloqueadas e ações que pedem confirmação), sempre ativas, com ou sem jail. Elas ficam
em `lib/claude/permissions.json`; sem esse arquivo, o script para antes de abrir o `claude`.
Um projeto que precise de uma ação bloqueada roda o `claude` fora do nick.

O `auto_scrum` lê `JAIL_ENABLED` da configuração do projeto: com `"true"`, o `claude` de
todas as etapas roda dentro do sandbox ai-jail, e o script para logo depois de carregar a
configuração (antes de qualquer pergunta) se a jail não funcionar nesta máquina. Veja a
seção [Sandbox (ai-jail)](#sandbox-ai-jail).

Todo menu (inclusive o de destino do `--init`) termina com a opção **Sair (q)**: o número
dela, ou `q`/`Q`, encerra o script sem fazer nada — nenhum log gravado, nenhuma
configuração criada e o `claude` não é aberto.

Cada etapa roda de forma independente (normalmente em terminais separados); não há
encadeamento automático entre elas — por exemplo, depois do Review, quem decide se volta
para o Desenvolvimento é o humano, lendo o resultado.

`auto_scrum/logs/*.md` é gitignorado (é um registro de trabalho pessoal, não artefato
do projeto). Os templates ficam em `auto_scrum/templates/` e podem ser editados
livremente sem tocar no script.

#### Stack por projeto

Os prompts e o menu usam a configuração do projeto: a stack técnica (ex: "Django, DRF,
PostgreSQL" ou "Rust, Actix") e flags opt-in que ligam ou desligam partes dos prompts e do
menu. Essa configuração pode ficar em dois lugares, procurados nesta ordem:

1. **`.nick/nick.conf` no diretório atual** (o projeto alvo, de onde o script é
   rodado). Se existir, é usado e o script imprime
   `Configuração do projeto carregada de: <caminho>` — sem menu de projetos. Se
   `--projeto=` também for passado, ele é ignorado com um aviso (mesmo que o nome não
   exista em `auto_scrum/projects/`). Só o diretório atual é olhado, não os diretórios
   pais. Um `.nick.conf` antigo na raiz (sem o `.nick/nick.conf`) faz o script parar com o
   comando de migração — veja "Migração (`.nick/`)" abaixo.
2. **`auto_scrum/projects/<nome>.conf`**, escolhido por `--projeto=<nome>` (erro claro se
   o arquivo não existir) ou, sem argumento, pelo menu com os projetos encontrados + a
   opção "Nenhum" (usa uma stack genérica).

O formato de `.nick/nick.conf` e de `auto_scrum/projects/<nome>.conf` é o mesmo. O exemplo
completo é o arquivo gerado por `--init`: ele traz todas as chaves aceitas, com um
comentário explicando cada uma. Uma linha preenchida fica assim:
`STACK_DESCRIPTION="Django, DRF, PostgreSQL"`.

Regra de formato: o arquivo **não é executado** — o script lê só linhas `CHAVE="valor"`
das chaves aceitas, com aspas duplas, sem aspas duplas dentro do valor e sem comentário no
fim da linha (comentário em linha própria, começando com `#`, pode). O valor é usado
literalmente (`$VAR`/`$(...)` não são expandidos). Qualquer outra linha é ignorada; uma
linha de chave aceita fora do formato (ex: `CHAVE=valor` sem aspas, `export CHAVE="valor"`
ou `CHAVE = "valor"`) é ignorada com um aviso mostrando o arquivo e o número da linha.

Duas regras valem além do formato:

- **Toda chave aceita precisa estar no arquivo**, mesmo as que o projeto não usa. Se faltar
  alguma, o script avisa no stderr, com o caminho do arquivo e as linhas a adicionar, e a
  chave ausente vale como vazia. Uma linha comentada ou com outro nome não conta.
- **As chaves `*_ENABLED` aceitam só `"true"`, `"false"` e `""`.** Qualquer outro valor (ex:
  `"TRUE"`, `"sim"`) mantém a chave no valor padrão dela (o comentário de cada chave no
  arquivo gerado por `--init` diz qual é) e gera um aviso com o arquivo e o número da
  linha.

Os avisos não interrompem a execução. Quando uma chave nova é criada, as configurações que
já existem passam a receber o aviso de chaves ausentes, com a linha a adicionar.

- Pra criar a configuração: `./auto_scrum/auto_scrum.sh --init` — pergunta o destino
  (`.nick/nick.conf` no diretório atual, ou `auto_scrum/projects/<nome>.conf`, que pede o
  nome) e cria o arquivo com as chaves aceitas no valor padrão e comentários explicando cada
  uma. O conteúdo é o mesmo nos dois destinos. Abre e preenche depois. Se o arquivo já
  existir, pergunta antes de sobrescrever. No destino do diretório atual, cria também o resto
  da pasta `.nick/` (abaixo) e mostra a estrutura na saída.
- `./auto_scrum/auto_scrum.sh --requisitos-jail` mostra o que a jail precisa nesta máquina
  (ver [Sandbox (ai-jail)](#sandbox-ai-jail)) e sai, sem carregar configuração.
- Só `--projeto=` (singular), `--init` e `--requisitos-jail` são reconhecidos — qualquer
  outra flag (ex: `--projetos=` com "s") dá erro claro em vez de ser ignorada
  silenciosamente.

#### Pasta `.nick/` do projeto alvo

Tudo o que o nick guarda no projeto alvo fica numa pasta só, na raiz dele:

```
.nick/
├── .gitignore    # versionado: ignora tudo em .nick/ menos ele e o nick.conf
├── nick.conf     # versionado: configuração do projeto
├── troca/        # não versionada: troca de arquivos com os agentes
└── docs/         # não versionada: plano, review e prints de cada ticket
    └── <ID>/     # ex: docs/SC-123/plan.md e docs/SC-123/review.md
```

- **O que é versionado:** só `.nick/nick.conf` e `.nick/.gitignore`. O `.gitignore` tem três
  linhas (`*`, `!.gitignore` e `!nick.conf`), então o `.gitignore` do projeto não precisa ser
  alterado. O nick cria `.gitignore`, `troca/` e `docs/` no `--init` (destino no diretório
  atual) e, se faltar algum, ao confirmar o envio do prompt; cancelar o envio não cria nada.
  Um `.nick/.gitignore` que já exista, editado ou não, nunca é sobrescrito.
- **Troca de arquivos (`.nick/troca/`):** pra mostrar um arquivo ao agente (uma print, um
  anexo) ou receber o que ele gera pra você conferir (ex: um relatório). O prompt de toda
  etapa termina dizendo isso ao agente. Ela fica dentro do projeto porque, com a jail ligada,
  o agente não enxerga nada fora dele (ex: `~/Downloads`).
- **Docs de cada ticket (`.nick/docs/<ID>/`):** o Tech Leader grava o plano em `plan.md`, o
  Review grava o resultado em `review.md`, e Desenvolvimento e Review salvam ali os prints de
  validação. O `<ID>` é o ID do Jira digitado (ele não pode ter `/`, `..`, espaço nem
  caractere de controle, porque vira o nome da pasta). No Desenvolvimento e no Review, o
  plano padrão é `.nick/docs/<ID>/plan.md`.
- **Pra versionar os docs** (vale pro time inteiro, porque o `.nick/.gitignore` é
  versionado), acrescente ao `.nick/.gitignore`:

  ```
  !docs/
  !docs/**
  ```

  `!docs/` reabre a pasta e `!docs/**` reabre o que está dentro dela, inclusive as subpastas
  `<ID>/`, que o `*` voltaria a ignorar. `troca/` continua ignorada.

`.nick/nick.conf` é feito pra ser versionado **no repositório do projeto alvo**, junto com o
código que ele descreve, e compartilhado com o time. Como o conteúdo dele entra no prompt
enviado ao `claude`, revise mudanças nesse arquivo no PR como qualquer outro arquivo que
influencia o agente (ex: `CLAUDE.md`) — não ser executado não o torna inofensivo. Já
`auto_scrum/projects/` é gitignorado — é configuração local, específica desta máquina, não
faz parte deste repositório (`.nick/` inteira também é gitignorada aqui, pra nunca ser
commitada neste repo por engano). O mesmo cuidado vale pro `JAIL_ENABLED` do `.nick/nick.conf` e pro
`.ai-jail` do projeto alvo: desligar a jail num PR tira a jail de todo o time, e o `.ai-jail`
pode ampliar o acesso do agente quando o projeto é confiado (ver
[Sandbox (ai-jail)](#sandbox-ai-jail)).

**Migração (`.nick/`):** a configuração do projeto saiu de `.nick.conf` pra
`.nick/nick.conf`, e os planos saíram de `docs/plans/<ID>_<resumo>.md` pra
`.nick/docs/<ID>/plan.md`. O `.nick.conf` antigo não é mais lido: se ele existir sem o
`.nick/nick.conf`, o script para antes de qualquer pergunta, com o comando de migração. Há
dois caminhos, na raiz do projeto alvo:

- **mover o arquivo**, mantendo os valores; na execução seguinte, ao confirmar o envio, o nick
  cria o resto da pasta (`.gitignore`, `troca/` e `docs/`):

  ```bash
  mkdir -p .nick && mv .nick.conf .nick/nick.conf
  ```

- **rodar o `--init`** com destino no diretório atual: ele cria a pasta `.nick/` inteira com o
  `nick.conf` no padrão (todas as chaves, com os comentários) e não lê nem altera o antigo. No
  fim, avisa pra copiar os valores do antigo pro novo e apagar o antigo.

Faça commit do `.nick/nick.conf`, do `.nick/.gitignore` e da remoção do `.nick.conf` antigo
(se ele era versionado). Com os dois arquivos, vale o novo, e o script avisa que o antigo pode
ser apagado. Os planos de `docs/plans/` não são migrados: no Desenvolvimento e no Review,
quando `.nick/docs/<ID>/plan.md` não existe e há um `docs/plans/<ID>_*.md`, o script avisa o
caminho antes da pergunta do plano — é só informá-lo ali. Um `.nick/troca/.gitignore` antigo
pode continuar onde está.

**Migração (`projects/*.sh`):** arquivos antigos `auto_scrum/projects/<nome>.sh` não são mais lidos. O
conteúdo não precisa mudar, só a extensão — o script avisa e sugere o comando pra cada um
(`mv 'auto_scrum/projects/<nome>.sh' 'auto_scrum/projects/<nome>.conf'`).

**Migração (`PRODUCTION_ENABLED`):** a pergunta de impacto em produção no prompt do PO
passou a depender da chave `PRODUCTION_ENABLED` e fica desligada por padrão. Configurações
criadas antes dela — tanto `.nick/nick.conf` versionados nos projetos alvo quanto
`auto_scrum/projects/<nome>.conf` — perdem a pergunta até ganharem
`PRODUCTION_ENABLED="true"`. Enquanto o arquivo carregado não tiver essa linha, o aviso de
chaves ausentes lista `PRODUCTION_ENABLED`, explicando que a pergunta está desligada; um
projeto sem produção usa `PRODUCTION_ENABLED="false"`, que mantém a pergunta desligada e
tira essa chave do aviso. Valores como `"TRUE"` ou `"sim"`, que antes desligavam a pergunta
em silêncio, agora geram o aviso de valor. Arquivos novos gerados por `--init` já vêm com
todas as chaves.

## Sandbox (ai-jail)

As ferramentas do nick que abrem um agente de IA (hoje, o `auto_scrum`) podem rodá-lo dentro
do [ai-jail](https://github.com/akitaonrails/ai-jail), um sandbox que usa bubblewrap +
Landlock no Linux e `sandbox-exec` no macOS. Dentro da jail, o agente só escreve no
diretório do projeto; o resto do sistema fica somente leitura, e `~/.ssh`, `~/.gnupg`,
`~/.aws` e os dados de navegador ficam escondidos (o `$HOME` da jail é privado). Isso limita
o estrago de uma prompt injection ou de um erro do agente, principalmente na etapa de
Desenvolvimento, que roda em `--permission-mode auto`.

Pra o projeto continuar rodando lá dentro, o nick monta sozinho, **somente leitura**, os
gerenciadores de versão que existirem no seu home: `~/.pyenv`, `~/.nvm`, `~/.rbenv`, `~/.asdf`,
`~/.local/share/mise`, `~/.local/share/uv/python`, `~/.sdkman` e `~/.volta` (sem eles, um venv
que aponta pro Python do pyenv quebra com "arquivo inexistente"), e também os navegadores do
Playwright (`~/.cache/ms-playwright`, ou o que estiver em `PLAYWRIGHT_BROWSERS_PATH`), pros
testes E2E — esses aparecem em `~/.ms-playwright`, com `PLAYWRIGHT_BROWSERS_PATH` apontando pra
lá. Nada de configuração ou credencial entra nessa lista, e você não precisa editar nada.
Dentro da jail, o sandbox do próprio Chromium não funciona: ele roda com `--no-sandbox`, que já
é o padrão do Playwright. O nick também avisa o agente (no system prompt do `claude`) de que
ele está na jail: o que ele enxerga, o que não alcança e que, se faltar algo, a primeira
hipótese é a jail — pra ele parar e te avisar, em vez de gastar tokens diagnosticando a
máquina.

Serviços da sua máquina em `localhost` (o banco do Docker que os testes usam, por exemplo)
ficam inalcançáveis de dentro da jail, que tem a própria rede. Pra liberar, liste as portas
na chave `JAIL_FORWARD_PORTS` da configuração do projeto, separadas por espaço:

```bash
JAIL_FORWARD_PORTS="54320 6379"
```

Só essas portas ficam acessíveis, e cada uma dá ao agente acesso total ao serviço dela. Como
o `.nick/nick.conf` é commitado com o projeto, o time inteiro herda a lista. Um valor fora do
formato (vírgula, porta 0 ou acima de 65535) faz a ferramenta parar com erro.

A rede da jail também é filtrada: além da lista padrão do ai-jail (a API do Claude, os
registries de pacotes e o GitHub), o nick libera sempre os MCPs ligados no projeto e a sua
**lista padrão**, em [`lib/jail/allow_hosts.txt`](lib/jail/allow_hosts.txt):
documentações de uso geral (Claude, AWS, Google Cloud, Python, Django/DRF, PostgreSQL, Rust,
MDN, React, OpenTofu etc.).
Pra incluir um host que vale pra todo projeto, acrescente uma linha nesse arquivo: um host por
linha, comentários com `#`, sem mexer no código nem nos testes (eles leem o arquivo e conferem
o formato). Outro site é recusado com "proxy refused the connection". O que é de um projeto
só vai na chave `JAIL_ALLOW_HOSTS`, separado por espaço:

```bash
JAIL_ALLOW_HOSTS="docs.djangoproject.com tailwindcss.com"
```

Cada host libera também os subdomínios dele e é mais um canal de saída pro agente. Use só o
nome do host: com `https://`, porta, caminho ou curinga (`*.`), a ferramenta para com erro.

É opt-in por projeto, pela chave `JAIL_ENABLED` da configuração do projeto (`.nick/nick.conf` ou
`auto_scrum/projects/<nome>.conf`). Vazio ou `"false"` roda o agente direto, como antes. Com
`"true"`, a ferramenta confere logo no início se a jail funciona nesta máquina e, se não
funcionar, para com uma mensagem dizendo o que falta. Não existe fallback: com `"true"` o
agente nunca roda sem a jail.

Com a jail ligada, a chave `JAIL_CLIPBOARD` escolhe quanto do terminal e do clipboard a jail
libera. A cada execução, a ferramenta mostra o nível numa linha, logo no começo, assim que
carrega a configuração (antes de qualquer pergunta), e de novo antes da confirmação — por
exemplo, `⚠️ Jail (ai-jail): ativa | Landlock: completo | modo de clipboard médio`. O brando
aparece em vermelho no terminal (sem cor com `NO_COLOR` definida). O `Landlock` é a segunda
camada da jail (um recurso do kernel que confere cada acesso a arquivo, além do isolamento do
bubblewrap): `completo` é o normal; `parcial` (kernel mais antigo, sem parte das regras) e
`inativo` (kernel sem suporte ou Landlock desligado) deixam a jail só com o bubblewrap. O
ai-jail também mostra isso ao abrir, mas o `claude` limpa a tela em seguida.

| Nível | Copiar de dentro do `claude` | Ctrl+V no `claude` | O que o agente ganha |
|---|---|---|---|
| `"restrito"` | não | não | nada além da jail |
| `"medio"` (ou vazio, o padrão) | sim (`--terminal-passthrough`) | não | escrever no seu clipboard (trocar o que você copiou, por exemplo por um comando) e mandar sequências de controle direto pro terminal |
| `"brando"` | sim | sim (`--terminal-passthrough --display`) | o do médio e também **ler** o seu clipboard (senhas e tokens copiados enquanto ele roda) e abrir janelas na sua tela |

Em todos os níveis, cole com o botão direito → Colar. No médio, olhe o que vai colar antes de
colar num shell; no brando, não copie nada sensível enquanto o agente roda. No restrito, pra
levar um trecho pra fora, peça ao agente que o grave num arquivo do projeto, ou use o
`/export` do `claude`, e copie pelo editor. Qualquer outro valor faz a ferramenta parar com
erro, antes de qualquer pergunta: um erro de digitação nunca escolhe um nível sozinho.

### Requisitos (opcionais, só para `JAIL_ENABLED="true"`)

Primeiro, rode:

```bash
./auto_scrum/auto_scrum.sh --requisitos-jail
```

Ele mostra o sistema detectado, o que já está instalado (`[ok]`), o que falta (`[falta]`) e
o comando de instalação de cada item que falta. O nick não instala nada: os comandos são
para você rodar.

- **ai-jail:**
  - Arch e derivadas (AUR): `yay -S ai-jail-bin` (com outro helper do AUR, o pacote é o
    mesmo);
  - macOS: `brew tap akitaonrails/tap && brew install ai-jail`;
  - outras plataformas: `cargo install --locked ai-jail`, ou o binário dos Releases do
    GitHub (https://github.com/akitaonrails/ai-jail/releases).
- **bubblewrap** (só Linux; o macOS usa o `sandbox-exec` do sistema):
  - Arch: `sudo pacman -S bubblewrap`;
  - Debian/Ubuntu: `sudo apt install bubblewrap`;
  - Fedora/RHEL: `sudo dnf install bubblewrap`;
  - outras distribuições: o pacote `bubblewrap` da distribuição.
- **Windows:** só via WSL 2, com o nick rodando dentro dele.

### Como ativar

1. Instale o que falta (acima).
2. Defina `JAIL_ENABLED="true"` na configuração do projeto.
3. Opcional, caminho alternativo pra necessidades que o nick não cobre sozinho (o nick já
   monta as toolchains e os navegadores do Playwright, libera os MCPs do Jira e do Sentry e as
   portas de `JAIL_FORWARD_PORTS`): um `.ai-jail` na raiz do projeto alvo, o arquivo de
   configuração do próprio ai-jail. Ex: liberar, somente leitura, um diretório de fora do
   projeto e esconder do agente o `.env`:

   ```toml
   ro_maps = ["~/Projetos/libs-compartilhadas"]
   mask = [".env"]
   ```

   O ai-jail trata o `.ai-jail` do projeto como **não confiável**, porque ele vem no
   repositório: o que *restringe* (`mask`, `deny_paths`) vale sempre, mas o que *amplia* o
   acesso (`ro_maps`, `rw_maps`, `allow_hosts`) só vale se o `~/.ai-jail` global de cada
   pessoa confiar no projeto:

   ```toml
   trust_project_config = ["~/caminho/do/projeto"]
   ```

   Sem isso, o ai-jail ignora a liberação e avisa ao abrir ("project .ai-jail map ... outside
   project ignored"). É assim que o projeto X vê o diretório Y e o projeto Z não. Como depende
   do `~/.ai-jail` de cada um, quem não configurar fica sem a liberação: prefira o que o nick
   já faz sozinho. O nick não lê, não gera e não altera nenhum dos dois arquivos.
4. Confira com `ai-jail --dry-run claude`, rodado na raiz do projeto alvo (mostra o que
   seria montado, sem gravar nada).

O ai-jail, por padrão, grava sozinho um `.ai-jail` no diretório onde roda, com o comando
inteiro — no caso do nick, o prompt, com o conteúdo de tickets e erros. Por isso o nick sempre
chama `ai-jail --no-save-config --no-status-bar [opções do JAIL_CLIPBOARD] [--map das
toolchains] [--allow-host do MCP, do JAIL_ALLOW_HOSTS e da lista padrão do nick]
[--forward-port do JAIL_FORWARD_PORTS] -- claude ...`: nada
é gravado no projeto, e o `.ai-jail` que você escreveu continua valendo. O `--no-status-bar`
tira a barra de status do ai-jail, que mostra o comando (o prompt
de novo) e se sobrepõe à interface do `claude`, bagunçando o terminal. O `--allow-host` libera
na rede da jail o MCP remoto que o projeto usa: `mcp.atlassian.com` com `JIRA_ENABLED="true"`
e `mcp.sentry.dev` com `SENTRY_ENABLED="true"` (os dois ficam fora da lista padrão do ai-jail,
que segue valendo). São as únicas opções que o nick passa, todas na linha de comando: nenhuma
depende do seu `~/.ai-jail`. Se você rodar o ai-jail à mão
na raiz do projeto, sem `--no-save-config`, revise o `.ai-jail` gerado antes de commitá-lo; um
`ai-jail` sem comando, rodado ali, relança o `command` salvo nele.

### O que a jail não protege

- O agente tem escrita no projeto inteiro, inclusive no `.git`, e os segredos dentro do
  projeto (ex: `.env`) continuam legíveis, a menos que o `.ai-jail` use `mask`/`deny_paths`.
- A rede é filtrada, mas não fechada: a API do Claude, os registries de pacotes e a lista
  padrão do nick (`lib/jail/allow_hosts.txt`) ficam liberados, e cada host extra liberado
  (ex: o de um MCP ou do `JAIL_ALLOW_HOSTS`) é mais um canal de saída.
- O estado do `claude` (`~/.claude`, credenciais incluídas) é montado com escrita dentro da
  jail.
- `git push` e outras ações remotas continuam possíveis por HTTPS, se houver credencial
  acessível. Por SSH não funcionam (ver abaixo).
- Só um conjunto mínimo de variáveis de ambiente entra na jail, mas o que for repassado
  explicitamente (`--env`, `env_pass`) fica visível ao agente.

A lista de `deny` do Claude Code (regras no `settings.json`) é outro mecanismo, tratado em
outro ticket.

### Problemas comuns

- **`bwrap` ausente:** instale o bubblewrap (acima).
- **User namespaces bloqueados** (Ubuntu 24.04+, Debian 13+, com AppArmor): o `bwrap` falha ao
  criar o namespace. Libere com `sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0` ou
  com um perfil AppArmor pro `bwrap`, conforme o README do ai-jail.
- **`bwrap` reprovado na validação de binário confiável:** o ai-jail só usa um `bwrap` com
  dono root e sem permissão de escrita pra grupo/outros; senão a verificação falha mesmo com
  tudo `[ok]` na lista de requisitos. Confira com `ls -l "$(command -v bwrap)"`. Dentro de um
  container ou sandbox sem root mapeado (o `bwrap` aparece com dono `nobody`/uid 65534), a
  jail não sobe: rode o nick fora dele.
- **Landlock indisponível** (kernel antigo ou sem o LSM ativo): o ai-jail segue sem ele,
  com uma camada a menos.
- **SSH e GPG inacessíveis dentro da jail:** `git push`/`git pull` por SSH e commits
  assinados falham. Faça essas ações fora da jail.
- **`claude` não encontrado dentro da jail:** o `$HOME` da jail é privado, então um `claude`
  instalado dentro do home (ex: `~/.local/bin`) pode não ser visível. Instale-o num caminho
  do sistema ou libere o diretório dele com `ro_maps` (com o projeto em
  `trust_project_config`).
- **Serviço da máquina em `localhost` inacessível** (banco, servidor de desenvolvimento): a
  jail tem a própria rede, e o `localhost` lá dentro não é o da sua máquina. Libere a porta
  com `JAIL_FORWARD_PORTS` na configuração do projeto (acima) — ex: `JAIL_FORWARD_PORTS="54320"`
  pro PostgreSQL do Docker que os testes usam.
- **O agente não acha um arquivo de fora do projeto** (ex: uma print em `~/Downloads`): a
  jail só mostra o projeto. Copie o arquivo pra `.nick/troca/` (ver
  [Pasta `.nick/` do projeto alvo](#pasta-nick-do-projeto-alvo)).
- **Ferramenta instalada no home não encontrada** (fora dos gerenciadores de versão que o
  nick monta, acima): o home da jail é privado. O agente avisa qual caminho faltou; abra um
  ticket no nick pra avaliar incluí-lo na lista.
- **MCP (Jira, Sentry) sem conexão:** a rede da jail só libera, por padrão, a API do Claude,
  os registries de pacotes e o GitHub. Os MCPs remotos oficiais o nick já libera sozinho
  (`mcp.atlassian.com` com `JIRA_ENABLED="true"`, `mcp.sentry.dev` com
  `SENTRY_ENABLED="true"`); confira se a chave do projeto está em `"true"`. Faça o login
  OAuth do MCP (`/mcp` no `claude`) fora da jail: lá dentro o navegador não abre. Outro
  servidor remoto (Jira ou Sentry self-hosted) você libera com o host dele em
  `JAIL_ALLOW_HOSTS`. Um MCP local que sobe via `npx`/`uvx` precisa também dos diretórios de
  cache dele (`rw_maps` no `~/.ai-jail` global ou no `.ai-jail` de um projeto listado em
  `trust_project_config`; num projeto não confiado, o `.ai-jail` só restringe).
- **Documentação ou site recusado** ("proxy refused the connection" no WebFetch): a jail só
  libera a lista acima. Adicione o host em `JAIL_ALLOW_HOSTS` (ex:
  `JAIL_ALLOW_HOSTS="docs.celeryq.dev"`) ou, se vale pra todo projeto, em
  `lib/jail/allow_hosts.txt`. O agente avisa qual host faltou.
- **Ctrl+V não cola dentro da jail:** o Ctrl+V do `claude` lê o clipboard direto do sistema,
  pelo socket do Wayland/X11, que a jail só abre no nível `"brando"` do `JAIL_CLIPBOARD`. Nos
  outros níveis, cole pelo terminal, com o botão direito → Colar. O Shift+Insert não serve: no
  terminal do VS Code no Linux, ele cola a seleção primária (o último texto selecionado com o
  mouse), e não o que você copiou.
- **Não consigo copiar de dentro da jail:** é o nível `"restrito"` do `JAIL_CLIPBOARD`: o
  ai-jail filtra o que o programa escreve no terminal, e o filtro barra a sequência (OSC 52)
  com que o `claude` copia. A seleção do próprio terminal também não serve, porque o `claude`
  controla o mouse (é o que faz a rolagem funcionar). Use `"medio"` (o padrão), aceitando o
  risco descrito acima, ou leve o trecho por um arquivo do projeto ou pelo `/export` do
  `claude`.
- **Windows:** só via WSL 2.
