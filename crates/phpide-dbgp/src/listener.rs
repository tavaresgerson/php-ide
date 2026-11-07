//! Listener DBGp, IDE key e XDEBUG_CONFIG — `T-RUN-DBG-001-01` (RUN-DBG-001).
//!
//! - Listener configurável (porta default 9003) com filtro de **IDE key**:
//!   conexão cujo `<init>` traz idekey inesperado é recusada com `detach`
//!   imediato (impressão digital antes dos breakpoints);
//! - [`xdebug_env`] monta `XDEBUG_CONFIG` mesclando com o valor existente do
//!   ambiente: `client_host`/`client_port` são sobrescritos para a IDE, mas
//!   chaves externas necessárias (`mode`, `start_with_request`, flags do
//!   usuário) são preservadas — RUN-DBG-001: "sem apagar env necessária";
//! - `XDEBUG_SESSION` transporta a IDE key para o trigger de sessão.

use std::io;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::proto::{attr, read_message, DbgpSession, DEFAULT_PORT};

/// IDE key padrão da phpide.
pub const DEFAULT_IDE_KEY: &str = "phpide";

/// Metadados do `<init>` do Xdebug.
#[derive(Debug, Clone, PartialEq)]
pub struct InitInfo {
    /// fileuri do script inicial (associação ao projeto, T-RUN-DBG-001-02).
    pub fileuri: String,
    /// IDE key anunciada pelo engine.
    pub idekey: Option<String>,
    /// appid do processo debugado.
    pub appid: String,
}

/// Parse do `<init>`.
pub fn parse_init(xml: &str) -> Option<InitInfo> {
    if !xml.contains("<init") {
        return None;
    }
    Some(InitInfo {
        fileuri: attr(xml, "fileuri").unwrap_or("").to_string(),
        idekey: attr(xml, "idekey").map(str::to_string),
        appid: attr(xml, "appid").unwrap_or("").to_string(),
    })
}

/// Configuração do listener.
#[derive(Debug, Clone)]
pub struct ListenerConfig {
    pub port: u16,
    /// `None` = aceita qualquer IDE key.
    pub ide_key: Option<String>,
    /// Prazo para o `<init>` após o accept (defesa contra cliente morto).
    pub init_timeout: Duration,
}

impl Default for ListenerConfig {
    fn default() -> Self {
        ListenerConfig {
            port: DEFAULT_PORT,
            ide_key: Some(DEFAULT_IDE_KEY.to_string()),
            init_timeout: Duration::from_secs(5),
        }
    }
}

/// Resultado de uma tentativa de aceite.
pub enum AcceptOutcome {
    /// Sessão aceita com o init correspondente.
    Accepted(DbgpSession, InitInfo),
    /// IDE key inesperada — conexão recusada (detach enviado, socket fechado).
    RejectedKey { got: Option<String> },
    /// Timeout sem conexão.
    Timeout,
}

impl std::fmt::Debug for AcceptOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcceptOutcome::Accepted(_, init) => f
                .debug_tuple("Accepted")
                .field(&format_args!("<sessão>"))
                .field(init)
                .finish(),
            AcceptOutcome::RejectedKey { got } => {
                f.debug_struct("RejectedKey").field("got", got).finish()
            }
            AcceptOutcome::Timeout => write!(f, "Timeout"),
        }
    }
}

/// Listener não-bloqueante: aceita UMA conexão por chamada (a UI faz poll).
pub struct DebugListener {
    listener: TcpListener,
    config: ListenerConfig,
}

