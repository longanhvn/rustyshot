use crate::storage::FtpConfig;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use suppaftp::types::FileType;
use suppaftp::{RustlsConnector, RustlsFtpStream};

#[derive(Clone, Debug)]
pub struct FtpUploadResult {
    pub remote_path: String,
    pub public_url: Option<String>,
}

#[derive(Debug)]
struct AcceptAnyCertVerifier;

impl ServerCertVerifier for AcceptAnyCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn build_rustls_config(ignore_tls_errors: bool) -> Result<ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS protocol config error: {e}"))?;

    if ignore_tls_errors {
        Ok(builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCertVerifier))
            .with_no_client_auth())
    } else {
        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Ok(builder
            .with_root_certificates(root_store)
            .with_no_client_auth())
    }
}

fn connect_and_login(cfg: &FtpConfig) -> Result<RustlsFtpStream, String> {
    let host = cfg.host.trim();
    if host.is_empty() {
        return Err("FTP host is empty".to_string());
    }
    let port = if cfg.port == 0 { 21 } else { cfg.port };
    let addr = format!("{host}:{port}");
    let mut ftp = RustlsFtpStream::connect(&addr)
        .map_err(|e| format!("FTP connect to {addr} failed: {e}"))?;

    if cfg.ftps_explicit {
        let tls_cfg = build_rustls_config(cfg.ignore_tls_errors)?;
        let connector = RustlsConnector::from(Arc::new(tls_cfg));
        ftp = ftp
            .into_secure(connector, host)
            .map_err(|e| format!("FTPS Explicit TLS handshake failed: {e}"))?;
    }

    let user = if cfg.username.trim().is_empty() {
        "anonymous"
    } else {
        cfg.username.trim()
    };
    ftp.login(user, &cfg.password)
        .map_err(|e| format!("FTP login failed for '{user}': {e}"))?;

    ftp.transfer_type(FileType::Binary)
        .map_err(|e| format!("FTP binary mode failed: {e}"))?;
    Ok(ftp)
}

fn ensure_and_cwd(ftp: &mut RustlsFtpStream, remote_dir: &str, username: &str) -> Result<(), String> {
    let trimmed = remote_dir.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return Ok(());
    }

    // 1. Try direct CWD first (works on non-chrooted servers or exact relative paths)
    if ftp.cwd(trimmed).is_ok() {
        return Ok(());
    }

    // 2. If user is chrooted in /home/<username> (HestiaCP/cPanel), try stripping /home/<username>
    let user_prefix = format!("/home/{}", username.trim());
    let effective = if let Some(rest) = trimmed.strip_prefix(&user_prefix) {
        let rel = rest.trim_start_matches('/');
        if !rel.is_empty() && ftp.cwd(rel).is_ok() {
            return Ok(());
        }
        if rel.is_empty() { trimmed } else { rel }
    } else {
        trimmed
    };

    if effective.starts_with('/') && ftp.cwd("/").is_err() {
        // Ignore if server doesn't allow cwd("/")
    }
    for segment in effective.split('/').filter(|s| !s.is_empty()) {
        if ftp.cwd(segment).is_err() {
            ftp.mkdir(segment)
                .map_err(|e| format!("FTP mkdir '{segment}' failed: {e}"))?;
            ftp.cwd(segment)
                .map_err(|e| format!("FTP cwd '{segment}' failed: {e}"))?;
        }
    }
    Ok(())
}

pub fn build_public_url(prefix: &str, filename: &str) -> Option<String> {
    let p = prefix.trim();
    if p.is_empty() {
        return None;
    }
    let clean = p.trim_end_matches('/');
    Some(format!("{clean}/{filename}"))
}

pub fn build_remote_path(remote_dir: &str, filename: &str) -> String {
    let dir = remote_dir.trim().trim_end_matches('/');
    if dir.is_empty() {
        format!("/{filename}")
    } else if dir.starts_with('/') {
        format!("{dir}/{filename}")
    } else {
        format!("/{dir}/{filename}")
    }
}

pub fn test_ftp_connection(cfg: &FtpConfig) -> Result<String, String> {
    let mut ftp = connect_and_login(cfg)?;
    ensure_and_cwd(&mut ftp, &cfg.remote_dir, &cfg.username)?;
    let pwd = ftp.pwd().unwrap_or_else(|_| cfg.remote_dir.clone());
    let _ = ftp.quit();
    let proto = if cfg.ftps_explicit { "FTPS" } else { "FTP" };
    Ok(format!("Connected ({proto})! Dir: {pwd}"))
}

pub fn upload_file_to_ftp(local_path: &Path, cfg: &FtpConfig) -> Result<FtpUploadResult, String> {
    if !local_path.exists() {
        return Err(format!("Local file not found: {}", local_path.display()));
    }
    let filename = local_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid local filename".to_string())?;

    let mut ftp = connect_and_login(cfg)?;
    ensure_and_cwd(&mut ftp, &cfg.remote_dir, &cfg.username)?;

    let file = File::open(local_path).map_err(|e| format!("Cannot open file: {e}"))?;
    let mut reader = BufReader::new(file);
    ftp.put_file(filename, &mut reader)
        .map_err(|e| format!("FTP upload failed: {e}"))?;
    let _ = ftp.quit();

    let remote_path = build_remote_path(&cfg.remote_dir, filename);
    let public_url = build_public_url(&cfg.public_url_prefix, filename);
    Ok(FtpUploadResult {
        remote_path,
        public_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_public_url() {
        assert_eq!(build_public_url("", "a.png"), None);
        assert_eq!(
            build_public_url("https://cdn.example.com/shots/", "a.png"),
            Some("https://cdn.example.com/shots/a.png".to_string())
        );
        assert_eq!(
            build_public_url("https://cdn.example.com/shots", "a.png"),
            Some("https://cdn.example.com/shots/a.png".to_string())
        );
    }

    #[test]
    fn test_build_remote_path() {
        assert_eq!(build_remote_path("/", "shot.png"), "/shot.png");
        assert_eq!(
            build_remote_path("/public_html/shots/", "shot.png"),
            "/public_html/shots/shot.png"
        );
        assert_eq!(build_remote_path("uploads", "v.mp4"), "/uploads/v.mp4");
    }
}
