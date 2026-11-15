//! Day Cream Paper / Candlelit Night Slate theme controller.

use leptos::prelude::*;

use crate::icons::{IconLamp, IconSun};

const THEME_STORAGE_KEY: &str = "log_journal_theme";

/// Active visual theme for the book spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Day,
    Night,
}

impl ThemeMode {
    /// Convert a boolean night flag into [`ThemeMode`].
    pub fn from_night(is_night: bool) -> Self {
        if is_night {
            Self::Night
        } else {
            Self::Day
        }
    }

    /// Return `true` when in Candlelit Night mode.
    pub fn is_night(self) -> bool {
        matches!(self, Self::Night)
    }

    /// HTML attribute value for `data-theme`.
    pub fn as_attr(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Night => "night",
        }
    }
}

/// Read persisted theme preference from `window.localStorage`, defaulting to Day Cream Paper.
pub fn load_initial_night_mode() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return false;
    };
    matches!(
        storage.get_item(THEME_STORAGE_KEY).ok().flatten().as_deref(),
        Some("night")
    )
}

/// Apply the theme cleanly to `<html data-theme="...">`, `body.night`, and `localStorage`.
pub fn apply_theme(night_mode: bool) {
    let mode = ThemeMode::from_night(night_mode);
    let Some(window) = web_sys::window() else {
        return;
    };

    if let Some(doc) = window.document() {
        if let Some(root) = doc.document_element() {
            let _ = root.set_attribute("data-theme", mode.as_attr());
        }
        if let Some(body) = doc.body() {
            let classes = body.class_list();
            if night_mode {
                let _ = classes.add_1("night");
            } else {
                let _ = classes.remove_1("night");
            }
        }
    }

    if let Ok(Some(storage)) = window.local_storage() {
        let _ = storage.set_item(THEME_STORAGE_KEY, mode.as_attr());
    }
}

/// Fine-line Daylight Paper / Candlelit Slate lamp toggle button.
#[component]
pub fn ThemeToggleButton(night_mode: RwSignal<bool>) -> impl IntoView {
    Effect::new(move |_| {
        apply_theme(night_mode.get());
    });

    let toggle = move |_| {
        night_mode.update(|v| *v = !*v);
    };

    view! {
        <button
            type="button"
            class="theme-toggle-btn"
            aria-label=move || {
                if night_mode.get() {
                    "Switch to Daylight Cream Paper"
                } else {
                    "Switch to Candlelit Night Slate"
                }
            }
            title=move || {
                if night_mode.get() {
                    "Daylight Paper"
                } else {
                    "Candlelit Desk Lamp"
                }
            }
            on:click=toggle
        >
            {move || {
                if night_mode.get() {
                    view! {
                        <span class="theme-toggle-inner">
                            <IconSun class="theme-toggle-icon" />
                            <span>"Daylight"</span>
                        </span>
                    }
                        .into_any()
                } else {
                    view! {
                        <span class="theme-toggle-inner">
                            <IconLamp class="theme-toggle-icon" />
                            <span>"Candlelit"</span>
                        </span>
                    }
                        .into_any()
                }
            }}
        </button>
    }
}