impl DebugListener {
    pub fn bind(config: ListenerConfig) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", config.port))?;
        listener.set_nonblocking(true)?;
        Ok(DebugListener { listener, config })
    }

    /// Porta efetiva (útil com porta 0 em testes).
    pub fn port(&self) -> u16 {
        self.listener
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(self.config.port)
    }

    pub fn config(&self) -> &ListenerConfig {
        &self.config
    }

    /// Tenta aceitar uma conexão DBGp dentro do prazo.
    pub fn accept(&self, timeout: Duration) -> io::Result<AcceptOutcome> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(outcome) = self.handshake(stream)? {
                        return Ok(outcome);
                    }
                    // init ilegível/EOF: segue aguardando até o prazo.
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return Ok(AcceptOutcome::Timeout);
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(15)),
            }
        }
    }

    fn handshake(&self, stream: TcpStream) -> io::Result<Option<AcceptOutcome>> {
        stream.set_read_timeout(Some(self.config.init_timeout))?;
        let mut reader = io::BufReader::new(stream.try_clone()?);
        // read_message já consome o pacote de comprimento que o Xdebug 3
        // prefixa em toda mensagem ("473\0" antes do <init> — achado com a
        // extensão real; sem isso a conexão era descartada no primeiro pacote).
        let Some(init_xml) = read_message(&mut reader)? else {
            return Ok(None);
        };
        let Some(init) = parse_init(&init_xml) else {
            return Ok(None);
        };
        let key_ok = match (&self.config.ide_key, &init.idekey) {
            (None, _) => true,
            (Some(expected), got) => got.as_ref().map(|g| g == expected).unwrap_or(false),
        };
        if !key_ok {
            // Recusa educada: detach + close (cleanup — T-RUN-DBG-001-04).
            let mut session = DbgpSession::attach(stream, reader);
            let _ = session.send("detach", &[]);
            session.close();
            return Ok(Some(AcceptOutcome::RejectedKey { got: init.idekey }));
        }
        Ok(Some(AcceptOutcome::Accepted(
            DbgpSession::attach(stream, reader),
            init,
        )))
    }
}

impl Drop for DebugListener {
    fn drop(&mut self) {
        // Libera a porta ao desligar o listener (cleanup — T-RUN-DBG-001-04).
        let _ = self.listener.set_nonblocking(false);
    }
}

