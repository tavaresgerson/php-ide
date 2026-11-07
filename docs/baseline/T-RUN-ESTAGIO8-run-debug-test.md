# Baseline — Estágio 8: Execução, depuração, testes e cobertura

> Evidências do conjunto `T-RUN-*` (RUN-001/002, RUN-DBG-001/002/003,
> RUN-TST-001/002/003) e do gate de integração. Data: 2026-11-03.
> Atualização (mesmo dia): gate de integração executado contra o **Xdebug 3
> real** (extensão carregada localmente, sem root) — bugs protocolares
> reais achados e corrigidos, limitações de ambiente removidas.

## Ambiente de validação

- PHP 8.2.30 (CLI real, `/usr/bin/php`).
- PHPUnit 11.5.57 (phar real, `target/tmp/phpunit.phar`, espelho de
  `https://phar.phpunit.de/phpunit-11.5.phar`).
- **Xdebug 3.2.0 real, carregado sem root**: `.deb` do Debian bookworm
  (`php8.2-xdebug`, mesma ABI `20220829` NTS do PHP 8.2 local) baixado com
  `apt-get download` e extraído com `dpkg-deb -x` em
  `target/tmp/xdebug-root` (gitignored). Uso:
  `php -d zend_extension=…/xdebug.so -d xdebug.mode=debug|coverage …`.
  Override do caminho por `PHPIDE_XDEBUG_SO`. Não há `pecl`/`phpize` nem
  docker no host — este atalho de pacote resolve a validação sem instalar
  nada no sistema.

## Bugs protocolares achados só com a extensão real

O peer protocolar fiel (ambiente sem Xdebug) fala o DBGp corretamente, mas
não reproduz três detalhes do Xdebug 3.2.0 — todos achados ao rodar o gate
com a extensão real e corrigidos em `phpide-dbgp`:

1. **Pacotes de comprimento em TODA mensagem do engine** — cada mensagem do
   Xdebug vem precedida de um pacote só com o comprimento em dígitos
   (`"473\0"` antes do XML), no `<init>` e em **toda** resposta a comando
   (`"204\0<?xml…"`). O listener descartava a conexão no primeiro pacote e
   as respostas eram lidas como `"204"` → parse inválido. Correção:
   `proto::read_message` pula pacotes só-dígitos (usado no handshake do
   listener, no `send_xml` e no `listen` legado). Regressão coberta em UT
   (`read_message_skips_xdebug3_length_headers`, init prefixado no IT do
   listener).
2. **`success` ausente significa sucesso** — o atributo foi depreciado no
   DBGp 1.0 e o Xdebug 3 não o envia (erros vêm como `<error>` — atenção:
   com atributo `code`, não `id` — ou `success="0"`). `parse_response` trata
   ausência como sucesso e `<error>` como falha; sem isso, todo
   breakpoint/eval/watch era marcado Inválido com o engine real.
3. **`eval` exige dados posicionais após `--`** — a forma é
   `eval -i N -- {base64}` (spec DBGp §8.3); o `-e` que usávamos é
   rejeitado pelo Xdebug com "invalid or missing options" (error 3).
   Correção: `DbgpSession::send_xml_data` para comandos com bloco de
   dados; `session::evaluate` (expressões que não começam com `$`) migrou
   para ela. Variáveis (`$a`, `$a->b`) continuam por `property_get -d`,
   que endereça o frame corretamente.
4. **Valores `encoding="base64"`** — o Xdebug 3 marca strings com
   `encoding="base64"` e manda o CDATA em base64; `parse_context` decodifica
   (decode RFC 4648 tolerante a padding em `session::base64_decode`).
   O lookup de `encoding` olha só a tag própria (atributo de `<property>`
   irmão não contamina o valor).
