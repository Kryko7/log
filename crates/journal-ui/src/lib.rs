//! Leptos CSR frontend and WebGPU ambient paper/ink shader canvas for **Log**.

pub mod audio_recorder;
pub mod bridge;
pub mod editor;
pub mod gpu_canvas;
pub mod icons;
pub mod sidebar;
pub mod theme;
pub mod views;

use chrono::{Datelike, NaiveDate, Utc};
use gloo_timers::future::TimeoutFuture;
use journal_types::{
    AssetManifest, CalendarDaySummary, DailyContext, Entry, EntrySummary, Notebook,
    OnThisDayGroup, SearchHit, SearchQuery, TagCount, Template, VaultStats,
};
use leptos::prelude::*;
use uuid::Uuid;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::editor::EntrySpreadEditor;
use crate::gpu_canvas::GpuPaperCanvas;
use crate::icons::{IconChevronRight, IconClose, IconSearch, IconStar};
use crate::sidebar::{SanctuaryTab, Sidebar};
use crate::views::{
    BackupSyncModal, CalendarMosaicView, MigrationModal, OnThisDayView, TimelineView,
};

/// Render an FTS5 snippet containing `<mark>...</mark>` tags safely as Leptos views.
fn render_highlighted_snippet(snippet: &str) -> impl IntoView {
    let mut nodes: Vec<AnyView> = Vec::new();
    let mut rest = snippet;

    while let Some(start) = rest.find("<mark>") {
        let prefix = &rest[..start];
        if !prefix.is_empty() {
            nodes.push(view! { <span>{prefix.to_string()}</span> }.into_any());
        }
        let after_open = &rest[start + 6..];
        if let Some(end) = after_open.find("</mark>") {
            let highlighted = &after_open[..end];
            nodes.push(view! { <mark>{highlighted.to_string()}</mark> }.into_any());
            rest = &after_open[end + 7..];
        } else {
            nodes.push(view! { <span>{after_open.to_string()}</span> }.into_any());
            rest = "";
            break;
        }
    }

    if !rest.is_empty() {
        nodes.push(view! { <span>{rest.to_string()}</span> }.into_any());
    }

    nodes.collect_view()
}

