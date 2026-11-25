use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    CaptureRegion,
    CaptureFullscreen,
    CaptureWindow,
    ToggleRecord,
    ColorPicker,
    OpenFolder,
    ShowApp,
    Quit,
}

pub fn generate_app_icon_rgba() -> (Vec<u8>, u32, u32) {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) * 0.5;

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let fx = x as f32;
            let fy = y as f32;
            let dx = (fx - center).abs();
            let dy = (fy - center).abs();

            // Rounded square mask
            let corner_r = 6.0f32;
            let half = 14.5f32;
            let inside = if dx > half - corner_r && dy > half - corner_r {
                let cx = dx - (half - corner_r);
                let cy = dy - (half - corner_r);
                (cx * cx + cy * cy).sqrt() <= corner_r
            } else {
                dx <= half && dy <= half
            };

            if !inside {
                continue;
            }

            // Dark slate background
            let mut r = 18u8;
            let mut g = 24u8;
            let mut b = 38u8;

            let dist = ((fx - center).powi(2) + (fy - center).powi(2)).sqrt();

            // Outer cyan viewfinder corners
            let in_bracket_x = (6..=11).contains(&x) || (20..=25).contains(&x);
            let in_bracket_y = (6..=11).contains(&y) || (20..=25).contains(&y);
            let on_edge_h = (y == 6 || y == 7 || y == 24 || y == 25) && in_bracket_x;
            let on_edge_v = (x == 6 || x == 7 || x == 24 || x == 25) && in_bracket_y;

            if on_edge_h || on_edge_v {
                r = 56;
                g = 189;
                b = 248;
            } else if (dist - 5.2).abs() <= 1.1 {
                // Bright amber/orange camera lens ring
                r = 249;
                g = 115;
                b = 22;
            } else if dist <= 2.3 {
                // White shutter center dot
                r = 248;
                g = 250;
                b = 252;
            }

            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255;
        }
    }

    (rgba, size, size)
}

pub struct TrayController {
    _tray: TrayIcon,
    id_region: MenuId,
    id_fullscreen: MenuId,
    id_window: MenuId,
    id_record: MenuId,
    id_color_picker: MenuId,
    id_open_folder: MenuId,
    id_show: MenuId,
    id_quit: MenuId,
}

impl TrayController {
    pub fn try_new() -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            if gtk::init().is_err() {
                return Err("GTK init failed; system tray disabled".to_string());
            }
        }
        let menu = Menu::new();
        let item_region = MenuItem::new("Capture Region", true, None);
        let item_fullscreen = MenuItem::new("Capture Fullscreen", true, None);
        let item_window = MenuItem::new("Capture Window", true, None);
        let item_record = MenuItem::new("Start / Stop Recording", true, None);
        let item_color_picker = MenuItem::new("Screen Color Picker", true, None);
        let item_folder = MenuItem::new("Open Screenshots Folder", true, None);
        let item_show = MenuItem::new("Show RustyShot", true, None);
        let item_quit = MenuItem::new("Quit RustyShot", true, None);

        let id_region = item_region.id().clone();
        let id_fullscreen = item_fullscreen.id().clone();
        let id_window = item_window.id().clone();
        let id_record = item_record.id().clone();
        let id_color_picker = item_color_picker.id().clone();
        let id_open_folder = item_folder.id().clone();
        let id_show = item_show.id().clone();
        let id_quit = item_quit.id().clone();

        let _ = menu.append(&item_region);
        let _ = menu.append(&item_fullscreen);
        let _ = menu.append(&item_window);
        let _ = menu.append(&item_record);
        let _ = menu.append(&item_color_picker);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&item_folder);
        let _ = menu.append(&item_show);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&item_quit);

        let (rgba, w, h) = generate_app_icon_rgba();
        let icon = Icon::from_rgba(rgba, w, h).map_err(|e| format!("Tray icon error: {e}"))?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("RustyShot - Fast Screenshot, Recorder & Annotation")
            .with_icon(icon)
            .build()
            .map_err(|e| format!("Failed to create tray icon: {e}"))?;

        Ok(Self {
            _tray: tray,
            id_region,
            id_fullscreen,
            id_window,
            id_record,
            id_color_picker,
            id_open_folder,
            id_show,
            id_quit,
        })
    }
}

