# PHPForge IDE — Plano de tarefas

> Documento derivado de `REQUISITOS.md`. A especificação define o comportamento esperado; este arquivo define a ordem e a unidade de execução.
> **Fila ativa:** Estágio 6.2 — superfície do editor.

## 1. Regras de uso

- Cada tarefa possui ID estável e referencia um requisito normativo.
- Uma tarefa deve produzir uma entrega pequena, testável e revisável, normalmente em um commit.
- A prioridade P0/P1/P2 é herdada do requisito associado.
- A coluna **Depende de** indica bloqueios técnicos mínimos. `BASE` significa usar o código existente e caracterizá-lo antes de alterá-lo.
- Uma caixa só pode ser marcada `[x]` depois que implementação, testes, documentação e revisão estiverem concluídos.
- A presença de shell visual não marca uma tarefa funcional como concluída.
- Os Estágios 0–6.1 permanecem registrados como entregues no README; este plano não reabre sua implementação histórica, apenas prevê consolidação quando o aceite normativo ainda não estiver completo.
- Tarefas P1/P2 de um requisito P0/P1/P2 permanecem separadas para não bloquear o fluxo essencial.
- Ao iniciar uma tarefa, registrar responsável e branch fora deste documento; ao concluir, registrar commit e evidência na entrega.
- Se uma tarefa precisar ser ampliada, criar subtarefas em vez de alterar silenciosamente seu aceite.

### Evidências abreviadas

| Sigla | Evidência |
|---|---|
| UT | Teste de unidade |
| CT | Teste de contrato/protocolo |
| IT | Teste de integração com processo, serviço ou ferramenta real |
| FT | Teste de fluxo do usuário |
| UI | Smoke test ou captura da interface |
| PERF | Benchmark ou medição reproduzível |
| DOC | Documentação e inventário atualizados |

## 2. Sequência de marcos

| Ordem | Marco | Gate de saída |
|---|---|---|
| 1 | Consolidação transversal necessária | Offsets, workers, processos e settings não bloqueiam o recorte ativo |
| 2 | Estágio 6.2 | Superfície do editor P0 integrada; P1 restante explicitado |
| 3 | Estágio 7.x | Fluxo PHP P0 e Composer inicial |
| 4 | Estágio 8.x | Run/Debug P0 e testes integrados |
| 5 | Estágio 9.0 | Frontend essencial |
| 6 | Estágio 10.0 | Git P0 |
| 7 | Estágio 11.x | DB read-only e depois edição transacional |
| 8 | Estágio 12.x | Ferramentas integradas |
| 9 | Estágio 13.0+ | Frameworks, drivers adicionais, IA e paridade avançada |

Não iniciar todas as frentes simultaneamente. Trabalho concorrente deve possuir arquivos e contratos delimitados.

## 3. Tarefas transversais de arquitetura e qualidade

### 3.1 Concorrência, documentos e processos

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [x] | `T-NFR-001-01` | `NFR-001` | Inventariar chamadas bloqueantes na UI e classificar por filesystem, processo, rede, LSP, Git e DB; DOC com plano de correção — entregue: `docs/baseline/T-NFR-001-01-ui-blocking-inventory.md` (DOC) | BASE |
| [x] | `T-NFR-001-02` | `NFR-001` | Criar abstração comum de job com estados, ID e retorno por canal; UT de conclusão e erro — entregue: `phpide-core::jobs` (JobSystem/JobHandle/estados/canal), UT. | `T-NFR-001-01` |
| [x] | `T-NFR-001-03` | `NFR-001` | Adicionar cancelamento e descarte de resultados obsoletos por projeto/documento/sessão; UT/IT Entregue: cancel cooperativo + `JobRegistry` (descarte por chave), UT + e2e fora de ordem. | `T-NFR-001-02` |
| [x] | `T-NFR-001-04` | `NFR-001` | Garantir cancelamento e encerramento de workers ao trocar projeto ou fechar a IDE; FT Entregue: `JobSystem::shutdown` na troca de projeto e `on_exit`; UT do shutdown. | `T-NFR-001-03` |
| [x] | `T-NFR-002-01` | `NFR-002` | Centralizar conversões byte/char/linha/UTF-16 e negociação de encoding; UT com Unicode e CRLF — entregue: `phpide-core::offsets` com negociação e conversões UTF-8/16/32; UT Unicode+CRLF | BASE |
| [ ] | `T-NFR-002-02` | `NFR-002` | Aplicar conversões centralizadas em LSP, diagnósticos, caret, folding e breakpoints; CT/IT | `T-NFR-002-01` |
| [ ] | `T-NFR-002-03` | `NFR-002` | Definir política de conflito para alterações externas com buffer dirty; UT/FT | BASE |
| [x] | `T-NFR-002-04` | `NFR-002` | Implementar reanexação/invalidação de estruturas por revisão do documento; UT Entregue: `Revisioned`/`PerFileRevision` + diagnósticos por revisão; UT + wiring UI. | `T-NFR-002-01` |
| [x] | `T-NFR-003-01` | `NFR-003` | Criar descoberta configurável de executáveis com diagnóstico de origem e versão; UT — entregue: crate `phpide-exec::discover` (explícito>PATH, origem, versão); UT | BASE |
| [x] | `T-NFR-003-02` | `NFR-003` | Padronizar spawn por vetor de argumentos, cwd, env, stdout/stderr e exit code; UT/IT Entregue: `CommandSpec` (vetor, cwd, env) + `Completed` (stdout/stderr/exit); UT/IT. | `T-NFR-003-01` |
| [x] | `T-NFR-003-03` | `NFR-003` | Implementar timeout, Stop e cleanup de processos-filhos; IT de término normal e forçado Entregue: timeout, Stop e kill de grupo (setsid+killpg); IT incl. processo-neto. | `T-NFR-001-02`, `T-NFR-003-02` |
| [x] | `T-NFR-003-04` | `NFR-003` | Criar mensagens e testes para ausência, permissão, versão incompatível e path com espaços; UT/IT/DOC Entregue: matriz de falhas em `phpide-exec/tests/failure_matrix.rs`. | `T-NFR-003-01` |

### 3.2 Configuração, logs e compatibilidade

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [x] | `T-NFR-004-01` | `NFR-004` | Adicionar versão e migrações incrementais ao settings.json; UT com configurações antigas — entregue: `settings_version` + cadeia de migrações (v1→v2); UT com configs antigas | BASE |
| [x] | `T-NFR-004-02` | `NFR-004` | Implementar escrita atômica, backup e recuperação de JSON inválido; UT Entregue: atômico + `.bak` + recuperação com `.invalid` preservado; UT. | `T-NFR-004-01` |
| [ ] | `T-NFR-004-03` | `NFR-004` | Separar preferências globais, configurações de projeto e dados compartilháveis; UT/DOC | `T-NFR-004-01` |
| [ ] | `T-NFR-004-04` | `NFR-004` | Criar porta de armazenamento de segredos e implementação por keyring, com fallback explícito; IT/DOC | `T-NFR-004-03` |
| [ ] | `T-NFR-004-05` | `NFR-004` | Excluir segredos de exportação, logs e arquivos versionáveis; UT/FT | `T-NFR-004-04` |
| [x] | `T-NFR-005-01` | `NFR-005` | Definir catálogo de erros e formato de mensagem com componente, operação e ação; DOC — entregue: catálogo `IdeError` (componente/operação/ação); DOC em `error.rs` + UT | BASE |
| [x] | `T-NFR-005-02` | `NFR-005` | Adicionar IDs de correlação para jobs, documentos e sessões; UT/IT Entregue: `Correlation` (job/doc/sessão) no `Logger`; uso em save/busca/sessão; UT. | `T-NFR-001-02` |
| [x] | `T-NFR-005-03` | `NFR-005` | Implementar redaction de tokens, senhas, env e dados sensíveis; UT com casos adversariais Entregue: `redact_text` + `Secret` no logger; UT adversariais. | `T-NFR-005-01` |
| [ ] | `T-NFR-005-04` | `NFR-005` | Exibir detalhes técnicos sem substituir mensagens úteis por eventos repetidos; UI/FT | `T-NFR-005-02` |
| [ ] | `T-NFR-010-01` | `NFR-010` | Definir versionamento de formatos de projeto, settings e manifests; DOC/UT | `T-NFR-004-01` |
| [ ] | `T-NFR-010-02` | `NFR-010` | Criar negotiation/capabilities para plugins e drivers; CT | `T-NFR-010-01` |
| [ ] | `T-NFR-010-03` | `NFR-010` | Adicionar feature flags para recursos experimentais com defaults seguros; UT/DOC | `T-NFR-010-01` |

