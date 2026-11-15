//! Two-column open-book spread editor with `/` slash palette, Polaroids, voice memos, stamps, and Reading Folio mode.

use std::collections::HashSet;

use gloo_timers::future::TimeoutFuture;
use journal_types::{
    AssetManifest, BacklinkRef, DailyContext, Entry, EntrySummary, GeoLocation, Notebook,
    PolaroidMeta, SaveEntryRequest, StampPlacement, Template, VoiceMemoMeta, WeatherInfo,
};
use leptos::prelude::*;
use uuid::Uuid;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlTextAreaElement};

use crate::audio_recorder::{VoiceRecorderControl, WaveformPlayer};
use crate::bridge;
use crate::icons::{
    IconBookOpen, IconCamera, IconCheck, IconChevronLeft, IconChevronRight, IconClose, IconEye,
    IconFeather, IconLeaf, IconLink, IconMapPin, IconMic, IconPlus, IconSpark, IconStamp,
    IconStar, IconTag, IconTrash, WeatherGlyph,
};
use crate::views::{PolaroidCard, StampPickerPopover, StampStickerLayer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorMode {
    Compose,
    Reading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlashAction {
    Heading1,
    Heading2,
    Todo,
    Quote,
    Callout,
    Divider,
    Photo,
    Voice,
    Stamp,
    Template,
    LinkEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlashCommandItem {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    glyph: &'static str,
    action: SlashAction,
}

const SLASH_COMMANDS: [SlashCommandItem; 11] = [
    SlashCommandItem {
        id: "h1",
        title: "Heading I",
        subtitle: "Large editorial chapter title",
        glyph: "H1",
        action: SlashAction::Heading1,
    },
    SlashCommandItem {
        id: "h2",
        title: "Heading II",
        subtitle: "Section subheading",
        glyph: "H2",
        action: SlashAction::Heading2,
    },
    SlashCommandItem {
        id: "todo",
        title: "Checklist",
        subtitle: "Trackable ink checkbox",
        glyph: "[ ]",
        action: SlashAction::Todo,
    },
    SlashCommandItem {
        id: "quote",
        title: "Literary Pull-Quote",
        subtitle: "Indented serif epigraph",
        glyph: "\"",
        action: SlashAction::Quote,
    },
    SlashCommandItem {
        id: "callout",
        title: "Botanical Note",
        subtitle: "Highlighted herbarium callout",
        glyph: "!",
        action: SlashAction::Callout,
    },
    SlashCommandItem {
        id: "divider",
        title: "Section Rule",
        subtitle: "Ornamental page divider",
        glyph: "---",
        action: SlashAction::Divider,
    },
    SlashCommandItem {
        id: "photo",
        title: "Attach Polaroid Photo",
        subtitle: "Upload image with washi tape",
        glyph: "IMG",
        action: SlashAction::Photo,
    },
    SlashCommandItem {
        id: "voice",
        title: "Record Voice Memo",
        subtitle: "Capture mic or upload audio",
        glyph: "REC",
        action: SlashAction::Voice,
    },
    SlashCommandItem {
        id: "stamp",
        title: "Place Stamp or Sticker",
        subtitle: "Die-cut botanicals & postmarks",
        glyph: "INK",
        action: SlashAction::Stamp,
    },
    SlashCommandItem {
        id: "template",
        title: "Apply Editorial Template",
        subtitle: "Guided reflection structure",
        glyph: "TPL",
        action: SlashAction::Template,
    },
    SlashCommandItem {
        id: "link",
        title: "Link Past Entry",
        subtitle: "Insert [[wikilink]] citation",
        glyph: "[[]]",
        action: SlashAction::LinkEntry,
    },
];

const DEFAULT_COVERS: [&str; 4] = [
    "/assets/covers/botanical-fern-wash.png",
    "/assets/covers/rainy-tea-desk.png",
    "/assets/covers/pressed-wildflowers.png",
    "/assets/covers/midnight-ink-garden.png",
];

const MOOD_PRESETS: [&str; 6] = [
    "reflective",
    "serene",
    "grateful",
    "inspired",
    "rainy",
    "unhurried",
];

/// Convert a DOM UTF-16 cursor offset into a safe UTF-8 byte index in `s`.
fn utf16_offset_to_byte_idx(s: &str, utf16_pos: usize) -> usize {
    let mut utf16_acc = 0;
    for (byte_idx, ch) in s.char_indices() {
        if utf16_acc >= utf16_pos {
            return byte_idx;
        }
        utf16_acc += ch.len_utf16();
    }
    s.len()
}

/// Convert a UTF-8 byte index in `s` back to a DOM UTF-16 offset.
fn byte_idx_to_utf16_offset(s: &str, byte_idx: usize) -> usize {
    let clamped = byte_idx.min(s.len());
    s[..clamped].chars().map(char::len_utf16).sum()
}

/// Extract `[[wikilink]]` titles from Markdown prose for frontmatter persistence.
fn extract_wikilinks(body: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut links = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find("]]") {
            let candidate = after[..end].trim();
            if !candidate.is_empty() && seen.insert(candidate.to_lowercase()) {
                links.push(candidate.to_string());
            }
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    links
}

#[derive(Debug, Clone)]
enum InlineSpan {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    Wikilink(String),
}

fn parse_inline_spans(line: &str) -> Vec<InlineSpan> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    let flush_buf = |buf: &mut String, spans: &mut Vec<InlineSpan>| {
        if !buf.is_empty() {
            spans.push(InlineSpan::Text(std::mem::take(buf)));
        }
    };

    while i < chars.len() {
        // [[wikilink]]
        if i + 1 < chars.len() && chars[i] == '[' && chars[i + 1] == '[' {
            if let Some(rel_end) = chars[i + 2..].windows(2).position(|w| w == [']', ']']) {
                let target: String = chars[i + 2..i + 2 + rel_end].iter().collect();
                flush_buf(&mut buf, &mut spans);
                spans.push(InlineSpan::Wikilink(target.trim().to_string()));
                i = i + 4 + rel_end;
                continue;
            }
        }
        // **bold**
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(rel_end) = chars[i + 2..].windows(2).position(|w| w == ['*', '*']) {
                let inner: String = chars[i + 2..i + 2 + rel_end].iter().collect();
                flush_buf(&mut buf, &mut spans);
                spans.push(InlineSpan::Bold(inner));
                i = i + 4 + rel_end;
                continue;
            }
        }
        // *italic*
        if chars[i] == '*' {
            if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '*') {
                if rel_end > 0 {
                    let inner: String = chars[i + 1..i + 1 + rel_end].iter().collect();
                    flush_buf(&mut buf, &mut spans);
                    spans.push(InlineSpan::Italic(inner));
                    i = i + 2 + rel_end;
                    continue;
                }
            }
        }
        // `code`
        if chars[i] == '`' {
            if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '`') {
                if rel_end > 0 {
                    let inner: String = chars[i + 1..i + 1 + rel_end].iter().collect();
                    flush_buf(&mut buf, &mut spans);
                    spans.push(InlineSpan::Code(inner));
                    i = i + 2 + rel_end;
                    continue;
                }
            }
        }

        buf.push(chars[i]);
        i += 1;
    }

    flush_buf(&mut buf, &mut spans);
    spans
}

