# T-NFR-006-02 — Baseline de desempenho (digitação, scroll, busca, abertura)

> Evidência: PERF (este documento + benches reproduzíveis no repo).
> Requisito: `NFR-006` (§15). Medir antes de otimizar; registrar hardware,
> massa e limiar.

## Ambiente de medição

| Item | Valor |
|---|---|
| Data | 2026-11-03 |
| Hardware | 12th Gen Intel(R) Core(TM) i5-12450H, 12 threads |
| SO | Linux x86_64 |
| Perfil | `cargo bench` (release + debuginfo) |

## Como reproduzir

```bash
cargo bench -p phpide-core --bench doc_ops
cargo bench -p phpide-ui  --bench search_large
```

Benches de harness próprio (sem deps externas): mediana de N execuções
após aquecimento. Massa: fixture de 10 mil linhas (`fixtures::large_php_document`)
e projeto de 300 arquivos PHP × 120 linhas (`fixtures::write_large_project`).

## Resultados (mediana)

| Operação | Custo | Observação |
|---|---|---|
| Digitação no fim do doc 10k (50 inserts) | 161.5 µs | ≈ **3.2 µs/tecla** — meta "sem travamento perceptível" atendida com 3 ordens de margem a 60fps (16.6ms/frame) |
| Digitação no meio do doc 10k (50 inserts) | 160.5 µs | Idem (rope: edição O(log n), sem cópia do texto) |
| line_col em 100 offsets aleatórios | 38.7 µs | ≈ 0.39 µs/conversão (caret/status bar) |
| Roundtrip UTF-16 (LSP) em 100 offsets | 82.9 µs | ≈ 0.83 µs/ida-e-volta; ponto de atenção se usado por frame em massa |
| Abertura de buffer 10k linhas | 135.6 µs | leitura + construção da rope |
| Save atômico 10k linhas (tmp+fsync+rename) | 5.8 ms | **dominado pelo fsync** do arquivo; esporádico (Ctrl+S), aceitável — sem escrita parcial (NFR-007-01) |
| Busca textual em 300 arquivos (~36k linhas) | 10.6 ms | worker do JobSystem; UI livre durante a busca |

## Scroll e interpolação de viewport

Scroll em si é custo de **renderização** do `TextEdit` (egui) sobre a rope;
não há benchmark headless fiel do pipeline de pixels sem GPU. O que o
núcleo garante (e que o bench cobre indiretamente): conversões de posição
usadas pelo scroll (line_col/UTF-16) estão na casa de sub-microsegundo por
consulta, e o texto nunca é copiado inteiro por frame.

**Medição de scroll e digitação na UI real (frame time sob X11)** fica
registrada como pendência: requer captura interativa ou harness gráfico
(issue derivada — não é fingida aqui).

## Leitura e gargalos

1. Digitação e abertura: **sem ação**; margem grande.
2. Save: fsync é o custo dominante e é o preço da durabilidade
   (NFR-007-01). Se o benchmark de fluxo (T-UI-ED-000-04) mostrar regressão,
   avaliar fsync assíncrono ou janela de coalescência — **não** antes.
3. UTF-16: aceitável para diagnósticos/completion por demanda; se inlay
   hints por frame entrar (T-UI-ED-005), reavaliar com índice incrementado.

## Limiar de regressão proposto (para T-NFR-006-03)

- Digitação: piorar > 2× sobre o baseline dispara investigação.
- Busca em 300 arquivos: piorar > 3× (disco variável) dispara investigação.
- Save: piorar > 2× dispara investigação (fsync em hardware diferente
  deve ser medido e anotado, não "corrigido").