### 3.3 Confiabilidade, segurança e acessibilidade

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [x] | `T-NFR-007-01` | `NFR-007` | Implementar save seguro contra escrita parcial e preservação do buffer em erro; UT/IT — entregue: `fsutil::write_atomic` (tmp+fsync+rename), buffer preservado em erro; UT | BASE |
| [x] | `T-NFR-007-02` | `NFR-007` | Cobrir fechamento de aba, split, projeto e aplicação com dirty confirmation; UT/FT Entregue: modal Salvar/Não Salvar/Cancelar em aba, Close Others, projeto e saída. | `T-NFR-007-01` |
| [ ] | `T-NFR-007-03` | `NFR-007` | Padronizar resultado de operações multiarquivo: sucesso, parcial e falha; UT/FT | `T-NFR-005-01` |
| [ ] | `T-NFR-007-04` | `NFR-007` | Testar crash/restart de cada processo externo sem derrubar a IDE; IT | `T-NFR-003-03` |
| [ ] | `T-NFR-008-01` | `NFR-008` | Criar modo de projeto não confiável e bloquear scripts/plugins/tasks automáticos; FT/DOC | BASE |
| [ ] | `T-NFR-008-02` | `NFR-008` | Revisar confirmações para comandos, consultas, upload e operações destrutivas; FT | `T-NFR-008-01` |
| [ ] | `T-NFR-008-03` | `NFR-008` | Criar fixtures sem segredos e testes que impeçam vazamento em logs/screenshots; UT/IT | `T-NFR-005-03` |
| [ ] | `T-NFR-008-04` | `NFR-008` | Inventariar licenças de dependências e assets em CI/release; DOC | BASE |
| [ ] | `T-NFR-009-01` | `NFR-009` | Auditar foco, tab order, atalhos e modais nos fluxos P0; DOC com lacunas | BASE |
| [ ] | `T-NFR-009-02` | `NFR-009` | Implementar foco visível e operação por teclado nos fluxos auditados; UI/FT | `T-NFR-009-01` |
| [ ] | `T-NFR-009-03` | `NFR-009` | Validar contraste, texto ampliado, Unicode e alternativas a cor/hover/ícone; UI | `T-NFR-009-01` |
| [x] | `T-NFR-006-01` | `NFR-006` | Criar fixtures de 10 mil linhas, projeto grande e resultset grande; DOC de geração — entregue: geradores determinísticos em `phpide-core::fixtures`; DOC de geração | BASE |
| [x] | `T-NFR-006-02` | `NFR-006` | Criar benchmark de digitação, scroll, busca e abertura; PERF baseline Entregue: benches `doc_ops`/`search_large` + `docs/baseline/T-NFR-006-02-perf-baseline.md`. | `T-NFR-006-01` |
| [ ] | `T-NFR-006-03` | `NFR-006` | Otimizar somente gargalos medidos e impedir regressões nos fluxos P0; PERF | `T-NFR-006-02` |

## 4. Interface: consolidação da shell

### 4.1 Window header, Navigation Bar e Main Menu

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-001-01` | `UI-001` | Auditar widgets, navegação por arquivo e estados vazios da shell existente; DOC/UI | BASE |
| [ ] | `T-UI-001-02` | `UI-001` | Consolidar Navigation Bar Bottom/Top/Hidden e persistência sem sobreposição; UT/UI | `T-UI-001-01` |
| [ ] | `T-UI-001-03` | `UI-001` | Implementar foco `Alt+Home` e navegação por teclado na Navigation Bar; FT | `T-NFR-009-01`, `T-UI-001-02` |
| [ ] | `T-UI-002-01` | `UI-002` | Implementar modos Merge, Hamburger e Show above com persistência; UT/UI | `T-UI-001-01` |
| [ ] | `T-UI-002-02` | `UI-002` | Sincronizar enabled state e atalhos do menu com comandos reais; UT/FT | `T-UI-002-01` |

### 4.2 Tool windows e status bar

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-TW-001-01` | `UI-TW-001` | Testar catálogo completo, títulos, conteúdo e atalhos `Alt+1…0`, Terminal e Database; UT/FT | BASE |
| [ ] | `T-UI-TW-002-01` | `UI-TW-002` | Consolidar Hide Active/All, More, Compact Mode e restauração de layout; UT/UI | BASE |
| [ ] | `T-UI-TW-002-02` | `UI-TW-002` | Implementar foco editor↔tool window com F12/Escape e isolamento de modais; FT | `T-NFR-009-01` |
| [ ] | `T-UI-TW-002-03` | `UI-TW-002` | Implementar split, drag/drop e redimensionamento por teclado; UI/FT | `T-UI-TW-002-01` |
| [ ] | `T-UI-TW-002-04` | `UI-TW-002` | Implementar speed search, Group Tabs e política de ícones no header; UT/UI | `T-UI-TW-002-01` |
| [ ] | `T-UI-SB-001-01` | `UI-SB-001` | Auditar cada widget contra estado real do buffer, Git, DBGp e deployment; DOC/UT | BASE |
| [ ] | `T-UI-SB-001-02` | `UI-SB-001` | Corrigir ações e indisponibilidade dos widgets sem falsos sucessos; FT/UI | `T-UI-SB-001-01` |
| [ ] | `T-UI-SB-002-01` | `UI-SB-002` | Implementar menu de visibilidade/ordem, Restore Defaults e persistência; UT/UI | `T-NFR-004-01`, `T-UI-SB-001-01` |

### 4.3 Welcome, Settings e temas

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-WEL-001-01` | `UI-WEL-001` | Cobrir New/Open/Get from VCS e diretórios inválidos em ambiente temporário; IT/FT | BASE |
| [ ] | `T-UI-WEL-001-02` | `UI-WEL-001` | Consolidar recents ausentes, importação e seleção de tema/keymap; UT/FT | `T-NFR-004-02` |
| [ ] | `T-UI-WEL-001-03` | `UI-WEL-001` | Representar Plugins e Remote Development somente com backends reais; UI/DOC | BASE |
| [ ] | `T-UI-SET-001-01` | `UI-SET-001` | Implementar transação de edição do diálogo: OK, Apply, Cancel e Escape; UT/FT | `T-NFR-004-02` |
| [ ] | `T-UI-SET-001-02` | `UI-SET-001` | Completar busca e navegação por teclado na árvore; FT | `T-NFR-009-01` |
| [ ] | `T-UI-SET-001-03` | `UI-SET-001` | Criar páginas funcionais sob demanda dos requisitos de domínio, removendo placeholders correspondentes; FT/DOC | `T-UI-SET-001-01` |
| [ ] | `T-UI-THEME-001-01` | `UI-THEME-001` | Validar sete temas e contraste em todos os painéis; UI | BASE |
| [ ] | `T-UI-THEME-001-02` | `UI-THEME-001` | Separar UI theme e editor Color Scheme com persistência; UT/UI | `T-NFR-004-01`, `T-UI-THEME-001-01` |
| [ ] | `T-UI-THEME-001-03` | `UI-THEME-001` | Implementar Sync with OS, fontes/ligatures e ajustes de acessibilidade; UT/UI | `T-UI-THEME-001-02`, `T-NFR-009-03` |

### 4.4 Sistema de iconografia

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-ICON-001-01` | `UI-ICON-001` | Selecionar conjunto visual inicial no Icones/Iconify ou alternativa; DOC com amostra para menus, ações, arquivos e estados | BASE |
| [ ] | `T-UI-ICON-001-02` | `UI-ICON-001` | Baixar somente SVGs usados e criar manifesto com nome, origem, autor, URL e licença; DOC | `T-UI-ICON-001-01`, `T-NFR-008-04` |
| [ ] | `T-UI-ICON-001-03` | `UI-ICON-001` | Implementar registry semântico, carregamento local, cache e fallback; UT | `T-UI-ICON-001-02` |
| [ ] | `T-UI-ICON-001-04` | `UI-ICON-001` | Implementar tint por tema, tamanhos 16/20/24, HiDPI e tooltip/nome acessível; UT/UI | `T-UI-ICON-001-03`, `T-NFR-009-03` |
| [ ] | `T-UI-ICON-001-05` | `UI-ICON-001` | Migrar controles novos do Estágio 6.2 sem alterar comportamento; UI | `T-UI-ICON-001-04` |
| [ ] | `T-UI-ICON-001-06` | `UI-ICON-001` | Migrar menus, tool windows, tabs e status bar por pequenos lotes; UI/FT | `T-UI-ICON-001-05` |
| [ ] | `T-UI-ICON-001-07` | `UI-ICON-001` | Validar abertura offline, assets ausentes, contraste e cobertura do inventário; IT/UI/DOC | `T-UI-ICON-001-06` |

## 5. Estágio 6.2 — superfície do editor — FILA ATIVA

Executar na ordem sempre que houver dependência de geometria ou estado. Não incluir Database, refatorações, novos run configurations ou implementação semântica própria nesta fila.

### 5.1 Baseline e geometria

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [x] | `T-UI-ED-000-01` | `UI-ED-000` | Caracterizar comportamento atual de scroll, caret, seleção, gutter, tabs e zoom; DOC/UI de baseline  — entregue: `docs/baseline/T-UI-ED-000-01-editor-baseline.md` (caracterização DOC/UI) | BASE |
| [x] | `T-UI-ED-000-02` | `UI-ED-000` | Centralizar mapa documento→linha visual→coordenada, incluindo gutter e viewport; UT  — entregue: `phpide-ui::geometry` puro + wrapper `EditorGeometry`; UT (15 testes) | `T-UI-ED-000-01`, `T-NFR-002-01` |
| [x] | `T-UI-ED-000-03` | `UI-ED-000` | Cobrir arquivo vazio, linhas longas, tabs, Unicode, zoom e gutters de diferentes larguras; UT/UI  — entregue: UT de vazio/Unicode/tabs/linhas longas/zoom/gutter por dígitos | `T-UI-ED-000-02` |
| [x] | `T-UI-ED-000-04` | `UI-ED-000` | Medir digitação e scroll no fixture de 10 mil linhas antes das novas camadas; PERF  — entregue: bench `viewport` (7.5ns/frame, 9.7ns/salto) registrado no baseline | `T-NFR-006-02`, `T-UI-ED-000-02` |

