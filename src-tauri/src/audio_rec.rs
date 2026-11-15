//! Native Linux voice recording fallback via PipeWire (`pw-record`), PulseAudio
//! (`parecord`), or ALSA (`arecord`) into a temporary WAV file ingested into the vault.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Local;
use journal_core::Vault;
use journal_types::VoiceMemoMeta;
use uuid::Uuid;

pub const UNSUPPORTED_NATIVE_RECORDER_ERR: &str = "UNSUPPORTED_NATIVE_RECORDER";
const PROBE_SETTLE_WAIT: Duration = Duration::from_millis(80);
const STOP_GRACE_TIMEOUT: Duration = Duration::from_millis(1500);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(25);
const MIN_WAV_BYTES: usize = 45;

struct ActiveRecording {
    backend: &'static str,
    child: Child,
    temp_path: PathBuf,
    started_at: Instant,
}

/// Manages at most one active native microphone capture subprocess and guarantees
/// process termination + temporary file cleanup on cancellation or drop.
pub struct RecorderManager {
    active: Mutex<Option<ActiveRecording>>,
}

impl Default for RecorderManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RecorderManager {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    /// Probes native Linux audio capture tools (`pw-record` -> `parecord` -> `arecord`)
    /// and starts capturing mono 48kHz 16-bit PCM WAV into a temporary file.
    pub fn start(&self) -> Result<String, String> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| "Recorder state lock poisoned".to_string())?;
        if guard.is_some() {
            return Err("Voice recording is already in progress".to_string());
        }

        #[cfg(not(target_os = "linux"))]
        {
            return Err(UNSUPPORTED_NATIVE_RECORDER_ERR.to_string());
        }

        #[cfg(target_os = "linux")]
        {
            let temp_path = std::env::temp_dir().join(format!("log-voice-{}.wav", Uuid::new_v4()));
            let Some(temp_str) = temp_path.to_str() else {
                return Err("Temporary directory path is not valid UTF-8".to_string());
            };

            let candidates: [(&'static str, Vec<&str>); 3] = [
                (
                    "pw-record",
                    vec![
                        "--rate",
                        "48000",
                        "--channels",
                        "1",
                        "--format",
                        "s16",
                        temp_str,
                    ],
                ),
                (
                    "parecord",
                    vec![
                        "--file-format=wav",
                        "--rate=48000",
                        "--channels=1",
                        "--format=s16le",
                        temp_str,
                    ],
                ),
                (
                    "arecord",
                    vec![
                        "-q", "-f", "S16_LE", "-r", "48000", "-c", "1", "-t", "wav", temp_str,
                    ],
                ),
            ];

            for (backend, args) in candidates {
                let mut child = match Command::new(backend)
                    .args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => child,
                    Err(_) => {
                        remove_temp_file(&temp_path);
                        continue;
                    }
                };

                std::thread::sleep(PROBE_SETTLE_WAIT);
                match child.try_wait() {
                    Ok(None) => {
                        *guard = Some(ActiveRecording {
                            backend,
                            child,
                            temp_path: temp_path.clone(),
                            started_at: Instant::now(),
                        });
                        return Ok(backend.to_string());
                    }
                    Ok(Some(_)) | Err(_) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        remove_temp_file(&temp_path);
                    }
                }
            }

            remove_temp_file(&temp_path);
            Err(UNSUPPORTED_NATIVE_RECORDER_ERR.to_string())
        }
    }

    /// Gracefully stops the active recording, repairs streaming WAV chunk sizes if needed,
    /// ingests the WAV audio into `vault`, and deletes the temporary capture file.
    pub fn stop(&self, vault: &Vault) -> Result<VoiceMemoMeta, String> {
        let mut active = {
            let mut guard = self
                .active
                .lock()
                .map_err(|_| "Recorder state lock poisoned".to_string())?;
            guard
                .take()
                .ok_or_else(|| "No active voice recording".to_string())?
        };

        let elapsed_secs = active.started_at.elapsed().as_secs_f32();
        graceful_stop_child(&mut active.child);

        let read_result = fs::read(&active.temp_path);
        remove_temp_file(&active.temp_path);

        let mut bytes =
            read_result.map_err(|e| format!("Failed to read recorded audio file: {e}"))?;
        if bytes.len() < MIN_WAV_BYTES {
            return Err(format!(
                "Recorded audio payload is empty ({} bytes from {})",
                bytes.len(),
                active.backend
            ));
        }

        patch_wav_riff_sizes(&mut bytes);
        let duration_secs = parse_wav_duration_secs(&bytes)
            .filter(|secs| *secs > 0.0 && secs.is_finite())
            .unwrap_or(elapsed_secs);

        let filename = format!("voice-memo-{}.wav", Local::now().format("%Y%m%d-%H%M%S"));
        vault
            .ingest_audio(
                &filename,
                &bytes,
                Some(duration_secs),
                Some("audio/wav".to_string()),
            )
            .map_err(|e| e.to_string())
    }

    /// Terminates any active native capture process and removes its temporary file.
    pub fn cancel(&self) -> Result<(), String> {
        let active = {
            let mut guard = self
                .active
                .lock()
                .map_err(|_| "Recorder state lock poisoned".to_string())?;
            guard.take()
        };

        if let Some(mut active) = active {
            terminate_and_cleanup(&mut active);
        }
        Ok(())
    }
}