fn render_inline_view(
    text: &str,
    entries: Signal<Vec<EntrySummary>>,
    on_open_entry: Callback<Uuid>,
) -> impl IntoView {
    parse_inline_spans(text)
        .into_iter()
        .map(|span| match span {
            InlineSpan::Text(t) => view! { <span>{t}</span> }.into_any(),
            InlineSpan::Bold(b) => view! { <strong>{b}</strong> }.into_any(),
            InlineSpan::Italic(it) => view! { <em>{it}</em> }.into_any(),
            InlineSpan::Code(c) => view! { <code class="folio-inline-code">{c}</code> }.into_any(),
            InlineSpan::Wikilink(target) => {
                let target_click = target.clone();
                let title_attr = format!("Open folio: {target}");
                view! {
                    <button
                        type="button"
                        class="folio-wikilink"
                        title=title_attr
                        on:click=move |_| {
                            let list = entries.get_untracked();
                            let needle = target_click.trim().to_lowercase();
                            if let Some(found) = list.iter().find(|e| e.title.trim().to_lowercase() == needle)
                                .or_else(|| list.iter().find(|e| e.title.to_lowercase().contains(&needle)))
                            {
                                on_open_entry.run(found.id);
                            }
                        }
                    >
                        <IconLink />
                        <span>{target}</span>
                    </button>
                }
                .into_any()
            }
        })
        .collect_view()
}

