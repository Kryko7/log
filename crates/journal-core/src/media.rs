use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::path::Path;

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use exif::{In, Reader as ExifReader, Tag, Value as ExifValue};
use hound::{SampleFormat, WavReader};
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader, Limits};
use journal_types::{GeoLocation, PolaroidMeta, VoiceMemoMeta};
use uuid::Uuid;

use crate::error::{Result, VaultError};
use crate::markdown::slugify;

const THUMB_MAX_DIM: u32 = 520;
const WAVEFORM_PEAKS: usize = 64;
const MIN_PEAK: f32 = 0.08;

/// Atomically write bytes to `target` by syncing a sibling temp file and renaming.
pub(crate) fn write_bytes_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_name = format!(
        ".{}.tmp-{}",
        target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file"),
        Uuid::now_v7()
    );
    let tmp_path = target.with_file_name(tmp_name);
    let write_result = (|| -> Result<()> {
        let mut file = File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp_path, target)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    write_result
}

/// Ingest a photograph into `media/photos/`, extract EXIF metadata, and generate a 520px thumbnail.
pub fn ingest_photo(
    vault_root: &Path,
    filename: &str,
    bytes: &[u8],
    caption: Option<String>,
) -> Result<PolaroidMeta> {
    if bytes.is_empty() {
        return Err(VaultError::InvalidInput(
            "Photo payload cannot be empty".to_string(),
        ));
    }

    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode()?;
    let (raw_w, raw_h) = decoded.dimensions();

    let exif_data = extract_exif_metadata(bytes);
    let (width, height) = if matches!(exif_data.orientation, Some(5..=8)) {
        (raw_h, raw_w)
    } else {
        (raw_w, raw_h)
    };

    let (stem, ext) =
        split_sanitized_filename(filename, "photo", "jpg", &["jpg", "jpeg", "png", "webp"]);
    let id = Uuid::now_v7().to_string();
    let saved_name = format!("{id}-{stem}.{ext}");
    let thumb_name = format!("{id}-{stem}.webp");

    let photo_path = vault_root.join("media").join("photos").join(&saved_name);
    write_bytes_atomic(&photo_path, bytes)?;

    let oriented = apply_exif_orientation(decoded, exif_data.orientation);
    let thumb = make_thumbnail(oriented, THUMB_MAX_DIM);
    let thumb_path = vault_root.join(".index").join("thumbs").join(&thumb_name);
    save_thumbnail_atomic(&thumb, &thumb_path)?;

    let rotation_deg = deterministic_tilt(&id);

    Ok(PolaroidMeta {
        id,
        filename: saved_name.clone(),
        url: format!("/vault-media/photos/{saved_name}"),
        thumbnail_url: Some(format!("/vault-thumbs/{thumb_name}")),
        caption,
        taken_at: exif_data.taken_at,
        width: Some(width),
        height: Some(height),
        rotation_deg,
        washi_asset: None,
        location: exif_data.location,
    })
}

/// Ingest an audio recording into `media/audio/` and compute 64 normalized `[0.08..=1.0]` peaks.
pub fn ingest_audio(
    vault_root: &Path,
    filename: &str,
    bytes: &[u8],
    duration_hint: Option<f32>,
    mime_type: Option<String>,
) -> Result<VoiceMemoMeta> {
    if bytes.is_empty() {
        return Err(VaultError::InvalidInput(
            "Audio payload cannot be empty".to_string(),
        ));
    }

    let (stem, ext) = split_sanitized_filename(
        filename,
        "voice-memo",
        "webm",
        &["wav", "webm", "ogg", "opus", "mp3", "m4a", "mp4", "flac"],
    );
    let id = Uuid::now_v7().to_string();
    let saved_name = format!("{id}-{stem}.{ext}");

    let audio_path = vault_root.join("media").join("audio").join(&saved_name);
    write_bytes_atomic(&audio_path, bytes)?;

    let is_wav = ext.eq_ignore_ascii_case("wav")
        || (bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE");

    let (peaks, duration_secs) = if is_wav {
        decode_wav_waveform(bytes)?
    } else {
        let peaks = compressed_blob_waveform(bytes);
        let dur = duration_hint
            .filter(|d| d.is_finite() && *d > 0.0)
            .unwrap_or_else(|| estimate_compressed_duration(bytes.len()));
        (peaks, dur)
    };

    let resolved_mime = mime_type
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| infer_audio_mime(&ext).to_string());

    Ok(VoiceMemoMeta {
        id,
        filename: saved_name.clone(),
        url: format!("/vault-media/audio/{saved_name}"),
        duration_secs,
        peaks,
        recorded_at: Utc::now(),
        transcript: None,
        mime_type: resolved_mime,
    })
}

