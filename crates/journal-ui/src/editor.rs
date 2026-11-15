//! Single-sheet vertical-scroll entry page with auto-growing prose, `/` slash palette,
//! inline metadata, optional cover wash, photographs, voice notes, and Entry Details ledger.

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
use web_sys::{HtmlElement, HtmlInputElement, HtmlTextAreaElement};

use crate::audio_recorder::{VoiceRecorderControl, WaveformPlayer};
use crate::bridge;
use crate::icons::{
    IconCamera, IconChevronLeft, IconChevronRight, IconClose, IconLeaf, IconLink, IconMapPin,
    IconMic, IconPlus, IconSpark, IconStamp, IconStar, IconTrash, WeatherGlyph,
};
use crate::views::{PolaroidCard, StampPickerPopover, StampStickerLayer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorMode {
    Write,
    Read,
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
        subtitle: "Large section heading",
        glyph: "H1",
        action: SlashAction::Heading1,
    },
    SlashCommandItem {
        id: "h2",
        title: "Heading II",
        subtitle: "Subheading",
        glyph: "H2",
        action: SlashAction::Heading2,
    },
    SlashCommandItem {
        id: "todo",
        title: "Checklist",
        subtitle: "Checkbox task item",
        glyph: "[ ]",
        action: SlashAction::Todo,
    },
    SlashCommandItem {
        id: "quote",
        title: "Pull-Quote",
        subtitle: "Indented quotation",
        glyph: "\"",
        action: SlashAction::Quote,
    },
    SlashCommandItem {
        id: "callout",
        title: "Margin Note",
        subtitle: "Highlighted note block",
        glyph: "!",
        action: SlashAction::Callout,
    },
    SlashCommandItem {
        id: "divider",
        title: "Section Break",
        subtitle: "Horizontal rule",
        glyph: "---",
        action: SlashAction::Divider,
    },
    SlashCommandItem {
        id: "photo",
        title: "Photograph",
        subtitle: "Attach a photo from disk",
        glyph: "IMG",
        action: SlashAction::Photo,
    },
    SlashCommandItem {
        id: "voice",
        title: "Voice Note",
        subtitle: "Record mic or upload audio",
        glyph: "REC",
        action: SlashAction::Voice,
    },
    SlashCommandItem {
        id: "stamp",
        title: "Stamp or Sticker",
        subtitle: "Botanical sticker or postmark",
        glyph: "INK",
        action: SlashAction::Stamp,
    },
    SlashCommandItem {
        id: "template",
        title: "Template",
        subtitle: "Insert reflection prompts",
        glyph: "TPL",
        action: SlashAction::Template,
    },
    SlashCommandItem {
        id: "link",
        title: "Link Entry",
        subtitle: "Insert [[wikilink]] reference",
        glyph: "[[]]",
        action: SlashAction::LinkEntry,
    },
];

const DEFAULT_COVERS: [(&str, &str); 4] = [
    ("Botanical Fern", "/assets/covers/botanical-fern-wash.png"),
    ("Rainy Tea Desk", "/assets/covers/rainy-tea-desk.png"),
    ("Pressed Wildflowers", "/assets/covers/pressed-wildflowers.png"),
    ("Midnight Ink Garden", "/assets/covers/midnight-ink-garden.png"),
];

const MOOD_PRESETS: [&str; 6] = [
    "reflective",
    "calm",
    "grateful",
    "inspired",
    "rainy",
    "unhurried",
];

fn strip_markdown_images(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("![") {
        out.push_str(&rest[..start]);
        let after_bang = &rest[start + 2..];
        if let Some(mid) = after_bang.find("](") {
            let after_mid = &after_bang[mid + 2..];
            if let Some(end) = after_mid.find(')') {
                rest = &after_mid[end + 1..];
                continue;
            }
        }
        out.push_str("![");
        rest = after_bang;
    }
    out.push_str(rest);
    out
}

