# Baseline de clippy (2026-11-03)

> Regra (§5.3 de `REQUISITOS.md`): clippy introduzido progressivamente;
> este arquivo registra o baseline e **impede warnings novos** nos diffs
> seguintes. Comando do gate:

```bash
cargo clippy --workspace --all-targets
```

## Ocorrências conhecidas (baseline — corrigir ao tocar o arquivo)

| Lint | Local | Nota |
|---|---|---|
| `ptr_arg` (`&PathBuf` → `&Path`) | `phpide-ui/src/welcome.rs:383` (`create_project`) | pré-existente à consolidação NFR |
| `ptr_arg` | `phpide-ui/src/completion.rs:46` | idem |
| `ptr_arg` | `phpide-ui/src/exec.rs:192` (`composer_scripts`) | idem |
| `collapsible_if` ×2 | `phpide-ui/src/main.rs:941,946` (Alt+←/→) | idem |
| `derivable_impls` | `phpide-lsp/src/lib.rs:126` (`Default for LspStatus`) | idem |
| `needless_lifetimes` | `phpide-php/src/lib.rs:57` | idem |
| `needless_borrows_for_generic_args` | `phpide-ui/src/tool_panels.rs:173` | idem |
| `unnecessary_sort_by` | `phpide-ui/src/palette.rs:214` | idem |

A consolidação NFR (T-NFR-001/002/003/004/005/007, 2026-11-03) entrou com
zero warnings novos; o que aparecer além desta lista deve ser tratado antes
do merge.
