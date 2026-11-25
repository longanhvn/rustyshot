use crate::capture::{capture_all_monitors_composite, crop_rgba_image};
use crate::storage::VideoFormatChoice;
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};
use mp4::{AvcConfig, Bytes, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig, TrackType};
use openh264::encoder::{Encoder, EncoderConfig, FrameRate};
use openh264::formats::{RgbSliceU8, YUVBuffer};
use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
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

pub fn start_recording(
    output_path: PathBuf,
    format: VideoFormatChoice,
    fps: u32,
    region: Option<[u32; 4]>,
) -> Result<RecordingHandle, String> {
    let fps = fps.clamp(5, 30);

    // Grab initial frame to determine exact dimensions
    let first_cap = capture_all_monitors_composite()?;
    let first_img = crop_optional(&first_cap.image, region);
    let raw_w = first_img.width().max(2);
    let raw_h = first_img.height().max(2);
    // H.264 YUV420p requires even dimensions
    let width = (raw_w & !1).max(2);
    let height = (raw_h & !1).max(2);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let frame_count = Arc::new(AtomicU64::new(0));
    let stop_bg = Arc::clone(&stop_flag);
    let frames_bg = Arc::clone(&frame_count);
    let out_bg = output_path.clone();

    let join_handle = thread::spawn(move || -> Result<RecordingResult, String> {
        let interval = Duration::from_secs_f64(1.0 / fps as f64);

        match format {
            VideoFormatChoice::Mp4 => {
                let file = File::create(&out_bg)
                    .map_err(|e| format!("Failed to create MP4 file: {e}"))?;
                let writer = BufWriter::new(file);
                let mut mp4_rec = BuiltInMp4Writer::new(writer, width, height, fps)?;
                mp4_rec.write_rgba_frame(&first_img)?;
                frames_bg.fetch_add(1, Ordering::Relaxed);

                while !stop_bg.load(Ordering::Relaxed) {
                    let tick = Instant::now();
                    if let Ok(cap) = capture_all_monitors_composite() {
                        let frame_img = crop_optional(&cap.image, region);
                        if mp4_rec.write_rgba_frame(&frame_img).is_err() {
                            break;
                        }
                        frames_bg.fetch_add(1, Ordering::Relaxed);
                    }
                    let spent = tick.elapsed();
                    if spent < interval {
                        thread::sleep(interval - spent);
                    }
                }
                mp4_rec.finish()?;
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

struct BuiltInMp4Writer<W: Write + Seek> {
    encoder: Encoder,
    mp4: Option<Mp4Writer<W>>,
    pending_writer: Option<W>,
    width: u32,
    height: u32,
    frame_duration_ms: u32,
    frame_idx: u64,
    rgb_scratch: Vec<u8>,
}

impl<W: Write + Seek> BuiltInMp4Writer<W> {
    fn new(writer: W, width: u32, height: u32, fps: u32) -> Result<Self, String> {
        let cfg = EncoderConfig::new()
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .bitrate(openh264::encoder::BitRate::from_bps(4_000_000))
            .skip_frames(false);
        let api = openh264::OpenH264API::from_source();
        let encoder = Encoder::with_api_config(api, cfg)
            .map_err(|e| format!("Failed to initialize built-in H.264 encoder: {e}"))?;
        let frame_duration_ms = (1000 / fps.max(1)).max(1);
        Ok(Self {
            encoder,
            mp4: None,
            pending_writer: Some(writer),
            width,
            height,
            frame_duration_ms,
            frame_idx: 0,
            rgb_scratch: Vec::with_capacity((width * height * 3) as usize),
        })
    }

    fn write_rgba_frame(&mut self, img: &RgbaImage) -> Result<(), String> {
        let normalized;
        let src = if img.width() == self.width && img.height() == self.height {
            img
        } else {
            normalized = image::imageops::resize(
                img,
                self.width,
                self.height,
                image::imageops::FilterType::Nearest,
            );
            &normalized
        };

        self.rgb_scratch.clear();
        for px in src.as_raw().chunks_exact(4) {
            self.rgb_scratch.push(px[0]);
            self.rgb_scratch.push(px[1]);
            self.rgb_scratch.push(px[2]);
        }

        let rgb_slice = RgbSliceU8::new(
            &self.rgb_scratch,
            (self.width as usize, self.height as usize),
        );
        let yuv = YUVBuffer::from_rgb_source(rgb_slice);
        let bitstream = self
            .encoder
            .encode(&yuv)
            .map_err(|e| format!("H.264 encode failed: {e}"))?;
        let annex_b = bitstream.to_vec();

        let nals = split_annex_b_nals(&annex_b);
        if nals.is_empty() {
            return Ok(());
        }

        let mut sps: Option<Vec<u8>> = None;
        let mut pps: Option<Vec<u8>> = None;
        let mut is_sync = false;
        let mut avcc_sample = Vec::new();

        for nal in nals {
            if nal.is_empty() {
                continue;
            }
            let nal_type = nal[0] & 0x1F;
            match nal_type {
                7 => sps = Some(nal.to_vec()),
                8 => pps = Some(nal.to_vec()),
                _ => {
                    if nal_type == 5 {
                        is_sync = true;
                    }
                    let len = (nal.len() as u32).to_be_bytes();
                    avcc_sample.extend_from_slice(&len);
                    avcc_sample.extend_from_slice(nal);
                }
            }
        }

        if self.mp4.is_none()
            && let (Some(sps_bytes), Some(pps_bytes)) = (sps, pps)
            && let Some(raw_writer) = self.pending_writer.take()
        {
            let mp4_cfg = Mp4Config {
                major_brand: FromStr::from_str("isom").map_err(|e| format!("{e}"))?,
                minor_version: 512,
                compatible_brands: vec![
                    FromStr::from_str("isom").map_err(|e| format!("{e}"))?,
                    FromStr::from_str("iso2").map_err(|e| format!("{e}"))?,
                    FromStr::from_str("avc1").map_err(|e| format!("{e}"))?,
                    FromStr::from_str("mp41").map_err(|e| format!("{e}"))?,
                ],
                timescale: 1000,
            };
            let mut mp4_writer = Mp4Writer::write_start(raw_writer, &mp4_cfg)
                .map_err(|e| format!("MP4 header write failed: {e}"))?;
            let track_cfg = TrackConfig {
                track_type: TrackType::Video,
                timescale: 1000,
                language: "und".to_string(),
                media_conf: MediaConfig::AvcConfig(AvcConfig {
                    width: self.width as u16,
                    height: self.height as u16,
                    seq_param_set: sps_bytes,
                    pic_param_set: pps_bytes,
                }),
            };
            mp4_writer
                .add_track(&track_cfg)
                .map_err(|e| format!("MP4 add_track failed: {e}"))?;
            self.mp4 = Some(mp4_writer);
        }

        if !avcc_sample.is_empty()
            && let Some(ref mut mp4_writer) = self.mp4
        {
            let sample = Mp4Sample {
                start_time: self.frame_idx * self.frame_duration_ms as u64,
                duration: self.frame_duration_ms,
                rendering_offset: 0,
                is_sync,
                bytes: Bytes::from(avcc_sample),
            };
            mp4_writer
                .write_sample(1, &sample)
                .map_err(|e| format!("MP4 write_sample failed: {e}"))?;
            self.frame_idx += 1;
        }

        Ok(())
    }

    fn finish(mut self) -> Result<(), String> {
        if let Some(mut mp4_writer) = self.mp4.take() {
            mp4_writer
                .write_end()
                .map_err(|e| format!("MP4 finalize failed: {e}"))?;
        }
        Ok(())
    }
}

fn split_annex_b_nals(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut i = 0;
    let mut start: Option<usize> = None;
    while i < data.len() {
        let sc_len = if i + 4 <= data.len() && data[i..i + 4] == [0, 0, 0, 1] {
            4
        } else if i + 3 <= data.len() && data[i..i + 3] == [0, 0, 1] {
            3
        } else {
            0
        };
        if sc_len > 0 {
            if let Some(s) = start
                && i > s
            {
                nals.push(&data[s..i]);
            }
            i += sc_len;
            start = Some(i);
        } else {
            i += 1;
        }
    }
    if let Some(s) = start
        && data.len() > s
    {
        nals.push(&data[s..]);
    }
    nals
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_builtin_mp4_encoding_writes_valid_mp4() {
        let buf = Cursor::new(Vec::new());
        let mut rec = BuiltInMp4Writer::new(buf, 64, 64, 15).expect("encoder init");
        let img = RgbaImage::from_pixel(64, 64, image::Rgba([56, 189, 248, 255]));
        for _ in 0..3 {
            rec.write_rgba_frame(&img).expect("write frame");
        }
        let mut mp4_writer = rec.mp4.take().expect("mp4 writer initialized");
        mp4_writer.write_end().expect("finalize");
        let out = mp4_writer.into_writer().into_inner();
        assert!(out.len() > 200);
        assert_eq!(&out[4..8], b"ftyp");
    }
}
