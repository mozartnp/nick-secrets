# Instalar o nick

Este guia é para quem nunca usou o nick. No fim, você terá o comando `nick` disponível em
qualquer terminal, sem `sudo` e sem precisar abrir a pasta do repositório para usá-lo.

## 1. Requisitos

Instale antes:

- **git** e **make** (na maioria das distribuições já vêm instalados).
- **Rust**, pelo [rustup](https://rustup.rs). O `make install` usa o `cargo` para compilar o
  nick.
- **Claude Code** ([`claude`](https://claude.ai/code)) no `PATH`: é ele que o nick abre como
  agente.
- **envsubst**, do pacote `gettext` (em geral já vem instalado; no Arch:
  `sudo pacman -S gettext`). Ele monta os prompts.
- Opcional: **ai-jail** e **bubblewrap** (`bwrap`), só para rodar o agente dentro do sandbox
  (`JAIL_ENABLED="true"` no projeto). Os comandos de instalação de cada sistema estão na seção
  [Sandbox (ai-jail)](README.md#sandbox-ai-jail) do README.

Se faltar algum deles, o nick abre do mesmo jeito e avisa na barra de baixo o que falta e o que
deixa de funcionar.

## 2. Baixar e instalar

```bash
git clone https://github.com/mozartnp/nick-secrets.git
cd nick-secrets
make install
```

O `make install` compila o nick e instala:

- o programa em `~/.local/bin/nick`;
- os arquivos de que ele precisa (scripts e modelos de prompt) em `~/.local/share/nick/`.

Para instalar em outro lugar, passe o prefixo: `make install PREFIX=/outro/lugar` (o programa
vai para `/outro/lugar/bin/nick`).

Depois de instalado, o nick não depende mais da pasta do repositório: você pode movê-la ou
apagá-la. Para atualizar ou desinstalar (seções 6 e 7), você vai precisar de um clone de novo
(pode ser um novo: `git clone ...` e, dentro dele, `make install` ou `make uninstall`).

## 3. Pôr o nick no PATH

Se o `~/.local/bin` não estiver no seu `PATH`, o `make install` avisa no fim e mostra a linha a
acrescentar:

- **bash/zsh:** acrescente ao `~/.bashrc` (ou ao `~/.zshrc`) a linha
  `export PATH="$HOME/.local/bin:$PATH"`;
- **fish:** rode `fish_add_path ~/.local/bin`.

Abra um terminal novo e confira com `nick --version`. A saída mostra a versão e o commit de onde
o nick foi compilado (ex: `nick 0.1.0 (commit abc1234)`). A mesma informação aparece na tela
"Configurações" do nick.

## 4. Usar

Rode `nick` em qualquer terminal. As teclas de cada tela aparecem no rodapé. Para o primeiro
uso, veja a seção [O programa `nick`](README.md#o-programa-nick) do README.

## 5. Onde ficam os seus dados

Os dados ficam fora da instalação, em `~/.local/state/nick/` (ou em `$XDG_STATE_HOME/nick/`,
se você usa essa variável):

- `nick.db`: o banco (projetos, tickets e agentes);
- `logs/<projeto>-<número>/`: os prompts enviados aos agentes, uma pasta por projeto.

O nick instalado e um nick rodado da pasta do repositório (`cargo run`) usam o mesmo banco. Se
um nick mais novo já atualizou o banco, um mais velho se recusa a abri-lo, sem alterar nada, e
pede para você atualizar.

## 6. Atualizar

Na pasta do repositório:

```bash
git pull && make install
```

A instalação nova substitui a anterior. Os dados (seção 5) continuam onde estão.

## 7. Desinstalar

Na pasta do repositório:

```bash
make uninstall
```

(Use o mesmo `PREFIX` da instalação, se você passou um.) Saem só os arquivos que o
`make install` instalou. Ficam:

- o banco e os logs (seção 5): apague `~/.local/state/nick/` à mão se não quiser mais;
- qualquer arquivo que você mesmo tenha posto dentro de `~/.local/share/nick/` (o
  `make uninstall` avisa quais pastas manteve por isso).
