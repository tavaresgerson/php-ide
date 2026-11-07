# PHPForge IDE — Especificação de produto e engenharia

> **Visão de referência:** PhpStorm 2026.2, complementada por fluxos de MySQL Workbench, DBeaver e VS Code.
> **Implementação:** Rust, `egui`/`eframe`, com componentes open source e processos externos especializados.
> **Estado do documento:** especificação viva; deve ser atualizado no mesmo commit da funcionalidade correspondente.
> **Recorte em andamento:** **Estágio 6.2 — superfície do editor**.

Este documento tem três finalidades:

1. descrever a visão completa do produto sem confundi-la com o escopo imediato;
2. transformar cada incremento em trabalho implementável e verificável;
3. estabelecer um fluxo de engenharia que permita a desenvolvedores e agentes de IA trabalharem sem perder arquitetura, contexto ou qualidade.

A referência ao PhpStorm descreve comportamento, organização e fluxos esperados. Não implica copiar código, assets proprietários ou reproduzir recursos sem passar pelos critérios de aceite deste documento.

## 1. Como interpretar esta especificação

### 1.1 Regras normativas

- **DEVE**: obrigatório para o requisito ser considerado concluído.
- **DEVE, quando habilitado**: obrigatório apenas na configuração indicada.
- **PODE**: comportamento opcional, sem bloquear o aceite.
- **NÃO FAZ PARTE**: fora do recorte; não deve ser implementado incidentalmente.
- **P0**: fluxo essencial, necessário para uma versão útil.
- **P1**: evolução importante após o fluxo essencial estar estável.
- **P2**: paridade avançada ou recurso especializado.

Um item só pode ser marcado como concluído quando sua implementação, seus testes e sua documentação estiverem disponíveis. A existência de uma janela, botão ou configuração sem o comportamento associado **não** caracteriza conclusão funcional.

### 1.2 Identificação e rastreabilidade

Cada requisito possui um identificador estável:

- `UI`: shell, aparência e superfície do editor;
- `PHP`: edição e inteligência PHP;
- `RUN`: execução, depuração, testes e HTTP;
- `FE`: frontend;
- `DB`: banco de dados e SQL;
- `GIT`: controle de versão;
- `TOOL`: terminal, deployment, containers e histórico;
- `FW`: frameworks;
- `AI`: recursos de IA;
- `NFR`: requisitos não funcionais.

Testes, commits e documentação devem referenciar o identificador quando isso ajudar na rastreabilidade. Exemplo: `UI-ED-003`.

### 1.3 Estados de planejamento

| Estado | Significado |
|---|---|
| Proposto | Visão registrada, ainda sem prontidão para implementação |
| Pronto | Escopo, dependências e aceite definidos |
| Em andamento | Existe trabalho ativo; alterações devem respeitar o recorte |
| Bloqueado | Dependência ou decisão impede continuar |
| Concluído | Implementação e Definition of Done atendidas |

Estados de planejamento não substituem evidências de teste. Ao concluir um requisito, registrar no README ou no roadmap o incremento correspondente.

## 2. Visão, objetivos e limites

### 2.1 Visão

Construir uma IDE nativa para PHP e frontend que una:

- edição e navegação de código comparáveis ao PhpStorm;
- execução, testes e depuração integrados;
- administração visual de bancos relacionais, com fluxos inspirados em Workbench e DBeaver;
- interface responsiva e extensível, com a simplicidade de uso associada ao VS Code.

### 2.2 Resultados que definem valor para o usuário

O produto deve priorizar estes fluxos completos:

1. abrir um projeto Composer, editar PHP, obter diagnósticos e navegar pelo código;
2. executar e depurar um script ou teste sem configuração manual excessiva;
3. trabalhar com HTML, CSS e JavaScript no mesmo projeto;
4. inspecionar mudanças, criar commits e consultar histórico Git;
5. conectar-se a um banco, explorar o schema e executar consultas com resultados paginados;
6. editar dados com preview de DML, transações e possibilidade explícita de rollback.

Uma tela adicional sem melhorar um desses fluxos tem prioridade menor que a estabilização de um fluxo existente.

### 2.3 Não objetivos do núcleo inicial

Não fazem parte dos primeiros fluxos estáveis:

- reimplementar um mecanismo semântico PHP completo dentro da IDE;
- suportar todos os SGBDs, frameworks ou serviços de nuvem simultaneamente;
- oferecer marketplace público, agentes de IA ou colaboração em tempo real;
- reproduzir todos os recursos avançados do PhpStorm antes de validar os fluxos P0;
- executar código, consultas ou operações remotas implicitamente sem ação do usuário.

### 2.4 Ambiente de referência

- Linux X11 como ambiente inicial obrigatório; Wayland deve ser validado quando houver suporte explícito.
- Rust edition 2021 e workspace Cargo definido na raiz.
- PHP CLI e Composer externos, com caminhos configuráveis.
- Servidores de linguagem e demais ferramentas instalados separadamente, salvo empacotamento explícito.
- Configurações legíveis e versionáveis sempre que não contiverem segredos.

Suporte adicional a macOS e Windows deve ser planejado por requisito próprio, evitando assumir que caminhos, processos e atalhos do Linux funcionam em outras plataformas.

## 3. Arquitetura obrigatória

### 3.1 Responsabilidades dos crates

| Crate | Responsabilidade | Não deve conter |
|---|---|---|
| `phpide-core` | Documentos, rope, buffers, workspace, arquivos, diagnósticos e histórico de navegação | Widgets, chamadas diretas a servidores ou SQL |
| `phpide-php` | Metadados e recursos específicos de PHP, highlight e convenções da linguagem | Estado visual e gerenciamento de janelas |
| `phpide-lsp` | JSON-RPC, framing, ciclo de vida LSP, sincronização e adaptação de tipos | Renderização e regras de apresentação |
| `phpide-syntax` | Tree-sitter, estrutura, blocos, folding e diagnósticos locais | Semântica completa de tipos PHP |
| `phpide-dbgp` | DBGp/Xdebug, breakpoints, comandos, frames e variáveis | Painéis e controles `egui` |
| `phpide-ui` | Shell, editor visual, tool windows, comandos e coordenação da aplicação | Regras de protocolo ou domínio duplicadas |

Novos módulos, como banco de dados, devem nascer fora de `phpide-ui` sempre que tiverem protocolo, persistência ou regra de negócio própria. A UI consome interfaces e tipos do domínio.

### 3.2 Direção das dependências

- A UI pode depender dos serviços do domínio; o domínio não pode depender da UI.
- Protocolos externos devem ser isolados por adaptadores.
- Tipos compartilhados devem pertencer ao crate de mais baixo nível semanticamente adequado.
- Não criar dependências circulares para facilitar acesso ao estado global.
- Um recurso deve preferir comandos e eventos explícitos a mutações arbitrárias de estado entre painéis.

### 3.3 Concorrência e responsividade

`NFR-001` — A thread de interface não deve bloquear em filesystem extenso, rede, processos externos, LSP, Git ou consultas SQL.

- Operações longas devem executar em workers e retornar resultados por canais ou mecanismo equivalente.
- Toda operação longa deve expor estado: ociosa, em execução, concluída, cancelada ou com erro.
- Resultados atrasados devem ser associados a documento, revisão, sessão ou consulta; resultados obsoletos devem ser descartados.
- Fechar um projeto deve cancelar ou isolar trabalhos pertencentes ao projeto anterior.
- O encerramento da IDE deve terminar processos-filhos gerenciados por ela.

### 3.4 Documentos e posições

`NFR-002` — A conversão entre índices de caractere, bytes, linhas e unidades LSP deve ser centralizada e testada.

- A rope é a fonte de verdade do texto editável.
- Edições devem manter caret, seleção, undo/redo e dirty state consistentes.
- LSP usa o encoding negociado; não assumir que offset UTF-16, byte UTF-8 e índice Unicode são equivalentes.
- Arquivos alterados externamente não podem sobrescrever silenciosamente edições locais.
- Diagnósticos, folding, breakpoints e breadcrumbs devem ser reanexados ou invalidados após mudanças de texto.

### 3.5 Processos externos e ferramentas

`NFR-003` — PHP, Composer, servidores LSP, Git, clientes SQL e utilitários devem ter descoberta, configuração e falhas observáveis.

- A descoberta deve respeitar configuração explícita antes do `PATH`.
- Ausência de ferramenta deve produzir orientação acionável, não panic nem falso sucesso.
- Argumentos devem ser passados como vetor; não concatenar entrada do usuário em comandos shell, exceto no Terminal explícito.
- Stdout, stderr, código de saída e cancelamento devem ser tratados separadamente.
- Credenciais nunca devem ser incluídas em logs.

### 3.6 Configuração e segredos

`NFR-004` — Preferências devem ter defaults compatíveis, migração e persistência atômica.

