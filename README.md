# phpide

IDE de desenvolvimento em **Rust**, com foco em **PHP**, para Linux (X11/Wayland).
Construído do zero sobre código aberto — sem nenhum componente proprietário.
Arquitetura inspirada nos princípios do guia `../docs/como-construir-um-ide.md`.

## Executar

```bash
cd phpide
cargo run --release -- /caminho/do/projeto-php [arquivos...]
```

(sem argumento, abre a **Welcome Screen**; arquivos extras são abertos em abas).
Atalhos: `Ctrl+S` salva, `Ctrl+Z`/`Ctrl+Shift+Z` desfaz/refaz.

## Estrutura (workspace)

| Crate | Responsabilidade | Equivalente clássico |
|---|---|---|
| `phpide-core` | `Document` (rope), `Buffer` (aba + undo), `Workspace`, árvore de arquivos, diagnósticos | Document/FileEditorManager/ProblemsView |
| `phpide-php` | Linguagem: metadados PHP, highlight (syntect), tipos de símbolo | Language/ParserDefinition |
| `phpide-lsp` | Cliente LSP: framing JSON-RPC, threads, sync de documentos, completion | LanguageServerManager |
| `phpide-syntax` | Análise estrutural tree-sitter: blocos, erros de sintaxe offline | LocalFileSystem/Folding (offline) |
| `phpide-dbgp` | Protocolo DBGp (Xdebug): breakpoints, run, pilha, variáveis | XDebugger/DBGp driver |
| `phpide-ui` | Shell gráfica egui/eframe: abas, painéis, editor, execução, plugins | IdeFrame |

## Roadmap por estágios

- [x] **Estágio 0 — Fundação**: workspace Cargo, 3 crates, build verde no Linux.
- [x] **Estágio 1 — Editor MVP**: rope com edições mínimas (diff prefixo/sufixo), abas com
      dirty/undo/redo, árvore de arquivos com colapso (ignora `.git/vendor/node_modules`),
      highlight PHP via syntect, salvar com Ctrl+S, barra de status com Ln/Col.
- [x] **Estágio 2 — Shell de IDE**: command palette com fuzzy match (Ctrl+P), busca em
      arquivos em thread separada (Ctrl+Shift+F), painel de problemas (Ctrl+Shift+M),
      temas claro/escuro (hoje: Quick Switch Scheme), zoom (Ctrl+= / Ctrl+- / Ctrl+0), watcher de arquivos
      (recarrega buffers sem edição local, atualiza a árvore).
- [x] **Estágio 3 — Inteligência PHP**: crate `phpide-lsp` (cliente LSP próprio, JSON-RPC
      sobre stdio, sem runtime async) rodando **intelephense** ou **phpactor** como processo
      externo: diagnostics no painel de problemas, ir para definição (F12), outline
      (documentSymbol) no painel Estrutura, sync didOpen/didChange/didSave/didClose,
      indicador de status na barra.