### 5.2 Gutter, bookmarks e breakpoints

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-ED-001-01` | `UI-ED-001` | Renderizar números e hit areas com a geometria compartilhada; UT/UI | `T-UI-ED-000-03` |
| [ ] | `T-UI-ED-001-02` | `UI-ED-001` | Integrar bookmarks e breakpoints por mouse/atalho ao backend existente; UT/FT | `T-UI-ED-001-01` |
| [ ] | `T-UI-ED-001-03` | `UI-ED-001` | Reanexar indicadores após edições e isolar estado por buffer; UT | `T-NFR-002-04`, `T-UI-ED-001-02` |
| [ ] | `T-UI-ED-001-04` | `UI-ED-001` | Representar estados enabled/disabled/verified/invalid/muted sem inventar confirmação do backend; CT/UI | `T-UI-ED-001-02`, `T-RUN-DBG-003-01` |
| [ ] | `T-UI-ED-001-05` | `UI-ED-001` | Validar scroll, folding, zoom e arquivo grande sem desenho fora da viewport; UI/PERF | `T-UI-ED-002-03`, `T-UI-ED-001-03` |

### 5.3 Folding estrutural

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-ED-002-01` | `UI-ED-002` | Gerar ranges Tree-sitter para classes, métodos, blocos, arrays e comentários; UT com aninhamento | BASE |
| [ ] | `T-UI-ED-002-02` | `UI-ED-002` | Criar estado de folding por revisão e política de persistência/reabertura; UT/DOC | `T-NFR-002-04`, `T-UI-ED-002-01` |
| [ ] | `T-UI-ED-002-03` | `UI-ED-002` | Renderizar fold/unfold, hover/Always e ajustar caret, seleção, scroll e cliques; UT/UI/FT | `T-UI-ED-000-03`, `T-UI-ED-002-02` |
| [ ] | `T-UI-ED-002-04` | `UI-ED-002` | Tratar syntax error, ranges inválidos e edição dentro/antes de regiões; UT/FT | `T-UI-ED-002-03` |

### 5.4 Tabs, split e detach

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [x] | `T-UI-ED-003-01` | `UI-ED-003` | Auditar e completar Closing Policy, preview, pin, Close Others, dirty confirmation e tabs ocultas; UT/FT  — entregue: matriz em `docs/baseline/T-UI-ED-003-01-tabs-audit.md`; `tab_limit` configurável com UT | BASE |
| [ ] | `T-UI-ED-003-02` | `UI-ED-003` | Implementar Switcher `Ctrl+Tab` com ordem MRU e roteamento de teclado; UT/FT | `T-UI-ED-003-01` |
| [ ] | `T-UI-ED-003-03` | `UI-ED-003` | Criar modelo de editor groups com vistas independentes e documento compartilhado; UT | `T-NFR-002-04`, `T-UI-ED-003-01` |
| [ ] | `T-UI-ED-003-04` | `UI-ED-003` | Implementar Split Right/Down, foco, caret, scroll e fechamento por vista; UT/UI/FT | `T-UI-ED-003-03` |
| [ ] | `T-UI-ED-003-05` | `UI-ED-003` | Implementar drag/drop de tabs e tab placement configurável; UT/UI | `T-UI-ED-003-04` |
| [ ] | `T-UI-ED-003-06` | `UI-ED-003` | Avaliar backend de múltiplas janelas e registrar decisão para detach `Shift+F4`; DOC | `T-UI-ED-003-03` |
| [ ] | `T-UI-ED-003-07` | `UI-ED-003` | Implementar detach/reattach preservando buffer, dirty state e navegação; FT | `T-UI-ED-003-06` |

### 5.5 Sticky lines

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-ED-004-01` | `UI-ED-004` | Obter pilha namespace/classe/função a partir do topo da viewport; UT | `T-UI-ED-002-01` |
| [ ] | `T-UI-ED-004-02` | `UI-ED-004` | Renderizar até cinco sticky lines sem interceptar eventos incorretamente; UT/UI | `T-UI-ED-000-03`, `T-UI-ED-004-01` |
| [ ] | `T-UI-ED-004-03` | `UI-ED-004` | Implementar clique, configuração por linguagem e limite zero; UT/FT | `T-UI-ED-004-02`, `T-NFR-004-01` |
| [ ] | `T-UI-ED-004-04` | `UI-ED-004` | Validar composição com folding, split e scroll sem flicker; UI/PERF | `T-UI-ED-002-03`, `T-UI-ED-003-04`, `T-UI-ED-004-03` |

### 5.6 Inlay hints e Code Vision

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-ED-005-01` | `UI-ED-005` | Definir provedor assíncrono e modelo de hints por revisão; CT com resposta atrasada | `T-NFR-001-03`, `T-NFR-002-04` |
| [ ] | `T-UI-ED-005-02` | `UI-ED-005` | Integrar inlay hints LSP e estados de servidor ausente; IT/FT | `T-PHP-001-03`, `T-UI-ED-005-01` |
| [ ] | `T-UI-ED-005-03` | `UI-ED-005` | Renderizar hints fora do documento, sem afetar save, busca ou clipboard; UT/UI | `T-UI-ED-000-03`, `T-UI-ED-005-02` |
| [ ] | `T-UI-ED-005-04` | `UI-ED-005` | Implementar All arguments, Pass by reference, toggle global e exclude list; UT/FT | `T-UI-ED-005-03`, `T-NFR-004-01` |
| [ ] | `T-UI-ED-005-05` | `UI-ED-005` | Implementar Code Vision Inheritors/Usages com navegação a resultados reais; IT/FT | `T-PHP-004-03`, `T-UI-ED-005-01` |
| [ ] | `T-UI-ED-005-06` | `UI-ED-005` | Implementar Code author usando dados Git reais; IT/UI | `T-GIT-004-02`, `T-UI-ED-005-01` |

### 5.7 Lens mode, scrollbar, blame e coverage

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-ED-006-01` | `UI-ED-006` | Renderizar markers agregados de diagnósticos e mudanças na scrollbar; UT/UI/PERF | `T-PHP-002-01`, `T-GIT-004-01` |
| [ ] | `T-UI-ED-006-02` | `UI-ED-006` | Implementar lens preview ancorado à região e ao tema; UT/UI | `T-UI-ED-000-03` |
| [ ] | `T-UI-ED-006-03` | `UI-ED-006` | Implementar Inspections widget com escopo e abertura correta de Problems; FT | `T-PHP-002-02` |
| [ ] | `T-UI-ED-007-01` | `UI-ED-007` | Renderizar blame com autor/revisão/data e invalidação por edição; IT/UI | `T-GIT-004-02`, `T-NFR-002-04` |
| [ ] | `T-UI-ED-007-02` | `UI-ED-007` | Renderizar coverage real e estado desconhecido sem confundir ausência com sucesso; IT/UI | `T-RUN-TST-003-02`, `T-NFR-002-04` |

### 5.8 Comportamentos complementares e gate 6.2

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-UI-ED-008-01` | `UI-ED-008` | Implementar configurações de caret, occurrences, whitespace, guides e separators; UT/UI | `T-NFR-004-01` |
| [ ] | `T-UI-ED-008-02` | `UI-ED-008` | Implementar method separators e HTML tag tree highlighting; UT/UI | `T-UI-ED-008-01`, `T-FE-001-01` |
| [ ] | `T-UI-ED-009-01` | `UI-ED-009` | Criar registry de ações contextuais e menu `Alt+Enter`; CT/FT | `T-PHP-010-01` |
| [ ] | `T-UI-ED-009-02` | `UI-ED-009` | Implementar lâmpada e preview de quick-fix com diff/undo; IT/UI | `T-UI-ED-009-01`, `T-PHP-010-03` |
| [x] | `T-UI-ED-010-01` | `UI-ED-010` | Auditar atalhos, limites e persistência de zoom; UT/FT  — entregue: auditoria no baseline do editor; `clamp_zoom` com UT e roundtrip | BASE |
| [ ] | `T-UI-ED-010-02` | `UI-ED-010` | Implementar Ctrl+wheel opcional, line height e ligatures; UT/UI | `T-UI-ED-010-01`, `T-UI-THEME-001-03` |
| [ ] | `T-UI-ED-GATE-01` | `UI-ED-000` | Executar gate integrado: gutter+folding+tabs sob scroll/zoom, smoke gráfico e fixture de 10 mil linhas; UI/PERF/DOC | `T-UI-ED-001-05`, `T-UI-ED-002-04`, `T-UI-ED-003-04`, `T-UI-ED-000-04` |
| [ ] | `T-UI-ED-GATE-02` | `UI-ED-003` | Atualizar README, roadmap e pendências P1 sem declarar recursos ausentes como prontos; DOC | `T-UI-ED-GATE-01` |

## 6. Estágio 7.x — Núcleo PHP e Composer