fn make_thumbnail(img: DynamicImage, max_dim: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    if w > max_dim || h > max_dim {
        img.thumbnail(max_dim, max_dim)
    } else {
        img
    }
}

fn save_thumbnail_atomic(img: &DynamicImage, target: &Path) -> Result<()> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::WebP)?;
    write_bytes_atomic(target, &buf.into_inner())
}

fn apply_exif_orientation(img: DynamicImage, orientation: Option<u32>) -> DynamicImage {
    match orientation {
        Some(2) => img.fliph(),
        Some(3) => img.rotate180(),
        Some(4) => img.flipv(),
        Some(5) => img.rotate90().fliph(),
        Some(6) => img.rotate90(),
        Some(7) => img.rotate270().fliph(),
        Some(8) => img.rotate270(),
        _ => img,
    }
}

#[derive(Default)]
struct ExtractedExif {
    taken_at: Option<DateTime<Utc>>,
    location: Option<GeoLocation>,
    orientation: Option<u32>,
}

fn extract_exif_metadata(bytes: &[u8]) -> ExtractedExif {
    let Ok(exif) = ExifReader::new().read_from_container(&mut Cursor::new(bytes)) else {
        return ExtractedExif::default();
    };

    let orientation = exif
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));

    let offset = exif
        .get_field(Tag::OffsetTimeOriginal, In::PRIMARY)
        .or_else(|| exif.get_field(Tag::OffsetTimeDigitized, In::PRIMARY))
        .or_else(|| exif.get_field(Tag::OffsetTime, In::PRIMARY))
        .and_then(first_ascii_string)
        .and_then(|s| parse_exif_offset(&s));

    let taken_at = exif
        .get_field(Tag::DateTimeOriginal, In::PRIMARY)
        .or_else(|| exif.get_field(Tag::DateTimeDigitized, In::PRIMARY))
        .or_else(|| exif.get_field(Tag::DateTime, In::PRIMARY))
        .and_then(first_ascii_string)
        .and_then(|raw| parse_exif_datetime(&raw, offset));

    let lat = parse_gps_coord(
        exif.get_field(Tag::GPSLatitude, In::PRIMARY),
        exif.get_field(Tag::GPSLatitudeRef, In::PRIMARY),
        'S',
    );
    let lon = parse_gps_coord(
        exif.get_field(Tag::GPSLongitude, In::PRIMARY),
        exif.get_field(Tag::GPSLongitudeRef, In::PRIMARY),
        'W',
    );

    let location = match (lat, lon) {
        (Some(lat), Some(lon))
            if lat.is_finite()
                && lon.is_finite()
                && (-90.0..=90.0).contains(&lat)
                && (-180.0..=180.0).contains(&lon) =>
        {
            Some(GeoLocation {
                place_name: format!("{lat:.4}, {lon:.4}"),
                city: None,
                country: None,
                latitude: Some(lat),
                longitude: Some(lon),
            })
        }
        _ => None,
    };

    ExtractedExif {
        taken_at,
        location,
        orientation,
    }
}