impl Drop for RecorderManager {
    fn drop(&mut self) {
        let active = match self.active.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(mut active) = active {
            terminate_and_cleanup(&mut active);
        }
    }
}

fn remove_temp_file(path: &Path) {
    let _ = fs::remove_file(path);
}

fn graceful_stop_child(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }

    #[cfg(unix)]
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        if pid > 0 {
            // Send SIGINT so PipeWire/PulseAudio/ALSA recorders flush buffers and write WAV sizes.
            unsafe {
                libc::kill(pid, libc::SIGINT);
            }
            let deadline = Instant::now() + STOP_GRACE_TIMEOUT;
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(STOP_POLL_INTERVAL);
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}

fn terminate_and_cleanup(active: &mut ActiveRecording) {
    graceful_stop_child(&mut active.child);
    remove_temp_file(&active.temp_path);
}

/// Repairs streaming RIFF/WAV header sizes when a recorder leaves either `RIFF` chunk
/// size (`bytes[4..8]`) or `data` subchunk size as `0` or `0xFFFF_FFFF`.
pub fn patch_wav_riff_sizes(bytes: &mut [u8]) {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return;
    }

    let riff_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if riff_size == 0 || riff_size == u32::MAX {
        let actual_riff = (bytes.len() - 8).min(u32::MAX as usize) as u32;
        bytes[4..8].copy_from_slice(&actual_riff.to_le_bytes());
    }

    let mut offset = 12usize;
    while offset.saturating_add(8) <= bytes.len() {
        let chunk_size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        if &bytes[offset..offset + 4] == b"data" {
            if chunk_size == 0 || chunk_size == u32::MAX {
                let actual_data = (bytes.len() - (offset + 8)).min(u32::MAX as usize) as u32;
                bytes[offset + 4..offset + 8].copy_from_slice(&actual_data.to_le_bytes());
            }
            break;
        }

        let padded_size = (chunk_size as usize).saturating_add((chunk_size as usize) & 1);
        let Some(next) = offset
            .checked_add(8)
            .and_then(|o| o.checked_add(padded_size))
        else {
            break;
        };
        if next <= offset {
            break;
        }
        offset = next;
    }
}

