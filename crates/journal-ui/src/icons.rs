//! Fine 1.4px stroke fountain-pen SVG icons (zero emojis anywhere in UI chrome).

use leptos::prelude::*;

fn icon_class(extra: &str) -> String {
    if extra.is_empty() {
        "log-icon".to_string()
    } else {
        format!("log-icon {extra}")
    }
}

/// Fountain-pen feather nib icon.
#[component]
pub fn IconFeather(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M20.24 12.24a6 6 0 0 0-8.49-8.49L5 10.5V19h8.5z" />
            <line x1="16" y1="8" x2="2" y2="22" />
            <line x1="17.5" y1="15" x2="9" y2="15" />
        </svg>
    }
}

/// Backward-compatible alias for [`IconFeather`].
#[component]
pub fn FeatherIcon(#[prop(optional, into)] class: String) -> impl IntoView {
    view! { <IconFeather class=class /> }
}

/// Bound cloth notebook icon.
#[component]
pub fn IconNotebook(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20" />
            <line x1="8" y1="2" x2="8" y2="17" />
            <line x1="12" y1="7" x2="16" y2="7" />
            <line x1="12" y1="11" x2="15" y2="11" />
        </svg>
    }
}

/// Open two-page book spread icon.
#[component]
pub fn IconBookOpen(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M2 4.5A2.5 2.5 0 0 1 4.5 2H12v18.5a2.5 2.5 0 0 0-2.5-2H2z" />
            <path d="M22 4.5A2.5 2.5 0 0 0 19.5 2H12v18.5a2.5 2.5 0 0 1 2.5-2H22z" />
        </svg>
    }
}

/// Warm desk lamp / candlelit night mode icon.
#[component]
pub fn IconLamp(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M8 2h8l3 8H5l3-8z" />
            <path d="M12 10v10" />
            <path d="M8 20h8" />
            <path d="M9 10v1.5a3 3 0 0 0 6 0V10" />
        </svg>
    }
}

/// Daylight sun icon.
#[component]
pub fn IconSun(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="4" />
            <line x1="12" y1="2" x2="12" y2="4.5" />
            <line x1="12" y1="19.5" x2="12" y2="22" />
            <line x1="4.93" y1="4.93" x2="6.7" y2="6.7" />
            <line x1="17.3" y1="17.3" x2="19.07" y2="19.07" />
            <line x1="2" y1="12" x2="4.5" y2="12" />
            <line x1="19.5" y1="12" x2="22" y2="12" />
            <line x1="4.93" y1="19.07" x2="6.7" y2="17.3" />
            <line x1="17.3" y1="6.7" x2="19.07" y2="4.93" />
        </svg>
    }
}

/// Calendar grid icon.
#[component]
pub fn IconCalendar(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="3" y="4" width="18" height="18" rx="2" />
            <line x1="16" y1="2" x2="16" y2="6" />
            <line x1="8" y1="2" x2="8" y2="6" />
            <line x1="3" y1="10" x2="21" y2="10" />
            <circle cx="8.5" cy="14.5" r="0.8" fill="currentColor" />
            <circle cx="12" cy="14.5" r="0.8" fill="currentColor" />
            <circle cx="15.5" cy="14.5" r="0.8" fill="currentColor" />
        </svg>
    }
}

/// Archival clock / "On This Day" history icon.
#[component]
pub fn IconClockHistory(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
            <path d="M3 3v5h5" />
            <polyline points="12 7 12 12 15.5 14" />
        </svg>
    }
}

/// Magnifying glass search icon.
#[component]
pub fn IconSearch(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="11" cy="11" r="7" />
            <line x1="20" y1="20" x2="16" y2="16" />
        </svg>
    }
}

/// Specimen / index tag icon.
#[component]
pub fn IconTag(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z" />
            <circle cx="7" cy="7" r="1.2" fill="currentColor" />
        </svg>
    }
}

/// Studio condenser microphone icon for voice memos.
#[component]
pub fn IconMic(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="9" y="2" width="6" height="11" rx="3" />
            <path d="M5 10a7 7 0 0 0 14 0" />
            <line x1="12" y1="17" x2="12" y2="21" />
            <line x1="8.5" y1="21" x2="15.5" y2="21" />
        </svg>
    }
}

/// Engraved play triangle icon.
#[component]
pub fn IconPlay(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polygon points="6 4 19 12 6 20 6 4" />
        </svg>
    }
}

/// Engraved pause bars icon.
#[component]
pub fn IconPause(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="8" y1="5" x2="8" y2="19" />
            <line x1="16" y1="5" x2="16" y2="19" />
        </svg>
    }
}

/// Analog camera icon for Polaroid attachments.
#[component]
pub fn IconCamera(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z" />
            <circle cx="12" cy="13" r="4" />
        </svg>
    }
}

/// Wax / wooden hand-stamp icon.
#[component]
pub fn IconStamp(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M5 17h14v3H5z" />
            <path d="M7 17v-2a3 3 0 0 1 3-3h4a3 3 0 0 1 3 3v2" />
            <path d="M10.5 12L9.5 6.5a2.5 2.5 0 0 1 5 0L13.5 12" />
            <line x1="4" y1="22" x2="20" y2="22" />
        </svg>
    }
}

