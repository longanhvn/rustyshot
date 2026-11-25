use crate::storage::ImageFormatChoice;
use arboard::{Clipboard, ImageData};
use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use std::borrow::Cow;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct WindowSummary {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct ScreenCaptureResult {
    pub image: RgbaImage,
    pub origin_x: i32,
    pub origin_y: i32,
    pub scale_factor: f32,
}

pub fn capture_primary_screen() -> Result<ScreenCaptureResult, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("No monitors detected".to_string());
    }
    let primary = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .unwrap_or(&monitors[0]);

    let origin_x = primary.x().unwrap_or(0);
    let origin_y = primary.y().unwrap_or(0);
    let scale_factor = primary.scale_factor().unwrap_or(1.0);
    let img = primary
        .capture_image()
        .map_err(|e| format!("Screen capture failed: {e}"))?;

    Ok(ScreenCaptureResult {
        image: img,
        origin_x,
        origin_y,
        scale_factor,
    })
}

pub fn capture_all_monitors_composite() -> Result<ScreenCaptureResult, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Failed to enumerate monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("No monitors detected".to_string());
    }
    if monitors.len() == 1 {
        return capture_primary_screen();
    }

    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    let mut captured = Vec::with_capacity(monitors.len());

    for m in &monitors {
        let mx = m.x().unwrap_or(0);
        let my = m.y().unwrap_or(0);
        if let Ok(img) = m.capture_image() {
            let mw = img.width() as i32;
            let mh = img.height() as i32;
            min_x = min_x.min(mx);
            min_y = min_y.min(my);
            max_x = max_x.max(mx + mw);
            max_y = max_y.max(my + mh);
            captured.push((mx, my, img));
        }
    }

    if captured.is_empty() {
        return Err("Failed to capture any monitor".to_string());
    }

    let total_w = (max_x - min_x).max(1) as u32;
    let total_h = (max_y - min_y).max(1) as u32;
    let mut canvas: RgbaImage = ImageBuffer::from_pixel(total_w, total_h, Rgba([15, 18, 26, 255]));

    for (mx, my, img) in captured {
        let ox = (mx - min_x) as i64;
        let oy = (my - min_y) as i64;
        image::imageops::overlay(&mut canvas, &img, ox, oy);
    }

    Ok(ScreenCaptureResult {
        image: canvas,
        origin_x: min_x,
        origin_y: min_y,
        scale_factor: 1.0,
    })
}

pub fn list_capturable_windows() -> Result<Vec<WindowSummary>, String> {
    let windows = xcap::Window::all().map_err(|e| format!("Failed to list windows: {e}"))?;
    let mut out = Vec::new();
    for w in windows {
        if w.is_minimized().unwrap_or(false) {
            continue;
        }
        let width = w.width().unwrap_or(0);
        let height = w.height().unwrap_or(0);
        if width < 40 || height < 40 {
            continue;
        }
        let title = w.title().unwrap_or_default();
        let app_name = w.app_name().unwrap_or_default();
        if title.trim().is_empty() && app_name.trim().is_empty() {
            continue;
        }
        if title.contains("RustyShot") {
            continue;
        }
        if let Ok(id) = w.id() {
            out.push(WindowSummary {
                id,
                title,
                app_name,
                x: w.x().unwrap_or(0),
                y: w.y().unwrap_or(0),
                width,
                height,
            });
        }
    }
    Ok(out)
}

pub fn capture_window_by_id(target_id: u32) -> Result<RgbaImage, String> {
    let windows = xcap::Window::all().map_err(|e| format!("Failed to list windows: {e}"))?;
    let win = windows
        .into_iter()
        .find(|w| w.id().ok() == Some(target_id))
        .ok_or_else(|| "Selected window is no longer available".to_string())?;
    win.capture_image()
        .map_err(|e| format!("Window capture failed: {e}"))
}

pub fn copy_image_to_clipboard(img: &RgbaImage) -> Result<(), String> {
    let mut clipboard =
        Clipboard::new().map_err(|e| format!("Failed to open system clipboard: {e}"))?;
    let data = ImageData {
        width: img.width() as usize,
        height: img.height() as usize,
        bytes: Cow::Borrowed(img.as_raw()),
    };
    clipboard
        .set_image(data)
        .map_err(|e| format!("Failed to copy image to clipboard: {e}"))
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard =
        Clipboard::new().map_err(|e| format!("Failed to open system clipboard: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("Failed to copy text to clipboard: {e}"))
}

pub fn paste_image_from_clipboard() -> Result<RgbaImage, String> {
    let mut clipboard =
        Clipboard::new().map_err(|e| format!("Failed to open system clipboard: {e}"))?;
    let img_data = clipboard
        .get_image()
        .map_err(|e| format!("No image in clipboard: {e}"))?;
    RgbaImage::from_raw(
        img_data.width as u32,
        img_data.height as u32,
        img_data.bytes.into_owned(),
    )
    .ok_or_else(|| "Invalid clipboard image buffer".to_string())
}

pub fn save_image_to_file(
    img: &RgbaImage,
    path: &Path,
    fmt: ImageFormatChoice,
    jpeg_quality: u8,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create output folder: {e}"))?;
    }
    match fmt {
        ImageFormatChoice::Png => img
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|e| format!("Failed to save PNG: {e}")),
        ImageFormatChoice::Jpeg => {
            let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            let file = File::create(path).map_err(|e| format!("Failed to create JPEG file: {e}"))?;
            let writer = BufWriter::new(file);
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(writer, jpeg_quality.clamp(10, 100));
            encoder
                .encode_image(&rgb)
                .map_err(|e| format!("Failed to encode JPEG: {e}"))
        }
    }
}

