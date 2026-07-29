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

1. Escolha o projeto (define a stack usada no prompt) — veja "Stack por projeto" abaixo.
   Só é usado de fato por Tech Leader/Desenvolvimento, mas a pergunta aparece sempre
   (é a primeira, antes do tipo).
2. Escolha o tipo de conversa no menu (PO, Tech Leader, Desenvolvimento ou Review).
3. Responda o roteiro de perguntas daquela etapa. Campos de descrição longa abrem
   `$EDITOR` num arquivo temporário (salve e feche para continuar) — colar texto grande
   direto no terminal buga a exibição, por isso a descrição sempre passa pelo editor.
4. O prompt final é mostrado na tela e salvo em `auto_scrum/logs/<tipo>_<timestamp>.md`.
5. Confirme (`s`/`N`) para abrir o `claude` com esse prompt em uma sessão nova, ou
   cancele — o prompt gerado continua salvo no log.

Cada etapa roda de forma independente (normalmente em terminais separados); não há
encadeamento automático entre elas — por exemplo, depois do Review, quem decide se volta
para o Desenvolvimento é o humano, lendo o resultado.

`auto_scrum/logs/*.md` é gitignorado (é um registro de trabalho pessoal, não artefato
do projeto). Os templates ficam em `auto_scrum/templates/` e podem ser editados
livremente sem tocar no script.

#### Stack por projeto

Tech Leader, Desenvolvimento e Review usam a stack técnica de um projeto (ex: "Django, DRF,
PostgreSQL" ou "Rust, Actix"). Cada projeto é um arquivo em `auto_scrum/projects/<nome>.sh`:

```bash
STACK_DESCRIPTION="Django avançado, django-tenants, Django ORM, Django CBV, Django auth, Bootstrap 5, PostgreSQL, JavaScript, pytest e TDD"
```

- Passe o nome direto: `./auto_scrum/auto_scrum.sh --projeto=meuprojeto` (erro claro se
  o arquivo não existir).
- Sem argumento: o script mostra um menu com os projetos encontrados + a opção
  "Nenhum" (usa uma stack genérica).
- Pra criar um projeto novo: `./auto_scrum/auto_scrum.sh --init` — pergunta o nome e
  cria `auto_scrum/projects/<nome>.sh` com as variáveis vazias (`STACK_DESCRIPTION=""`) e um
  comentário de exemplo. Abre e preenche depois. Se o projeto já existir, pergunta antes
  de sobrescrever.
- Só `--projeto=` (singular) e `--init` são reconhecidos — qualquer outra flag (ex:
  `--projetos=` com "s") dá erro claro em vez de ser ignorada silenciosamente.

`auto_scrum/projects/*.sh` é gitignorado — é configuração local, específica de cada
máquina/projeto, não faz parte do repositório.
