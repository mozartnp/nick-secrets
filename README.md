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

#### Uso

```bash
./auto_scrum/auto_scrum.sh [--projeto=<nome>]
```

1. Configuração do projeto (stack e flags): vem do `.nick.conf` do diretório atual, se
   existir (o script mostra de onde carregou); senão o script pergunta qual projeto usar.
   Veja "Stack por projeto" abaixo.
2. Escolha o tipo de conversa no menu (PO, Tech Leader, Desenvolvimento ou Review).
3. Responda o roteiro de perguntas daquela etapa. Campos de descrição longa abrem
   `$EDITOR` num arquivo temporário (salve e feche para continuar) — colar texto grande
   direto no terminal buga a exibição, por isso a descrição sempre passa pelo editor.
4. O prompt final é mostrado na tela e salvo em `auto_scrum/logs/<tipo>_<timestamp>.md`.
5. Confirme (`s`/`N`) para abrir o `claude` com esse prompt em uma sessão nova, ou
   cancele — o prompt gerado continua salvo no log.

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

1. **`.nick.conf` na raiz do diretório atual** (o projeto alvo, de onde o script é
   rodado). Se existir, é usado e o script imprime
   `Configuração do projeto carregada de: <caminho>` — sem menu de projetos. Se
   `--projeto=` também for passado, ele é ignorado com um aviso (mesmo que o nome não
   exista em `auto_scrum/projects/`). Só o diretório atual é olhado, não os diretórios
   pais.
2. **`auto_scrum/projects/<nome>.conf`**, escolhido por `--projeto=<nome>` (erro claro se
   o arquivo não existir) ou, sem argumento, pelo menu com os projetos encontrados + a
   opção "Nenhum" (usa uma stack genérica).

O formato de `.nick.conf` e de `auto_scrum/projects/<nome>.conf` é o mesmo. O exemplo
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
  `"TRUE"`, `"sim"`) deixa a opção desligada e gera um aviso com o arquivo e o número da
  linha.

Os avisos não interrompem a execução. Quando uma chave nova é criada, as configurações que
já existem passam a receber o aviso de chaves ausentes, com a linha a adicionar.

- Pra criar a configuração: `./auto_scrum/auto_scrum.sh --init` — pergunta o destino
  (`.nick.conf` no diretório atual, ou `auto_scrum/projects/<nome>.conf`, que pede o nome)
  e cria o arquivo com as chaves aceitas, desligadas/vazias, e comentários explicando cada
  uma. O conteúdo é o mesmo nos dois destinos. Abre e preenche depois. Se o arquivo já
  existir, pergunta antes de sobrescrever.
- Só `--projeto=` (singular) e `--init` são reconhecidos — qualquer outra flag (ex:
  `--projetos=` com "s") dá erro claro em vez de ser ignorada silenciosamente.

`.nick.conf` é feito pra ser versionado **no repositório do projeto alvo**, junto com o
código que ele descreve, e compartilhado com o time. Como o conteúdo dele entra no prompt
enviado ao `claude`, revise mudanças nesse arquivo no PR como qualquer outro arquivo que
influencia o agente (ex: `CLAUDE.md`) — não ser executado não o torna inofensivo. Já
`auto_scrum/projects/` é gitignorado — é configuração local, específica desta máquina, não
faz parte deste repositório (`.nick.conf` também é gitignorado aqui, pra nunca ser commitado
neste repo por engano).

**Migração:** arquivos antigos `auto_scrum/projects/<nome>.sh` não são mais lidos. O
conteúdo não precisa mudar, só a extensão — o script avisa e sugere o comando pra cada um
(`mv 'auto_scrum/projects/<nome>.sh' 'auto_scrum/projects/<nome>.conf'`).

**Migração (`PRODUCTION_ENABLED`):** a pergunta de impacto em produção no prompt do PO
passou a depender da chave `PRODUCTION_ENABLED` e fica desligada por padrão. Configurações
criadas antes dela — tanto `.nick.conf` versionados nos projetos alvo quanto
`auto_scrum/projects/<nome>.conf` — perdem a pergunta até ganharem
`PRODUCTION_ENABLED="true"`. Enquanto o arquivo carregado não tiver essa linha, o aviso de
chaves ausentes lista `PRODUCTION_ENABLED`, explicando que a pergunta está desligada; um
projeto sem produção usa `PRODUCTION_ENABLED="false"`, que mantém a pergunta desligada e
tira essa chave do aviso. Valores como `"TRUE"` ou `"sim"`, que antes desligavam a pergunta
em silêncio, agora geram o aviso de valor. Arquivos novos gerados por `--init` já vêm com
todas as chaves.