/// Instant SQLite FTS5 Search Palette (`Ctrl+K`).
#[component]
fn SearchPaletteModal(
    open: Signal<bool>,
    selected_notebook: Signal<Option<String>>,
    selected_tag: Signal<Option<String>>,
    on_close: Callback<()>,
    on_open_entry: Callback<Uuid>,
) -> impl IntoView {
    let input_ref = NodeRef::<leptos::html::Input>::new();
    let query_text = RwSignal::new(String::new());
    let hits = RwSignal::new(Vec::<SearchHit>::new());
    let searching = RwSignal::new(false);
    let search_seq = RwSignal::new(0_u64);
    let error_msg = RwSignal::new(Option::<String>::None);

    let run_search = move |raw_q: String| {
        let seq = search_seq.get_untracked().wrapping_add(1);
        search_seq.set(seq);
        let nb = selected_notebook.get_untracked();
        let tg = selected_tag.get_untracked();

        spawn_local(async move {
            TimeoutFuture::new(150).await;
            if search_seq.get_untracked() != seq {
                return;
            }
            let trimmed = raw_q.trim().to_string();
            searching.set(true);
            error_msg.set(None);

            let q = SearchQuery {
                text: if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                },
                notebook_id: nb,
                tag: tg,
                limit: Some(25),
                ..Default::default()
            };

            match bridge::search_entries(q).await {
                Ok(res) => {
                    if search_seq.get_untracked() == seq {
                        hits.set(res);
                        searching.set(false);
                    }
                }
                Err(err) => {
                    if search_seq.get_untracked() == seq {
                        error_msg.set(Some(err));
                        searching.set(false);
                    }
                }
            }
        });
    };

    Effect::new(move |_| {
        if open.get() {
            if let Some(el) = input_ref.get() {
                let _ = el.focus();
            }
            run_search(query_text.get_untracked());
        }
    });

    view! {
        <Show when=move || open.get()>
            <div
                class="search-modal-backdrop"
                role="dialog"
                aria-modal="true"
                aria-label="Search journal archive"
                on:click=move |_| on_close.run(())
            >
                <div
                    class="search-modal-sheet"
                    on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()
                >
                    <div class="search-modal-input-row">
                        <IconSearch />
                        <input
                            node_ref=input_ref
                            type="text"
                            class="search-modal-input"
                            placeholder="Search entries…"
                            prop:value=move || query_text.get()
                            on:input=move |ev| {
                                let val = event_target_value(&ev);
                                query_text.set(val.clone());
                                run_search(val);
                            }
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if ev.key() == "Escape" {
                                    ev.prevent_default();
                                    on_close.run(());
                                }
                            }
                        />
                        <button
                            type="button"
                            class="icon-btn subtle"
                            title="Close search"
                            on:click=move |_| on_close.run(())
                        >
                            <IconClose />
                        </button>
                    </div>

                    {move || {
                        error_msg
                            .get()
                            .map(|err| view! { <p class="form-error">{err}</p> })
                    }}

                    <div class="search-results-list">
                        {move || {
                            if searching.get() && hits.get().is_empty() {
                                return view! {
                                    <div class="muted-empty-note">"Searching…"</div>
                                }.into_any();
                            }
                            let list = hits.get();
                            if list.is_empty() {
                                return view! {
                                    <div class="muted-empty-note">"No matching entries found."</div>
                                }.into_any();
                            }

                            list.into_iter()
                                .map(|hit| {
                                    let entry_id = hit.summary.id;
                                    let date_label = hit.summary.date.format("%b %d, %Y").to_string();
                                    let title = if hit.summary.title.trim().is_empty() {
                                        "Untitled Entry".to_string()
                                    } else {
                                        hit.summary.title.clone()
                                    };
                                    let starred = hit.summary.starred;
                                    let snippet = if hit.highlighted_snippet.is_empty() {
                                        hit.summary.snippet.clone()
                                    } else {
                                        hit.highlighted_snippet.clone()
                                    };
                                    view! {
                                        <button
                                            type="button"
                                            class="search-hit-card"
                                            on:click=move |_| {
                                                on_open_entry.run(entry_id);
                                                on_close.run(());
                                            }
                                        >
                                            <div class="search-hit-head">
                                                <span class="search-hit-title">{title}</span>
                                                <span class="small-caps-meta">{date_label}</span>
                                                {starred.then(|| view! { <IconStar filled=true /> })}
                                            </div>
                                            <p class="search-hit-snippet">
                                                {render_highlighted_snippet(&snippet)}
                                            </p>
                                        </button>
                                    }
                                })
                                .collect_view()
                                .into_any()
                        }}
                    </div>
                </div>
            </div>
        </Show>
    }
}