5. **Closer cego quebrava o `stop` protocolar** — `DebugEngine::send()`
   desligava o socket para Detach/Stop/Shutdown **em qualquer fase**; com a
   sessão parada no breakpoint, o shutdown imediato matava o socket antes
   de `session.stop()` escrever o comando → EPIPE, e fechar o socket tem
   semântica de *detach* (o script segue rodando), não de *stop* (o Xdebug
   mata o script só ao receber o comando). Os peers fake não distinguiram
   (esperavam só o fechamento do socket) — o engine real revelou. Correção:
   máquina de fases (`SessionState { stream, phase }`,
   `Accepting/Stopped/Continuing`); o shutdown externo só é aplicado na
   fase `Continuing` (leitura de `run`/`step` bloqueada). Janela residual
   documentada: Detach/Stop no instante exato em que o `run` desbloqueou
   mas a fase ainda não virou degrada para `ConnectionLost` em vez de
   Detached/Stopped (o gerenciador de execução mata o processo no Stop
   de qualquer forma).

## Arquitetura entregue

| Crate | Incremento |
|---|---|
| `phpide-run` (novo) | `config` (modelo versionado + validação), `session` (estado/cwd/env/output/Pause Output), `manager` (process list, Rerun/Stop/Pin, redaction no log), `php_script` (build validado), `phpunit` (resolução do runner, consolidação config/bootstrap, escopos, parser TeamCity → árvore), `coverage` (modelo arquivo/linha + importador Clover) |
| `phpide-exec` | `spawn_stream` — stdout/stderr linha a linha por canal com Stop de grupo; o evento `Exit` só é sinalizado após o join das pumps de output (corrida real de ordenação achada pelo gate) |
| `phpide-dbgp` | `proto` (`read_message` com skip de pacotes de comprimento, `send_xml_data` para `eval`, `parse_response` com semântica DBGp 1.0), `breakpoint` (modelo + estados Verified/Invalid/Disabled/Pending + sync), `mapping` (path mappings local↔remoto, associação da conexão ao projeto), `listener` (porta 9003, IDE key, `xdebug_env` que mescla sem apagar env do usuário), `session` (run/stepping, frames, variáveis com decode base64, evaluate/watches por frame via `property_get -d` / `eval --`), `engine` (máquina de estados em thread com **fases** Accepting/Stopped/Continuing; Detach/Stop imediatos via shutdown cirúrgico só durante `run`/`step`) |
| `phpide-ui` | painel Run (process list + ações + output + árvore de testes + cobertura), painel Debug (toolbar F7/F8/F9/Shift+F8/Alt+F10, frames, variáveis, watches, evaluate, breakpoints com estados), gutter com faixa de cobertura, atalhos contextuais de debug |

## Evidências de teste

| Camada | Onde | O que exercita |
|---|---|---|
| UT | `phpide-run` (32) | modelo versionado (roundtrip, versão desconhecida → erro recuperável), validação de campos por tipo, sessões (estado, pause output, stop, rerun, allow-multiple), redaction de env no log, parser TeamCity (suítes, data sets, duração, falha com `arquivo:linha`, escapes), escopos → filtros (âncoras verificadas com PHPUnit real), consolidação config/bootstrap, Clover (linhas executáveis vs cobertas, agregação) |
| UT | `phpide-dbgp` (31) | encode/parse DBGp, `read_message` pulando pacotes de comprimento, `parse_response` (success ausente = ok; `<error>` = falha), `fileuri`↔path com espaços/Unicode, base64 encode/decode, valores `encoding="base64"`, parse de stack/context, estados de breakpoint, sync com respostas sem `success` |
| IT contrato | `phpide-dbgp` (7 fake) | sessão completa contra peer fiel (sync → run → parada com frames+variáveis → step_over → resume → fim), Detach/Stop durante run bloqueado (socket fecha imediatamente, porta liberada), evaluate/watches por frame, estados Invalid vindo do backend, listener com filtro de IDE key (recusa com detach), **init prefixado por comprimento como o Xdebug 3 manda** |
| IT ferramenta real | `phpide-run/tests/run_php_real.rs` (6) | PHP real: cwd/env/args/stderr, Stop mata processo, Rerun, InvalidConfig acionável, segredos fora do log, múltiplas instâncias |
| IT ferramenta real | `phpide-run/tests/phpunit_real.rs` (4) | PHPUnit 11 real no fixture `tests/fixtures/phpunit-sample`: árvore completa (7 testes, 1 falha em `CalculadoraTest.php:41`, 1 skip, data sets nomeados), escopos classe/método/data set/diretório executam só o casado, consolidação phpunit.xml + bootstrap, **cobertura real** (PHPUnit + Xdebug modo coverage geram o Clover; importador disponibiliza por arquivo/linha — linha 9 de `Calculadora.php` coberta) |
| **Gate Xdebug real** | `gate_run_debug.rs` (3, sufixo `_xdebug_real`) | **Extensão Xdebug 3.2.0 de verdade** (sem root, `.so` do `.deb` Debian): sessão completa (init real com pacote de comprimento, breakpoint_set aceito sem `success`, parada com frames/variáveis, step_over, evaluate `$a` via property_get, eval `40+2` via `-- base64`, watches, resume, script exit 0 com output correto); **Detach real** (comando `detach` protocolar chega, PHP completa com exit 0); **Stop real** (comando `stop` protocolar chega — socket fechado só após o envio, fase Stopped —, o Xdebug mata o script, processo morre sem kill externo, porta liberada). Autoskip sem a extensão |
| Gate | `gate_run_debug.rs` (3, peer fiel) | executar + depurar + testar um fluxo PHP real com o peer protocolar (o mesmo DBGp do Xdebug, sem a extensão): garante que o gate segue verde em ambientes sem Xdebug |

