use mio::net::TcpStream;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, Connection, RootCertStore, ServerConfig, ServerConnection,
};
use std::io::{self, ErrorKind, Read, Write};
use std::sync::{Arc, OnceLock};

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn web_roots() -> RootCertStore {
    RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned())
}

fn certificates(pem: &str, what: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let certs = CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{what}: invalid PEM certificate: {e}"))?;
    if certs.is_empty() {
        return Err(format!("{what}: no certificate found in the PEM text"));
    }
    Ok(certs)
}

fn build_client(extra_roots: Option<&str>) -> Result<Arc<ClientConfig>, String> {
    let mut roots = web_roots();
    if let Some(pem) = extra_roots {
        for cert in certificates(pem, "ca")? {
            roots
                .add(cert)
                .map_err(|e| format!("ca: the certificate cannot be a trust anchor: {e}"))?;
        }
    }
    ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("tls: {e}"))
        .map(|builder| {
            Arc::new(
                builder
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
}

fn default_client() -> Result<&'static Arc<ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| build_client(None))
        .as_ref()
        .map_err(Clone::clone)
}

pub fn server_config(cert_pem: &str, key_pem: &str) -> Result<Arc<ServerConfig>, String> {
    let chain = certificates(cert_pem, "cert")?;
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())
        .map_err(|e| format!("key: invalid PEM private key: {e}"))?;
    ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("tls: {e}"))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map(Arc::new)
        .map_err(|e| format!("cert/key: {e}"))
}

pub struct Tls {
    conn: Connection,
    peer_closed: bool,
}

impl Tls {
    pub fn client(host: &str, extra_roots: Option<&str>) -> Result<Self, String> {
        let name = ServerName::try_from(host.to_owned())
            .map_err(|e| format!("'{host}' is not a valid server name: {e}"))?;
        let config = match extra_roots {
            Some(pem) => build_client(Some(pem))?,
            None => Arc::clone(default_client()?),
        };
        let conn = ClientConnection::new(config, name)
            .map_err(|e| format!("cannot start the TLS session: {e}"))?;
        Ok(Self {
            conn: conn.into(),
            peer_closed: false,
        })
    }

    pub fn server(config: &Arc<ServerConfig>) -> Result<Self, String> {
        let conn = ServerConnection::new(Arc::clone(config))
            .map_err(|e| format!("cannot start the TLS session: {e}"))?;
        Ok(Self {
            conn: conn.into(),
            peer_closed: false,
        })
    }

    pub fn is_handshaking(&self) -> bool {
        self.conn.is_handshaking()
    }

    pub fn pump(&mut self, stream: &mut TcpStream) -> io::Result<()> {
        loop {
            let mut progressed = false;
            while self.conn.wants_write() {
                match self.conn.write_tls(stream) {
                    Ok(0) => break,
                    Ok(_) => progressed = true,
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
            if self.conn.wants_read() && !self.peer_closed {
                match self.conn.read_tls(stream) {
                    Ok(0) => self.peer_closed = true,
                    Ok(_) => {
                        if let Err(e) = self.conn.process_new_packets() {
                            let _ = self.conn.write_tls(stream);
                            return Err(io::Error::new(ErrorKind::InvalidData, e));
                        }
                        progressed = true;
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e),
                }
            }
            if !progressed {
                return Ok(());
            }
        }
    }

    pub fn read(&mut self, stream: &mut TcpStream, buf: &mut [u8]) -> io::Result<usize> {
        self.pump(stream)?;
        match self.conn.reader().read(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == ErrorKind::WouldBlock && self.peer_closed => Ok(0),
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(0),
            Err(e) => Err(e),
        }
    }

    pub fn write(&mut self, stream: &mut TcpStream, data: &[u8]) -> io::Result<usize> {
        let accepted = self.conn.writer().write(data)?;
        self.pump(stream)?;
        if accepted == 0 && !data.is_empty() {
            return Err(io::Error::from(ErrorKind::WouldBlock));
        }
        Ok(accepted)
    }

    pub fn close(&mut self, stream: &mut TcpStream) {
        self.conn.send_close_notify();
        let _ = self.conn.write_tls(stream);
    }
}
