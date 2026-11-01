//! Cliente DBGp mínimo — protocolo de depuração do Xdebug (Estágio 4).
//!
//! XML sobre TCP, mensagens terminadas em `\0`. Escopo MVP: a IDE escuta a
//! porta, o PHP (com Xdebug) conecta de volta, registramos breakpoints,
//! damos `run`, e ao parar lemos pilha (`stack_get`) e variáveis locais
//! (`context_get`) antes do `detach`. Stepping interativo fica para o 4.1.
//!
//! Referência: DBGp spec (https://xdebug.org/docs/dbgp) — as respostas são
//! parseadas por atributos, sem depender de crate XML.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 9003;

/// Mensagens para o painel Execução durante uma sessão de debug.
pub enum DebugMsg {
    Line(String),
    Done,
}

// ---------------------------------------------------------------------------
// Codificação / parsing
// ---------------------------------------------------------------------------

/// Monta um comando DBGp: `cmd -a v ... -i id\0` (sem suporte a `-- data` no MVP).
pub fn encode_command(cmd: &str, args: &[(&str, &str)], transaction: i64) -> Vec<u8> {
    let mut s = cmd.to_string();
    for (k, v) in args {
        s.push_str(&format!(" -{k} {v}"));
    }
    s.push_str(&format!(" -i {transaction}\0"));
    s.into_bytes()
}

/// Lê um pacote terminado em \0. None em EOF.
pub fn read_packet(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = reader.read_until(0, &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&0) {
        buf.pop();
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// Extrai o valor de um atributo XML: `status="break"`.
pub fn attr<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = xml.find(&needle)? + needle.len();
    let rel_end = xml[start..].find('"')?;
    Some(&xml[start..start + rel_end])
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResponseInfo {
    pub command: String,
    pub transaction: String,
    pub success: bool,
    pub status: String,
}

pub fn parse_response(xml: &str) -> Option<ResponseInfo> {
    Some(ResponseInfo {
        command: attr(xml, "command")?.to_string(),
        transaction: attr(xml, "transaction_id").unwrap_or("").to_string(),
        success: attr(xml, "success") == Some("1"),
        status: attr(xml, "status").unwrap_or("").to_string(),
    })
}

/// Nomes das propriedades (variáveis) de um context_get.
pub fn property_names(xml: &str) -> Vec<String> {
    xml.match_indices("<property ")
        .filter_map(|(i, _)| attr(&xml[i..], "name").map(str::to_string))
        .collect()
}

/// file:// URI para breakpoint_set (mesma codificação mínima do cliente LSP).
pub fn fileuri(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::from("file://");
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '~' | '@' | ':' | '+' | '=' | ',') {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Sessão
// ---------------------------------------------------------------------------

pub struct DbgpSession {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    transaction: i64,
}

impl DbgpSession {
    /// Envia comando e devolve o XML completo da resposta.
    pub fn send_xml(&mut self, cmd: &str, args: &[(&str, &str)]) -> io::Result<String> {
        self.transaction += 1;
        self.stream.write_all(&encode_command(cmd, args, self.transaction))?;
        read_packet(&mut self.reader)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "debugger encerrou"))
    }

    /// Envia comando e devolve só os metadados.
    pub fn send(&mut self, cmd: &str, args: &[(&str, &str)]) -> io::Result<ResponseInfo> {
        let xml = self.send_xml(cmd, args)?;
        parse_response(&xml).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "resposta DBGp inválida"))
    }
}

