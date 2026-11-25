use crate::capture::{apply_blur_region, apply_pixelate_region};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Rounding, Stroke, Vec2, pos2, vec2};
use image::{Rgba, RgbaImage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Select,
    Pen,
    Highlighter,
    Arrow,
    Rect,
    Ellipse,
    StepBadge,
    Text,
    Pixelate,
    Blur,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Pen => "Pen",
            Self::Highlighter => "Marker",
            Self::Arrow => "Arrow",
            Self::Rect => "Rect",
            Self::Ellipse => "Circle",
            Self::StepBadge => "Step 1·2·3",
            Self::Text => "Text",
            Self::Pixelate => "Pixelate",
            Self::Blur => "Blur",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Select => "🖱",
            Self::Pen => "✏",
            Self::Highlighter => "🖍",
            Self::Arrow => "↗",
            Self::Rect => "⬜",
            Self::Ellipse => "⭕",
            Self::StepBadge => "①",
            Self::Text => "T",
            Self::Pixelate => "▦",
            Self::Blur => "🌫",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AnnotationItem {
    pub kind: ToolKind,
    pub points: Vec<[f32; 2]>,
    pub color: [u8; 4],
    pub stroke_width: f32,
    pub text: String,
    pub step_number: u32,
}

pub const PALETTE_COLORS: [Color32; 8] = [
    Color32::from_rgb(239, 68, 68),   // Red
    Color32::from_rgb(245, 158, 11),  // Amber
    Color32::from_rgb(250, 204, 21),  // Yellow
    Color32::from_rgb(34, 197, 94),   // Green
    Color32::from_rgb(56, 189, 248),  // Sky Cyan
    Color32::from_rgb(59, 130, 246),  // Blue
    Color32::from_rgb(168, 85, 247),  // Purple
    Color32::from_rgb(248, 250, 252), // White
];

pub struct AnnotationHistory {
    pub items: Vec<AnnotationItem>,
    pub redo_stack: Vec<AnnotationItem>,
}

impl AnnotationHistory {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn push(&mut self, item: AnnotationItem) {
        self.items.push(item);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(last) = self.items.pop() {
            self.redo_stack.push(last);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(item) = self.redo_stack.pop() {
            self.items.push(item);
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.redo_stack.clear();
    }
}

impl Default for AnnotationHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Paint annotations onto an egui Painter given image->screen transform:
/// screen_pos = origin + image_pos * zoom
pub fn paint_annotations_egui(
    painter: &Painter,
    items: &[AnnotationItem],
    origin: Pos2,
    zoom: f32,
) {
    let to_screen = |pt: [f32; 2]| -> Pos2 { pos2(origin.x + pt[0] * zoom, origin.y + pt[1] * zoom) };

    for item in items {
        let col = Color32::from_rgba_unmultiplied(
            item.color[0],
            item.color[1],
            item.color[2],
            item.color[3],
        );
        let sw = (item.stroke_width * zoom).max(1.2);

        match item.kind {
            ToolKind::Pen => {
                if item.points.len() >= 2 {
                    let pts: Vec<Pos2> = item.points.iter().map(|&p| to_screen(p)).collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(sw, col)));
                } else if let Some(&p) = item.points.first() {
                    painter.circle_filled(to_screen(p), sw * 0.5, col);
                }
            }
            ToolKind::Highlighter => {
                let marker_col = Color32::from_rgba_unmultiplied(
                    item.color[0],
                    item.color[1],
                    item.color[2],
                    95,
                );
                let marker_w = (item.stroke_width * 4.5 * zoom).max(6.0);
                if item.points.len() >= 2 {
                    let pts: Vec<Pos2> = item.points.iter().map(|&p| to_screen(p)).collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(marker_w, marker_col)));
                }
            }
            ToolKind::Arrow => {
                if item.points.len() >= 2 {
                    let a = to_screen(item.points[0]);
                    let b = to_screen(*item.points.last().unwrap());
                    draw_arrow_egui(painter, a, b, sw, col);
                }
            }
            ToolKind::Rect => {
                if item.points.len() >= 2 {
                    let a = to_screen(item.points[0]);
                    let b = to_screen(*item.points.last().unwrap());
                    let rect = Rect::from_two_pos(a, b);
                    painter.rect_stroke(rect, Rounding::same(2.0), Stroke::new(sw, col));
                }
            }
            ToolKind::Ellipse => {
                if item.points.len() >= 2 {
                    let a = to_screen(item.points[0]);
                    let b = to_screen(*item.points.last().unwrap());
                    let rect = Rect::from_two_pos(a, b);
                    painter.add(egui::Shape::ellipse_stroke(
                        rect.center(),
                        rect.size() * 0.5,
                        Stroke::new(sw, col),
                    ));
                }
            }
            ToolKind::StepBadge => {
                if let Some(&p) = item.points.first() {
                    let c = to_screen(p);
                    let r = (13.0 * zoom).clamp(9.0, 26.0);
                    painter.circle_filled(c, r, col);
                    painter.circle_stroke(c, r, Stroke::new(1.5, Color32::WHITE));
                    painter.text(
                        c,
                        Align2::CENTER_CENTER,
                        item.step_number.to_string(),
                        FontId::proportional((12.0 * zoom).clamp(9.0, 22.0)),
                        Color32::WHITE,
                    );
                }
            }
            ToolKind::Text => {
                if let Some(&p) = item.points.first()
                    && !item.text.is_empty()
                {
                    let pos = to_screen(p);
                    let font_sz = (item.stroke_width * 5.0 * zoom).clamp(11.0, 48.0);
                    // Subtle dark shadow for readability
                    painter.text(
                        pos + vec2(1.0, 1.0),
                        Align2::LEFT_TOP,
                        &item.text,
                        FontId::proportional(font_sz),
                        Color32::from_black_alpha(170),
                    );
                    painter.text(
                        pos,
                        Align2::LEFT_TOP,
                        &item.text,
                        FontId::proportional(font_sz),
                        col,
                    );
                }
            }
            ToolKind::Pixelate | ToolKind::Blur => {
                if item.points.len() >= 2 {
                    let a = to_screen(item.points[0]);
                    let b = to_screen(*item.points.last().unwrap());
                    let rect = Rect::from_two_pos(a, b);
                    let fill = if item.kind == ToolKind::Pixelate {
                        Color32::from_rgba_unmultiplied(15, 23, 42, 110)
                    } else {
                        Color32::from_rgba_unmultiplied(30, 41, 59, 95)
                    };
                    painter.rect(
                        rect,
                        Rounding::same(2.0),
                        fill,
                        Stroke::new(1.2, Color32::from_rgb(148, 163, 184)),
                    );
                    let tag = if item.kind == ToolKind::Pixelate {
                        "PIXELATE"
                    } else {
                        "BLUR"
                    };
                    if rect.width() > 44.0 && rect.height() > 16.0 {
                        painter.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            tag,
                            FontId::proportional(10.0),
                            Color32::from_rgb(226, 232, 240),
                        );
                    }
                }
            }
            ToolKind::Select => {}
        }
    }
}

