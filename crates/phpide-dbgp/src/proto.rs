//! Codificação/parsing DBGp — protocolo Xdebug (Estágio 4, estendido no 8).
//!
//! XML sobre TCP, mensagens terminadas em `\0`. Parsing por atributos, sem
//! crate XML (mesma política do cliente LSP). Referência:
//! https://xdebug.org/docs/dbgp

use std::io::{self, BufRead, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 9003;

/// Monta um comando DBGp: `cmd -a v ... -i id\0`.
pub fn encode_command(cmd: &str, args: &[(&str, &str)], transaction: i64) -> Vec<u8> {
    let mut s = cmd.to_string();
    for (k, v) in args {
        s.push_str(&format!(" -{k} {v}"));
    }
    s.push_str(&format!(" -i {transaction}\0"));
    s.into_bytes()
}

/// Lê um pacote terminado em \0. `None` em EOF.
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

/// Lê a próxima mensagem XML, pulando os pacotes de comprimento que o
/// Xdebug 3 prefixa em TODA mensagem sua (`"204\0"` antes do XML — achado
/// com a extensão real; o peer fiel do ambiente sem Xdebug não os manda).
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    for _ in 0..8 {
        let Some(packet) = read_packet(reader)? else {
            return Ok(None);
        };
        if !packet.is_empty() && packet.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        return Ok(Some(packet));
    }
    Ok(None)
}

/// Extrai o valor de um atributo XML: `status="break"`.
///
/// A chave exige fronteira de espaço/quebra: `id` não casa dentro de
/// `transaction_id` (bug real encontrado na sincronização de breakpoints).
pub fn attr<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let start = xml.find(&needle)? + needle.len();
    let rel_end = xml[start..].find('"')?;
    Some(&xml[start..start + rel_end])
}

/// Metadados da resposta DBGp.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseInfo {
    pub command: String,
    pub transaction: String,
    pub success: bool,
    pub status: String,
}

pub fn parse_response(xml: &str) -> Option<ResponseInfo> {
    // `success` ausente = sucesso: o atributo foi depreciado no DBGp 1.0 e o
    // Xdebug 3 não o envia (erros vêm como `<error>` ou `success="0"` — o
    // peer fiel do ambiente sem extensão manda `success="1"` explicitamente).
    Some(ResponseInfo {
        command: attr(xml, "command")?.to_string(),
        transaction: attr(xml, "transaction_id").unwrap_or("").to_string(),
        success: attr(xml, "success").map(|s| s == "1").unwrap_or(true)
            && error_message(xml).is_none(),
        status: attr(xml, "status").unwrap_or("").to_string(),
    })
}

/// Mensagem de erro embutida na resposta (`<error ...><message>`).
pub fn error_message(xml: &str) -> Option<String> {
    if !xml.contains("<error") {
        return None;
    }
    let start = xml.find("<message>")? + "<message>".len();
    let end = xml[start..].find("</message>")? + start;
    Some(xml[start..end].to_string())
}

/// Nomes das propriedades (variáveis) de um context_get.
pub fn property_names(xml: &str) -> Vec<String> {
    xml.match_indices("<property ")
        .filter_map(|(i, _)| attr(&xml[i..], "name").map(str::to_string))
        .collect()
}

/// file:// URI para breakpoint_set (percent-encoding mínimo e compatível com
/// o que o Xdebug devolve no stack_get).
pub fn fileuri(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::from("file://");
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric()
            || matches!(c, '/' | '-' | '_' | '.' | '~' | '@' | ':' | '+' | '=' | ',')
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Converte um file:// URI (possivelmente percent-encoded) de volta em path.
pub fn uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut out = Vec::new();
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(hex);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok().map(std::path::PathBuf::from)
}

/// Sessão DBGp sobre um socket conectado.
pub struct DbgpSession {
    stream: TcpStream,
    reader: std::io::BufReader<TcpStream>,
    transaction: i64,
    closed: bool,
}

impl DbgpSession {
    /// Anexa um socket já conectado (handshake do listener).
    pub fn attach(stream: TcpStream, reader: std::io::BufReader<TcpStream>) -> Self {
        DbgpSession {
            stream,
            reader,
            transaction: 0,
            closed: false,
        }
    }

    /// Envia comando e devolve o XML completo da resposta.
    pub fn send_xml(&mut self, cmd: &str, args: &[(&str, &str)]) -> io::Result<String> {
        self.transaction += 1;
        self.stream
            .write_all(&encode_command(cmd, args, self.transaction))?;
        read_message(&mut self.reader)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "debugger encerrou"))
    }

    /// Envia comando cujo argumento principal é um bloco de dados posicional
    /// após `--` (forma da spec DBGp para `eval`/`exec`/`stdin`:
    /// `eval -i N -- {DATA base64}` — o `-e` que usávamos é rejeitado pelo
    /// Xdebug 3 com "invalid or missing options", achado com a extensão real).
    pub fn send_xml_data(&mut self, cmd: &str, data: &str) -> io::Result<String> {
        self.transaction += 1;
        let wire = format!("{cmd} -i {} -- {data}\0", self.transaction);
        self.stream.write_all(wire.as_bytes())?;
        read_message(&mut self.reader)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "debugger encerrou"))
    }

    /// Envia comando e devolve só os metadados.
    pub fn send(&mut self, cmd: &str, args: &[(&str, &str)]) -> io::Result<ResponseInfo> {
        let xml = self.send_xml(cmd, args)?;
        parse_response(&xml)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "resposta DBGp inválida"))
    }

    /// Acesso ao socket bruto (interrupção de Detach/Stop da engine).
    pub fn raw_stream(&self) -> &TcpStream {
        &self.stream
    }

    /// Ajusta o read timeout do socket (a engine usa leitura bloqueante e se
    /// interrompe pelo shutdown do closer — T-RUN-DBG-001-04).
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    /// Fecha o socket explicitamente (Detach/Stop — T-RUN-DBG-001-04).
    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            let _ = self.stream.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

