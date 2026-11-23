# Design tokens

Reference values for Phase 2 (`crates/phpide-ui/src/design/tokens.rs`), kept from the previous design-system document. The text below is in Portuguese, as written originally. Icons use [Tabler Icons](https://tabler.io/icons) (MIT).

## Tokens

### 4.1 Superfícies e cores semânticas

Valores iniciais obrigatórios do tema **Islands Dark**. Ajustes posteriores exigem revisão de contraste e captura da matriz visual.

| Token | Valor | Uso |
|---|---:|---|
| `color.bg.canvas` | `#101216` | Fundo geral da aplicação |
| `color.bg.panel` | `#16191F` | Tool windows e regiões estruturais |
| `color.bg.editor` | `#111318` | Superfície de código |
| `color.bg.elevated` | `#1D222B` | Dialogs, palette e popovers |
| `color.bg.inset` | `#0D0F13` | Terminal, console e áreas inset |
| `color.bg.hover` | `#222832` | Hover discreto |
| `color.bg.pressed` | `#29313D` | Pressed |
| `color.bg.selected` | `#26364D` | Item selecionado sem foco |
| `color.bg.selected_focused` | `#2E4668` | Item selecionado com foco |
| `color.border.subtle` | `#282E38` | Divisórias e separadores |
| `color.border.strong` | `#3A4350` | Inputs, foco estrutural e splitter ativo |
| `color.text.primary` | `#DCE2EA` | Texto principal |
| `color.text.secondary` | `#A5AFBC` | Labels e detalhes |
| `color.text.muted` | `#7D8794` | Metadados e placeholders |
| `color.text.disabled` | `#59616C` | Desabilitado, ainda identificável |
| `color.accent.default` | `#6C9CFF` | Ação e seleção principal |
| `color.accent.hover` | `#82ACFF` | Hover de acento |
| `color.focus.ring` | `#8CB4FF` | Focus ring de 2 px |
| `color.success` | `#63B995` | Sucesso |
| `color.warning` | `#E2B93D` | Aviso |
| `color.error` | `#EF6A73` | Erro e ação destrutiva |
| `color.info` | `#65B5E8` | Informação |

Tokens adicionais obrigatórios: `git.added`, `git.modified`, `git.deleted`, `git.conflict`, `debug.breakpoint`, `debug.current_line`, `coverage.full`, `coverage.partial`, `coverage.none`, `diff.added_bg`, `diff.removed_bg`, `search.match_bg`, `search.current_match_bg`, `editor.current_line_bg`, `editor.caret`, `editor.selection_bg`, `editor.indent_guide`, `editor.sticky_bg` e `editor.sticky_shadow`.

**Islands Light não é uma inversão do Dark.** Deve possuir valores próprios para canvas, panels, editor, textos, bordas, seleção e syntax highlighting. High Contrast deve priorizar legibilidade e foco, não apenas amarelo sobre preto.

### 4.2 Sintaxe do editor

O tema de sintaxe é separado do tema da shell e usa papéis semânticos: keyword, type, function, method, variable, parameter, property, string, number, comment, documentation, operator, punctuation, attribute, constant, namespace, invalid e deprecated.

Regras:

- comentários têm contraste legível e não desaparecem no fundo;
- erro/warning não alteram a cor do token a ponto de destruir o highlighting;
- no máximo um peso/estilo adicional por papel sem justificativa;
- strings, keywords e tipos devem ser distinguíveis sem saturar toda a linha;
- Dark, Light e High Contrast têm snapshots dos mesmos arquivos PHP, JS, CSS, HTML e SQL.

### 4.3 Tipografia

| Token | Tamanho/estilo padrão | Uso |
|---|---|---|
| `font.ui.family` | Inter ou fallback métrico disponível | Shell |
| `font.ui.body` | 13 px, regular, line-height 18 px | Texto geral |
| `font.ui.label` | 12 px, medium | Labels e metadados interativos |
| `font.ui.small` | 11 px, regular | Informação não essencial; nunca texto principal longo |
| `font.ui.title` | 16 px, semibold | Títulos de páginas/dialogs |
| `font.ui.display` | 22 px, semibold | Welcome e estados hero discretos |
| `font.mono.family` | JetBrains Mono ou fallback métrico | Editor, terminal e dados |
| `font.editor.body` | 14 px, line-height 21–22 px | Código |
| `font.editor.gutter` | 12 px | Números de linha |
| `font.shortcut` | 11 px, regular | Atalhos em menus e keycaps |

Requisitos:

- fontes ficam empacotadas ou têm fallback validado; nenhum glifo quebrado;
- IDE zoom afeta toda a shell de forma proporcional;
- editor font zoom pode ser independente;
- nomes de arquivos, caminhos e código devem suportar acentos e Unicode real;
- não usar texto pequeno para resolver falta de espaço.

### 4.4 Espaçamento, dimensões e densidade

Escala: `space.0=0`, `space.1=2`, `space.2=4`, `space.3=6`, `space.4=8`, `space.5=12`, `space.6=16`, `space.7=20`, `space.8=24`, `space.9=32` px.

| Elemento | Comfortable | Compact |
|---|---:|---:|
| Window header | 40 px | 36 px |
| Tab strip | 34 px | 30 px |
| Tool window header | 32 px | 28 px |
| Tool window rail | 40 px | 36 px |
| Tree/list row | 28 px | 24 px |
| Input/button padrão | 30 px | 28 px |
| Status bar | 26 px | 24 px |
| Icon button | 28×28 px | 24×24 px |
| Ícone padrão | 16 px | 16 px |
| Ícone de header/projeto | 20 px | 18 px |

Padding padrão:

- tool window: 8 px; conteúdo respirável: 12 px;
- dialog: 20–24 px;
- popover/menu: 6–8 px;
- gap entre controles relacionados: 6–8 px;
- gap entre grupos: 12–16 px;
- área clicável mínima: 24×24 px no Compact e 28×28 px no Comfortable.

### 4.5 Forma, borda e elevação

- `radius.control = 4 px`;
- `radius.card = 6 px`;
- `radius.dialog = 8 px`;
- editor, terminal e grades não recebem arredondamento decorativo interno;
- borda padrão é sutil e não deve cercar cada label/status;
- botões ghost não têm preenchimento permanente;
- sombras somente em overlays, sticky lines e arraste;
- foco usa ring externo, não apenas mudança de cor do texto;
- splitters têm hit area maior que a linha visível e cursor adequado.

### 4.6 Movimento

- hover: 80 ms;
- seleção e tabs: 120 ms;
- abertura de popover/dialog: 120–160 ms;
- tool windows: até 180 ms, sem animar o conteúdo do editor;
- nenhuma animação contínua decorativa;
- spinner somente durante trabalho real;
- Reduced Motion remove transições não essenciais sem remover feedback de estado.

