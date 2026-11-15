//! "On This Day" archival memories view across prior years.

use journal_types::OnThisDayGroup;
use leptos::prelude::*;
use uuid::Uuid;

use crate::icons::{
    IconClockHistory, IconMapPin, IconMic, IconStar, WeatherGlyph,
};
use crate::views::{clean_excerpt_snippet, is_auto_derived_title};

/// Historical memory list displaying past entries from the same calendar month and day.
#[component]
pub fn OnThisDayView(
    groups: Signal<Vec<OnThisDayGroup>>,
    on_open_entry: Callback<Uuid>,
) -> impl IntoView {
    let today_label = chrono::Utc::now().format("%B %d").to_string();

    view! {
        <section class="timeline-river" aria-label="On This Day">
            <header class="stage-header">
                <div>
                    <h1 class="stage-serif-title">"On This Day"</h1>
                    <p class="stage-subtitle">{today_label}</p>
                </div>
            </header>

            {move || {
                let list = groups.get();
                if list.is_empty() {
                    return view! {
                        <div class="editorial-empty-card">
                            <IconClockHistory class="empty-emblem" />
                            <h2 class="empty-serif-heading">"No past entries on this date"</h2>
                            <p class="empty-prose-note">
                                "Entries written on this calendar day in previous years will appear here."
                            </p>
                        </div>
                    }
                        .into_any();
                }

                view! {
                    <div class="timeline-months-list">
                        {list
                            .into_iter()
                            .map(|group| {
                                let years_label = if group.years_ago == 1 {
                                    format!("1 year ago · {}", group.year)
                                } else {
                                    format!("{} years ago · {}", group.years_ago, group.year)
                                };
                                let count = group.entries.len();

                                view! {
                                    <section class="timeline-month-group">
                                        <div class="timeline-month-divider">
                                            <span class="timeline-month-heading">{years_label}</span>
                                            <span class="timeline-month-count small-caps-meta">
                                                {format!("{count} {}", if count == 1 { "entry" } else { "entries" })}
                                            </span>
                                        </div>

                                        <div class="timeline-rows">
                                            {group
                                                .entries
                                                .into_iter()
                                                .map(|entry| {
                                                    let entry_id = entry.id;
                                                    let snippet = clean_excerpt_snippet(&entry.snippet);
                                                    let show_title = !is_auto_derived_title(&entry.title, &snippet);
                                                    let title = entry.title.clone();
                                                    let day_num = entry.date.format("%d").to_string();
                                                    let mon_day = entry.date.format("%b · %Y").to_string().to_uppercase();
                                                    let time_hm = entry.date.format("%H:%M").to_string();
                                                    let cover_photo = entry.cover_photo.clone();
                                                    let location = entry.location.clone();
                                                    let weather = entry.weather.clone();
                                                    let starred = entry.starred;
                                                    let has_audio = entry.has_audio;
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
                                                                    view! { <h3 class="timeline-row-title">{title}</h3> }
                                                                })}
                                                                {(!snippet.is_empty()).then(|| {
                                                                    view! { <p class="timeline-row-excerpt">{snippet}</p> }
                                                                })}

                                                                <div class="timeline-row-meta">
                                                                    {location.map(|loc| {
                                                                        view! {
                                                                            <span class="meta-text-item">
                                                                                <IconMapPin class="tiny-icon" />
                                                                                <span>{loc.place_name}</span>
                                                                            </span>
                                                                        }
                                                                    })}
                                                                    {weather.map(|w| {
                                                                        view! {
                                                                            <span class="meta-text-item">
                                                                                <WeatherGlyph icon_key=w.icon_key class="tiny-icon" />
                                                                                <span>{w.condition}</span>
                                                                            </span>
                                                                        }
                                                                    })}
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
                                                                    {starred.then(|| {
                                                                        view! {
                                                                            <span class="meta-star-mark" title="Starred">
                                                                                <IconStar filled=true class="tiny-icon" />
                                                                            </span>
                                                                        }
                                                                    })}
                                                                </div>
                                                            </div>

                                                            {cover_photo.map(|src| {
                                                                view! {
                                                                    <div class="timeline-photo-thumb">
                                                                        <img src=src alt="" loading="lazy" />
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