- Configuração inválida deve gerar erro recuperável e preservar o arquivo anterior quando possível.
- Campos novos precisam de default e teste de roundtrip.
- Configurações do projeto devem ser separadas das preferências globais.
- Senhas, tokens e chaves privadas devem usar keyring do sistema ou mecanismo dedicado; arquivos versionáveis guardam apenas referências.
- Exportação de configurações deve excluir segredos por padrão.

### 3.7 Erros, logs e observabilidade

`NFR-005` — Toda falha deve indicar o componente, a operação e uma ação possível.

- Mensagens ao usuário devem ser curtas; detalhes técnicos ficam disponíveis no log.
- Não apagar uma mensagem útil ao receber evento repetido de status.
- Protocolos devem ter logs com redaction de código, consultas e segredos conforme a sensibilidade.
- Operações concorrentes precisam de identificadores correlacionáveis.

## 4. Fluxo maduro de desenvolvimento

### 4.1 Unidade de trabalho

A unidade recomendada é **um requisito ou sub-requisito vertical**, não uma parte inteira da visão. Exemplos adequados:

- `UI-ED-002`: folding estrutural com Tree-sitter;
- `PHP-003`: completion básica usando servidor configurado;
- `DB-004`: execução de seleção com cancelamento e paginação.

Exemplos excessivamente amplos:

- “terminar o editor”;
- “implementar banco de dados”;
- “fazer paridade PHP”;
- “melhorar toda a UI”.

### 4.2 Definition of Ready

Antes de programar, a tarefa deve responder:

1. Qual requisito e prioridade estão sendo atendidos?
2. Qual fluxo do usuário ficará funcional?
3. Quais arquivos/crates provavelmente serão alterados?
4. Quais dependências já existem e foram verificadas?
5. Quais comportamentos estão explicitamente fora do escopo?
6. Quais testes demonstram sucesso e regressão?
7. Existe outra pessoa ou agente trabalhando nos mesmos arquivos?

Se essas respostas não estiverem claras, primeiro refine a especificação. Não compense falta de contexto ampliando a implementação.

### 4.3 Sequência obrigatória de execução

1. **Inspecionar:** ler o requisito, arquitetura, código e testes existentes.
2. **Estabelecer baseline:** verificar branch, diff e testes relevantes antes das alterações.
3. **Planejar:** descrever a menor alteração capaz de satisfazer o aceite.
4. **Implementar:** preservar contratos e evitar refactors oportunistas.
5. **Verificar:** executar testes de unidade, integração e fluxo real aplicáveis.
6. **Revisar:** examinar o diff, arquivos não relacionados, warnings e limitações.
7. **Documentar:** atualizar README, configuração e esta especificação quando necessário.
8. **Entregar:** informar requisito, comportamento, evidências e pendências reais.

Commit e push são operações distintas. Commit deve representar um incremento coeso; push ou alteração do repositório remoto exige solicitação do responsável pelo projeto.

### 4.4 Regras para agentes de IA

- Não substituir arquitetura ou dependências sem um ADR e aprovação.
- Não marcar requisito como concluído com base apenas em compilação.
- Não apresentar shell visual como funcionalidade de domínio concluída.
- Não executar formatação em massa que altere arquivos fora do escopo.
- Não descartar alterações existentes nem assumir que o working tree está limpo.
- Não criar mocks que contornem justamente o comportamento que o teste deveria validar.
- Não esconder falhas de ambiente: diferenciar ausência de ferramenta, timeout, erro de permissão e falha de implementação.
- Em trabalho concorrente, limitar-se aos arquivos atribuídos e comunicar pontos de integração.
- Se o escopo precisar mudar, atualizar primeiro o plano e o requisito afetado.

### 4.5 Definition of Done

Um requisito está concluído somente quando:

- todos os critérios de aceite obrigatórios passam;
- testes novos e existentes relevantes passam;
- erros e estados vazios foram tratados;
- não há warnings novos atribuíveis à alteração;
- a UI permanece responsiva no fluxo afetado;
- a documentação distingue o que funciona do que ainda é placeholder;
- o diff contém somente mudanças relacionadas;
- as limitações restantes estão registradas com identificador próprio.

## 5. Estratégia de verificação

### 5.1 Pirâmide de testes

| Camada | Uso obrigatório |
|---|---|
| Unidade | Rope, offsets, settings, parsing, comandos, serialização e regras puras |
| Contrato | Adaptadores LSP, DBGp, Git e drivers com mensagens representativas |
| Integração | Processos e servidores reais ou simulados com comportamento protocolar fiel |
| Fluxo | Editar/salvar, completion, debug, commit e query console |
| UI smoke | Inicialização, Welcome, projeto aberto, Settings e superfície do editor |
| Não funcional | Arquivos grandes, projetos grandes, cancelamento, memória e reconexão |

Testes simulados devem cobrir protocolo e falhas; pelo menos um teste de integração deve cobrir a ferramenta real quando ela for requisito do fluxo.

### 5.2 Matriz mínima de falhas

Cada integração externa deve testar, quando aplicável:

- executável não encontrado;
- falha ao iniciar;
- desconexão durante a operação;
- resposta inválida ou incompleta;
- timeout e cancelamento;
- projeto/documento fechado antes da resposta;
- permissão negada;
- caminho com espaços e caracteres Unicode.

### 5.3 Gate básico do workspace