fn draw_arrow_egui(painter: &Painter, a: Pos2, b: Pos2, width: f32, color: Color32) {
    let dir = b - a;
    let len = dir.length();
    if len < 2.0 {
        return;
    }
    painter.line_segment([a, b], Stroke::new(width, color));
    let unit = dir / len;
    let perp = Vec2::new(-unit.y, unit.x);
    let head_len = (width * 4.0).clamp(10.0, 28.0).min(len * 0.65);
    let head_w = head_len * 0.55;
    let tip = b;
    let base = b - unit * head_len;
    let left = base + perp * head_w;
    let right = base - perp * head_w;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        color,
        Stroke::NONE,
    ));
}

/// Render all annotations directly onto a full-resolution RgbaImage for Clipboard / Save.
pub fn render_annotations_to_image(base: &RgbaImage, items: &[AnnotationItem]) -> RgbaImage {
    let mut out = base.clone();

    // First pass: Pixelate & Blur regions so subsequent annotations can draw cleanly on top
    for item in items {
        if item.points.len() < 2 {
            continue;
        }
        let p0 = item.points[0];
        let p1 = *item.points.last().unwrap();
        let min_x = p0[0].min(p1[0]).max(0.0) as u32;
        let min_y = p0[1].min(p1[1]).max(0.0) as u32;
        let max_x = p0[0].max(p1[0]).max(0.0) as u32;
        let max_y = p0[1].max(p1[1]).max(0.0) as u32;
        let w = max_x.saturating_sub(min_x);
        let h = max_y.saturating_sub(min_y);
        if w == 0 || h == 0 {
            continue;
        }
        match item.kind {
            ToolKind::Pixelate => apply_pixelate_region(&mut out, min_x, min_y, w, h, 10),
            ToolKind::Blur => apply_blur_region(&mut out, min_x, min_y, w, h, 6),
            _ => {}
        }
    }

    // Second pass: Vector strokes, shapes, arrows, badges, text
    for item in items {
        let col = Rgba(item.color);
        let radius = (item.stroke_width * 0.5).max(1.0);

        match item.kind {
            ToolKind::Pen => {
                if item.points.len() == 1 {
                    draw_filled_circle(&mut out, item.points[0][0], item.points[0][1], radius, col);
                } else {
                    for win in item.points.windows(2) {
                        draw_thick_line(&mut out, win[0], win[1], radius, col);
                    }
                }
            }
            ToolKind::Highlighter => {
                let marker = Rgba([item.color[0], item.color[1], item.color[2], 95]);
                let mr = (item.stroke_width * 2.25).max(3.0);
                for win in item.points.windows(2) {
                    draw_thick_line(&mut out, win[0], win[1], mr, marker);
                }
            }
            ToolKind::Arrow => {
                if item.points.len() >= 2 {
                    let a = item.points[0];
                    let b = *item.points.last().unwrap();
                    draw_thick_line(&mut out, a, b, radius, col);
                    draw_arrow_head_img(&mut out, a, b, item.stroke_width, col);
                }
            }
            ToolKind::Rect => {
                if item.points.len() >= 2 {
                    let a = item.points[0];
                    let b = *item.points.last().unwrap();
                    let x0 = a[0].min(b[0]);
                    let y0 = a[1].min(b[1]);
                    let x1 = a[0].max(b[0]);
                    let y1 = a[1].max(b[1]);
                    draw_thick_line(&mut out, [x0, y0], [x1, y0], radius, col);
                    draw_thick_line(&mut out, [x1, y0], [x1, y1], radius, col);
                    draw_thick_line(&mut out, [x1, y1], [x0, y1], radius, col);
                    draw_thick_line(&mut out, [x0, y1], [x0, y0], radius, col);
                }
            }
            ToolKind::Ellipse => {
                if item.points.len() >= 2 {
                    let a = item.points[0];
                    let b = *item.points.last().unwrap();
                    let cx = (a[0] + b[0]) * 0.5;
                    let cy = (a[1] + b[1]) * 0.5;
                    let rx = (a[0] - b[0]).abs() * 0.5;
                    let ry = (a[1] - b[1]).abs() * 0.5;
                    if rx >= 1.0 && ry >= 1.0 {
                        let steps = ((rx + ry) * 2.0).clamp(32.0, 720.0) as usize;
                        for i in 0..steps {
                            let t0 = (i as f32 / steps as f32) * std::f32::consts::TAU;
                            let t1 = ((i + 1) as f32 / steps as f32) * std::f32::consts::TAU;
                            let p0 = [cx + rx * t0.cos(), cy + ry * t0.sin()];
                            let p1 = [cx + rx * t1.cos(), cy + ry * t1.sin()];
                            draw_thick_line(&mut out, p0, p1, radius, col);
                        }
                    }
                }
            }
            ToolKind::StepBadge => {
                if let Some(&p) = item.points.first() {
                    let r = 13.0f32;
                    draw_filled_circle(&mut out, p[0], p[1], r, col);
                    draw_circle_outline(&mut out, p[0], p[1], r, 1.5, Rgba([255, 255, 255, 255]));
                    let num_str = item.step_number.to_string();
                    let scale = 2;
                    let text_w = (num_str.len() as f32) * 6.0 * (scale as f32) - (scale as f32);
                    let text_h = 7.0 * (scale as f32);
                    draw_bitmap_text(
                        &mut out,
                        p[0] - text_w * 0.5,
                        p[1] - text_h * 0.5,
                        &num_str,
                        scale,
                        Rgba([255, 255, 255, 255]),
                    );
                }
            }
            ToolKind::Text => {
                if let Some(&p) = item.points.first()
                    && !item.text.is_empty()
                {
                    let scale = ((item.stroke_width * 0.85).round() as i32).clamp(2, 6);
                    draw_bitmap_text(
                        &mut out,
                        p[0] + 1.5,
                        p[1] + 1.5,
                        &item.text,
                        scale,
                        Rgba([0, 0, 0, 180]),
                    );
                    draw_bitmap_text(&mut out, p[0], p[1], &item.text, scale, col);
                }
            }
            ToolKind::Pixelate | ToolKind::Blur | ToolKind::Select => {}
        }
    }

    out
}