#[derive(Clone)]
pub struct TrayIdMap {
    id_region: MenuId,
    id_fullscreen: MenuId,
    id_window: MenuId,
    id_record: MenuId,
    id_color_picker: MenuId,
    id_open_folder: MenuId,
    id_show: MenuId,
    id_quit: MenuId,
}

impl TrayIdMap {
    pub fn action_for(&self, id: &MenuId) -> Option<TrayAction> {
        if *id == self.id_region {
            Some(TrayAction::CaptureRegion)
        } else if *id == self.id_fullscreen {
            Some(TrayAction::CaptureFullscreen)
        } else if *id == self.id_window {
            Some(TrayAction::CaptureWindow)
        } else if *id == self.id_record {
            Some(TrayAction::ToggleRecord)
        } else if *id == self.id_color_picker {
            Some(TrayAction::ColorPicker)
        } else if *id == self.id_open_folder {
            Some(TrayAction::OpenFolder)
        } else if *id == self.id_show {
            Some(TrayAction::ShowApp)
        } else if *id == self.id_quit {
            Some(TrayAction::Quit)
        } else {
            None
        }
    }
}

#[derive(Clone, Default)]
pub struct HotkeyIdMap {
    pub region_id: Option<u32>,
    pub fullscreen_id: Option<u32>,
    pub record_id: Option<u32>,
}

impl HotkeyIdMap {
    pub fn action_for(&self, id: u32) -> Option<TrayAction> {
        if self.region_id == Some(id) {
            Some(TrayAction::CaptureRegion)
        } else if self.fullscreen_id == Some(id) {
            Some(TrayAction::CaptureFullscreen)
        } else if self.record_id == Some(id) {
            Some(TrayAction::ToggleRecord)
        } else {
            None
        }
    }
}

pub fn poll_background_tray_and_hotkeys(
    tray_ids: Option<&TrayIdMap>,
    hk_ids: Option<&Arc<Mutex<HotkeyIdMap>>>,
) -> Vec<TrayAction> {
    let mut actions = Vec::new();
    if let Some(tmap) = tray_ids {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some(act) = tmap.action_for(&ev.id) {
                actions.push(act);
            }
        }
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                ev,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                actions.push(TrayAction::ShowApp);
            }
        }
    }
    if let Some(hk_lock) = hk_ids
        && let Ok(map) = hk_lock.lock()
    {
        while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
            if ev.state == HotKeyState::Pressed
                && let Some(act) = map.action_for(ev.id)
            {
                actions.push(act);
            }
        }
    }
    actions
}

impl TrayController {
    pub fn id_map(&self) -> TrayIdMap {
        TrayIdMap {
            id_region: self.id_region.clone(),
            id_fullscreen: self.id_fullscreen.clone(),
            id_window: self.id_window.clone(),
            id_record: self.id_record.clone(),
            id_color_picker: self.id_color_picker.clone(),
            id_open_folder: self.id_open_folder.clone(),
            id_show: self.id_show.clone(),
            id_quit: self.id_quit.clone(),
        }
    }

    pub fn pump_gtk_events() {
        #[cfg(target_os = "linux")]
        {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        }
    }
}

pub struct HotkeyController {
    manager: GlobalHotKeyManager,
    region_hk: Option<HotKey>,
    fullscreen_hk: Option<HotKey>,
    record_hk: Option<HotKey>,
    shared_ids: Arc<Mutex<HotkeyIdMap>>,
}

impl HotkeyController {
    pub fn try_new(
        region_spec: &str,
        fullscreen_spec: &str,
        record_spec: &str,
    ) -> Result<(Self, String), String> {
        let manager =
            GlobalHotKeyManager::new().map_err(|e| format!("Global hotkey unavailable: {e}"))?;
        let mut ctrl = Self {
            manager,
            region_hk: None,
            fullscreen_hk: None,
            record_hk: None,
            shared_ids: Arc::new(Mutex::new(HotkeyIdMap::default())),
        };
        let status = ctrl.reregister(region_spec, fullscreen_spec, record_spec);
        Ok((ctrl, status))
    }

    pub fn shared_ids(&self) -> Arc<Mutex<HotkeyIdMap>> {
        Arc::clone(&self.shared_ids)
    }

