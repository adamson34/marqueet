//! Facts about the device for the first-boot screen, and the password-reset
//! file.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::Path;

use crate::settings_store::{SettingsStore, StoreError};

/// The device's host name, lowercased ("marqueet").
pub fn hostname() -> Option<String> {
    let name = gethostname::gethostname().into_string().ok()?;
    let name = name.trim().trim_end_matches(".local").to_lowercase();
    (!name.is_empty() && name != "localhost").then_some(name)
}

/// The address other devices on the network reach this one at: the source
/// address the OS would use to reach the internet. Nothing is sent.
pub fn lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// Where the setup page can be reached from another device, best first
/// (`http://marqueet.local:7878/setup`, then by IP). Empty when the server
/// only listens on loopback: then nothing else can reach it anyway.
pub fn setup_urls(listen: SocketAddr, hostname: Option<&str>, lan_ip: Option<IpAddr>) -> Vec<String> {
    if listen.ip().is_loopback() {
        return Vec::new();
    }
    let port = listen.port();
    let host = |h: &str| if port == 80 { format!("http://{h}/setup") } else { format!("http://{h}:{port}/setup") };
    let ip = if listen.ip().is_unspecified() { lan_ip } else { Some(listen.ip()) };
    let mut urls: Vec<String> = hostname.map(|h| host(&format!("{h}.local"))).into_iter().collect();
    urls.extend(ip.map(|ip| match ip {
        IpAddr::V6(v6) => host(&format!("[{v6}]")),
        IpAddr::V4(v4) => host(&v4.to_string()),
    }));
    urls
}

/// If the reset file exists and is new, clears the admin password so the
/// device goes back to first-boot setup. Returns true if it did. The file is
/// removed when possible; either way the same file is only acted on once.
pub fn apply_reset_file(path: &Path, db: &SettingsStore) -> Result<bool, StoreError> {
    let Ok(meta) = std::fs::metadata(path) else { return Ok(false) };
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    let marker = format!("{}:{}", modified.map_or(0, |d| d.as_secs()), meta.len());
    if db.reset_marker()?.as_deref() == Some(marker.as_str()) {
        return Ok(false);
    }
    db.set_admin_password_hash(None)?;
    db.set_reset_marker(&marker)?;
    match std::fs::remove_file(path) {
        Ok(()) => log::warn!("admin password reset by {}; the file was removed", path.display()),
        Err(e) => log::warn!("admin password reset by {} (couldn't remove it: {e})", path.display()),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_urls_prefer_the_name() {
        let any: SocketAddr = "0.0.0.0:7878".parse().unwrap();
        let ip: IpAddr = "192.168.1.20".parse().unwrap();
        assert_eq!(
            setup_urls(any, Some("marqueet"), Some(ip)),
            ["http://marqueet.local:7878/setup", "http://192.168.1.20:7878/setup"]
        );
        assert_eq!(setup_urls("10.0.0.5:80".parse().unwrap(), None, Some(ip)), ["http://10.0.0.5/setup"]);
        assert!(setup_urls("127.0.0.1:7878".parse().unwrap(), Some("marqueet"), Some(ip)).is_empty());
    }

    #[test]
    fn reset_file_clears_the_password_once() {
        let dir = std::env::temp_dir().join(format!("marqueet-reset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("reset-password.txt");
        let db = SettingsStore::in_memory().unwrap();
        db.set_admin_password_hash(Some("hash")).unwrap();
        assert!(!apply_reset_file(&file, &db).unwrap(), "no file");
        std::fs::write(&file, "reset").unwrap();
        assert!(apply_reset_file(&file, &db).unwrap());
        assert_eq!(db.admin_password_hash().unwrap(), None);
        assert!(!file.exists(), "removed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