fn first_ascii_string(field: &exif::Field) -> Option<String> {
    match &field.value {
        ExifValue::Ascii(items) => items
            .first()
            .map(|b| {
                String::from_utf8_lossy(b)
                    .trim_matches('\0')
                    .trim()
                    .to_string()
            })
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

fn parse_exif_datetime(raw: &str, offset: Option<FixedOffset>) -> Option<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(raw.trim(), "%Y:%m:%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(raw.trim(), "%Y-%m-%d %H:%M:%S"))
        .ok()?;
    match offset {
        Some(off) => off
            .from_local_datetime(&naive)
            .single()
            .map(|dt| dt.with_timezone(&Utc)),
        None => Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)),
    }
}

fn parse_exif_offset(raw: &str) -> Option<FixedOffset> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("Z") {
        return FixedOffset::east_opt(0);
    }
    let sign = if s.starts_with('-') {
        -1
    } else if s.starts_with('+') {
        1
    } else {
        return None;
    };
    let rest = &s[1..];
    let (h_str, m_str) = rest.split_once(':')?;
    let h: i32 = h_str.parse().ok()?;
    let m: i32 = m_str.parse().ok()?;
    FixedOffset::east_opt(sign * (h * 3600 + m * 60))
}

fn parse_gps_coord(
    coord_field: Option<&exif::Field>,
    ref_field: Option<&exif::Field>,
    negative_ref: char,
) -> Option<f64> {
    let field = coord_field?;
    let ExifValue::Rational(rationals) = &field.value else {
        return None;
    };
    if rationals.len() < 3 {
        return None;
    }
    let deg = rationals[0].to_f64();
    let min = rationals[1].to_f64();
    let sec = rationals[2].to_f64();
    if !deg.is_finite() || !min.is_finite() || !sec.is_finite() {
        return None;
    }
    let mut decimal = deg + min / 60.0 + sec / 3600.0;
    if let Some(ref_str) = ref_field.and_then(first_ascii_string) {
        if ref_str
            .chars()
            .next()
            .is_some_and(|c| c.eq_ignore_ascii_case(&negative_ref))
        {
            decimal = -decimal.abs();
        }
    }
    Some(decimal)
}

fn decode_wav_waveform(bytes: &[u8]) -> Result<(Vec<f32>, f32)> {
    let reader = WavReader::new(Cursor::new(bytes))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let sample_rate = spec.sample_rate.max(1) as f32;
    let header_frames = reader.duration() as usize;

    if header_frames == 0 {
        return Ok((vec![MIN_PEAK; WAVEFORM_PEAKS], 0.0));
    }

    let mut bin_sum_sq = [0.0_f64; WAVEFORM_PEAKS];
    let mut bin_counts = [0_usize; WAVEFORM_PEAKS];
    let mut frame_idx = 0usize;

    let mut push_frame = |sample: f32| {
        let bin = ((frame_idx as u128 * WAVEFORM_PEAKS as u128) / header_frames.max(1) as u128)
            .min((WAVEFORM_PEAKS - 1) as u128) as usize;
        let s = sample as f64;
        bin_sum_sq[bin] += s * s;
        bin_counts[bin] += 1;
        frame_idx += 1;
    };

    match spec.sample_format {
        SampleFormat::Int => {
            let bits = spec.bits_per_sample.clamp(1, 32);
            let scale = (1_i64 << (bits - 1)) as f32;
            let mut acc = 0.0_f32;
            let mut ch_idx = 0usize;
            for sample in reader.into_samples::<i32>() {
                acc += sample? as f32 / scale;
                ch_idx += 1;
                if ch_idx == channels {
                    push_frame(acc / channels as f32);
                    acc = 0.0;
                    ch_idx = 0;
                }
            }
            if ch_idx > 0 {
                push_frame(acc / ch_idx as f32);
            }
        }
        SampleFormat::Float => {
            let mut acc = 0.0_f32;
            let mut ch_idx = 0usize;
            for sample in reader.into_samples::<f32>() {
                acc += sample?;
                ch_idx += 1;
                if ch_idx == channels {
                    push_frame(acc / channels as f32);
                    acc = 0.0;
                    ch_idx = 0;
                }
            }
            if ch_idx > 0 {
                push_frame(acc / ch_idx as f32);
            }
        }
    }

    let duration_secs = frame_idx as f32 / sample_rate;
    if frame_idx == 0 {
        return Ok((vec![MIN_PEAK; WAVEFORM_PEAKS], 0.0));
    }

    let mut raw_rms = Vec::with_capacity(WAVEFORM_PEAKS);
    let mut last_rms = 0.0_f32;
    for bin in 0..WAVEFORM_PEAKS {
        if bin_counts[bin] > 0 {
            last_rms = (bin_sum_sq[bin] / bin_counts[bin] as f64).sqrt() as f32;
        }
        raw_rms.push(last_rms);
    }

    Ok((normalize_peaks(&raw_rms), duration_secs))
}

