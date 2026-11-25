# Third-party components shipped with PHP IDE

## PHPantom

The PHP language server bundled with the IDE, installed as
`<prefix>/lib/phpide/phpantom_lsp`. It runs as a separate process and talks
to the IDE over the Language Server Protocol.

- Project: https://github.com/PHPantom-dev/phpantom_lsp
- License: MIT (`phpantom/LICENSE`)
- Version: pinned in `third_party/phpantom.lock`; the release archive is
  checked against the SHA-256 recorded there.

## phpstorm-stubs

Built into the PHPantom binary: the signatures of PHP's built-in functions,
classes and extensions.

- Project: https://github.com/JetBrains/phpstorm-stubs
- License: Apache License 2.0 (`phpstorm-stubs/LICENSE`)
- Copyright JetBrains s.r.o. and contributors.
- Revision: the commit pinned in PHPantom's `stubs.lock` for the release
  above (`2049ca3cfa1feabf52db074108fe776a5713cf02` for PHPantom 1.1.0).