### 6.1 LSP, diagnósticos, completion e navegação

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [x] | `T-PHP-001-01` | `PHP-001` | Consolidar descoberta e seleção Intelephense/Phpactor com capabilities e versão; UT/IT  Entregue: `ServerCapabilities` parse do initialize; recursos gated por capability; seleção via detect_server; UT/IT. | `T-NFR-003-01` |
| [x] | `T-PHP-001-02` | `PHP-001` | Implementar supervisionamento starting/ready/degraded/stopped/failed e restart; IT  Entregue: estados Starting/Running/Failed, crash não vira falso positivo (stopping), restart via palette; IT. | `T-NFR-003-03`, `T-PHP-001-01` |
| [x] | `T-PHP-001-03` | `PHP-001` | Garantir ordenação de didOpen/Change/Save/Close e versões por documento; CT/IT  Entregue: `intelligence::plan_sync` (didOpen→didChange→didClose, sem reprogrer versão); UT. | `T-NFR-002-02`, `T-PHP-001-02` |
| [x] | `T-PHP-001-04` | `PHP-001` | Isolar sessões por projeto e cancelar respostas após fechamento/troca; IT/FT  Entregue: troca de projeto descarta sessão; definição/outline/completion descartam por (path, revisão); log correlacionado. | `T-NFR-001-03`, `T-PHP-001-03` |
| [x] | `T-PHP-001-05` | `PHP-001` | Manter edição e diagnósticos locais em ausência/crash do servidor; IT/FT/DOC  Entregue: crash mantém edição e diagnósticos locais (tree-sitter), mensagem com ação de restart; UT gate_tests. | `T-PHP-001-02` |
| [x] | `T-PHP-002-01` | `PHP-002` | Armazenar diagnósticos por arquivo/revisão e descartar publicações obsoletas; UT/CT  Entregue: diagnósticos por (arquivo, revisão) no Workspace; invalidação por revisão já integrada; UT. | `T-PHP-001-03` |
| [x] | `T-PHP-002-02` | `PHP-002` | Implementar Problems por arquivo/projeto/serviço com origem da ferramenta; UT/FT  Entregue: Problems com escopo projeto/arquivo e resumo por ferramenta de origem; UT de agrupamento. | `T-PHP-002-01` |
| [x] | `T-PHP-002-03` | `PHP-002` | Completar F2/Shift+F2 e política ao fechar/corrigir arquivos; UT/FT  Entregue: F2/Shift+F2 com wrap-around via `next_problem_index`; correção remove por revisão; UT. | `T-PHP-002-02` |
| [x] | `T-PHP-003-01` | `PHP-003` | Implementar request/cancel de completion e gatilhos show-as-you-type; CT/IT  Entregue: completion com id + `$/cancelRequest` no fechar/reiniciar; gatilhos `$`,`->`,`\`; UT. | `T-PHP-001-03` |
| [x] | `T-PHP-003-02` | `PHP-003` | Aplicar insertText/textEdit com ranges Unicode e aceite Enter/Tab; CT/UT/FT  Entregue: textEdit/insertText/textEditText com ranges UTF-16 via conversões centrais (Unicode/CRLF); UT. | `T-NFR-002-01`, `T-PHP-003-01` |
| [x] | `T-PHP-003-03` | `PHP-003` | Adicionar documentação, filtro e popup sem bloquear digitação; IT/UI/PERF  Entregue: documentação do item selecionado no popup; filtro próprio; resposta assíncrona sem bloqueio. | `T-PHP-003-02` |
| [ ] | `T-PHP-003-04` | `PHP-003` | Integrar live templates registrados e precedência com completion; UT/FT | `T-PHP-020-01`, `T-PHP-003-02` |
| [ ] | `T-PHP-003-05` | `PHP-003` | Implementar auto-import como WorkspaceEdit e undo único; IT/FT | `T-PHP-021-01`, `T-PHP-003-02` |
| [ ] | `T-PHP-003-06` | `PHP-003` | Implementar type-matching, statement completion, Hippie, postfix e parameter info; CT/FT | `T-PHP-003-02` |
| [ ] | `T-PHP-003-07` | `PHP-003` | Implementar configurações de match case, single suggestion, sort e documentação; UT/FT | `T-PHP-003-03`, `T-NFR-004-01` |
| [x] | `T-PHP-004-01` | `PHP-004` | Integrar Declaration/Implementation/Type Declaration com histórico de navegação; IT/FT  Entregue: definition/implementation/typeDefinition com gate por capability e UTF-16; IT com intelephense real. | `T-PHP-001-03` |
| [x] | `T-PHP-004-02` | `PHP-004` | Implementar Structure e Go to Symbol com resultados por projeto; IT/FT  Entregue: Structure (documentSymbol por revisão) + Go to Symbol (workspace/symbol assíncrono na palette). | `T-PHP-001-03` |
| [x] | `T-PHP-004-03` | `PHP-004` | Implementar Find Usages com preview e navegação; IT/FT  Entregue: Find Usages (references) alimenta o painel Find com preview da linha. | `T-PHP-004-01` |
| [ ] | `T-PHP-004-04` | `PHP-004` | Implementar Type Hierarchy e Related Symbol; IT/FT | `T-PHP-004-03` |
| [x] | `T-PHP-005-01` | `PHP-005` | Integrar semantic tokens por revisão sem flicker; CT/UI  Entregue: semanticTokens/full decodificado (delta encoding, UTF-16), cores por tipo no layouter, por revisão; UT. | `T-PHP-001-03` |
| [x] | `T-PHP-005-02` | `PHP-005` | Implementar highlight de ocorrências e fallback sintático offline; UT/UI  Entregue: ocorrências documentHighlight pintadas na galley; fallback sintático offline por palavra; UT. | `T-PHP-005-01` |

### 6.2 Inspeções e intenções

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-PHP-010-01` | `PHP-010` | Criar modelo comum de inspeção, regra, severidade, profile e origem; UT | BASE |
| [ ] | `T-PHP-010-02` | `PHP-010` | Implementar profiles/escopos e atualização de severidade sem rerun; UT/FT | `T-PHP-010-01`, `T-NFR-004-01` |
| [ ] | `T-PHP-010-03` | `PHP-010` | Implementar quick-fix/WorkspaceEdit com preview e undo; CT/FT | `T-PHP-021-01`, `T-PHP-010-01` |
| [ ] | `T-PHP-010-04` | `PHP-010` | Implementar batch Inspect Code com paridade de regras e escopo; IT/FT | `T-PHP-010-02` |
| [ ] | `T-PHP-010-05` | `PHP-010` | Implementar disable por regra/arquivo/escopo e auditoria; UT/DOC | `T-PHP-010-02` |
| [ ] | `T-PHP-011-01` | `PHP-011` | Criar catálogo rastreável das categorias e indicar nativo/externo/indisponível; DOC | `T-PHP-010-01` |
| [ ] | `T-PHP-011-02` | `PHP-011` | Implementar cada inspeção nativa em subtarefa com positivos, negativos, severidade e fix; UT | `T-PHP-011-01` |
| [ ] | `T-PHP-011-03` | `PHP-011` | Integrar categorias externas sem contá-las como nativas; CT/DOC | `T-PHP-040-01` |
| [ ] | `T-PHP-012-01` | `PHP-012` | Implementar intenções iniciais de propriedade/parâmetro com preview; UT/FT | `T-UI-ED-009-01` |
| [ ] | `T-PHP-012-02` | `PHP-012` | Implementar ArrayShape/ObjectShape, Composer script, enable inspection e Move HTTP Request; UT/FT | `T-PHP-022-01`, `T-PHP-031-02`, `T-RUN-HTTP-001-01` |

