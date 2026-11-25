use crate::capture::{
    WindowSummary, capture_all_monitors_composite, capture_window_by_id, copy_image_to_clipboard,
    copy_text_to_clipboard, crop_rgba_image, flip_horizontal, flip_vertical,
    list_capturable_windows, paste_image_from_clipboard, rotate_90_ccw, rotate_90_cw,
    save_image_to_file,
};
use crate::editor::{
    AnnotationHistory, AnnotationItem, PALETTE_COLORS, ToolKind, paint_annotations_egui,
    render_annotations_to_image,
};
use crate::recorder::{RecordingHandle, detect_ffmpeg_path, start_recording};
use crate::storage::{
    CaptureRecord, ImageFormatChoice, RustyShotConfig, StorageManager, VideoFormatChoice,
};
use crate::tray::{
    HotkeyController, TrayAction, TrayController, poll_background_tray_and_hotkeys,
};
use crate::uploader::{test_ftp_connection, upload_file_to_ftp};
use chrono::Local;
use eframe::egui;
use egui::{
    Align, Align2, Color32, ColorImage, CursorIcon, FontId, Key, Layout, Pos2, Rect, RichText,
    Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2, ViewportBuilder, ViewportCommand,
    ViewportId, pos2, vec2,
};
use image::RgbaImage;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
fn extract_win32_hwnd(cc: &eframe::CreationContext<'_>) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = cc.window_handle()
        && let RawWindowHandle::Win32(h) = handle.as_raw()
    {
        return h.hwnd.get();
    }
    0
}

#[cfg(not(target_os = "windows"))]
fn extract_win32_hwnd(_cc: &eframe::CreationContext<'_>) -> isize {
    0
}

#[cfg(target_os = "windows")]
fn win32_show_window(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe extern "system" {
        fn ShowWindow(hwnd: isize, n_cmd_show: i32) -> i32;
        fn SetForegroundWindow(hwnd: isize) -> i32;
    }
    const SW_SHOW: i32 = 5;
    const SW_RESTORE: i32 = 9;
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}