Os comandos abaixo são o gate de referência, complementados pelos testes do requisito:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets
```

Clippy pode ser introduzido progressivamente se o baseline ainda contiver ocorrências; nesse caso, registrar o baseline e impedir warnings novos. Formatação de entrega deve evitar alterar código não relacionado.

### 5.4 Evidências por domínio

- **UI:** captura ou teste visual do estado inicial e do estado alterado; navegação por teclado quando aplicável.
- **LSP:** sincronização com edição, resposta atrasada e servidor indisponível.
- **DBGp:** sessão simulada completa e sessão Xdebug real no ambiente de validação.
- **Git:** repositório temporário, sem tocar no repositório do desenvolvedor.
- **DB:** banco descartável, rollback de testes e nenhum dado de produção.
- **Performance:** medir antes de otimizar e registrar hardware, massa de dados e limiar.

## 6. Interface do usuário

### 6.1 Shell principal

`UI-001` — **Window header e Navigation Bar — P0**

A janela principal deve conter:

- Main Menu;
- Project widget;
- VCS widget com branch e ações disponíveis;
- Run widget;
- editor central e tool window bars;
- status bar;
- Navigation Bar na parte inferior por padrão, com alternativas no topo e oculta.

A Navigation Bar deve refletir o projeto e o arquivo ativo, permitir foco por `Alt+Home` quando essa interação estiver implementada e não ocupar espaço quando estiver oculta.

**Aceite mínimo:** abrir projeto, trocar de arquivo, alterar a posição da Navigation Bar e persistir a preferência sem sobrepor editor ou status bar.

`UI-002` — **Main Menu — P1**

Deve oferecer os modos Merge with Main Toolbar, Hamburger (`Alt+\`) e Show above. O menu deve expor somente ações existentes como habilitadas; ações futuras devem ser identificadas ou omitidas, nunca aparentar execução concluída.

### 6.2 Tool windows

`UI-TW-001` — **Catálogo e atalhos — P0**

| Tool window | Atalho | Tool window | Atalho |
|---|---|---|---|
| Project | `Alt+1` | Services | `Alt+8` |
| Bookmarks | `Alt+2` | Version Control | `Alt+9` |
| Find | `Alt+3` | Commit | `Alt+0` |
| Run | `Alt+4` | Terminal | `Alt+F12` |
| Debug | `Alt+5` | Database | `Ctrl+Alt+3` |
| Problems | `Alt+6` | Notifications | Sem atalho padrão |
| Structure | `Alt+7` | Remote Host | Sem atalho padrão |

**Aceite mínimo:** cada atalho abre ou fecha a janela correta; não há dois destinos para o mesmo atalho; o conteúdo e o título correspondem à janela selecionada.

`UI-TW-002` — **Gestão de janelas — P0/P1**

P0:

- Hide Active com `Shift+Escape`;
- Hide All com `Ctrl+Shift+F12`, restaurando o layout anterior;
- More tool windows;
- Compact Mode;
- header com Options e Hide;
- foco previsível entre editor e tool window.

P1:

- split por arrasto;
- redimensionamento por `Ctrl+Alt+Shift+setas`;
- speed search em árvores;
- Group Tabs;
- exibição de ícones sempre ou apenas no hover.

**Aceite mínimo de foco:** `F12` leva foco à última tool window, `Escape` devolve foco ao editor e nenhum atalho de edição é disparado enquanto um diálogo modal captura o teclado.

### 6.3 Status bar

`UI-SB-001` — **Widgets essenciais — P0**

A barra deve apresentar, conforme o contexto:

- Processes e mensagens/notificações;
- PHP Language Level;
- Line:Column;
- line endings;
- encoding;
- Column Selection Mode;
- indentação;
- branch Git;
- read-only/read-write;
- listener PHP Debug;
- deployment server;
- memória.

Cada widget deve refletir estado real ou indicar indisponibilidade. Um seletor visual não pode afirmar que alterou interpretador, encoding ou deployment sem executar e confirmar a alteração.

**Aceite mínimo:** caret e encoding acompanham o buffer ativo; branch acompanha o repositório; listener acompanha a sessão DBGp; cliques abrem a ação ou configuração correspondente.

`UI-SB-002` — **Configuração — P1**

Clique direito deve permitir mostrar/ocultar widgets e restaurar defaults. A ordem e a visibilidade devem persistir.

### 6.4 Welcome, Settings e aparência

`UI-WEL-001` — **Welcome Screen — P0**

Seções: Projects, Customize, Plugins, Learn e Remote Development.

- Projects: New Project, Open, Get from VCS e recentes.
- Customize: tema, keymap, fonte/zoom e importação.
- Plugins: instalados e estado do marketplace, sem simular catálogo remoto.
- Remote Development: somente caminhos montados ou backends efetivamente suportados.

**Aceite mínimo:** criar/abrir/clonar em diretório temporário; entradas inválidas não sobrescrevem projetos; projeto removido aparece como indisponível; importação preserva valores importados após aplicar tema e zoom.

`UI-SET-001` — **Settings — P0/P1**

`Ctrl+Alt+S` abre Settings com busca e árvore contendo:

- Appearance & Behavior;
- Keymap;
- Plugins;
- Editor;
- PHP;
- Languages & Frameworks;
- Version Control;
- Build, Execution, Deployment;
- Tools;
- Project Structure;
- Advanced Settings.

P0 exige páginas reais para aparência, keymap, editor, fonte/zoom e PHP. Páginas futuras devem ser identificadas como módulo pendente. P1 implementa as subcategorias associadas aos requisitos funcionais correspondentes.

**Aceite mínimo:** busca direciona a uma página existente; Cancel não persiste alterações; Apply persiste sem fechar; OK persiste e fecha; Escape fecha sem aplicar alterações pendentes.

`UI-THEME-001` — **Temas e acessibilidade — P0/P1**

Temas UI: Islands Dark, Islands Light, Islands Darcula, Dark, Light, High Contrast e Darcula.

- P0: seleção, persistência e contraste consistente entre painéis.
- P1: Color Schemes independentes, Sync with OS, fontes, ligatures e ajustes para daltonismo.
- Texto deve permanecer legível sem depender de glifos ausentes na fonte.
- Informação não pode ser transmitida somente por cor.

`UI-ICON-001` — **Sistema de iconografia e obtenção de assets — P1**

A IDE deve possuir um catálogo interno de ícones para menus, tool windows, botões, gutter, tabs, status bar, notificações, explorers e demais ações. O código de apresentação deve referenciar significados semânticos — por exemplo, `Run`, `Debug`, `Stop`, `Project`, `GitBranch`, `Settings`, `Warning` e `Close` — e não nomes de arquivos espalhados pelos widgets.

Fontes de assets permitidas:

- **[Icones](https://icones.js.org/)**, como catálogo de pesquisa e seleção baseado no ecossistema Iconify;
- SVGs próprios produzidos para o projeto;
- outros catálogos open source, desde que licença, autoria e redistribuição sejam verificadas;
- composições específicas de linguagens, frameworks, SGBDs e tipos de arquivo quando um conjunto geral não oferecer o símbolo necessário.

Regras de aquisição e distribuição:

- Baixar somente os SVGs efetivamente utilizados, preferencialmente durante o desenvolvimento, e versioná-los em um diretório de assets da UI.
- A aplicação não pode depender de acesso ao Icones, ao Iconify ou a outro serviço em tempo de execução.
- Cada conjunto adotado deve registrar nome do ícone, origem, autor, URL e licença em manifesto; avisos de atribuição devem ser incluídos quando exigidos.
- A licença precisa permitir o uso e a redistribuição pretendidos. Ícones com restrições incompatíveis não devem ser incorporados.
- A escolha inicial deve privilegiar um único conjunto coerente para ações gerais; conjuntos adicionais devem ser reservados a domínios específicos.
- Assets de terceiros não devem ser modificados de forma a violar sua licença ou remover atribuições obrigatórias.

Regras de renderização e interação:

- Ícones monocromáticos devem herdar cores do tema; ícones de estado devem combinar cor, forma e/ou texto.
- O catálogo deve prever tamanhos de 16, 20 e 24 px, HiDPI, alinhamento e áreas de clique adequadas.
- SVGs podem ser rasterizados em build/cache ou renderizados por biblioteca compatível; a decisão deve evitar alocações repetidas por frame.
- Todo botão identificado apenas por ícone deve possuir tooltip e nome acessível.
- Ícone ausente ou inválido deve gerar fallback controlado, nunca panic, quadrado de glifo ou espaço sem ação compreensível.
- Emojis e caracteres Unicode decorativos não devem ser o mecanismo principal de iconografia.

**Aceite mínimo:** a IDE abre offline com todos os ícones do catálogo; os mesmos comandos usam o mesmo símbolo em menus e painéis; contraste e escala são verificados em Islands Dark, Islands Light e High Contrast; existe fallback textual; o inventário de licenças cobre todos os assets incorporados.

**Integração com o Estágio 6.2:** o recorte pode introduzir o catálogo para os novos controles do editor, mas não exige substituir todos os ícones existentes no mesmo commit. A migração dos demais menus e tool windows deve ser incremental, sem alterar comportamento.

### 6.5 Estágio 6.2 — superfície do editor — EM ANDAMENTO

**Objetivo:** completar a superfície visual e de interação do editor sem transformar o incremento em implementação de todo o núcleo PHP, Git, coverage ou LSP.

**Dependências disponíveis:** buffers e rope em `phpide-core`; estrutura Tree-sitter em `phpide-syntax`; breakpoints DBGp existentes; estado de bookmarks; tabs e breadcrumbs existentes.

**Limites do estágio:**

- folding deve usar estrutura local já disponível;
- inlay hints devem consumir um provedor; não exigem criar inferência semântica própria;
- blame depende de adaptador Git e pode ser entregue separadamente;
- coverage depende de relatório de cobertura e não deve ser simulado;
- split de editor não significa split de tool windows;
- detach em janela nativa pode ser recorte independente se o backend gráfico exigir arquitetura adicional;
- Database, refatorações e novos tipos de run configuration não fazem parte.

#### 6.5.1 Sequência interna recomendada

| Ordem | Sub-requisito | Entrega vertical |
|---|---|---|
| 1 | `UI-ED-000` | Baseline, geometria e testes da superfície atual |
| 2 | `UI-ED-001` | Gutter, bookmarks e breakpoints alinhados |
| 3 | `UI-ED-002` | Folding estrutural |
| 4 | `UI-ED-003` | Tabs, split e política de fechamento |
| 5 | `UI-ED-004` | Sticky lines |
| 6 | `UI-ED-005` | Inlay hints e Code Vision por provedores |
| 7 | `UI-ED-006` | Lens mode e faixas da scrollbar |
| 8 | `UI-ED-007` | Blame e coverage quando os provedores existirem |

Cada linha pode ser um commit independente. Não é necessário aguardar o estágio inteiro para integrar uma linha concluída.

*Estado (2026-11-07): Fase 5 entregue — iconografia própria com manifesto (UI-ICON-001-01..05); folding estrutural real com projeção e estado por revisão (UI-ED-002); gutter com estados de breakpoint reais e numeração visual sob folding (UI-ED-001); switcher MRU + groups/split com documento compartilhado (UI-ED-003-02/03/04 — render do segundo painel empilhado, eixo lado a lado na próxima iteração); sticky configuráveis (UI-ED-004); inlay hints LSP + Code Vision com dados reais (UI-ED-005); lens/markers/inspections (UI-ED-006); blame gutter + coverage com estado desconhecido honesto (UI-ED-007). P1s fora do recorte: UI-ICON-001-06/07 (migração por lotes), UI-ED-003-05/06/07 (drag/drop, detach), UI-ED-008/009.*

`UI-ED-000` — **Baseline e geometria — P0**

Antes de adicionar recursos, devem existir testes ou caracterização para:

- mapa entre posição do documento, linha visual e coordenada da tela;
- scroll horizontal e vertical;
- gutter com números de diferentes larguras;
- zoom da IDE e zoom do editor;
- linhas longas, tabs, Unicode e arquivo vazio;
- seleção e caret próximos às bordas.

**Aceite:** a posição visual usada por gutter, breakpoints e folding deriva da mesma geometria do texto; alterar zoom não desalinha clicáveis; não há mudança de comportamento fora do editor.

`UI-ED-001` — **Gutter, bookmarks e breakpoints — P0**

- Números de linha alinhados ao texto.
- Breakpoints sobre os números na New UI.
- Indicadores de bookmark e ações Run/Debug quando houver alvo real.
- Clique deve considerar a linha visual correta, inclusive após scroll e folding.
- Estados de breakpoint: enabled, disabled, verified, invalid e muted quando suportados pelo backend.

**Aceite:** criar breakpoint por clique e por atalho, editar o arquivo antes da linha e verificar reanexação; abrir outro buffer sem transferir indicadores; arquivo grande não exige redesenho proporcional a todas as linhas fora da viewport.

`UI-ED-002` — **Folding — P0**

- Regiões iniciais: classes, métodos/funções, blocos, arrays multilinha e comentários de bloco quando identificados pelo parser.
- Ícones no hover, com opção Always.
- Fold/unfold por mouse e comandos de teclado quando atribuídos.
- Caret e seleção não podem desaparecer em região oculta sem revelação automática adequada.
- O estado deve ser recalculado após edição sem manter ranges inválidos.

**Aceite:** dobrar e expandir regiões aninhadas; editar antes e dentro da região; salvar/reabrir preserva ou descarta estado de forma documentada; syntax error não trava o editor nem oculta o arquivo inteiro indevidamente.

`UI-ED-003` — **Tabs, split e detach — P0/P1**

P0:

- Closing Policy com limite configurável, default 10;
- preview tab em itálico e reutilização até edição;
- pin/unpin, Close Others e fechamento com dirty confirmation;
- Switcher `Ctrl+Tab`, fechamento `Ctrl+F4` e tabs ocultas acessíveis;
- split right/down com buffers e caret independentes.

P1:

- detach com `Shift+F4`;
- tab placement configurável;
- arrastar tabs entre grupos e janelas.

**Aceite de split:** duas vistas do mesmo documento compartilham texto, dirty state e undo de forma coerente, mas mantêm scroll e seleção independentes; fechar uma vista não fecha a outra; salvar uma atualiza ambas.

`UI-ED-004` — **Sticky lines — P1**

- Até cinco linhas estruturais por padrão.
- Namespace, classe e função/método conforme o contexto do topo da viewport.
- Clique navega à declaração.
- Configuração global e por linguagem.

**Aceite:** rolar métodos aninhados atualiza a pilha sem piscar; a linha fixa não duplica eventos de mouse; folding e sticky lines podem coexistir; limite zero desativa o recurso.

`UI-ED-005` — **Inlay hints e Code Vision — P1**

- Parameter name hints com opções All arguments e Pass by reference.
- Toggle global e exclude list.
- Code Vision para Inheritors, Usages e Code author somente quando houver provedor.
- Hints não alteram o texto nem entram em copiar/colar, busca ou save.

**Aceite:** hints acompanham edição e scroll; resposta antiga do provedor é descartada; ausência de servidor oculta hints com status explicativo; clicar em Usages abre resultados reais, não contadores estáticos.

`UI-ED-006` — **Lens mode e scrollbar — P1**

- Preview do código ao pairar sobre a scrollbar.
- Faixas de diagnósticos e mudanças com cores do tema.
- Inspections widget abre Problems no arquivo/escopo correto.
- Não incluir minimap como substituto do lens mode.

**Aceite:** preview corresponde à região indicada, respeita tema e desaparece ao sair; markers são limitados/agregados em arquivos muito grandes; erros de diagnóstico não bloqueiam o scroll.

`UI-ED-007` — **Blame e coverage — P1/P2**

Blame exige autor, revisão, data e navegação ao commit obtidos do Git. Coverage exige importação de relatório e mapeamento real às linhas executáveis.

**Aceite:** linhas sem autoria ou sem cobertura aparecem como desconhecidas, não como sucesso; arquivo modificado invalida ou ajusta anotações; nenhum dado fictício é exibido quando o provedor está ausente.

#### 6.5.2 Gate de conclusão do Estágio 6.2

O estágio inteiro só pode ser marcado como concluído quando:

- os sub-requisitos P0 estiverem implementados e testados;
- os P1 não entregues estiverem listados separadamente, sem serem apresentados como prontos;
- gutter, folding e tabs funcionarem juntos sob scroll e zoom;
- existir smoke test gráfico de abertura, edição, folding e split;
- um arquivo de pelo menos 10 mil linhas permanecer navegável, com medição registrada;
- README e matriz de rastreabilidade refletirem exatamente o escopo entregue.

### 6.6 Outros comportamentos do editor

`UI-ED-008` — **Aparência e caret — P1**

Configurações de blink, block caret, Snappy/Gliding, occurrences, method separators, whitespace, indent guides, separadores de classe/namespace PHP e HTML tag tree highlighting.

`UI-ED-009` — **Quick-fix e intentions — P1**

`Alt+Enter` abre ações aplicáveis ao contexto. Preview deve mostrar o diff antes de aplicar quando a alteração não for trivial. A lâmpada no gutter deve corresponder à mesma lista de ações do menu.

`UI-ED-010` — **Fonte e zoom — P0/P1**

P0: aumentar/reduzir com `Alt+Shift+.` e `Alt+Shift+,`, persistência e limites seguros. P1: Ctrl+wheel opcional, line height e ligatures.

### 6.7 Atalhos essenciais de referência

A keymap Default deve evitar conflitos e documentar variações por plataforma:

| Ação | Atalho |
|---|---|
| Search Everywhere | `Shift×2` |
| Go to Class / File / Symbol | `Ctrl+N` / `Ctrl+Shift+N` / `Ctrl+Alt+Shift+N` |
| Find Action | `Ctrl+Shift+A` |
| Toggle line breakpoint | `Ctrl+F8` |
| Resume debug session | `F9` |
| Context actions | `Alt+Enter` |
| Próximo/anterior problema | `F2` / `Shift+F2` |
| Recent Files | `Ctrl+E` |
| Extend selection | `Ctrl+W` |
| Basic / type-matching completion | `Ctrl+Space` / `Ctrl+Shift+Space` |
| Statement completion | `Ctrl+Shift+Enter` |
| Comment | `Ctrl+/` |
| Find Usages | `Alt+F7` |
| Go to Declaration / Implementation | `Ctrl+B` / `Ctrl+Alt+B` |
| Structure popup | `Ctrl+F12` |
| Generate | `Alt+Insert` |
| Refactor This | `Ctrl+Alt+Shift+T` |
| VCS popup | `Alt+`` |
| Quick Documentation | `Ctrl+Q` |
| Type Hierarchy | `Ctrl+H` |
| Quick Switch Scheme | ``Ctrl+` `` |
| Settings | `Ctrl+Alt+S` |

Atalhos só entram na keymap padrão quando a ação correspondente existe. Conflitos com o editor, tool windows e diálogos devem ter teste de roteamento de teclado.

**Migrações conhecidas do baseline:** o código atual pode usar `Ctrl+N` para novo arquivo, `F9` para breakpoint e `Ctrl+P` para command palette, enquanto a keymap de referência associa essas teclas a Go to Class, Resume e Parameter Info. Esses conflitos devem ser resolvidos em um requisito explícito de keymap, atualizando menus, README, testes e documentação no mesmo commit. Até essa migração, nenhum agente deve presumir que os dois comportamentos podem coexistir sem regra contextual.

## 7. Núcleo PHP

### 7.1 Princípio de implementação

A análise semântica deve ser fornecida prioritariamente por servidores de linguagem e ferramentas do ecossistema. Tree-sitter cobre estrutura e degradação offline; não deve ser expandido para duplicar inferência de tipos, autoload e regras de framework.

`PHP-001` — **Ciclo de vida e sincronização LSP — P0**

- Descoberta por configuração explícita, instalação local do projeto e `PATH`.
- Suporte inicial a Intelephense e Phpactor, com capabilities negociadas.
- `didOpen`, `didChange`, `didSave` e `didClose` na ordem correta.
- Restart, timeout e encerramento sem bloquear a UI.
- Estado visível: starting, ready, degraded, stopped e failed.

**Aceite:** editar rapidamente não reordena versões; fechar arquivo encerra o documento no servidor; trocar de projeto isola sessões; derrubar o servidor apresenta erro e permite reiniciar; ausência do servidor mantém edição e diagnósticos locais disponíveis.

`PHP-002` — **Diagnósticos e Problems — P0**

- Publicar diagnósticos por arquivo e revisão.
- F2/Shift+F2 navegam entre problemas do contexto definido.
- Problems permite arquivo, projeto e erros do serviço de linguagem quando suportado.
- Diagnósticos externos devem indicar a ferramenta de origem.

**Aceite:** corrigir o texto remove o diagnóstico correspondente; resposta de versão antiga não apaga resultado mais recente; fechar arquivo remove ou arquiva entradas conforme política documentada.

`PHP-003` — **Completion — P0/P1**

P0:

- Basic completion por `Ctrl+Space` e show-as-you-type configurável;
- símbolos no escopo, membros, variáveis e live templates registrados;
- filtro, documentação e insertText/textEdit;
- aceite por Enter/Tab sem duplicar delimitadores.

P1:

- segunda invocação e símbolos adicionais quando suportados;
- type-matching `Ctrl+Shift+Space`;
- statement completion `Ctrl+Shift+Enter`;
- Hippie `Alt+/`, postfix e parameter info;
- auto-import com undo único;
- configurações de match case, single suggestion, ordenação e popup de documentação.

**Aceite P0:** `$this->`, namespaces e variáveis locais retornam itens do servidor; selecionar item aplica o range correto com Unicode no documento; digitar durante resposta descarta sugestões obsoletas; servidor indisponível não bloqueia digitação.

`PHP-004` — **Navegação e símbolos — P0/P1**

- Go to Declaration, Implementation e Type Declaration conforme capabilities.
- Find Usages, Structure, breadcrumbs e Go to Symbol.
- Type Hierarchy e Related Symbol como P1.
- Resultados devem abrir localização exata e integrar histórico Back/Forward.

**Aceite:** navegar entre arquivos do projeto e vendor indexado; símbolo inexistente mostra estado vazio; voltar restaura arquivo, caret e scroll.

`PHP-005` — **Highlight semântico e ocorrências — P1**

Highlight sintático continua disponível offline. Highlight semântico pode complementar o resultado do servidor, mas deve ser invalidado por revisão e não introduzir flicker durante digitação.

### 7.2 Inspeções, intenções e correções

`PHP-010` — **Infraestrutura de inspeções — P1**

- Severidades e profiles por projeto.
- Escopos e execução on-the-fly/batch.
- Quick-fixes com preview e undo.
- Prefixo de origem para PHPStan, Psalm, PHPCS e outras ferramentas.
- Desativação por regra, arquivo ou escopo persistida de maneira auditável.

**Aceite:** alterar severidade atualiza apresentação sem executar novamente o provedor; desativar regra não oculta as demais; batch produz o mesmo conjunto de regras do modo on-the-fly no mesmo escopo.

`PHP-011` — **Catálogo de paridade — P2**

A visão inclui as categorias oficiais de referência: Undefined symbols, Probable bugs, Code smell, Code style, Type compatibility, Control flow, General, Naming conventions, Attributes, PHPDoc, PHPUnit, Pest, Composer, Quality tools, Error handling, Regular expressions, Replaceable assignments, PHP strict standards, Symfony, Behat, Psalm, Taint analysis e Unused symbols.

Cada inspeção implementada internamente deve ter regra, exemplos positivos/negativos, severidade default e quick-fix documentados. Integrações externas não devem ser contadas como inspeções nativas.

`PHP-012` — **Intention actions — P1**

Ações contextuais por `Alt+Enter`, gerenciamento em Settings e preview. Exemplos de visão: inicializar propriedade no construtor, criar parâmetro, gerar ArrayShape/ObjectShape, executar Composer script, habilitar inspeção e mover HTTP Request.

### 7.3 Templates e refatoração

`PHP-020` — **Live templates — P1**

- Expansão por abreviação e Tab.
- Variáveis, `$END$`, `$SELECTION$` e surround.
- Grupos por linguagem e reformat according to style.
- Importação/exportação em formato documentado.
- Conjunto inicial: `rqr`, `pubsf`, `fore` e `eco`.

**Aceite:** expansão cria uma única operação de undo; Tab percorre variáveis na ordem; Escape encerra sem apagar texto; conflito com completion é resolvido por precedência documentada.

`PHP-021` — **Refatorações seguras — P1/P2**

P1 inicial:

- Rename com atualização de referências;
- Extract/Introduce Variable;
- Extract Method;
- Change Signature com preview;
- Safe Delete com busca de usos.

P2:

- Move Namespace/Class/Function/Member, Copy, Extract Constant/Field/Parameter/Class/Interface/Superclass/Include, Inline, Make Static, Pull Members Up e Push Members Down.

Toda refatoração deve executar análise de conflitos, apresentar preview para múltiplos arquivos, aplicar WorkspaceEdit atomicamente quando possível e oferecer undo coeso. “Refactor Anyway” deve deixar claro o risco e não ocultar arquivos com falha.

`PHP-022` — **PHPDoc e tipos avançados — P1/P2**

- Array/object shapes em PHPDoc e atributos.
- Integer ranges e inspeções de incompatibilidade.
- Anotações Psalm, pseudotipos, aliases e imports.
- Generics com casos de teste por padrão suportado.

A implementação deve declarar quais recursos vêm do servidor e quais são locais. Não inferir suporte completo a partir de highlight ou completion de sintaxe.

### 7.4 Composer

`PHP-030` — **Descoberta de projeto Composer — P0**

- Detectar `composer.json`, autoload PSR-0/PSR-4, vendor e require.php.
- Mapear source/test roots e language level sem apagar configuração manual explícita.
- Avisar vendor ausente sem executar install automaticamente.

**Aceite:** abrir projeto temporário com PSR-4 indexa os diretórios corretos; alteração externa do composer.json solicita reload; múltiplos composer.json são isolados por subprojeto quando esse recorte for implementado.

`PHP-031` — **Comandos Composer — P1**

Init, install, update, validate, dump-autoload, clear-cache, licenses, diagnose, simulate update, self-update e scripts. Cada comando deve usar interpretador configurado, exibir Composer Log e permitir cancelamento quando seguro.

`PHP-032` — **Assistance e dependências — P1/P2**

Completion e documentação no composer.json, navegação para pacotes/classes/scripts, create-project, install/update/uninstall por ações explícitas e repositórios customizados.

Comandos que modificam dependências devem mostrar comando, diretório e consequência antes da execução quando não forem estritamente read-only.

### 7.5 Ferramentas de qualidade

`PHP-040` — **Adaptadores de qualidade — P1**

| Ferramenta | Integração esperada |
|---|---|
| PHPStan/Larastan | Config neon, vendor/bin, saída estruturada e prefixo de diagnóstico |
| Psalm | psalm.xml, cache opcional e análise de projeto |
| PHP CS Fixer | Regras, config e External Formatter |
| Laravel Pint | Presets, pint.json e formatação de escopo selecionado |
| PHP_CodeSniffer | Standards, auto-detecção e phpcbf |
| PHP Mess Detector | Rulesets e diagnósticos sem autofix implícito |

**Aceite comum:** descoberta local, versão incompatível, config ausente, saída inválida, cancelamento e paths com espaços. Formatters devem preservar dirty state de forma consistente e produzir diff/undo; nunca salvar silenciosamente outros arquivos.

## 8. Execução, depuração, testes e HTTP

### 8.1 Run configurations

`RUN-001` — **Modelo e ciclo de vida — P0**

Campos comuns: nome, working directory, ambiente, interpreter, allow multiple instances, store as project file e before launch.

- Configurações temporárias podem ser salvas.
- Rerun, Stop, Pause Output, Pin Tab e lista de processos devem refletir sessões reais.
- Variáveis de ambiente secretas não aparecem em logs.

*Estado (2026-11-03): entregue em `phpide-run` — modelo versionado, sessões com
estado/cwd/env/output, process list com Rerun/Stop/Pause Output/Pin e redaction
de env nos logs. Before-launch tasks (T-RUN-001-04) pendente.*

`RUN-002` — **Tipos de configuração — P0/P1/P2**

P0: PHP Script, Composer Script e PHPUnit básico.

P1: PHP Web Page, PHP Remote Debug, HTTP Request, Pest e cobertura.

P2: Behat, PHPSpec, Codeception, ParaTest, mutation e execuções compostas.

Campos específicos devem ser validados antes de iniciar. PHP Script inclui arquivo, argumentos, interpreter options, env e break at first line. Testes incluem scope, config/bootstrap e opções do runner.

*Estado (2026-11-03): PHP Script e PHPUnit básico entregues (validação de
campos, escopos, config/bootstrap, break_at_first_line via breakpoint na linha
1). Composer Script tem montagem de comando; execução depende do executor
Composer (T-PHP-031). Demais tipos P1/P2 pendentes.*

### 8.2 Xdebug/DBGp

`RUN-DBG-001` — **Sessão CLI zero-config — P0**

- Listener configurável, porta default 9003.
- IDE key e `XDEBUG_CONFIG` sem apagar configuração externa necessária.
- Line breakpoints, run, detach, frames e variáveis.
- Path mapping para projeto local quando necessário.

**Aceite:** sessão simulada completa e sessão PHP real; breakpoint interrompe na linha esperada; Resume termina processo; Stop/Detach fecha socket e libera a UI; conexão de projeto diferente exige confirmação/mapping.

*Estado (2026-11-03): entregue — listener 9003 com IDE key, XDEBUG_CONFIG
mesclando env do usuário, path mapping com associação Owned/Foreign, breakpoints
com estados, Resume/Detach/Stop confiáveis (socket + processo). Validado de
ponta a ponta contra o **Xdebug 3.2.0 real** (extensão carregada sem root via
`.so` extraído de `.deb` do Debian — `apt-get download` + `dpkg-deb -x`), o que
exigiu correções protocolares achadas só com a extensão: pacotes de comprimento
em toda mensagem do engine (`read_message`), `success` ausente = sucesso
(DBGp 1.0), `eval` com dados posicionais após `--`, valores
`encoding="base64"`, e shutdown cirúrgico de Detach/Stop (máquina de fases
Accepting/Stopped/Continuing — fechar o socket na fase parada tem semântica de
detach, não de stop). Detalhes e evidências no baseline do Estágio 8.*

`RUN-DBG-002` — **Stepping e inspeção — P0/P1**

P0: Step Into `F7`, Over `F8`, Out `Shift+F8`, Resume `F9` e Show Execution Point `Alt+F10`.

P1: Watches, Evaluate Expression `Alt+F8`, Quick Evaluate, Set Value, inline values, JSON/XML estruturado e console REPL.

Avaliações devem estar associadas ao frame selecionado e ser invalidadas após Resume.

*Estado (2026-11-03): stepping F7/F8/Shift+F8/F9/Alt+F10, Watches e Evaluate por
frame entregues (evaluate por frame via `property_get -d`; `eval` no topo da
pilha é limitação do DBGp). Set Value, inline values, JSON/XML e REPL
pendentes (T-RUN-DBG-002-03).*

`RUN-DBG-003` — **Breakpoints avançados — P1/P2**

Conditional, method, exception, logpoint, hit message/stack, remove once hit, trigger por outro breakpoint, grupos, mute e skipped paths. Estados Verified/Warning/Invalid/Inactive/Muted devem vir do backend ou ser claramente locais.

*Estado (2026-11-03): modelo com estados Pending/Verified/Invalid/Disabled,
sincronização com o backend e hit count entregues (line/conditional/exception/
method/logpoint no modelo). Comportamentos avançados (hit conditions, triggers,
grupos, skipped paths) pendentes (T-RUN-DBG-003-02/03).*

`RUN-DBG-004` — **Web, remoto e concorrência — P1/P2**

Servers e path mappings, validação de web server, sessões simultâneas, proxy DBGp multiuser, túnel SSH e comandos de frameworks. Zend Debugger entra apenas como adaptador separado e testado.

### 8.3 Testes e cobertura

`RUN-TST-001` — **Test Runner — P1**

- Descoberta e execução por diretório, classe, método e data set.
- Árvore de resultados, duração, output e navegação à falha.
- Rerun e Rerun Automatically com debounce.
- Navegação test↔subject e geração por template.

*Estado (2026-11-03): execução por escopo, árvore com duração/output e navegação
à falha entregues (parser TeamCity, validado com PHPUnit 11 real). Rerun
Automaticamente e navegação test↔subject pendentes (T-RUN-TST-001-03/04).*

`RUN-TST-002` — **Frameworks — P1/P2**

PHPUnit primeiro; Pest em seguida; Behat, PHPSpec e Codeception por adaptadores. Recursos Parallel e Mutation devem mostrar versões e dependências exigidas.

`RUN-TST-003` — **Coverage — P1**

Importar Xdebug, PCOV ou formato intermediário definido; apresentar percentual por arquivo e linhas executáveis. Coverage não é sucesso de teste e não deve ser inferido de output textual.

*Estado (2026-11-03): entregue via Clover (saída `--coverage-clover` do PHPUnit,
gerada por Xdebug/PCOV): modelo por arquivo/linha executável, importador,
percentual por arquivo e gutter no editor. Validado end-to-end com driver real:
PHPUnit 11 + Xdebug 3.2.0 (modo coverage) geram o Clover e o importador
disponibiliza por arquivo/linha (gate `coverage_real_com_xdebug_por_arquivo_e_linha`).*

### 8.4 Servidor web e HTTP Client

`RUN-WEB-001` — **Servidor web embutido — P1**

Porta default 63342, conteúdo estático e PHP via `php -S` quando configurado. Deve ter ciclo de vida explícito, proteção contra conexões externas por padrão e nenhuma liberação de unsigned requests sem ação do usuário.

`RUN-HTTP-001` — **Editor e execução .http — P1/P2**

P1:

- arquivos `.http`/`.rest`, múltiplos requests e separador `###`;
- environments públicos e privados;
- variáveis, headers, corpo, Basic/Digest e timeout;
- resposta por content-type, histórico e cookies com limites;
- conversão cURL e exportação.

