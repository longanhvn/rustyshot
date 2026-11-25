use crate::capture::{capture_all_monitors_composite, crop_rgba_image};
use crate::storage::VideoFormatChoice;
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct RecordingResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub frames: u64,
    pub format: VideoFormatChoice,
}

pub struct RecordingHandle {
    stop_flag: Arc<AtomicBool>,
    frame_count: Arc<AtomicU64>,
    started_at: Instant,
    output_path: PathBuf,
    format: VideoFormatChoice,
    join_handle: Option<JoinHandle<Result<RecordingResult, String>>>,
}

impl RecordingHandle {
    pub fn frames(&self) -> u64 {
        self.frame_count.load(Ordering::Relaxed)
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn format(&self) -> VideoFormatChoice {
        self.format
    }

    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    pub fn stop(mut self) -> Result<RecordingResult, String> {
        self.stop_flag.store(true, Ordering::Relaxed);
        match self.join_handle.take() {
            Some(h) => h
                .join()
                .map_err(|_| "Recorder thread panicked".to_string())?,
            None => Err("Recorder already stopped".to_string()),
        }
    }
}

pub fn detect_ffmpeg_path() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let beside = dir.join(exe_name);
        if beside.exists() {
            return Some(beside);
        }
    }
    let probe = Command::new(exe_name)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if probe.is_ok_and(|s| s.success()) {
        return Some(PathBuf::from(exe_name));
    }
    None
}

pub fn start_recording(
    output_path: PathBuf,
    format: VideoFormatChoice,
    fps: u32,
    region: Option<[u32; 4]>,
) -> Result<RecordingHandle, String> {
    let fps = fps.clamp(5, 30);
    let ffmpeg_bin = if format == VideoFormatChoice::Mp4 {
        Some(detect_ffmpeg_path().ok_or_else(|| {
            "FFmpeg not found! Install ffmpeg (or place ffmpeg.exe next to RustyShot) or switch Video Format to GIF in Settings.".to_string()
        })?)
    } else {
        None
    };

    // Grab initial frame to determine exact dimensions
    let first_cap = capture_all_monitors_composite()?;
    let first_img = crop_optional(&first_cap.image, region);
    let width = first_img.width().max(2);
    let height = first_img.height().max(2);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let frame_count = Arc::new(AtomicU64::new(0));
    let stop_bg = Arc::clone(&stop_flag);
    let frames_bg = Arc::clone(&frame_count);
    let out_bg = output_path.clone();

    let join_handle = thread::spawn(move || -> Result<RecordingResult, String> {
        let interval = Duration::from_secs_f64(1.0 / fps as f64);

        match format {
            VideoFormatChoice::Mp4 => {
                let bin = ffmpeg_bin.expect("checked above");
                let mut cmd = Command::new(&bin);
                cmd.args([
                    "-y",
                    "-f",
                    "rawvideo",
                    "-pixel_format",
                    "rgba",
                    "-video_size",
                    &format!("{width}x{height}"),
                    "-framerate",
                    &fps.to_string(),
                    "-i",
                    "-",
                    "-vf",
                    "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "ultrafast",
                    "-pix_fmt",
                    "yuv420p",
                    "-movflags",
                    "+faststart",
                ])
                .arg(&out_bg)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());

                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }

                let mut child = cmd
                    .spawn()
                    .map_err(|e| format!("Failed to start ffmpeg: {e}"))?;
                let mut stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| "Failed to open ffmpeg stdin".to_string())?;

                write_raw_frame(&mut stdin, &first_img, width, height)?;
                frames_bg.fetch_add(1, Ordering::Relaxed);

                while !stop_bg.load(Ordering::Relaxed) {
                    let tick = Instant::now();
                    if let Ok(cap) = capture_all_monitors_composite() {
                        let frame_img = crop_optional(&cap.image, region);
                        if write_raw_frame(&mut stdin, &frame_img, width, height).is_err() {
                            break;
                        }
                        frames_bg.fetch_add(1, Ordering::Relaxed);
                    }
                    let spent = tick.elapsed();
                    if spent < interval {
                        thread::sleep(interval - spent);
                    }
                }

                drop(stdin);
                let out = child
                    .wait_with_output()
                    .map_err(|e| format!("FFmpeg wait failed: {e}"))?;
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr);
                    return Err(format!("FFmpeg encoding error: {err}"));
                }
            }
            VideoFormatChoice::Gif => {
                let file = File::create(&out_bg)
                    .map_err(|e| format!("Failed to create GIF file: {e}"))?;
                let writer = BufWriter::new(file);
                let mut encoder = GifEncoder::new_with_speed(writer, 10);
                encoder
                    .set_repeat(Repeat::Infinite)
                    .map_err(|e| format!("GIF repeat error: {e}"))?;

                let delay_ms = (1000 / fps).max(20);
                let delay = Delay::from_numer_denom_ms(delay_ms, 1);

                encoder
                    .encode_frame(Frame::from_parts(first_img.clone(), 0, 0, delay))
                    .map_err(|e| format!("GIF encode error: {e}"))?;
                frames_bg.fetch_add(1, Ordering::Relaxed);

                while !stop_bg.load(Ordering::Relaxed) {
                    let tick = Instant::now();
                    if let Ok(cap) = capture_all_monitors_composite() {
                        let mut frame_img = crop_optional(&cap.image, region);
                        if frame_img.width() != width || frame_img.height() != height {
                            frame_img = image::imageops::resize(
                                &frame_img,
                                width,
                                height,
                                image::imageops::FilterType::Nearest,
                            );
                        }
                        if encoder
                            .encode_frame(Frame::from_parts(frame_img, 0, 0, delay))
                            .is_err()
                        {
                            break;
                        }
                        frames_bg.fetch_add(1, Ordering::Relaxed);
                    }
                    let spent = tick.elapsed();
                    if spent < interval {
                        thread::sleep(interval - spent);
                    }
                }
            }
        }

        Ok(RecordingResult {
            path: out_bg,
            width,
            height,
            frames: frames_bg.load(Ordering::Relaxed),
            format,
        })
    });

    Ok(RecordingHandle {
        stop_flag,
        frame_count,
        started_at: Instant::now(),
        output_path,
        format,
        join_handle: Some(join_handle),
    })
}

fn crop_optional(img: &RgbaImage, region: Option<[u32; 4]>) -> RgbaImage {
    match region {
        Some([x, y, w, h]) => crop_rgba_image(img, x, y, w, h),
        None => img.clone(),
    }
}

fn write_raw_frame<W: Write>(
    writer: &mut W,
    img: &RgbaImage,
    target_w: u32,
    target_h: u32,
) -> Result<(), String> {
    if img.width() == target_w && img.height() == target_h {
        writer
            .write_all(img.as_raw())
            .map_err(|e| format!("Pipe write failed: {e}"))
    } else {
        let resized = image::imageops::resize(
            img,
            target_w,
            target_h,
            image::imageops::FilterType::Nearest,
        );
        writer
            .write_all(resized.as_raw())
            .map_err(|e| format!("Pipe write failed: {e}"))
    }
}