#[cfg(not(target_os = "windows"))]
fn win32_show_window(_hwnd: isize) {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppMode {
    Dashboard,
    RegionOverlay,
    Editor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DashboardTab {
    Capture,
    Tools,
    Upload,
    History,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResizeHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragAction {
    None,
    CreatingSelection,
    MovingSelection,
    ResizingSelection(ResizeHandle),
    DrawingAnnotation,
}

struct PinnedShot {
    id: ViewportId,
    image: RgbaImage,
    texture: Option<TextureHandle>,
    close_flag: Arc<AtomicBool>,
    zoom_bits: Arc<std::sync::atomic::AtomicU32>,
    opacity_bits: Arc<std::sync::atomic::AtomicU32>,
}

pub struct RustyShotApp {
    storage: StorageManager,
    config: RustyShotConfig,
    tray: Option<TrayController>,
    hotkeys: Option<HotkeyController>,
    hotkey_status: String,

    mode: AppMode,
    active_tab: DashboardTab,
    history_search: String,
    status_toast: Option<(String, Instant)>,
    show_window_picker: bool,
    window_list: Vec<WindowSummary>,
    overlay_color_picker_only: bool,

    // Pending delayed capture (so main window hides cleanly before grabbing screen)
    pending_capture: Option<(TrayAction, Instant)>,
    capture_delay_secs: u64,
    force_quit: bool,
    pending_actions: Arc<Mutex<VecDeque<TrayAction>>>,

    // Active image & texture (used in RegionOverlay & Editor)
    working_image: Option<RgbaImage>,
    working_texture: Option<TextureHandle>,
    working_origin: (i32, i32),
    working_scale: f32,
    loupe_texture: Option<TextureHandle>,

    // Pinned floating screenshots
    pinned_shots: Vec<PinnedShot>,
    next_pin_id: u64,

    // Screen recorder & FTP background tasks
    active_recording: Option<RecordingHandle>,
    ffmpeg_available: bool,
    upload_rx: Option<mpsc::Receiver<Result<String, String>>>,
    ftp_test_status: Option<String>,

    // Region selection rect in IMAGE pixel coordinates
    selection_rect: Option<Rect>,
    drag_action: DragAction,
    drag_anchor: Pos2,
    drag_orig_rect: Option<Rect>,

    // Annotations
    history: AnnotationHistory,
    active_tool: ToolKind,
    active_color: Color32,
    stroke_width: f32,
    next_step_number: u32,
    in_progress_item: Option<AnnotationItem>,

    // Inline text input popup
    pending_text_pos: Option<[f32; 2]>,
    pending_text_buf: String,

    // Editor zoom & crop
    editor_zoom: f32,
    editor_crop_rect: Option<Rect>,
    editor_crop_anchor: Option<Pos2>,

    // Thumbnail cache for Dashboard recent captures
    thumb_cache: HashMap<String, TextureHandle>,
}

impl RustyShotApp {
    pub fn new(cc: &eframe::CreationContext<'_>, show_signal: Arc<AtomicBool>) -> Self {
        Self::apply_dark_theme(&cc.egui_ctx);
        let storage = StorageManager::new();
        let config = storage.load_config();

        let tray = TrayController::try_new().ok();
        let (hotkeys, hotkey_status) = match HotkeyController::try_new(
            &config.region_hotkey,
            &config.fullscreen_hotkey,
            &config.record_hotkey,
        ) {
            Ok((hk, st)) => (Some(hk), st),
            Err(e) => (None, e),
        };

        let active_color = Color32::from_rgba_unmultiplied(
            config.default_color[0],
            config.default_color[1],
            config.default_color[2],
            config.default_color[3],
        );
        let stroke_width = config.default_stroke_width;

        let hwnd = extract_win32_hwnd(cc);
        let pending_actions = Arc::new(Mutex::new(VecDeque::new()));
        let tray_ids = tray.as_ref().map(|t| t.id_map());
        let hk_ids = hotkeys.as_ref().map(|h| h.shared_ids());
        let pending_bg = Arc::clone(&pending_actions);
        let ctx_wake = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(50));
                let mut woke = false;
                if show_signal.swap(false, Ordering::Relaxed) {
                    if let Ok(mut q) = pending_bg.lock() {
                        q.push_back(TrayAction::ShowApp);
                    }
                    win32_show_window(hwnd);
                    woke = true;
                }
                let evs = poll_background_tray_and_hotkeys(tray_ids.as_ref(), hk_ids.as_ref());
                if !evs.is_empty() {
                    let needs_wake = evs.iter().any(|a| {
                        matches!(
                            a,
                            TrayAction::ShowApp
                                | TrayAction::CaptureWindow
                                | TrayAction::CaptureRegion
                                | TrayAction::ColorPicker
                                | TrayAction::CaptureFullscreen
                                | TrayAction::Quit
                        )
                    });
                    if let Ok(mut q) = pending_bg.lock() {
                        for a in evs {
                            q.push_back(a);
                        }
                    }
                    if needs_wake {
                        win32_show_window(hwnd);
                    }
                    woke = true;
                }
                if woke {
                    ctx_wake.request_repaint();
                }
            }
        });

        Self {
            storage,
            config,
            tray,
            hotkeys,
            hotkey_status,
            mode: AppMode::Dashboard,
            active_tab: DashboardTab::Capture,
            history_search: String::new(),
            status_toast: None,
            show_window_picker: false,
            window_list: Vec::new(),
            overlay_color_picker_only: false,
            pending_capture: None,
            capture_delay_secs: 0,
            force_quit: false,
            pending_actions,
            working_image: None,
            working_texture: None,
            working_origin: (0, 0),
            working_scale: 1.0,
            loupe_texture: None,
            pinned_shots: Vec::new(),
            next_pin_id: 1,
            active_recording: None,
            ffmpeg_available: detect_ffmpeg_path().is_some(),
            upload_rx: None,
            ftp_test_status: None,
            selection_rect: None,
            drag_action: DragAction::None,
            drag_anchor: Pos2::ZERO,
            drag_orig_rect: None,
            history: AnnotationHistory::new(),
            active_tool: ToolKind::Arrow,
            active_color,
            stroke_width,
            next_step_number: 1,
            in_progress_item: None,
            pending_text_pos: None,
            pending_text_buf: String::new(),
            editor_zoom: 1.0,
            editor_crop_rect: None,
            editor_crop_anchor: None,
            thumb_cache: HashMap::new(),
        }
    }

    fn apply_dark_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.window_fill = Color32::from_rgb(15, 18, 26);
        visuals.panel_fill = Color32::from_rgb(15, 18, 26);
        visuals.extreme_bg_color = Color32::from_rgb(11, 14, 20);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(22, 27, 38);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(28, 35, 48);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(71, 85, 105));
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 48, 66);
        visuals.widgets.active.bg_fill = Color32::from_rgb(56, 189, 248);
        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(56, 189, 248, 80);
        ctx.set_visuals(visuals);
    }

    fn set_toast(&mut self, msg: impl Into<String>) {
        self.status_toast = Some((msg.into(), Instant::now()));
    }

    fn rgba_to_texture(ctx: &egui::Context, name: &str, img: &RgbaImage) -> TextureHandle {
        let size = [img.width() as usize, img.height() as usize];
        let color_img = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        ctx.load_texture(name, color_img, TextureOptions::LINEAR)
    }

    fn trigger_capture(&mut self, ctx: &egui::Context, action: TrayAction) {
        match action {
            TrayAction::CaptureRegion | TrayAction::CaptureFullscreen | TrayAction::ColorPicker => {
                self.overlay_color_picker_only = action == TrayAction::ColorPicker;
                let extra_ms = self.capture_delay_secs * 1000;
                if self.config.hide_window_on_capture && self.mode == AppMode::Dashboard {
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                    self.pending_capture = Some((
                        action,
                        Instant::now() + Duration::from_millis(180 + extra_ms),
                    ));
                } else if extra_ms > 0 {
                    self.pending_capture =
                        Some((action, Instant::now() + Duration::from_millis(extra_ms)));
                } else {
                    self.execute_capture_now(ctx, action);
                }
            }
            TrayAction::ToggleRecord => {
                if self.active_recording.is_some() {
                    self.stop_screen_recording();
                } else {
                    self.start_screen_recording(ctx, None);
                }
            }
            TrayAction::CaptureWindow => {
                self.window_list = list_capturable_windows().unwrap_or_default();
                self.show_window_picker = true;
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
            TrayAction::OpenFolder => {
                let _ = std::fs::create_dir_all(&self.config.save_dir);
                open_folder_in_os(&self.config.save_dir);
            }
            TrayAction::ShowApp => {
                self.exit_overlay_to_dashboard(ctx);
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
            TrayAction::Quit => {
                self.force_quit = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn execute_capture_now(&mut self, ctx: &egui::Context, action: TrayAction) {
        match action {
            TrayAction::CaptureRegion | TrayAction::ColorPicker => {
                self.overlay_color_picker_only = action == TrayAction::ColorPicker;
                self.window_list = list_capturable_windows().unwrap_or_default();
                match capture_all_monitors_composite() {
                    Ok(cap) => {
                        let tex = Self::rgba_to_texture(ctx, "rustyshot_overlay", &cap.image);
                        self.working_origin = (cap.origin_x, cap.origin_y);
                        self.working_image = Some(cap.image);
                        self.working_texture = Some(tex);
                        self.working_scale = cap.scale_factor.max(1.0);
                        self.selection_rect = None;
                        self.history.clear();
                        self.next_step_number = 1;
                        self.in_progress_item = None;
                        self.pending_text_pos = None;
                        self.mode = AppMode::RegionOverlay;
                        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(ViewportCommand::Decorations(false));
                        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
                        ctx.send_viewport_cmd(ViewportCommand::Focus);
                    }
                    Err(e) => {
                        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                        self.set_toast(format!("Capture failed: {e}"));
                    }
                }
            }
            TrayAction::CaptureFullscreen => match capture_all_monitors_composite() {
                Ok(cap) => {
                    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    let open_ed = self.config.after_capture_open_editor;
                    self.finalize_captured_image(ctx, cap.image, "Fullscreen", open_ed);
                }
                Err(e) => {
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    self.set_toast(format!("Fullscreen capture failed: {e}"));
                }
            },
            _ => {}
        }
    }

    fn pin_image(&mut self, img: RgbaImage) {
        let id = ViewportId::from_hash_of(format!("rustyshot_pin_{}", self.next_pin_id));
        self.next_pin_id += 1;
        self.pinned_shots.push(PinnedShot {
            id,
            image: img,
            texture: None,
            close_flag: Arc::new(AtomicBool::new(false)),
            zoom_bits: Arc::new(std::sync::atomic::AtomicU32::new(1.0f32.to_bits())),
            opacity_bits: Arc::new(std::sync::atomic::AtomicU32::new(1.0f32.to_bits())),
        });
        self.set_toast("Pinned to screen (scroll to zoom)");
    }

    fn render_pinned_viewports(&mut self, ctx: &egui::Context) {
        self.pinned_shots
            .retain(|p| !p.close_flag.load(Ordering::Relaxed));

        for pin in &mut self.pinned_shots {
            if pin.texture.is_none() {
                let size = [pin.image.width() as usize, pin.image.height() as usize];
                let cimg = ColorImage::from_rgba_unmultiplied(size, pin.image.as_raw());
                pin.texture = Some(ctx.load_texture(
                    format!("pin_tex_{:?}", pin.id),
                    cimg,
                    TextureOptions::LINEAR,
                ));
            }
            let Some(tex) = pin.texture.clone() else {
                continue;
            };
            let img_copy = pin.image.clone();
            let close_flag = Arc::clone(&pin.close_flag);
            let zoom_bits = Arc::clone(&pin.zoom_bits);
            let opacity_bits = Arc::clone(&pin.opacity_bits);
            let cur_zoom = f32::from_bits(zoom_bits.load(Ordering::Relaxed)).clamp(0.3, 3.0);
            let base_w = (pin.image.width() as f32).clamp(120.0, 960.0);
            let base_h = (pin.image.height() as f32).clamp(80.0, 720.0);
            let w = (base_w * cur_zoom).clamp(100.0, 1600.0);
            let h = (base_h * cur_zoom).clamp(70.0, 1100.0);

            ctx.show_viewport_deferred(
                pin.id,
                ViewportBuilder::default()
                    .with_title("RustyShot Pin")
                    .with_decorations(false)
                    .with_always_on_top()
                    .with_inner_size([w, h]),
                move |pctx, _class| {
                    if pctx.input(|i| i.viewport().close_requested() || i.key_pressed(Key::Escape)) {
                        close_flag.store(true, Ordering::Relaxed);
                        pctx.send_viewport_cmd(ViewportCommand::Close);
                        return;
                    }

                    // Scroll wheel zooms pinned window
                    let scroll_y = pctx.input(|i| i.raw_scroll_delta.y);
                    if scroll_y.abs() > 0.1 {
                        let old_z = f32::from_bits(zoom_bits.load(Ordering::Relaxed));
                        let new_z = (old_z + scroll_y * 0.002).clamp(0.3, 3.0);
                        zoom_bits.store(new_z.to_bits(), Ordering::Relaxed);
                        let nw = (base_w * new_z).clamp(100.0, 1600.0);
                        let nh = (base_h * new_z).clamp(70.0, 1100.0);
                        pctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(nw, nh)));
                    }

                    let opacity = f32::from_bits(opacity_bits.load(Ordering::Relaxed)).clamp(0.25, 1.0);
                    let tint = Color32::from_white_alpha((opacity * 255.0) as u8);

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().stroke(Stroke::new(2.0, Color32::from_rgb(56, 189, 248))))
                        .show(pctx, |ui| {
                            let rect = ui.max_rect();
                            let resp = ui.allocate_rect(rect, Sense::click_and_drag());
                            if resp.drag_started() {
                                pctx.send_viewport_cmd(ViewportCommand::StartDrag);
                            }
                            ui.painter().image(
                                tex.id(),
                                rect,
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                tint,
                            );

                            if ui.rect_contains_pointer(rect) {
                                egui::Area::new(egui::Id::new("pin_ctrl_bar"))
                                    .anchor(Align2::RIGHT_TOP, vec2(-6.0, 6.0))
                                    .order(egui::Order::Foreground)
                                    .show(pctx, |ui| {
                                        egui::Frame::none()
                                            .fill(Color32::from_black_alpha(190))
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::same(3.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    let op_label = format!("{}%", (opacity * 100.0).round() as u32);
                                                    if ui.small_button(op_label).on_hover_text("Cycle opacity").clicked() {
                                                        let next: f32 = if opacity > 0.85 {
                                                            0.7
                                                        } else if opacity > 0.55 {
                                                            0.45
                                                        } else {
                                                            1.0
                                                        };
                                                        opacity_bits.store(next.to_bits(), Ordering::Relaxed);
                                                    }
                                                    if ui.small_button("📋").on_hover_text("Copy").clicked() {
                                                        let _ = copy_image_to_clipboard(&img_copy);
                                                    }
                                                    if ui.small_button("✖").on_hover_text("Unpin (Esc)").clicked() {
                                                        close_flag.store(true, Ordering::Relaxed);
                                                        pctx.send_viewport_cmd(ViewportCommand::Close);
                                                    }
                                                });
                                            });
                                    });
                            }
                        });
                },
            );
        }
    }

    fn exit_overlay_to_dashboard(&mut self, ctx: &egui::Context) {
        if self.mode == AppMode::RegionOverlay {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
            ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
        }
        self.mode = AppMode::Dashboard;
        self.selection_rect = None;
        self.in_progress_item = None;
        self.pending_text_pos = None;
    }

    fn open_in_editor(&mut self, ctx: &egui::Context, img: RgbaImage) {
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        let tex = Self::rgba_to_texture(ctx, "rustyshot_editor", &img);
        self.working_image = Some(img);
        self.working_texture = Some(tex);
        self.history.clear();
        self.next_step_number = 1;
        self.editor_zoom = 1.0;
        self.mode = AppMode::Editor;
    }

    fn finalize_captured_image(
        &mut self,
        ctx: &egui::Context,
        img: RgbaImage,
        mode_label: &str,
        open_editor_after: bool,
    ) {
        let mut saved_path: Option<PathBuf> = None;
        let mut notes = Vec::new();

        if self.config.auto_copy_clipboard {
            match copy_image_to_clipboard(&img) {
                Ok(()) => notes.push("Copied to clipboard".to_string()),
                Err(e) => notes.push(format!("Clipboard error: {e}")),
            }
        }
        if self.config.auto_save_file {
            match StorageManager::build_output_path(&self.config.save_dir, self.config.format)
                .and_then(|p| {
                    save_image_to_file(&img, &p, self.config.format, self.config.jpeg_quality)?;
                    Ok(p)
                }) {
                Ok(p) => {
                    notes.push(format!("Saved {}", p.file_name().unwrap_or_default().to_string_lossy()));
                    saved_path = Some(p);
                }
                Err(e) => notes.push(format!("Save error: {e}")),
            }
        }

        let record = CaptureRecord {
            id: Local::now().format("%Y%m%d%H%M%S%3f").to_string(),
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            width: img.width(),
            height: img.height(),
            file_path: saved_path.clone(),
            mode: mode_label.to_string(),
        };
        self.config.recent_captures.insert(0, record);
        self.config.recent_captures.truncate(24);
        let _ = self.storage.save_config(&self.config);

        if self.config.ftp.enabled
            && self.config.ftp.auto_upload
            && let Some(ref p) = saved_path
        {
            self.spawn_ftp_upload(p.clone());
        }

        if self.config.after_capture_auto_pin {
            self.pin_image(img.clone());
        }

        if open_editor_after {
            self.open_in_editor(ctx, img);
        } else {
            self.exit_overlay_to_dashboard(ctx);
        }

        if !notes.is_empty() {
            self.set_toast(notes.join(" · "));
        }
    }

    fn start_screen_recording(&mut self, ctx: &egui::Context, region: Option<[u32; 4]>) {
        if self.active_recording.is_some() {
            self.set_toast("Recording already in progress");
            return;
        }
        let fmt = self.config.video_format;
        let out_path = match StorageManager::build_video_output_path(&self.config.save_dir, fmt) {
            Ok(p) => p,
            Err(e) => {
                self.set_toast(e);
                return;
            }
        };
        if self.mode == AppMode::RegionOverlay {
            self.exit_overlay_to_dashboard(ctx);
        }
        match start_recording(out_path, fmt, self.config.video_fps, region) {
            Ok(handle) => {
                self.active_recording = Some(handle);
                self.set_toast(format!("Recording started ({})", fmt.extension().to_uppercase()));
            }
            Err(e) => self.set_toast(e),
        }
    }

    fn stop_screen_recording(&mut self) {
        let Some(handle) = self.active_recording.take() else {
            return;
        };
        match handle.stop() {
            Ok(res) => {
                let record = CaptureRecord {
                    id: Local::now().format("%Y%m%d%H%M%S%3f").to_string(),
                    timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    width: res.width,
                    height: res.height,
                    file_path: Some(res.path.clone()),
                    mode: format!("Video ({})", res.format.extension().to_uppercase()),
                };
                self.config.recent_captures.insert(0, record);
                self.config.recent_captures.truncate(24);
                let _ = self.storage.save_config(&self.config);
                self.set_toast(format!(
                    "Saved {} ({} frames)",
                    res.path.file_name().unwrap_or_default().to_string_lossy(),
                    res.frames
                ));
                if self.config.ftp.enabled && self.config.ftp.auto_upload {
                    self.spawn_ftp_upload(res.path);
                }
            }
            Err(e) => self.set_toast(format!("Recording failed: {e}")),
        }
    }

    fn spawn_ftp_upload(&mut self, path: PathBuf) {
        let ftp_cfg = self.config.ftp.clone();
        if ftp_cfg.host.trim().is_empty() {
            self.set_toast("FTP host is not configured in Settings");
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.upload_rx = Some(rx);
        self.set_toast("Uploading to FTP...");
        std::thread::spawn(move || {
            let res = upload_file_to_ftp(&path, &ftp_cfg).map(|up| {
                if let Some(url) = up.public_url {
                    let _ = copy_text_to_clipboard(&url);
                    format!("Uploaded! URL copied: {url}")
                } else {
                    format!("Uploaded to FTP: {}", up.remote_path)
                }
            });
            let _ = tx.send(res);
        });
    }

    fn build_selected_region_image(&self) -> Option<RgbaImage> {
        let base = self.working_image.as_ref()?;
        let rendered = render_annotations_to_image(base, &self.history.items);
        if let Some(sel) = self.selection_rect {
            let x = sel.min.x.max(0.0) as u32;
            let y = sel.min.y.max(0.0) as u32;
            let w = sel.width().max(1.0) as u32;
            let h = sel.height().max(1.0) as u32;
            Some(crop_rgba_image(&rendered, x, y, w, h))
        } else {
            Some(rendered)
        }
    }

    fn render_recording_floating_bar(&mut self, ctx: &egui::Context) {
        let Some(ref rec_handle) = self.active_recording else {
            return;
        };
        let secs = rec_handle.elapsed_secs();
        let frames = rec_handle.frames();
        let ext = rec_handle.format().extension().to_uppercase();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_inner = Arc::clone(&stop_flag);
        let rec_id = ViewportId::from_hash_of("rustyshot_floating_rec_bar");

        ctx.show_viewport_deferred(
            rec_id,
            ViewportBuilder::default()
                .with_title("RustyShot REC")
                .with_decorations(false)
                .with_always_on_top()
                .with_inner_size([290.0, 40.0]),
            move |rctx, _class| {
                rctx.request_repaint_after(Duration::from_millis(250));
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(Color32::from_rgb(69, 10, 10))
                            .stroke(Stroke::new(1.5, Color32::from_rgb(239, 68, 68)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0)),
                    )
                    .show(rctx, |ui| {
                        let resp = ui.interact(ui.max_rect(), ui.id().with("drag"), Sense::drag());
                        if resp.drag_started() {
                            rctx.send_viewport_cmd(ViewportCommand::StartDrag);
                        }
                        ui.horizontal_centered(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "⏺ REC {:02}:{:02} · {}f · {}",
                                    secs / 60,
                                    secs % 60,
                                    frames,
                                    ext
                                ))
                                .color(Color32::from_rgb(254, 202, 202))
                                .strong()
                                .size(12.0),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("⏹ Stop").clicked() {
                                    stop_flag_inner.store(true, Ordering::Relaxed);
                                }
                            });
                        });
                    });
            },
        );

        if stop_flag.load(Ordering::Relaxed) {
            self.stop_screen_recording();
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn render_dashboard(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("rustyshot_topbar")
            .exact_height(44.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("📷 RustyShot")
                            .size(17.0)
                            .strong()
                            .color(Color32::from_rgb(56, 189, 248)),
                    );
                    ui.label(
                        RichText::new("v0.3.6")
                            .size(11.0)
                            .color(Color32::from_rgb(148, 163, 184)),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("📂 Folder").clicked() {
                            let _ = std::fs::create_dir_all(&self.config.save_dir);
                            open_folder_in_os(&self.config.save_dir);
                        }
                        egui::ComboBox::from_id_salt("delay_combo")
                            .width(74.0)
                            .selected_text(format!("⏱ {}s", self.capture_delay_secs))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.capture_delay_secs, 0, "0s (Now)");
                                ui.selectable_value(&mut self.capture_delay_secs, 3, "3s Delay");
                                ui.selectable_value(&mut self.capture_delay_secs, 5, "5s Delay");
                            });
                        let has_hk_err = self.hotkey_status.contains("in use")
                            || self.hotkey_status.contains("unavailable")
                            || self.hotkey_status.contains("failed");
                        if has_hk_err {
                            if ui
                                .small_button(
                                    RichText::new("⚠ Hotkey conflict")
                                        .size(11.0)
                                        .color(Color32::from_rgb(251, 191, 36)),
                                )
                                .on_hover_text(format!("{}\nClick to change in Settings", self.hotkey_status))
                                .clicked()
                            {
                                self.active_tab = DashboardTab::Settings;
                            }
                        } else {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.hotkey_status)
                                        .size(11.0)
                                        .color(Color32::from_rgb(100, 116, 139)),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&self.hotkey_status);
                        }
                    });
                });
            });

        // Left Navigation Sidebar
        egui::SidePanel::left("rustyshot_sidebar")
            .exact_width(156.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                let nav_items = [
                    (DashboardTab::Capture, "📷  Capture"),
                    (DashboardTab::Tools, "🛠  Tools"),
                    (DashboardTab::Upload, "☁  Upload & FTP"),
                    (DashboardTab::History, "🕘  History"),
                    (DashboardTab::Settings, "⚙  Settings"),
                ];
                for (tab, label) in nav_items {
                    let active = self.active_tab == tab;
                    let fill = if active {
                        Color32::from_rgb(28, 45, 72)
                    } else {
                        Color32::TRANSPARENT
                    };
                    let stroke = if active {
                        Stroke::new(1.0, Color32::from_rgb(56, 189, 248))
                    } else {
                        Stroke::NONE
                    };
                    let btn = egui::Button::new(
                        RichText::new(label)
                            .size(13.0)
                            .color(if active { Color32::WHITE } else { Color32::from_rgb(180, 195, 215) }),
                    )
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(Rounding::same(6.0))
                    .min_size(vec2(138.0, 34.0));
                    if ui.add(btn).clicked() {
                        self.active_tab = tab;
                    }
                    ui.add_space(4.0);
                }

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.add_space(8.0);
                    if self.active_recording.is_some() {
                        if ui
                            .button(RichText::new("⏹ Stop REC").color(Color32::from_rgb(252, 165, 165)))
                            .clicked()
                        {
                            self.stop_screen_recording();
                        }
                    } else {
                        ui.label(
                            RichText::new("Tip: Ctrl+V or Drop image")
                                .size(10.5)
                                .color(Color32::from_rgb(100, 116, 139)),
                        );
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            // Live Recording Banner (visible across tabs)
            if let Some(ref rec_handle) = self.active_recording {
                let secs = rec_handle.elapsed_secs();
                let frames = rec_handle.frames();
                let ext = rec_handle.format().extension().to_uppercase();
                let fname = rec_handle
                    .output_path()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let mut stop_now = false;
                egui::Frame::none()
                    .fill(Color32::from_rgb(69, 10, 10))
                    .stroke(Stroke::new(1.2, Color32::from_rgb(239, 68, 68)))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "⏺ REC {:02}:{:02} · {} frames · {} · {}",
                                    secs / 60,
                                    secs % 60,
                                    frames,
                                    ext,
                                    fname
                                ))
                                .color(Color32::from_rgb(254, 202, 202))
                                .strong(),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("⏹ Stop").clicked() {
                                    stop_now = true;
                                }
                            });
                        });
                    });
                ui.add_space(6.0);
                if stop_now {
                    self.stop_screen_recording();
                }
            }

            match self.active_tab {
                DashboardTab::Capture => {
                    let card_w = ((ui.available_width() - 20.0) / 3.0).max(165.0);
                    // Row 1: Screenshot actions
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if capture_card_btn(
                            ui,
                            "✂ Region Capture",
                            &self.config.region_hotkey,
                            Color32::from_rgb(14, 165, 233),
                            card_w,
                        )
                        .clicked()
                        {
                            self.trigger_capture(ctx, TrayAction::CaptureRegion);
                        }
                        if capture_card_btn(
                            ui,
                            "🖥 Fullscreen",
                            &self.config.fullscreen_hotkey,
                            Color32::from_rgb(59, 130, 246),
                            card_w,
                        )
                        .clicked()
                        {
                            self.trigger_capture(ctx, TrayAction::CaptureFullscreen);
                        }
                        if capture_card_btn(
                            ui,
                            "🪟 Window",
                            "Pick open window",
                            Color32::from_rgb(168, 85, 247),
                            card_w,
                        )
                        .clicked()
                        {
                            self.trigger_capture(ctx, TrayAction::CaptureWindow);
                        }
                    });
                    ui.add_space(8.0);
                    // Row 2: Recording, Color Picker, Paste/Open
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let rec_sub = format!(
                            "{} · {}fps ({})",
                            self.config.video_format.extension().to_uppercase(),
                            self.config.video_fps,
                            self.config.record_hotkey
                        );
                        if capture_card_btn(
                            ui,
                            "🎬 Record Screen",
                            &rec_sub,
                            Color32::from_rgb(239, 68, 68),
                            card_w,
                        )
                        .clicked()
                        {
                            self.trigger_capture(ctx, TrayAction::ToggleRecord);
                        }
                        if capture_card_btn(
                            ui,
                            "🎨 Color Picker",
                            "Pick pixel HEX/RGB",
                            Color32::from_rgb(245, 158, 11),
                            card_w,
                        )
                        .clicked()
                        {
                            self.trigger_capture(ctx, TrayAction::ColorPicker);
                        }
                        if capture_card_btn(
                            ui,
                            "🖼 Open / Paste",
                            "File or Ctrl+V",
                            Color32::from_rgb(34, 197, 94),
                            card_w,
                        )
                        .clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("Images", &["png", "jpg", "jpeg"])
                                .pick_file()
                        {
                            match image::open(&path) {
                                Ok(dyn_img) => self.open_in_editor(ctx, dyn_img.to_rgba8()),
                                Err(e) => self.set_toast(format!("Failed to open image: {e}")),
                            }
                        }
                    });

                    ui.add_space(10.0);
                    // After-Capture Tasks Strip
                    egui::Frame::none()
                        .fill(Color32::from_rgb(20, 25, 36))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(38, 46, 62)))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("After Capture:").strong().size(11.5).color(Color32::from_rgb(148, 163, 184)));
                                let mut changed = false;
                                changed |= ui.checkbox(&mut self.config.auto_copy_clipboard, "Copy").changed();
                                changed |= ui.checkbox(&mut self.config.auto_save_file, "Save File").changed();
                                changed |= ui.checkbox(&mut self.config.after_capture_open_editor, "Open Editor").changed();
                                changed |= ui.checkbox(&mut self.config.after_capture_auto_pin, "Pin").changed();
                                changed |= ui.checkbox(&mut self.config.ftp.auto_upload, "Upload FTP").changed();
                                if changed {
                                    let _ = self.storage.save_config(&self.config);
                                }
                            });
                        });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Recent Captures").size(13.5).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !self.config.recent_captures.is_empty() && ui.small_button("View All →").clicked() {
                                self.active_tab = DashboardTab::History;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    self.render_history_list(ctx, ui, 4);
                }

                DashboardTab::Tools => {
                    ui.label(RichText::new("🛠 Screen & Image Tools").size(15.0).strong());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("🎨 Launch Screen Color Picker").clicked() {
                            self.trigger_capture(ctx, TrayAction::ColorPicker);
                        }
                        if ui.button("📋 Paste Clipboard Image to Editor (Ctrl+V)").clicked() {
                            match paste_image_from_clipboard() {
                                Ok(img) => self.open_in_editor(ctx, img),
                                Err(e) => self.set_toast(e),
                            }
                        }
                        if ui.button("🎬 Record Region (Select First)").clicked() {
                            self.trigger_capture(ctx, TrayAction::CaptureRegion);
                        }
                    });

                    ui.add_space(12.0);
                    ui.label(RichText::new("Recent Picked Colors").size(13.0).strong());
                    ui.add_space(6.0);
                    if self.config.picked_colors.is_empty() {
                        ui.label(
                            RichText::new("No colors picked yet. Use Color Picker or press C on Region Overlay.")
                                .size(12.0)
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                    } else {
                        let mut copy_hex: Option<String> = None;
                        egui::Grid::new("picked_colors_grid").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
                            for hex in &self.config.picked_colors {
                                let (r, g, b) = parse_hex_rgb(hex).unwrap_or((128, 128, 128));
                                let swatch = Color32::from_rgb(r, g, b);
                                let (srect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                                ui.painter().rect(srect, Rounding::same(4.0), swatch, Stroke::new(1.0, Color32::WHITE));
                                ui.label(RichText::new(hex).monospace().strong());
                                ui.label(RichText::new(format!("rgb({r}, {g}, {b})")).size(11.5).color(Color32::from_rgb(148, 163, 184)));
                                if ui.small_button("📋 Copy").clicked() {
                                    copy_hex = Some(hex.clone());
                                }
                                ui.end_row();
                            }
                        });
                        if let Some(h) = copy_hex
                            && copy_text_to_clipboard(&h).is_ok()
                        {
                            self.set_toast(format!("Copied {h}"));
                        }
                    }
                }

                DashboardTab::Upload => {
                    ui.label(RichText::new("☁ Upload & FTP Destination").size(15.0).strong());
                    ui.add_space(8.0);
                    if ui.button("📤 Upload Any File Now...").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_file()
                    {
                        self.spawn_ftp_upload(path);
                    }
                    ui.add_space(8.0);
                    ui.checkbox(&mut self.config.ftp.enabled, "Enable FTP Upload");
                    ui.checkbox(
                        &mut self.config.ftp.auto_upload,
                        "Auto-upload after capture / recording",
                    );
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.config.ftp.ftps_explicit, "🔒 FTPS (Explicit TLS)");
                        ui.checkbox(&mut self.config.ftp.ignore_tls_errors, "Accept self-signed certs");
                    });
                    ui.add_space(6.0);
                    egui::Grid::new("ftp_grid_tab").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Host:");
                        ui.text_edit_singleline(&mut self.config.ftp.host);
                        ui.end_row();
                        ui.label("Port:");
                        let mut port_u32 = self.config.ftp.port as u32;
                        if ui.add(egui::DragValue::new(&mut port_u32).range(1..=65535)).changed() {
                            self.config.ftp.port = port_u32 as u16;
                        }
                        ui.end_row();
                        ui.label("Username:");
                        ui.text_edit_singleline(&mut self.config.ftp.username);
                        ui.end_row();
                        ui.label("Password:");
                        ui.add(egui::TextEdit::singleline(&mut self.config.ftp.password).password(true));
                        ui.end_row();
                        ui.label("Remote Dir:");
                        ui.text_edit_singleline(&mut self.config.ftp.remote_dir);
                        ui.end_row();
                        ui.label("Public URL:");
                        ui.text_edit_singleline(&mut self.config.ftp.public_url_prefix)
                            .on_hover_text("Optional prefix e.g. https://cdn.example.com/shots/");
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("🔌 Test FTP Connection").clicked() {
                            let cfg = self.config.ftp.clone();
                            let (tx, rx) = mpsc::channel();
                            self.upload_rx = Some(rx);
                            self.ftp_test_status = Some("Testing FTP...".to_string());
                            std::thread::spawn(move || {
                                let _ = tx.send(test_ftp_connection(&cfg));
                            });
                        }
                        if ui.button("💾 Save FTP Config").clicked() {
                            let _ = self.storage.save_config(&self.config);
                            self.set_toast("FTP settings saved");
                        }
                        if let Some(ref st) = self.ftp_test_status {
                            ui.label(RichText::new(st).size(11.5).color(Color32::from_rgb(56, 189, 248)));
                        }
                    });
                }

                DashboardTab::History => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🕘 Capture History").size(15.0).strong());
                        ui.add_space(12.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.history_search)
                                .hint_text("Filter by name, mode, or date...")
                                .desired_width(220.0),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !self.config.recent_captures.is_empty() && ui.small_button("Clear All").clicked() {
                                self.config.recent_captures.clear();
                                self.thumb_cache.clear();
                                let _ = self.storage.save_config(&self.config);
                            }
                        });
                    });
                    ui.add_space(8.0);
                    self.render_history_list(ctx, ui, usize::MAX);
                }

                DashboardTab::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(RichText::new("⚙ General & Output").size(15.0).strong());
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Save Folder:");
                            let mut dir_str = self.config.save_dir.to_string_lossy().to_string();
                            ui.add_enabled(false, egui::TextEdit::singleline(&mut dir_str).desired_width(260.0));
                            if ui.button("Browse...").clicked()
                                && let Some(dir) = rfd::FileDialog::new().pick_folder()
                            {
                                self.config.save_dir = dir;
                                let _ = self.storage.save_config(&self.config);
                            }
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Image Format:");
                            egui::ComboBox::from_id_salt("fmt_combo")
                                .selected_text(self.config.format.label())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.config.format, ImageFormatChoice::Png, ImageFormatChoice::Png.label());
                                    ui.selectable_value(&mut self.config.format, ImageFormatChoice::Jpeg, ImageFormatChoice::Jpeg.label());
                                });
                        });
                        if self.config.format == ImageFormatChoice::Jpeg {
                            ui.add(egui::Slider::new(&mut self.config.jpeg_quality, 50..=100).text("JPEG Quality"));
                        }
                        ui.checkbox(&mut self.config.hide_window_on_capture, "Hide RustyShot window before capture");
                        ui.checkbox(&mut self.config.minimize_to_tray, "Minimize to System Tray on close");

                        ui.add_space(8.0);
                        ui.separator();
                        ui.label(RichText::new("🎬 Video Recording").strong());
                        ui.horizontal(|ui| {
                            ui.label("Video Format:");
                            egui::ComboBox::from_id_salt("vfmt_combo")
                                .selected_text(self.config.video_format.label())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.config.video_format,
                                        VideoFormatChoice::Mp4,
                                        VideoFormatChoice::Mp4.label(),
                                    );
                                    ui.selectable_value(
                                        &mut self.config.video_format,
                                        VideoFormatChoice::Gif,
                                        VideoFormatChoice::Gif.label(),
                                    );
                                });
                        });
                        ui.add(egui::Slider::new(&mut self.config.video_fps, 5..=30).text("FPS"));
                        ui.label(
                            RichText::new(if self.ffmpeg_available {
                                "FFmpeg: Detected (MP4 ready)"
                            } else {
                                "FFmpeg: Not found (GIF works built-in; place ffmpeg next to exe for MP4)"
                            })
                            .size(11.0)
                            .color(Color32::from_rgb(148, 163, 184)),
                        );

                        ui.add_space(8.0);
                        ui.separator();
                        ui.label(RichText::new("⌨ Global Hotkeys").strong());
                        egui::Grid::new("hk_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                            ui.label("Region:");
                            ui.text_edit_singleline(&mut self.config.region_hotkey);
                            ui.end_row();
                            ui.label("Fullscreen:");
                            ui.text_edit_singleline(&mut self.config.fullscreen_hotkey);
                            ui.end_row();
                            ui.label("Record Toggle:");
                            ui.text_edit_singleline(&mut self.config.record_hotkey);
                            ui.end_row();
                        });
                        ui.add_space(8.0);
                        if ui.button("💾 Save & Apply Settings").clicked() {
                            let _ = self.storage.save_config(&self.config);
                            if let Some(ref mut hk) = self.hotkeys {
                                self.hotkey_status = hk.reregister(
                                    &self.config.region_hotkey,
                                    &self.config.fullscreen_hotkey,
                                    &self.config.record_hotkey,
                                );
                            }
                            self.set_toast("Settings saved");
                        }
                    });
                }
            }
        });

        // Window Picker Modal
        if self.show_window_picker {
            let mut open = self.show_window_picker;
            let mut picked_id: Option<u32> = None;
            egui::Window::new("🪟 Select Window to Capture")
                .open(&mut open)
                .collapsible(false)
                .default_size(vec2(460.0, 340.0))
                .show(ctx, |ui| {
                    if ui.button("🔄 Refresh Window List").clicked() {
                        self.window_list = list_capturable_windows().unwrap_or_default();
                    }
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for w in &self.window_list {
                            let label = format!(
                                "{} — {} ({}×{})",
                                w.app_name, w.title, w.width, w.height
                            );
                            if ui.selectable_label(false, label).clicked() {
                                picked_id = Some(w.id);
                            }
                        }
                    });
                });
            self.show_window_picker = open;
            if let Some(wid) = picked_id {
                self.show_window_picker = false;
                match capture_window_by_id(wid) {
                    Ok(img) => {
                        let open_ed = self.config.after_capture_open_editor;
                        self.finalize_captured_image(ctx, img, "Window", open_ed);
                    }
                    Err(e) => self.set_toast(format!("Window capture failed: {e}")),
                }
            }
        }
    }

    fn render_history_list(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, max_items: usize) {
        let query = self.history_search.trim().to_lowercase();
        let filtered: Vec<CaptureRecord> = self
            .config
            .recent_captures
            .iter()
            .filter(|rec| {
                if query.is_empty() || max_items != usize::MAX {
                    return true;
                }
                let fname = rec
                    .file_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                fname.contains(&query)
                    || rec.mode.to_lowercase().contains(&query)
                    || rec.timestamp.to_lowercase().contains(&query)
            })
            .take(max_items)
            .cloned()
            .collect();

        if filtered.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No captures yet. Click Region Capture or press Ctrl+Shift+S to start.")
                        .color(Color32::from_rgb(148, 163, 184)),
                );
            });
            return;
        }

        let mut open_path: Option<PathBuf> = None;
        let mut copy_path: Option<PathBuf> = None;
        let mut pin_path: Option<PathBuf> = None;
        let mut ftp_path: Option<PathBuf> = None;
        let mut delete_id: Option<String> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for rec in &filtered {
                egui::Frame::none()
                    .fill(Color32::from_rgb(21, 26, 36))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(38, 46, 62)))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(ref p) = rec.file_path {
                                if !self.thumb_cache.contains_key(&rec.id)
                                    && let Ok(img) = image::open(p)
                                {
                                    let thumb = img.thumbnail(80, 48).to_rgba8();
                                    let tex = Self::rgba_to_texture(
                                        ctx,
                                        &format!("thumb_{}", rec.id),
                                        &thumb,
                                    );
                                    self.thumb_cache.insert(rec.id.clone(), tex);
                                }
                                if let Some(tex) = self.thumb_cache.get(&rec.id) {
                                    ui.image((tex.id(), vec2(80.0, 48.0)));
                                }
                            }

                            ui.vertical(|ui| {
                                let name = rec
                                    .file_path
                                    .as_ref()
                                    .and_then(|p| p.file_name())
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_else(|| format!("{} Capture", rec.mode));
                                ui.label(RichText::new(name).strong().size(12.5));
                                ui.label(
                                    RichText::new(format!(
                                        "{} × {} px  ·  {}  ·  {}",
                                        rec.width, rec.height, rec.mode, rec.timestamp
                                    ))
                                    .size(11.0)
                                    .color(Color32::from_rgb(148, 163, 184)),
                                );
                            });

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("🗑").on_hover_text("Remove from history").clicked() {
                                    delete_id = Some(rec.id.clone());
                                }
                                if let Some(ref p) = rec.file_path
                                    && p.exists()
                                {
                                    let is_video = rec.mode.starts_with("Video");
                                    if ui.small_button("☁ FTP").on_hover_text("Upload to FTP").clicked() {
                                        ftp_path = Some(p.clone());
                                    }
                                    if !is_video {
                                        if ui.small_button("📌 Pin").clicked() {
                                            pin_path = Some(p.clone());
                                        }
                                        if ui.small_button("✏ Edit").clicked() {
                                            open_path = Some(p.clone());
                                        }
                                        if ui.small_button("📋 Copy").clicked() {
                                            copy_path = Some(p.clone());
                                        }
                                    }
                                }
                            });
                        });
                    });
                ui.add_space(4.0);
            }
        });

        if let Some(p) = open_path {
            match image::open(&p) {
                Ok(d) => self.open_in_editor(ctx, d.to_rgba8()),
                Err(e) => self.set_toast(format!("Failed to load image: {e}")),
            }
        }
        if let Some(p) = copy_path {
            match image::open(&p) {
                Ok(d) => match copy_image_to_clipboard(&d.to_rgba8()) {
                    Ok(()) => self.set_toast("Copied image to clipboard"),
                    Err(e) => self.set_toast(format!("Clipboard error: {e}")),
                },
                Err(e) => self.set_toast(format!("Failed to load image: {e}")),
            }
        }
        if let Some(p) = pin_path {
            match image::open(&p) {
                Ok(d) => self.pin_image(d.to_rgba8()),
                Err(e) => self.set_toast(format!("Failed to load image: {e}")),
            }
        }
        if let Some(p) = ftp_path {
            self.spawn_ftp_upload(p);
        }
        if let Some(id) = delete_id {
            self.config.recent_captures.retain(|r| r.id != id);
            self.thumb_cache.remove(&id);
            let _ = self.storage.save_config(&self.config);
        }
    }

    fn render_region_overlay(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.exit_overlay_to_dashboard(ctx);
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            if let Some(img) = self.build_selected_region_image() {
                self.finalize_captured_image(ctx, img, "Region", false);
            }
            return;
        }
        if ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(Key::Z)) {
            self.history.undo();
        }
        if ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(Key::Y)) {
            self.history.redo();
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let screen_rect = ui.max_rect();
                let resp = ui.allocate_rect(screen_rect, Sense::click_and_drag());

                let (Some(tex), Some(raw_img)) =
                    (self.working_texture.as_ref(), self.working_image.as_ref())
                else {
                    return;
                };

                let img_w = raw_img.width() as f32;
                let img_h = raw_img.height() as f32;
                let scale_x = screen_rect.width() / img_w.max(1.0);
                let scale_y = screen_rect.height() / img_h.max(1.0);
                let zoom = scale_x.min(scale_y);
                let draw_size = vec2(img_w * zoom, img_h * zoom);
                let origin = pos2(
                    screen_rect.center().x - draw_size.x * 0.5,
                    screen_rect.center().y - draw_size.y * 0.5,
                );
                let img_screen_rect = Rect::from_min_size(origin, draw_size);

                let painter = ui.painter();
                painter.image(
                    tex.id(),
                    img_screen_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );

                // Helper closures for coordinate mapping
                let to_img = |p: Pos2| -> Pos2 {
                    pos2(
                        ((p.x - origin.x) / zoom).clamp(0.0, img_w),
                        ((p.y - origin.y) / zoom).clamp(0.0, img_h),
                    )
                };
                let to_scr = |r: Rect| -> Rect {
                    Rect::from_min_max(
                        pos2(origin.x + r.min.x * zoom, origin.y + r.min.y * zoom),
                        pos2(origin.x + r.max.x * zoom, origin.y + r.max.y * zoom),
                    )
                };

                // Detect hovered window for smart window snap when no selection exists
                let hovered_win_rect: Option<(Rect, String)> = if self.selection_rect.is_none()
                    && let Some(hover) = ctx.input(|i| i.pointer.hover_pos())
                {
                    let ip = to_img(hover);
                    let gx = ip.x as i32 + self.working_origin.0;
                    let gy = ip.y as i32 + self.working_origin.1;
                    self.window_list.iter().find_map(|w| {
                        let wx0 = w.x;
                        let wy0 = w.y;
                        let wx1 = w.x + w.width as i32;
                        let wy1 = w.y + w.height as i32;
                        if gx >= wx0 && gx <= wx1 && gy >= wy0 && gy <= wy1 {
                            let r = Rect::from_min_max(
                                pos2(
                                    ((wx0 - self.working_origin.0) as f32).clamp(0.0, img_w),
                                    ((wy0 - self.working_origin.1) as f32).clamp(0.0, img_h),
                                ),
                                pos2(
                                    ((wx1 - self.working_origin.0) as f32).clamp(0.0, img_w),
                                    ((wy1 - self.working_origin.1) as f32).clamp(0.0, img_h),
                                ),
                            );
                            if r.width() > 20.0 && r.height() > 20.0 {
                                let title = if w.title.is_empty() {
                                    w.app_name.clone()
                                } else {
                                    w.title.clone()
                                };
                                return Some((r, title));
                            }
                        }
                        None
                    })
                } else {
                    None
                };

                // Color Picker Mode: click any pixel to copy HEX and return
                if self.overlay_color_picker_only
                    && resp.clicked()
                    && let Some(hover) = ctx.input(|i| i.pointer.hover_pos())
                {
                    let ip = to_img(hover);
                        let px = (ip.x as i32).clamp(0, raw_img.width().saturating_sub(1) as i32);
                        let py = (ip.y as i32).clamp(0, raw_img.height().saturating_sub(1) as i32);
                        let rgb = raw_img.get_pixel(px as u32, py as u32).0;
                        let hex = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                        let _ = copy_text_to_clipboard(&hex);
                        self.config.picked_colors.retain(|c| c != &hex);
                        self.config.picked_colors.insert(0, hex.clone());
                        self.config.picked_colors.truncate(16);
                        let _ = self.storage.save_config(&self.config);
                        self.overlay_color_picker_only = false;
                        self.exit_overlay_to_dashboard(ctx);
                        self.set_toast(format!("Picked {hex} (rgb({}, {}, {})) — copied!", rgb[0], rgb[1], rgb[2]));
                        return;
                }

                // Arrow keys nudge selection by 1px (Shift+Arrow by 10px)
                if let Some(ref mut sel) = self.selection_rect {
                    let step = if ctx.input(|i| i.modifiers.shift) { 10.0 } else { 1.0 };
                    let mut delta = Vec2::ZERO;
                    if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) { delta.x -= step; }
                    if ctx.input(|i| i.key_pressed(Key::ArrowRight)) { delta.x += step; }
                    if ctx.input(|i| i.key_pressed(Key::ArrowUp)) { delta.y -= step; }
                    if ctx.input(|i| i.key_pressed(Key::ArrowDown)) { delta.y += step; }
                    if delta != Vec2::ZERO {
                        *sel = sel.translate(delta);
                    }
                }

                // Handle pointer interaction on overlay
                let shift_held = ctx.input(|i| i.modifiers.shift);
                if !self.overlay_color_picker_only && let Some(pos) = resp.interact_pointer_pos() {
                    let img_pt = to_img(pos);
                    if resp.drag_started() {
                        if let Some(sel) = self.selection_rect {
                            let scr_sel = to_scr(sel);
                            if let Some(handle) = hit_test_resize_handle(scr_sel, pos) {
                                self.drag_action = DragAction::ResizingSelection(handle);
                                self.drag_anchor = img_pt;
                                self.drag_orig_rect = Some(sel);
                            } else if scr_sel.contains(pos) && self.active_tool != ToolKind::Select {
                                self.drag_action = DragAction::DrawingAnnotation;
                                if self.active_tool == ToolKind::StepBadge {
                                    self.history.push(AnnotationItem {
                                        kind: ToolKind::StepBadge,
                                        points: vec![[img_pt.x, img_pt.y]],
                                        color: self.active_color.to_array(),
                                        stroke_width: self.stroke_width,
                                        text: String::new(),
                                        step_number: self.next_step_number,
                                    });
                                    self.next_step_number += 1;
                                    self.drag_action = DragAction::None;
                                } else if self.active_tool == ToolKind::Text {
                                    self.pending_text_pos = Some([img_pt.x, img_pt.y]);
                                    self.pending_text_buf.clear();
                                    self.drag_action = DragAction::None;
                                } else {
                                    self.in_progress_item = Some(AnnotationItem {
                                        kind: self.active_tool,
                                        points: vec![[img_pt.x, img_pt.y]],
                                        color: self.active_color.to_array(),
                                        stroke_width: self.stroke_width,
                                        text: String::new(),
                                        step_number: 0,
                                    });
                                }
                            } else if scr_sel.contains(pos) {
                                self.drag_action = DragAction::MovingSelection;
                                self.drag_anchor = img_pt;
                                self.drag_orig_rect = Some(sel);
                            } else {
                                self.drag_action = DragAction::CreatingSelection;
                                self.drag_anchor = img_pt;
                                self.selection_rect = Some(Rect::from_two_pos(img_pt, img_pt));
                            }
                        } else {
                            self.drag_action = DragAction::CreatingSelection;
                            self.drag_anchor = img_pt;
                            self.selection_rect = Some(Rect::from_two_pos(img_pt, img_pt));
                        }
                    } else if resp.dragged() {
                        match self.drag_action {
                            DragAction::CreatingSelection => {
                                let mut end_pt = img_pt;
                                if shift_held {
                                    let dx = end_pt.x - self.drag_anchor.x;
                                    let dy = end_pt.y - self.drag_anchor.y;
                                    let side = dx.abs().max(dy.abs());
                                    end_pt = pos2(
                                        self.drag_anchor.x + side * dx.signum(),
                                        self.drag_anchor.y + side * dy.signum(),
                                    );
                                }
                                self.selection_rect = Some(Rect::from_two_pos(self.drag_anchor, end_pt));
                            }
                            DragAction::MovingSelection => {
                                if let Some(orig) = self.drag_orig_rect {
                                    let delta = img_pt - self.drag_anchor;
                                    let mut moved = orig.translate(delta);
                                    let dx = if moved.min.x < 0.0 {
                                        -moved.min.x
                                    } else if moved.max.x > img_w {
                                        img_w - moved.max.x
                                    } else {
                                        0.0
                                    };
                                    let dy = if moved.min.y < 0.0 {
                                        -moved.min.y
                                    } else if moved.max.y > img_h {
                                        img_h - moved.max.y
                                    } else {
                                        0.0
                                    };
                                    moved = moved.translate(vec2(dx, dy));
                                    self.selection_rect = Some(moved);
                                }
                            }
                            DragAction::ResizingSelection(handle) => {
                                if let Some(orig) = self.drag_orig_rect {
                                    self.selection_rect =
                                        Some(apply_resize_handle(orig, handle, img_pt, img_w, img_h));
                                }
                            }
                            DragAction::DrawingAnnotation => {
                                if let Some(ref mut item) = self.in_progress_item {
                                    if matches!(item.kind, ToolKind::Pen | ToolKind::Highlighter)
                                        || item.points.len() == 1
                                    {
                                        item.points.push([img_pt.x, img_pt.y]);
                                    } else {
                                        item.points[1] = [img_pt.x, img_pt.y];
                                    }
                                }
                            }
                            DragAction::None => {}
                        }
                    }
                }

                // Single click snaps to hovered window
                if resp.clicked()
                    && self.selection_rect.is_none()
                    && let Some((wr, _)) = hovered_win_rect
                {
                    self.selection_rect = Some(wr);
                }

                if resp.drag_stopped() {
                    if let Some(item) = self.in_progress_item.take() {
                        self.history.push(item);
                    }
                    if let Some(sel) = self.selection_rect
                        && (sel.width() < 4.0 || sel.height() < 4.0)
                    {
                        if let Some((wr, _)) = hovered_win_rect {
                            self.selection_rect = Some(wr);
                        } else {
                            self.selection_rect = None;
                        }
                    }
                    self.drag_action = DragAction::None;
                }

                // Update cursor icon when hovering resize handles
                if let (Some(sel), Some(hover)) =
                    (self.selection_rect, ctx.input(|i| i.pointer.hover_pos()))
                    && let Some(handle) = hit_test_resize_handle(to_scr(sel), hover)
                {
                    ctx.set_cursor_icon(handle_cursor_icon(handle));
                }

                // Draw dimmed scrim outside selection
                let scrim = Color32::from_black_alpha(130);
                if let Some(sel) = self.selection_rect {
                    let s = to_scr(sel);
                    painter.rect_filled(Rect::from_min_max(screen_rect.min, pos2(screen_rect.max.x, s.min.y)), 0.0, scrim);
                    painter.rect_filled(Rect::from_min_max(pos2(screen_rect.min.x, s.max.y), screen_rect.max), 0.0, scrim);
                    painter.rect_filled(Rect::from_min_max(pos2(screen_rect.min.x, s.min.y), pos2(s.min.x, s.max.y)), 0.0, scrim);
                    painter.rect_filled(Rect::from_min_max(pos2(s.max.x, s.min.y), pos2(screen_rect.max.x, s.max.y)), 0.0, scrim);

                    // Paint annotations clipped to selection
                    let clipped = painter.with_clip_rect(s);
                    paint_annotations_egui(&clipped, &self.history.items, origin, zoom);
                    if let Some(ref item) = self.in_progress_item {
                        paint_annotations_egui(&clipped, std::slice::from_ref(item), origin, zoom);
                    }

                    // Selection border + 8 resize handles + dimensions badge
                    painter.rect_stroke(s, Rounding::ZERO, Stroke::new(1.5, Color32::from_rgb(56, 189, 248)));
                    for (_, pt) in resize_handle_positions(s) {
                        let hr = Rect::from_center_size(pt, vec2(8.0, 8.0));
                        painter.rect_filled(hr, Rounding::same(1.5), Color32::WHITE);
                        painter.rect_stroke(hr, Rounding::same(1.5), Stroke::new(1.0, Color32::from_rgb(14, 165, 233)));
                    }

                    let dim_text = format!("{} × {} px", sel.width().round() as u32, sel.height().round() as u32);
                    let badge_pos = pos2(s.min.x, (s.min.y - 22.0).max(4.0));
                    let badge_rect = Rect::from_min_size(badge_pos, vec2(96.0, 18.0));
                    painter.rect_filled(badge_rect, Rounding::same(4.0), Color32::from_rgb(15, 23, 42));
                    painter.text(
                        badge_rect.center(),
                        Align2::CENTER_CENTER,
                        dim_text,
                        FontId::monospace(10.5),
                        Color32::from_rgb(56, 189, 248),
                    );
                } else if let Some((wr, ref wtitle)) = hovered_win_rect {
                    let s = to_scr(wr);
                    painter.rect_filled(Rect::from_min_max(screen_rect.min, pos2(screen_rect.max.x, s.min.y)), 0.0, scrim);
                    painter.rect_filled(Rect::from_min_max(pos2(screen_rect.min.x, s.max.y), screen_rect.max), 0.0, scrim);
                    painter.rect_filled(Rect::from_min_max(pos2(screen_rect.min.x, s.min.y), pos2(s.min.x, s.max.y)), 0.0, scrim);
                    painter.rect_filled(Rect::from_min_max(pos2(s.max.x, s.min.y), pos2(screen_rect.max.x, s.max.y)), 0.0, scrim);
                    painter.rect_stroke(s, Rounding::ZERO, Stroke::new(2.0, Color32::from_rgb(34, 197, 94)));
                    let label = format!("Click to snap: {wtitle} ({}×{})", wr.width().round() as u32, wr.height().round() as u32);
                    let badge_rect = Rect::from_min_size(pos2(s.min.x, (s.min.y - 22.0).max(4.0)), vec2(260.0, 18.0));
                    painter.rect_filled(badge_rect, Rounding::same(4.0), Color32::from_rgb(15, 23, 42));
                    painter.text(
                        badge_rect.center(),
                        Align2::CENTER_CENTER,
                        label,
                        FontId::proportional(10.5),
                        Color32::from_rgb(34, 197, 94),
                    );
                } else if self.overlay_color_picker_only {
                    painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(60));
                    painter.text(
                        pos2(screen_rect.center().x, 36.0),
                        Align2::CENTER_CENTER,
                        "🎨 Color Picker: Click any pixel to copy HEX  ·  Esc: Cancel",
                        FontId::proportional(14.0),
                        Color32::WHITE,
                    );
                } else {
                    painter.rect_filled(screen_rect, 0.0, scrim);
                    painter.text(
                        pos2(screen_rect.center().x, 36.0),
                        Align2::CENTER_CENTER,
                        "Drag to select (Shift: 1:1 square)  ·  Click window to snap  ·  Arrows: 1px nudge  ·  Esc: Cancel",
                        FontId::proportional(13.5),
                        Color32::WHITE,
                    );
                }

                // Fullscreen Crosshair Guides when selecting or picking color
                if (self.selection_rect.is_none() || self.overlay_color_picker_only)
                    && let Some(hover) = ctx.input(|i| i.pointer.hover_pos())
                {
                    let guide_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(56, 189, 248, 120));
                    painter.line_segment([pos2(screen_rect.min.x, hover.y), pos2(screen_rect.max.x, hover.y)], guide_stroke);
                    painter.line_segment([pos2(hover.x, screen_rect.min.y), pos2(hover.x, screen_rect.max.y)], guide_stroke);
                }

                // Zoomed 11x11 Pixel Magnifier Loupe near cursor
                if let Some(hover) = ctx.input(|i| i.pointer.hover_pos())
                    && (self.selection_rect.is_none()
                        || matches!(
                            self.drag_action,
                            DragAction::CreatingSelection | DragAction::ResizingSelection(_)
                        ))
                {
                    let ip = to_img(hover);
                    let px = (ip.x as i32).clamp(0, raw_img.width().saturating_sub(1) as i32);
                    let py = (ip.y as i32).clamp(0, raw_img.height().saturating_sub(1) as i32);
                    let rgb = raw_img.get_pixel(px as u32, py as u32).0;
                    let hex = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                    if ctx.input(|i| i.key_pressed(Key::C)) {
                        let _ = copy_text_to_clipboard(&hex);
                    }

                    // Build 11x11 pixel patch around cursor
                    const RAD: i32 = 5;
                    const GRID: usize = 11;
                    let mut pixels = Vec::with_capacity(GRID * GRID);
                    for dy in -RAD..=RAD {
                        for dx in -RAD..=RAD {
                            let sx = (px + dx).clamp(0, raw_img.width().saturating_sub(1) as i32) as u32;
                            let sy = (py + dy).clamp(0, raw_img.height().saturating_sub(1) as i32) as u32;
                            let p = raw_img.get_pixel(sx, sy).0;
                            pixels.push(Color32::from_rgb(p[0], p[1], p[2]));
                        }
                    }
                    let cimg = ColorImage { size: [GRID, GRID], pixels };
                    let ltex = self.loupe_texture.get_or_insert_with(|| {
                        ctx.load_texture("loupe_tex", cimg.clone(), TextureOptions::NEAREST)
                    });
                    ltex.set(cimg, TextureOptions::NEAREST);

                    let loupe_w = 132.0;
                    let loupe_h = 136.0;
                    let loupe_pos = pos2(
                        (hover.x + 18.0).min(screen_rect.max.x - loupe_w - 8.0),
                        (hover.y + 18.0).min(screen_rect.max.y - loupe_h - 8.0),
                    );
                    let loupe_rect = Rect::from_min_size(loupe_pos, vec2(loupe_w, loupe_h));
                    painter.rect(
                        loupe_rect,
                        Rounding::same(6.0),
                        Color32::from_rgb(15, 23, 42),
                        Stroke::new(1.0, Color32::from_rgb(56, 189, 248)),
                    );

                    let grid_rect = Rect::from_min_size(loupe_pos + vec2(11.0, 8.0), vec2(88.0, 88.0));
                    painter.image(
                        ltex.id(),
                        grid_rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    // Center pixel crosshair
                    let cell = 88.0 / 11.0;
                    let center_cell = Rect::from_center_size(grid_rect.center(), vec2(cell, cell));
                    painter.rect_stroke(center_cell, Rounding::ZERO, Stroke::new(1.2, Color32::from_rgb(56, 189, 248)));

                    // Color swatch + coords + HEX
                    painter.rect_filled(
                        Rect::from_min_size(loupe_pos + vec2(105.0, 8.0), vec2(18.0, 88.0)),
                        Rounding::same(3.0),
                        Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    );
                    painter.text(
                        loupe_pos + vec2(10.0, 106.0),
                        Align2::LEFT_CENTER,
                        format!("XY: {px}, {py}"),
                        FontId::monospace(10.0),
                        Color32::from_rgb(148, 163, 184),
                    );
                    painter.text(
                        loupe_pos + vec2(10.0, 122.0),
                        Align2::LEFT_CENTER,
                        format!("{hex} (C: Copy)"),
                        FontId::monospace(10.5),
                        Color32::WHITE,
                    );
                }
            });

        // Floating Toolbar when selection exists
        if let Some(sel) = self.selection_rect
            && sel.width() > 8.0
            && sel.height() > 8.0
            && self.drag_action != DragAction::CreatingSelection
        {
            let mut do_copy = false;
            let mut do_save = false;
            let mut do_pin = false;
            let mut do_rec = false;
            let mut do_editor = false;
            let mut do_cancel = false;

            egui::Area::new(egui::Id::new("overlay_toolbar"))
                .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -18.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(18, 24, 38))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(56, 189, 248)))
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for tool in [
                                    ToolKind::Select,
                                    ToolKind::Pen,
                                    ToolKind::Highlighter,
                                    ToolKind::Arrow,
                                    ToolKind::Rect,
                                    ToolKind::Ellipse,
                                    ToolKind::StepBadge,
                                    ToolKind::Text,
                                    ToolKind::Pixelate,
                                    ToolKind::Blur,
                                ] {
                                    let active = self.active_tool == tool;
                                    if ui
                                        .selectable_label(active, tool.icon())
                                        .on_hover_text(tool.label())
                                        .clicked()
                                    {
                                        self.active_tool = tool;
                                    }
                                }
                                ui.separator();
                                for col in PALETTE_COLORS {
                                    let is_sel = self.active_color == col;
                                    let (r, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                                    ui.painter().circle_filled(r.center(), 6.0, col);
                                    if is_sel {
                                        ui.painter().circle_stroke(r.center(), 7.5, Stroke::new(1.5, Color32::WHITE));
                                    }
                                    if resp.clicked() {
                                        self.active_color = col;
                                    }
                                }
                                ui.separator();
                                if ui.small_button("↩").on_hover_text("Undo (Ctrl+Z)").clicked() {
                                    self.history.undo();
                                }
                                if ui.small_button("↪").on_hover_text("Redo (Ctrl+Y)").clicked() {
                                    self.history.redo();
                                }
                                ui.separator();
                                if ui.button("📋 Copy").clicked() {
                                    do_copy = true;
                                }
                                if ui.button("💾 Save").clicked() {
                                    do_save = true;
                                }
                                if ui.button("📌 Pin").clicked() {
                                    do_pin = true;
                                }
                                if ui.button("🎬 Rec").on_hover_text("Record this region").clicked() {
                                    do_rec = true;
                                }
                                if ui.button("✏ Editor").clicked() {
                                    do_editor = true;
                                }
                                if ui.button("❌").on_hover_text("Cancel (Esc)").clicked() {
                                    do_cancel = true;
                                }
                            });
                        });
                });

            if do_cancel {
                self.exit_overlay_to_dashboard(ctx);
            } else if do_rec {
                let r = [
                    sel.min.x.max(0.0) as u32,
                    sel.min.y.max(0.0) as u32,
                    sel.width().max(2.0) as u32,
                    sel.height().max(2.0) as u32,
                ];
                self.start_screen_recording(ctx, Some(r));
            } else if do_pin {
                if let Some(img) = self.build_selected_region_image() {
                    self.pin_image(img);
                    self.exit_overlay_to_dashboard(ctx);
                }
            } else if do_editor {
                if let Some(img) = self.build_selected_region_image() {
                    self.open_in_editor(ctx, img);
                }
            } else if do_copy {
                if let Some(img) = self.build_selected_region_image() {
                    let prev_save = self.config.auto_save_file;
                    self.config.auto_save_file = false;
                    self.finalize_captured_image(ctx, img, "Region", false);
                    self.config.auto_save_file = prev_save;
                }
            } else if do_save
                && let Some(img) = self.build_selected_region_image()
            {
                self.finalize_captured_image(ctx, img, "Region", false);
            }
        }

        // Inline Text Input Modal on Overlay
        if let Some(pos) = self.pending_text_pos {
            egui::Window::new("Add Text")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    let resp = ui.text_edit_singleline(&mut self.pending_text_buf);
                    resp.request_focus();
                    ui.horizontal(|ui| {
                        if (ui.button("Add").clicked() || ui.input(|i| i.key_pressed(Key::Enter)))
                            && !self.pending_text_buf.trim().is_empty()
                        {
                            self.history.push(AnnotationItem {
                                kind: ToolKind::Text,
                                points: vec![pos],
                                color: self.active_color.to_array(),
                                stroke_width: self.stroke_width,
                                text: self.pending_text_buf.clone(),
                                step_number: 0,
                            });
                            self.pending_text_pos = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_text_pos = None;
                        }
                    });
                });
        }
    }

    fn render_editor(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(Key::Z)) {
            self.history.undo();
        }
        if ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(Key::Y)) {
            self.history.redo();
        }

        let mut go_back = false;
        let mut do_copy = false;
        let mut do_save = false;
        let mut do_save_as = false;
        let mut do_pin = false;
        let mut do_rotate_cw = false;
        let mut do_rotate_ccw = false;
        let mut do_flip_h = false;
        let mut do_flip_v = false;
        let mut do_apply_crop = false;
        let mut do_fit_zoom = false;

        // Ctrl + Mouse Wheel zoom in Editor
        let wheel_delta = ctx.input(|i| {
            if i.modifiers.command || i.modifiers.ctrl {
                i.raw_scroll_delta.y
            } else {
                0.0
            }
        });
        if wheel_delta.abs() > 0.1 {
            self.editor_zoom = (self.editor_zoom + wheel_delta * 0.0015).clamp(0.2, 4.0);
        }

        egui::TopBottomPanel::top("editor_toolbar")
            .exact_height(44.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button("⬅ Back").clicked() {
                        go_back = true;
                    }
                    ui.separator();
                    for tool in [
                        ToolKind::Select,
                        ToolKind::Pen,
                        ToolKind::Highlighter,
                        ToolKind::Arrow,
                        ToolKind::Rect,
                        ToolKind::Ellipse,
                        ToolKind::StepBadge,
                        ToolKind::Text,
                        ToolKind::Pixelate,
                        ToolKind::Blur,
                    ] {
                        let active = self.active_tool == tool;
                        if ui
                            .selectable_label(active, format!("{} {}", tool.icon(), tool.label()))
                            .clicked()
                        {
                            self.active_tool = tool;
                        }
                    }
                    ui.separator();
                    for col in PALETTE_COLORS {
                        let is_sel = self.active_color == col;
                        let (r, resp) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 6.0, col);
                        if is_sel {
                            ui.painter().circle_stroke(r.center(), 7.5, Stroke::new(1.5, Color32::WHITE));
                        }
                        if resp.clicked() {
                            self.active_color = col;
                        }
                    }
                    ui.separator();
                    ui.add(egui::Slider::new(&mut self.stroke_width, 1.5..=10.0).show_value(false))
                        .on_hover_text("Stroke width");
                    if ui.button("↩").on_hover_text("Undo (Ctrl+Z)").clicked() {
                        self.history.undo();
                    }
                    if ui.button("↪").on_hover_text("Redo (Ctrl+Y)").clicked() {
                        self.history.redo();
                    }
                    if !self.history.items.is_empty()
                        && ui.button("🧹").on_hover_text("Clear annotations").clicked()
                    {
                        self.history.clear();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("💾 Save As...").clicked() {
                            do_save_as = true;
                        }
                        if ui.button("💾 Save").clicked() {
                            do_save = true;
                        }
                        if ui.button("📌 Pin").clicked() {
                            do_pin = true;
                        }
                        if ui.button("📋 Copy").clicked() {
                            do_copy = true;
                        }
                    });
                });
            });

        // Secondary Editor Bar: Image Transforms (Crop, Rotate, Flip) + Zoom
        egui::TopBottomPanel::top("editor_transform_bar")
            .exact_height(32.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("Image:").size(11.5).color(Color32::from_rgb(148, 163, 184)));
                    if ui.small_button("↺ 90°").on_hover_text("Rotate 90° CCW").clicked() {
                        do_rotate_ccw = true;
                    }
                    if ui.small_button("↻ 90°").on_hover_text("Rotate 90° CW").clicked() {
                        do_rotate_cw = true;
                    }
                    if ui.small_button("⇄ Flip H").clicked() {
                        do_flip_h = true;
                    }
                    if ui.small_button("⇅ Flip V").clicked() {
                        do_flip_v = true;
                    }
                    if let Some(cr) = self.editor_crop_rect
                        && cr.width() >= 4.0
                        && cr.height() >= 4.0
                    {
                        ui.separator();
                        if ui.small_button("✂ Apply Crop").clicked() {
                            do_apply_crop = true;
                        }
                        if ui.small_button("✖ Clear Crop").clicked() {
                            self.editor_crop_rect = None;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Fit").clicked() {
                            do_fit_zoom = true;
                        }
                        if ui.small_button("+").clicked() {
                            self.editor_zoom = (self.editor_zoom + 0.15).min(4.0);
                        }
                        if ui.small_button(format!("{}%", (self.editor_zoom * 100.0).round() as u32)).clicked() {
                            self.editor_zoom = 1.0;
                        }
                        if ui.small_button("-").clicked() {
                            self.editor_zoom = (self.editor_zoom - 0.15).max(0.2);
                        }
                        if let Some(ref img) = self.working_image {
                            ui.label(
                                RichText::new(format!("{} × {} px", img.width(), img.height()))
                                    .size(11.0)
                                    .color(Color32::from_rgb(148, 163, 184)),
                            );
                        }
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let avail = ui.available_size();
            let (Some(tex), Some(raw_img)) =
                (self.working_texture.as_ref(), self.working_image.as_ref())
            else {
                return;
            };
            if do_fit_zoom {
                let zx = (avail.x - 16.0) / (raw_img.width() as f32).max(1.0);
                let zy = (avail.y - 16.0) / (raw_img.height() as f32).max(1.0);
                self.editor_zoom = zx.min(zy).clamp(0.2, 4.0);
            }

            egui::ScrollArea::both().show(ui, |ui| {
                let img_w = raw_img.width() as f32 * self.editor_zoom;
                let img_h = raw_img.height() as f32 * self.editor_zoom;
                let (canvas_rect, resp) =
                    ui.allocate_exact_size(vec2(img_w, img_h), Sense::click_and_drag());
                let origin = canvas_rect.min;

                ui.painter().image(
                    tex.id(),
                    canvas_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );

                let to_img = |p: Pos2| -> [f32; 2] {
                    [
                        ((p.x - origin.x) / self.editor_zoom).clamp(0.0, raw_img.width() as f32),
                        ((p.y - origin.y) / self.editor_zoom).clamp(0.0, raw_img.height() as f32),
                    ]
                };

                if let Some(pos) = resp.interact_pointer_pos() {
                    let pt = to_img(pos);
                    if resp.drag_started() {
                        if self.active_tool == ToolKind::Select {
                            let p = pos2(pt[0], pt[1]);
                            self.editor_crop_anchor = Some(p);
                            self.editor_crop_rect = Some(Rect::from_two_pos(p, p));
                        } else if self.active_tool == ToolKind::StepBadge {
                            self.history.push(AnnotationItem {
                                kind: ToolKind::StepBadge,
                                points: vec![pt],
                                color: self.active_color.to_array(),
                                stroke_width: self.stroke_width,
                                text: String::new(),
                                step_number: self.next_step_number,
                            });
                            self.next_step_number += 1;
                        } else if self.active_tool == ToolKind::Text {
                            self.pending_text_pos = Some(pt);
                            self.pending_text_buf.clear();
                        } else {
                            self.in_progress_item = Some(AnnotationItem {
                                kind: self.active_tool,
                                points: vec![pt],
                                color: self.active_color.to_array(),
                                stroke_width: self.stroke_width,
                                text: String::new(),
                                step_number: 0,
                            });
                        }
                    } else if resp.dragged() {
                        if self.active_tool == ToolKind::Select {
                            if let Some(anchor) = self.editor_crop_anchor {
                                self.editor_crop_rect = Some(Rect::from_two_pos(anchor, pos2(pt[0], pt[1])));
                            }
                        } else if let Some(ref mut item) = self.in_progress_item {
                            if matches!(item.kind, ToolKind::Pen | ToolKind::Highlighter)
                                || item.points.len() == 1
                            {
                                item.points.push(pt);
                            } else {
                                item.points[1] = pt;
                            }
                        }
                    }
                }

                if resp.drag_stopped() {
                    self.editor_crop_anchor = None;
                    if let Some(item) = self.in_progress_item.take() {
                        self.history.push(item);
                    }
                }

                let clipped = ui.painter().with_clip_rect(canvas_rect);
                paint_annotations_egui(&clipped, &self.history.items, origin, self.editor_zoom);
                if let Some(ref item) = self.in_progress_item {
                    paint_annotations_egui(
                        &clipped,
                        std::slice::from_ref(item),
                        origin,
                        self.editor_zoom,
                    );
                }
                if let Some(cr) = self.editor_crop_rect {
                    let sr = Rect::from_min_max(
                        pos2(origin.x + cr.min.x * self.editor_zoom, origin.y + cr.min.y * self.editor_zoom),
                        pos2(origin.x + cr.max.x * self.editor_zoom, origin.y + cr.max.y * self.editor_zoom),
                    );
                    clipped.rect_stroke(sr, Rounding::ZERO, Stroke::new(1.5, Color32::from_rgb(245, 158, 11)));
                }
            });
        });

        if let Some(pos) = self.pending_text_pos {
            egui::Window::new("Add Text")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    let resp = ui.text_edit_singleline(&mut self.pending_text_buf);
                    resp.request_focus();
                    ui.horizontal(|ui| {
                        if (ui.button("Add").clicked() || ui.input(|i| i.key_pressed(Key::Enter)))
                            && !self.pending_text_buf.trim().is_empty()
                        {
                            self.history.push(AnnotationItem {
                                kind: ToolKind::Text,
                                points: vec![pos],
                                color: self.active_color.to_array(),
                                stroke_width: self.stroke_width,
                                text: self.pending_text_buf.clone(),
                                step_number: 0,
                            });
                            self.pending_text_pos = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_text_pos = None;
                        }
                    });
                });
        }

        if (do_rotate_cw || do_rotate_ccw || do_flip_h || do_flip_v || do_apply_crop)
            && let Some(ref base) = self.working_image
        {
            let mut rendered = render_annotations_to_image(base, &self.history.items);
                if do_rotate_cw {
                    rendered = rotate_90_cw(&rendered);
                } else if do_rotate_ccw {
                    rendered = rotate_90_ccw(&rendered);
                } else if do_flip_h {
                    rendered = flip_horizontal(&rendered);
                } else if do_flip_v {
                    rendered = flip_vertical(&rendered);
                } else if do_apply_crop && let Some(cr) = self.editor_crop_rect {
                    rendered = crop_rgba_image(
                        &rendered,
                        cr.min.x.max(0.0) as u32,
                        cr.min.y.max(0.0) as u32,
                        cr.width().max(1.0) as u32,
                        cr.height().max(1.0) as u32,
                    );
                    self.editor_crop_rect = None;
                }
                self.open_in_editor(ctx, rendered);
        }

        if go_back {
            self.mode = AppMode::Dashboard;
        } else if do_pin
            && let Some(ref base) = self.working_image
        {
            let rendered = render_annotations_to_image(base, &self.history.items);
            self.pin_image(rendered);
        } else if do_copy
            && let Some(ref base) = self.working_image
        {
            let rendered = render_annotations_to_image(base, &self.history.items);
            match copy_image_to_clipboard(&rendered) {
                Ok(()) => self.set_toast("Copied annotated image to clipboard"),
                Err(e) => self.set_toast(format!("Clipboard error: {e}")),
            }
        } else if do_save
            && let Some(ref base) = self.working_image
        {
            let rendered = render_annotations_to_image(base, &self.history.items);
            self.finalize_captured_image(ctx, rendered, "Edited", false);
        } else if do_save_as
            && let Some(ref base) = self.working_image
            && let Some(path) = rfd::FileDialog::new()
                .set_file_name("RustyShot.png")
                .add_filter("PNG", &["png"])
                .add_filter("JPEG", &["jpg", "jpeg"])
                .save_file()
        {
            let rendered = render_annotations_to_image(base, &self.history.items);
            let fmt = if path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
            {
                ImageFormatChoice::Jpeg
            } else {
                ImageFormatChoice::Png
            };
            match save_image_to_file(&rendered, &path, fmt, self.config.jpeg_quality) {
                Ok(()) => self.set_toast(format!("Saved to {}", path.display())),
                Err(e) => self.set_toast(format!("Save failed: {e}")),
            }
        }
    }
}