### 6.3 Templates, refatorações e PHPDoc

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-PHP-020-01` | `PHP-020` | Criar engine de templates com variáveis, `$END$`, `$SELECTION$` e undo único; UT | BASE |
| [ ] | `T-PHP-020-02` | `PHP-020` | Adicionar grupos, surround, reformat e templates `rqr`, `pubsf`, `fore`, `eco`; UT/FT | `T-PHP-020-01` |
| [ ] | `T-PHP-020-03` | `PHP-020` | Implementar import/export versionado e validação de templates; UT/DOC | `T-NFR-010-01`, `T-PHP-020-01` |
| [ ] | `T-PHP-021-01` | `PHP-021` | Criar infraestrutura de WorkspaceEdit multiarquivo, conflitos, preview e undo coeso; UT/IT | `T-NFR-007-03` |
| [ ] | `T-PHP-021-02` | `PHP-021` | Implementar Rename com referências e conflitos; IT/FT | `T-PHP-004-03`, `T-PHP-021-01` |
| [ ] | `T-PHP-021-03` | `PHP-021` | Implementar Extract Variable e Extract Method; UT/FT | `T-PHP-021-01` |
| [ ] | `T-PHP-021-04` | `PHP-021` | Implementar Change Signature e propagação por hierarquia; IT/FT | `T-PHP-004-04`, `T-PHP-021-01` |
| [ ] | `T-PHP-021-05` | `PHP-021` | Implementar Safe Delete com usos e Refactor Anyway explícito; IT/FT | `T-PHP-004-03`, `T-PHP-021-01` |
| [ ] | `T-PHP-021-06` | `PHP-021` | Implementar Move/Copy e demais Extract/Inline/Hierarchy refactors em subtarefas separadas; IT/FT | `T-PHP-021-02` |
| [ ] | `T-PHP-022-01` | `PHP-022` | Suportar array/object shapes em PHPDoc/atributos com fixtures; CT/UT | `T-PHP-001-03` |
| [ ] | `T-PHP-022-02` | `PHP-022` | Suportar integer ranges e diagnóstico de incompatibilidade; CT/UT | `T-PHP-010-01` |
| [ ] | `T-PHP-022-03` | `PHP-022` | Suportar anotações Psalm, aliases e imports com declaração do provedor; CT/DOC | `T-PHP-040-03` |
| [ ] | `T-PHP-022-04` | `PHP-022` | Suportar generics com casos explícitos e limitações documentadas; CT/DOC | `T-PHP-022-01` |

### 6.4 Composer e ferramentas de qualidade

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [x] | `T-PHP-030-01` | `PHP-030` | Ler composer.json, require.php e autoload PSR-0/4 sem executar código; UT/IT  Entregue: `phpide-php::composer` (composer.json, constraint PHP, PSR-4/0); UT. | BASE |
| [x] | `T-PHP-030-02` | `PHP-030` | Mapear roots, vendor e language level preservando overrides explícitos; UT/FT  Entregue: source/test roots, vendor, language level mapeados; nunca sobrescreve config manual; UT. | `T-PHP-030-01` |
| [x] | `T-PHP-030-03` | `PHP-030` | Detectar mudanças externas e vendor ausente com reload/notificação; IT/FT  Entregue: watcher re-executa descoberta por mtime; vendor ausente vira aviso acionável (sem install automático); UT/IT. | `T-PHP-030-02` |
| [ ] | `T-PHP-030-04` | `PHP-030` | Isolar múltiplos composer.json por subprojeto; IT/FT | `T-PHP-030-02` |
| [ ] | `T-PHP-031-01` | `PHP-031` | Implementar executor Composer com interpretador configurado, log e cancelamento; IT | `T-NFR-003-03`, `T-PHP-030-01` |
| [ ] | `T-PHP-031-02` | `PHP-031` | Implementar comandos read-only e scripts com output/código de saída; IT/FT | `T-PHP-031-01` |
| [ ] | `T-PHP-031-03` | `PHP-031` | Implementar install/update/init e operações modificativas com confirmação; IT/FT | `T-NFR-008-02`, `T-PHP-031-01` |
| [ ] | `T-PHP-032-01` | `PHP-032` | Implementar completion/docs/navegação no composer.json; IT/FT | `T-PHP-030-01` |
| [ ] | `T-PHP-032-02` | `PHP-032` | Implementar create-project e gestão explícita de dependências/repositórios; IT/FT | `T-PHP-031-03` |
| [ ] | `T-PHP-040-01` | `PHP-040` | Criar contrato comum de quality tool com versão, config, diagnósticos, formatter e cancelamento; CT | `T-NFR-003-02`, `T-PHP-010-01` |
| [ ] | `T-PHP-040-02` | `PHP-040` | Integrar PHPStan/Larastan com neon e saída estruturada; IT | `T-PHP-040-01` |
| [ ] | `T-PHP-040-03` | `PHP-040` | Integrar Psalm com XML e cache opcional; IT | `T-PHP-040-01` |
| [ ] | `T-PHP-040-04` | `PHP-040` | Integrar PHP CS Fixer e External Formatter com diff/undo; IT/FT | `T-PHP-040-01`, `T-PHP-021-01` |
| [ ] | `T-PHP-040-05` | `PHP-040` | Integrar Laravel Pint; IT/FT | `T-PHP-040-04` |
| [ ] | `T-PHP-040-06` | `PHP-040` | Integrar PHPCS/phpcbf e standards customizados; IT/FT | `T-PHP-040-01` |
| [ ] | `T-PHP-040-07` | `PHP-040` | Integrar PHP Mess Detector sem autofix implícito; IT | `T-PHP-040-01` |
| [ ] | `T-PHP-040-08` | `PHP-040` | Cobrir versão incompatível, config ausente, saída inválida, cancelamento e paths com espaços; CT/IT | `T-PHP-040-02`, `T-PHP-040-03`, `T-PHP-040-04`, `T-PHP-040-06`, `T-PHP-040-07` |

## 7. Estágio 8.x — Execução, debug, testes e HTTP

### 7.1 Run configurations

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-RUN-001-01` | `RUN-001` | Criar modelo versionado de configuração, validação e escopo projeto/global; UT | `T-NFR-010-01` |
| [ ] | `T-RUN-001-02` | `RUN-001` | Implementar sessões com allow multiple, working dir, env, output e status; UT/IT | `T-NFR-003-03`, `T-RUN-001-01` |
| [ ] | `T-RUN-001-03` | `RUN-001` | Implementar temporary/save, Rerun, Stop, Pause Output, Pin Tab e process list; FT/UI | `T-RUN-001-02` |
| [ ] | `T-RUN-001-04` | `RUN-001` | Implementar before-launch tasks com ordem, falha e cancelamento; UT/FT | `T-RUN-001-02` |
| [ ] | `T-RUN-001-05` | `RUN-001` | Aplicar redaction de env e segredos nos logs; UT/IT | `T-NFR-005-03`, `T-RUN-001-02` |
| [ ] | `T-RUN-002-01` | `RUN-002` | Implementar PHP Script completo e validação de campos; IT/FT | `T-RUN-001-02` |
| [ ] | `T-RUN-002-02` | `RUN-002` | Implementar Composer Script; IT/FT | `T-PHP-031-02`, `T-RUN-001-02` |
| [ ] | `T-RUN-002-03` | `RUN-002` | Implementar PHPUnit básico por escopo; IT/FT | `T-RUN-001-02` |
| [ ] | `T-RUN-002-04` | `RUN-002` | Implementar PHP Web Page, Remote Debug, HTTP Request e Pest; IT/FT | `T-RUN-DBG-004-01`, `T-RUN-HTTP-001-03`, `T-RUN-TST-002-02` |
| [ ] | `T-RUN-002-05` | `RUN-002` | Implementar Behat, PHPSpec, Codeception, ParaTest, Mutation e configurações compostas; IT/FT | `T-RUN-TST-002-03`, `T-RUN-TST-002-04` |

### 7.2 Xdebug/DBGp

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-RUN-DBG-001-01` | `RUN-DBG-001` | Consolidar listener 9003, IDE key e XDEBUG_CONFIG sem apagar env necessária; UT/IT | BASE |
| [ ] | `T-RUN-DBG-001-02` | `RUN-DBG-001` | Implementar path mapping local e associação da conexão ao projeto; UT/IT | `T-RUN-DBG-001-01` |
| [ ] | `T-RUN-DBG-001-03` | `RUN-DBG-001` | Cobrir breakpoint, frames, variáveis, Resume e Detach em fake Xdebug e PHP real; CT/IT/FT | `T-RUN-DBG-001-02` |
| [ ] | `T-RUN-DBG-001-04` | `RUN-DBG-001` | Garantir Stop/Detach com fechamento de socket/processos e UI responsiva; IT | `T-NFR-003-03`, `T-RUN-DBG-001-03` |
| [ ] | `T-RUN-DBG-002-01` | `RUN-DBG-002` | Implementar stepping F7/F8/Shift+F8/F9/Alt+F10 e migração de atalhos; IT/FT/DOC | `T-RUN-DBG-001-03` |
| [ ] | `T-RUN-DBG-002-02` | `RUN-DBG-002` | Implementar Watches e Evaluate por frame com invalidação após Resume; CT/IT/FT | `T-RUN-DBG-002-01` |
| [ ] | `T-RUN-DBG-002-03` | `RUN-DBG-002` | Implementar Set Value, inline values, visualização JSON/XML e REPL; CT/IT/UI | `T-RUN-DBG-002-02` |
| [ ] | `T-RUN-DBG-003-01` | `RUN-DBG-003` | Implementar modelo de estados/propriedades de breakpoints e sincronização com backend; CT/UT | `T-RUN-DBG-001-03` |
| [ ] | `T-RUN-DBG-003-02` | `RUN-DBG-003` | Implementar conditional, exception, method, logpoint e hit behavior; IT/FT | `T-RUN-DBG-003-01` |
| [ ] | `T-RUN-DBG-003-03` | `RUN-DBG-003` | Implementar grupos, mute, triggers e skipped paths; UT/IT | `T-RUN-DBG-003-01` |
| [ ] | `T-RUN-DBG-004-01` | `RUN-DBG-004` | Implementar PHP Servers, web path mappings e validação de configuração; IT/FT | `T-RUN-DBG-001-02` |
| [ ] | `T-RUN-DBG-004-02` | `RUN-DBG-004` | Implementar sessões simultâneas e isolamento por projeto; IT/FT | `T-RUN-DBG-001-04` |
| [ ] | `T-RUN-DBG-004-03` | `RUN-DBG-004` | Implementar proxy DBGp multiuser e túnel SSH; IT/DOC | `T-RUN-DBG-004-01` |
| [ ] | `T-RUN-DBG-004-04` | `RUN-DBG-004` | Avaliar e implementar Zend Debugger como adaptador separado; CT/IT/DOC | `T-RUN-DBG-001-03` |

### 7.3 Testes, coverage e servidor web

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-RUN-TST-001-01` | `RUN-TST-001` | Implementar descoberta/execução por diretório, classe, método e data set; IT/FT | `T-RUN-002-03` |
| [ ] | `T-RUN-TST-001-02` | `RUN-TST-001` | Implementar árvore, duração, output e navegação à falha; IT/UI | `T-RUN-TST-001-01` |
| [ ] | `T-RUN-TST-001-03` | `RUN-TST-001` | Implementar Rerun Automatically com debounce/cancelamento; UT/IT | `T-NFR-001-03`, `T-RUN-TST-001-01` |
| [ ] | `T-RUN-TST-001-04` | `RUN-TST-001` | Implementar test↔subject e geração por template; UT/FT | `T-PHP-004-01`, `T-RUN-TST-001-01` |
| [ ] | `T-RUN-TST-002-01` | `RUN-TST-002` | Consolidar PHPUnit via Composer/phar, config e bootstrap; IT | `T-RUN-002-03` |
| [ ] | `T-RUN-TST-002-02` | `RUN-TST-002` | Integrar Pest, Parallel e Mutation com verificação de versões; IT | `T-RUN-TST-002-01` |
| [ ] | `T-RUN-TST-002-03` | `RUN-TST-002` | Integrar Behat e PHPSpec com fixtures próprios; IT | `T-RUN-TST-001-01` |
| [ ] | `T-RUN-TST-002-04` | `RUN-TST-002` | Integrar Codeception e ParaTest; IT | `T-RUN-TST-002-01` |
| [ ] | `T-RUN-TST-003-01` | `RUN-TST-003` | Definir modelo de cobertura por arquivo/linha executável e importador Xdebug/PCOV; CT/UT | `T-RUN-TST-002-01` |
| [ ] | `T-RUN-TST-003-02` | `RUN-TST-003` | Apresentar cobertura por arquivo e alimentar o editor sem inferir resultado de testes; IT/FT | `T-RUN-TST-003-01` |
| [ ] | `T-RUN-WEB-001-01` | `RUN-WEB-001` | Implementar lifecycle do servidor 63342, estático e php -S; IT | `T-NFR-003-03` |
| [ ] | `T-RUN-WEB-001-02` | `RUN-WEB-001` | Implementar browser/preview e políticas seguras para conexões/requests; IT/FT/DOC | `T-NFR-008-02`, `T-RUN-WEB-001-01` |

