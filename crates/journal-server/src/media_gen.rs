//! Procedural Polaroid photograph and warm acoustic chord `.wav` synthesis for `seed-demo`.

use std::io::Cursor;

use anyhow::Result;
use hound::{SampleFormat, WavSpec, WavWriter};
use image::{ImageFormat, Rgba, RgbaImage};

/// Synthesize a warm 960x720 atmospheric monsoon-twilight veranda photograph as PNG bytes.
pub fn generate_veranda_polaroid_png() -> Result<Vec<u8>> {
    let width: u32 = 960;
    let height: u32 = 720;
    let mut img = RgbaImage::new(width, height);

    for y in 0..height {
        let ny = y as f32 / height as f32;
        for x in 0..width {
            let nx = x as f32 / width as f32;

            // Warm dusk sky to misty veranda sage-umber gradient.
            let sky_t = (ny / 0.62).clamp(0.0, 1.0);
            let mut r = lerp(218.0, 179.0, sky_t);
            let mut g = lerp(164.0, 122.0, sky_t);
            let mut b = lerp(118.0, 96.0, sky_t);

            // Soft golden horizon glow off-center left.
            let dx = nx - 0.34;
            let dy = ny - 0.44;
            let glow = (-((dx * dx) * 9.0 + (dy * dy) * 16.0)).exp();
            r += glow * 44.0;
            g += glow * 32.0;
            b += glow * 14.0;

            // Misty rain-washed horizon & garden canopy below y = 0.56.
            if ny > 0.54 {
                let canopy_t = ((ny - 0.54) / 0.46).clamp(0.0, 1.0);
                let wave = 0.025 * (nx * 14.0).sin() + 0.015 * (nx * 31.0).cos();
                let blend = ((ny - (0.55 + wave)) * 18.0).clamp(0.0, 1.0);
                let garden_r = lerp(82.0, 42.0, canopy_t);
                let garden_g = lerp(98.0, 56.0, canopy_t);
                let garden_b = lerp(78.0, 48.0, canopy_t);
                r = lerp(r, garden_r, blend);
                g = lerp(g, garden_g, blend);
                b = lerp(b, garden_b, blend);
            }

            // Rain streaks (subtle diagonal monsoon shimmer).
            let streak = ((nx * 240.0 + ny * 38.0).sin() * 0.5 + 0.5).powi(10);
            if ny < 0.85 {
                r += streak * 16.0;
                g += streak * 15.0;
                b += streak * 13.0;
            }

            // Foreground teak veranda rail & pillar silhouette.
            if (0.08..=0.125).contains(&nx) || (0.875..=0.92).contains(&nx) {
                r *= 0.28;
                g *= 0.24;
                b *= 0.22;
            } else if ny > 0.80 && ny < 0.845 {
                r *= 0.32;
                g *= 0.27;
                b *= 0.24;
            }

            // Gentle film vignette + organic grain.
            let vx = (nx - 0.5) * 2.0;
            let vy = (ny - 0.5) * 2.0;
            let vig = (1.0 - 0.28 * (vx * vx + vy * vy)).clamp(0.55, 1.0);
            let grain = (((x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263)) & 0xff) as f32
                / 255.0
                - 0.5)
                * 7.0;

            let pr = (r * vig + grain).clamp(0.0, 255.0) as u8;
            let pg = (g * vig + grain).clamp(0.0, 255.0) as u8;
            let pb = (b * vig + grain).clamp(0.0, 255.0) as u8;
            img.put_pixel(x, y, Rgba([pr, pg, pb, 255]));
        }
    }

    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png)?;
    Ok(buf.into_inner())
}

/// Synthesize a pleasant 3-second warm acoustic Cmaj9 chord `.wav` (44.1 kHz, 16-bit mono).
pub fn generate_acoustic_chord_wav() -> Result<Vec<u8>> {
    let sample_rate: u32 = 44_100;
    let duration_secs: f32 = 3.0;
    let total_samples = (sample_rate as f32 * duration_secs) as usize;

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    // Staggered arpeggiated warm Cmaj9 voicing (Hz, onset_sec, decay_rate).
    let notes: &[(f32, f32, f32)] = &[
        (261.63, 0.02, 0.95), // C4
        (329.63, 0.09, 1.05), // E4
        (392.00, 0.16, 1.10), // G4
        (493.88, 0.23, 1.20), // B4
        (587.33, 0.30, 1.30), // D5
    ];

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut cursor, spec)?;
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let mut sum = 0.0_f32;

            for &(freq, onset, decay) in notes {
                if t >= onset {
                    let nt = t - onset;
                    let attack = (nt / 0.018).min(1.0);
                    let env = attack * (-decay * nt).exp();
                    let vib = 1.0 + 0.0018 * (2.0 * std::f32::consts::PI * 4.2 * nt).sin();
                    let f = freq * vib;
                    let tone = 0.72 * (2.0 * std::f32::consts::PI * f * nt).sin()
                        + 0.21 * (2.0 * std::f32::consts::PI * (f * 2.0) * nt).sin()
                        + 0.07 * (2.0 * std::f32::consts::PI * (f * 3.0) * nt).sin();
                    sum += env * tone;
                }
            }

            // Gentle overall fade-out in the final 0.35s so looping/ending is click-free.
            let tail_fade = if t > 2.65 {
                ((3.0 - t) / 0.35).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let normalized = (sum * 0.24 * tail_fade).clamp(-0.95, 0.95);
            let pcm = (normalized * i16::MAX as f32) as i16;
            writer.write_sample(pcm)?;
        }
        writer.finalize()?;
    }

    Ok(cursor.into_inner())
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
