# T-UI-ED-003-01 — Auditoria e consolidação de tabs

> Evidência: UT (core + UI) e DOC (este arquivo); FT manual no checklist do
> gate. Requisito: `UI-ED-003` (P0). Estado alvo: "Auditar e completar
> Closing Policy, preview, pin, Close Others, dirty confirmation e tabs
> ocultas".

## Matriz de auditoria

| Item | Estado | Evidência |
|---|---|---|
| **Preview tab** (itálico, reutilizada até edição) | ✅ | `phpide-core`: `preview_tab_is_reused_until_edited`; edição promove a permanente (`Buffer::insert` zera `preview`). |
| **Pin/unpin** | ✅ | Contexto da aba alterna `pinned`; fixadas nunca são vítimas da Closing Policy (`closing_policy_keeps_at_most_10_unpinned`). |
| **Close Others** | ✅ | `close_other_tabs` preserva fixadas; com dirty, a UI abre a confirmação (NFR-007-02) antes de fechar. |
| **Dirty confirmation** | ✅ | Modal Salvar/Não Salvar/Cancelar em aba única, Close Others, troca de projeto e fechamento da IDE. |
| **Closing Policy** | ✅ configurável | Limite era constante 10; agora `Workspace.tab_limit` aplicado a partir de `Settings.tab_limit` (default 10, clamp 1–99 na leitura) — UT `closing_policy_uses_configured_tab_limit` e roundtrip em `settings::tests`. |
| **Tabs ocultas acessíveis** | ✅ (wrap) | A strip usa `horizontal_wrapped`: com muitas abas elas quebram para a linha seguinte em vez de sumir — não há aba inacessível por falta de espaço. O Switcher `Ctrl+Tab` com MRU é T-UI-ED-003-02 (separado). |

## Fechamentos desta consolidação

- `Workspace` ganha `tab_limit` (campo, default `TAB_LIMIT`); `enforce_tab_limit`
  respeita o valor configurado; UI aplica `settings.tab_limit` na abertura e
  na troca de projeto.
- `Settings` ganha `tab_limit: u32` com default, clamp na leitura e
  persistência versionada (campo novo com default — sem bump de versão,
  conforme NFR-004).

## Fora de escopo (tarefas próprias)

- Switcher `Ctrl+Tab` com ordem MRU e roteamento: `T-UI-ED-003-02`.
- Split de editor e drag/drop de abas: `T-UI-ED-003-03/05`.
- UI de configuração do limite na página Editor (hoje editável via
  `settings.json`); entra com a página "Editor Tabs" em `T-UI-SET-001-03`.

## Checklist FT (execução manual registrada)

1. Abrir 12 arquivos com limite 10 → as 2 mais antigas não fixadas fecham.
2. Fixar a primeira e repetir → ela permanece.
3. Editar aba de preview e abrir outro arquivo → aba promovida, nova aba criada.
4. Dirty + Close/Close Others/trocar projeto/sair → modal pergunta; Salvar
   grava, Não Salvar descarta, Cancelar volta ao fluxo.
