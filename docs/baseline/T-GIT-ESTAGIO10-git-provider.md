# Baseline — Estágio 10.0: Git como provedor do editor

> Evidências do conjunto `T-GIT-*` (GIT-001-01, GIT-002-01/02/03,
> GIT-003-01, GIT-004-01/02) e do gate de integração. Data: 2026-11-07.

## Ambiente de validação

- `git` CLI real do sistema (wrap no binário, mesma política dos demais
  crates — comparação contra o Git de verdade, sem biblioteca nativa).
- Repositórios temporários (`tempfile`) criados e destruídos por teste;
  sem `git` no ambiente os testes se autopulam com aviso (NFR-003).

## Arquitetura entregue

| Módulo (`phpide-vcs`) | Incremento |
|---|---|
| `repo` (GIT-001-01) | `detect` → `RepoState::Branch{root,branch}` / `Detached{root,sha}` / erro recuperável (`InvalidInput` + ação NFR-005); worktree root resolvido por `rev-parse --show-toplevel`; bare e git ausente tratados; `label()` para widgets |
| `status` | Porcelain v1 (pares XY, untracked, unquote estilo C de paths com espaços/escapes) — base do status pós-operação |
| `diff` (GIT-002-01) | Parser de diff unificado (`---/+++`, `@@ -a,b +c,d @@` com contexto, `\ No newline`, `/dev/null`, renames) + `diff` (workdir), `diff_staged`, `diff_head` (vs HEAD, base dos markers) + diff sintético 100% adicionado para untracked |
| `ops` (GIT-002-02/03) | `stage` (`git add`), `unstage` (`git restore --staged`), `commit` — mensagem vazia rejeitada antes de executar; falha (hook) vira `IdeError` com **output do hook no detail** e o índice fica exatamente como estava |
| `log` (GIT-003-01) | `git log --format=%x1e…%x1f… --name-status` com filtros `max_count`/`path`/`rev` (rev = navegação a partir do blame). **Detalhe real:** o separador \x1e só pode ir no INÍCIO do format — com \x1e também no fim, o `--name-status` cai num chunk separado na divisão e os arquivos se perdem |
| `markers` (GIT-004-01) | Mapa linha → Added/Modified/Removed derivado do diff vs HEAD: pares del+add consecutivos viram Modified; excedente de add vira Added; excedente de del ancora na próxima linha existente (ou na última vista no hunk). Stage mantém, commit limpa — recalculado por chamada, sem cache a invalidar |
| `blame` (GIT-004-02) | Parse do `blame --porcelain` (boundary `^`, meta herdada por grupo quando o cabeçalho de continuação não a traz, linhas não commitadas com sha 0…0), agregação de consecutivos, `line_blame`, `blame_rev` — navegação ao commit via `log(rev=sha)` |

UI: `phpide-ui/src/vcs.rs` passa a delegar ao crate (header/status bar
mostram branch ou `(detached <sha>)`).

## Evidências de teste

| Camada | Onde | O que exercita |
|---|---|---|
| UT | `phpide-vcs` lib (23) | parse de unified (linhas/old/new, `/dev/null`, rename, contexto no `@@`), markers (Added/Modified/Removed, âncoras de remoção), porcelain do blame (merge de grupo, boundary, lookup), porcelain de status (XY, untracked, unquote), format do log (registros, name-status, rename R087), validação de mensagem vazia, erros recuperáveis fora do worktree |
| IT ferramenta real | `phpide-vcs/tests/git_real.rs` (7) | repo temporário real: detect (branch a partir de subdir, detached com sha, inválido), stage seletivo + status pós-operação, diff workdir/staged/HEAD + untracked sintético, commit (vazio não executa; **hook pre-commit falho preserva output e índice**; commit leva só o stageado), log com arquivos + filtros path/rev, markers acompanhando save→stage→commit, blame por autor + navegação por revisão + linha não commitada |
| Gate | `phpide-vcs/tests/gate_git.rs` (1) | narrativa completa: inválido recuperável → repo novo → diff de dois arquivos → **stage seletivo** (só a.php) → mensagem vazia não executa → **hook falha preservando output e índice** → commit real (b.php continua sujo) → markers atualizados (commit limpa, novo arquivo Added, stage mantém) → blame com navegação ao commit |

## Gate do workspace (2026-11-07)

- `cargo fmt --all -- --check`: verde.
- `cargo test --workspace --no-fail-fast`: **0 falhas** (vcs entra com
  23 UT + 7 IT + 1 gate).
- `cargo clippy --workspace --all-targets`: **0 warnings**.

## Decisões e limitações registradas

- **Markers vs HEAD** (`diff HEAD`), não vs index: stage não limpa o
  gutter; commit limpa. É o comportamento esperado por "atualizar após
  save, stage e commit" (stage é refresh, commit é resolução).
- **Sem `git2`/libgit2**: wrap do CLI mantém o projeto consistente (php,
  phpunit, LSP e Xdebug também são ferramentas reais) e o diff/log/blame
  sempre falam o formato do Git real — três bugs de formato foram achados
  assim e corrigidos no parser (chunk do `--name-status`, contexto no
  `@@`, meta de grupo no porcelain).
- **Fora do recorte** (próximos marcos, conforme TAREFAS.md): GIT-001-02
  (multi-root/refresh incremental), GIT-002-04/05/06 (stage por chunk,
  changelists/amend, commit inline), GIT-003-02/03/04 (branches popup,
  fetch/pull/push, force-with-lease), GIT-004 de renderização (UI-ED tasks)
  e GIT-005/006 (merge/rebase, GitHub/GitLab).