P2:

- pre-request/response scripts, GraphQL, WebSocket, OpenAPI, Postman, multipart, redirects para arquivo e Crypto API.

Arquivos privados de ambiente devem ser ignorados pelo VCS por padrão. Execução de scripts e requests destrutivos exige ação explícita e identificação do environment.

## 9. Frontend

`FE-001` — **HTML, CSS e JavaScript essenciais — P1**

- Highlight e diagnósticos.
- Completion e navegação por Language Service.
- Emmet, documentação MDN e color preview.
- Auto-import JavaScript quando suportado.
- `node_modules` indexado como biblioteca, sem varredura redundante pela árvore comum.

**Aceite:** abrir projeto Vite simples, editar componente e CSS, obter diagnóstico real, navegar a definição e manter a UI responsiva com node_modules presente.

`FE-002` — **TypeScript — P1**

Seleção de versão, Project Errors, inlay hints do Language Service, Go to Type Declaration e auto-import com modifier `type` quando aplicável. Suporte a TypeScript-Go fica condicionado a adaptador e testes próprios.

`FE-003` — **Frameworks frontend — P2**

- React: componentes, props, JSX, colar HTML→JSX e templates.
- Angular: Language Service, templates, signals, schematics e related symbols.
- Vue: SFC e Volar, componentes, slots e Nuxt.
- Tailwind: Language Server, completion e preview.