fn normalize_for_title_cmp(s: &str) -> String {
    strip_markdown_images(s)
        .trim()
        .trim_end_matches('…')
        .trim_end_matches("...")
        .chars()
        .filter(|c| !matches!(c, '#' | '*' | '_' | '`' | '>'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Returns `true` when `title` is empty, generic (`Untitled Entry`/`Untitled Folio`),
/// or auto-derived from the opening characters of `body_or_snippet` (common in Day One imports).
fn is_auto_derived_title(title: &str, body_or_snippet: &str) -> bool {
    let t = title.trim();
    if t.is_empty()
        || t.eq_ignore_ascii_case("Untitled Entry")
        || t.eq_ignore_ascii_case("Untitled Folio")
    {
        return true;
    }

    let norm_title = normalize_for_title_cmp(t);
    let norm_body = normalize_for_title_cmp(body_or_snippet);
    if norm_title.chars().count() < 10 || norm_body.is_empty() {
        return false;
    }

    let prefix: String = norm_title.chars().take(28).collect();
    norm_body.starts_with(&prefix) || norm_body.starts_with(&norm_title)
}

fn derive_fallback_title(body: &str) -> String {
    let first_line = body
        .lines()
        .map(|l| strip_markdown_images(l.trim()))
        .map(|l| l.trim().trim_start_matches('#').trim().to_string())
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    if first_line.is_empty() {
        return "Untitled Entry".to_string();
    }
    let count = first_line.chars().count();
    if count <= 64 {
        first_line
    } else {
        let mut out: String = first_line.chars().take(60).collect();
        if let Some(space) = out.rfind(' ') {
            if space > 20 {
                out.truncate(space);
            }
        }
        format!("{out}…")
    }
}

fn resize_textarea(textarea_ref: &NodeRef<leptos::html::Textarea>) {
    if let Some(el) = textarea_ref.get() {
        let html_el: &HtmlElement = el.as_ref();
        let style = html_el.style();
        let _ = style.set_property("height", "auto");
        let next_h = el.scroll_height().max(480);
        let _ = style.set_property("height", &format!("{next_h}px"));
    }
}

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

/// Extract `[[wikilink]]` titles from Markdown prose.
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

fn format_provenance(source: Option<&str>) -> String {
    match source.map(str::trim) {
        Some("Day One") | Some("dayone") => "Imported from Day One".to_string(),
        Some("Notion") | Some("notion") => "Imported from Notion".to_string(),
        Some("log") | Some("") | None => "Local Vault".to_string(),
        Some(other) => other.to_string(),
    }
}

fn parse_standalone_image(trimmed: &str) -> Option<(String, String)> {
    let inner = trimmed.strip_prefix("![")?.strip_suffix(')')?;
    let (alt, src) = inner.split_once("](")?;
    if alt.contains(']') || src.contains('(') || src.contains(')') {
        return None;
    }
    let src_clean = src.trim();
    if src_clean.is_empty() {
        return None;
    }
    Some((alt.trim().to_string(), src_clean.to_string()))
}

fn is_safe_href(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with('/')
        || lower.starts_with("mailto:")
}

#[derive(Debug, Clone)]
enum InlineSpan {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    Wikilink(String),
    Link { label: String, url: String },
    Image { alt: String, src: String },
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
        // Inline ![alt](src)
        if i + 1 < chars.len() && chars[i] == '!' && chars[i + 1] == '[' {
            if let Some(rel_close_alt) = chars[i + 2..].iter().position(|&c| c == ']') {
                let mid_idx = i + 2 + rel_close_alt;
                if mid_idx + 1 < chars.len() && chars[mid_idx + 1] == '(' {
                    if let Some(rel_close_url) = chars[mid_idx + 2..].iter().position(|&c| c == ')')
                    {
                        let alt: String = chars[i + 2..mid_idx].iter().collect();
                        let src: String = chars[mid_idx + 2..mid_idx + 2 + rel_close_url]
                            .iter()
                            .collect();
                        if !src.trim().is_empty() {
                            flush_buf(&mut buf, &mut spans);
                            spans.push(InlineSpan::Image {
                                alt: alt.trim().to_string(),
                                src: src.trim().to_string(),
                            });
                            i = mid_idx + 3 + rel_close_url;
                            continue;
                        }
                    }
                }
            }
        }
        if i + 1 < chars.len() && chars[i] == '[' && chars[i + 1] == '[' {
            if let Some(rel_end) = chars[i + 2..].windows(2).position(|w| w == [']', ']']) {
                let target: String = chars[i + 2..i + 2 + rel_end].iter().collect();
                flush_buf(&mut buf, &mut spans);
                spans.push(InlineSpan::Wikilink(target.trim().to_string()));
                i = i + 4 + rel_end;
                continue;
            }
        }
        // Standard Markdown [label](url)
        if chars[i] == '[' {
            if let Some(rel_close_lbl) = chars[i + 1..].iter().position(|&c| c == ']') {
                let mid_idx = i + 1 + rel_close_lbl;
                if mid_idx + 1 < chars.len() && chars[mid_idx + 1] == '(' {
                    if let Some(rel_close_url) = chars[mid_idx + 2..].iter().position(|&c| c == ')')
                    {
                        let label: String = chars[i + 1..mid_idx].iter().collect();
                        let url: String = chars[mid_idx + 2..mid_idx + 2 + rel_close_url]
                            .iter()
                            .collect();
                        let url_trimmed = url.trim().to_string();
                        if !label.is_empty() && is_safe_href(&url_trimmed) {
                            flush_buf(&mut buf, &mut spans);
                            spans.push(InlineSpan::Link {
                                label,
                                url: url_trimmed,
                            });
                            i = mid_idx + 3 + rel_close_url;
                            continue;
                        }
                    }
                }
            }
        }
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(rel_end) = chars[i + 2..].windows(2).position(|w| w == ['*', '*']) {
                let inner: String = chars[i + 2..i + 2 + rel_end].iter().collect();
                flush_buf(&mut buf, &mut spans);
                spans.push(InlineSpan::Bold(inner));
                i = i + 4 + rel_end;
                continue;
            }
        }
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
            InlineSpan::Code(c) => view! { <code class="entry-inline-code">{c}</code> }.into_any(),
            InlineSpan::Wikilink(target) => {
                let target_click = target.clone();
                let title_attr = format!("Open entry: {target}");
                view! {
                    <button
                        type="button"
                        class="entry-wikilink"
                        title=title_attr
                        on:click=move |_| {
                            let list = entries.get_untracked();
                            let needle = target_click.trim().to_lowercase();
                            if let Some(found) = list
                                .iter()
                                .find(|e| e.title.trim().to_lowercase() == needle)
                                .or_else(|| {
                                    list.iter().find(|e| e.title.to_lowercase().contains(&needle))
                                })
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
            InlineSpan::Link { label, url } => view! {
                <a
                    class="entry-external-link"
                    href=url
                    target="_blank"
                    rel="noopener noreferrer"
                >
                    {label}
                </a>
            }
            .into_any(),
            InlineSpan::Image { alt, src } => {
                let alt_cap = alt.clone();
                view! {
                    <figure class="entry-inline-figure">
                        <img src=src alt=alt loading="lazy" />
                        {(!alt_cap.is_empty()).then(|| view! { <figcaption>{alt_cap}</figcaption> })}
                    </figure>
                }
                .into_any()
            }
        })
        .collect_view()
}

/// Generous single-sheet vertical-scroll journal entry page.
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

    // Working entry state synchronized whenever `entry` changes.
    let loaded_id = RwSignal::new(Option::<Uuid>::None);
    let loaded_date = RwSignal::new(chrono::Utc::now());
    let loaded_updated_at = RwSignal::new(chrono::Utc::now());
    let loaded_source = RwSignal::new(Option::<String>::None);
    let loaded_rel_path = RwSignal::new(String::new());
    let last_reset_nonce = RwSignal::new(0_u64);

    let title = RwSignal::new(String::new());
    let auto_fallback_title = RwSignal::new(Option::<String>::None);
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

    let mode = RwSignal::new(EditorMode::Write);
    let cover_override = RwSignal::new(Option::<String>::None);
    let show_cover_picker = RwSignal::new(false);
    let show_voice_recorder = RwSignal::new(false);
    let show_stamp_picker = RwSignal::new(false);
    let show_template_picker = RwSignal::new(false);
    let show_link_picker = RwSignal::new(false);
    let link_filter = RwSignal::new(String::new());
    let prompt_dismissed = RwSignal::new(false);

    let new_tag_input = RwSignal::new(String::new());
    let editing_location = RwSignal::new(false);
    let location_draft = RwSignal::new(String::new());
    let editing_weather = RwSignal::new(false);
    let weather_draft = RwSignal::new(String::new());

    // Slash palette state
    let slash_open = RwSignal::new(false);
    let slash_query = RwSignal::new(String::new());
    let slash_index = RwSignal::new(0_usize);
    let slash_range = RwSignal::new((0_usize, 0_usize));

    // Persistence & feedback state
    let dirty = RwSignal::new(false);
    let saving = RwSignal::new(false);
    let save_status = RwSignal::new("Saved".to_string());
    let save_revision = RwSignal::new(0_u64);
    let confirm_delete = RwSignal::new(false);
    let drag_over = RwSignal::new(false);

    // Sync working state when parent selects a different entry (or creates a fresh one).
    Effect::new(move |_| {
        if let Some(current) = entry.get() {
            let incoming_id = Some(current.id);
            let should_reset = loaded_id.get_untracked() != incoming_id
                || (!dirty.get_untracked()
                    && current.frontmatter.updated_at != loaded_updated_at.get_untracked());

            if should_reset {
                loaded_id.set(incoming_id);
                loaded_date.set(current.frontmatter.date);
                loaded_updated_at.set(current.frontmatter.updated_at);
                loaded_source.set(current.frontmatter.source.clone());
                loaded_rel_path.set(current.path.clone());

                if is_auto_derived_title(&current.frontmatter.title, &current.body) {
                    auto_fallback_title.set(Some(current.frontmatter.title.clone()));
                    title.set(String::new());
                } else {
                    auto_fallback_title.set(None);
                    title.set(current.frontmatter.title.clone());
                }

                body.set(current.body.clone());
                notebook_id.set(current.frontmatter.notebook_id.clone());
                tags.set(current.frontmatter.tags.clone());
                mood.set(current.frontmatter.mood.clone());
                starred.set(current.frontmatter.starred);
                weather.set(current.frontmatter.weather.clone());
                weather_draft.set(
                    current
                        .frontmatter
                        .weather
                        .as_ref()
                        .map(|w| w.condition.clone())
                        .unwrap_or_default(),
                );
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
                prompt_dismissed.set(false);
                save_status.set("Saved".to_string());

                // Default to Read mode for existing non-empty entries, Write mode for empty drafts?
                // Wait: keeping Write mode or preserving user's current mode lets users click straight into prose,
                // and auto-growing textarea ensures full reading comfort either way.
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
                loaded_updated_at.set(chrono::Utc::now());
                loaded_source.set(Some("log".to_string()));
                loaded_rel_path.set(String::new());
                auto_fallback_title.set(None);
                title.set(String::new());
                body.set(String::new());
                notebook_id.set(default_notebook_id.and_then(|s| s.get_untracked()));
                tags.set(Vec::new());
                mood.set(None);
                starred.set(false);
                weather.set(None);
                weather_draft.set(String::new());
                location.set(None);
                location_draft.set(String::new());
                stamps.set(Vec::new());
                photos.set(Vec::new());
                audio.set(Vec::new());
                backlinks.set(Vec::new());
                cover_override.set(None);
                dirty.set(false);
                confirm_delete.set(false);
                prompt_dismissed.set(false);
                mode.set(EditorMode::Write);
                save_status.set("New draft".to_string());
            }
        }
    });

    // Auto-grow prose textarea whenever entry, mode, or body changes.
    Effect::new(move |_| {
        let _ = loaded_id.get();
        let _ = mode.get();
        let _ = body.get();
        resize_textarea(&textarea_ref);
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            resize_textarea(&textarea_ref);
        });
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

        let effective_title = if !raw_title.is_empty() {
            raw_title
        } else if let Some(fb) = auto_fallback_title.get_untracked() {
            if !fb.trim().is_empty() {
                fb
            } else {
                derive_fallback_title(&raw_body)
            }
        } else {
            derive_fallback_title(&raw_body)
        };

        saving.set(true);
        save_status.set("Saving…".to_string());

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
                    loaded_updated_at.set(saved.frontmatter.updated_at);
                    loaded_source.set(saved.frontmatter.source.clone());
                    loaded_rel_path.set(saved.path.clone());
                    backlinks.set(saved.backlinks.clone());
                    save_status.set("Saved".to_string());
                    on_saved.run(saved);
                }
                Err(err) => {
                    saving.set(false);
                    save_status.set(format!("Error: {err}"));
                }
            }
        });
    };

    // Mark dirty & schedule 1.4s debounced auto-save.
    let mark_dirty = move || {
        dirty.set(true);
        save_status.set("Unsaved".to_string());
        let next_rev = save_revision.get_untracked().wrapping_add(1);
        save_revision.set(next_rev);
        spawn_local(async move {
            TimeoutFuture::new(1400).await;
            if save_revision.get_untracked() == next_rev && dirty.get_untracked() {
                trigger_save();
            }
        });
    };

    // Live metrics for header & Entry Details ledger.
    let word_count = Memo::new(move |_| Entry::count_words(&body.get()));
    let char_count = Memo::new(move |_| body.get().chars().count());
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

    // Replace range (byte offsets) in `body` and position textarea cursor.
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
        resize_textarea(&textarea_ref);
    };

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

    let handle_photo_file = move |file: web_sys::File| {
        save_status.set("Uploading photo…".to_string());
        spawn_local(async move {
            match bridge::upload_photo(&file).await {
                Ok(mut polaroid) => {
                    let count = photos.get_untracked().len();
                    let tilts = [-1.5_f32, 1.2, -0.8, 1.6, -1.1, 0.9];
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
                    save_status.set(format!("Upload error: {err}"));
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

    let on_body_input = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        body.set(val.clone());
        resize_textarea(&textarea_ref);
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
        if (ev.ctrl_key() || ev.meta_key()) && ev.key().eq_ignore_ascii_case("e") {
            ev.prevent_default();
            mode.update(|m| {
                *m = match *m {
                    EditorMode::Write => EditorMode::Read,
                    EditorMode::Read => EditorMode::Write,
                }
            });
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

    let commit_location = move || {
        let txt = location_draft.get_untracked().trim().to_string();
        if txt.is_empty() {
            location.set(None);
        } else {
            let prev = location.get_untracked();
            location.set(Some(GeoLocation {
                place_name: txt,
                city: prev.as_ref().and_then(|l| l.city.clone()),
                country: prev.as_ref().and_then(|l| l.country.clone()),
                latitude: prev.as_ref().and_then(|l| l.latitude),
                longitude: prev.as_ref().and_then(|l| l.longitude),
            }));
        }
        editing_location.set(false);
        mark_dirty();
    };

    let commit_weather = move || {
        let txt = weather_draft.get_untracked().trim().to_string();
        if txt.is_empty() {
            weather.set(None);
        } else {
            let lower = txt.to_lowercase();
            let icon_key = if lower.contains("rain") || lower.contains("drizzle") {
                "rain".to_string()
            } else if lower.contains("sun") || lower.contains("clear") {
                "clear-day".to_string()
            } else {
                "cloudy".to_string()
            };
            let prev_temp = weather.get_untracked().and_then(|w| w.temperature_c);
            weather.set(Some(WeatherInfo {
                condition: txt,
                temperature_c: prev_temp,
                icon_key,
                humidity_pct: None,
                summary: None,
            }));
        }
        editing_weather.set(false);
        mark_dirty();
    };

    let open_insert_at_cursor = move || {
        let current = body.get_untracked();
        let cursor = textarea_ref
            .get()
            .and_then(|ta: HtmlTextAreaElement| ta.selection_start().ok().flatten())
            .map(|p| utf16_offset_to_byte_idx(&current, p as usize))
            .unwrap_or(current.len());
        slash_range.set((cursor, cursor));
    };

    let resolved_notebook_name = Memo::new(move |_| {
        if let Some(nb_id) = notebook_id.get() {
            if let Some(nb) = notebooks.get().into_iter().find(|n| n.id == nb_id) {
                return nb.name;
            }
            return nb_id;
        }
        "Daily".to_string()
    });

    view! {
        <article
            class=move || {
                let turn_cls = page_turn_class.map(|s| s.get()).unwrap_or("");
                if drag_over.get() {
                    format!("entry-page-sheet is-drag-over {turn_cls}")
                } else {
                    format!("entry-page-sheet {turn_cls}")
                }
            }
            on:dragover=move |ev: leptos::ev::DragEvent| {
                ev.prevent_default();
                drag_over.set(true);
            }
            on:dragleave=move |_| drag_over.set(false)
            on:drop=on_drop_file
        >
            <input
                node_ref=photo_input_ref
                type="file"
                accept="image/*"
                class="sr-only"
                on:change=on_photo_input_change
            />

            // Optional cover banner — rendered ONLY when explicitly chosen by the user.
            {move || {
                cover_override.get().map(|cover_url| {
                    view! {
                        <div class="entry-cover-banner">
                            <img src=cover_url alt="Entry cover wash" class="entry-cover-img" />
                            <button
                                type="button"
                                class="entry-cover-remove-btn"
                                title="Remove cover"
                                on:click=move |_| cover_override.set(None)
                            >
                                <IconClose />
                            </button>
                        </div>
                    }
                })
            }}

            <div class="entry-column">
                // Quiet Top Bar
                <header class="entry-topbar">
                    <div class="entry-topbar-left">
                        <button
                            type="button"
                            class="topbar-ghost-btn"
                            title="Previous entry"
                            on:click=move |_| on_page_turn.run(-1)
                        >
                            <IconChevronLeft />
                            <span>"Prev"</span>
                        </button>
                        <button
                            type="button"
                            class="topbar-ghost-btn"
                            title="Next entry"
                            on:click=move |_| on_page_turn.run(1)
                        >
                            <span>"Next"</span>
                            <IconChevronRight />
                        </button>

                        <span class="topbar-divider" aria-hidden="true"></span>

                        <select
                            class="topbar-notebook-select"
                            aria-label="Select notebook"
                            prop:value=move || notebook_id.get().unwrap_or_default()
                            on:change=move |ev| {
                                let val = event_target_value(&ev);
                                notebook_id.set(if val.is_empty() { None } else { Some(val) });
                                mark_dirty();
                            }
                        >
                            <option value="">"Notebook"</option>
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

                    <div class="entry-topbar-right">
                        <span
                            class="entry-save-indicator"
                            class:is-dirty=move || dirty.get()
                        >
                            {move || save_status.get()}
                        </span>

                        <div class="entry-mode-switch" role="tablist" aria-label="Editor mode">
                            <button
                                type="button"
                                class="mode-switch-btn"
                                class:is-active=move || mode.get() == EditorMode::Write
                                on:click=move |_| mode.set(EditorMode::Write)
                            >
                                "Write"
                            </button>
                            <button
                                type="button"
                                class="mode-switch-btn"
                                class:is-active=move || mode.get() == EditorMode::Read
                                on:click=move |_| mode.set(EditorMode::Read)
                            >
                                "Read"
                            </button>
                        </div>

                        <button
                            type="button"
                            class="topbar-icon-btn"
                            class:is-starred=move || starred.get()
                            title="Star entry"
                            aria-label="Star entry"
                            on:click=move |_| {
                                starred.update(|s| *s = !*s);
                                mark_dirty();
                            }
                        >
                            {move || view! { <IconStar filled=starred.get() /> }}
                        </button>

                        <button
                            type="button"
                            class="topbar-icon-btn"
                            title="Cover wash"
                            aria-label="Toggle cover wash picker"
                            on:click=move |_| show_cover_picker.update(|v| *v = !*v)
                        >
                            <IconLeaf />
                        </button>

                        {move || {
                            loaded_id.get().map(|id| {
                                if confirm_delete.get() {
                                    view! {
                                        <span class="topbar-delete-confirm">
                                            <button
                                                type="button"
                                                class="topbar-danger-btn"
                                                on:click=move |_| {
                                                    confirm_delete.set(false);
                                                    spawn_local(async move {
                                                        if bridge::delete_entry(id).await.is_ok() {
                                                            on_deleted.run(id);
                                                        }
                                                    });
                                                }
                                            >
                                                "Delete?"
                                            </button>
                                            <button
                                                type="button"
                                                class="topbar-ghost-btn"
                                                on:click=move |_| confirm_delete.set(false)
                                            >
                                                "Cancel"
                                            </button>
                                        </span>
                                    }.into_any()
                                } else {
                                    view! {
                                        <button
                                            type="button"
                                            class="topbar-icon-btn"
                                            title="Delete entry"
                                            aria-label="Delete entry"
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

                // Optional Cover Swatch Popover
                <Show when=move || show_cover_picker.get()>
                    <div class="cover-swatch-popover">
                        {move || {
                            let covers = manifest.get().covers;
                            let items: Vec<(String, String)> = if covers.is_empty() {
                                DEFAULT_COVERS
                                    .iter()
                                    .map(|(l, p)| ((*l).to_string(), (*p).to_string()))
                                    .collect()
                            } else {
                                covers.into_iter().map(|c| (c.label, c.path)).collect()
                            };
                            items
                                .into_iter()
                                .map(|(label, path)| {
                                    let p_src = path.clone();
                                    let p_click = path.clone();
                                    let title_attr = label.clone();
                                    view! {
                                        <button
                                            type="button"
                                            class="cover-mini-btn"
                                            title=title_attr
                                            on:click=move |_| {
                                                cover_override.set(Some(p_click.clone()));
                                                show_cover_picker.set(false);
                                            }
                                        >
                                            <img src=p_src alt=label />
                                        </button>
                                    }
                                })
                                .collect_view()
                        }}
                        <Show when=move || cover_override.get().is_some()>
                            <button
                                type="button"
                                class="topbar-ghost-btn"
                                on:click=move |_| {
                                    cover_override.set(None);
                                    show_cover_picker.set(false);
                                }
                            >
                                "None"
                            </button>
                        </Show>
                    </div>
                </Show>

                // Entry Header: Dateline, Optional Title, Quiet Inline Metadata
                <header class="entry-header">
                    <div class="entry-dateline">
                        <span class="small-caps-meta">
                            {move || {
                                loaded_date
                                    .get()
                                    .format("%A, %B %d, %Y · %-I:%M %p")
                                    .to_string()
                                    .to_uppercase()
                            }}
                        </span>

                        {move || {
                            let otd = daily_context
                                .get()
                                .map(|c| c.on_this_day_count)
                                .unwrap_or(0);
                            (otd > 0).then(|| {
                                view! {
                                    <span class="entry-dateline-otd">
                                        {format!("· {otd} On This Day")}
                                    </span>
                                }
                            })
                        }}
                    </div>

                    <Show
                        when=move || mode.get() == EditorMode::Write
                        fallback=move || {
                            let t = title.get();
                            let trimmed = t.trim().to_string();
                            (!trimmed.is_empty()).then(|| {
                                view! { <h1 class="entry-title-heading">{trimmed}</h1> }
                            })
                        }
                    >
                        <input
                            type="text"
                            class="entry-title-input"
                            placeholder="Title (optional)"
                            prop:value=move || title.get()
                            on:input=move |ev| {
                                title.set(event_target_value(&ev));
                                auto_fallback_title.set(None);
                                mark_dirty();
                            }
                        />
                    </Show>

                    // Quiet Inline Metadata Strip
                    <div class="entry-meta-inline">
                        // Location
                        <div class="meta-inline-item">
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
                                                    .unwrap_or_else(|| "+ Add location".to_string())
                                            }}
                                        </button>
                                    }
                                }
                            >
                                <input
                                    type="text"
                                    class="meta-inline-input"
                                    placeholder="City or place…"
                                    prop:value=move || location_draft.get()
                                    on:input=move |ev| location_draft.set(event_target_value(&ev))
                                    on:keydown=move |ev| {
                                        if ev.key() == "Enter" || ev.key() == "Escape" {
                                            ev.prevent_default();
                                            commit_location();
                                        }
                                    }
                                    on:blur=move |_| commit_location()
                                />
                            </Show>
                        </div>

                        // Weather
                        <div class="meta-inline-item">
                            {move || match weather.get() {
                                Some(w) => view! { <WeatherGlyph icon_key=w.icon_key /> }.into_any(),
                                None => view! { <IconSpark /> }.into_any(),
                            }}
                            <Show
                                when=move || editing_weather.get()
                                fallback=move || {
                                    view! {
                                        <button
                                            type="button"
                                            class="meta-inline-btn"
                                            on:click=move |_| editing_weather.set(true)
                                        >
                                            {move || {
                                                weather
                                                    .get()
                                                    .map(|w| {
                                                        let temp = w
                                                            .temperature_c
                                                            .map(|t| format!(" · {:.0}°C", t))
                                                            .unwrap_or_default();
                                                        format!("{}{}", w.condition, temp)
                                                    })
                                                    .unwrap_or_else(|| "+ Weather".to_string())
                                            }}
                                        </button>
                                    }
                                }
                            >
                                <input
                                    type="text"
                                    class="meta-inline-input"
                                    placeholder="e.g. Light Rain"
                                    prop:value=move || weather_draft.get()
                                    on:input=move |ev| weather_draft.set(event_target_value(&ev))
                                    on:keydown=move |ev| {
                                        if ev.key() == "Enter" || ev.key() == "Escape" {
                                            ev.prevent_default();
                                            commit_weather();
                                        }
                                    }
                                    on:blur=move |_| commit_weather()
                                />
                            </Show>
                        </div>

                        // Mood
                        <div class="meta-inline-item">
                            <select
                                class="meta-inline-select"
                                aria-label="Mood"
                                prop:value=move || mood.get().unwrap_or_default()
                                on:change=move |ev| {
                                    let val = event_target_value(&ev);
                                    mood.set(if val.is_empty() { None } else { Some(val) });
                                    mark_dirty();
                                }
                            >
                                <option value="">"+ Mood"</option>
                                {MOOD_PRESETS
                                    .iter()
                                    .map(|m| view! { <option value=*m>{*m}</option> })
                                    .collect_view()}
                            </select>
                        </div>

                        // Inline Tags (zero pills)
                        {move || {
                            tags.get()
                                .into_iter()
                                .map(|tag| {
                                    let tag_del = tag.clone();
                                    view! {
                                        <span class="meta-tag-item">
                                            <span>{format!("#{tag}")}</span>
                                            <button
                                                type="button"
                                                class="meta-tag-remove"
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
                            class="meta-tag-input"
                            placeholder="+ tag"
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
                </header>

                // Blank-Entry Context Prompt (rendered ONLY on empty drafts until dismissed)
                <Show when=move || body.get().trim().is_empty() && !prompt_dismissed.get()>
                    {move || {
                        daily_context.get().and_then(|ctx| {
                            let first_event = ctx.events.first().cloned();
                            let first_prompt = ctx.suggested_prompts.first().cloned();
                            let wh = ctx.weather_hint.clone();
                            (first_event.is_some() || first_prompt.is_some() || wh.is_some()).then(|| {
                                view! {
                                    <div class="entry-blank-prompt">
                                        <div class="blank-prompt-items">
                                            {wh.map(|w| {
                                                let w_click = w.clone();
                                                let temp = w.temperature_c.map(|t| format!(" · {:.0}°C", t)).unwrap_or_default();
                                                let lbl = format!("Attach weather: {}{}", w.condition, temp);
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="blank-prompt-btn"
                                                        on:click=move |_| {
                                                            weather.set(Some(w_click.clone()));
                                                            weather_draft.set(w_click.condition.clone());
                                                            mark_dirty();
                                                        }
                                                    >
                                                        <WeatherGlyph icon_key=w.icon_key.clone() />
                                                        <span>{lbl}</span>
                                                    </button>
                                                }
                                            })}
                                            {first_event.map(|ev| {
                                                let snippet = format!("### {}\n> {}\n\n", ev.title, ev.suggested_prompt);
                                                let label = format!("Reflect on \"{}\"", ev.title);
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="blank-prompt-btn"
                                                        on:click=move |_| {
                                                            body.update(|b| b.push_str(&snippet));
                                                            resize_textarea(&textarea_ref);
                                                            mark_dirty();
                                                        }
                                                    >
                                                        <span>{label}</span>
                                                    </button>
                                                }
                                            })}
                                            {first_prompt.map(|p| {
                                                let snippet = format!("> {p}\n\n");
                                                let label = p.clone();
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="blank-prompt-btn"
                                                        on:click=move |_| {
                                                            body.update(|b| b.push_str(&snippet));
                                                            resize_textarea(&textarea_ref);
                                                            mark_dirty();
                                                        }
                                                    >
                                                        <span>{label}</span>
                                                    </button>
                                                }
                                            })}
                                        </div>
                                        <button
                                            type="button"
                                            class="topbar-icon-btn"
                                            title="Dismiss prompt"
                                            on:click=move |_| prompt_dismissed.set(true)
                                        >
                                            <IconClose />
                                        </button>
                                    </div>
                                }
                            })
                        })
                    }}
                </Show>

                // Main Prose Region (auto-growing textarea or clean literary reading view)
                <section class="entry-prose-region">
                    <Show when=move || !stamps.get().is_empty() || mode.get() == EditorMode::Write>
                        <StampStickerLayer
                            stamps=stamps.into()
                            editable=true
                            on_change=Callback::new(move |next_stamps| {
                                stamps.set(next_stamps);
                                mark_dirty();
                            })
                        />
                    </Show>

                    <Show
                        when=move || mode.get() == EditorMode::Write
                        fallback=move || {
                            view! {
                                <div class="entry-reading-prose">
                                    {move || {
                                        let lines: Vec<(usize, String)> = body
                                            .get()
                                            .lines()
                                            .enumerate()
                                            .map(|(i, l)| (i, l.to_string()))
                                            .collect();
                                        lines
                                            .into_iter()
                                            .map(|(line_idx, raw)| {
                                                let trimmed = raw.trim();
                                                if trimmed.is_empty() {
                                                    return view! { <div class="entry-para-spacer"></div> }.into_any();
                                                }
                                                if trimmed == "---" || trimmed == "***" {
                                                    return view! { <hr class="entry-section-rule" /> }.into_any();
                                                }
                                                if let Some((alt, src)) = parse_standalone_image(trimmed) {
                                                    let alt_for_cap = alt.clone();
                                                    return view! {
                                                        <figure class="entry-inline-figure">
                                                            <img src=src alt=alt loading="lazy" />
                                                            {(!alt_for_cap.is_empty()).then(|| view! {
                                                                <figcaption>{alt_for_cap}</figcaption>
                                                            })}
                                                        </figure>
                                                    }.into_any();
                                                }
                                                if let Some(h1) = trimmed.strip_prefix("# ") {
                                                    return view! {
                                                        <h2 class="entry-h1">{render_inline_view(h1, all_entries, on_open_entry)}</h2>
                                                    }.into_any();
                                                }
                                                if let Some(h2) = trimmed.strip_prefix("## ") {
                                                    return view! {
                                                        <h3 class="entry-h2">{render_inline_view(h2, all_entries, on_open_entry)}</h3>
                                                    }.into_any();
                                                }
                                                if let Some(h3) = trimmed.strip_prefix("### ") {
                                                    return view! {
                                                        <h4 class="entry-h3">{render_inline_view(h3, all_entries, on_open_entry)}</h4>
                                                    }.into_any();
                                                }
                                                if let Some(note) = trimmed.strip_prefix("> [!NOTE]") {
                                                    let content = note.trim();
                                                    return view! {
                                                        <aside class="entry-callout-note">
                                                            <IconLeaf />
                                                            <span>{render_inline_view(content, all_entries, on_open_entry)}</span>
                                                        </aside>
                                                    }.into_any();
                                                }
                                                if let Some(quote) = trimmed.strip_prefix("> ") {
                                                    return view! {
                                                        <blockquote class="entry-pull-quote">
                                                            {render_inline_view(quote, all_entries, on_open_entry)}
                                                        </blockquote>
                                                    }.into_any();
                                                }
                                                if let Some(todo) = trimmed
                                                    .strip_prefix("- [ ] ")
                                                    .or_else(|| trimmed.strip_prefix("* [ ] "))
                                                {
                                                    return view! {
                                                        <label class="entry-checklist-row">
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
                                                if let Some(done) = trimmed
                                                    .strip_prefix("- [x] ")
                                                    .or_else(|| trimmed.strip_prefix("- [X] "))
                                                {
                                                    return view! {
                                                        <label class="entry-checklist-row is-done">
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

                                                view! {
                                                    <p class="entry-paragraph">{render_inline_view(trimmed, all_entries, on_open_entry)}</p>
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
                            class="entry-prose-textarea"
                            placeholder="Begin writing… Type '/' for blocks, photos, voice notes, or entry links."
                            prop:value=move || body.get()
                            on:input=on_body_input
                            on:keydown=on_body_keydown
                        />
                    </Show>
                </section>

                // Quiet Insert Bar (Write mode only)
                <Show when=move || mode.get() == EditorMode::Write>
                    <div class="entry-insert-bar">
                        <button
                            type="button"
                            class="insert-action-btn"
                            on:click=move |_| {
                                open_insert_at_cursor();
                                slash_query.set(String::new());
                                slash_index.set(0);
                                slash_open.update(|v| *v = !*v);
                            }
                        >
                            <IconPlus />
                            <span>"Slash menu (/)"</span>
                        </button>

                        <button
                            type="button"
                            class="insert-action-btn"
                            on:click=move |_| {
                                if let Some(input) = photo_input_ref.get() {
                                    input.click();
                                }
                            }
                        >
                            <IconCamera />
                            <span>"+ Photo"</span>
                        </button>

                        <button
                            type="button"
                            class="insert-action-btn"
                            on:click=move |_| show_voice_recorder.update(|v| *v = !*v)
                        >
                            <IconMic />
                            <span>"+ Voice note"</span>
                        </button>

                        <button
                            type="button"
                            class="insert-action-btn"
                            on:click=move |_| show_stamp_picker.update(|v| *v = !*v)
                        >
                            <IconStamp />
                            <span>"+ Stamp"</span>
                        </button>

                        <button
                            type="button"
                            class="insert-action-btn"
                            on:click=move |_| show_template_picker.update(|v| *v = !*v)
                        >
                            <span>"+ Template"</span>
                        </button>

                        <button
                            type="button"
                            class="insert-action-btn"
                            on:click=move |_| {
                                open_insert_at_cursor();
                                link_filter.set(String::new());
                                show_link_picker.update(|v| *v = !*v);
                            }
                        >
                            <IconLink />
                            <span>"+ Link entry"</span>
                        </button>
                    </div>
                </Show>

                // Anchored Slash Command Palette
                <Show when=move || slash_open.get()>
                    <div class="slash-palette-popover" role="listbox" aria-label="Insert block">
                        <div class="slash-palette-header">
                            <span class="small-caps-meta">"Insert"</span>
                            <span class="small-caps-meta">"↑↓ · Enter · Esc"</span>
                        </div>
                        <div class="slash-palette-list">
                            {move || {
                                let items = filtered_slash.get();
                                if items.is_empty() {
                                    view! {
                                        <div class="slash-empty">"No matching commands"</div>
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

                // Template Picker Popover
                <Show when=move || show_template_picker.get()>
                    <div class="inline-picker-card">
                        <div class="inline-picker-header">
                            <span class="small-caps-meta">"Templates"</span>
                            <button
                                type="button"
                                class="topbar-icon-btn"
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
                                                    resize_textarea(&textarea_ref);
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

                // Wikilink Entry Picker Popover
                <Show when=move || show_link_picker.get()>
                    <div class="inline-picker-card">
                        <div class="inline-picker-header">
                            <span class="small-caps-meta">"Link Entry"</span>
                            <button
                                type="button"
                                class="topbar-icon-btn"
                                on:click=move |_| show_link_picker.set(false)
                            >
                                <IconClose />
                            </button>
                        </div>
                        <input
                            type="text"
                            class="editorial-input"
                            placeholder="Filter entry titles…"
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
                                                    insert_snippet_at(
                                                        s.min(current_len),
                                                        end.min(current_len),
                                                        &insertion,
                                                    );
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

                // Stamp Picker Popover
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

                // SCROLL-DOWN MEDIA & ENTRY DETAILS ZONE
                <div class="entry-below-fold">
                    // 1. Photographs (ONLY rendered when photos exist)
                    <Show when=move || !photos.get().is_empty()>
                        <section class="entry-detail-section entry-photos-section">
                            <div class="detail-section-header">
                                <span class="small-caps-meta">
                                    {move || format!("Photographs ({})", photos.get().len())}
                                </span>
                                <button
                                    type="button"
                                    class="insert-action-btn"
                                    on:click=move |_| {
                                        if let Some(input) = photo_input_ref.get() {
                                            input.click();
                                        }
                                    }
                                >
                                    <IconPlus />
                                    <span>"Add photo"</span>
                                </button>
                            </div>

                            <div
                                class="entry-photos-grid"
                                class:is-single=move || photos.get().len() == 1
                            >
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
                                                                p.caption = if new_cap.trim().is_empty() {
                                                                    None
                                                                } else {
                                                                    Some(new_cap)
                                                                };
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
                        </section>
                    </Show>

                    // 2. Voice Notes (ONLY rendered when audio exists or recorder open)
                    <Show when=move || !audio.get().is_empty() || show_voice_recorder.get()>
                        <section class="entry-detail-section entry-audio-section">
                            <div class="detail-section-header">
                                <span class="small-caps-meta">
                                    {move || format!("Voice Notes ({})", audio.get().len())}
                                </span>
                                <button
                                    type="button"
                                    class="insert-action-btn"
                                    on:click=move |_| show_voice_recorder.update(|v| *v = !*v)
                                >
                                    <IconMic />
                                    <span>{move || if show_voice_recorder.get() { "Hide recorder" } else { "Record note" }}</span>
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
                    </Show>

                    // 3. Entry Details & Archive Ledger (ALWAYS rendered at bottom of scroll)
                    <section class="entry-detail-section entry-ledger-section">
                        <div class="detail-section-header">
                            <span class="small-caps-meta">"Entry Details"</span>
                        </div>

                        <dl class="entry-ledger-grid">
                            <div class="ledger-cell">
                                <dt class="ledger-term">"Date & Time"</dt>
                                <dd class="ledger-val">
                                    {move || {
                                        loaded_date
                                            .get()
                                            .format("%A, %B %d, %Y · %H:%M UTC")
                                            .to_string()
                                    }}
                                </dd>
                            </div>

                            <div class="ledger-cell">
                                <dt class="ledger-term">"Reading & Length"</dt>
                                <dd class="ledger-val">
                                    {move || {
                                        format!(
                                            "{} words · {} characters · {} min read",
                                            word_count.get(),
                                            char_count.get(),
                                            reading_mins.get()
                                        )
                                    }}
                                </dd>
                            </div>

                            <div class="ledger-cell">
                                <dt class="ledger-term">"Notebook"</dt>
                                <dd class="ledger-val">{move || resolved_notebook_name.get()}</dd>
                            </div>

                            <div class="ledger-cell">
                                <dt class="ledger-term">"Location & Coordinates"</dt>
                                <dd class="ledger-val">
                                    {move || match location.get() {
                                        Some(loc) => {
                                            let coords = match (loc.latitude, loc.longitude) {
                                                (Some(lat), Some(lng)) => format!(" ({:.4}, {:.4})", lat, lng),
                                                _ => String::new(),
                                            };
                                            format!("{}{}", loc.place_name, coords)
                                        }
                                        None => "—".to_string(),
                                    }}
                                </dd>
                            </div>

                            <div class="ledger-cell">
                                <dt class="ledger-term">"Atmosphere"</dt>
                                <dd class="ledger-val">
                                    {move || {
                                        let w_str = weather.get().map(|w| {
                                            let t = w.temperature_c.map(|c| format!(" {:.0}°C", c)).unwrap_or_default();
                                            format!("{}{}", w.condition, t)
                                        });
                                        let m_str = mood.get();
                                        match (w_str, m_str) {
                                            (Some(w), Some(m)) => format!("{w} · {m}"),
                                            (Some(w), None) => w,
                                            (None, Some(m)) => m,
                                            (None, None) => "—".to_string(),
                                        }
                                    }}
                                </dd>
                            </div>

                            <div class="ledger-cell">
                                <dt class="ledger-term">"Provenance & File"</dt>
                                <dd class="ledger-val">
                                    {move || {
                                        let prov = format_provenance(loaded_source.get().as_deref());
                                        let media_summary = format!(
                                            "{} photos, {} voice notes",
                                            photos.get().len(),
                                            audio.get().len()
                                        );
                                        let rel = loaded_rel_path.get();
                                        if rel.is_empty() {
                                            format!("{prov} · {media_summary}")
                                        } else {
                                            format!("{prov} · {media_summary} · {rel}")
                                        }
                                    }}
                                </dd>
                            </div>
                        </dl>
                    </section>

                    // 4. Connections / Linked Entries (rendered when outgoing or incoming links exist)
                    {move || {
                        let outgoing = extract_wikilinks(&body.get());
                        let incoming = backlinks.get();
                        (!outgoing.is_empty() || !incoming.is_empty()).then(|| {
                            view! {
                                <section class="entry-detail-section entry-links-section">
                                    <div class="detail-section-header">
                                        <span class="small-caps-meta">"Connections"</span>
                                    </div>

                                    {(!outgoing.is_empty()).then(|| {
                                        view! {
                                            <div class="connections-group">
                                                <span class="connections-sublabel">"Links in this entry"</span>
                                                <div class="connections-list">
                                                    {outgoing.into_iter().map(|lnk| {
                                                        let needle = lnk.to_lowercase();
                                                        view! {
                                                            <button
                                                                type="button"
                                                                class="entry-wikilink"
                                                                on:click=move |_| {
                                                                    let list = all_entries.get_untracked();
                                                                    if let Some(found) = list
                                                                        .iter()
                                                                        .find(|e| e.title.trim().to_lowercase() == needle)
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
                                            </div>
                                        }
                                    })}

                                    {(!incoming.is_empty()).then(|| {
                                        view! {
                                            <div class="connections-group">
                                                <span class="connections-sublabel">"Referenced in"</span>
                                                <div class="backlinks-list">
                                                    {incoming.into_iter().map(|bl| {
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
                                            </div>
                                        }
                                    })}
                                </section>
                            }
                        })
                    }}
                </div>
            </div>
        </article>
    }
}
