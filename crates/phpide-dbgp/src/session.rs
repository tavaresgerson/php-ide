//! Sessão DBGp completa — `T-RUN-DBG-001-03`, `T-RUN-DBG-002-01/02`.
//!
//! Sobre [`DbgpSession`] (proto):
//! - **Resume** (`run`), **Step Into/Over/Out** (`step_into`/`step_over`/
//!   `step_out` — F9/F7/F8/Shift+F8) com resultado padronizado
//!   [`StepOutcome`]: parou (com motivo), script encerrou ou sessão morreu;
//! - **Show Execution Point** (`Alt+F10`): `stack` devolve os frames e o
//!   nível 0 aponta o ponto de execução atual;
//! - **Variáveis por frame**: `variables(depth)` → `context_get -c 0 -d d`;
//!   valores com CDATA; filhos indicados por `numchildren`;
//! - **Evaluate por frame** (`Alt+F8`) e **Watches**: `evaluate` tenta
//!   `property_get` (endereça o frame pelo `-d`); avaliações ficam
//!   associadas ao frame e são inválidas após Resume (a UI descarta os
//!   valores ao retomar — RUN-DBG-002);
//! - **Detach/Stop**: `detach` fecha o socket pelo Drop/close explícito
//!   (cleanup — T-RUN-DBG-001-04).

use std::io;
use std::path::PathBuf;

use crate::proto::{attr, DbgpSession};

/// Frame de pilha (stack_get).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub level: u32,
    /// Nome/where: `{main}`, `Class->method`, função.
    pub name: String,
    /// Arquivo local (já convertido de fileuri).
    pub file: Option<PathBuf>,
    pub line: u32,
    /// Tipo do frame: file, eval, include...
    pub kind: String,
}

/// Variável de um contexto.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Variable {
    pub name: String,
    pub fullname: String,
    pub type_name: String,
    /// Valor texto (CDATA) quando o backend o enviou.
    pub value: Option<String>,
    /// Número de filhos declarado pelo backend.
    pub num_children: u32,
}

/// Por que a execução parou.
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// Parada em breakpoint/linha: arquivo e linha locais.
    Location { file: PathBuf, line: u32 },
    /// Exceção lançada (mensagem do xdebug:message).
    Exception(String),
    /// Fim do script (status stopped/stopping).
    Ended,
}

/// Resultado de run/step.
#[derive(Debug, Clone, PartialEq)]
pub enum StepOutcome {
    /// Parou (breakpoint, step ou exceção).
    Stopped(StopReason),
    /// Script chegou ao fim.
    Ended,
    /// Sessão encerrada pelo engine (socket morto).
    Disconnected,
}

/// Comando de continuação da execução (toolbar de debug).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinueCommand {
    /// F9 — retoma até o próximo breakpoint/fim.
    Resume,
    /// F7 — entra na chamada.
    StepInto,
    /// F8 — passa por cima.
    StepOver,
    /// Shift+F8 — sai da função atual.
    StepOut,
}

impl ContinueCommand {
    pub const fn as_str(self) -> &'static str {
        match self {
            ContinueCommand::Resume => "run",
            ContinueCommand::StepInto => "step_into",
            ContinueCommand::StepOver => "step_over",
            ContinueCommand::StepOut => "step_out",
        }
    }

    /// Atalho de referência (documentado no keymap de debug).
    pub const fn shortcut(self) -> &'static str {
        match self {
            ContinueCommand::Resume => "F9",
            ContinueCommand::StepInto => "F7",
            ContinueCommand::StepOver => "F8",
            ContinueCommand::StepOut => "Shift+F8",
        }
    }
}

/// Parse do stack_get → frames (arquivos convertidos de fileuri).
pub fn parse_stack(xml: &str, to_local: &dyn Fn(&str) -> Option<PathBuf>) -> Vec<Frame> {
    xml.match_indices("<stack ")
        .filter_map(|(i, _)| {
            let chunk = &xml[i..];
            Some(Frame {
                level: attr(chunk, "level")?.parse().ok()?,
                name: attr(chunk, "where").unwrap_or("?").to_string(),
                file: attr(chunk, "filename").and_then(to_local),
                line: attr(chunk, "lineno")
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(0),
                kind: attr(chunk, "type").unwrap_or("file").to_string(),
            })
        })
        .collect()
}

/// Parse de context_get → variáveis (com valor CDATA quando presente).
///
/// Valores com `encoding="base64"` (o Xdebug 3 os usa para strings) são
/// decodificados para o texto original.
pub fn parse_context(xml: &str) -> Vec<Variable> {
    let mut out = Vec::new();
    for (i, _) in xml.match_indices("<property ") {
        let chunk = &xml[i..];
        let tag_end = chunk.find('>').map(|p| p + 1).unwrap_or(chunk.len());
        let own_tag = &chunk[..tag_end];
        let mut value = cdata_of(chunk);
        // `encoding` olha só a tag própria: o atributo de um <property> irmão
        // mais adiante no XML não pode contaminar este valor.
        if attr(own_tag, "encoding").is_some_and(|e| e == "base64") {
            value = value
                .as_deref()
                .and_then(base64_decode)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        }
        out.push(Variable {
            name: attr(chunk, "name").unwrap_or("?").to_string(),
            fullname: attr(chunk, "fullname").unwrap_or("?").to_string(),
            type_name: attr(chunk, "type").unwrap_or("uninitialized").to_string(),
            value,
            num_children: attr(chunk, "numchildren")
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
        });
    }
    out
}