impl Drop for DbgpSession {
    fn drop(&mut self) {
        self.close();
    }
}

/// Escuta a porta com prazo e aceita uma conexão DBGp, consumindo o `<init>`.
/// `None` em timeout. O socket fica com read timeout curto para o polling da
/// engine não travar em fim de sessão abrupto.
pub fn listen(port: u16, timeout: Duration) -> io::Result<Option<DbgpSession>> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut reader = std::io::BufReader::new(match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => continue,
                });
                if let Ok(Some(init)) = read_message(&mut reader) {
                    if init.contains("<init") {
                        return Ok(Some(DbgpSession {
                            stream,
                            reader,
                            transaction: 0,
                            closed: false,
                        }));
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

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
        let xml = r#"<response xmlns="urn:debugger_protocol_v1" command="run" transaction_id="3" status="break" reason="ok"></response>"#;
        let r = parse_response(xml).unwrap();
        assert_eq!(r.command, "run");
        assert_eq!(r.status, "break");
        // Sem atributo success e sem <error>: sucesso (DBGp 1.0 deprecou o
        // atributo; Xdebug 3 não o envia).
        assert!(r.success);

        let bp = r#"<response command="breakpoint_set" transaction_id="2" id="10" success="1"></response>"#;
        assert!(parse_response(bp).unwrap().success);
        assert_eq!(attr(xml, "transaction_id"), Some("3"));
        assert_eq!(attr(xml, "inexistente"), None);
    }

    #[test]
    fn response_with_error_element_is_failure_even_without_success_attr() {
        // Xdebug 3 sinaliza erro só com <error> (sem success="0").
        let xml = r#"<response xmlns="urn:debugger_protocol_v1" command="run" transaction_id="9"><error id="4"><message>command is not available</message></error></response>"#;
        let r = parse_response(xml).unwrap();
        assert!(!r.success);
        assert_eq!(
            error_message(xml).as_deref(),
            Some("command is not available")
        );
    }

    #[test]
    fn read_message_skips_xdebug3_length_headers() {
        // Formato real do Xdebug 3: cada mensagem vem precedida de um pacote
        // só com o comprimento ("204\0" antes do XML).
        let wire = b"204\0<?xml version=\"1.0\"?><response command=\"run\" status=\"break\"/>\0";
        let mut reader = std::io::BufReader::new(&wire[..]);
        let msg = read_message(&mut reader).unwrap().unwrap();
        assert!(msg.contains("<response"), "{msg}");
        // Sem header (peer fiel): devolve o primeiro pacote direto.
        let plain = b"<response command=\"run\"/>\0";
        let mut reader = std::io::BufReader::new(&plain[..]);
        let msg = read_message(&mut reader).unwrap().unwrap();
        assert!(msg.contains("<response"), "{msg}");
        // Só headers até EOF: None.
        let lonely = b"111\x00222\0";
        let mut reader = std::io::BufReader::new(&lonely[..]);
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn extracts_error_message() {
        let xml = r#"<response command="breakpoint_set" transaction_id="2" success="0"><error id="200"><message>breakpoint inválido</message></error></response>"#;
        assert_eq!(error_message(xml).as_deref(), Some("breakpoint inválido"));
        let ok = r#"<response command="run" transaction_id="3" status="break"></response>"#;
        assert_eq!(error_message(ok), None);
    }

    #[test]
    fn extracts_property_names() {
        let xml = r#"<response command="context_get" transaction_id="5"><property name="$total" fullname="$total" type="int"><![CDATA[42]]></property><property name="$items" fullname="$items" type="array"></property></response>"#;
        assert_eq!(property_names(xml), vec!["$total", "$items"]);
    }

    #[test]
    fn packet_roundtrip() {
        let mut reader = std::io::BufReader::new("dGVzdA==\0".as_bytes());
        assert_eq!(read_packet(&mut reader).unwrap().unwrap(), "dGVzdA==");
    }

    #[test]
    fn fileuri_roundtrip_with_spaces_and_unicode() {
        let path = std::path::Path::new("/tmp/meus arquivos/café.php");
        let uri = fileuri(path);
        assert!(uri.starts_with("file:///tmp/meus%20arquivos/"));
        assert!(uri.contains("%C3%A9"), "é percent-encoded: {uri}");
        let back = uri_to_path(&uri).unwrap();
        assert_eq!(back, path);
    }
}
