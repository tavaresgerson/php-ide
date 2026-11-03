# T-NFR-006-01 — Fixtures de massa de dados

> Evidência: DOC (este arquivo) + UT dos geradores. Requisito: `NFR-006`
> (§15): "arquivos grandes, projetos grandes, cancelamento, memória".
> Meta inicial do produto: arquivo de **10 mil linhas** navegável.

## Onde vivem

Os geradores são código (`phpide_core::fixtures`, sempre compilado,
`#[doc(hidden)]`) e não arquivos versionados — a massa é produzida de forma
**determinística** (sem timestamps nem aleatoriedade) no ponto de uso
(teste/benchmark), garantindo reproduzibilidade.

| Gerador | Massa produzida | Uso |
|---|---|---|
| `large_php_document(lines)` | Arquivo PHP sintaticamente plausível: `<?php` + `declare` + namespace + classe com N métodos e corpo | Arquivo grande (≥ 10 mil linhas) para digitação, scroll, abertura, save |
| `write_large_project(dir, files, lines_per_file)` | Projeto Composer com M arquivos `.php` em 16 subdiretórios + `composer.json` | Busca/indexação em projeto grande |
| `large_resultset_csv(rows)` | CSV com header + N linhas, NULLs (campo vazio), acentos e strings longas | Preparação do RF-DB (resultset grande paginado, DB-005) |

Testes de unidade (`phpide-core::fixtures::tests`) fixam o contrato: tamanho
mínimo, determinismo e forma do CSV.

## Como regenerar manualmente (inspeção visual/debug)

```bash
# Arquivo de 10 mil linhas em /tmp/big10k.php
cat > /tmp/gen.rs <<'EOF'
fn main() {
    print!("{}", phpide_core::fixtures::large_php_document(10_000));
}
EOF
# (rodar como exemplo dentro do crate phpide-core, ou:)
cargo test -p phpide-core fixtures -- --nocapture
```

Para o benchmark de busca, o próprio bench gera o projeto sob
`$TMPDIR/phpide-bench-proj-<pid>/` e o remove ao final
(`crates/phpide-ui/benches/search_large.rs`).

## Política

- Nada de massa binária/MB versionada no repositório: tudo é gerado.
- Fixtures com segredos: **nenhum** — dados sintéticos marcados com domínio
  `*.invalid` (NFR-008-03 quando o RF correspondente entrar).
- Resultset grande fica em CSV de texto para não prender o formato a um
  driver; o RF-DB define o contrato real de valores (DB-001-02).

## Consumidores atuais

- `phpide-core::fixtures::tests` (UT de contrato).
- `crates/phpide-core/benches/doc_ops.rs` (T-NFR-006-02).
- `crates/phpide-ui/benches/search_large.rs` (T-NFR-006-02).