/// Extrai o primeiro CDATA do elemento (`<property ...><![CDATA[x]]>`).
fn cdata_of(chunk: &str) -> Option<String> {
    let start = chunk.find("<![CDATA[")? + "<![CDATA[".len();
    let end = chunk[start..].find("]]>")? + start;
    Some(chunk[start..end].to_string())
}

/// Watch registrado (avaliado a cada parada; invalidado após Resume).
#[derive(Debug, Clone, PartialEq)]
pub struct Watch {
    pub id: u64,
    pub expression: String,
}

/// Valor avaliado de um watch na parada corrente.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchValue {
    pub watch_id: u64,
    pub result: Result<Option<Variable>, String>,
}

/// Comandos de inspeção sobre a sessão.
impl DbgpSession {
    /// Continua a execução (run/step_*), mapeando o resultado.
    ///
    /// `mappings` converte fileuris do engine em caminhos locais.
    pub fn continue_exec(
        &mut self,
        cmd: ContinueCommand,
        to_local: &dyn Fn(&str) -> Option<PathBuf>,
    ) -> io::Result<StepOutcome> {
        let xml = self.send_xml(cmd.as_str(), &[])?;
        let Some(info) = crate::proto::parse_response(&xml) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resposta run inválida",
            ));
        };
        match info.status.as_str() {
            "break" => {
                let reason = stop_reason_from(&xml, to_local);
                Ok(StepOutcome::Stopped(reason))
            }
            "stopping" | "stopped" => Ok(StepOutcome::Ended),
            _ => Ok(StepOutcome::Disconnected),
        }
    }

    /// Pilha de chamadas (frames). Frame 0 = ponto de execução.
    pub fn stack(&mut self, to_local: &dyn Fn(&str) -> Option<PathBuf>) -> io::Result<Vec<Frame>> {
        let xml = self.send_xml("stack_get", &[])?;
        Ok(parse_stack(&xml, to_local))
    }

    /// Variáveis locais do frame `depth` (0 = frame atual).
    pub fn variables(&mut self, depth: u32) -> io::Result<Vec<Variable>> {
        let d = depth.to_string();
        let xml = self.send_xml("context_get", &[("c", "0"), ("d", &d)])?;
        Ok(parse_context(&xml))
    }

    /// Avalia expressão associada a um frame.
    ///
    /// Variáveis (`$a`, `$a->b`, `$a[0]`) endereçam o frame diretamente via
    /// `property_get -n <expr> -d <depth> -c 0` — é o que mantém a avaliação
    /// associada ao frame selecionado (RUN-DBG-002). Demais expressões
    /// (`1+2`, chamadas) usam `eval`, que o Xdebug avalia no topo da pilha
    /// (limitação do DBGp para frames profundos — documentada).
    pub fn evaluate(&mut self, expression: &str, depth: u32) -> io::Result<Option<Variable>> {
        if expression.starts_with('$') {
            let d = depth.to_string();
            let xml = self.send_xml("property_get", &[("n", expression), ("d", &d), ("c", "0")])?;
            match crate::proto::parse_response(&xml) {
                Some(info) if info.success => Ok(parse_context(&xml).into_iter().next()),
                // success=0: variável indisponível neste frame — não é erro.
                Some(_) => Ok(None),
                None => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "resposta property_get inválida",
                )),
            }
        } else {
            let encoded = base64_encode(expression.as_bytes());
            let xml = self.send_xml_data("eval", &encoded)?;
            match crate::proto::parse_response(&xml) {
                Some(info) if info.success => Ok(parse_context(&xml).into_iter().next()),
                Some(_) => Ok(None),
                None => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "resposta eval inválida",
                )),
            }
        }
    }

    /// Avalia todos os watches no frame selecionado (T-RUN-DBG-002-02).
    pub fn evaluate_watches(&mut self, watches: &[Watch], depth: u32) -> Vec<WatchValue> {
        watches
            .iter()
            .map(|w| {
                let result = self
                    .evaluate(&w.expression, depth)
                    .map_err(|e| e.to_string());
                WatchValue {
                    watch_id: w.id,
                    result,
                }
            })
            .collect()
    }

    /// Detach: o engine segue rodando; socket fechado de forma confiável.
    pub fn detach(&mut self) -> io::Result<()> {
        let _ = self.send("detach", &[]);
        self.close();
        Ok(())
    }

    /// Stop: pede ao engine que pare e fecha o socket.
    pub fn stop(&mut self) -> io::Result<()> {
        let _ = self.send("stop", &[]);
        self.close();
        Ok(())
    }
}