pub fn crop_rgba_image(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    let iw = img.width();
    let ih = img.height();
    if iw == 0 || ih == 0 {
        return img.clone();
    }
    let sx = x.min(iw.saturating_sub(1));
    let sy = y.min(ih.saturating_sub(1));
    let cw = w.max(1).min(iw - sx);
    let ch = h.max(1).min(ih - sy);
    image::imageops::crop_imm(img, sx, sy, cw, ch).to_image()
}

pub fn rotate_90_cw(img: &RgbaImage) -> RgbaImage {
    image::imageops::rotate90(img)
}

pub fn rotate_90_ccw(img: &RgbaImage) -> RgbaImage {
    image::imageops::rotate270(img)
}

pub fn flip_horizontal(img: &RgbaImage) -> RgbaImage {
    image::imageops::flip_horizontal(img)
}

pub fn flip_vertical(img: &RgbaImage) -> RgbaImage {
    image::imageops::flip_vertical(img)
}

pub fn apply_pixelate_region(
    img: &mut RgbaImage,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
    block_size: u32,
) {
    let iw = img.width();
    let ih = img.height();
    let x0 = rx.min(iw);
    let y0 = ry.min(ih);
    let x1 = (rx.saturating_add(rw)).min(iw);
    let y1 = (ry.saturating_add(rh)).min(ih);
    let step = block_size.max(4);

    let mut by = y0;
    while by < y1 {
        let bh = (by + step).min(y1);
        let mut bx = x0;
        while bx < x1 {
            let bw = (bx + step).min(x1);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for py in by..bh {
                for px in bx..bw {
                    let p = img.get_pixel(px, py).0;
                    sum[0] += p[0] as u64;
                    sum[1] += p[1] as u64;
                    sum[2] += p[2] as u64;
                    sum[3] += p[3] as u64;
                    count += 1;
                }
            }
            let denom = count.max(1);
            let avg = Rgba([
                (sum[0] / denom) as u8,
                (sum[1] / denom) as u8,
                (sum[2] / denom) as u8,
                (sum[3] / denom) as u8,
            ]);
            for py in by..bh {
                for px in bx..bw {
                    img.put_pixel(px, py, avg);
                }
            }
            bx = bw;
        }
        by = bh;
    }
}

pub fn apply_blur_region(img: &mut RgbaImage, rx: u32, ry: u32, rw: u32, rh: u32, radius: u32) {
    let iw = img.width();
    let ih = img.height();
    let x0 = rx.min(iw);
    let y0 = ry.min(ih);
    let x1 = (rx.saturating_add(rw)).min(iw);
    let y1 = (ry.saturating_add(rh)).min(ih);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let sub = crop_rgba_image(img, x0, y0, x1 - x0, y1 - y0);
    let sigma = (radius.max(2) as f32) * 1.4;
    let blurred = image::imageops::blur(&sub, sigma);
    image::imageops::replace(img, &blurred, x0 as i64, y0 as i64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crop_clamps_bounds() {
        let img: RgbaImage = ImageBuffer::from_pixel(100, 80, Rgba([10, 20, 30, 255]));
        let cropped = crop_rgba_image(&img, 50, 40, 200, 200);
        assert_eq!(cropped.dimensions(), (50, 40));
    }

    #[test]
    fn test_pixelate_region_averages_blocks() {
        let mut img: RgbaImage = ImageBuffer::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                let val = if (x + y) % 2 == 0 { 0 } else { 200 };
                img.put_pixel(x, y, Rgba([val, val, val, 255]));
            }
        }
        apply_pixelate_region(&mut img, 0, 0, 8, 8, 8);
        assert_eq!(img.get_pixel(0, 0).0, [100, 100, 100, 255]);
        assert_eq!(img.get_pixel(7, 7).0, [100, 100, 100, 255]);
        // Untouched outside region
        assert_ne!(img.get_pixel(15, 15).0, [100, 100, 100, 255]);
    }
}
