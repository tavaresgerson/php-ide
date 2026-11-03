# T-UI-ED-000-01…04 — Baseline da superfície do editor (geometria, cobertura, medida)

> Evidências: DOC (este arquivo) + UT (`phpide-ui::geometry`, 15 testes)
> + PERF (bench `viewport`). Recorte: `UI-ED-000` (baseline e geometria
> antes das novas camadas do Estágio 6.2).

## 000-01 — Caracterização do comportamento atual

| Aspecto | Comportamento observado (código em `main.rs`/`editor_view.rs`) |
|---|---|
| **Caret** | Índice de caractere (`CCursor.index`) sobre a rope; `cursor_char` atualizado por frame; saltos (busca/F2/palette) usam `request_cursor` → `line_start(line0)+col0`, com clamp ao tamanho do documento. |
| **Seleção** | `cursor_range` do `TextEdit` (egui); ainda não há múltiplos carets nem colunas (Column Selection é flag visual de status bar). |
| **Scroll** | `ScrollArea::vertical()` do egui; scroll horizontal por `desired_width(f32::INFINITY)` (sem wrap). Faixas da scrollbar derivam de fração linha/total. |
| **Gutter** | Largura fixa `GUTTER_WIDTH = 46px`; números alinhados à direita; breakpoint sobre o número; marcador de bookmark na borda; destaque da linha do cursor. Hit test por `y → linha`. |
| **Tabs** | Strip com wrap horizontal; preview em itálico; pin `[P]`; dirty `*`; Close/Close Others/Pin no contexto; Closing Policy de 10 (agora configurável via `tab_limit`). |
| **Zoom** | `set_zoom` com clamp 0.5–3.0 (`clamp_zoom`, NaN → 1.0); atalhos `Ctrl+=`, `Ctrl+-`, `Ctrl+0`, `Alt+Shift+.`, `Alt+Shift+,`; persistido em `settings.json` a cada mudança; aplicado via `ctx.set_zoom_factor` (afeta fonte da IDE inteira — zoom do editor separado é P1). |

Gaps registrados (não escopo desta baseline): folding ausente (UI-ED-002),
split (UI-ED-003), wrap de linhas longas, coluna visual de tabs no caret
(hoje o `TextEdit` do egui gerencia tabulação internamente).

## 000-02 — Mapa centralizado documento → linha visual → coordenada

Fonte única: `phpide-ui::geometry::Geometry` (puro, sem egui) — espaço de
conteúdo + scroll, com conversões de tela (`origin_y - scroll + content_y`,
equivalente ao modelo `galley_pos` do egui, conforme o wrapper
`EditorGeometry`, que delega todas as fórmulas). Cobre:

- linha ↔ y de conteúdo e de tela; hit test `screen_y → linha`;
- intervalo de linhas visíveis por scroll (clampado ao documento);
- clamp/máximo de scroll (conteúdo menor que o viewport ⇒ sem scroll);
- `scroll_to_line` com margem de contexto (ancora topo/fim, sem saltos
  desnecessários quando a linha já está visível);
- largura de gutter por número de dígitos e coluna visual com tabs.

`paint_gutter` agora consome `visible_line_range` do mesmo mapa — gutter,
sticky lines e faixas da scrollbar derivam da mesma geometria do texto.

## 000-03 — Cobertura de casos (UT)

Arquivo vazio (1 linha, sem scroll), linhas longas (altura fixa por linha,
sem efeito vertical), tabs (avanço para múltiplo de `tab_width`), Unicode
(1 coluna visual por char astral; largura de glifo é da fonte), zoom
(`row_height` escalado muda a contagem de linhas visíveis; gutter fixo),
gutters de larguras diferentes (fórmula validada contra a largura fixa
real — comporta 5 dígitos; 6+ clipa com alinhamento à direita, documentado).

## 000-04 — Medição no fixture de 10 mil linhas

Bench `crates/phpide-ui/benches/viewport.rs` (debuginfo de medição com
`black_box`; viewport 800×600, linha 18px, ~33 linhas visíveis):

| Cenário | Mediana | Por unidade |
|---|---|---|
| Sweep de 600 frames (visible_lines + hit test) | 4.5 µs | **7.5 ns/frame** |
| 50 saltos "go to line" (F2/busca/palette) | 485 ns | ~9.7 ns/salto |
| Frame com 33 hit tests (pintura de gutter) | 102 ns | 3 ns/hit |

Leitura: a geometria do núcleo é **4–5 ordens de magnitude abaixo** do
orçamento de um frame a 60fps (16.6 ms) — scroll e cliques no gutter não
são gargalo; o custo perceptível de scroll está no pipeline de pixels do
egui (fora de escopo desta medida headless, como registrado no baseline
NFR-006-02).

## Zoom — auditoria T-UI-ED-010-01

- Atalhos: `Ctrl+=`/`Ctrl+-`/`Ctrl+0` e `Alt+Shift+.`/`,` (sem conflito
  com os atalhos de tool window `Alt+1…0`); roda do mouse com Ctrl é
  flag experimental `zoom_on_ctrl_wheel` (default off).
- Limites: `clamp_zoom` 0.5–3.0; NaN → 1.0; ±∞ caem nos extremos (UT).
- Persistência: `settings.zoom` gravado a cada mudança e na carga do
  projeto; roundtrip coberto por UT (inclui clamp na leitura de valores
  fora da faixa escritos à mão).
