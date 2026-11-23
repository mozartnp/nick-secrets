# shellcheck shell=bash
#
# keys.sh: the registry of every key of the nick project config (.nick/nick.conf) — the only
# place that lists them, in skeleton order (see ABOUT.md, "Registro de chaves da
# configuração"). Part of the config library: it calls config_register_key, so
# lib/config/config.sh must be loaded first. The tools load both and then call
# config_register_nick_keys once, at the top, outside any function:
#   source lib/config/config.sh; source lib/config/keys.sh; config_register_nick_keys
#
# Why a function and not the calls in the file body: like every library in lib/, loading it
# only defines things (here, config_register_nick_keys) — it prints nothing, runs no command
# and registers nothing. The tool decides when to register. No shebang, no executable bit,
# no set of its own, no exit, source, . nor eval; the function is prefixed config_ and no
# global is created. The registry never initializes the key globals: each tool owns the ones
# it reads (auto_scrum.sh initializes them; bin/nick-init doesn't read the config at all).
#
# config_register_key <name> <default> <note> <skeleton comment> [<init question>]
# A toggle key ("true"/"false") must end in _ENABLED (config_is_toggle_key) and default to
# "false" ("true" only for TDD_ENABLED, on by default); a text key defaults to "". The note is
# the key-specific text both config warnings show, and only two keys have one, each because
# the generic text would mislead:
#   - PRODUCTION_ENABLED is the only flag whose off default removes something the PO prompt
#     already had. The note shows both ways out, "true" and "false", so a project without
#     production doesn't turn the question on just to silence the warning. It must not
#     repeat the PO question itself: end-to-end checks read stdout and stderr together and
#     look for that question to tell whether it reached the prompt;
#   - TDD_ENABLED is the only flag that is on by default: an absent or invalid value keeps
#     TDD on, so the note says that instead of "a opção está desligada.".
# The init question (5th argument, NS-53) is asked by config_init (bin/nick-init) and its answer
# goes into the skeleton. Only text keys can have one, and today only JIRA_URL has.
# A bad registration is a programming error: the first failing call makes the function
# return 1 right there (without exit, and without registering the keys after it), so a
# caller under set -e stops on load.

# config_register_nick_keys: registers every key of the nick config, in skeleton order.
# Returns 1 when any registration fails (calling it twice fails too: the keys are already
# registered).
config_register_nick_keys() {
  config_register_key STACK_DESCRIPTION "" "" \
'STACK_DESCRIPTION: stack técnica usada no prompt do Tech Leader/Desenvolvimento
(texto livre, sem ponto final no fim — o template já adiciona).
Exemplo: STACK_DESCRIPTION="Django avançado, django-tenants, Django ORM, PostgreSQL, pytest e TDD"' || return 1
  config_register_key SENTRY_ENABLED "false" "" \
'SENTRY_ENABLED: "true" se esse projeto usa Sentry — a opção "PO - Criar ticket a partir
do Sentry" aparece no menu principal (o PO busca o issue pelo MCP do Sentry do .mcp.json
do projeto alvo), e os prompts de PO e Tech Leader passam a mencionar links de erro
externo. Padrão é opt-in: vazio ou "false" esconde a opção e tira a menção dos prompts.' || return 1
  config_register_key PERMISSION_ENABLED "false" "" \
'PERMISSION_ENABLED: "true" se esse projeto tem um sistema de permissões — o PO passa a
avaliar, pra cada pedido, se será necessário adicionar uma nova permissão. Padrão é
opt-in: vazio ou "false" tira essa pergunta do prompt do PO.' || return 1
  config_register_key JIRA_ENABLED "false" "" \
'JIRA_ENABLED: "true" se esse projeto tem o MCP do Jira configurado (.mcp.json do
projeto alvo) — só então a opção "PO - Criar ticket a partir do Jira" aparece no menu
principal. Padrão é opt-in: vazio ou "false" esconde a opção do menu.' || return 1
  # JIRA_URL is a text key, independent of JIRA_ENABLED: the nick shell builds each Jira link as
  # base + ID (tickets::jira_link) and gets the value only from bin/nick-agent (--jira-url and
  # the 4th line of --check). Never validated.
  config_register_key JIRA_URL "" "" \
