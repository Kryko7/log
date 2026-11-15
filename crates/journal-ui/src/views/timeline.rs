//! Clean editorial river Timeline view grouped chronologically by month.

use std::collections::BTreeMap;

use chrono::Datelike;
use journal_types::{EntrySummary, Notebook};
use leptos::prelude::*;
use uuid::Uuid;

use crate::icons::{
    IconFeather, IconMapPin, IconMic, IconStamp, IconStar, WeatherGlyph,
};
use crate::views::{clean_excerpt_snippet, format_month_year, is_auto_derived_title};

/// Chronological editorial river timeline grouped by month.
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
                let label = format_month_year(y, m);
                (label, list)
            })
            .collect::<Vec<_>>()
    });

    view! {
        <section class="timeline-river" aria-label="All entries">
            <header class="stage-header">
                <div>
                    <h1 class="stage-serif-title">"All Entries"</h1>
                    <p class="stage-subtitle">
                        {move || {
                            let n = entries.get().len();
                            format!("{n} {}", if n == 1 { "entry" } else { "entries" })
                        }}
                    </p>
                </div>
                {on_new_entry.map(|cb| {
                    view! {
                        <button
                            type="button"
                            class="btn-editorial btn-sage"
                            on:click=move |_| cb.run(())
                        >
                            <IconFeather />
                            <span>"New Entry"</span>
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
                            <h2 class="empty-serif-heading">"No entries yet"</h2>
                            <p class="empty-prose-note">
                                "Start writing a new entry or clear the active filter."
                            </p>
                            {on_new_entry.map(|cb| {
                                view! {
                                    <button
                                        type="button"
                                        class="btn-editorial btn-sage"
                                        on:click=move |_| cb.run(())
                                    >
                                        <IconFeather />
                                        <span>"New Entry"</span>
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
                                            <span class="timeline-month-heading">{month_label}</span>
                                            <span class="timeline-month-count small-caps-meta">
                                                {format!("{count} {}", if count == 1 { "entry" } else { "entries" })}
                                            </span>
                                        </div>

                                        <div class="timeline-rows">
                                            {list
                                                .into_iter()
                                                .map(|entry| {
                                                    let nb_name = entry
                                                        .notebook_id
                                                        .as_deref()
                                                        .and_then(|id| nb_list.iter().find(|n| n.id == id))
                                                        .map(|n| n.name.clone());

                                                    let entry_id = entry.id;
                                                    let day_num = entry.date.format("%d").to_string();
                                                    let mon_day = entry.date.format("%b · %a").to_string().to_uppercase();
                                                    let time_hm = entry.date.format("%H:%M").to_string();

                                                    let snippet_str = clean_excerpt_snippet(&entry.snippet);
                                                    let show_title = !is_auto_derived_title(&entry.title, &snippet_str);
                                                    let title_str = entry.title.clone();
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
                                                            class="timeline-entry-row"
                                                            on:click=move |_| on_open_entry.run(entry_id)
                                                        >
                                                            <div class="timeline-date-col">
                                                                <span class="timeline-day-num">{day_num}</span>
                                                                <span class="timeline-day-sub small-caps-meta">{mon_day}</span>
                                                                <span class="timeline-day-time">{time_hm}</span>
                                                            </div>

                                                            <div class="timeline-prose-col">
                                                                {show_title.then(|| {
                                                                    view! { <h3 class="timeline-row-title">{title_str}</h3> }
                                                                })}

                                                                {(!snippet_str.is_empty()).then(|| {
                                                                    view! { <p class="timeline-row-excerpt">{snippet_str}</p> }
                                                                })}

                                                                <div class="timeline-row-meta">
                                                                    {nb_name.map(|name| {
                                                                        view! { <span class="meta-text-item">{name}</span> }
                                                                    })}
                                                                    {location.map(|loc| {
                                                                        view! {
                                                                            <span class="meta-text-item">
                                                                                <IconMapPin class="tiny-icon" />
                                                                                <span>{loc.place_name}</span>
                                                                            </span>
                                                                        }
                                                                    })}
                                                                    {weather.map(|w| {
                                                                        let temp = w.temperature_c.map(|t| format!(" {:.0}°C", t)).unwrap_or_default();
                                                                        view! {
                                                                            <span class="meta-text-item">
                                                                                <WeatherGlyph icon_key=w.icon_key class="tiny-icon" />
                                                                                <span>{format!("{}{}", w.condition, temp)}</span>
                                                                            </span>
                                                                        }
                                                                    })}
                                                                    {tags
                                                                        .into_iter()
                                                                        .take(4)
                                                                        .map(|tag| {
                                                                            let tag_click = tag.clone();
                                                                            view! {
                                                                                <button
                                                                                    type="button"
                                                                                    class="inline-tag-link"
                                                                                    on:click=move |ev| {
                                                                                        ev.stop_propagation();
                                                                                        if let Some(cb) = on_select_tag {
                                                                                            cb.run(tag_click.clone());
                                                                                        }
                                                                                    }
                                                                                >
                                                                                    {format!("#{tag}")}
                                                                                </button>
                                                                            }
                                                                        })
                                                                        .collect_view()}
                                                                    {(read_mins > 0).then(|| {
                                                                        view! { <span class="meta-text-item">{format!("{read_mins} min read")}</span> }
                                                                    })}
                                                                    {has_audio.then(|| {
                                                                        view! {
                                                                            <span class="meta-text-item" title="Voice note">
                                                                                <IconMic class="tiny-icon" />
                                                                            </span>
                                                                        }
                                                                    })}
                                                                    {(stamp_count > 0).then(|| {
                                                                        view! {
                                                                            <span class="meta-text-item" title="Stamps">
                                                                                <IconStamp class="tiny-icon" />
                                                                                <span>{stamp_count}</span>
                                                                            </span>
                                                                        }
                                                                    })}
                                                                    {starred.then(|| {
                                                                        view! {
                                                                            <span class="meta-star-mark" title="Starred">
                                                                                <IconStar filled=true class="tiny-icon" />
                                                                            </span>
                                                                        }
                                                                    })}
                                                                </div>
                                                            </div>

                                                            {cover_photo.map(|thumb| {
                                                                view! {
                                                                    <div class="timeline-photo-thumb">
                                                                        <img src=thumb alt="" loading="lazy" />
                                                                    </div>
                                                                }
                                                            })}
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