Cada framework deve ser um incremento independente; suporte genérico a JavaScript não deve ser anunciado como suporte completo ao framework.

`FE-004` — **Execução e debug frontend — P2**

Attach Node.js, Chromium, interactive console e runners Jest/Karma/Mocha/Cucumber.js/Protractor. Source maps devem ser resolvidos antes de anunciar breakpoints TypeScript/JSX como confiáveis.

`FE-005` — **CSS avançado — P2**

Sass/SCSS/Less/Stylus/PostCSS, variáveis, mixins, Baseline/browser compatibility, conversões de cor, Introduce Variable/Ruleset e Show Applied Styles.

## 10. Database Tools and SQL

### 10.1 Princípios e ordem de entrega

A shell Database não representa o RF-DB concluído. A implementação deve ocorrer em três ondas:

1. **Fundação e leitura:** conexões seguras, introspection e query console read-only.
2. **Edição controlada:** grade editável, preview DML, transações e import/export.
3. **Administração avançada:** diagramas, compare, execution plans, dumps e mais SGBDs.

SQLite deve validar o fluxo local; PostgreSQL e MySQL/MariaDB devem validar rede, sessões e dialetos. Outros bancos entram por adaptadores com testes próprios.

`DB-001` — **Arquitetura de drivers — P0**

