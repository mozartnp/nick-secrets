# nick-panel: protótipo do painel de terminal (NS-29)

**Protótipo descartável** do épico NS-28 ("nick como programa"). Ele existe pra responder uma
pergunta: um painel em Rust (ratatui) consegue rodar o `claude` real, dentro da ai-jail e com as
regras de permissão do nick, com várias sessões simultâneas, de um jeito usável? O código **não**
é a base da casca futura.

O painel mostra uma lista fixa de duas sessões fictícias (`Projeto Demo > NS-0001 > Tech
Leader` e `Projeto Demo > a1b2c3 > Dev`) e, ao lado, o terminal da sessão selecionada. As duas
sessões rodam ao mesmo tempo. Cada uma é um PTY com `bash launch_session.sh`, que abre o
`claude` pelo mesmo caminho do nick: `jail_check` + `claude_settings_load` +
`jail_agent_notice` + `jail_exec` de `lib/`. A jail fica **sempre ligada**: não existe opção que
a desligue.

```
nick-panel ──PTY──▶ bash launch_session.sh ──exec──▶ ai-jail <jail_options> -- claude --settings … --append-system-prompt …
                                                         └─▶ bwrap ─▶ claude
```

## Como rodar

Precisa do Rust (cargo) e de tudo o que a jail do nick já exige (ai-jail 3.x, bwrap; ver
"Sandbox (ai-jail)" no README da raiz). Rode **fora da jail do nick** (ela não aninha outra
jail), com o cwd no projeto onde se vai trabalhar. Esse cwd é o de todas as sessões e o único
diretório gravável dentro da jail:

```bash
cd ~/Projetos/git/<projeto>
cargo run --manifest-path ~/Projetos/git/nick_secrets/prototype/panel/Cargo.toml --release
```

O launcher é achado pelo diretório do crate (`CARGO_MANIFEST_DIR`, fixado na compilação). Por
isso o binário só funciona a partir do checkout, onde o `cargo run` o deixa. O painel não lê o
`.nick/nick.conf`: as opções da jail vêm destas variáveis de ambiente:

| Variável | Padrão | Uso |
|---|---|---|
| `NICK_PANEL_CLIPBOARD` | vazio (= `medio`) | nível do `JAIL_CLIPBOARD`: `restrito`, `medio` ou `brando` |
| `NICK_PANEL_ALLOW_HOSTS` | vazio | hosts extras, separados por espaço (ex: `mcp.atlassian.com`) |
| `NICK_PANEL_FORWARD_PORTS` | vazio | portas de `localhost`, separadas por espaço |
| `NICK_PANEL_TEST_PANIC` | — | **só teste:** definida, o painel entra em panic 1,5 s depois de abrir, na thread principal; com o valor `thread`, numa thread secundária (prova que o terminal volta ao normal, as sessões são encerradas e o painel sai com 101) |

## Atalhos