'JIRA_URL: base dos links do Jira (ex: https://empresa.atlassian.net/jira/browse/). O nick
monta o link de cada ticket e épico com ID do Jira como base + ID e o mostra no painel e
nos formulários das etapas. Guardada exatamente como digitada. Vazio: sem link.' \
'URL base dos links do Jira (opcional, Enter para pular; ex: https://empresa.atlassian.net/jira/browse/): ' || return 1
  config_register_key PRODUCTION_ENABLED "false" \
'a pergunta de impacto em produção está desligada no prompt do PO. Use "true" para ligá-la ou "false" para mantê-la desligada.' \
'PRODUCTION_ENABLED: "true" se esse projeto já está em produção — o PO passa a avaliar,
pra cada pedido, se existe impacto em produção. Padrão é opt-in: vazio ou "false" tira
essa pergunta do prompt do PO. Ligue ("true") quando o projeto entrar em produção.' || return 1
  config_register_key TDD_ENABLED "true" \
'a metodologia TDD continua ligada nos prompts de Tech Leader, Desenvolvimento e Review. Use "false" só em projeto sem testes automatizados.' \
'TDD_ENABLED: "true" mantém a metodologia TDD nos prompts de Tech Leader, Desenvolvimento
e Review. Use "false" só em projeto sem testes automatizados (ex: site em HTML estático):
o plano passa a dizer como cada critério de aceite será verificado, e a suíte só roda se
existir. Ao contrário das outras chaves *_ENABLED, vazio ou valor inválido mantêm a TDD
ligada — só "false" desliga.' || return 1
  config_register_key JAIL_ENABLED "false" "" \
'JAIL_ENABLED: "true" roda o claude de todas as etapas dentro do sandbox ai-jail: escrita
só no diretório do projeto, o resto do sistema somente leitura e ~/.ssh, ~/.gnupg, ~/.aws
e dados de navegador escondidos. Precisa do ai-jail e, no Linux, do bubblewrap (bwrap),
instalados por você — veja a seção "Sandbox (ai-jail)" do README ou rode
auto_scrum.sh --requisitos-jail. Se ligada e a jail não funcionar nesta máquina, o
auto_scrum para antes de qualquer pergunta. A rede da jail é filtrada, e ela libera os
MCPs remotos: mcp.atlassian.com quando JIRA_ENABLED="true" e mcp.sentry.dev
quando SENTRY_ENABLED="true". Padrão é opt-in: vazio ou "false" roda o claude direto, como
antes.' || return 1
  # JAIL_CLIPBOARD is a text key (three levels), so lib/config/config.sh doesn't check its value:
  # jail_check does, and only with JAIL_ENABLED="true", where it has an effect.
  config_register_key JAIL_CLIPBOARD "" "" \
'JAIL_CLIPBOARD: só vale com JAIL_ENABLED="true". Quanto do terminal e do clipboard a jail
libera; o auto_scrum mostra o nível a cada execução:
  "restrito": não dá pra copiar de dentro do claude;
  "medio" (ou vazio, o padrão): copia (--terminal-passthrough do ai-jail), mas o agente
  pode escrever no seu clipboard e mandar sequências de controle ao terminal;
  "brando": copia e o Ctrl+V do claude cola (--terminal-passthrough --display), mas o
  agente pode LER o seu clipboard (senhas, tokens) e abrir janelas na sua tela.
Nos três, cole com o botão direito → Colar. Outro valor faz o auto_scrum parar com erro.
Veja a seção "Sandbox (ai-jail)" do README.' || return 1
  # JAIL_FORWARD_PORTS is a text key too (a list of ports), checked by jail_check.
  config_register_key JAIL_FORWARD_PORTS "" "" \
'JAIL_FORWARD_PORTS: só vale com JAIL_ENABLED="true". Portas de localhost da sua máquina
que o claude alcança de dentro da jail (--forward-port do ai-jail), separadas por espaço —
ex: o PostgreSQL do Docker que os testes usam. Exemplo:
  JAIL_FORWARD_PORTS="54320 6379"
Cada porta liberada dá ao agente acesso total ao serviço dela. Este arquivo é commitado
com o projeto, então o time inteiro herda a lista. Vazio: nenhuma porta (o localhost da
máquina fica inalcançável). Outro formato faz o auto_scrum parar com erro. Veja a seção
"Sandbox (ai-jail)" do README.' || return 1
  # JAIL_ALLOW_HOSTS is a text key too (a list of hosts), checked by jail_check.
  config_register_key JAIL_ALLOW_HOSTS "" "" \
'JAIL_ALLOW_HOSTS: só vale com JAIL_ENABLED="true". Hosts que o claude alcança de dentro
da jail (--allow-host do ai-jail), separados por espaço — ex: a documentação da stack do
projeto. Exemplo:
  JAIL_ALLOW_HOSTS="docs.djangoproject.com tailwindcss.com"
Cada host libera também os subdomínios dele. Já são liberados os hosts dos MCPs ligados
acima e os da lista padrão do nick (lib/jail/allow_hosts.txt, com a documentação do
Claude, de Python, Django etc.). Este arquivo é commitado com o projeto, então o time
inteiro herda a lista. Vazio: nenhum host além desses. Outro formato (com https://, porta
ou caminho) faz o auto_scrum parar com erro. Veja a seção "Sandbox (ai-jail)" do README.' || return 1
}