## Gate do workspace (2026-11-03, pós-Xdebug-real)

- `cargo fmt --all -- --check`: **verde** (os 4 arquivos legados
  desalinhados foram formatados neste incremento).
- `cargo test --workspace --no-fail-fast`: **240 testes, 0 falhas**
  (core 59+4, dbgp 31+7, exec 14+6, lsp 7+1, php 7+1, run 32+3+4+6,
  syntax 5, ui 52).
- `cargo clippy --workspace --all-targets`: **0 warnings** (os warnings
  legados de `phpide-lsp`/`phpide-php`/`phpide-ui` foram zerados neste
  incremento).

## Correções de robustez (achadas pelo gate)

- **Ordenação no streaming** (`phpide-exec`): o evento `Exit` podia ser
  drenado antes das linhas finais de stdout/stderr (~20% sob carga).
  Correção determinística: join das pumps no waiter antes de `Exit`.
- **`composer_flow` (Estágio 7) — flakiness de indexação**: completion e
  definition eram pedidos uma vez logo após didOpen/didSave; a indexação
  assíncrona do intelephense voltava lista vazia em ambiente lento
  (falha intermitente confirmada em HEAD com `node_modules` local).
  Correção: poll até o item esperado aparecer (deadline de 60s), o que
  espelha o uso real (o usuário completa depois que a indexação assenta).
  Estável em 3/3 execuções consecutivas com o servidor local.

## Decisões e limitações registradas

- **`eval` fica no topo da pilha** (limitação do próprio DBGp para frames
  profundos — o `-d` do `eval` é só sugerido pela spec e o Xdebug avalia
  no frame 0); variáveis endereçam o frame via `property_get -d`.
- **Before-launch tasks** (T-RUN-001-04), **conditional/exception/logpoint
  completos** (T-RUN-DBG-003-02/03), **Rerun Automatically** (T-RUN-TST-001-03)
  e **sessões web/remotas** (RUN-DBG-004) seguem fora deste recorte, conforme
  planejamento em TAREFAS.md.
- **Atalhos de debug contextuais**: com sessão ativa, F9 = Resume (e
  F7/F8/Shift+F8/Alt+F10); fora de sessão, F9 mantém o toggle de breakpoint
  do baseline. A migração global de keymap (conflitos documentados em
  REQUISITOS 6.7) é um requisito próprio.
- **Xdebug carregado por `.so` local** é um meio de validação, não uma
  instalação de sistema: em máquinas com a extensão instalada, o mesmo
  código é exercitado com o Xdebug do sistema (os testes só precisam que
  `zend_extension` carregue; `PHPIDE_XDEBUG_SO` aponta para qualquer `.so`).
