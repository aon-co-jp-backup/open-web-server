//! 第1層: 伝送路暗号化 (TLS 1.3 / rustls)

use std::{fs::File, io::BufReader, path::Path, sync::Arc};

use rustls_pki_types::{CertificateDer, PrivateKeyDer};

#[derive(Debug, Clone)]
pub struct TlsServerConfig {
    pub cert_path: String,
    pub key_path: String,
}

impl TlsServerConfig {
    pub fn load(&self) -> anyhow::Result<Arc<rustls::ServerConfig>> {
        let certs = load_certs(&self.cert_path)?;
        let key = load_key(&self.key_path)?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(Arc::new(config))
    }
}

fn load_certs(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(Path::new(path))?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    Ok(certs)
}

fn load_key(path: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    let file = File::open(Path::new(path))?;
    let mut reader = BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("no private key found at {path}"))?;
    Ok(key)
}