/// Deterministic windowed energy fallback for compressed browser `MediaRecorder` blobs.
fn compressed_blob_waveform(bytes: &[u8]) -> Vec<f32> {
    let payload = if bytes.len() > 256 {
        let skip = (bytes.len() / 16).min(64);
        &bytes[skip..]
    } else {
        bytes
    };

    if payload.is_empty() {
        return vec![MIN_PEAK; WAVEFORM_PEAKS];
    }

    let mut raw_energy = Vec::with_capacity(WAVEFORM_PEAKS);
    for bin in 0..WAVEFORM_PEAKS {
        let start = bin * payload.len() / WAVEFORM_PEAKS;
        let mut end = (bin + 1) * payload.len() / WAVEFORM_PEAKS;
        if end <= start {
            end = (start + 1).min(payload.len());
        }
        let slice = &payload[start..end];
        let energy = if slice.len() <= 1 {
            (slice[0] as f32 - 128.0).abs() / 128.0
        } else {
            let mut sum_sq = 0.0_f32;
            for pair in slice.windows(2) {
                let diff = (pair[1] as f32 - pair[0] as f32) / 255.0;
                sum_sq += diff * diff;
            }
            (sum_sq / (slice.len() - 1) as f32).sqrt()
        };
        raw_energy.push(energy);
    }

    normalize_peaks(&raw_energy)
}

fn normalize_peaks(raw: &[f32]) -> Vec<f32> {
    let max_val = raw.iter().copied().fold(0.0_f32, f32::max);
    if max_val <= 1e-6 {
        return vec![MIN_PEAK; WAVEFORM_PEAKS];
    }
    raw.iter()
        .map(|&v| (MIN_PEAK + (1.0 - MIN_PEAK) * (v / max_val)).clamp(MIN_PEAK, 1.0))
        .collect()
}

fn estimate_compressed_duration(byte_len: usize) -> f32 {
    // Assume ~48 kbps (~6 KB/s) typical MediaRecorder Opus/WebM speech stream.
    (byte_len as f32 / 6000.0).max(0.5)
}

fn infer_audio_mime(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "wav" => "audio/wav",
        "ogg" | "opus" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" => "audio/mp4",
        "flac" => "audio/flac",
        _ => "audio/webm",
    }
}

fn split_sanitized_filename(
    filename: &str,
    default_stem: &str,
    default_ext: &str,
    allowed_exts: &[&str],
) -> (String, String) {
    let path = Path::new(filename);
    let raw_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(default_stem);
    let stem = slugify(raw_stem);
    let raw_ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let ext = if allowed_exts.iter().any(|allowed| *allowed == raw_ext) {
        raw_ext
    } else {
        default_ext.to_string()
    };

    (stem, ext)
}

fn deterministic_tilt(id: &str) -> f32 {
    let hash = id
        .bytes()
        .fold(0_u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    let unit = ((hash % 49) as f32 / 24.0) - 1.0; // -1.0..=1.0
    (unit * 2.2 * 10.0).round() / 10.0
}