### 7.4 HTTP Client

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-RUN-HTTP-001-01` | `RUN-HTTP-001` | Implementar parser de `.http`/`.rest`, `###`, nomes, headers, corpo e highlight; UT | BASE |
| [ ] | `T-RUN-HTTP-001-02` | `RUN-HTTP-001` | Implementar environments públicos/privados, variáveis e proteção de arquivos privados; UT/DOC | `T-NFR-004-04`, `T-RUN-HTTP-001-01` |
| [ ] | `T-RUN-HTTP-001-03` | `RUN-HTTP-001` | Implementar execução, auth, timeout, cancelamento e identificação do environment; IT/FT | `T-NFR-008-02`, `T-RUN-HTTP-001-02` |
| [ ] | `T-RUN-HTTP-001-04` | `RUN-HTTP-001` | Implementar viewer por content-type, histórico e cookies com limites; IT/UI | `T-RUN-HTTP-001-03` |
| [ ] | `T-RUN-HTTP-001-05` | `RUN-HTTP-001` | Implementar conversão/export cURL e redirect de resposta; UT/IT | `T-RUN-HTTP-001-03` |
| [ ] | `T-RUN-HTTP-001-06` | `RUN-HTTP-001` | Implementar scripts pre-request/response com sandbox e ação explícita; IT/DOC | `T-NFR-008-01`, `T-RUN-HTTP-001-03` |
| [ ] | `T-RUN-HTTP-001-07` | `RUN-HTTP-001` | Implementar GraphQL, WebSocket, multipart, OpenAPI/Postman e Crypto API em subtarefas; CT/IT | `T-RUN-HTTP-001-04`, `T-RUN-HTTP-001-06` |

## 8. Estágio 9.0 — Frontend

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-FE-001-01` | `FE-001` | Integrar highlight/diagnósticos HTML, CSS e JS por serviços especializados; IT | `T-PHP-001-02` |
| [ ] | `T-FE-001-02` | `FE-001` | Implementar completion, navegação e auto-import JavaScript; IT/FT | `T-FE-001-01`, `T-PHP-003-05` |
| [ ] | `T-FE-001-03` | `FE-001` | Implementar Emmet, MDN e color preview; UT/UI/FT | `T-FE-001-01` |
| [ ] | `T-FE-001-04` | `FE-001` | Indexar node_modules como biblioteca sem varredura redundante; IT/PERF | `T-NFR-006-02`, `T-FE-001-01` |
| [ ] | `T-FE-001-05` | `FE-001` | Validar projeto Vite simples de ponta a ponta; FT/DOC | `T-FE-001-02`, `T-FE-001-03`, `T-FE-001-04` |
| [ ] | `T-FE-002-01` | `FE-002` | Integrar TypeScript Language Service e seleção de versão; IT | `T-FE-001-01` |
| [ ] | `T-FE-002-02` | `FE-002` | Implementar Project Errors, hints, Type Declaration e import type; IT/FT | `T-FE-002-01` |
| [ ] | `T-FE-002-03` | `FE-002` | Avaliar TypeScript-Go como adaptador separado com testes de paridade; CT/DOC | `T-FE-002-02` |
| [ ] | `T-FE-003-01` | `FE-003` | Integrar React/JSX, props, templates e HTML→JSX; IT/FT | `T-FE-002-02` |
| [ ] | `T-FE-003-02` | `FE-003` | Integrar Angular Language Service, templates, signals e schematics; IT/FT | `T-FE-002-02` |
| [ ] | `T-FE-003-03` | `FE-003` | Integrar Vue SFC/Volar, componentes, slots e Nuxt; IT/FT | `T-FE-002-02` |
| [ ] | `T-FE-003-04` | `FE-003` | Integrar Tailwind Language Server, completion e preview; IT/UI | `T-FE-001-03` |
| [ ] | `T-FE-004-01` | `FE-004` | Implementar attach Node.js e Chromium com source maps; IT/FT | `T-FE-002-02` |
| [ ] | `T-FE-004-02` | `FE-004` | Integrar console e runners frontend em adaptadores separados; IT/FT | `T-FE-004-01` |
| [ ] | `T-FE-005-01` | `FE-005` | Integrar pré-processadores, variáveis/mixins e browser compatibility; IT | `T-FE-001-03` |
| [ ] | `T-FE-005-02` | `FE-005` | Implementar conversões de cor, refactors CSS e Show Applied Styles; UT/FT | `T-FE-005-01`, `T-PHP-021-01` |

## 9. Estágio 10.0 — Git

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-GIT-001-01` | `GIT-001` | Detectar worktree, branch, detached HEAD e repositório inválido sem bloquear UI; UT/IT | `T-NFR-001-02` |
| [ ] | `T-GIT-001-02` | `GIT-001` | Isolar root/multi-root e refresh de status; IT/FT | `T-GIT-001-01` |
| [ ] | `T-GIT-002-01` | `GIT-002` | Implementar diff working tree/staged por arquivo; IT/UI | `T-GIT-001-01` |
| [ ] | `T-GIT-002-02` | `GIT-002` | Implementar stage/unstage por arquivo e status pós-operação; IT/FT | `T-GIT-002-01` |
| [ ] | `T-GIT-002-03` | `GIT-002` | Implementar commit, mensagem, hooks e preservação do índice em falha; IT/FT | `T-GIT-002-02` |
| [ ] | `T-GIT-002-04` | `GIT-002` | Implementar stage/unstage por chunk/linha; UT/IT/FT | `T-GIT-002-02` |
| [ ] | `T-GIT-002-05` | `GIT-002` | Implementar changelists, Amend e staging area opcional; IT/FT | `T-GIT-002-03` |
| [ ] | `T-GIT-002-06` | `GIT-002` | Implementar before/after commit e commit inline pelo gutter; IT/FT | `T-PHP-040-01`, `T-GIT-002-03`, `T-UI-ED-001-02` |
| [ ] | `T-GIT-003-01` | `GIT-003` | Implementar Log com commits/arquivos e filtros; IT/UI | `T-GIT-001-01` |
| [ ] | `T-GIT-003-02` | `GIT-003` | Implementar Branches popup, compare e incoming/outgoing; IT/FT | `T-GIT-003-01` |
| [ ] | `T-GIT-003-03` | `GIT-003` | Implementar fetch/pull/push com credenciais e output; IT/FT | `T-NFR-004-04`, `T-GIT-003-02` |
| [ ] | `T-GIT-003-04` | `GIT-003` | Implementar confirmação para reescrita e force-with-lease; IT/FT | `T-NFR-008-02`, `T-GIT-003-03` |
| [ ] | `T-GIT-004-01` | `GIT-004` | Gerar line markers do diff e atualizar após save/stage/commit; UT/IT/UI | `T-GIT-002-01`, `T-NFR-002-04` |
| [ ] | `T-GIT-004-02` | `GIT-004` | Implementar provedor de blame por revisão e navegação ao commit; IT | `T-GIT-003-01` |
| [ ] | `T-GIT-005-01` | `GIT-005` | Implementar merge 3-way e resolução de conflitos; IT/FT | `T-GIT-002-01` |
| [ ] | `T-GIT-005-02` | `GIT-005` | Implementar rebase interativo/reword com precondições de dirty tree; IT/FT | `T-GIT-003-04`, `T-GIT-005-01` |
| [ ] | `T-GIT-005-03` | `GIT-005` | Implementar stash, worktrees e submodules com caminho de recuperação; IT/FT/DOC | `T-GIT-003-02` |
| [ ] | `T-GIT-006-01` | `GIT-006` | Integrar GitHub clone/share/fork e Pull Requests com token seguro; IT | `T-NFR-004-04`, `T-GIT-003-03` |
| [ ] | `T-GIT-006-02` | `GIT-006` | Implementar review/timeline/checkout/merge GitHub com privacidade de logs; IT/FT | `T-NFR-005-03`, `T-GIT-006-01` |
| [ ] | `T-GIT-006-03` | `GIT-006` | Integrar GitLab e Merge Requests em adaptador equivalente; IT/FT | `T-GIT-006-02` |

## 10. Estágio 11.x — Database Tools and SQL