impl eframe::App for RustyShotApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Color32::from_rgb(15, 18, 26).to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Intercept window close (X) to minimize to tray when enabled
        if !self.force_quit
            && self.config.minimize_to_tray
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }

        // Poll background tray, hotkey & single-instance actions
        TrayController::pump_gtk_events();
        let queued: Vec<TrayAction> = if let Ok(mut q) = self.pending_actions.lock() {
            q.drain(..).collect()
        } else {
            Vec::new()
        };
        for action in queued {
            self.trigger_capture(ctx, action);
        }
        if self.tray.is_some() || self.hotkeys.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        // Poll background FTP upload / test results
        if let Some(ref rx) = self.upload_rx
            && let Ok(res) = rx.try_recv()
        {
            self.upload_rx = None;
            match res {
                Ok(msg) => {
                    self.ftp_test_status = Some(msg.clone());
                    self.set_toast(msg);
                }
                Err(err) => {
                    self.ftp_test_status = Some(err.clone());
                    self.set_toast(err);
                }
            }
        }

        // Execute delayed capture once window has minimized / countdown elapsed
        if let Some((action, when)) = self.pending_capture {
            let now = Instant::now();
            if now >= when {
                self.pending_capture = None;
                self.execute_capture_now(ctx, action);
            } else {
                let rem = when.duration_since(now).as_secs_f32();
                if rem > 0.3 {
                    self.set_toast(format!("Capturing in {:.1}s...", rem));
                }
                ctx.request_repaint_after(Duration::from_millis(40));
            }
        }

        match self.mode {
            AppMode::Dashboard => self.render_dashboard(ctx),
            AppMode::RegionOverlay => self.render_region_overlay(ctx),
            AppMode::Editor => self.render_editor(ctx),
        }

        self.render_pinned_viewports(ctx);
        self.render_recording_floating_bar(ctx);

        // Global Ctrl+V paste image or Drag & Drop image in Dashboard
        if self.mode == AppMode::Dashboard {
            let paste_req = ctx.input(|i| i.modifiers.command && i.key_pressed(Key::V));
            if paste_req {
                match paste_image_from_clipboard() {
                    Ok(img) => {
                        self.open_in_editor(ctx, img);
                        self.set_toast("Pasted image from clipboard into Editor");
                    }
                    Err(e) => self.set_toast(e),
                }
            }
            let dropped_path = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .first()
                    .and_then(|f| f.path.clone())
            });
            if let Some(p) = dropped_path {
                match image::open(&p) {
                    Ok(dyn_img) => {
                        self.open_in_editor(ctx, dyn_img.to_rgba8());
                        self.set_toast(format!("Opened {}", p.file_name().unwrap_or_default().to_string_lossy()));
                    }
                    Err(e) => self.set_toast(format!("Failed to open dropped image: {e}")),
                }
            }
        }

        // Toast notification overlay
        if let Some((ref msg, created)) = self.status_toast {
            if created.elapsed() < Duration::from_secs(4) {
                egui::Area::new(egui::Id::new("rustyshot_toast"))
                    .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -16.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(22, 30, 46))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(56, 189, 248)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).color(Color32::WHITE).size(12.0));
                            });
                    });
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
                self.status_toast = None;
            }
        }

        // Poll tray/hotkeys periodically when idle
        ctx.request_repaint_after(Duration::from_millis(120));
    }
}