Interfaces separadas para conexão, metadados, execução, cancelamento, transações e edição. Tipos comuns devem preservar valores nulos, binários, datas, decimais e erros do driver sem convertê-los indiscriminadamente para String.

**Aceite:** um driver fake cobre o contrato da UI; SQLite cobre integração real; trocar o driver não altera o modelo de sessões ou o query console.

`DB-002` — **Data Sources and Drivers — P0**

- Criar, editar, duplicar, remover e testar conexões.
- Project/global, cores, pastas e URL-only.
- SSL/TLS e SSH como configurações explícitas.
- Seleção de schemas e introspecção sob demanda.
- Segredos fora de arquivos versionáveis e logs.

**Aceite:** credencial inválida e host indisponível mostram erros distintos; Test Connection não abre console nem altera schema; reabrir projeto não expõe senha; excluir conexão encerra sessões associadas.

`DB-003` — **Introspection e Database Explorer — P0**

- Schemas, tabelas, views, colunas, chaves e índices.
- Refresh por subtree e invalidação após DDL executada pela IDE.
- Filtro de schemas e objetos sem perder a árvore carregada.
- Quick Documentation/DDL quando o driver oferecer metadados.

**Aceite:** criar tabela em banco descartável, atualizar explorer e localizar colunas/constraints; erro de introspecção parcial não apaga metadados válidos nem aparenta schema vazio.

`DB-004` — **Query console — P0**

- Console por data source e sessão.
- Default schema e dialeto do driver.
- Executar statement sob o caret ou seleção com `Ctrl+Enter`.
- Destaque do statement que será executado antes de comandos potencialmente destrutivos.
- Output separado de resultsets, linhas afetadas, duração e erros.
- Cancelamento real ou indicação explícita de que o driver não suporta cancelamento.
- Query History local com redaction de parâmetros sensíveis quando identificados.