### 10.1 Fundação e conexões

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-DB-001-01` | `DB-001` | Criar crate/módulo DB e contratos de conexão, metadados, execução, cancelamento e transações; UT/DOC | `T-NFR-001-02` |
| [ ] | `T-DB-001-02` | `DB-001` | Definir tipos de valores preservando NULL, binário, datas e decimais; CT/UT | `T-DB-001-01` |
| [ ] | `T-DB-001-03` | `DB-001` | Implementar driver fake para contratos da UI e falhas; CT | `T-DB-001-02` |
| [ ] | `T-DB-001-04` | `DB-001` | Implementar SQLite real e banco descartável de testes; IT | `T-DB-001-03` |
| [ ] | `T-DB-001-05` | `DB-001` | Implementar PostgreSQL real em ambiente descartável; IT | `T-DB-001-03` |
| [ ] | `T-DB-001-06` | `DB-001` | Implementar MySQL/MariaDB real em ambiente descartável; IT | `T-DB-001-03` |
| [ ] | `T-DB-002-01` | `DB-002` | Implementar CRUD/duplicar de data sources e escopos project/global; UT/FT | `T-NFR-004-03`, `T-DB-001-01` |
| [ ] | `T-DB-002-02` | `DB-002` | Implementar credenciais por referência segura e ausência de senha em logs; IT/UT | `T-NFR-004-04`, `T-DB-002-01` |
| [ ] | `T-DB-002-03` | `DB-002` | Implementar Test Connection e erros distintos de host/credencial/driver; IT/UI | `T-DB-001-04`, `T-DB-002-02` |
| [ ] | `T-DB-002-04` | `DB-002` | Implementar URL-only, cores, pastas, SSL/TLS e seleção de schemas; UT/IT | `T-DB-002-03` |
| [ ] | `T-DB-002-05` | `DB-002` | Implementar SSH e encerramento de sessões ao excluir/trocar conexão; IT/FT | `T-DB-002-04`, `T-NFR-001-04` |

### 10.2 Explorer, console e grade read-only

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-DB-003-01` | `DB-003` | Implementar introspection de schemas/tabelas/views/colunas/chaves/índices; IT | `T-DB-001-04`, `T-DB-002-03` |
| [ ] | `T-DB-003-02` | `DB-003` | Implementar explorer assíncrono, refresh por subtree e filtros; UT/UI/FT | `T-DB-003-01` |
| [ ] | `T-DB-003-03` | `DB-003` | Tratar erro parcial sem apagar metadados válidos e invalidar após DDL; IT/FT | `T-DB-003-02` |
| [ ] | `T-DB-003-04` | `DB-003` | Implementar Quick Documentation/DDL por driver; IT/UI | `T-DB-003-01` |
| [ ] | `T-DB-004-01` | `DB-004` | Implementar consoles/sessões por data source, default schema e dialeto; UT/IT | `T-DB-002-03` |
| [ ] | `T-DB-004-02` | `DB-004` | Implementar parser de statement/seleção e destaque do que será executado; UT/UI | `T-DB-004-01` |
| [ ] | `T-DB-004-03` | `DB-004` | Executar statement/seleção com Ctrl+Enter, output, duração, erros e linhas afetadas; IT/FT | `T-DB-004-02`, `T-NFR-008-02` |
| [ ] | `T-DB-004-04` | `DB-004` | Implementar cancelamento e isolamento de resultados concorrentes; IT/PERF | `T-NFR-001-03`, `T-DB-004-03` |
| [ ] | `T-DB-004-05` | `DB-004` | Implementar Query History com redaction e limite/retenção; UT/IT | `T-NFR-005-03`, `T-DB-004-03` |
| [ ] | `T-DB-004-06` | `DB-004` | Encerrar sessão ao fechar console/projeto e cobrir consulta longa; IT/FT | `T-NFR-001-04`, `T-DB-004-04` |
| [ ] | `T-DB-005-01` | `DB-005` | Implementar grade paginada sem carga integral e com NULL distinguível; UT/IT/UI | `T-DB-004-03` |
| [ ] | `T-DB-005-02` | `DB-005` | Implementar Table/Text/Record View e Value Editor para LOB/binário; IT/UI | `T-DB-005-01` |
| [ ] | `T-DB-005-03` | `DB-005` | Implementar ordenação/filtros server-side com consulta visível; IT/FT | `T-DB-005-01` |
| [ ] | `T-DB-005-04` | `DB-005` | Implementar exportação de resultado com encoding/delimitadores/NULL; UT/IT | `T-DB-005-01` |
| [ ] | `T-DB-005-05` | `DB-005` | Validar PostgreSQL/MySQL além de SQLite e medir resultset grande; IT/PERF | `T-DB-001-05`, `T-DB-001-06`, `T-DB-005-03`, `T-NFR-006-01` |

### 10.3 Edição, transações e administração

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-DB-006-01` | `DB-006` | Implementar modelo de mudanças pendentes por célula/linha e Revert; UT/UI | `T-DB-005-01` |
| [ ] | `T-DB-006-02` | `DB-006` | Implementar Add/Delete/Clone, NULL/DEFAULT/UUID e LOB; IT/FT | `T-DB-006-01` |
| [ ] | `T-DB-006-03` | `DB-006` | Gerar Preview DML parametrizado com estratégia de chave explícita; UT/IT | `T-DB-006-02` |
| [ ] | `T-DB-006-04` | `DB-006` | Implementar transações Auto/Manual, Commit/Rollback e isolamento; IT/FT | `T-DB-001-04`, `T-DB-006-03` |
| [ ] | `T-DB-006-05` | `DB-006` | Implementar Submit parcial, erros por registro e conflito de concorrência; IT/FT | `T-NFR-007-03`, `T-DB-006-04` |
| [ ] | `T-DB-007-01` | `DB-007` | Implementar extractors TXT/CSV/JSON/XML/Markdown/Excel/SQL INSERT; UT/IT | `T-DB-005-04` |
| [ ] | `T-DB-007-02` | `DB-007` | Implementar import CSV com mapping, tipos, DDL e preview transacional; IT/FT | `T-DB-006-04` |
| [ ] | `T-DB-007-03` | `DB-007` | Implementar Run SQL Script com política de transação/continuação e relatório; IT/FT | `T-NFR-008-02`, `T-DB-006-04` |
| [ ] | `T-DB-007-04` | `DB-007` | Integrar mysqldump/pg_dump com compatibilidade e credenciais seguras; IT/DOC | `T-NFR-003-02`, `T-DB-002-02` |
| [ ] | `T-DB-008-01` | `DB-008` | Implementar diagramas e exportações por formato em subtarefas; UT/UI/IT | `T-DB-003-01` |
| [ ] | `T-DB-008-02` | `DB-008` | Implementar compare de estruturas/dados com diff e escopo; IT/FT | `T-DB-003-01`, `T-DB-005-01` |
| [ ] | `T-DB-008-03` | `DB-008` | Implementar execution plans por dialeto/versão; IT/UI | `T-DB-004-03` |
| [ ] | `T-DB-009-01` | `DB-009` | Criar checklist de capabilities e fixture para cada novo SGBD; DOC/CT | `T-DB-001-01` |
| [ ] | `T-DB-009-02` | `DB-009` | Adicionar drivers relacionais adicionais individualmente com teste real; IT/DOC | `T-DB-009-01` |
| [ ] | `T-DB-009-03` | `DB-009` | Projetar adaptadores NoSQL sem forçar contrato relacional; CT/IT/DOC | `T-DB-009-01` |

## 11. Estágio 12.x — Ferramentas integradas

### 11.1 Terminal e Local History

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-TOOL-TERM-001-01` | `TOOL-TERM-001` | Consolidar shell configurável, cwd, env e output incremental; IT/FT | `T-NFR-003-02` |
| [ ] | `T-TOOL-TERM-001-02` | `TOOL-TERM-001` | Isolar Terminal interativo dos launchers internos da IDE; IT/DOC | `T-TOOL-TERM-001-01` |
| [ ] | `T-TOOL-TERM-002-01` | `TOOL-TERM-002` | Implementar abas/splits e ciclo de vida de sessões; IT/UI/FT | `T-TOOL-TERM-001-01` |
| [ ] | `T-TOOL-TERM-002-02` | `TOOL-TERM-002` | Implementar histórico, busca, links e drag-and-drop sem reexecutar comandos na restauração; UT/IT/FT | `T-TOOL-TERM-002-01` |
| [ ] | `T-TOOL-TERM-002-03` | `TOOL-TERM-002` | Implementar SSH e command completion por shell suportado; IT/FT | `T-NFR-004-04`, `T-TOOL-TERM-002-01` |
| [ ] | `T-TOOL-HIST-001-01` | `TOOL-HIST-001` | Implementar storage versionado de revisões por save/evento; UT/IT | `T-NFR-010-01` |
| [ ] | `T-TOOL-HIST-001-02` | `TOOL-HIST-001` | Implementar Show History, diff, labels e revert total/parcial; UT/FT | `T-TOOL-HIST-001-01`, `T-PHP-021-01` |
| [ ] | `T-TOOL-HIST-001-03` | `TOOL-HIST-001` | Implementar restauração de deletados e Create Patch; IT/FT | `T-TOOL-HIST-001-02` |
| [ ] | `T-TOOL-HIST-001-04` | `TOOL-HIST-001` | Implementar retenção, exclusões de segredos/arquivos grandes e limpeza; UT/IT/DOC | `T-NFR-008-03`, `T-TOOL-HIST-001-01` |

### 11.2 Deployment e Remote Host

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-TOOL-DEP-001-01` | `TOOL-DEP-001` | Implementar modelo de servidores/mappings/exclusions e credenciais seguras; UT/IT | `T-NFR-004-04` |
| [ ] | `T-TOOL-DEP-001-02` | `TOOL-DEP-001` | Implementar SFTP e local primeiro, com Remote Host e compare; IT/FT | `T-TOOL-DEP-001-01` |
| [ ] | `T-TOOL-DEP-001-03` | `TOOL-DEP-001` | Implementar upload/download/sync com diff e confirmação destrutiva; IT/FT | `T-NFR-008-02`, `T-TOOL-DEP-001-02` |
| [ ] | `T-TOOL-DEP-001-04` | `TOOL-DEP-001` | Adicionar FTP/FTPS/WebDAV e server groups em adaptadores separados; IT | `T-TOOL-DEP-001-02` |
| [ ] | `T-TOOL-DEP-002-01` | `TOOL-DEP-002` | Implementar fila visível de auto-upload on save/commit, desativada por default; IT/FT | `T-TOOL-DEP-001-03`, `T-GIT-002-03` |
| [ ] | `T-TOOL-DEP-002-02` | `TOOL-DEP-002` | Tratar retry/falha sem marcar sincronização como concluída; IT/UT | `T-TOOL-DEP-002-01` |

### 11.3 Docker, WSL e Dev Containers

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-TOOL-CONT-001-01` | `TOOL-CONT-001` | Implementar conexões Docker TCP/Unix/SSH/contexts e descoberta de versão; IT | `T-NFR-003-01` |
| [ ] | `T-TOOL-CONT-001-02` | `TOOL-CONT-001` | Implementar Services para images/containers/Compose; IT/UI | `T-TOOL-CONT-001-01` |
| [ ] | `T-TOOL-CONT-001-03` | `TOOL-CONT-001` | Implementar run configurations e interpretadores PHP/Node com mappings/env; IT/FT | `T-RUN-001-02`, `T-TOOL-CONT-001-02` |
| [ ] | `T-TOOL-CONT-002-01` | `TOOL-CONT-002` | Implementar descoberta WSL e conversão de caminhos; UT/IT | `T-NFR-003-01` |
| [ ] | `T-TOOL-CONT-002-02` | `TOOL-CONT-002` | Integrar projetos, PHP e Git do WSL no ambiente correto; IT/FT | `T-GIT-001-01`, `T-TOOL-CONT-002-01` |
| [ ] | `T-TOOL-CONT-003-01` | `TOOL-CONT-003` | Ler/validar devcontainer.json e pré-requisitos Docker/Buildx; UT/IT | `T-TOOL-CONT-001-01` |
| [ ] | `T-TOOL-CONT-003-02` | `TOOL-CONT-003` | Implementar create/rebuild com logs e proteção de volumes; IT/FT | `T-NFR-008-02`, `T-TOOL-CONT-003-01` |