/// Root Leptos application component for **Log**.
#[component]
pub fn App() -> impl IntoView {
    let today = Utc::now().date_naive();

    let night_mode = RwSignal::new(theme::load_initial_night_mode());
    Effect::new(move |_| {
        theme::apply_theme(night_mode.get());
    });

    // Core vault state signals
    let stats = RwSignal::new(Option::<VaultStats>::None);
    let notebooks = RwSignal::new(Vec::<Notebook>::new());
    let tags = RwSignal::new(Vec::<TagCount>::new());
    let filtered_entries = RwSignal::new(Vec::<EntrySummary>::new());
    let all_entries = RwSignal::new(Vec::<EntrySummary>::new());
    let active_entry = RwSignal::new(Option::<Entry>::None);
    let templates = RwSignal::new(Vec::<Template>::new());
    let daily_context = RwSignal::new(Option::<DailyContext>::None);
    let manifest = RwSignal::new(AssetManifest::default());
    let on_this_day = RwSignal::new(Vec::<OnThisDayGroup>::new());

    // Calendar state
    let cal_year = RwSignal::new(today.year());
    let cal_month = RwSignal::new(today.month());
    let cal_days = RwSignal::new(Vec::<CalendarDaySummary>::new());

    // Navigation & filters
    let active_tab = RwSignal::new(SanctuaryTab::Spread);
    let sidebar_collapsed = RwSignal::new(false);
    let selected_notebook = RwSignal::new(Option::<String>::None);
    let selected_tag = RwSignal::new(Option::<String>::None);
    let reset_nonce = RwSignal::new(0_u64);
    let page_turn_class = RwSignal::new("");

    // Modals
    let search_open = RwSignal::new(false);
    let migration_open = RwSignal::new(false);
    let backup_open = RwSignal::new(false);

    // Refresh aggregate metadata + entry lists
    let refresh_vault = move |open_first_if_none: bool| {
        let nb_filter = selected_notebook.get_untracked();
        let tag_filter = selected_tag.get_untracked();
        let cy = cal_year.get_untracked();
        let cm = cal_month.get_untracked();

        spawn_local(async move {
            if let Ok(st) = bridge::get_vault_stats().await {
                stats.set(Some(st));
            }
            if let Ok(nbs) = bridge::list_notebooks().await {
                notebooks.set(nbs);
            }
            if let Ok(tgs) = bridge::list_tags().await {
                tags.set(tgs);
            }
            if let Ok(all) = bridge::list_entries(None, None, Some(300), Some(0)).await {
                let first_id = all.first().map(|e| e.id);
                all_entries.set(all.clone());
                if nb_filter.is_none() && tag_filter.is_none() {
                    filtered_entries.set(all);
                }
                if open_first_if_none && active_entry.get_untracked().is_none() {
                    if let Some(id) = first_id {
                        if let Ok(loaded) = bridge::get_entry(id).await {
                            active_entry.set(Some(loaded));
                        }
                    }
                }
            }
            if nb_filter.is_some() || tag_filter.is_some() {
                if let Ok(filtered) =
                    bridge::list_entries(nb_filter.clone(), tag_filter, Some(300), Some(0)).await
                {
                    filtered_entries.set(filtered);
                }
            }
            if let Ok(otd) = bridge::get_on_this_day(None, None).await {
                on_this_day.set(otd);
            }
            if let Ok(days) = bridge::get_calendar_month(cy, cm, nb_filter).await {
                cal_days.set(days);
            }
        });
    };

    // Initial bootstrap on mount
    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(m) = bridge::get_asset_manifest().await {
                manifest.set(m);
            }
            if let Ok(tpls) = bridge::list_templates().await {
                templates.set(tpls);
            }
            if let Ok(ctx) = bridge::get_daily_context(None, None, None).await {
                daily_context.set(Some(ctx));
            }
        });
        refresh_vault(true);
    });

    // Global Ctrl+K / Cmd+K shortcut for instant FTS5 search
    Effect::new(move |_| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let closure = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |ev: web_sys::KeyboardEvent| {
                if (ev.ctrl_key() || ev.meta_key()) && ev.key().eq_ignore_ascii_case("k") {
                    ev.prevent_default();
                    search_open.update(|v| *v = !*v);
                } else if (ev.ctrl_key() || ev.meta_key()) && ev.key() == "\\" {
                    ev.prevent_default();
                    sidebar_collapsed.update(|v| *v = !*v);
                } else if ev.key() == "Escape" && search_open.get_untracked() {
                    search_open.set(false);
                }
            },
        );
        let _ = window.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());
        closure.forget();
    });

    // Open a specific entry by UUID and switch to Spread view
    let open_entry_by_id = Callback::new(move |id: Uuid| {
        active_tab.set(SanctuaryTab::Spread);
        spawn_local(async move {
            if let Ok(loaded) = bridge::get_entry(id).await {
                active_entry.set(Some(loaded));
            }
        });
    });

    // Create a blank folio in the active notebook
    let start_new_folio = Callback::new(move |_: ()| {
        active_entry.set(None);
        reset_nonce.update(|n| *n = n.wrapping_add(1));
        active_tab.set(SanctuaryTab::Spread);
    });

    // Select notebook filter & reload filtered entries + calendar
    let on_select_notebook = Callback::new(move |nb: Option<String>| {
        selected_notebook.set(nb.clone());
        let tag = selected_tag.get_untracked();
        let cy = cal_year.get_untracked();
        let cm = cal_month.get_untracked();

        if active_tab.get_untracked() == SanctuaryTab::Spread {
            active_tab.set(SanctuaryTab::Timeline);
        }

        spawn_local(async move {
            if let Ok(list) = bridge::list_entries(nb.clone(), tag, Some(300), Some(0)).await {
                filtered_entries.set(list);
            }
            if let Ok(days) = bridge::get_calendar_month(cy, cm, nb).await {
                cal_days.set(days);
            }
        });
    });

    // Select tag filter & reload filtered entries
    let on_select_tag = Callback::new(move |tag: Option<String>| {
        selected_tag.set(tag.clone());
        let nb = selected_notebook.get_untracked();
        if tag.is_some() {
            active_tab.set(SanctuaryTab::Timeline);
        }
        spawn_local(async move {
            if let Ok(list) = bridge::list_entries(nb, tag, Some(300), Some(0)).await {
                filtered_entries.set(list);
            }
        });
    });

    // Page-turn navigation across chronological folios with 3D perspective transition
    let on_page_turn = Callback::new(move |delta: i32| {
        let list = all_entries.get_untracked();
        if list.is_empty() {
            return;
        }
        let current_id = active_entry.get_untracked().map(|e| e.id);
        let current_idx = current_id
            .and_then(|cid| list.iter().position(|item| item.id == cid))
            .unwrap_or(0);

        let next_idx = if delta < 0 {
            // Previous folio (step backward in time along reverse-chronological list)
            (current_idx + 1).min(list.len().saturating_sub(1))
        } else {
            // Next folio (step forward toward most recent)
            current_idx.saturating_sub(1)
        };

        let anim_cls = if delta < 0 {
            "page-turn-prev"
        } else {
            "page-turn-next"
        };
        page_turn_class.set(anim_cls);
        spawn_local(async move {
            TimeoutFuture::new(380).await;
            page_turn_class.set("");
        });

        if let Some(target) = list.get(next_idx) {
            let target_id = target.id;
            spawn_local(async move {
                if let Ok(loaded) = bridge::get_entry(target_id).await {
                    active_entry.set(Some(loaded));
                }
            });
        }
    });

    // Badge count for On This Day memories
    let on_this_day_count = Signal::derive(move || {
        let groups_count: usize = on_this_day.get().iter().map(|g| g.entries.len()).sum();
        if groups_count > 0 {
            groups_count
        } else {
            daily_context
                .get()
                .map(|ctx| ctx.on_this_day_count)
                .unwrap_or(0)
        }
    });

    view! {
        <div class="log-root">
            <GpuPaperCanvas night_mode=night_mode.into() />

            <div class="sanctuary-shell" class:is-sidebar-collapsed=move || sidebar_collapsed.get()>
                <Sidebar
                    active_tab=active_tab.read_only()
                    on_select_tab=Callback::new(move |tab| active_tab.set(tab))
                    notebooks=notebooks.into()
                    selected_notebook=selected_notebook.read_only()
                    on_select_notebook=on_select_notebook
                    tags=tags.into()
                    selected_tag=selected_tag.read_only()
                    on_select_tag=on_select_tag
                    stats=stats.into()
                    on_this_day_count=on_this_day_count
                    manifest=manifest.into()
                    night_mode=night_mode
                    on_new_folio=start_new_folio
                    on_open_search=Callback::new(move |_| search_open.set(true))
                    on_open_migration=Callback::new(move |_| migration_open.set(true))
                    on_open_backup=Callback::new(move |_| backup_open.set(true))
                    on_notebook_created=Callback::new(move |nb| {
                        notebooks.update(|list| list.push(nb));
                        refresh_vault(false);
                    })
                    on_toggle_collapse=Callback::new(move |_| sidebar_collapsed.update(|c| *c = !*c))
                />

                <main class="sanctuary-main">
                    <Show when=move || sidebar_collapsed.get()>
                        <button
                            type="button"
                            class="sidebar-reopen-btn"
                            title="Show sidebar (Ctrl+\\)"
                            aria-label="Show sidebar"
                            on:click=move |_| sidebar_collapsed.set(false)
                        >
                            <IconChevronRight />
                        </button>
                    </Show>
                    {move || match active_tab.get() {
                        SanctuaryTab::Spread => view! {
                            <EntrySpreadEditor
                                entry=active_entry.into()
                                notebooks=notebooks.into()
                                templates=templates.into()
                                daily_context=daily_context.into()
                                manifest=manifest.into()
                                all_entries=all_entries.into()
                                default_notebook_id=selected_notebook.into()
                                reset_nonce=reset_nonce.into()
                                page_turn_class=page_turn_class.into()
                                on_saved=Callback::new(move |saved: Entry| {
                                    active_entry.set(Some(saved));
                                    refresh_vault(false);
                                })
                                on_deleted=Callback::new(move |_| {
                                    active_entry.set(None);
                                    refresh_vault(true);
                                })
                                on_open_entry=open_entry_by_id
                                on_page_turn=on_page_turn
                            />
                        }
                        .into_any(),

                        SanctuaryTab::Timeline => view! {
                            <TimelineView
                                entries=filtered_entries.into()
                                notebooks=notebooks.into()
                                on_open_entry=open_entry_by_id
                                on_select_tag=Callback::new(move |t: String| {
                                    on_select_tag.run(Some(t));
                                })
                                on_new_entry=start_new_folio
                            />
                        }
                        .into_any(),

                        SanctuaryTab::Calendar => view! {
                            <CalendarMosaicView
                                year=cal_year.into()
                                month=cal_month.into()
                                days=cal_days.into()
                                entries=filtered_entries.into()
                                on_change_month=Callback::new(move |(y, m): (i32, u32)| {
                                    cal_year.set(y);
                                    cal_month.set(m);
                                    let nb = selected_notebook.get_untracked();
                                    spawn_local(async move {
                                        if let Ok(days) = bridge::get_calendar_month(y, m, nb).await {
                                            cal_days.set(days);
                                        }
                                    });
                                })
                                on_select_date=Callback::new(move |date: NaiveDate| {
                                    if let Some(day_summary) = cal_days
                                        .get_untracked()
                                        .into_iter()
                                        .find(|d| d.date == date)
                                    {
                                        if let Some(pid) = day_summary.primary_entry_id {
                                            open_entry_by_id.run(pid);
                                            return;
                                        }
                                    }
                                    if let Some(found) = all_entries
                                        .get_untracked()
                                        .into_iter()
                                        .find(|e| e.date.date_naive() == date)
                                    {
                                        open_entry_by_id.run(found.id);
                                    } else {
                                        start_new_folio.run(());
                                    }
                                })
                            />
                        }
                        .into_any(),

                        SanctuaryTab::OnThisDay => view! {
                            <OnThisDayView
                                groups=on_this_day.into()
                                on_open_entry=open_entry_by_id
                            />
                        }
                        .into_any(),
                    }}
                </main>
            </div>

            // Search Palette Modal (Ctrl+K)
            <SearchPaletteModal
                open=search_open.into()
                selected_notebook=selected_notebook.into()
                selected_tag=selected_tag.into()
                on_close=Callback::new(move |_| search_open.set(false))
                on_open_entry=open_entry_by_id
            />

            // Archive Migration Studio Modal
            <MigrationModal
                open=migration_open.into()
                on_close=Callback::new(move |_| migration_open.set(false))
                on_migrated=Callback::new(move |_| refresh_vault(true))
            />

            // Encrypted Sanctuary Backup & Google Drive Sync Modal
            <BackupSyncModal
                open=backup_open.into()
                on_close=Callback::new(move |_| backup_open.set(false))
                on_restored=Callback::new(move |_| refresh_vault(true))
            />
        </div>
    }
}

/// Mount the Leptos application into `#app`.
pub fn mount_app() {
    console_error_panic_hook::set_once();
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Ok(Some(root)) = doc.query_selector("#app") {
            let html_root: web_sys::HtmlElement = root.unchecked_into();
            leptos::mount::mount_to(html_root, App).forget();
            return;
        }
    }
    leptos::mount::mount_to_body(App);
}