fn blend_pixel(img: &mut RgbaImage, x: i32, y: i32, src: Rgba<u8>) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let dst = img.get_pixel(x as u32, y as u32).0;
    let sa = src.0[3] as f32 / 255.0;
    let da = 1.0 - sa;
    let out = Rgba([
        (src.0[0] as f32 * sa + dst[0] as f32 * da).round() as u8,
        (src.0[1] as f32 * sa + dst[1] as f32 * da).round() as u8,
        (src.0[2] as f32 * sa + dst[2] as f32 * da).round() as u8,
        (dst[3] as f32 + (255.0 - dst[3] as f32) * sa).round() as u8,
    ]);
    img.put_pixel(x as u32, y as u32, out);
}

fn draw_filled_circle(img: &mut RgbaImage, cx: f32, cy: f32, r: f32, col: Rgba<u8>) {
    let min_x = (cx - r - 1.0).floor() as i32;
    let max_x = (cx + r + 1.0).ceil() as i32;
    let min_y = (cy - r - 1.0).floor() as i32;
    let max_y = (cy + r + 1.0).ceil() as i32;
    let r2 = r * r;
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let dx = px as f32 - cx;
            let dy = py as f32 - cy;
            if dx * dx + dy * dy <= r2 {
                blend_pixel(img, px, py, col);
            }
        }
    }
}

