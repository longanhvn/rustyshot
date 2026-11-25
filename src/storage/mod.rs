use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImageFormatChoice {
    #[default]
    Png,
    Jpeg,
}

impl ImageFormatChoice {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG (Lossless)",
            Self::Jpeg => "JPEG (Compact)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VideoFormatChoice {
    #[default]
    Mp4,
    Gif,
}

impl VideoFormatChoice {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Gif => "gif",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4 (H.264 Built-in)",
            Self::Gif => "GIF (Built-in)",
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UploadProtocol {
    Ftp,
    #[default]
    FtpsExplicit,
    Sftp,
}

impl UploadProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ftp => "FTP (Port 21)",
            Self::FtpsExplicit => "FTPS Explicit TLS (Port 21)",
            Self::Sftp => "SFTP / SSH (Port 22)",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FtpConfig {
    pub enabled: bool,
    pub auto_upload: bool,
    pub protocol: UploadProtocol,
    #[serde(default = "default_true")]
    pub ftps_explicit: bool,
    #[serde(default = "default_true")]
    pub ignore_tls_errors: bool,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub remote_dir: String,
    pub public_url_prefix: String,
}

impl Default for FtpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_upload: false,
            protocol: UploadProtocol::Sftp,
            ftps_explicit: true,
            ignore_tls_errors: true,
            host: String::new(),
            port: 22,
            username: "anonymous".to_string(),
            password: String::new(),
            remote_dir: "/".to_string(),
            public_url_prefix: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureRecord {
    pub id: String,
    pub timestamp: String,
    pub width: u32,
    pub height: u32,
    pub file_path: Option<PathBuf>,
    pub mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RustyShotConfig {
    pub save_dir: PathBuf,
    pub format: ImageFormatChoice,
    pub jpeg_quality: u8,
    pub auto_copy_clipboard: bool,
    pub auto_save_file: bool,
    pub after_capture_open_editor: bool,
    pub after_capture_auto_pin: bool,
    pub hide_window_on_capture: bool,
    pub minimize_to_tray: bool,
    pub region_hotkey: String,
    pub fullscreen_hotkey: String,
    pub record_hotkey: String,
    pub default_color: [u8; 4],
    pub default_stroke_width: f32,
    pub video_format: VideoFormatChoice,
    pub video_fps: u32,
    pub ftp: FtpConfig,
    pub picked_colors: Vec<String>,
    pub recent_captures: Vec<CaptureRecord>,
}

pub fn portable_data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let data_dir = exe_dir.join("data");
        if data_dir.exists() || fs::create_dir_all(&data_dir).is_ok() {
            return data_dir;
        }
    }
    PathBuf::from("data")
}

impl Default for RustyShotConfig {
    fn default() -> Self {
        Self {
            save_dir: portable_data_dir(),
            format: ImageFormatChoice::Png,
            jpeg_quality: 92,
            auto_copy_clipboard: true,
            auto_save_file: true,
            after_capture_open_editor: false,
            after_capture_auto_pin: false,
            hide_window_on_capture: true,
            minimize_to_tray: true,
            region_hotkey: "Ctrl+Shift+S".to_string(),
            fullscreen_hotkey: "Ctrl+Shift+F".to_string(),
            record_hotkey: "Ctrl+Shift+R".to_string(),
            default_color: [239, 68, 68, 255],
            default_stroke_width: 3.0,
            video_format: VideoFormatChoice::Mp4,
            video_fps: 15,
            ftp: FtpConfig::default(),
            picked_colors: Vec::new(),
            recent_captures: Vec::new(),
        }
    }
}

pub struct StorageManager {
    config_path: PathBuf,
}

impl StorageManager {
    pub fn new() -> Self {
        let config_path = Self::resolve_config_path();
        Self { config_path }
    }

    fn resolve_config_path() -> PathBuf {
        let data_dir = portable_data_dir();
        let _ = fs::create_dir_all(&data_dir);
        data_dir.join("RustyShot.json")
    }

    #[allow(dead_code)]
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn load_config(&self) -> RustyShotConfig {
        let mut cfg: RustyShotConfig = match fs::read_to_string(&self.config_path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => RustyShotConfig::default(),
        };
        // Migrate legacy external Pictures/RustyShot path to portable ./data
        if let Some(pic) = dirs::picture_dir()
            && cfg.save_dir == pic.join("RustyShot")
        {
            cfg.save_dir = portable_data_dir();
            let _ = self.save_config(&cfg);
        }
        let _ = fs::create_dir_all(cfg.save_dir.join("image"));
        let _ = fs::create_dir_all(cfg.save_dir.join("video"));
        cfg
    }

    pub fn save_config(&self, cfg: &RustyShotConfig) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(cfg)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;
        fs::write(&self.config_path, json).map_err(|e| format!("Failed to write config: {e}"))
    }

    pub fn build_output_path(save_dir: &Path, fmt: ImageFormatChoice) -> Result<PathBuf, String> {
        let img_dir = save_dir.join("image");
        Self::build_named_output_path(&img_dir, "RustyShot", fmt.extension())
    }

    pub fn build_video_output_path(
        save_dir: &Path,
        fmt: VideoFormatChoice,
    ) -> Result<PathBuf, String> {
        let vid_dir = save_dir.join("video");
        Self::build_named_output_path(&vid_dir, "RustyShot_Rec", fmt.extension())
    }

    fn build_named_output_path(save_dir: &Path, prefix: &str, ext: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(save_dir)
            .map_err(|e| format!("Failed to create screenshots directory: {e}"))?;
        let stamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let mut candidate = save_dir.join(format!("{prefix}_{stamp}.{ext}"));
        let mut idx = 1u32;
        while candidate.exists() {
            candidate = save_dir.join(format!("{prefix}_{stamp}_{idx}.{ext}"));
            idx += 1;
        }
        Ok(candidate)
    }
}

impl Default for StorageManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let cfg = RustyShotConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: RustyShotConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.format, ImageFormatChoice::Png);
        assert_eq!(parsed.jpeg_quality, 92);
        assert!(parsed.auto_copy_clipboard);
    }

    #[test]
    fn test_build_output_path_unique() {
        let dir = std::env::temp_dir().join("rustyshot_test_out");
        let _ = fs::remove_dir_all(&dir);
        let p1 = StorageManager::build_output_path(&dir, ImageFormatChoice::Png).unwrap();
        fs::write(&p1, b"x").unwrap();
        let p2 = StorageManager::build_output_path(&dir, ImageFormatChoice::Png).unwrap();
        assert_ne!(p1, p2);
        let _ = fs::remove_dir_all(&dir);
    }
}