    pub fn reregister(
        &mut self,
        region_spec: &str,
        fullscreen_spec: &str,
        record_spec: &str,
    ) -> String {
        if let Some(old) = self.region_hk.take() {
            let _ = self.manager.unregister(old);
        }
        if let Some(old) = self.fullscreen_hk.take() {
            let _ = self.manager.unregister(old);
        }
        if let Some(old) = self.record_hk.take() {
            let _ = self.manager.unregister(old);
        }

        let mut notes = Vec::new();
        if let Some(hk) = parse_hotkey_spec(region_spec) {
            match self.manager.register(hk) {
                Ok(_) => {
                    self.region_hk = Some(hk);
                    notes.push(format!("Region: {region_spec}"));
                }
                Err(e) => {
                    let short = if e.to_string().contains("already registered") {
                        "in use"
                    } else {
                        "unavailable"
                    };
                    notes.push(format!("Region ({region_spec}): {short}"));
                }
            }
        }
        if let Some(hk) = parse_hotkey_spec(fullscreen_spec) {
            match self.manager.register(hk) {
                Ok(_) => {
                    self.fullscreen_hk = Some(hk);
                    notes.push(format!("Fullscreen: {fullscreen_spec}"));
                }
                Err(e) => {
                    let short = if e.to_string().contains("already registered") {
                        "in use"
                    } else {
                        "unavailable"
                    };
                    notes.push(format!("Fullscreen ({fullscreen_spec}): {short}"));
                }
            }
        }
        if let Some(hk) = parse_hotkey_spec(record_spec) {
            match self.manager.register(hk) {
                Ok(_) => {
                    self.record_hk = Some(hk);
                    notes.push(format!("Record: {record_spec}"));
                }
                Err(e) => {
                    let short = if e.to_string().contains("already registered") {
                        "in use"
                    } else {
                        "unavailable"
                    };
                    notes.push(format!("Record ({record_spec}): {short}"));
                }
            }
        }
        if let Ok(mut ids) = self.shared_ids.lock() {
            ids.region_id = self.region_hk.map(|h| h.id());
            ids.fullscreen_id = self.fullscreen_hk.map(|h| h.id());
            ids.record_id = self.record_hk.map(|h| h.id());
        }
        if notes.is_empty() {
            "No global hotkeys registered".to_string()
        } else {
            notes.join(" · ")
        }
    }
}

pub fn parse_hotkey_spec(spec: &str) -> Option<HotKey> {
    let parts: Vec<String> = spec
        .split('+')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }

    let mut mods = Modifiers::empty();
    let mut key_code: Option<Code> = None;

    for part in parts {
        match part.as_str() {
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" => mods |= Modifiers::ALT,
            "super" | "cmd" | "win" | "meta" => mods |= Modifiers::META,
            "printscreen" | "prtsc" | "prtscr" => key_code = Some(Code::PrintScreen),
            other => {
                let candidate = if other.len() == 1 {
                    let ch = other.chars().next().unwrap().to_ascii_uppercase();
                    if ch.is_ascii_alphabetic() {
                        format!("Key{ch}")
                    } else if ch.is_ascii_digit() {
                        format!("Digit{ch}")
                    } else {
                        other.to_string()
                    }
                } else if let Some(rest) = other.strip_prefix('f')
                    && rest.chars().all(|c| c.is_ascii_digit())
                {
                    format!("F{rest}")
                } else {
                    other.to_string()
                };
                if let Ok(code) = Code::from_str(&candidate) {
                    key_code = Some(code);
                }
            }
        }
    }

    let code = key_code?;
    let mods_opt = if mods.is_empty() { None } else { Some(mods) };
    Some(HotKey::new(mods_opt, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hotkey_spec() {
        assert!(parse_hotkey_spec("Ctrl+Shift+S").is_some());
        assert!(parse_hotkey_spec("PrintScreen").is_some());
        assert!(parse_hotkey_spec("Alt+F1").is_some());
        assert!(parse_hotkey_spec("").is_none());
    }

    #[test]
    fn test_icon_dimensions() {
        let (rgba, w, h) = generate_app_icon_rgba();
        assert_eq!(w, 32);
        assert_eq!(h, 32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
    }
}