/// Extracts duration in seconds from a PCM WAV header (`fmt ` byte rate + `data` chunk size).
pub fn parse_wav_duration_secs(bytes: &[u8]) -> Option<f32> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut byte_rate: Option<u32> = None;
    let mut data_size: Option<u32> = None;
    let mut offset = 12usize;

    while offset.saturating_add(8) <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        let payload_start = offset + 8;

        if chunk_id == b"fmt "
            && chunk_size >= 16
            && payload_start.saturating_add(12) <= bytes.len()
        {
            byte_rate = Some(u32::from_le_bytes([
                bytes[payload_start + 8],
                bytes[payload_start + 9],
                bytes[payload_start + 10],
                bytes[payload_start + 11],
            ]));
        } else if chunk_id == b"data" {
            let available = bytes.len().saturating_sub(payload_start);
            let effective = if chunk_size == 0 || chunk_size == u32::MAX {
                available.min(u32::MAX as usize) as u32
            } else {
                chunk_size.min(available.min(u32::MAX as usize) as u32)
            };
            data_size = Some(effective);
            break;
        }

        let padded_size = (chunk_size as usize).saturating_add((chunk_size as usize) & 1);
        let next = payload_start.checked_add(padded_size)?;
        if next <= offset {
            break;
        }
        offset = next;
    }

    match (byte_rate, data_size) {
        (Some(rate), Some(size)) if rate > 0 && size > 0 => Some(size as f32 / rate as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_wav_duration_secs, patch_wav_riff_sizes};

    fn sample_wav(
        riff_override: Option<u32>,
        data_override: Option<u32>,
        pcm_len: usize,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(44 + pcm_len);
        buf.extend_from_slice(b"RIFF");
        let riff = riff_override.unwrap_or((36 + pcm_len) as u32);
        buf.extend_from_slice(&riff.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&1u16.to_le_bytes()); // Mono
        buf.extend_from_slice(&48_000u32.to_le_bytes()); // Sample rate
        buf.extend_from_slice(&96_000u32.to_le_bytes()); // Byte rate = 48000 * 1 * 2
        buf.extend_from_slice(&2u16.to_le_bytes()); // Block align
        buf.extend_from_slice(&16u16.to_le_bytes()); // Bits per sample
        buf.extend_from_slice(b"data");
        let data = data_override.unwrap_or(pcm_len as u32);
        buf.extend_from_slice(&data.to_le_bytes());
        buf.resize(44 + pcm_len, 0);
        buf
    }

    #[test]
    fn patch_wav_riff_sizes_repairs_zero_and_streaming_max_sizes() {
        let mut zeroed = sample_wav(Some(0), Some(0), 96_000);
        patch_wav_riff_sizes(&mut zeroed);
        assert_eq!(
            u32::from_le_bytes([zeroed[4], zeroed[5], zeroed[6], zeroed[7]]),
            36 + 96_000
        );
        assert_eq!(
            u32::from_le_bytes([zeroed[40], zeroed[41], zeroed[42], zeroed[43]]),
            96_000
        );
        let secs = parse_wav_duration_secs(&zeroed).expect("duration parsed");
        assert!((secs - 1.0).abs() < 0.001);

        let mut streaming = sample_wav(Some(u32::MAX), Some(u32::MAX), 48_000);
        patch_wav_riff_sizes(&mut streaming);
        assert_eq!(
            u32::from_le_bytes([streaming[4], streaming[5], streaming[6], streaming[7]]),
            36 + 48_000
        );
        assert_eq!(
            u32::from_le_bytes([streaming[40], streaming[41], streaming[42], streaming[43]]),
            48_000
        );
        let half_sec = parse_wav_duration_secs(&streaming).expect("half sec duration");
        assert!((half_sec - 0.5).abs() < 0.001);
    }

    #[test]
    fn patch_wav_riff_sizes_ignores_non_wav_and_preserves_valid_sizes() {
        let mut valid = sample_wav(None, None, 1024);
        let snapshot = valid.clone();
        patch_wav_riff_sizes(&mut valid);
        assert_eq!(valid, snapshot);

        let mut garbage = b"not a wav".to_vec();
        patch_wav_riff_sizes(&mut garbage);
        assert_eq!(garbage, b"not a wav");
    }
}
