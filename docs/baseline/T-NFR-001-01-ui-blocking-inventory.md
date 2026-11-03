# T-NFR-001-01 — Inventário de operações bloqueantes na thread de UI

> Evidência: DOC (este arquivo). Data: 2026-11-03. Requisito: `NFR-001`
> (§3.3 de `REQUISITOS.md`): a thread de interface não deve bloquear em
> filesystem extenso, rede, processos externos, LSP, Git ou SQL.

## Método

Varredura estática dos pontos de chamada de `std::fs`, `std::process` e
operações de rede em `crates/phpide-ui/src`, classificados pelo tipo de
recurso. "Thread de UI" = `eframe::App::update` e tudo o que ele invoca
por frame; workers de `JobSystem` e threads de canal estão fora da
classificação (já são o destino correto).

Legenda: ✅ já fora da UI (worker/canal) · ⚠️ na thread de UI com mitigação
· ❌ na thread de UI sem mitigação (plano abaixo).

## Inventário

### Filesystem

| Local | Operação | Classe | Estado |
|---|---|---|---|
| `Workspace::open` → `scan` (`phpide-core`, chamado em `PhpIdeApp::new` e `load_project`) | `read_dir` recursivo da árvore do projeto | filesystem extenso | ❌ roda na UI na abertura/troca de projeto; projeto grande (10k+ arquivos) trava o frame. **Plano: job `scan` + aplicação incremental (T-NFR-001-02 já entregue a base).** |
| `poll_watcher` → `ws.refresh_files` (`main.rs`) | re-`scan` completo após debounce de 300ms | filesystem extenso | ❌ mesmo caso, disparado por mudanças externas. |
| `Buffer::open` via `open_file_ex` (`phpide-core`) | `fs::read_to_string` do arquivo aberto | filesystem | ⚠️ arquivo único costuma ser pequeno; arquivo de 10–50MB geraria dezenas de ms travados. **Plano: job de leitura com placeholder na aba.** |
| `save_buffer_at` → `Buffer::save` (`phpide-core`, `fsutil::write_atomic`) | tmp + fsync + rename | filesystem (escrita) | ⚠️ escrita atômica é rápida para arquivos típicos; o fsync pode custar ms em disco lento. Aceito como está; registrar se benchmark (T-NFR-006-02) mostrar regressão. |
| `reload_clean_buffers` (via `poll_watcher`) | re-leitura de buffers afetados | filesystem | ⚠️ mesma ordem de `Buffer::open`. |
| `rss_mb` (`main.rs`, widget de memória) | `read_to_string("/proc/self/status")` por frame | filesystem (pseudo) | ⚠️ `/proc` é barato (não toca disco); cacheado seria melhor, impacto desprezível. |
| `composer_scripts` (`exec.rs`, ao abrir a palette) | `read_to_string(composer.json)` | filesystem (arquivo único) | ⚠️ pequeno e esporádico; aceito. |
| `welcome.rs` browser de pastas | `read_dir` do diretório corrente | filesystem | ⚠️ diretório único, sem recursão; aceito. |
| `plugins::discover_plugins` (no `new`) | `read_dir` + leitura de `plugin.json` | filesystem | ⚠️ no startup; aceito. |
| `settings` load/save | arquivo único de config | filesystem | ✅ atômico e esporádico (NFR-004-02). |

### Processos externos

| Local | Operação | Classe | Estado |
|---|---|---|---|
| `ExecState::run_env` (`exec.rs`) | `Command::new(...).spawn()` do worker | processo | ✅ spawn na worker thread; stdout/stderr por canal; **Stop mata o grupo de processos** (NFR-003-03). |
| `vcs::current_branch` (`main.rs::git_branch`, debounce 2s) | `git branch --show-current` síncrono | processo (Git) | ❌ spawn + espera na UI quando o cache expira (máx. 1×/2s, mas um git lento = frame travado). **Plano: job com resultado por canal + `JobRegistry` (infra pronta).** |
| `welcome.rs::clone_from_vcs` | `git clone` síncrono | processo | ❌ trava a Welcome durante o clone. **Plano: `phpide_exec::spawn_job` com progresso no canal.** |
| `debug_run` (`main.rs`) | spawn do `php` com `XDEBUG_CONFIG` | processo | ✅ em thread separada, saída por canal. |
| `start_lsp` (`main.rs::new`/`load_project`) | spawn do servidor de linguagem | processo (LSP) | ⚠️ spawn local é sub-ms; o `LspClient` segura toda a conversação em threads próprias. Aceito. |

### Rede

Nenhuma operação de rede direta na UI: o listener DBGp
(`phpide-dbgp::listen`) e o framing LSP rodam em threads dedicadas com
canais. Estado: ✅ (matriz de falhas de rede fica para T-PHP-001/T-DB-00x).

### LSP

| Local | Operação | Estado |
|---|---|---|
| `poll_lsp` (`main.rs`) | drena canais, `did_open/did_change/...` (não-bloqueantes) | ✅ sem bloqueio; respostas por `mpsc`. |
| `LspClient::spawn` | framing JSON-RPC em threads próprias | ✅. |

### Git

Ver "Processos externos": apenas `current_branch` e `clone_from_vcs` na UI
hoje. Diff/log/stage ainda não implementados (Estágio 10.0); quando
entrarem, **devem** nascer sobre `JobSystem` + `phpide_exec` (contrato desta
consolidação).

### DB

Inexistente neste recorte (shell apenas). Drivers futuros devem nascer
fora da UI com jobs canceláveis (dependência `T-NFR-001-02`, já entregue).

## Plano de correção (por prioridade)

1. **`scan`/tree refresh em job** — mover `Workspace::scan` para um worker
   do `JobSystem` (id correlacionado "tree-scan"); a UI aplica `FileEntry`s
   por canal e mantém a árvore anterior enquanto varre. Remove os dois ❌
   de filesystem extenso. Depende de: nada (infra pronta).
2. **`git_branch` em job** — `spawn_job` via `phpide_exec::CommandSpec`
   com cache por 2s; resultado obsoleto descartado por `JobRegistry`.
3. **`Buffer::open` em job** — leitura do arquivo no worker; a aba abre com
   estado "carregando" e o texto chega pelo canal anexado à aba (id de
   job por path).
4. **`git clone` da Welcome em job** — mesmo padrão, com cancelamento.
5. Manter `save`/settings/composer.json na UI (custo pontual pequeno),
   revisando após o benchmark T-NFR-006-02.

## O que já foi mitigado nesta consolidação

- Busca em arquivos roda em `JobSystem` com cancelamento e descarte de
  resultados obsoletos por id de job (T-NFR-001-02/03).
- Execução Run/Debug: spawn fora da UI, Stop mata o **grupo** de processos
  (filhos não órfãos), timeout e cancelamento disponíveis via
  `phpide_exec` (T-NFR-003-03).
- Troca de projeto e fechamento da IDE: `JobSystem::shutdown` cancela e
  encerra workers; processos e LSP da sessão são terminados
  (T-NFR-001-04).