/// Authentic two-column open-book spread editor.
#[component]
pub fn EntrySpreadEditor(
    entry: Signal<Option<Entry>>,
    notebooks: Signal<Vec<Notebook>>,
    templates: Signal<Vec<Template>>,
    daily_context: Signal<Option<DailyContext>>,
    manifest: Signal<AssetManifest>,
    all_entries: Signal<Vec<EntrySummary>>,
    on_saved: Callback<Entry>,
    on_deleted: Callback<Uuid>,
    on_open_entry: Callback<Uuid>,
    on_page_turn: Callback<i32>,
    #[prop(optional)] default_notebook_id: Option<Signal<Option<String>>>,
    #[prop(optional)] reset_nonce: Option<Signal<u64>>,
    #[prop(optional)] page_turn_class: Option<Signal<&'static str>>,
) -> impl IntoView {
    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();
    let photo_input_ref = NodeRef::<leptos::html::Input>::new();

    // Working folio state synchronized whenever `entry` changes.
    let loaded_id = RwSignal::new(Option::<Uuid>::None);
    let loaded_date = RwSignal::new(chrono::Utc::now());
    let last_reset_nonce = RwSignal::new(0_u64);
    let title = RwSignal::new(String::new());
    let body = RwSignal::new(String::new());
    let notebook_id = RwSignal::new(Option::<String>::None);
    let tags = RwSignal::new(Vec::<String>::new());
    let mood = RwSignal::new(Option::<String>::None);
    let starred = RwSignal::new(false);
    let weather = RwSignal::new(Option::<WeatherInfo>::None);
    let location = RwSignal::new(Option::<GeoLocation>::None);
    let stamps = RwSignal::new(Vec::<StampPlacement>::new());
    let photos = RwSignal::new(Vec::<PolaroidMeta>::new());
    let audio = RwSignal::new(Vec::<VoiceMemoMeta>::new());
    let backlinks = RwSignal::new(Vec::<BacklinkRef>::new());

    let mode = RwSignal::new(EditorMode::Compose);
    let cover_override = RwSignal::new(Option::<String>::None);
    let show_cover_picker = RwSignal::new(false);
    let show_voice_recorder = RwSignal::new(false);
    let show_stamp_picker = RwSignal::new(false);
    let show_template_picker = RwSignal::new(false);
    let show_link_picker = RwSignal::new(false);
    let link_filter = RwSignal::new(String::new());

    let new_tag_input = RwSignal::new(String::new());
    let editing_location = RwSignal::new(false);
    let location_draft = RwSignal::new(String::new());

    // Slash palette state
    let slash_open = RwSignal::new(false);
    let slash_query = RwSignal::new(String::new());
    let slash_index = RwSignal::new(0_usize);
    let slash_range = RwSignal::new((0_usize, 0_usize));

    // Persistence & feedback state
    let dirty = RwSignal::new(false);
    let saving = RwSignal::new(false);
    let save_status = RwSignal::new("Saved to local vault".to_string());
    let save_revision = RwSignal::new(0_u64);
    let confirm_delete = RwSignal::new(false);
    let drag_over = RwSignal::new(false);

    // Sync working state when parent selects a different entry (or creates a fresh one).
    Effect::new(move |_| {
        if let Some(current) = entry.get() {
            let incoming_id = Some(current.id);
            let should_reset = loaded_id.get_untracked() != incoming_id
                || (!dirty.get_untracked() && current.frontmatter.updated_at != loaded_date.get_untracked());

            if should_reset {
                loaded_id.set(incoming_id);
                loaded_date.set(current.frontmatter.date);
                title.set(current.frontmatter.title.clone());
                body.set(current.body.clone());
                notebook_id.set(current.frontmatter.notebook_id.clone());
                tags.set(current.frontmatter.tags.clone());
                mood.set(current.frontmatter.mood.clone());
                starred.set(current.frontmatter.starred);
                weather.set(current.frontmatter.weather.clone());
                location.set(current.frontmatter.location.clone());
                location_draft.set(
                    current
                        .frontmatter
                        .location
                        .as_ref()
                        .map(|l| l.place_name.clone())
                        .unwrap_or_default(),
                );
                stamps.set(current.frontmatter.stamps.clone());
                photos.set(current.frontmatter.photos.clone());
                audio.set(current.frontmatter.audio.clone());
                backlinks.set(current.backlinks.clone());
                cover_override.set(None);
                dirty.set(false);
                confirm_delete.set(false);
                save_status.set("Saved to local vault".to_string());

                // Refresh backlinks cleanly from backend when opening an existing entry.
                let id_for_bl = current.id;
                spawn_local(async move {
                    if let Ok(bl) = bridge::get_backlinks(id_for_bl).await {
                        if loaded_id.get_untracked() == Some(id_for_bl) {
                            backlinks.set(bl);
                        }
                    }
                });
            }
        } else {
            let nonce = reset_nonce.map(|s| s.get()).unwrap_or(0);
            if loaded_id.get_untracked().is_some() || nonce != last_reset_nonce.get_untracked() {
                last_reset_nonce.set(nonce);
                loaded_id.set(None);
                loaded_date.set(chrono::Utc::now());
                title.set(String::new());
                body.set(String::new());
                notebook_id.set(default_notebook_id.and_then(|s| s.get_untracked()));
                tags.set(Vec::new());
                mood.set(None);
                starred.set(false);
                weather.set(None);
                location.set(None);
                location_draft.set(String::new());
                stamps.set(Vec::new());
                photos.set(Vec::new());
                audio.set(Vec::new());
                backlinks.set(Vec::new());
                cover_override.set(None);
                dirty.set(false);
                confirm_delete.set(false);
                save_status.set("Fresh folio".to_string());
            }
        }
    });

    // Build and persist `SaveEntryRequest`.
    let trigger_save = move || {
        if saving.get_untracked() {
            return;
        }
        let raw_title = title.get_untracked().trim().to_string();
        let raw_body = body.get_untracked();
        if raw_title.is_empty() && raw_body.trim().is_empty() {
            return;
        }

        let effective_title = if raw_title.is_empty() {
            "Untitled Folio".to_string()
        } else {
            raw_title
        };

        saving.set(true);
        save_status.set("Saving to vault...".to_string());

        let req = SaveEntryRequest {
            id: loaded_id.get_untracked(),
            title: effective_title,
            date: Some(loaded_date.get_untracked()),
            notebook_id: notebook_id.get_untracked(),
            tags: tags.get_untracked(),
            mood: mood.get_untracked(),
            starred: starred.get_untracked(),
            weather: weather.get_untracked(),
            location: location.get_untracked(),
            stamps: stamps.get_untracked(),
            photos: photos.get_untracked(),
            audio: audio.get_untracked(),
            body: raw_body,
        };

        spawn_local(async move {
            match bridge::save_entry(req).await {
                Ok(saved) => {
                    saving.set(false);
                    dirty.set(false);
                    loaded_id.set(Some(saved.id));
                    loaded_date.set(saved.frontmatter.date);
                    backlinks.set(saved.backlinks.clone());
                    save_status.set("Saved to local vault".to_string());
                    on_saved.run(saved);
                }
                Err(err) => {
                    saving.set(false);
                    save_status.set(format!("Save error: {err}"));
                }
            }
        });
    };

    // Mark dirty & schedule 1.4s debounced auto-save.
    let mark_dirty = move || {
        dirty.set(true);
        save_status.set("Unsaved ink...".to_string());
        let next_rev = save_revision.get_untracked().wrapping_add(1);
        save_revision.set(next_rev);
        spawn_local(async move {
            TimeoutFuture::new(1400).await;
            if save_revision.get_untracked() == next_rev && dirty.get_untracked() {
                trigger_save();
            }
        });
    };

    // Active watercolor cover banner URL.
    let active_cover_url = Memo::new(move |_| {
        if let Some(custom) = cover_override.get() {
            return custom;
        }
        if let Some(nb_id) = notebook_id.get() {
            if let Some(nb) = notebooks.get().into_iter().find(|n| n.id == nb_id) {
                return nb.cover_asset;
            }
        }
        "/assets/covers/botanical-fern-wash.png".to_string()
    });

    // Live word count & reading duration.
    let word_count = Memo::new(move |_| Entry::count_words(&body.get()));
    let reading_mins = Memo::new(move |_| Entry::estimate_reading_minutes(word_count.get()));

    // Filtered slash commands.
    let filtered_slash = Memo::new(move |_| {
        let q = slash_query.get().trim().to_lowercase();
        SLASH_COMMANDS
            .iter()
            .copied()
            .filter(|item| {
                q.is_empty()
                    || item.id.contains(&q)
                    || item.title.to_lowercase().contains(&q)
                    || item.subtitle.to_lowercase().contains(&q)
            })
            .collect::<Vec<_>>()
    });

    // Helper: replace range (byte offsets) in `body` and position textarea cursor.
    let insert_snippet_at = move |replace_start: usize, replace_end: usize, snippet: &str| {
        let current = body.get_untracked();
        let start = replace_start.min(current.len());
        let end = replace_end.min(current.len()).max(start);
        let mut next = String::with_capacity(current.len() + snippet.len());
        next.push_str(&current[..start]);
        next.push_str(snippet);
        next.push_str(&current[end..]);
        let new_byte_cursor = start + snippet.len();
        body.set(next.clone());
        mark_dirty();

        if let Some(ta) = textarea_ref.get() {
            let _ = ta.focus();
            let utf16_pos = byte_idx_to_utf16_offset(&next, new_byte_cursor) as u32;
            let _ = ta.set_selection_range(utf16_pos, utf16_pos);
        }
    };

    // Execute a chosen slash command item.
    let execute_slash_action = move |action: SlashAction| {
        let (start, end) = slash_range.get_untracked();
        slash_open.set(false);
        slash_query.set(String::new());

        match action {
            SlashAction::Heading1 => insert_snippet_at(start, end, "# "),
            SlashAction::Heading2 => insert_snippet_at(start, end, "## "),
            SlashAction::Todo => insert_snippet_at(start, end, "- [ ] "),
            SlashAction::Quote => insert_snippet_at(start, end, "> "),
            SlashAction::Callout => insert_snippet_at(start, end, "> [!NOTE] "),
            SlashAction::Divider => insert_snippet_at(start, end, "\n---\n\n"),
            SlashAction::Photo => {
                insert_snippet_at(start, end, "");
                if let Some(input) = photo_input_ref.get() {
                    input.click();
                }
            }
            SlashAction::Voice => {
                insert_snippet_at(start, end, "");
                show_voice_recorder.set(true);
            }
            SlashAction::Stamp => {
                insert_snippet_at(start, end, "");
                show_stamp_picker.set(true);
            }
            SlashAction::Template => {
                insert_snippet_at(start, end, "");
                show_template_picker.set(true);
            }
            SlashAction::LinkEntry => {
                insert_snippet_at(start, end, "");
                link_filter.set(String::new());
                show_link_picker.set(true);
            }
        }
    };

    // Handle photo file upload (either via file input or drag-and-drop).
    let handle_photo_file = move |file: web_sys::File| {
        save_status.set("Developing Polaroid...".to_string());
        spawn_local(async move {
            match bridge::upload_photo(&file).await {
                Ok(mut polaroid) => {
                    let count = photos.get_untracked().len();
                    let tilts = [-2.2_f32, 1.8, -1.4, 2.4, -0.9, 1.5];
                    polaroid.rotation_deg = tilts[count % tilts.len()];
                    if polaroid.washi_asset.is_none() {
                        polaroid.washi_asset = Some(if count % 2 == 0 {
                            "/assets/svg/washi-cream.svg".to_string()
                        } else {
                            "/assets/svg/washi-terracotta.svg".to_string()
                        });
                    }
                    photos.update(|list| list.push(polaroid));
                    mark_dirty();
                }
                Err(err) => {
                    save_status.set(format!("Photo upload error: {err}"));
                }
            }
        });
    };

    let on_photo_input_change = move |ev: leptos::ev::Event| {
        let Some(target) = ev.target() else {
            return;
        };
        let Ok(input) = target.dyn_into::<HtmlInputElement>() else {
            return;
        };
        let Some(files) = input.files() else {
            return;
        };
        if let Some(file) = files.get(0) {
            handle_photo_file(file);
        }
        input.set_value("");
    };

    let on_drop_file = move |ev: leptos::ev::DragEvent| {
        ev.prevent_default();
        drag_over.set(false);
        let Some(dt) = ev.data_transfer() else {
            return;
        };
        let Some(files) = dt.files() else {
            return;
        };
        for idx in 0..files.length() {
            if let Some(file) = files.get(idx) {
                let mime = file.type_();
                if mime.starts_with("image/") {
                    handle_photo_file(file);
                } else if mime.starts_with("audio/") {
                    spawn_local(async move {
                        if let Ok(memo) = bridge::upload_audio_file(&file).await {
                            audio.update(|list| list.push(memo));
                            mark_dirty();
                        }
                    });
                }
            }
        }
    };

    // Textarea input: sync body and inspect current line for `/` slash trigger.
    let on_body_input = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        body.set(val.clone());
        mark_dirty();

        if let Some(ta) = textarea_ref.get() {
            let utf16_cursor = ta
                .selection_start()
                .ok()
                .flatten()
                .map(|v| v as usize)
                .unwrap_or_else(|| val.encode_utf16().count());
            let byte_cursor = utf16_offset_to_byte_idx(&val, utf16_cursor);
            let before = &val[..byte_cursor];
            let line_start = before.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
            let current_line = &before[line_start..];

            if let Some(after_slash) = current_line.strip_prefix('/') {
                if !after_slash.contains(' ') && after_slash.len() <= 16 {
                    slash_range.set((line_start, byte_cursor));
                    slash_query.set(after_slash.to_string());
                    slash_index.set(0);
                    slash_open.set(true);
                    return;
                }
            }
        }
        slash_open.set(false);
    };

    let on_body_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if (ev.ctrl_key() || ev.meta_key()) && ev.key().eq_ignore_ascii_case("s") {
            ev.prevent_default();
            trigger_save();
            return;
        }

        if !slash_open.get_untracked() {
            return;
        }

        let items = filtered_slash.get_untracked();
        match ev.key().as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                if !items.is_empty() {
                    slash_index.update(|idx| *idx = (*idx + 1) % items.len());
                }
            }
            "ArrowUp" => {
                ev.prevent_default();
                if !items.is_empty() {
                    slash_index.update(|idx| *idx = (*idx + items.len() - 1) % items.len());
                }
            }
            "Enter" => {
                if let Some(chosen) = items.get(slash_index.get_untracked()).copied() {
                    ev.prevent_default();
                    execute_slash_action(chosen.action);
                }
            }
            "Escape" => {
                ev.prevent_default();
                slash_open.set(false);
            }
            _ => {}
        }
    };

    let open_insert_palette = move |_| {
        let current = body.get_untracked();
        let cursor = textarea_ref
            .get()
            .and_then(|ta: HtmlTextAreaElement| ta.selection_start().ok().flatten())
            .map(|p| utf16_offset_to_byte_idx(&current, p as usize))
            .unwrap_or(current.len());
        slash_range.set((cursor, cursor));
        slash_query.set(String::new());
        slash_index.set(0);
        slash_open.update(|v| *v = !*v);
    };

    let add_tag = move || {
        let cleaned = new_tag_input
            .get_untracked()
            .trim()
            .trim_start_matches('#')
            .to_string();
        if cleaned.is_empty() {
            return;
        }
        tags.update(|list| {
            if !list.iter().any(|t| t.eq_ignore_ascii_case(&cleaned)) {
                list.push(cleaned);
            }
        });
        new_tag_input.set(String::new());
        mark_dirty();
    };

    view! {
        <article
            class=move || {
                let turn_cls = page_turn_class.map(|s| s.get()).unwrap_or("");
                if drag_over.get() {
                    format!("book-spread-container is-drag-over {turn_cls}")
                } else {
                    format!("book-spread-container {turn_cls}")
                }
            }
            on:dragover=move |ev: leptos::ev::DragEvent| {
                ev.prevent_default();
                drag_over.set(true);
            }
            on:dragleave=move |_| drag_over.set(false)
            on:drop=on_drop_file
        >
            // Hidden photo picker used by `/photo` and Polaroid toolbar
            <input
                node_ref=photo_input_ref
                type="file"
                accept="image/*"
                class="sr-only"
                on:change=on_photo_input_change
            />

            // Top Watercolor Cover Banner
            <div class="folio-cover-banner">
                <img
                    src=move || active_cover_url.get()
                    alt="Notebook watercolor cover"
                    class="folio-cover-img"
                />
                <div class="folio-cover-scrim"></div>
                <div class="folio-cover-controls">
                    <button
                        type="button"
                        class="cover-switch-pill"
                        on:click=move |_| show_cover_picker.update(|v| *v = !*v)
                    >
                        <IconLeaf />
                        <span>"Cover Wash"</span>
                    </button>

                    <Show when=move || show_cover_picker.get()>
                        <div class="cover-swatch-popover">
                            {move || {
                                let covers = manifest.get().covers;
                                let paths: Vec<String> = if covers.is_empty() {
                                    DEFAULT_COVERS.iter().map(|s| (*s).to_string()).collect()
                                } else {
                                    covers.into_iter().map(|c| c.path).collect()
                                };
                                paths
                                    .into_iter()
                                    .map(|path| {
                                        let p_src = path.clone();
                                        let p_click = path.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="cover-mini-btn"
                                                on:click=move |_| {
                                                    cover_override.set(Some(p_click.clone()));
                                                    show_cover_picker.set(false);
                                                }
                                            >
                                                <img src=p_src alt="Cover wash" />
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </Show>
                </div>
            </div>

            // Top Action & Page-Turn Navigation Bar
            <header class="folio-action-bar">
                <div class="page-turn-group">
                    <button
                        type="button"
                        class="btn-editorial btn-ghost"
                        title="Previous folio"
                        on:click=move |_| on_page_turn.run(-1)
                    >
                        <IconChevronLeft />
                        <span>"Previous"</span>
                    </button>
                    <button
                        type="button"
                        class="btn-editorial btn-ghost"
                        title="Next folio"
                        on:click=move |_| on_page_turn.run(1)
                    >
                        <span>"Next"</span>
                        <IconChevronRight />
                    </button>
                </div>

                <div class="folio-mode-toggle" role="tablist" aria-label="Editor mode">
                    <button
                        type="button"
                        class="mode-tab-btn"
                        class:active=move || mode.get() == EditorMode::Compose
                        on:click=move |_| mode.set(EditorMode::Compose)
                    >
                        <IconFeather />
                        <span>"Compose"</span>
                    </button>
                    <button
                        type="button"
                        class="mode-tab-btn"
                        class:active=move || mode.get() == EditorMode::Reading
                        on:click=move |_| mode.set(EditorMode::Reading)
                    >
                        <IconEye />
                        <span>"Reading Folio"</span>
                    </button>
                </div>

                <div class="folio-save-group">
                    <span class="save-status-pill" class:is-dirty=move || dirty.get()>
                        <IconCheck />
                        <span>{move || save_status.get()}</span>
                    </span>

                    <button
                        type="button"
                        class="btn-editorial btn-sage"
                        disabled=move || saving.get()
                        on:click=move |_| trigger_save()
                    >
                        <span>{move || if saving.get() { "Saving..." } else { "Save Folio" }}</span>
                    </button>

                    {move || {
                        loaded_id.get().map(|id| {
                            if confirm_delete.get() {
                                view! {
                                    <button
                                        type="button"
                                        class="btn-editorial btn-terracotta"
                                        on:click=move |_| {
                                            confirm_delete.set(false);
                                            spawn_local(async move {
                                                if bridge::delete_entry(id).await.is_ok() {
                                                    on_deleted.run(id);
                                                }
                                            });
                                        }
                                    >
                                        <span>"Confirm Delete"</span>
                                    </button>
                                }.into_any()
                            } else {
                                view! {
                                    <button
                                        type="button"
                                        class="icon-btn subtle"
                                        title="Delete this folio"
                                        on:click=move |_| confirm_delete.set(true)
                                    >
                                        <IconTrash />
                                    </button>
                                }.into_any()
                            }
                        })
                    }}
                </div>
            </header>

            // Authentic Two-Column Book Spread Body
            <div class="book-spread-columns">
                // LEFT FOLIO: Literary Prose, Context Whisper Ribbon & Slash Composer
                <section class="folio-page-left">
                    // Interactive Stamp & Botanical Sticker Overlay
                    <StampStickerLayer
                        stamps=stamps.into()
                        on_change=Callback::new(move |next_stamps| {
                            stamps.set(next_stamps);
                            mark_dirty();
                        })
                    />

                    // Today's Context Whisper Ribbon
                    {move || {
                        daily_context.get().map(|ctx| {
                            let has_content = ctx.weather_hint.is_some()
                                || !ctx.events.is_empty()
                                || !ctx.suggested_prompts.is_empty();
                            has_content.then(|| {
                                view! {
                                    <div class="context-whisper-ribbon">
                                        <span class="small-caps-meta whisper-label">"Today's Context"</span>
                                        <div class="whisper-pills">
                                            {ctx.weather_hint.clone().map(|wh| {
                                                let wh_click = wh.clone();
                                                let temp_label = wh
                                                    .temperature_c
                                                    .map(|t| format!(" · {:.0}°C", t))
                                                    .unwrap_or_default();
                                                let label = format!("{}{}", wh.condition, temp_label);
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="whisper-pill"
                                                        title="Attach weather to this folio"
                                                        on:click=move |_| {
                                                            weather.set(Some(wh_click.clone()));
                                                            mark_dirty();
                                                        }
                                                    >
                                                        <WeatherGlyph icon_key=wh.icon_key.clone() />
                                                        <span>{label}</span>
                                                    </button>
                                                }
                                            })}

                                            {ctx.events.into_iter().map(|ev| {
                                                let time_prefix = ev
                                                    .start_time
                                                    .as_deref()
                                                    .map(|t| format!("{t} · "))
                                                    .unwrap_or_default();
                                                let pill_text = format!("{time_prefix}{}", ev.title);
                                                let prompt_block = format!(
                                                    "\n\n### {}\n> {}\n\n",
                                                    ev.title, ev.suggested_prompt
                                                );
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="whisper-pill"
                                                        title=ev.suggested_prompt.clone()
                                                        on:click=move |_| {
                                                            body.update(|b| b.push_str(&prompt_block));
                                                            mark_dirty();
                                                        }
                                                    >
                                                        <IconSpark />
                                                        <span>{pill_text}</span>
                                                    </button>
                                                }
                                            }).collect_view()}

                                            {ctx.suggested_prompts.into_iter().take(2).map(|prompt| {
                                                let snippet = format!("\n\n> {prompt}\n\n");
                                                let label = prompt.clone();
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="whisper-pill subtle"
                                                        on:click=move |_| {
                                                            body.update(|b| b.push_str(&snippet));
                                                            mark_dirty();
                                                        }
                                                    >
                                                        <IconFeather />
                                                        <span>{label}</span>
                                                    </button>
                                                }
                                            }).collect_view()}
                                        </div>
                                    </div>
                                }
                            })
                        })
                    }}

                    // Small-caps Date, Reading Time & Star Row
                    <div class="folio-dateline-row">
                        <span class="small-caps-meta">
                            {move || {
                                let date_str = loaded_date.get().format("%A · %B %d, %Y · %H:%M").to_string();
                                let wc = word_count.get();
                                let mins = reading_mins.get();
                                format!("{date_str}  ·  {wc} words  ·  {mins} min read")
                            }}
                        </span>

                        <button
                            type="button"
                            class="star-toggle-btn"
                            class:is-starred=move || starred.get()
                            title="Star this folio"
                            on:click=move |_| {
                                starred.update(|s| *s = !*s);
                                mark_dirty();
                            }
                        >
                            {move || view! { <IconStar filled=starred.get() /> }}
                        </button>
                    </div>

                    // Literary Serif Editable Title
                    <input
                        type="text"
                        class="folio-title-input"
                        placeholder="Title this folio..."
                        prop:value=move || title.get()
                        on:input=move |ev| {
                            title.set(event_target_value(&ev));
                            mark_dirty();
                        }
                    />

                    // Compact Metadata Strip: Notebook, Location, Mood, Weather, Tags
                    <div class="folio-metadata-strip">
                        <div class="meta-select-pill">
                            <IconBookOpen />
                            <select
                                class="editorial-select"
                                aria-label="Select notebook"
                                prop:value=move || notebook_id.get().unwrap_or_default()
                                on:change=move |ev| {
                                    let val = event_target_value(&ev);
                                    notebook_id.set(if val.is_empty() { None } else { Some(val) });
                                    mark_dirty();
                                }
                            >
                                <option value="">"Unassigned Folio"</option>
                                {move || {
                                    notebooks
                                        .get()
                                        .into_iter()
                                        .map(|nb| {
                                            let id_val = nb.id.clone();
                                            view! { <option value=id_val>{nb.name}</option> }
                                        })
                                        .collect_view()
                                }}
                            </select>
                        </div>

                        <div class="meta-pill-control">
                            <IconMapPin />
                            <Show
                                when=move || editing_location.get()
                                fallback=move || {
                                    view! {
                                        <button
                                            type="button"
                                            class="meta-inline-btn"
                                            on:click=move |_| editing_location.set(true)
                                        >
                                            {move || {
                                                location
                                                    .get()
                                                    .map(|l| l.place_name)
                                                    .filter(|s| !s.is_empty())
                                                    .unwrap_or_else(|| "Add place".to_string())
                                            }}
                                        </button>
                                    }
                                }
                            >
                                <input
                                    type="text"
                                    class="meta-inline-input"
                                    placeholder="City or desk..."
                                    prop:value=move || location_draft.get()
                                    on:input=move |ev| location_draft.set(event_target_value(&ev))
                                    on:keydown=move |ev| {
                                        if ev.key() == "Enter" || ev.key() == "Escape" {
                                            ev.prevent_default();
                                            let txt = location_draft.get_untracked().trim().to_string();
                                            if txt.is_empty() {
                                                location.set(None);
                                            } else {
                                                location.set(Some(GeoLocation {
                                                    place_name: txt,
                                                    city: None,
                                                    country: None,
                                                    latitude: None,
                                                    longitude: None,
                                                }));
                                            }
                                            editing_location.set(false);
                                            mark_dirty();
                                        }
                                    }
                                    on:blur=move |_| {
                                        let txt = location_draft.get_untracked().trim().to_string();
                                        if txt.is_empty() {
                                            location.set(None);
                                        } else {
                                            location.set(Some(GeoLocation {
                                                place_name: txt,
                                                city: None,
                                                country: None,
                                                latitude: None,
                                                longitude: None,
                                            }));
                                        }
                                        editing_location.set(false);
                                        mark_dirty();
                                    }
                                />
                            </Show>
                        </div>

                        <div class="meta-select-pill">
                            <IconSpark />
                            <select
                                class="editorial-select"
                                aria-label="Select mood"
                                prop:value=move || mood.get().unwrap_or_default()
                                on:change=move |ev| {
                                    let val = event_target_value(&ev);
                                    mood.set(if val.is_empty() { None } else { Some(val) });
                                    mark_dirty();
                                }
                            >
                                <option value="">"Mood..."</option>
                                {MOOD_PRESETS
                                    .iter()
                                    .map(|m| view! { <option value=*m>{*m}</option> })
                                    .collect_view()}
                            </select>
                        </div>

                        {move || {
                            weather.get().map(|w| {
                                let temp = w.temperature_c.map(|t| format!(" {:.0}°C", t)).unwrap_or_default();
                                view! {
                                    <span class="meta-pill-static">
                                        <WeatherGlyph icon_key=w.icon_key.clone() />
                                        <span>{format!("{}{}", w.condition, temp)}</span>
                                    </span>
                                }
                            })
                        }}
                    </div>

                    // Tags Strip
                    <div class="folio-tags-row">
                        {move || {
                            tags.get()
                                .into_iter()
                                .map(|tag| {
                                    let tag_del = tag.clone();
                                    view! {
                                        <span class="folio-tag-chip">
                                            <IconTag />
                                            <span>{tag}</span>
                                            <button
                                                type="button"
                                                class="tag-remove-btn"
                                                title="Remove tag"
                                                on:click=move |_| {
                                                    tags.update(|list| list.retain(|t| t != &tag_del));
                                                    mark_dirty();
                                                }
                                            >
                                                <IconClose />
                                            </button>
                                        </span>
                                    }
                                })
                                .collect_view()
                        }}

                        <input
                            type="text"
                            class="tag-inline-input"
                            placeholder="#add tag"
                            prop:value=move || new_tag_input.get()
                            on:input=move |ev| new_tag_input.set(event_target_value(&ev))
                            on:keydown=move |ev| {
                                if ev.key() == "Enter" || ev.key() == "," {
                                    ev.prevent_default();
                                    add_tag();
                                }
                            }
                            on:blur=move |_| add_tag()
                        />
                    </div>

                    // Subtle Fountain-Pen Insert Bar & Floating Popovers
                    <div class="folio-composer-toolbar">
                        <button
                            type="button"
                            class="btn-insert-pen"
                            on:click=open_insert_palette
                        >
                            <IconPlus />
                            <span>"Insert"</span>
                            <span class="slash-hint">"/"</span>
                        </button>

                        <button
                            type="button"
                            class="toolbar-mini-btn"
                            title="Attach Polaroid Photo"
                            on:click=move |_| {
                                if let Some(input) = photo_input_ref.get() {
                                    input.click();
                                }
                            }
                        >
                            <IconCamera />
                            <span>"Polaroid"</span>
                        </button>

                        <button
                            type="button"
                            class="toolbar-mini-btn"
                            title="Record Voice Memo"
                            on:click=move |_| show_voice_recorder.update(|v| *v = !*v)
                        >
                            <IconMic />
                            <span>"Voice"</span>
                        </button>

                        <button
                            type="button"
                            class="toolbar-mini-btn"
                            title="Place Botanical Sticker or Postal Stamp"
                            on:click=move |_| show_stamp_picker.update(|v| *v = !*v)
                        >
                            <IconStamp />
                            <span>"Stamp"</span>
                        </button>

                        <button
                            type="button"
                            class="toolbar-mini-btn"
                            title="Apply Reflection Template"
                            on:click=move |_| show_template_picker.update(|v| *v = !*v)
                        >
                            <IconFeather />
                            <span>"Template"</span>
                        </button>

                        <button
                            type="button"
                            class="toolbar-mini-btn"
                            title="Cite a Past Entry"
                            on:click=move |_| {
                                link_filter.set(String::new());
                                show_link_picker.update(|v| *v = !*v);
                            }
                        >
                            <IconLink />
                            <span>"Wikilink"</span>
                        </button>
                    </div>

                    // Notion-Style `/` Slash Command Palette
                    <Show when=move || slash_open.get()>
                        <div class="slash-palette-popover" role="listbox" aria-label="Slash insert commands">
                            <div class="slash-palette-header">
                                <span class="small-caps-meta">"Insert Block or Artifact"</span>
                                <span class="small-caps-meta">"↑↓ · Enter · Esc"</span>
                            </div>
                            <div class="slash-palette-list">
                                {move || {
                                    let items = filtered_slash.get();
                                    if items.is_empty() {
                                        view! {
                                            <div class="slash-empty">"No matching ink blocks"</div>
                                        }.into_any()
                                    } else {
                                        items
                                            .into_iter()
                                            .enumerate()
                                            .map(|(idx, item)| {
                                                let action = item.action;
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="slash-palette-item"
                                                        class:is-selected=move || slash_index.get() == idx
                                                        on:mouseenter=move |_| slash_index.set(idx)
                                                        on:click=move |_| execute_slash_action(action)
                                                    >
                                                        <span class="slash-item-glyph">{item.glyph}</span>
                                                        <div class="slash-item-text">
                                                            <span class="slash-item-title">{item.title}</span>
                                                            <span class="slash-item-sub">{item.subtitle}</span>
                                                        </div>
                                                        <span class="slash-item-cmd">{format!("/{}", item.id)}</span>
                                                    </button>
                                                }
                                            })
                                            .collect_view()
                                            .into_any()
                                    }
                                }}
                            </div>
                        </div>
                    </Show>

                    // Editorial Template Picker Popover
                    <Show when=move || show_template_picker.get()>
                        <div class="inline-picker-card">
                            <div class="inline-picker-header">
                                <span class="small-caps-meta">"Editorial Templates"</span>
                                <button
                                    type="button"
                                    class="icon-btn subtle"
                                    on:click=move |_| show_template_picker.set(false)
                                >
                                    <IconClose />
                                </button>
                            </div>
                            <div class="template-picker-grid">
                                {move || {
                                    templates
                                        .get()
                                        .into_iter()
                                        .map(|tpl| {
                                            let tpl_body = tpl.body_markdown.clone();
                                            let tpl_tags = tpl.default_tags.clone();
                                            let tpl_mood = tpl.default_mood.clone();
                                            view! {
                                                <button
                                                    type="button"
                                                    class="template-choice-card"
                                                    on:click=move |_| {
                                                        body.update(|b| {
                                                            if !b.trim().is_empty() {
                                                                b.push_str("\n\n");
                                                            }
                                                            b.push_str(&tpl_body);
                                                        });
                                                        for t in &tpl_tags {
                                                            tags.update(|list| {
                                                                if !list.iter().any(|x| x == t) {
                                                                    list.push(t.clone());
                                                                }
                                                            });
                                                        }
                                                        if mood.get_untracked().is_none() {
                                                            mood.set(tpl_mood.clone());
                                                        }
                                                        show_template_picker.set(false);
                                                        mark_dirty();
                                                    }
                                                >
                                                    <span class="template-choice-cat small-caps-meta">{tpl.category}</span>
                                                    <span class="template-choice-name">{tpl.name}</span>
                                                    <span class="template-choice-desc">{tpl.description}</span>
                                                </button>
                                            }
                                        })
                                        .collect_view()
                                }}
                            </div>
                        </div>
                    </Show>

                    // Quick Past-Entry Wikilink Picker Popover
                    <Show when=move || show_link_picker.get()>
                        <div class="inline-picker-card">
                            <div class="inline-picker-header">
                                <span class="small-caps-meta">"Cite Past Folio [[Wikilink]]"</span>
                                <button
                                    type="button"
                                    class="icon-btn subtle"
                                    on:click=move |_| show_link_picker.set(false)
                                >
                                    <IconClose />
                                </button>
                            </div>
                            <input
                                type="text"
                                class="editorial-input"
                                placeholder="Filter folio titles..."
                                prop:value=move || link_filter.get()
                                on:input=move |ev| link_filter.set(event_target_value(&ev))
                            />
                            <div class="wikilink-picker-list">
                                {move || {
                                    let q = link_filter.get().trim().to_lowercase();
                                    let current = loaded_id.get();
                                    all_entries
                                        .get()
                                        .into_iter()
                                        .filter(|e| Some(e.id) != current)
                                        .filter(|e| q.is_empty() || e.title.to_lowercase().contains(&q))
                                        .take(8)
                                        .map(|e| {
                                            let link_title = e.title.clone();
                                            let date_str = e.date.format("%b %d, %Y").to_string();
                                            view! {
                                                <button
                                                    type="button"
                                                    class="wikilink-choice-row"
                                                    on:click=move |_| {
                                                        let insertion = format!("[[{}]]", link_title);
                                                        let current_len = body.get_untracked().len();
                                                        let (s, end) = slash_range.get_untracked();
                                                        let pos = s.min(current_len);
                                                        insert_snippet_at(pos, end.min(current_len), &insertion);
                                                        show_link_picker.set(false);
                                                    }
                                                >
                                                    <IconLink />
                                                    <span class="wikilink-choice-title">{e.title}</span>
                                                    <span class="small-caps-meta">{date_str}</span>
                                                </button>
                                            }
                                        })
                                        .collect_view()
                                }}
                            </div>
                        </div>
                    </Show>

                    // Compose Textarea OR Formatted Reading Folio
                    <Show
                        when=move || mode.get() == EditorMode::Compose
                        fallback=move || {
                            view! {
                                <div class="folio-reading-prose prose-folio">
                                    {move || {
                                        let lines: Vec<(usize, String)> = body
                                            .get()
                                            .lines()
                                            .enumerate()
                                            .map(|(i, l)| (i, l.to_string()))
                                            .collect();
                                        let mut first_para_seen = false;
                                        lines
                                            .into_iter()
                                            .map(|(line_idx, raw)| {
                                                let trimmed = raw.trim();
                                                if trimmed.is_empty() {
                                                    return view! { <div class="folio-para-spacer"></div> }.into_any();
                                                }
                                                if trimmed == "---" || trimmed == "***" {
                                                    return view! { <hr class="folio-section-rule" /> }.into_any();
                                                }
                                                if let Some(h1) = trimmed.strip_prefix("# ") {
                                                    return view! {
                                                        <h2 class="folio-h1">{render_inline_view(h1, all_entries, on_open_entry)}</h2>
                                                    }.into_any();
                                                }
                                                if let Some(h2) = trimmed.strip_prefix("## ") {
                                                    return view! {
                                                        <h3 class="folio-h2">{render_inline_view(h2, all_entries, on_open_entry)}</h3>
                                                    }.into_any();
                                                }
                                                if let Some(h3) = trimmed.strip_prefix("### ") {
                                                    return view! {
                                                        <h4 class="folio-h3">{render_inline_view(h3, all_entries, on_open_entry)}</h4>
                                                    }.into_any();
                                                }
                                                if let Some(note) = trimmed.strip_prefix("> [!NOTE]") {
                                                    let content = note.trim();
                                                    return view! {
                                                        <aside class="folio-botanical-callout">
                                                            <IconLeaf />
                                                            <span>{render_inline_view(content, all_entries, on_open_entry)}</span>
                                                        </aside>
                                                    }.into_any();
                                                }
                                                if let Some(quote) = trimmed.strip_prefix("> ") {
                                                    return view! {
                                                        <blockquote class="folio-pull-quote">
                                                            {render_inline_view(quote, all_entries, on_open_entry)}
                                                        </blockquote>
                                                    }.into_any();
                                                }
                                                if let Some(todo) = trimmed.strip_prefix("- [ ] ").or_else(|| trimmed.strip_prefix("* [ ] ")) {
                                                    return view! {
                                                        <label class="folio-checklist-row">
                                                            <input
                                                                type="checkbox"
                                                                checked=false
                                                                on:change=move |_| {
                                                                    body.update(|b| {
                                                                        let mut updated: Vec<String> = b.lines().map(str::to_string).collect();
                                                                        if let Some(line) = updated.get_mut(line_idx) {
                                                                            *line = line.replacen("[ ]", "[x]", 1);
                                                                        }
                                                                        *b = updated.join("\n");
                                                                    });
                                                                    mark_dirty();
                                                                }
                                                            />
                                                            <span>{render_inline_view(todo, all_entries, on_open_entry)}</span>
                                                        </label>
                                                    }.into_any();
                                                }
                                                if let Some(done) = trimmed.strip_prefix("- [x] ").or_else(|| trimmed.strip_prefix("- [X] ")) {
                                                    return view! {
                                                        <label class="folio-checklist-row is-done">
                                                            <input
                                                                type="checkbox"
                                                                checked=true
                                                                on:change=move |_| {
                                                                    body.update(|b| {
                                                                        let mut updated: Vec<String> = b.lines().map(str::to_string).collect();
                                                                        if let Some(line) = updated.get_mut(line_idx) {
                                                                            *line = line.replacen("[x]", "[ ]", 1).replacen("[X]", "[ ]", 1);
                                                                        }
                                                                        *b = updated.join("\n");
                                                                    });
                                                                    mark_dirty();
                                                                }
                                                            />
                                                            <span>{render_inline_view(done, all_entries, on_open_entry)}</span>
                                                        </label>
                                                    }.into_any();
                                                }

                                                let cls = if !first_para_seen {
                                                    first_para_seen = true;
                                                    "folio-paragraph folio-drop-cap"
                                                } else {
                                                    "folio-paragraph"
                                                };
                                                view! {
                                                    <p class=cls>{render_inline_view(trimmed, all_entries, on_open_entry)}</p>
                                                }.into_any()
                                            })
                                            .collect_view()
                                    }}
                                </div>
                            }
                        }
                    >
                        <textarea
                            node_ref=textarea_ref
                            class="folio-prose-textarea prose-folio"
                            placeholder="Write slowly, or type '/' to weave Polaroids, voice memos, botanical stamps, or links..."
                            prop:value=move || body.get()
                            on:input=on_body_input
                            on:keydown=on_body_keydown
                        />
                    </Show>

                    // Extracted outgoing wikilinks quick-jump pills (when in Compose mode)
                    {move || {
                        let links = extract_wikilinks(&body.get());
                        (!links.is_empty()).then(|| {
                            view! {
                                <div class="outgoing-links-bar">
                                    <span class="small-caps-meta">"Citations"</span>
                                    {links.into_iter().map(|lnk| {
                                        let needle = lnk.to_lowercase();
                                        view! {
                                            <button
                                                type="button"
                                                class="folio-wikilink"
                                                on:click=move |_| {
                                                    let list = all_entries.get_untracked();
                                                    if let Some(found) = list.iter().find(|e| e.title.trim().to_lowercase() == needle)
                                                        .or_else(|| list.iter().find(|e| e.title.to_lowercase().contains(&needle)))
                                                    {
                                                        on_open_entry.run(found.id);
                                                    }
                                                }
                                            >
                                                <IconLink />
                                                <span>{lnk}</span>
                                            </button>
                                        }
                                    }).collect_view()}
                                </div>
                            }
                        })
                    }}
                </section>

                // RIGHT FOLIO: Scrapbook Polaroids, Voice Ribbons, Stamp Picker & Backlinks
                <aside class="folio-page-right">
                    // Stamp Picker Popover (triggered by `/stamp` or toolbar)
                    <Show when=move || show_stamp_picker.get()>
                        <StampPickerPopover
                            manifest=manifest
                            on_pick=Callback::new(move |placed| {
                                stamps.update(|list| list.push(placed));
                                show_stamp_picker.set(false);
                                mark_dirty();
                            })
                            on_close=Callback::new(move |_| show_stamp_picker.set(false))
                        />
                    </Show>

                    // Polaroid Gallery Section
                    <section class="scrapbook-section">
                        <div class="scrapbook-section-header">
                            <span class="small-caps-meta">"Scrapbook Polaroids"</span>
                            <button
                                type="button"
                                class="btn-editorial btn-ghost small"
                                on:click=move |_| {
                                    if let Some(input) = photo_input_ref.get() {
                                        input.click();
                                    }
                                }
                            >
                                <IconCamera />
                                <span>"Add Photo"</span>
                            </button>
                        </div>

                        <Show
                            when=move || !photos.get().is_empty()
                            fallback=|| view! {
                                <div
                                    class="polaroid-empty-dropzone"
                                    on:click=move |_| {}
                                >
                                    <IconCamera />
                                    <span>"Drop a photograph here or use /photo"</span>
                                </div>
                            }
                        >
                            <div class="polaroid-gallery-grid">
                                {move || {
                                    photos
                                        .get()
                                        .into_iter()
                                        .enumerate()
                                        .map(|(idx, photo)| {
                                            let photo_id_del = photo.id.clone();
                                            let photo_id_cap = photo.id.clone();
                                            view! {
                                                <PolaroidCard
                                                    photo=photo
                                                    figure_number=idx + 1
                                                    on_caption_change=Callback::new(move |new_cap: String| {
                                                        photos.update(|list| {
                                                            if let Some(p) = list.iter_mut().find(|x| x.id == photo_id_cap) {
                                                                p.caption = if new_cap.trim().is_empty() { None } else { Some(new_cap) };
                                                            }
                                                        });
                                                        mark_dirty();
                                                    })
                                                    on_remove=Callback::new(move |_| {
                                                        photos.update(|list| list.retain(|x| x.id != photo_id_del));
                                                        mark_dirty();
                                                    })
                                                />
                                            }
                                        })
                                        .collect_view()
                                }}
                            </div>
                        </Show>
                    </section>

                    // Voice Memo Ribbons Section
                    <section class="scrapbook-section">
                        <div class="scrapbook-section-header">
                            <span class="small-caps-meta">"Voice Ribbons"</span>
                            <button
                                type="button"
                                class="btn-editorial btn-ghost small"
                                on:click=move |_| show_voice_recorder.update(|v| *v = !*v)
                            >
                                <IconMic />
                                <span>{move || if show_voice_recorder.get() { "Hide Recorder" } else { "Record Audio" }}</span>
                            </button>
                        </div>

                        <Show when=move || show_voice_recorder.get()>
                            <VoiceRecorderControl
                                on_recorded=Callback::new(move |memo| {
                                    audio.update(|list| list.push(memo));
                                    show_voice_recorder.set(false);
                                    mark_dirty();
                                })
                                on_cancel=Callback::new(move |_| show_voice_recorder.set(false))
                            />
                        </Show>

                        <div class="voice-ribbons-stack">
                            {move || {
                                audio
                                    .get()
                                    .into_iter()
                                    .map(|memo| {
                                        view! {
                                            <WaveformPlayer
                                                memo=memo
                                                on_delete=Callback::new(move |del_id: String| {
                                                    audio.update(|list| list.retain(|m| m.id != del_id));
                                                    mark_dirty();
                                                })
                                            />
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </section>

                    // Placed Stamps & Botanicals Summary
                    <section class="scrapbook-section">
                        <div class="scrapbook-section-header">
                            <span class="small-caps-meta">
                                {move || format!("Ink Stamps & Stickers ({})", stamps.get().len())}
                            </span>
                            <button
                                type="button"
                                class="btn-editorial btn-ghost small"
                                on:click=move |_| show_stamp_picker.update(|v| *v = !*v)
                            >
                                <IconStamp />
                                <span>"Place Stamp"</span>
                            </button>
                        </div>
                    </section>

                    // Archival Footer: Referenced In (Backlinks)
                    <footer class="backlinks-archival-footer">
                        <span class="small-caps-meta">"Referenced In (Backlinks)"</span>
                        {move || {
                            let list = backlinks.get();
                            if list.is_empty() {
                                view! {
                                    <p class="backlinks-empty">"No folios cite this entry yet. Link entries with [[Title]]."</p>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="backlinks-list">
                                        {list.into_iter().map(|bl| {
                                            let target_id = bl.entry_id;
                                            let date_label = bl.date.format("%b %d, %Y").to_string();
                                            view! {
                                                <button
                                                    type="button"
                                                    class="backlink-card"
                                                    on:click=move |_| on_open_entry.run(target_id)
                                                >
                                                    <div class="backlink-card-head">
                                                        <IconLink />
                                                        <span class="backlink-title">{bl.title}</span>
                                                        <span class="small-caps-meta">{date_label}</span>
                                                    </div>
                                                    {(!bl.snippet.is_empty()).then(|| view! {
                                                        <p class="backlink-snippet">{bl.snippet}</p>
                                                    })}
                                                </button>
                                            }
                                        }).collect_view()}
                                    </div>
                                }.into_any()
                            }
                        }}
                    </footer>
                </aside>
            </div>
        </article>
    }
}