| Foco | Tecla | Efeito |
|---|---|---|
| painel | `↑`/`↓`, `k`/`j` | troca a sessão selecionada (as outras continuam rodando) |
| painel | `Enter` | passa o foco pro terminal da sessão |
| painel | `Shift+PageUp`/`Shift+PageDown`, roda do mouse | rola o histórico da sessão |
| painel | `q` | sai; com sessão rodando, pergunta `Encerrar N sessão(ões) ativa(s)? (s/N)` |
| terminal | `Ctrl+\` | **atalho reservado:** volta o foco ao painel, sem encerrar a sessão |
| terminal | qualquer outra | vai pro `claude` (inclusive `Ctrl+C`, `Esc`, `Enter`, `q`) |

Por que `Ctrl+\`: a documentação de keybindings do Claude Code
(code.claude.com/docs/en/keybindings, conferida em 2026-11-12, `claude` 2.1.319) não usa
`Ctrl+\` em nenhum contexto, nem como atalho reservado. Já o `Ctrl+]` foi evitado de propósito.
Em raw mode, o `Ctrl+\` chega como o byte 0x1c, que o crossterm entrega como `Ctrl+4` (e como
`Ctrl+\` com o protocolo de teclado do kitty), e não vira SIGQUIT: o raw mode desliga o ISIG.

**No terminal integrado do VS Code o `Ctrl+\` não chega ao painel:** o VS Code fica com a
tecla, que é atalho dele ("dividir editor"). Num terminal standalone, ela funciona (conferido
pelo usuário). Pra usar o painel no VS Code, acrescente uma vez ao `keybindings.json` do
usuário (`Ctrl+Shift+P` → "Preferences: Open Keyboard Shortcuts (JSON)"; no Linux, fica em
`~/.config/Code/User/keybindings.json`), dentro da lista `[...]`:

```json
{
  "key": "ctrl+\\",
  "command": "workbench.action.terminal.sendSequence",
  "args": { "text": "\u001c" },
  "when": "terminalFocus"
}
```

Com o terminal em foco, a regra manda o byte 0x1c ao programa em vez de dividir o editor. Fora
do terminal, o `Ctrl+\` continua dividindo o editor.

## Testes

```bash
cargo test --manifest-path prototype/panel/Cargo.toml          # unitários + integração em PTY
cargo clippy --manifest-path prototype/panel/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path prototype/panel/Cargo.toml --check
bash prototype/panel/tests/test_launch_session.sh             # também roda no make test
```

- `tests/test_launch_session.sh` (bash, achada pelo `make test`): confere que o argv que o
  ai-jail recebe é exatamente o que a lib gera (`jail_options` + `--` + `claude --settings
  <permissions.json> --append-system-prompt <jail_agent_notice>`). Também confere que o `claude`
  só roda dentro da jail (sem ai-jail, com versão fora da faixa ou com nível inválido ele não é
  chamado), a estrutura do launcher e o `shellcheck -x`.
- `src/keys.rs`, `src/app.rs`, `src/ui.rs`: testes unitários da tradução de teclas, colar e
  mouse; do autômato de foco e saída; e do desenho (`TestBackend`).
- `tests/pty_launch.rs`: sessões reais em PTY, abertas pelo mesmo construtor do painel, com os
  falsos de `tests/fixtures/bin/`. Cobre argv, cor na tela, escrita, resize, encerramento do
  grupo (e só dos grupos de sessões ainda não colhidas), leitura em segundo plano, o repasse do
  copiar (OSC 52) e o teste estrutural de que `src/` só lança `bash` com o launcher.
- `tests/panic_hook.rs`: o binário `nick-panel` num PTY, com os falsos e o
  `NICK_PANEL_TEST_PANIC` na thread principal e numa secundária: sai com 101, sai da tela
  alternativa e nenhum processo das sessões sobra.
- `src/session.rs`: testes unitários do repasse do copiar (OSC 52) e da tela mínima 2x2 (com
  o panic do `vt100` a 1 coluna e a 1 linha como controle).

O `make test` da raiz chegou ao NS-29 com 2 falhas que não vinham dele:
`auto_scrum/tests/test_auto_scrum.sh` esperava que o `.gitignore` ignorasse `docs/plans/x.md`,
e o commit `3db87b5` ("adição no gitignore", já na `main`) tinha tirado o `docs/plans/*` de lá
(a pasta não é mais usada). Com autorização do usuário, o teste foi ajustado no NS-29 (a única
mudança em `auto_scrum/`), e o `make test` passa inteiro.

## Roteiro da verificação manual

Rode num terminal comum, **fora da jail**, com o painel aberto num terminal e as coletas em
outro: `bash .nick/docs/NS-29/coleta.sh <cenario>`, da raiz do nick_secrets. A coleta grava em
`.nick/troca/NS-29/`. O `coleta.sh` é uma evidência local desta verificação (`.nick/` não é
versionado): quem clonar o repositório não o tem, e o protótipo não depende dele. Anote em cada item: funcionou, falhou ou parcial, e o terminal usado
(ex: o terminal do VS Code).

1. **Jail e regras.** Em cada sessão, peça ao `claude` pra rodar `ls ~/.ssh` (sem acesso),
   `curl -sS https://example.com` (recusado) e `sudo true` (negado pela regra deny). Coleta:
   `coleta.sh arvore`, que deve mostrar `bash → ai-jail → bwrap → claude`.
2. **Interação.** Texto com acento, `Enter`, `Esc` (cancelar uma resposta), `Ctrl+C`
   (interromper), setas no histórico, `/` (menu de comandos), `Shift+Tab` (modos de permissão).
3. **Cores.** O realce de diff e o tema aparecem como fora do painel? Peça ao agente
   `echo $TERM $COLORTERM` e anote a saída.
4. **Rolagem.** Uma saída longa; roda do mouse e `Shift+PageUp`/`Shift+PageDown` (estes no foco
   do painel).
5. **Redimensionar** a janela várias vezes, com uma resposta em andamento e com a outra sessão
   escondida. Não pode ficar lixo na tela, e a sessão escondida tem de aparecer certa ao voltar.
6. **Atalho reservado e troca de sessão.** `Ctrl+\` volta ao painel. Troque pra outra sessão e
   volte: as duas continuam rodando (o `claude` de cada uma responde).
7. **Copiar/colar e mouse,** nos três níveis (`NICK_PANEL_CLIPBOARD=restrito`, `medio` e
   `brando`, um painel por nível), anotando o terminal usado. Em cada nível, separadamente:
   (i) copiar pelo próprio `claude` (selecionar com o mouse dentro dele, sem `Shift`, e colar
   fora do painel); (ii) seleção nativa do terminal com `Shift` segurado; (iii) `Ctrl+V` no
   `claude`; (iv) botão direito → Colar; (v) colar texto de várias linhas; e um clique.
   Compare linha a linha com a tabela de copiar/colar da seção da jail no `ABOUT.md` (o
   esperado: (i) não copia no `restrito`, copia no `medio` e no `brando`; (iii) só cola no
   `brando`).
8. **Saída e órfãos.** Antes de cada cenário rode `coleta.sh antes-<x>`, e depois,
   `coleta.sh depois-<x>`. Depois do cenário, só pode sobrar o que já existia antes. Anote também
   se o terminal voltou ao normal ou se precisou de `reset`.
   - (a) `q` + `s` com as duas sessões ativas;
   - (b) `q` + `n` (as sessões continuam);
   - (c) `kill -TERM $(pgrep -n -x nick-panel)`;
   - (d) `kill -9 $(pgrep -n -x nick-panel)` (queda);
   - (e) fechar a janela do terminal.
9. **Uso real e CPU.** Use o painel numa sessão de trabalho de verdade, com duas sessões ao
   mesmo tempo, por um tempo razoável (ex: uma etapa do nick, com o MCP do Jira via
   `NICK_PANEL_ALLOW_HOSTS="mcp.atlassian.com"`). CPU: `coleta.sh cpu-ocioso`,
   `coleta.sh cpu-resposta` (com o `claude` gerando resposta) e `coleta.sh cpu-resize`
   (redimensionando). Pra comparar: `coleta.sh cpu-claude-direto`, com o `claude` rodando direto
   num terminal comum. Anote a duração do uso e as suas impressões.

## Relatório

Evidências em `.nick/docs/NS-29/` (fora do git).

### Já verificado (automático, dentro da jail do agente, com ai-jail e claude falsos)

| Item | Resultado | Observação |
|---|---|---|
| Lançamento pelo caminho do nick | funcionou | argv exato calculado pela lib (suíte bash); a sessão do painel chega ao `ai-jail` falso com `-- claude --settings …` (teste Rust); `src/` só lança `bash` com o launcher (teste estrutural) |
| Sem `claude` direto, sem jail desligável | funcionou | sem ai-jail, com versão 2.14.0 ou com nível inválido, o launcher para e o `claude` falso não é chamado |
| Saída e cor na tela | funcionou | o `OK` vermelho do `claude` falso chega à célula com a cor 1 |
| Entrada | funcionou | `\x03`, texto, `\r` e `ç` chegam ao filho em raw mode |
| Resize | funcionou | o parser e o PTY mudam; o filho recebe SIGWINCH; a sessão escondida já está no tamanho novo ao voltar (smoke test) |
| Sessão em segundo plano | funcionou | 1 MB de saída sem ninguém olhar: o filho termina, e o fim da saída está no parser |
| `Ctrl+\` em raw mode | funcionou | chega como tecla (0x1c → `Ctrl+4`) e volta o foco; o painel não morre de SIGQUIT |
| Saída `q`+`s`, `q`+`n` | funcionou | `(s/N)`; `n` mantém as sessões; `s` sai com 0 e nenhum processo do grupo sobra |
| `kill -TERM`, `kill -HUP` no painel | funcionou | o mesmo encerramento; terminal restaurado; nenhum processo sobra |
| Panic | funcionou | `NICK_PANEL_TEST_PANIC`, na thread principal e numa secundária (`tests/panic_hook.rs`): o terminal é restaurado antes da mensagem, sai com 101, nenhum processo sobra. Antes do review, um panic numa thread secundária deixava o laço principal desenhando sobre o terminal restaurado |
| `kill -9` no painel | funcionou com os falsos | nada sobrou (o fechamento do master manda SIGHUP ao grupo). Com o ai-jail real: item 8(d), abaixo |
| CPU ociosa | funcionou | 0 tick de CPU em 5 s com duas sessões abertas (laço bloqueado no canal, sem timer) |

### Verificação manual com o `claude` e o ai-jail reais (passo 7)

O agente conferiu por PTY o que dava sem olho humano. O usuário rodou o roteiro e relatou
"tudo ok" nos itens que só ele podia ver (2026-11-12). O review de 2026-11-17 mostrou que o
item 7 não se sustentava (ver a linha dele) e que o item 9 não tem a duração nem as
impressões; os dois estão pra refazer.

| Item do roteiro | Resultado | Observação |
|---|---|---|
| 1. Jail e regras / árvore de processos | funcionou | O agente verificou, com o `claude` real no painel: `curl` pro example.com foi recusado pelo proxy da jail (403, rc 7); `sudo -n true` foi barrado pela regra deny do nick ("Permission to use Bash ... has been denied"); `ls ~/.config` mostrou só os manifests de native messaging da extensão do Claude pra Chrome (gravados pelo próprio `claude` no home privado da jail), o `mimeapps.list` e um `ai-jail` vazio, sem nenhum perfil real (`~/.ssh` não existe nesta máquina, então não serviu de prova). Árvore: `nick-panel → ai-jail → bwrap → bwrap → claude → proxy-bridge`, com o `bwrap` em sessão e grupo próprios (`jail_regras_real.txt`, `jail_config_real.txt`, `smoke_real.txt`) |
| 2. Interação | funcionou | Verificado pelo agente, por PTY: o acento (`olá ação ç`) aparece no campo; `Enter` envia e o `claude` responde; `Esc` e `Ctrl+C` interrompem uma resposta em andamento; a seta pra cima traz o prompt anterior; `/` abre o menu de comandos; `Shift+Tab` troca o modo; `Ctrl+U` limpa o campo. O `Esc` não limpa o campo no `claude` 2.1.319, nem fora do painel (comportamento dele) |
| 3. Cores, `TERM`/`COLORTERM` dentro da jail | funcionou | `TERM=xterm-256color` e `COLORTERM=truecolor` chegam dentro da jail real apesar do `--no-inherit-env`, que barra outras variáveis (`term_dentro_da_jail.txt`, e o `echo` do próprio agente). Comparação visual das cores e do realce de diff com o `claude` fora do painel: ok (usuário) |
| 4. Rolagem | funcionou | Roda do mouse e `Shift+PgUp`/`Shift+PgDn`: ok (usuário) |
| 5. Redimensionar | funcionou | Verificado pelo agente: 10 resizes seguidos com as duas sessões, e o painel ficou inteiro (lista, rodapé e `claude` redesenhados); com os falsos, a sessão escondida já volta no tamanho novo. Sem lixo visual ao redimensionar com resposta em andamento: ok (usuário) |
| 6. Atalho reservado e troca de sessão | funcionou | Troca de sessão com o `claude` real: **funcionou** (as duas ficam vivas, a resposta da sessão 1 continua lá na volta; verificado pelo agente). `Ctrl+\` volta ao painel num terminal standalone; no terminal integrado do VS Code só volta com a regra do `keybindings.json` (seção "Atalhos"), porque o VS Code fica com a tecla (atalho dele, "dividir editor"). Com a regra no VS Code: ok (usuário) |
| 7. Copiar/colar e mouse (restrito/médio/brando) | **a refazer** | O usuário relatou "ok" nos três níveis (2026-11-12), sem separar o que foi feito nem o terminal. Mas, na versão testada, o painel descartava o OSC 52 que o `claude` manda ao copiar (o `vt100` o entrega a um callback, e o padrão não faz nada): copiar pelo próprio `claude` não podia funcionar em nenhum nível, e o "ok" deve ter sido a seleção nativa com `Shift`. Depois do review, o painel repassa o OSC 52 ao terminal de fora ("Copiar", abaixo). Falta refazer o item, nível a nível e com o terminal |
| 8. Saída e órfãos (a–e), terminal restaurado | funcionou (com a ressalva do (d)) | (a) `q`+`s` com duas sessões, terminal standalone: **funcionou**, e o terminal voltou ao normal sem `reset`. As 2 árvores `ai-jail → bwrap → bwrap → claude → proxy-bridge` (10 processos) sumiram, e o resto da lista do `pgrep` ficou igual (coletas de 12:22). O ai-jail põe o `bwrap` numa sessão e num grupo de processos próprios, então o SIGTERM do painel no grupo só chega ao `ai-jail`; mesmo assim, ele encerra o que está abaixo dele. (c) `kill -TERM`: **funcionou** (nada sobrou). (d) `kill -9`: **parcial**, nada sobrou (o `ai-jail` recebe o SIGHUP do PTY e encerra o resto), mas o terminal ficou sujo: restos do painel na tela, porque o `kill -9` não deixa o painel restaurar a tela alternativa, o mouse e o raw mode (print `kill9_terminal_sujo.png`). (e) fechar a janela: **funcionou** (nada sobrou). Tudo no Konsole, 12:33–12:36. (b) `q`+`n`: **funcionou**, o painel e os 10 processos das sessões continuaram, com os mesmos PIDs (12:38). |
| 9. Uso real e CPU | parcial | Uso real com duas sessões: "ok" (usuário), sem a duração e as impressões que o roteiro pede (pendente). As coletas `coleta.sh cpu-*` não vieram pra `.nick/troca/`; a CPU registrada é a medida pelo agente (seção "CPU") |

### Decisões e limitações conhecidas

- **`vt100` 0.16.2, e não 0.18.1.** O `tui-term` 0.3.4 (o mais novo) depende do `vt100` 0.16 e
  o reexporta "pra garantir compatibilidade de versão". Com o 0.18, os tipos não batem com o
  `PseudoTerminal`. Fixar a versão compatível era a ação prevista no plano, e o 0.16 tem tudo
  o que o painel usa (modos de mouse, bracketed paste, cursor de aplicação, scrollback). As
  outras crates estão na versão estável mais recente: `ratatui` 0.30.2, `crossterm` 0.29.0,
  `portable-pty` 0.9.0, `tui-term` 0.3.4, `nix` 0.31.3, `signal-hook` 0.4.6.
- **`TERM`/`COLORTERM`:** o painel os passa ao filho, e eles chegam ao `claude` dentro da jail
  real apesar do `--no-inherit-env` (item 3, conferido).
- **Grupos de processos com o ai-jail real:** o ai-jail põe o `bwrap` (e o `claude` abaixo dele)
  numa sessão e num grupo de processos próprios. O SIGTERM e o SIGHUP que o painel manda ao grupo
  da sessão só chegam ao `ai-jail`, e quem encerra o resto é ele. Na saída por `q`+`s` isso
  funcionou, e também no `kill -TERM`, no `kill -9` e ao fechar a janela: nenhum órfão. A mitigação com
  `PR_SET_PDEATHSIG` não foi necessária.
- **`kill -9` deixa o terminal sujo:** nenhum programa consegue reagir a SIGKILL, então a tela
  alternativa, a captura de mouse e o bracketed paste ficam ligados. O conserto é rodar `reset`
  no terminal. É a mesma limitação de qualquer TUI (vim, htop).
- **`PR_SET_PDEATHSIG`:** o `portable-pty` 0.9 não expõe `pre_exec` (ele mesmo usa o `pre_exec`
  pro `setsid`/`TIOCSCTTY`). Não fez falta: o item 8(d) não deixou órfãos.
- **`Ctrl+\` no VS Code:** o terminal integrado do VS Code não o repassa sem a regra do
  `keybindings.json` (seção "Atalhos"). A decisão do usuário foi manter o atalho e documentar a
  regra. A casca de verdade precisa de um atalho que funcione no VS Code sem configuração.
- **`~/.config` dentro da jail (achado da `lib/jail`, fora do escopo do NS-29):** o
  `jail_agent_notice` diz ao agente que o `~/.config` não existe lá dentro, mas o próprio `claude`
  cria nele, ao subir, os manifests de native messaging da extensão do Chrome (um por navegador)
  e há também um `mimeapps.list`. Não é perfil real, mas o agente estranhou a diferença. Vale um
  ticket pra ajustar o texto do aviso.
- **Modo de permissão:** as sessões abrem em "auto mode", mas o painel não passa
  `--permission-mode` (a suíte bash confere). O modo vem da configuração do próprio `claude`
  do usuário.
- **Mouse:** com a captura ligada, a seleção nativa do terminal normalmente exige `Shift`
  segurado (item 7). O painel repassa ao `claude` só a roda, o clique esquerdo e o arrasto com
  o botão esquerdo, quando ele pede mouse; senão, a roda rola o histórico.
- **Colar:** quebras de linha viram CR, como no xterm. Dentro do bracketed paste, todo caractere
  de controle menos tab e CR é descartado (C0, DEL e C1, como no xterm): sem nenhum ESC, o texto
  colado não forma o `ESC[201~` que terminaria a colagem antes da hora e mandaria o resto como
  digitado. A primeira versão só tirava o ESC da sequência de fim, e um `ESC ESC[201~` colado
  a formava de novo (review, problema 1).
- **Copiar (OSC 52):** o `claude` copia mandando ao terminal um `ESC]52;c;<base64>`, que o
  `vt100` não mostra: ele o entrega a um callback. O painel guarda o pedido e o escreve no
  terminal de fora, entre dois quadros; quem copia é esse terminal. Só o copiar é repassado: um
  pedido de leitura do clipboard (`ESC]52;c;?`) é descartado, então o agente nunca lê o
  clipboard pelo painel. Que pedidos chegam ao painel é decisão da jail: no `restrito`, o filtro
  do ai-jail barra o OSC 52 antes (como fora do painel). A primeira versão descartava todos
  (review, problema 2).
- **Encerramento:** SIGTERM no grupo de processos de cada sessão, SIGKILL depois de 2 s no que
  sobrar, e `wait` de cada filho. Só os grupos de sessões ainda não colhidas recebem sinal:
  depois que o filho é colhido, o pid dele pode ir pra outro líder de grupo (um job do shell do
  usuário), que levaria SIGTERM e SIGKILL. O custo: o que uma sessão já encerrada deixou no
  grupo não é encerrado pelo painel (com o ai-jail real, quem limpa é ele). A primeira versão
  sinalizava todos (review, problema 4).
- **Tela mínima da sessão, 2x2:** o `vt100` 0.16 entra em panic com um caractere largo (emoji,
  CJK) numa tela de 1 coluna (`cols - largura` dá overflow) ou de 1 linha (a quebra subtrai a
  rolagem da linha). O painel dava 1 coluna à sessão em qualquer janela com menos de 33
  colunas, e o `claude` escreve emoji (o ⚠️ da linha da jail): encolher a janela derrubava o
  painel com todas as sessões. Achado ao conferir o repasse do OSC 52 num PTY de 0x0; agora o
  parser e o PTY têm pelo menos 2x2, e numa janela menor a tela é cortada na borda.
- **Panic:** o hook vale pra qualquer thread: restaura o terminal, encerra as sessões e sai com
  101. Sem a saída, um panic numa thread secundária (a leitora de uma sessão, por exemplo)
  deixava o laço principal rodando e desenhando sobre o terminal restaurado (review,
  problema 3).

### CPU

Medido pelo agente fora da jail, com `/proc/<pid>/stat` (% de um núcleo), duas sessões do
`claude` real, terminal de 40x140:

| Momento | nick-panel | cada `claude` |
|---|---|---|
| ocioso (10 s) | 0,00% | 4,9% e 5,1% |
| resposta curta em andamento (3,2 s) | 0,94% | 2,5% (a ociosa) e 46,6% (a que respondia) |
| 10 resizes em 6 s | 0,83% | 11,5% e 12,0% |
| `claude` direto num PTY, ocioso (comparação) | — | 2,6% (varia de 2,6% a 4,7% entre rodadas) |

O painel não fica em laço ocupado: ocioso, ele não gasta nada, e o custo dele fica abaixo de 1%
mesmo redesenhando. O gasto é do próprio `claude`, que ocioso fica na mesma faixa dentro e
fora do painel. As coletas `coleta.sh cpu-*` do uso real não foram enviadas (item 9).

### Recomendação

**Requisito da casca levantado no uso (pedido do usuário, 2026-11-12):** o `nick` roda de
qualquer diretório e mostra todos os projetos, navegando por Projeto > Ticket > Agente, e cada
sessão abre no diretório do próprio projeto. O protótipo exige rodar de dentro do projeto e
usa um cwd só pra todas as sessões, porque a navegação com dados reais ficou fora do escopo do
NS-29. Na casca, o cwd tem de ser por sessão: é ele que o ai-jail deixa gravável e que faz o
`claude` carregar o `.mcp.json` e o `CLAUDE.md` daquele projeto. O `Session::spawn` já recebe o
cwd por sessão (`LaunchConfig`). Fica em aberto de onde vêm a lista de projetos e os caminhos.

**Seguir com Rust e ratatui na casca.** Com o `claude` e o ai-jail reais, os itens 1–6
funcionaram, o 8 funcionou com a ressalva do `kill -9` (terminal sujo, como em qualquer TUI),
e o 7 e o 9 estão pra refazer (acima). Nenhum achado até aqui pede outra pilha, e a
recomendação vale desde que o item 7 confirme o copiar com o repasse do OSC 52:
- o `claude` real roda dentro do painel sem diferença visível: cores, interação, rolagem e
  resize;
- a jail e as regras valem sem nenhuma opção reimplementada em Rust: o lançamento pelo
  launcher bash e pelo `lib/` funcionou, e nada em `lib/` mudou;
- nenhum órfão em nenhuma saída, e o painel gasta perto de zero de CPU (o custo é do `claude`);
- a pilha (ratatui + crossterm, portable-pty, vt100 + tui-term, laço guiado por evento num
  canal, sem tokio) não teve obstáculo de arquitetura; os problemas do review (colar, copiar,
  panic e encerramento) se resolveram dentro dela.

O que a casca precisa resolver, e não é motivo pra trocar de pilha:
- um atalho reservado que funcione no VS Code sem configuração (o `Ctrl+\` exige a regra do
  `keybindings.json`);
- o cwd por sessão e a navegação por todos os projetos (requisito acima);
- a dependência do `tui-term` no `vt100` 0.16: seguir a versão dele ou desenhar o
  `vt100::Screen` à mão;
- a revisão da regra de linguagem do `ABOUT.md` ("Rust no projeto"), no ticket da casca;
- o texto do `jail_agent_notice` sobre o `~/.config` (ticket à parte, da `lib/jail`);
- o que um emulador de terminal embutido tem de repassar ou filtrar: o OSC 52 (copiar), os
  controles do colar e, possivelmente, outras sequências que o `vt100` só entrega por callback
  (título, sino).

O código do protótipo continua descartável: a casca começa do zero, levando só os achados.