/// Monta as variáveis de ambiente para disparar o Xdebug contra a IDE.
///
/// `existing` é o valor atual de `XDEBUG_CONFIG` no ambiente do processo
/// (pode conter `mode=debug start_with_request=yes` etc.): os pares são
/// preservados, e só `client_host`/`client_port` são sobrescritos.
pub fn xdebug_env(port: u16, ide_key: &str, existing: Option<&str>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(existing) = existing {
        for token in existing.split_whitespace() {
            if let Some((k, v)) = token.split_once('=') {
                if k == "client_host" || k == "client_port" {
                    continue;
                }
                pairs.push((k.to_string(), v.to_string()));
            }
        }
    }
    pairs.push(("client_host".into(), "127.0.0.1".into()));
    pairs.push(("client_port".into(), port.to_string()));
    let xdebug_config = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        ("XDEBUG_CONFIG".into(), xdebug_config),
        ("XDEBUG_SESSION".into(), ide_key.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_attrs() {
        let xml = r#"<init xmlns="urn:debugger_protocol_v1" fileuri="file:///proj/index.php" language="PHP" protocol_version="1.0" appid="4242" idekey="phpide"></init>"#;
        let init = parse_init(xml).unwrap();
        assert_eq!(init.fileuri, "file:///proj/index.php");
        assert_eq!(init.idekey.as_deref(), Some("phpide"));
        assert_eq!(init.appid, "4242");
        assert_eq!(parse_init("<response command='run'/>"), None);
    }

    #[test]
    fn xdebug_env_merges_without_clobbering() {
        let env = xdebug_env(
            9003,
            "phpide",
            Some("mode=debug start_with_request=yes client_port=1"),
        );
        let (name, value) = &env[0];
        assert_eq!(name, "XDEBUG_CONFIG");
        assert!(value.contains("mode=debug"), "preserva mode: {value}");
        assert!(
            value.contains("start_with_request=yes"),
            "preserva trigger: {value}"
        );
        assert!(
            value.contains("client_port=9003"),
            "sobrescreve a porta: {value}"
        );
        assert!(
            !value.contains("client_port=1"),
            "porta antiga removida: {value}"
        );
        assert!(value.contains("client_host=127.0.0.1"));
        assert_eq!(env[1], ("XDEBUG_SESSION".to_string(), "phpide".to_string()));
    }

    #[test]
    fn xdebug_env_without_existing_has_only_client() {
        let env = xdebug_env(9000, "IDEA", None);
        assert_eq!(env[0].1, "client_host=127.0.0.1 client_port=9000");
    }

    #[test]
    fn listener_rejects_wrong_ide_key() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let listener = DebugListener::bind(ListenerConfig {
            port,
            ide_key: Some("phpide".into()),
            ..Default::default()
        })
        .unwrap();
        // Conecta com idekey errada.
        std::thread::spawn(move || {
            for _ in 0..50 {
                if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
                    use std::io::Write;
                    let init = r#"<init xmlns="urn:debugger_protocol_v1" fileuri="file:///x.php" language="PHP" appid="1" idekey="outro"></init>"#;
                    s.write_all(init.as_bytes()).unwrap();
                    s.write_all(&[0]).unwrap();
                    // Espera o detach e devolve EOF.
                    let mut buf = Vec::new();
                    use std::io::Read;
                    let _ = s.read_to_end(&mut buf);
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let outcome = listener.accept(Duration::from_secs(10)).unwrap();
        match outcome {
            AcceptOutcome::RejectedKey { got } => assert_eq!(got.as_deref(), Some("outro")),
            other => panic!("esperava rejeição por ide key: {other:?}"),
        }
        // Timeout depois: porta ainda viva, sem conexão.
        let outcome = listener.accept(Duration::from_millis(300)).unwrap();
        assert!(matches!(outcome, AcceptOutcome::Timeout));
    }

    #[test]
    fn listener_accepts_matching_key_and_port_is_reusable() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let listener = DebugListener::bind(ListenerConfig {
            port,
            ide_key: Some("phpide".into()),
            ..Default::default()
        })
        .unwrap();
        let effective_port = listener.port();
        assert_eq!(effective_port, port);
        std::thread::spawn(move || {
            for _ in 0..50 {
                if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
                    use std::io::Write;
                    let init = r#"<init xmlns="urn:debugger_protocol_v1" fileuri="file:///x.php" language="PHP" appid="1" idekey="phpide"></init>"#;
                    s.write_all(init.as_bytes()).unwrap();
                    s.write_all(&[0]).unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let outcome = listener.accept(Duration::from_secs(10)).unwrap();
        match outcome {
            AcceptOutcome::Accepted(mut session, init) => {
                assert_eq!(init.idekey.as_deref(), Some("phpide"));
                session.close();
            }
            other => panic!("esperava aceite: {other:?}"),
        }
        drop(listener);
        // Cleanup: a porta é liberada após o drop (T-RUN-DBG-001-04).
        std::thread::sleep(Duration::from_millis(100));
        TcpListener::bind(("127.0.0.1", port)).expect("porta liberada após drop do listener");
    }

    #[test]
    fn listener_accepts_length_prefixed_init_like_xdebug3() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let listener = DebugListener::bind(ListenerConfig {
            port,
            ide_key: Some("phpide".into()),
            ..Default::default()
        })
        .unwrap();
        // Xdebug 3 real manda "473\0<?xml ...\n<init .../>\0" — o primeiro
        // pacote é só o comprimento do XML em dígitos.
        std::thread::spawn(move || {
            for _ in 0..50 {
                if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
                    use std::io::Write;
                    let init = r#"<?xml version="1.0" encoding="iso-8859-1"?>
<init xmlns="urn:debugger_protocol_v1" xmlns:xdebug="https://xdebug.org/dbgp/xdebug" fileuri="file:///x.php" language="PHP" xdebug:language_version="8.2.30" protocol_version="1.0" appid="3766286" idekey="phpide"><engine version="3.2.0"><![CDATA[Xdebug]]></engine></init>"#;
                    s.write_all(init.len().to_string().as_bytes()).unwrap();
                    s.write_all(&[0]).unwrap();
                    s.write_all(init.as_bytes()).unwrap();
                    s.write_all(&[0]).unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let outcome = listener.accept(Duration::from_secs(10)).unwrap();
        match outcome {
            AcceptOutcome::Accepted(mut session, init) => {
                assert_eq!(init.idekey.as_deref(), Some("phpide"));
                assert_eq!(init.fileuri, "file:///x.php");
                session.close();
            }
            other => panic!("esperava aceite com init prefixado por comprimento: {other:?}"),
        }
    }
}