- [x] **Estágio 3.1 — Completamento e estrutura**: popup de completamento LSP
      (Ctrl+Espaço; gatilhos `$`, `->`, `\`) com campo de filtro próprio; crate
      `phpide-syntax` (tree-sitter-php) com blocos estruturais, saltos Ctrl+[ / Ctrl+]
      e diagnósticos de sintaxe offline quando o LSP está fora.
- [x] **Estágio 4 — Executar & depurar**: painel Execução (thread + canal) para
      `php arquivo.php` (Ctrl+R), scripts do `composer.json`, PHPUnit; crate
      `phpide-dbgp` com o protocolo DBGp do Xdebug (escuta porta 9003, breakpoints F9,
      run, pilha e variáveis ao parar, detach) — validado com um "xdebug falso" em socket.
- [x] **Estágio 5 — Extensões**: plugins por script (`plugin.json` em
      `.phpide/plugins/` ou `~/.config/phpide/plugins/`) com comandos na palette;
      tema e zoom persistentes em `settings.json`. WASM fica como evolução documentada.
- [x] **Estágio 6 — Shell de IDE (paridade visual New UI, REQUISITOS Parte 1)**:
      window header unificado (main menu ☰, widget de Projeto, widget VCS com
      branch git, widget Run); tool windows com nomes/atalhos idênticos ao
      PhpStorm (Project `Alt+1`, Find `Alt+3`, Run `Alt+4`, Debug `Alt+5`,
      Problems `Alt+6`, Structure `Alt+7`, Hide All `Ctrl+Shift+F12`) + tool
      window bars laterais e base; status bar com widgets (PHP Language Level,
      Ln:Col → Go to Line, LF/CRLF, UTF-8, indent, branch VCS, ☎ debug listen,
      read-only, RSS); Search Everywhere (`Shift×2`), Go to File (`Ctrl+Shift+N`),
      Recent Files (`Ctrl+E`), Find Action (`Ctrl+Shift+A`, ou `>` no campo);
      Quick Switch Scheme (``Ctrl+` ``) com temas Islands Dark/Light/Darcula/Light;
      Welcome Screen (Projects/Customize, projetos recentes, browser de pastas,
      New Project com esqueleto composer.json); editor tabs com preview (itálico),
      pin, Close Others, Closing Policy de 10; breadcrumbs estruturais clicáveis
      (tree-sitter, offline); F2/Shift+F2 entre problemas; Alt+←/→ no histórico
      de navegação; debug zero-config (listener DBGp + `XDEBUG_CONFIG` automático);
      Stop `Ctrl+F2`. Ver `../docs/roadmap-paridade-phpstorm.md`.

## Atalhos

| Tecla | Ação |
|---|---|
| Shift×2 / Ctrl+P | Search Everywhere |
| Ctrl+Shift+N / Ctrl+E / Ctrl+Shift+A | Go to File / Recent Files / Find Action |
| Ctrl+S / Ctrl+Z / Ctrl+Shift+Z | salvar / desfazer / refazer |
| Ctrl+N | novo arquivo |
| Ctrl+F4 | fechar aba |
| Alt+1 / Alt+3..7 | toggle tool windows (Project, Find, Run, Debug, Problems, Structure) |
| Ctrl+Shift+F12 | Hide All Tool Windows |
| Ctrl+Shift+F | busca em arquivos (tool window Find) |
| F2 / Shift+F2 | próximo / anterior problema |
| Alt+← / Alt+→ | navegação back/forward |
| Ctrl+Espaço | completamento (e gatilhos `$`, `->`, `\`) |
| F12 / Ctrl+B | ir para definição |
| Ctrl+[ / Ctrl+] | início/fim do bloco estrutural |
| Ctrl+R / Ctrl+D | executar arquivo atual / debug com Xdebug |
| Ctrl+F2 | parar execução/debug |
| F9 | breakpoint na linha do cursor |
| Ctrl+` | Quick Switch Scheme |
| Ctrl+= / Ctrl+- / Ctrl+0 | zoom |

## Servidor de linguagem (LSP)

A IDE procura o servidor nesta ordem (`crates/phpide-lsp`):

1. Variável de ambiente `PHPIDE_LSP` (ex.: `PHPIDE_LSP="phpactor language-server"`)
2. `third_party/node_modules/.bin/intelephense` — instale localmente com:
   `npm --prefix third_party install intelephense`
3. `intelephense` no PATH
4. `phpactor` no PATH (open source, MIT — alternativa ao intelephense)

Verificação rápida do stack LSP (sobe intelephense real e espera diagnósticos):

```bash
cargo test -p phpide-lsp
```

## Decisões técnicas do Estágio 1

- **Rope** (`ropey`): edições e medição de linha em O(log n); o `TextEdit` do egui devolve
  o texto inteiro, então reduzimos a diferença a `(offset, removidos, inserido)` e aplicamos
  na rope — a String é só o adaptador do widget.
- **Undo por snapshots** com teto de 128 estados (simples e correto; otimizar para
  operações invertíveis no Estágio 2).
- **Highlight** com syntect (lexer declarativo estilo Sublime). No Estágio 3, análise
  semântica (LSP) sobrepõe cores por uso, como nos IDEs clássicos.
- **Sem threads ainda**: tudo síncrono e pequeno. LSP e `notify` introduzem async no Estágio 2/3.

## Licença

MIT OR Apache-2.0 (código próprio). Dependências sob suas próprias licenças (ver `cargo tree`); intelephense é freeware (instalado localmente via npm, não redistribuído).
