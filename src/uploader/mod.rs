use crate::storage::{FtpConfig, UploadProtocol};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use ssh2::Session;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
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

fn connect_tcp_with_timeout(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, String> {
    let addr = format!("{host}:{port}");
    let sock_addr = addr
        .to_socket_addrs()
        .map_err(|e| format!("Cannot resolve {addr}: {e}"))?
        .next()
        .ok_or_else(|| format!("No IP address found for {addr}"))?;
    let stream = TcpStream::connect_timeout(&sock_addr, timeout)
        .map_err(|e| format!("Connect to {addr} failed: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    Ok(stream)
}

fn connect_and_login_ftp(cfg: &FtpConfig) -> Result<RustlsFtpStream, String> {
    let host = cfg.host.trim();
    if host.is_empty() {
        return Err("Host is empty".to_string());
    }
    let port = if cfg.port == 0 { 21 } else { cfg.port };
    let tcp = connect_tcp_with_timeout(host, port, Duration::from_secs(10))?;
    let mut ftp = RustlsFtpStream::connect_with_stream(tcp)
        .map_err(|e| format!("FTP connect to {host}:{port} failed: {e}"))?;
    ftp.set_passive_nat_workaround(true);

    if matches!(cfg.protocol, UploadProtocol::FtpsExplicit) {
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

    if ftp.cwd(trimmed).is_ok() {
        return Ok(());
    }

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

fn connect_sftp(cfg: &FtpConfig) -> Result<(Session, ssh2::Sftp), String> {
    let host = cfg.host.trim();
    if host.is_empty() {
        return Err("Host is empty".to_string());
    }
    let port = if cfg.port == 0 { 22 } else { cfg.port };
    let tcp = connect_tcp_with_timeout(host, port, Duration::from_secs(10))?;

    let mut sess = Session::new().map_err(|e| format!("SSH session init failed: {e}"))?;
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| format!("SSH handshake with {host}:{port} failed: {e}"))?;

    let user = cfg.username.trim();
    if user.is_empty() {
        return Err("SFTP username is empty".to_string());
    }
    sess.userauth_password(user, &cfg.password)
        .map_err(|e| format!("SFTP authentication failed for '{user}': {e}"))?;

    if !sess.authenticated() {
        return Err("SFTP authentication was not accepted".to_string());
    }
    let sftp = sess
        .sftp()
        .map_err(|e| format!("SFTP subsystem init failed: {e}"))?;
    Ok((sess, sftp))
}

fn ensure_sftp_dir(sftp: &ssh2::Sftp, remote_dir: &str) -> Result<String, String> {
    let trimmed = remote_dir.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return Ok("/".to_string());
    }

    if sftp.stat(Path::new(trimmed)).is_ok() {
        return Ok(trimmed.to_string());
    }

    let mut current = if trimmed.starts_with('/') {
        String::new()
    } else {
        ".".to_string()
    };

    for seg in trimmed.split('/').filter(|s| !s.is_empty()) {
        if current.is_empty() {
            current = format!("/{seg}");
        } else if current == "." {
            current = seg.to_string();
        } else {
            current = format!("{current}/{seg}");
        }
        let p = Path::new(&current);
        if sftp.stat(p).is_err() {
            let _ = sftp.mkdir(p, 0o755);
        }
    }

    if sftp.stat(Path::new(trimmed)).is_err() {
        return Err(format!("SFTP remote directory unavailable: {trimmed}"));
    }
    Ok(trimmed.to_string())
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
    match cfg.protocol {
        UploadProtocol::Sftp => {
            let (_sess, sftp) = connect_sftp(cfg)?;
            let dir = ensure_sftp_dir(&sftp, &cfg.remote_dir)?;
            Ok(format!("Connected (SFTP)! Dir: {dir}"))
        }
        UploadProtocol::Ftp | UploadProtocol::FtpsExplicit => {
            let mut ftp = connect_and_login_ftp(cfg)?;
            ensure_and_cwd(&mut ftp, &cfg.remote_dir, &cfg.username)?;
            let pwd = ftp.pwd().unwrap_or_else(|_| cfg.remote_dir.clone());
            let _ = ftp.quit();
            let proto = if cfg.protocol == UploadProtocol::FtpsExplicit {
                "FTPS"
            } else {
                "FTP"
            };
            Ok(format!("Connected ({proto})! Dir: {pwd}"))
        }
    }
}

pub fn upload_file_to_ftp(local_path: &Path, cfg: &FtpConfig) -> Result<FtpUploadResult, String> {
    if !local_path.exists() {
        return Err(format!("Local file not found: {}", local_path.display()));
    }
    let filename = local_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid local filename".to_string())?;

    let remote_path = build_remote_path(&cfg.remote_dir, filename);
    let public_url = build_public_url(&cfg.public_url_prefix, filename);

    match cfg.protocol {
        UploadProtocol::Sftp => {
            let (_sess, sftp) = connect_sftp(cfg)?;
            let dir = ensure_sftp_dir(&sftp, &cfg.remote_dir)?;
            let full_remote = if dir == "/" {
                format!("/{filename}")
            } else {
                format!("{dir}/{filename}")
            };

            let mut local_file =
                File::open(local_path).map_err(|e| format!("Cannot open file: {e}"))?;
            let mut remote_file = sftp
                .create(Path::new(&full_remote))
                .map_err(|e| format!("SFTP create '{full_remote}' failed: {e}"))?;
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = local_file
                    .read(&mut buf)
                    .map_err(|e| format!("Read error: {e}"))?;
                if n == 0 {
                    break;
                }
                remote_file
                    .write_all(&buf[..n])
                    .map_err(|e| format!("SFTP write failed: {e}"))?;
            }
            Ok(FtpUploadResult {
                remote_path: full_remote,
                public_url,
            })
        }
        UploadProtocol::Ftp | UploadProtocol::FtpsExplicit => {
            let mut ftp = connect_and_login_ftp(cfg)?;
            ensure_and_cwd(&mut ftp, &cfg.remote_dir, &cfg.username)?;

            let file = File::open(local_path).map_err(|e| format!("Cannot open file: {e}"))?;
            let mut reader = BufReader::new(file);
            ftp.put_file(filename, &mut reader)
                .map_err(|e| format!("FTP upload failed: {e}"))?;
            let _ = ftp.quit();

            Ok(FtpUploadResult {
                remote_path,
                public_url,
            })
        }
    }
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
