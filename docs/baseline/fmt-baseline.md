# Baseline de rustfmt (2026-11-03)

> Regra (§5.3 de `REQUISITOS.md`): gate `cargo fmt --all -- --check`. O
> código legado (Estágios 0–6.1) **não estava fmt-clean** quando a
> consolidação NFR começou. Para não formatar em massa arquivos fora do
> escopo (regra §4.4), os arquivos abaixo mantêm seu estilo original até
> serem tocados por uma tarefa relacionada; a partir daí entram no padrão.

## Arquivos divergentes conhecidos (baseline)

- `crates/phpide-dbgp/src/lib.rs`
- `crates/phpide-dbgp/tests/fake_xdebug.rs`
- `crates/phpide-lsp/src/lib.rs`
- `crates/phpide-lsp/tests/integration_lsp.rs`
- `crates/phpide-php/src/lib.rs`
- `crates/phpide-syntax/src/lib.rs`
- `crates/phpide-ui/src/plugins.rs`
- `crates/phpide-ui/src/vcs.rs`

## Garantia da consolidação NFR

Todo arquivo **criado ou substancialmente alterado** pela consolidação
NFR (2026-11-03) está no padrão rustfmt:

- `phpide-core`: `offsets.rs`, `revision.rs`, `error.rs`, `redact.rs`,
  `log.rs`, `jobs.rs`, `settings.rs`, `fsutil.rs`, `fixtures.rs`,
  `lib.rs` (trechos alterados), `benches/*`;
- `phpide-exec`: crate inteiro (novo);
- `phpide-ui`: `main.rs` (trechos alterados), `search.rs`, `exec.rs`,
  `settings.rs`, `tabs.rs`, `welcome.rs` (trechos alterados),
  `benches/*`.

Conferência pontual do escopo:

```bash
cargo fmt --all -- --check 2>&1 | grep '^Diff in' | sort -u
# deve listar apenas os arquivos do baseline acima
```