fn draw_circle_outline(
    img: &mut RgbaImage,
    cx: f32,
    cy: f32,
    r: f32,
    thickness: f32,
    col: Rgba<u8>,
) {
    let min_x = (cx - r - thickness - 1.0).floor() as i32;
    let max_x = (cx + r + thickness + 1.0).ceil() as i32;
    let min_y = (cy - r - thickness - 1.0).floor() as i32;
    let max_y = (cy + r + thickness + 1.0).ceil() as i32;
    let inner = (r - thickness * 0.5).max(0.0);
    let outer = r + thickness * 0.5;
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let dist = ((px as f32 - cx).powi(2) + (py as f32 - cy).powi(2)).sqrt();
            if dist >= inner && dist <= outer {
                blend_pixel(img, px, py, col);
            }
        }
    }
}

fn draw_thick_line(img: &mut RgbaImage, a: [f32; 2], b: [f32; 2], radius: f32, col: Rgba<u8>) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        draw_filled_circle(img, a[0], a[1], radius, col);
        return;
    }
    let steps = (len * 1.5).ceil().max(1.0) as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let px = a[0] + dx * t;
        let py = a[1] + dy * t;
        draw_filled_circle(img, px, py, radius, col);
    }
}

fn draw_arrow_head_img(img: &mut RgbaImage, a: [f32; 2], b: [f32; 2], width: f32, col: Rgba<u8>) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();
    if len < 2.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let px = -uy;
    let py = ux;
    let head_len = (width * 4.0).clamp(10.0, 28.0).min(len * 0.65);
    let head_w = head_len * 0.55;
    let base_x = b[0] - ux * head_len;
    let base_y = b[1] - uy * head_len;
    let left = [base_x + px * head_w, base_y + py * head_w];
    let right = [base_x - px * head_w, base_y - py * head_w];
    fill_triangle(img, b, left, right, col);
}