fn capture_card_btn(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    accent: Color32,
    width: f32,
) -> egui::Response {
    let desired = vec2(width, 64.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let fill = if resp.hovered() {
        Color32::from_rgb(28, 36, 52)
    } else {
        Color32::from_rgb(21, 27, 38)
    };
    let stroke = if resp.hovered() {
        Stroke::new(1.2, accent)
    } else {
        Stroke::new(1.0, Color32::from_rgb(38, 48, 66))
    };
    ui.painter().rect(rect, Rounding::same(8.0), fill, stroke);
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.5),
        Color32::WHITE,
    );
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.top() + 42.0),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(10.5),
        Color32::from_rgb(148, 163, 184),
    );
    resp
}

fn open_folder_in_os(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

fn resize_handle_positions(r: Rect) -> [(ResizeHandle, Pos2); 8] {
    [
        (ResizeHandle::TopLeft, r.left_top()),
        (ResizeHandle::Top, pos2(r.center().x, r.top())),
        (ResizeHandle::TopRight, r.right_top()),
        (ResizeHandle::Right, pos2(r.right(), r.center().y)),
        (ResizeHandle::BottomRight, r.right_bottom()),
        (ResizeHandle::Bottom, pos2(r.center().x, r.bottom())),
        (ResizeHandle::BottomLeft, r.left_bottom()),
        (ResizeHandle::Left, pos2(r.left(), r.center().y)),
    ]
}

fn hit_test_resize_handle(scr_rect: Rect, pos: Pos2) -> Option<ResizeHandle> {
    const HIT_R: f32 = 8.0;
    for (h, pt) in resize_handle_positions(scr_rect) {
        if pos.distance(pt) <= HIT_R {
            return Some(h);
        }
    }
    None
}

fn handle_cursor_icon(h: ResizeHandle) -> CursorIcon {
    match h {
        ResizeHandle::TopLeft | ResizeHandle::BottomRight => CursorIcon::ResizeNwSe,
        ResizeHandle::TopRight | ResizeHandle::BottomLeft => CursorIcon::ResizeNeSw,
        ResizeHandle::Top | ResizeHandle::Bottom => CursorIcon::ResizeVertical,
        ResizeHandle::Left | ResizeHandle::Right => CursorIcon::ResizeHorizontal,
    }
}

fn apply_resize_handle(orig: Rect, h: ResizeHandle, pt: Pos2, max_w: f32, max_h: f32) -> Rect {
    let mut left = orig.min.x;
    let mut top = orig.min.y;
    let mut right = orig.max.x;
    let mut bottom = orig.max.y;

    let px = pt.x.clamp(0.0, max_w);
    let py = pt.y.clamp(0.0, max_h);

    match h {
        ResizeHandle::TopLeft => {
            left = px;
            top = py;
        }
        ResizeHandle::Top => {
            top = py;
        }
        ResizeHandle::TopRight => {
            right = px;
            top = py;
        }
        ResizeHandle::Right => {
            right = px;
        }
        ResizeHandle::BottomRight => {
            right = px;
            bottom = py;
        }
        ResizeHandle::Bottom => {
            bottom = py;
        }
        ResizeHandle::BottomLeft => {
            left = px;
            bottom = py;
        }
        ResizeHandle::Left => {
            left = px;
        }
    }

    Rect::from_two_pos(pos2(left, top), pos2(right, bottom))
}

fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}