**Aceite:** executar seleção e erro SQL em sequência; apenas a seleção é enviada quando houver seleção; fechar console encerra a sessão; cancelar consulta longa libera a UI; duas consultas não misturam resultados.

`DB-005` — **Grade de resultados read-only — P0**

- Paginação e limite de linhas configurável.
- Ordenação e filtros server-side quando possível, com consulta visível.
- Table/Text/Record View; NULL e valores truncados distinguíveis.
- Tipos binários/LOB abrem em Value Editor sem carregar todo o conteúdo na grade.
- Exportação do resultado respeita delimitadores, encoding e NULL.

**Aceite:** tabela grande não é carregada integralmente; mudar de página não mistura registros; ordenação não altera dados; célula vazia e NULL têm representações diferentes.

`DB-006` — **Edição e transações — P1**

- Add/Delete/Clone Row e edição de célula.
- Set NULL/DEFAULT/UUID e arquivos LOB.
- Preview DML antes de Submit.
- Revert por célula/linha e Submit com erros por registro.
- Auto/Manual transaction, Commit/Rollback e isolamento quando suportado.

**Aceite:** alterar e reverter sem tocar no banco; Submit gera DML com chave ou informa estratégia quando não houver chave; rollback descarta alterações pendentes; conflito de concorrência não sobrescreve dados silenciosamente.

`DB-007` — **Import, export e scripts — P1/P2**

Extractors TXT, CSV, JSON, XML, Markdown, Excel e SQL INSERT. Import CSV com charset, header, mapping, tipos, DDL editável e preview. Run SQL Script deve informar transação, statements com erro e política de continuação.

Dumps via `mysqldump`/`pg_dump` devem mostrar executável, versão, destino e compatibilidade; credenciais preferem mecanismos seguros do cliente, não argumentos expostos.

`DB-008` — **Diagramas, comparação e planos — P2**

Schema/table diagrams, pan/zoom, cores e export GraphML/drawio/DOT/Mermaid/PlantUML/PNG. Compare data/estruturas e execution plans visuais dependem do dialeto e devem identificar a versão do servidor.

`DB-009` — **Cobertura adicional de SGBDs — P2**

Visão de longo prazo: SQL Server, Oracle, H2, HSQLDB, Derby, Db2, Sybase ASE, ClickHouse, CockroachDB, Exasol, Greenplum, Vertica, Snowflake, BigQuery, Redshift, Azure SQL, Hive, Cassandra, MongoDB, Redis, Couchbase, DocumentDB e DynamoDB.

Bancos não relacionais exigem modelo próprio de explorer/query/resultados; não devem ser forçados ao contrato relacional. Cada novo driver precisa declarar suporte a transações, cancelamento, introspecção e edição.

## 11. Git e controle de versão

`GIT-001` — **Repositório e branch — P0**

*Estado (2026-11-07): entregue (GIT-001-01) — `phpide-vcs::repo` detecta worktree (root resolvido), branch, detached HEAD (sha curto) e repositório inválido/bare como erro recuperável com ação (NFR-001/005); UI (header/status bar) consome via `vcs::current_branch`, que agora mostra `(detached <sha>)`. Multi-root (GIT-001-02) segue pendente, junto do refresh incremental de status.*

`GIT-002` — **Diff e Commit — P0/P1**

`GIT-002` — **Diff e Commit — P0/P1**

P0:

- diff de working tree e staged;
- stage/unstage por arquivo;
- commit com mensagem e output;
- status real após a operação.

P1:

- stage por chunk/linha;
- changelists, Amend e ações before/after commit;
- staging area opcional;
- commit inline pelo gutter.

**Aceite P0:** criar repositório temporário, modificar arquivos, stagear apenas um, commitar e conferir que o outro permaneceu fora; falha de hook preserva output e índice; mensagem vazia não executa commit.
*Estado (2026-11-07): P0 entregue (GIT-002-01/02/03) — `phpide-vcs::diff` (diff working tree/staged/HEAD com parser unified e arquivo untracked sintético), `ops::stage/unstage` com status porcelain real pós-operação e `ops::commit` (mensagem vazia não executa; falha de hook preserva output no detalhe do erro e mantém o índice intacto). Aceite P0 validado em gate com repositório temporário real. P1 (chunk/linha, changelists, amend, staging opcional, commit inline) pendente.*

`GIT-003` — **Log, branches e sincronização — P1**

`GIT-003` — **Log, branches e sincronização — P1**

Log com commits/arquivos, filtros, Branches popup, incoming/outgoing, compare, fetch, pull e push. Operações que reescrevem histórico ou force push devem usar confirmação específica; force deve preferir `--force-with-lease`.
*Estado (2026-11-07): log entregue (GIT-003-01) — `phpide-vcs::log` com `--name-status` (arquivos por commit), filtros de quantidade, path e revisão inicial (`rev`, usado na navegação a partir do blame). Branches popup, incoming/outgoing, compare e fetch/pull/push (GIT-003-02/03/04) pendente.*

`GIT-004` — **Mudanças locais no editor — P1**

`GIT-004` — **Mudanças locais no editor — P1**

Markers de linhas adicionadas/modificadas/removidas devem derivar do diff e atualizar após save, stage e commit. Gutter de blame segue `UI-ED-007`.
*Estado (2026-11-07): entregue (GIT-004-01/02) — `phpide-vcs::markers` deriva Added/Modified/Removed do diff vs HEAD (pares del+add viram Modified; remoção ancora na próxima linha existente) e `markers_for_file` reflete save/stage/commit sem cache a invalidar; `phpide-vcs::blame` parseia o porcelain (meta herdada por grupo, boundary, linhas não commitadas) com `line_blame` e navegação ao commit via `log(rev=sha)`. Renderização no editor (gutter/blame gutter) segue com UI-ED-005-06/006-01/007-01.*

`GIT-005` — **Merge, rebase e recuperação — P1/P2**

`GIT-005` — **Merge, rebase e recuperação — P1/P2**

Merge 3-way, rebase interativo, reword, worktrees, stash e submodules. Operações com dirty tree devem validar precondições e preservar caminho de recuperação.

`GIT-006` — **GitHub e GitLab — P2**

Clone/share/fork, Pull Requests e Merge Requests, timeline, review, checkout e merge. Tokens devem usar armazenamento seguro; comentários privados não devem ser copiados para logs.

## 12. Ferramentas integradas

### 12.1 Terminal

`TOOL-TERM-001` — **Terminal executável — P0**

Shell configurável, working directory do projeto, env controlado e output incremental. O shell explícito do usuário pode aceitar comandos arbitrários; demais recursos da IDE não devem reutilizar esse canal para ocultar execução de shell.

`TOOL-TERM-002` — **Sessões — P1**

Abas, splits, histórico, busca, links de arquivos, drag-and-drop, SSH e command completion. Restaurar sessão não deve reexecutar comandos automaticamente.

### 12.2 Local History

`TOOL-HIST-001` — **Histórico local — P1**

- Revisões por save e eventos rotulados.
- Show History com diff e revert total/parcial.
- Restauração de arquivos deletados e Create Patch.
- Retenção configurável, default de referência de cinco dias úteis.
- Exclusão de segredos e arquivos grandes por política documentada.

Local History não substitui Git e não deve ser apresentado como backup remoto.

### 12.3 Deployment e Remote Host

`TOOL-DEP-001` — **Conexões e mappings — P1/P2**

FTP/FTPS/SFTP/WebDAV/local, mappings, excluded paths, server groups e Remote Host. Upload/download/sync/compare devem oferecer diff e confirmação para operações destrutivas.

`TOOL-DEP-002` — **Automação — P2**

Auto-upload on save/commit somente quando habilitado e com fila visível. Falha não pode marcar arquivo como sincronizado; credenciais seguem `NFR-004`.

### 12.4 Docker, WSL e Dev Containers

`TOOL-CONT-001` — **Docker — P2**

Conexões TCP/Unix/SSH/contexts, Services para images/containers/Compose e run configurations. Interpretadores PHP/Node em containers exigem path mappings e propagação controlada de env.

`TOOL-CONT-002` — **WSL — P2**

Projetos no filesystem WSL, interpretadores e Git do ambiente correto. Caminhos Windows/WSL devem ter testes de conversão.

`TOOL-CONT-003` — **Dev Containers — P2**

Ler `devcontainer.json`, usar Dev Container CLI e validar Docker CLI/Buildx. Criação e rebuild devem mostrar logs e nunca apagar volumes sem confirmação específica.

## 13. Frameworks PHP

Frameworks devem ser módulos independentes sobre o núcleo PHP, sem condicionais espalhados pela UI.

`FW-SYMFONY-001` — **Symfony — P2**