fn fill_triangle(img: &mut RgbaImage, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], col: Rgba<u8>) {
    let min_x = p0[0].min(p1[0]).min(p2[0]).floor() as i32;
    let max_x = p0[0].max(p1[0]).max(p2[0]).ceil() as i32;
    let min_y = p0[1].min(p1[1]).min(p2[1]).floor() as i32;
    let max_y = p0[1].max(p1[1]).max(p2[1]).ceil() as i32;

    let edge = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| -> f32 {
        (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
    };
    let area = edge(p0, p1, p2);
    if area.abs() < 0.001 {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let w0 = edge(p1, p2, p);
            let w1 = edge(p2, p0, p);
            let w2 = edge(p0, p1, p);
            if (area > 0.0 && w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0)
                || (area < 0.0 && w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0)
            {
                blend_pixel(img, x, y, col);
            }
        }
    }
}

fn draw_bitmap_text(
    img: &mut RgbaImage,
    x: f32,
    y: f32,
    text: &str,
    scale: i32,
    col: Rgba<u8>,
) {
    let mut cx = x.round() as i32;
    let cy = y.round() as i32;
    let s = scale.max(1);
    for ch in text.chars() {
        if ch == '\n' {
            cx = x.round() as i32;
            continue;
        }
        let glyph = glyph_5x7(ch);
        for (row_idx, &bits) in glyph.iter().enumerate() {
            for col_idx in 0..5 {
                if (bits >> (4 - col_idx)) & 1 == 1 {
                    for dy in 0..s {
                        for dx in 0..s {
                            blend_pixel(
                                img,
                                cx + col_idx * s + dx,
                                cy + (row_idx as i32) * s + dy,
                                col,
                            );
                        }
                    }
                }
            }
        }
        cx += 6 * s;
    }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x08],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageBuffer;

    #[test]
    fn test_annotation_history_undo_redo() {
        let mut hist = AnnotationHistory::new();
        assert!(!hist.undo());
        hist.push(AnnotationItem {
            kind: ToolKind::Rect,
            points: vec![[10.0, 10.0], [50.0, 50.0]],
            color: [255, 0, 0, 255],
            stroke_width: 3.0,
            text: String::new(),
            step_number: 0,
        });
        assert_eq!(hist.items.len(), 1);
        assert!(hist.undo());
        assert_eq!(hist.items.len(), 0);
        assert!(hist.redo());
        assert_eq!(hist.items.len(), 1);
    }

    #[test]
    fn test_render_annotations_draws_arrow_and_badge() {
        let base: RgbaImage = ImageBuffer::from_pixel(100, 100, Rgba([0, 0, 0, 255]));
        let items = vec![
            AnnotationItem {
                kind: ToolKind::Arrow,
                points: vec![[10.0, 10.0], [80.0, 10.0]],
                color: [255, 0, 0, 255],
                stroke_width: 3.0,
                text: String::new(),
                step_number: 0,
            },
            AnnotationItem {
                kind: ToolKind::StepBadge,
                points: vec![[50.0, 50.0]],
                color: [56, 189, 248, 255],
                stroke_width: 3.0,
                text: String::new(),
                step_number: 1,
            },
        ];
        let rendered = render_annotations_to_image(&base, &items);
        assert_eq!(rendered.get_pixel(20, 10).0, [255, 0, 0, 255]);
        assert_ne!(rendered.get_pixel(50, 50).0, [0, 0, 0, 255]);
    }
}
