//! Browser MediaRecorder voice memo capture and interactive 64-bar waveform player.

use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use journal_types::VoiceMemoMeta;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Blob, BlobEvent, BlobPropertyBag, HtmlAudioElement, HtmlInputElement, MediaRecorder,
    MediaRecorderOptions, MediaStream, MediaStreamConstraints, MediaStreamTrack,
};

use crate::bridge;
use crate::icons::{IconClose, IconMic, IconPause, IconPlay, IconPlus};

struct RecorderSession {
    recorder: MediaRecorder,
    stream: MediaStream,
    _on_data: Closure<dyn FnMut(BlobEvent)>,
    _on_stop: Closure<dyn FnMut()>,
}

/// Format seconds as `MM:SS`.
pub fn format_duration(secs: f32) -> String {
    let total = secs.max(0.0).round() as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Resample arbitrary peak amplitudes into 64 normalized bars (`0.08..=1.0`).
fn resample_64_bars(peaks: &[f32]) -> Vec<f32> {
    const BARS: usize = 64;
    if peaks.is_empty() {
        return (0..BARS)
            .map(|i| {
                let phase = (i as f32 * 0.35).sin().abs();
                0.14 + phase * 0.22
            })
            .collect();
    }

    let mut out = Vec::with_capacity(BARS);
    let len = peaks.len();
    for i in 0..BARS {
        let start = (i * len) / BARS;
        let end = (((i + 1) * len) / BARS).max(start + 1).min(len);
        let max_val = peaks[start..end]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        out.push(max_val.clamp(0.08, 1.0));
    }
    out
}

fn stop_stream_tracks(stream: &MediaStream) {
    let tracks = stream.get_tracks();
    for idx in 0..tracks.length() {
        if let Ok(track) = tracks.get(idx).dyn_into::<MediaStreamTrack>() {
            track.stop();
        }
    }
}

/// Voice memo capture control using `MediaDevices::get_user_media` + `MediaRecorder`,
/// paired with a graceful "Upload audio file" fallback for environments without a mic.
#[component]
pub fn VoiceRecorderControl(
    on_recorded: Callback<VoiceMemoMeta>,
    #[prop(optional)] on_cancel: Option<Callback<()>>,
) -> impl IntoView {
    let is_recording = RwSignal::new(false);
    let is_uploading = RwSignal::new(false);
    let elapsed_secs = RwSignal::new(0_u32);
    let error_msg = RwSignal::new(Option::<String>::None);

    let session_holder: StoredValue<Rc<RefCell<Option<RecorderSession>>>, LocalStorage> =
        StoredValue::new_local(Rc::new(RefCell::new(None)));

    let start_recording = move |_| {
        if is_recording.get_untracked() || is_uploading.get_untracked() {
            return;
        }
        error_msg.set(None);

        let holder = session_holder.get_value();
        let on_recorded_cb = on_recorded;

        spawn_local(async move {
            let Some(window) = web_sys::window() else {
                error_msg.set(Some("Browser window unavailable.".to_string()));
                return;
            };
            let Ok(media_devices) = window.navigator().media_devices() else {
                error_msg.set(Some(
                    "Microphone API unavailable in this browser context. Use \"Upload audio file\" below."
                        .to_string(),
                ));
                return;
            };

            let constraints = MediaStreamConstraints::new();
            constraints.set_audio(&wasm_bindgen::JsValue::TRUE);

            let promise = match media_devices.get_user_media_with_constraints(&constraints) {
                Ok(p) => p,
                Err(err) => {
                    error_msg.set(Some(format!(
                        "Could not request microphone ({err:?}). Try \"Upload audio file\"."
                    )));
                    return;
                }
            };

            let stream_val = match JsFuture::from(promise).await {
                Ok(val) => val,
                Err(err) => {
                    error_msg.set(Some(format!(
                        "Microphone permission denied or no mic connected ({err:?}). You can upload an audio file instead."
                    )));
                    return;
                }
            };

            let Ok(stream) = stream_val.dyn_into::<MediaStream>() else {
                error_msg.set(Some("Unexpected MediaStream object from browser.".to_string()));
                return;
            };

            let (recorder, mime_type) = if MediaRecorder::is_type_supported("audio/webm;codecs=opus") {
                let opts = MediaRecorderOptions::new();
                opts.set_mime_type("audio/webm;codecs=opus");
                match MediaRecorder::new_with_media_stream_and_media_recorder_options(&stream, &opts) {
                    Ok(rec) => (rec, "audio/webm".to_string()),
                    Err(_) => match MediaRecorder::new_with_media_stream(&stream) {
                        Ok(rec) => (rec, "audio/webm".to_string()),
                        Err(err) => {
                            stop_stream_tracks(&stream);
                            error_msg.set(Some(format!("MediaRecorder failed: {err:?}")));
                            return;
                        }
                    },
                }
            } else {
                match MediaRecorder::new_with_media_stream(&stream) {
                    Ok(rec) => (rec, "audio/webm".to_string()),
                    Err(err) => {
                        stop_stream_tracks(&stream);
                        error_msg.set(Some(format!("MediaRecorder failed: {err:?}")));
                        return;
                    }
                }
            };

            let chunks: Rc<RefCell<Vec<Blob>>> = Rc::new(RefCell::new(Vec::new()));
            let chunks_for_data = Rc::clone(&chunks);
            let on_data = Closure::<dyn FnMut(BlobEvent)>::new(move |ev: BlobEvent| {
                if let Some(blob) = ev.data() {
                    if blob.size() > 0.0 {
                        chunks_for_data.borrow_mut().push(blob);
                    }
                }
            });
            recorder.set_ondataavailable(Some(on_data.as_ref().unchecked_ref()));

            let holder_for_stop = Rc::clone(&holder);
            let stream_for_stop = stream.clone();
            let chunks_for_stop = Rc::clone(&chunks);
            let mime_for_stop = mime_type.clone();
            let on_stop = Closure::<dyn FnMut()>::new(move || {
                stop_stream_tracks(&stream_for_stop);
                is_recording.set(false);
                is_uploading.set(true);

                let parts = js_sys::Array::new();
                for chunk in chunks_for_stop.borrow().iter() {
                    parts.push(chunk.as_ref());
                }
                let bag = BlobPropertyBag::new();
                bag.set_type(&mime_for_stop);
                let combined = Blob::new_with_blob_sequence_and_options(&parts, &bag);

                // Drop active session after cloning chunks.
                *holder_for_stop.borrow_mut() = None;

                match combined {
                    Ok(blob) => {
                        spawn_local(async move {
                            match bridge::upload_audio_blob(&blob, "voice-memo.webm").await {
                                Ok(meta) => {
                                    is_uploading.set(false);
                                    on_recorded_cb.run(meta);
                                }
                                Err(err) => {
                                    is_uploading.set(false);
                                    error_msg.set(Some(format!("Audio upload failed: {err}")));
                                }
                            }
                        });
                    }
                    Err(err) => {
                        is_uploading.set(false);
                        error_msg.set(Some(format!("Failed assembling audio blob: {err:?}")));
                    }
                }
            });
            recorder.set_onstop(Some(on_stop.as_ref().unchecked_ref()));

            if let Err(err) = recorder.start_with_time_slice(250) {
                stop_stream_tracks(&stream);
                error_msg.set(Some(format!("Failed to start recording: {err:?}")));
                return;
            }

            *holder.borrow_mut() = Some(RecorderSession {
                recorder,
                stream,
                _on_data: on_data,
                _on_stop: on_stop,
            });

            elapsed_secs.set(0);
            is_recording.set(true);

            // Live elapsed timer while recording is active.
            spawn_local(async move {
                while is_recording.get_untracked() {
                    TimeoutFuture::new(1000).await;
                    if is_recording.get_untracked() {
                        elapsed_secs.update(|s| *s += 1);
                    }
                }
            });
        });
    };

    let stop_recording = move |_| {
        let holder = session_holder.get_value();
        let borrowed = holder.borrow();
        if let Some(session) = borrowed.as_ref() {
            if let Err(err) = session.recorder.stop() {
                error_msg.set(Some(format!("Could not stop recorder: {err:?}")));
            }
        }
    };

    let on_file_selected = move |ev: leptos::ev::Event| {
        let Some(target) = ev.target() else {
            return;
        };
        let Ok(input) = target.dyn_into::<HtmlInputElement>() else {
            return;
        };
        let Some(files) = input.files() else {
            return;
        };
        let Some(file) = files.get(0) else {
            return;
        };

        error_msg.set(None);
        is_uploading.set(true);
        let on_recorded_cb = on_recorded;
        spawn_local(async move {
            match bridge::upload_audio_file(&file).await {
                Ok(meta) => {
                    is_uploading.set(false);
                    on_recorded_cb.run(meta);
                }
                Err(err) => {
                    is_uploading.set(false);
                    error_msg.set(Some(format!("Audio upload failed: {err}")));
                }
            }
        });
    };

    view! {
        <div class="voice-recorder-card">
            <div class="voice-recorder-header">
                <div class="voice-recorder-title">
                    <span class="voice-recorder-icon"><IconMic /></span>
                    <span class="small-caps-meta">"Voice Memo Ribbon"</span>
                </div>
                {on_cancel.map(|cancel_cb| {
                    view! {
                        <button
                            type="button"
                            class="icon-btn subtle"
                            title="Close recorder"
                            on:click=move |_| {
                                // Stop active recording cleanly if cancelled mid-stream.
                                let holder = session_holder.get_value();
                                if let Some(session) = holder.borrow_mut().take() {
                                    let _ = session.recorder.stop();
                                    stop_stream_tracks(&session.stream);
                                }
                                is_recording.set(false);
                                cancel_cb.run(());
                            }
                        >
                            <IconClose />
                        </button>
                    }
                })}
            </div>

            <div class="voice-recorder-controls">
                <Show
                    when=move || is_recording.get()
                    fallback=move || {
                        view! {
                            <button
                                type="button"
                                class="btn-editorial btn-sage"
                                disabled=move || is_uploading.get()
                                on:click=start_recording
                            >
                                <IconMic />
                                <span>
                                    {move || if is_uploading.get() { "Archiving memo..." } else { "Record Voice Memo" }}
                                </span>
                            </button>
                        }
                    }
                >
                    <button
                        type="button"
                        class="btn-editorial btn-terracotta recording-active"
                        on:click=stop_recording
                    >
                        <span class="recording-pulse-dot"></span>
                        <span>{move || format!("Stop & Save ({})", format_duration(elapsed_secs.get() as f32))}</span>
                    </button>
                </Show>

                <label class="btn-editorial btn-ghost upload-audio-label">
                    <IconPlus />
                    <span>"Upload audio file"</span>
                    <input
                        type="file"
                        accept="audio/*"
                        class="sr-only"
                        disabled=move || is_recording.get() || is_uploading.get()
                        on:change=on_file_selected
                    />
                </label>
            </div>

            {move || {
                error_msg.get().map(|err| {
                    view! { <p class="voice-recorder-error">{err}</p> }
                })
            }}
        </div>
    }
}

/// Interactive 64-bar botanical sage ink waveform backed by a hidden `<audio>` element.
#[component]
pub fn WaveformPlayer(
    memo: VoiceMemoMeta,
    #[prop(optional)] on_delete: Option<Callback<String>>,
) -> impl IntoView {
    let audio_ref = NodeRef::<leptos::html::Audio>::new();
    let is_playing = RwSignal::new(false);
    let current_secs = RwSignal::new(0.0_f32);
    let total_secs = RwSignal::new(memo.duration_secs.max(0.1));

    let bars = resample_64_bars(&memo.peaks);
    let memo_id = memo.id.clone();
    let memo_url = memo.url.clone();
    let recorded_label = memo.recorded_at.format("%b %d · %H:%M").to_string();
    let transcript = memo.transcript.clone();

    let toggle_play = move |_| {
        let Some(audio): Option<HtmlAudioElement> = audio_ref.get() else {
            return;
        };
        if is_playing.get_untracked() {
            let _ = audio.pause();
            is_playing.set(false);
        } else {
            match audio.play() {
                Ok(promise) => {
                    is_playing.set(true);
                    spawn_local(async move {
                        if JsFuture::from(promise).await.is_err() {
                            is_playing.set(false);
                        }
                    });
                }
                Err(_) => {
                    is_playing.set(false);
                }
            }
        }
    };

    let on_time_update = move |_| {
        let Some(audio): Option<HtmlAudioElement> = audio_ref.get() else {
            return;
        };
        current_secs.set(audio.current_time() as f32);
        let dur = audio.duration();
        if dur.is_finite() && dur > 0.0 {
            total_secs.set(dur as f32);
        }
    };

    let on_ended = move |_| {
        is_playing.set(false);
        current_secs.set(0.0);
    };

    let seek_at_click = move |ev: leptos::ev::MouseEvent| {
        let Some(audio): Option<HtmlAudioElement> = audio_ref.get() else {
            return;
        };
        let Some(target) = ev.current_target() else {
            return;
        };
        let Ok(el) = target.dyn_into::<web_sys::Element>() else {
            return;
        };
        let rect = el.get_bounding_client_rect();
        let width = rect.width().max(1.0);
        let ratio = ((ev.client_x() as f64 - rect.left()) / width).clamp(0.0, 1.0);

        let dur = audio.duration();
        let effective_total = if dur.is_finite() && dur > 0.0 {
            dur
        } else {
            total_secs.get_untracked() as f64
        };
        let next_time = ratio * effective_total;
        audio.set_current_time(next_time);
        current_secs.set(next_time as f32);
    };

    view! {
        <div class="waveform-ribbon">
            <audio
                node_ref=audio_ref
                src=memo_url
                preload="metadata"
                on:timeupdate=on_time_update
                on:ended=on_ended
                on:loadedmetadata=on_time_update
            />

            <button
                type="button"
                class="waveform-play-btn"
                title=move || if is_playing.get() { "Pause voice memo" } else { "Play voice memo" }
                on:click=toggle_play
            >
                <Show when=move || is_playing.get() fallback=|| view! { <IconPlay /> }>
                    <IconPause />
                </Show>
            </button>

            <div class="waveform-body">
                <div
                    class="waveform-bars"
                    role="slider"
                    aria-label="Voice memo waveform seek bar"
                    on:click=seek_at_click
                >
                    {bars
                        .into_iter()
                        .enumerate()
                        .map(|(idx, amp)| {
                            let pct_pos = (idx as f32 + 0.5) / 64.0;
                            let height_pct = (amp * 100.0).clamp(10.0, 100.0);
                            view! {
                                <span
                                    class="waveform-bar"
                                    class:played=move || {
                                        let dur = total_secs.get().max(0.1);
                                        let prog = (current_secs.get() / dur).clamp(0.0, 1.0);
                                        pct_pos <= prog
                                    }
                                    style=format!("height: {height_pct:.1}%")
                                ></span>
                            }
                        })
                        .collect_view()}
                </div>

                <div class="waveform-meta">
                    <span class="small-caps-meta">{recorded_label}</span>
                    <span class="waveform-time">
                        {move || {
                            format!(
                                "{} / {}",
                                format_duration(current_secs.get()),
                                format_duration(total_secs.get())
                            )
                        }}
                    </span>
                </div>

                {transcript.map(|txt| {
                    view! { <p class="waveform-transcript">{txt}</p> }
                })}
            </div>

            {on_delete.map(|del_cb| {
                let id_for_del = memo_id.clone();
                view! {
                    <button
                        type="button"
                        class="icon-btn subtle"
                        title="Remove voice memo"
                        on:click=move |_| del_cb.run(id_for_del.clone())
                    >
                        <IconClose />
                    </button>
                }
            })}
        </div>
    }
}