/// Escuta a porta com prazo; aceita a conexão do Xdebug e consome o `<init>`.
pub fn listen(port: u16, timeout: Duration) -> io::Result<Option<DbgpSession>> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => continue,
                });
                if let Ok(Some(init)) = read_packet(&mut reader) {
                    if init.contains("<init") {
                        return Ok(Some(DbgpSession {
                            stream,
                            reader,
                            transaction: 0,
                        }));
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Ok(None);
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Sessão completa do MVP: breakpoints -> run -> (pilha+variáveis se parar) -> detach.
/// Tudo o que acontece é reportado linha a linha pela canal.
pub fn run_with_breakpoints(
    port: u16,
    wait: Duration,
    breakpoints: &[(PathBuf, u32)],
    tx: mpsc::Sender<DebugMsg>,
) -> io::Result<()> {
    let say = |tx: &mpsc::Sender<DebugMsg>, line: String| {
        let _ = tx.send(DebugMsg::Line(line));
    };
    say(&tx, format!("[debug] aguardando xdebug em 127.0.0.1:{port} ({:?})...", wait));
    let Some(mut session) = listen(port, wait)? else {
        say(&tx, "[debug] tempo esgotado — xdebug não conectou".to_string());
        let _ = tx.send(DebugMsg::Done);
        return Ok(());
    };
    say(&tx, "[debug] xdebug conectado".to_string());

    for (path, line) in breakpoints {
        let uri = fileuri(path);
        let ln = line.to_string();
        match session.send("breakpoint_set", &[("t", "line"), ("f", &uri), ("n", &ln)]) {
            Ok(r) if r.success => say(&tx, format!("[debug] breakpoint {uri}:{ln} ok")),
            Ok(_) => say(&tx, format!("[debug] breakpoint {uri}:{ln} recusado")),
            Err(e) => say(&tx, format!("[debug] erro no breakpoint: {e}")),
        }
    }

    match session.send("run", &[]) {
        Ok(r) => {
            say(&tx, format!("[debug] run -> status={}", r.status));
            if r.status == "break" {
                if let Ok(stack) = session.send_xml("stack_get", &[]) {
                    let frame = stack
                        .match_indices("<stack ")
                        .next()
                        .and_then(|(i, _)| {
                            let f = attr(&stack[i..], "filename")?;
                            let l = attr(&stack[i..], "lineno")?;
                            Some(format!("{f}:{}", l))
                        })
                        .unwrap_or_else(|| "?".to_string());
                    say(&tx, format!("[debug] parado em {frame}"));
                }
                if let Ok(ctx) = session.send_xml("context_get", &[("c", "0")]) {
                    let names = property_names(&ctx);
                    say(
                        &tx,
                        format!("[debug] variáveis locais: {}", names.join(", ")),
                    );
                }
            }
        }
        Err(e) => say(&tx, format!("[debug] erro no run: {e}")),
    }

    let _ = session.send("detach", &[]);
    say(&tx, "[debug] sessão encerrada (detach)".to_string());
    let _ = tx.send(DebugMsg::Done);
    Ok(())
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_format() {
        let bytes = encode_command("breakpoint_set", &[("t", "line"), ("n", "3")], 7);
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, "breakpoint_set -t line -n 3 -i 7\0");
    }

    #[test]
    fn parses_response_attrs() {
        // run carrega status; success só aparece em comandos como breakpoint_set.
        let xml = r#"<response xmlns="urn:debugger_protocol_v1" command="run" transaction_id="3" status="break" reason="ok"></response>"#;
        let r = parse_response(xml).unwrap();
        assert_eq!(r.command, "run");
        assert_eq!(r.status, "break");
        assert!(!r.success);

        let bp = r#"<response command="breakpoint_set" transaction_id="2" id="10" success="1"></response>"#;
        assert!(parse_response(bp).unwrap().success);

        assert_eq!(attr(xml, "transaction_id"), Some("3"));
        assert_eq!(attr(xml, "inexistente"), None);
    }

    #[test]
    fn extracts_property_names() {
        let xml = r#"<response command="context_get" transaction_id="5"><property name="$total" fullname="$total" type="int"><![CDATA[42]]></property><property name="$items" fullname="$items" type="array"></property></response>"#;
        assert_eq!(property_names(xml), vec!["$total", "$items"]);
    }

    #[test]
    fn packet_roundtrip() {
        let mut reader = BufReader::new("dGVzdA==\0".as_bytes());
        assert_eq!(read_packet(&mut reader).unwrap().unwrap(), "dGVzdA==");
    }
}