/// Extrai o motivo da parada do XML do run/step.
fn stop_reason_from(xml: &str, to_local: &dyn Fn(&str) -> Option<PathBuf>) -> StopReason {
    if let Some(msg_idx) = xml.find("<xdebug:message ") {
        let chunk = &xml[msg_idx..];
        let message = attr(chunk, "exception").map(str::to_string);
        if let Some(message) = message {
            return StopReason::Exception(message);
        }
        let file = attr(chunk, "filename").and_then(to_local);
        let line = attr(chunk, "lineno")
            .and_then(|l| l.parse().ok())
            .unwrap_or(0);
        if let Some(file) = file {
            return StopReason::Location { file, line };
        }
    }
    StopReason::Exception("parada sem localização".to_string())
}

/// Base64 para o argumento -e do eval (RFC 4648, sem wraps).
pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decode base64 tolerante a padding (valores `encoding="base64"` do Xdebug).
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut acc = 0u32;
    let mut nbits = 0u32;
    for &b in &bytes {
        if b == b'=' {
            break;
        }
        acc = (acc << 6) | val(b)?;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_local(uri: &str) -> Option<PathBuf> {
        crate::proto::uri_to_path(uri)
            .or_else(|| PathBuf::from(uri).is_absolute().then(|| PathBuf::from(uri)))
    }

    #[test]
    fn parses_stack_frames_with_local_paths() {
        let xml = r#"<response command="stack_get" transaction_id="4"><stack where="{main}" level="0" type="file" filename="file:///proj/index.php" lineno="12"/><stack where="App\Calc->soma" level="1" type="file" filename="file:///proj/src/Calc.php" lineno="8"/></response>"#;
        let frames = parse_stack(xml, &identity_local);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].name, "{main}");
        assert_eq!(frames[0].file, Some(PathBuf::from("/proj/index.php")));
        assert_eq!(frames[0].line, 12);
        assert_eq!(frames[1].name, r"App\Calc->soma");
        assert_eq!(frames[1].level, 1);
    }

    #[test]
    fn parses_context_variables_with_values() {
        let xml = r#"<response command="context_get" transaction_id="5"><property name="$total" fullname="$total" type="int"><![CDATA[42]]></property><property name="$items" fullname="$items" type="array" numchildren="2" encoding="base64"></property></response>"#;
        let vars = parse_context(xml);
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "$total");
        assert_eq!(vars[0].value.as_deref(), Some("42"));
        assert_eq!(vars[1].type_name, "array");
        assert_eq!(vars[1].num_children, 2);
        assert_eq!(vars[1].value, None);
    }

    #[test]
    fn stop_reason_break_location() {
        let xml = r#"<response command="run" transaction_id="3" status="break" reason="ok"><xdebug:message filename="file:///proj/index.php" lineno="7"/></response>"#;
        let reason = stop_reason_from(xml, &identity_local);
        assert_eq!(
            reason,
            StopReason::Location {
                file: PathBuf::from("/proj/index.php"),
                line: 7
            }
        );
    }

    #[test]
    fn stop_reason_exception_message() {
        let xml = r#"<response command="run" transaction_id="3" status="break" reason="exception"><xdebug:message exception="DivisionByZeroError" filename="file:///proj/x.php" lineno="9"/></response>"#;
        let reason = stop_reason_from(xml, &identity_local);
        assert_eq!(
            reason,
            StopReason::Exception("DivisionByZeroError".to_string())
        );
    }

    #[test]
    fn continue_command_names_match_dbgp() {
        assert_eq!(ContinueCommand::Resume.as_str(), "run");
        assert_eq!(ContinueCommand::StepInto.as_str(), "step_into");
        assert_eq!(ContinueCommand::StepOver.as_str(), "step_over");
        assert_eq!(ContinueCommand::StepOut.as_str(), "step_out");
        assert_eq!(ContinueCommand::Resume.shortcut(), "F9");
        assert_eq!(ContinueCommand::StepOut.shortcut(), "Shift+F8");
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64_encode(b"1+2"), "MSsy");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b""), "");
        // Roundtrip com padding e sem.
        assert_eq!(base64_decode("YWI=").unwrap(), b"ab");
        assert_eq!(base64_decode("b2kh").unwrap(), b"oi!");
        assert_eq!(
            base64_decode(&base64_encode(b"debug $a[0]")).unwrap(),
            b"debug $a[0]"
        );
        assert!(base64_decode("!@#").is_none());
    }

    #[test]
    fn parses_base64_encoded_property_values() {
        // Xdebug 3 marca strings com encoding="base64" e manda o CDATA em
        // base64 — o valor decodificado é o texto original.
        let xml = r#"<response command="eval" transaction_id="6"><property type="string" size="3" encoding="base64"><![CDATA[b2kh]]></property></response>"#;
        let vars = parse_context(xml);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].value.as_deref(), Some("oi!"));
        let plain =
            r#"<response command="eval"><property type="int"><![CDATA[42]]></property></response>"#;
        assert_eq!(parse_context(plain)[0].value.as_deref(), Some("42"));
    }
}