## 12. Estágio 13.x — Frameworks PHP

Cada framework deve ter fixture mínimo, detecção neutra fora do framework, versões suportadas e testes próprios.

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-FW-SYMFONY-001-01` | `FW-SYMFONY-001` | Criar fixture/detecção Symfony e índice de services/routes; IT/DOC | `T-PHP-030-02` |
| [ ] | `T-FW-SYMFONY-001-02` | `FW-SYMFONY-001` | Implementar navegação/completion, gutter, Symfony Symbol e integrations Twig/Console; IT/FT | `T-FW-SYMFONY-001-01`, `T-PHP-004-03` |
| [ ] | `T-FW-SYMFONY-001-03` | `FW-SYMFONY-001` | Integrar forms/events/Doctrine/translations e profiler em subtarefas; IT/UI | `T-FW-SYMFONY-001-01` |
| [ ] | `T-FW-LARAVEL-001-01` | `FW-LARAVEL-001` | Criar fixture/detecção Laravel e índice de routes/Eloquent; IT/DOC | `T-PHP-030-02` |
| [ ] | `T-FW-LARAVEL-001-02` | `FW-LARAVEL-001` | Implementar request/validation/gates, Blade e navegação de templates; IT/FT | `T-FW-LARAVEL-001-01`, `T-PHP-004-03` |
| [ ] | `T-FW-LARAVEL-001-03` | `FW-LARAVEL-001` | Integrar Artisan, Pint, Pest e debug de Blade com mapa de cache; IT/FT | `T-PHP-040-05`, `T-RUN-TST-002-02`, `T-RUN-DBG-004-01`, `T-FW-LARAVEL-001-02` |
| [ ] | `T-FW-WORDPRESS-001-01` | `FW-WORDPRESS-001` | Criar fixture/detecção WordPress e índice de hooks/callbacks; IT/DOC | `T-PHP-030-02` |
| [ ] | `T-FW-WORDPRESS-001-02` | `FW-WORDPRESS-001` | Implementar navegação/completion, documentação, PHPCS e WP-CLI; IT/FT | `T-PHP-040-06`, `T-FW-WORDPRESS-001-01` |
| [ ] | `T-FW-DRUPAL-001-01` | `FW-DRUPAL-001` | Criar fixtures por versão suportada, file associations e include paths; IT/DOC | `T-PHP-030-02` |
| [ ] | `T-FW-DRUPAL-001-02` | `FW-DRUPAL-001` | Implementar hooks, Coder, Drush e Symfony Bridge; IT/FT | `T-PHP-040-06`, `T-FW-DRUPAL-001-01` |
| [ ] | `T-FW-JOOMLA-001-01` | `FW-JOOMLA-001` | Criar detecção Joomla, DocBlocks e code style; IT/DOC | `T-PHP-030-02` |
| [ ] | `T-FW-JOOMLA-001-02` | `FW-JOOMLA-001` | Implementar importação explícita de data source sem vazamento de credenciais; IT/FT | `T-DB-002-02`, `T-FW-JOOMLA-001-01` |

## 13. IA e recursos avançados

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-AI-001-01` | `AI-001` | Especificar runtime/modelo local, requisitos de CPU e política de dados; DOC | BASE |
| [ ] | `T-AI-001-02` | `AI-001` | Implementar Full Line Completion assíncrona, atalhos e cancelamento; UT/IT/PERF | `T-NFR-001-03`, `T-AI-001-01` |
| [ ] | `T-AI-001-03` | `AI-001` | Validar separação de LSP, ausência de envio de código e settings; IT/FT/DOC | `T-PHP-003-02`, `T-AI-001-02` |
| [ ] | `T-AI-002-01` | `AI-002` | Criar política de contexto, redaction e consentimento para provedores locais/cloud/BYOK; DOC/UT | `T-NFR-008-03` |
| [ ] | `T-AI-002-02` | `AI-002` | Implementar chat contextual com indicação de dados compartilhados; IT/FT | `T-AI-002-01` |
| [ ] | `T-AI-002-03` | `AI-002` | Integrar agentes com permissões mínimas e trilha de auditoria; IT/FT | `T-AI-002-01` |
| [ ] | `T-AI-003-01` | `AI-003` | Implementar Agent Sessions com worktree temporário e revisão de diff; IT/FT | `T-GIT-005-03`, `T-AI-002-03` |
| [ ] | `T-AI-003-02` | `AI-003` | Expor MCP read-only inicialmente e permissões explícitas para escrita/execução; CT/IT/DOC | `T-NFR-008-01` |
| [ ] | `T-AI-004-01` | `AI-004` | Separar engine de inspeções da UI e executar projeto headless; IT | `T-PHP-010-04` |
| [ ] | `T-AI-004-02` | `AI-004` | Criar adaptador CI/Qodana com saída reproduzível; IT/DOC | `T-AI-004-01` |

## 14. Gates de release e fechamento dos marcos

Estas tarefas não substituem os aceites individuais; verificam a integração do fluxo.

| Estado | Tarefa | Requisito | Entrega e evidência | Depende de |
|---|---|---|---|---|
| [ ] | `T-RELEASE-62-01` | `NFR-006` | Release candidate 6.2: build limpo, testes, smoke UI, benchmark e pendências P1/P2 registrados; UI/PERF/DOC | `T-UI-ED-GATE-02` |
| [ ] | `T-RELEASE-70-01` | `PHP-001` | Fluxo PHP P0 em projeto Composer limpo: abrir, editar, diagnosticar, completar, navegar e salvar; IT/FT/DOC | `T-PHP-001-05`, `T-PHP-002-03`, `T-PHP-003-03`, `T-PHP-004-02`, `T-PHP-030-03` |
| [ ] | `T-RELEASE-80-01` | `RUN-DBG-001` | Fluxo Run/Debug P0 com PHP real, breakpoints, stepping, variáveis e Stop/Detach; IT/FT/DOC | `T-RUN-DBG-001-04`, `T-RUN-DBG-002-01`, `T-RUN-002-01` |
| [ ] | `T-RELEASE-90-01` | `FE-001` | Fluxo frontend essencial em fixture Vite com node_modules; IT/FT/PERF/DOC | `T-FE-001-05`, `T-FE-002-02` |
| [ ] | `T-RELEASE-100-01` | `GIT-002` | Fluxo Git P0 em repositório temporário: diff, stage seletivo, commit e falha de hook; IT/FT/DOC | `T-GIT-002-03` |
| [ ] | `T-RELEASE-110-01` | `DB-004` | Fluxo DB read-only em SQLite/PostgreSQL/MySQL: conectar, explorar, consultar, cancelar e paginar; IT/FT/PERF/DOC | `T-DB-004-06`, `T-DB-005-05` |
| [ ] | `T-RELEASE-111-01` | `DB-006` | Fluxo DB editável: preview DML, Submit, erro parcial, Commit e Rollback; IT/FT/DOC | `T-DB-006-05` |
| [ ] | `T-RELEASE-FINAL-01` | `NFR-007` | Executar testes de recuperação, segurança, acessibilidade e upgrade antes de release; IT/FT/DOC | `T-NFR-007-04`, `T-NFR-008-03`, `T-NFR-009-03`, `T-NFR-010-03` |
| [ ] | `T-RELEASE-FINAL-02` | `NFR-008` | Revisar licenças, ausência de segredos e instalação em ambiente limpo; IT/DOC | `T-NFR-008-04`, `T-RELEASE-FINAL-01` |
| [ ] | `T-RELEASE-FINAL-03` | `NFR-010` | Publicar notas por requisito, limitações conhecidas e migração; DOC | `T-RELEASE-FINAL-02` |

## 15. Rotina de manutenção do plano

Ao concluir qualquer tarefa:

1. marcar a caixa somente após o aceite;
2. registrar commit e evidência na entrega ou PR;
3. atualizar o status do requisito correspondente em `REQUISITOS.md`;
4. abrir tarefas derivadas para limitações reais, sem escondê-las na descrição da concluída;
5. verificar se alguma dependência foi desbloqueada;
6. manter o roadmap e o README coerentes com o comportamento entregue.

Ao alterar um requisito:

- revisar todas as tarefas que o referenciam;
- preservar IDs históricos e criar subtarefas para o novo escopo;
- atualizar dependências e gates afetados;
- não apagar tarefas concluídas: registrar substituição quando necessário.

## 16. Checklist para escolher a próxima tarefa

- [ ] O requisito e o aceite estão claros.
- [ ] As dependências estão concluídas ou existe decisão explícita para contorná-las.
- [ ] Nenhum outro trabalho ativo altera os mesmos contratos sem coordenação.
- [ ] A entrega cabe em um incremento revisável.
- [ ] Os testes e a evidência já estão definidos.
- [ ] O que não faz parte está registrado.
- [ ] Existe ambiente para executar o fluxo real, não apenas compilar.

A regra de execução permanece: **uma tarefa, um fluxo verificável, uma revisão e um registro de evidência**.