/// Botanical four-point compass spark icon (replaces AI/sparkle emoji).
#[component]
pub fn IconSpark(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M12 2l1.9 6.6L20.5 10.5l-6.6 1.9L12 19l-1.9-6.6L3.5 10.5l6.6-1.9L12 2z" />
            <circle cx="18.5" cy="5.5" r="0.8" fill="currentColor" />
        </svg>
    }
}

/// Encrypted cloud vault padlock icon.
#[component]
pub fn IconCloudLock(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M18 10h-1.26A6 6 0 1 0 6 14h1" />
            <rect x="9" y="13" width="9" height="7" rx="1.5" />
            <path d="M11 13v-2a2.5 2.5 0 0 1 5 0v2" />
        </svg>
    }
}

/// Archival tray import icon.
#[component]
pub fn IconArchiveImport(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="21 8 21 21 3 21 3 8" />
            <rect x="1" y="3" width="22" height="5" rx="1" />
            <line x1="12" y1="11" x2="12" y2="17" />
            <polyline points="9 14 12 17 15 14" />
        </svg>
    }
}

/// Left chevron icon for page turns and month navigation.
#[component]
pub fn IconChevronLeft(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="15 18 9 12 15 6" />
        </svg>
    }
}

/// Right chevron icon for page turns and month navigation.
#[component]
pub fn IconChevronRight(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="9 18 15 12 9 6" />
        </svg>
    }
}

/// Starred / favorite folio icon.
#[component]
pub fn IconStar(
    #[prop(optional, into)] class: String,
    #[prop(optional)] filled: bool,
) -> impl IntoView {
    let cls = icon_class(&class);
    let fill_attr = if filled { "currentColor" } else { "none" };
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill=fill_attr stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polygon points="12 2.5 14.9 8.4 21.4 9.3 16.7 13.9 17.8 20.4 12 17.3 6.2 20.4 7.3 13.9 2.6 9.3 9.1 8.4 12 2.5" />
        </svg>
    }
}

/// Plus icon.
#[component]
pub fn IconPlus(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
        </svg>
    }
}

/// Close / dismiss cross icon.
#[component]
pub fn IconClose(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
    }
}

/// Checkmark icon.
#[component]
pub fn IconCheck(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="20 6 9 17 4 12" />
        </svg>
    }
}

/// Geographic place pin icon.
#[component]
pub fn IconMapPin(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M21 10c0 6.5-9 12.5-9 12.5S3 16.5 3 10a9 9 0 1 1 18 0z" />
            <circle cx="12" cy="10" r="3" />
        </svg>
    }
}

/// Rain cloud atmospheric weather icon.
#[component]
pub fn IconRain(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M18 14.5a4.5 4.5 0 0 0-.6-8.96A6 6 0 0 0 5.6 7.5 3.8 3.8 0 0 0 6.5 15h11.5" />
            <line x1="8" y1="18" x2="7" y2="21" />
            <line x1="12" y1="18" x2="11" y2="21" />
            <line x1="16" y1="18" x2="15" y2="21" />
        </svg>
    }
}

/// Overcast cloud atmospheric weather icon.
#[component]
pub fn IconCloud(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M17.5 18H7a4.5 4.5 0 0 1-.5-8.97A6 6 0 0 1 18.2 10a4 4 0 0 1-.7 8z" />
        </svg>
    }
}

/// Wikilink / chain link icon.
#[component]
pub fn IconLink(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
            <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
        </svg>
    }
}

/// Literary reading eye icon.
#[component]
pub fn IconEye(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M2 12s3.5-6.5 10-6.5S22 12 22 12s-3.5 6.5-10 6.5S2 12 2 12z" />
            <circle cx="12" cy="12" r="2.8" />
        </svg>
    }
}

/// Botanical leaf emblem for herbarium accents.
#[component]
pub fn IconLeaf(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19 2c1 2 2 4.18 2 8 0 5.5-4.78 10-10 10z" />
            <path d="M2 21c0-3 1.85-5.36 5.08-6C9.5 14.52 12 13 13 12" />
        </svg>
    }
}

/// Editorial trash / delete icon.
#[component]
pub fn IconTrash(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="3 6 5 6 21 6" />
            <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
            <path d="M10 11v6" />
            <path d="M14 11v6" />
            <path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
        </svg>
    }
}

/// Upload arrow icon.
#[component]
pub fn IconUpload(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="17 8 12 3 7 8" />
            <line x1="12" y1="3" x2="12" y2="15" />
        </svg>
    }
}

/// Refresh / sync arrows icon.
#[component]
pub fn IconRefresh(#[prop(optional, into)] class: String) -> impl IntoView {
    let cls = icon_class(&class);
    view! {
        <svg class=cls viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="23 4 23 10 17 10" />
            <polyline points="1 20 1 14 7 14" />
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
        </svg>
    }
}

/// Render the matching fine-line weather SVG from a `WeatherInfo::icon_key`.
#[component]
pub fn WeatherGlyph(
    #[prop(into)] icon_key: String,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let key = icon_key.to_lowercase();
    if key.contains("rain") || key.contains("drizzle") || key.contains("shower") {
        view! { <IconRain class=class /> }.into_any()
    } else if key.contains("sun") || key.contains("clear") {
        view! { <IconSun class=class /> }.into_any()
    } else {
        view! { <IconCloud class=class /> }.into_any()
    }
}
