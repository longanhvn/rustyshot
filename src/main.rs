#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod editor;
mod recorder;
mod storage;
mod tray;
mod uploader;

use app::RustyShotApp;
use eframe::egui;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tray::generate_app_icon_rgba;

const SINGLE_INSTANCE_ADDR: &str = "127.0.0.1:47831";

fn main() -> eframe::Result<()> {
    let show_signal = Arc::new(AtomicBool::new(false));

    // Enforce single instance: if another RustyShot is running, tell it to show and exit.
    let listener = match TcpListener::bind(SINGLE_INSTANCE_ADDR) {
        Ok(l) => Some(l),
        Err(_) => {
            if let Ok(mut stream) = TcpStream::connect(SINGLE_INSTANCE_ADDR) {
                let _ = stream.write_all(b"SHOW\n");
                let _ = stream.flush();
            }
            return Ok(());
        }
    };

    if let Some(listener) = listener {
        let sig = Arc::clone(&show_signal);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut s = stream;
                let mut buf = [0u8; 32];
                let _ = s.read(&mut buf);
                sig.store(true, Ordering::Relaxed);
            }
        });
    }

    let (rgba, w, h) = generate_app_icon_rgba();
    let app_icon = egui::IconData { rgba, width: w, height: h };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("rustyshot")
            .with_title("RustyShot v0.3.11 - Cross-Platform Screenshot, Recorder & Annotation")
            .with_inner_size([860.0, 560.0])
            .with_min_inner_size([720.0, 460.0])
            .with_icon(app_icon),
        ..Default::default()
    };

    eframe::run_native(
        "RustyShot",
        native_options,
        Box::new(move |cc| Ok(Box::new(RustyShotApp::new(cc, show_signal)))),
    )
}