Completion/navegação de services, routes, forms, events, Doctrine e translations; Symfony Symbol; gutter template/controller; profiler; console e Twig.

`FW-LARAVEL-001` — **Laravel — P2**

Eloquent, routes, request fields, validation, gates, Blade, Artisan, Pint e Pest. Debug de Blade deve mapear cache de views ao template original.

`FW-WORDPRESS-001` — **WordPress — P2**

Hooks, callbacks, documentação, code style, PHPCS e WP-CLI.

`FW-DRUPAL-001` — **Drupal — P2**

Extensões de arquivo PHP, hooks, include paths, Coder, Drush e integração Symfony conforme a versão suportada.

`FW-JOOMLA-001` — **Joomla — P2**

Auto-detecção, DocBlocks, code style e importação explícita de data sources a partir de configuração local, sem registrar credenciais em logs.

**Aceite comum:** cada framework possui projeto fixture mínimo, detecção, versão suportada, testes de navegação/completion e comportamento neutro quando o framework não está presente.

## 14. IA e recursos avançados

`AI-001` — **Full Line Completion local — P2**

Modelo on-device, sem envio de código, requisitos de CPU explicitados e configuração separada de completion LSP. Sugestões não podem bloquear digitação nem ser aceitas acidentalmente por atalhos já usados no contexto.

`AI-002` — **AI Assistant e agentes — P2**

Chat contextual, BYOK, modelos locais/cloud e agentes devem declarar exatamente quais dados saem do ambiente. Código, consultas, credenciais e arquivos privados exigem política de contexto e redaction.

`AI-003` — **Agent Sessions e MCP — P2**

Worktrees temporários, revisão de diff, presets e exposição de modelo do código por MCP devem operar com menor privilégio possível. Nenhuma ferramenta de escrita ou execução deve ser habilitada implicitamente.

`AI-004` — **Inspeções headless/Qodana — P2**

A arquitetura de inspeções deve permitir execução fora da UI. Integração CI/CD deve ser um adaptador separado, com resultados reproduzíveis.

## 15. Requisitos não funcionais complementares

`NFR-006` — **Desempenho**

Metas iniciais de referência, a validar em hardware documentado:

- digitação sem travamento perceptível em arquivo de 10 mil linhas;
- viewport renderiza somente linhas necessárias ou usa estratégia equivalente;
- busca e indexação ignoram `.git`, `vendor` e `node_modules` conforme configuração;
- resultset usa paginação desde o primeiro release DB;
- cancelamento de operação atualiza a UI imediatamente, ainda que o backend demore a confirmar.

Metas numéricas adicionais só devem ser adicionadas após benchmark reproduzível.

`NFR-007` — **Confiabilidade e recuperação**

- Crash de servidor externo não pode derrubar a IDE.
- Save deve evitar arquivo parcial e informar falha de escrita.
- Dirty buffers precisam de confirmação ao fechar.
- Operações multiarquivo devem informar sucesso parcial.
- Estado persistido deve ter migração e fallback seguro.

`NFR-008` — **Segurança e privacidade**

- Nenhum segredo em logs, screenshots de teste ou fixtures.
- Projetos não confiáveis não executam scripts, plugins ou tasks automaticamente.
- Links e arquivos remotos exigem validação de destino.
- Consultas e comandos destrutivos dependem de ação explícita.
- Dependências devem ter licença compatível e inventário verificável.

`NFR-009` — **Acessibilidade e internacionalização**

- Navegação por teclado nos fluxos P0.
- Contraste e foco visível.
- Não depender apenas de cor, hover ou ícone.
- Layout deve aceitar textos maiores e caracteres Unicode.
- Textos podem nascer em português ou inglês, mas comandos e identificadores persistidos não devem depender da tradução.

`NFR-010` — **Compatibilidade e upgrades**

- Migrações de settings devem ser incrementais e testadas com configurações antigas.
- Formatos de projeto devem ter versão.
- Plugins e drivers precisam de capabilities, não apenas versão nominal.
- Features experimentais devem ser identificadas e desativáveis.

## 16. Roadmap por fluxos verticais

A numeração preserva o histórico do projeto; não exige que toda a visão de uma categoria esteja concluída antes de avançar para a seguinte.

| Estágio | Estado | Resultado exigido |
|---|---|---|
| 0–5 | Concluído | Fundação, editor MVP, shell inicial, LSP básico, execução/DBGp e plugins |
| 6.0–6.1 | Concluído | Shell visual, tool windows, Settings, Welcome e temas |
| **6.2** | **Em andamento** | Superfície do editor conforme `UI-ED-000` a `UI-ED-007` |
| 7.0 | Proposto | Fluxo PHP P0 confiável: sync LSP, diagnósticos, completion e navegação |
| 7.1 | Proposto | Composer P0 e qualidade integrada inicial |
| 8.0 | Em andamento | Run/Debug P0 e Test Runner inicial — RUN-001/002 (sem before-launch), RUN-DBG-001, RUN-DBG-002 P0, RUN-DBG-003 modelo, RUN-TST-001/002/003 entregues; ver `docs/baseline/T-RUN-ESTAGIO8-run-debug-test.md` |
| 9.0 | Proposto | Frontend essencial com Language Services |
| 10.0 | Proposto | Git P0: diff, stage, commit e status |
| 11.0 | Proposto | DB P0: SQLite/PostgreSQL/MySQL, explorer, console e grade read-only |
| 11.1 | Proposto | DB P1: edição, DML preview e transações |
| 12.0 | Proposto | Local History, deployment e terminal avançado |
| 13.0+ | Proposto | Frameworks, mais SGBDs, containers, IA e paridade avançada |

O roadmap pode ser reordenado por valor, mas dependências técnicas não devem ser ignoradas. Por exemplo, blame não antecede o adaptador Git, e edição de dados não antecede transações e preview DML.

## 17. Matriz inicial de rastreabilidade

| Requisito | Evidência mínima | Fluxo de aceite |
|---|---|---|
| `UI-ICON-001` | Smoke offline + inventário de assets | Menus e painéis nos três temas de referência |
| `UI-ED-001` | Unidade de geometria + smoke UI | Breakpoint após scroll, zoom e edição |
| `UI-ED-002` | Testes de ranges Tree-sitter + smoke UI | Fold aninhado com edição |
| `UI-ED-003` | Core/workspace + UI | Mesmo buffer em dois splits |
| `PHP-001` | Integração com servidor real e simulado | Editar, fechar e reiniciar LSP |
| `PHP-003` | Contrato textEdit + fluxo | Completion `$this->` e namespace |
| `RUN-DBG-001` | Fake Xdebug + PHP real | Breakpoint, frame, Resume e Detach |
| `GIT-002` | Repositório temporário | Stage seletivo e commit |
| `DB-004` | Driver fake + banco descartável | Seleção, erro e cancelamento |
| `DB-006` | Teste transacional | Preview, Submit e Rollback |
| `NFR-004` | Roundtrip e migração | Settings antigas e segredos separados |

A matriz deve crescer junto com a implementação. Testes devem citar requisitos apenas quando realmente exercitam o comportamento descrito.

## 18. Modelo obrigatório de tarefa

Antes de iniciar um incremento, copiar este modelo para a issue, PR ou descrição da tarefa:

```markdown
## Requisito
ID e título:

## Resultado para o usuário
Fluxo que passa a funcionar:

## Escopo
Inclui:
Não inclui:

## Contexto técnico
Crates/arquivos:
Contratos afetados:
Dependências:

## Critérios de aceite
- [ ] Comportamento principal
- [ ] Estado vazio/erro
- [ ] Cancelamento ou concorrência, quando aplicável
- [ ] Persistência, quando aplicável

## Testes
Unidade:
Integração:
Fluxo/UI:

## Documentação
README/REQUISITOS/configuração a atualizar:

## Entrega
Evidências:
Limitações restantes:
Commit:
```

## 19. Revisão da especificação

Uma alteração de requisito deve responder:

- o comportamento anterior está sendo corrigido ou substituído?
- há impacto em atalhos, settings, formatos persistidos ou APIs?
- algum teste deixa de representar o comportamento correto?
- o roadmap e os limites do estágio continuam verdadeiros?

Mudanças de visão devem alterar prioridades e roadmap. Mudanças de comportamento P0 exigem aceite e testes atualizados no mesmo incremento. Não reescrever IDs antigos apenas para refletir ordem visual; criar novos IDs e registrar substituição quando necessário.

## 20. Critério de sucesso do produto

O sucesso não será medido pela quantidade de menus ou pela semelhança isolada com outra IDE. Um marco é bem-sucedido quando um fluxo real pode ser repetido por outra pessoa, em ambiente limpo, com resultados previsíveis e sem intervenção do autor da implementação.

A prioridade permanente é: **correção do núcleo, fluxo completo, evidência de qualidade e então amplitude de recursos**.
