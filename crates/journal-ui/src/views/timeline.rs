//! Editorial Chronicle / Timeline view grouped chronologically by month.

use std::collections::BTreeMap;

use chrono::Datelike;
use journal_types::{EntrySummary, Notebook};
use leptos::prelude::*;
use uuid::Uuid;

use crate::icons::{
    IconFeather, IconMapPin, IconMic, IconStamp, IconStar, IconTag, WeatherGlyph,
};
use crate::views::format_entry_date;

const DEFAULT_COVER: &str = "/assets/covers/botanical-fern-wash.png";

/// Chronological folio timeline grouped by month with watercolor notebook headers,
/// Polaroid previews, small-caps metadata, and tag ribbons.
#[component]
pub fn TimelineView(
    entries: Signal<Vec<EntrySummary>>,
    notebooks: Signal<Vec<Notebook>>,
    on_open_entry: Callback<Uuid>,
    #[prop(optional)] on_select_tag: Option<Callback<String>>,
    #[prop(optional)] on_new_entry: Option<Callback<()>>,
) -> impl IntoView {
    let grouped = Memo::new(move |_| {
        let mut items = entries.get();
        items.sort_by(|a, b| b.date.cmp(&a.date));

        let mut map: BTreeMap<(i32, u32), Vec<EntrySummary>> = BTreeMap::new();
        for entry in items {
            let key = (-entry.date.year(), 13 - entry.date.month());
            map.entry(key).or_default().push(entry);
        }

        map.into_iter()
            .map(|((neg_year, inv_month), list)| {
                let y = -neg_year;
                let m = 13 - inv_month;
                let label = crate::views::format_month_year(y, m);
                (label, list)
            })
            .collect::<Vec<_>>()
    });

    view! {
        <section class="folio-stage timeline-stage" aria-label="Chronicle timeline">
            <header class="stage-header">
                <div>
                    <span class="small-caps-meta">"Chronicle"</span>
                    <h1 class="stage-serif-title">"Bound Folios & Margin Notes"</h1>
                </div>
                {on_new_entry.map(|cb| {
                    view! {
                        <button
                            type="button"
                            class="btn-editorial btn-sage"
                            on:click=move |_| cb.run(())
                        >
                            <IconFeather />
                            <span>"Begin New Folio"</span>
                        </button>
                    }
                })}
            </header>

            {move || {
                let groups = grouped.get();
                let nb_list = notebooks.get();

                if groups.is_empty() {
                    return view! {
                        <div class="editorial-empty-card">
                            <IconFeather class="empty-emblem" />
                            <h2 class="empty-serif-heading">"An Unwritten Volume"</h2>
                            <p class="empty-prose-note">
                                "No folios match this shelf yet. Open a fresh sheet to begin today's record."
                            </p>
                            {on_new_entry.map(|cb| {
                                view! {
                                    <button
                                        type="button"
                                        class="btn-editorial btn-sage"
                                        on:click=move |_| cb.run(())
                                    >
                                        <IconFeather />
                                        <span>"Write First Folio"</span>
                                    </button>
                                }
                            })}
                        </div>
                    }
                        .into_any();
                }

                view! {
                    <div class="timeline-months-list">
                        {groups
                            .into_iter()
                            .map(|(month_label, list)| {
                                let count = list.len();
                                view! {
                                    <section class="timeline-month-group">
                                        <div class="timeline-month-divider">
                                            <span class="small-caps-meta">{month_label}</span>
                                            <span class="timeline-month-rule"></span>
                                            <span class="small-caps-meta">{format!("{count} {}", if count == 1 { "folio" } else { "folios" })}</span>
                                        </div>

                                        <div class="timeline-cards-grid">
                                            {list
                                                .into_iter()
                                                .map(|entry| {
                                                    let nb = entry
                                                        .notebook_id
                                                        .as_deref()
                                                        .and_then(|id| nb_list.iter().find(|n| n.id == id));
                                                    let cover_src = nb
                                                        .map(|n| n.cover_asset.clone())
                                                        .unwrap_or_else(|| DEFAULT_COVER.to_string());
                                                    let nb_name = nb.map(|n| n.name.clone());
                                                    let accent = nb
                                                        .map(|n| n.accent_color.clone())
                                                        .unwrap_or_else(|| "#546E5A".to_string());

                                                    let entry_id = entry.id;
                                                    let date_str = format_entry_date(&entry.date);
                                                    let title_str = if entry.title.trim().is_empty() {
                                                        "Untitled Folio".to_string()
                                                    } else {
                                                        entry.title.clone()
                                                    };
                                                    let snippet_str = entry.snippet.clone();
                                                    let weather = entry.weather.clone();
                                                    let location = entry.location.clone();
                                                    let cover_photo = entry.cover_photo.clone();
                                                    let tags = entry.tags.clone();
                                                    let has_audio = entry.has_audio;
                                                    let stamp_count = entry.stamps.len();
                                                    let starred = entry.starred;
                                                    let read_mins = entry.reading_minutes;

                                                    view! {
                                                        <article
                                                            class="timeline-folio-card"
                                                            style=format!("--card-spine-accent: {accent};")
                                                            on:click=move |_| on_open_entry.run(entry_id)
                                                        >
                                                            <div class="timeline-card-cover">
                                                                <img src=cover_src alt="" loading="lazy" />
                                                                <div class="timeline-card-cover-scrim"></div>
                                                                {nb_name.map(|name| {
                                                                    view! { <span class="timeline-nb-pill small-caps-meta">{name}</span> }
                                                                })}
                                                                {starred.then(|| {
                                                                    view! {
                                                                        <span class="timeline-star-badge" title="Starred folio">
                                                                            <IconStar filled=true />
                                                                        </span>
                                                                    }
                                                                })}
                                                            </div>

                                                            <div class="timeline-card-body">
                                                                <div class="timeline-card-meta-row">
                                                                    <span class="small-caps-meta">{date_str}</span>
                                                                    {location.map(|loc| {
                                                                        view! {
                                                                            <span class="meta-chip">
                                                                                <IconMapPin class="tiny-icon" />
                                                                                <span>{loc.place_name}</span>
                                                                            </span>
                                                                        }
                                                                    })}
                                                                    {weather.map(|w| {
                                                                        let temp = w.temperature_c.map(|t| format!(" {:.0}°C", t)).unwrap_or_default();
                                                                        view! {
                                                                            <span class="meta-chip">
                                                                                <WeatherGlyph icon_key=w.icon_key class="tiny-icon" />
                                                                                <span>{format!("{}{}", w.condition, temp)}</span>
                                                                            </span>
                                                                        }
                                                                    })}
                                                                </div>

                                                                <div class="timeline-card-content-row">
                                                                    <div class="timeline-card-text-col">
                                                                        <h3 class="timeline-card-title">{title_str}</h3>
                                                                        <p class="timeline-card-excerpt">{snippet_str}</p>
                                                                    </div>

                                                                    {cover_photo.map(|thumb| {
                                                                        view! {
                                                                            <div class="timeline-polaroid-mini">
                                                                                <img src=thumb alt="Entry Polaroid preview" loading="lazy" />
                                                                            </div>
                                                                        }
                                                                    })}
                                                                </div>

                                                                <footer class="timeline-card-footer">
                                                                    <div class="timeline-tags-ribbon">
                                                                        {tags
                                                                            .into_iter()
                                                                            .take(4)
                                                                            .map(|tag| {
                                                                                let tag_click = tag.clone();
                                                                                view! {
                                                                                    <button
                                                                                        type="button"
                                                                                        class="folio-tag-chip"
                                                                                        on:click=move |ev| {
                                                                                            ev.stop_propagation();
                                                                                            if let Some(cb) = on_select_tag {
                                                                                                cb.run(tag_click.clone());
                                                                                            }
                                                                                        }
                                                                                    >
                                                                                        <IconTag class="tiny-icon" />
                                                                                        <span>{tag}</span>
                                                                                    </button>
                                                                                }
                                                                            })
                                                                            .collect_view()}
                                                                    </div>

                                                                    <div class="timeline-artifacts-meta">
                                                                        {has_audio.then(|| {
                                                                            view! {
                                                                                <span class="artifact-indicator" title="Contains voice memo">
                                                                                    <IconMic class="tiny-icon" />
                                                                                </span>
                                                                            }
                                                                        })}
                                                                        {(stamp_count > 0).then(|| {
                                                                            view! {
                                                                                <span class="artifact-indicator" title="Contains botanical stamps">
                                                                                    <IconStamp class="tiny-icon" />
                                                                                    <span>{stamp_count}</span>
                                                                                </span>
                                                                            }
                                                                        })}
                                                                        {(read_mins > 0).then(|| {
                                                                            view! {
                                                                                <span class="small-caps-meta">{format!("{read_mins}m")}</span>
                                                                            }
                                                                        })}
                                                                    </div>
                                                                </footer>
                                                            </div>
                                                        </article>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    </section>
                                }
                            })
                            .collect_view()}
                    </div>
                }
                    .into_any()
            }}
        </section>
    }
}
