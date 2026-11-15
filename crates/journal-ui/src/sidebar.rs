//! Left spine index sidebar: Masthead, Navigation, Notebooks Shelf, Tags, Utilities & Lamp Toggle.

use journal_types::{AssetManifest, CreateNotebookRequest, Notebook, TagCount, VaultStats};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::bridge;
use crate::icons::{
    IconArchiveImport, IconBookOpen, IconCalendar, IconClockHistory, IconClose, IconCloudLock,
    IconFeather, IconLamp, IconNotebook, IconPlus, IconSearch, IconSun, IconTag,
};

/// Top-level navigation sections inside the book spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanctuaryTab {
    Spread,
    Timeline,
    Calendar,
    OnThisDay,
}

const DEFAULT_COVER_CHOICES: [(&str, &str); 4] = [
    ("Botanical Fern", "/assets/covers/botanical-fern-wash.png"),
    ("Rainy Tea Desk", "/assets/covers/rainy-tea-desk.png"),
    ("Pressed Wildflowers", "/assets/covers/pressed-wildflowers.png"),
    ("Midnight Ink Garden", "/assets/covers/midnight-ink-garden.png"),
];

const ACCENT_PALETTE: [&str; 4] = ["#546E5A", "#B5563E", "#6D6157", "#3F5963"];

/// Left sanctuary spine shelf with navigation, notebooks, tags, archive modals, and lamp toggle.
#[component]
pub fn Sidebar(
    active_tab: ReadSignal<SanctuaryTab>,
    on_select_tab: Callback<SanctuaryTab>,
    notebooks: Signal<Vec<Notebook>>,
    selected_notebook: ReadSignal<Option<String>>,
    on_select_notebook: Callback<Option<String>>,
    tags: Signal<Vec<TagCount>>,
    selected_tag: ReadSignal<Option<String>>,
    on_select_tag: Callback<Option<String>>,
    stats: Signal<Option<VaultStats>>,
    on_this_day_count: Signal<usize>,
    manifest: Signal<AssetManifest>,
    night_mode: RwSignal<bool>,
    on_new_folio: Callback<()>,
    on_open_search: Callback<()>,
    on_open_migration: Callback<()>,
    on_open_backup: Callback<()>,
    on_notebook_created: Callback<Notebook>,
) -> impl IntoView {
    let creating_nb = RwSignal::new(false);
    let new_nb_name = RwSignal::new(String::new());
    let new_nb_cover = RwSignal::new("/assets/covers/botanical-fern-wash.png".to_string());
    let new_nb_accent = RwSignal::new("#546E5A".to_string());
    let nb_saving = RwSignal::new(false);
    let nb_error = RwSignal::new(Option::<String>::None);

    let submit_new_notebook = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let name = new_nb_name.get_untracked().trim().to_string();
        if name.is_empty() || nb_saving.get_untracked() {
            return;
        }
        nb_saving.set(true);
        nb_error.set(None);
        let cover = new_nb_cover.get_untracked();
        let accent = new_nb_accent.get_untracked();

        spawn_local(async move {
            let req = CreateNotebookRequest {
                name,
                description: None,
                cover_asset: Some(cover),
                accent_color: Some(accent),
            };
            match bridge::create_notebook(req).await {
                Ok(nb) => {
                    nb_saving.set(false);
                    new_nb_name.set(String::new());
                    creating_nb.set(false);
                    on_notebook_created.run(nb);
                }
                Err(err) => {
                    nb_saving.set(false);
                    nb_error.set(Some(err));
                }
            }
        });
    };

    let toggle_lamp = move |_| {
        night_mode.update(|n| *n = !*n);
    };

    view! {
        <aside class="sanctuary-sidebar" aria-label="Journal spine index">
            <div class="sidebar-masthead">
                <div class="masthead-brand">
                    <img
                        src="/assets/svg/app-icon.svg"
                        alt="Log emblem"
                        class="masthead-emblem"
                        width="28"
                        height="28"
                    />
                    <div class="masthead-titles">
                        <span class="masthead-wordmark">"LOG"</span>
                        <span class="small-caps-meta masthead-subtitle">"Personal Folio"</span>
                    </div>
                </div>

                <button
                    type="button"
                    class="lamp-toggle-btn"
                    title=move || if night_mode.get() { "Switch to Daylight Paper" } else { "Switch to Candlelit Slate" }
                    on:click=toggle_lamp
                >
                    <Show when=move || night_mode.get() fallback=|| view! { <IconLamp /> }>
                        <IconSun />
                    </Show>
                </button>
            </div>

            <div class="sidebar-primary-actions">
                <button
                    type="button"
                    class="btn-new-folio"
                    on:click=move |_| on_new_folio.run(())
                >
                    <IconFeather />
                    <span>"New Folio"</span>
                </button>

                <button
                    type="button"
                    class="btn-search-trigger"
                    title="Search archive (Ctrl+K)"
                    on:click=move |_| on_open_search.run(())
                >
                    <IconSearch />
                    <span>"Search"</span>
                    <kbd class="kbd-hint">"Ctrl K"</kbd>
                </button>
            </div>

            <nav class="sidebar-nav" aria-label="Primary views">
                <button
                    type="button"
                    class="sidebar-nav-item"
                    class:active=move || active_tab.get() == SanctuaryTab::Spread
                    on:click=move |_| on_select_tab.run(SanctuaryTab::Spread)
                >
                    <IconBookOpen />
                    <span>"Open Spread"</span>
                </button>

                <button
                    type="button"
                    class="sidebar-nav-item"
                    class:active=move || active_tab.get() == SanctuaryTab::Timeline
                    on:click=move |_| on_select_tab.run(SanctuaryTab::Timeline)
                >
                    <IconNotebook />
                    <span>"Chronicle"</span>
                </button>

                <button
                    type="button"
                    class="sidebar-nav-item"
                    class:active=move || active_tab.get() == SanctuaryTab::Calendar
                    on:click=move |_| on_select_tab.run(SanctuaryTab::Calendar)
                >
                    <IconCalendar />
                    <span>"Calendar Mosaic"</span>
                </button>

                <button
                    type="button"
                    class="sidebar-nav-item"
                    class:active=move || active_tab.get() == SanctuaryTab::OnThisDay
                    on:click=move |_| on_select_tab.run(SanctuaryTab::OnThisDay)
                >
                    <IconClockHistory />
                    <span>"On This Day"</span>
                    {move || {
                        let count = on_this_day_count.get();
                        (count > 0).then(|| view! {
                            <span class="sidebar-badge">{count}</span>
                        })
                    }}
                </button>
            </nav>

            <div class="sidebar-scroll-region">
                <section class="sidebar-section">
                    <div class="sidebar-section-header">
                        <span class="small-caps-meta">"Notebooks"</span>
                        <button
                            type="button"
                            class="icon-btn subtle"
                            title="New Notebook"
                            on:click=move |_| creating_nb.update(|v| *v = !*v)
                        >
                            <Show when=move || creating_nb.get() fallback=|| view! { <IconPlus /> }>
                                <IconClose />
                            </Show>
                        </button>
                    </div>

                    <Show when=move || creating_nb.get()>
                        <form class="inline-notebook-form" on:submit=submit_new_notebook>
                            <input
                                type="text"
                                class="editorial-input"
                                placeholder="Notebook title..."
                                prop:value=move || new_nb_name.get()
                                on:input=move |ev| new_nb_name.set(event_target_value(&ev))
                            />

                            <div class="cover-swatch-row">
                                {move || {
                                    let covers = manifest.get().covers;
                                    if covers.is_empty() {
                                        DEFAULT_COVER_CHOICES
                                            .iter()
                                            .map(|(label, path)| {
                                                let path_s = (*path).to_string();
                                                let path_click = path_s.clone();
                                                let label_s = (*label).to_string();
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="cover-swatch-btn"
                                                        class:selected=move || new_nb_cover.get() == path_s
                                                        title=label_s
                                                        on:click=move |_| new_nb_cover.set(path_click.clone())
                                                    >
                                                        <img src=*path alt=*label />
                                                    </button>
                                                }
                                            })
                                            .collect_view()
                                            .into_any()
                                    } else {
                                        covers
                                            .into_iter()
                                            .map(|item| {
                                                let path_s = item.path.clone();
                                                let path_click = path_s.clone();
                                                let path_src = path_s.clone();
                                                let label_s = item.label.clone();
                                                let title_s = label_s.clone();
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="cover-swatch-btn"
                                                        class:selected=move || new_nb_cover.get() == path_s
                                                        title=title_s
                                                        on:click=move |_| new_nb_cover.set(path_click.clone())
                                                    >
                                                        <img src=path_src alt=label_s />
                                                    </button>
                                                }
                                            })
                                            .collect_view()
                                            .into_any()
                                    }
                                }}
                            </div>

                            <div class="accent-swatch-row">
                                {ACCENT_PALETTE
                                    .iter()
                                    .map(|hex| {
                                        let hex_s = (*hex).to_string();
                                        let hex_click = hex_s.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="accent-dot-btn"
                                                class:selected=move || new_nb_accent.get() == hex_s
                                                style=format!("background-color: {hex};")
                                                title=format!("Spine color {hex}")
                                                on:click=move |_| new_nb_accent.set(hex_click.clone())
                                            ></button>
                                        }
                                    })
                                    .collect_view()}
                            </div>

                            {move || nb_error.get().map(|err| view! { <p class="form-error">{err}</p> })}

                            <button
                                type="submit"
                                class="btn-editorial btn-sage full-width"
                                disabled=move || nb_saving.get()
                            >
                                {move || if nb_saving.get() { "Binding..." } else { "Bind Notebook" }}
                            </button>
                        </form>
                    </Show>

                    <div class="notebook-shelf-list">
                        <button
                            type="button"
                            class="notebook-shelf-item"
                            class:active=move || selected_notebook.get().is_none()
                            on:click=move |_| on_select_notebook.run(None)
                        >
                            <span class="notebook-all-dot"></span>
                            <span class="notebook-name">"All Folios"</span>
                        </button>

                        {move || {
                            notebooks
                                .get()
                                .into_iter()
                                .map(|nb| {
                                    let nb_id = nb.id.clone();
                                    let nb_id_cmp = nb_id.clone();
                                    let nb_id_click = nb_id.clone();
                                    view! {
                                        <button
                                            type="button"
                                            class="notebook-shelf-item"
                                            class:active=move || selected_notebook.get().as_deref() == Some(nb_id_cmp.as_str())
                                            on:click=move |_| on_select_notebook.run(Some(nb_id_click.clone()))
                                        >
                                            <img
                                                src=nb.cover_asset.clone()
                                                alt=nb.name.clone()
                                                class="notebook-mini-cover"
                                            />
                                            <span
                                                class="notebook-spine-pip"
                                                style=format!("background-color: {};", nb.accent_color)
                                            ></span>
                                            <span class="notebook-name">{nb.name}</span>
                                            <span class="notebook-count">{nb.entry_count}</span>
                                        </button>
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                </section>

                <section class="sidebar-section">
                    <div class="sidebar-section-header">
                        <span class="small-caps-meta">"Index Tags"</span>
                        <Show when=move || selected_tag.get().is_some()>
                            <button
                                type="button"
                                class="link-clear-btn"
                                on:click=move |_| on_select_tag.run(None)
                            >
                                "Clear"
                            </button>
                        </Show>
                    </div>

                    <div class="sidebar-tag-cloud">
                        {move || {
                            let list = tags.get();
                            if list.is_empty() {
                                view! { <span class="muted-empty-note">"No tags indexed yet"</span> }.into_any()
                            } else {
                                list.into_iter()
                                    .map(|tc| {
                                        let tag_name = tc.tag.clone();
                                        let tag_cmp = tag_name.clone();
                                        let tag_click = tag_name.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="sidebar-tag-pill"
                                                class:active=move || selected_tag.get().as_deref() == Some(tag_cmp.as_str())
                                                on:click=move |_| {
                                                    if selected_tag.get_untracked().as_deref() == Some(tag_click.as_str()) {
                                                        on_select_tag.run(None);
                                                    } else {
                                                        on_select_tag.run(Some(tag_click.clone()));
                                                    }
                                                }
                                            >
                                                <IconTag />
                                                <span>{tag_name}</span>
                                                <span class="tag-pill-count">{tc.count}</span>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            }
                        }}
                    </div>
                </section>

                <section class="sidebar-section">
                    <div class="sidebar-section-header">
                        <span class="small-caps-meta">"Sanctuary Tools"</span>
                    </div>
                    <div class="sidebar-utility-list">
                        <button
                            type="button"
                            class="sidebar-utility-btn"
                            on:click=move |_| on_open_migration.run(())
                        >
                            <IconArchiveImport />
                            <span>"Migrate Day One & Notion"</span>
                        </button>
                        <button
                            type="button"
                            class="sidebar-utility-btn"
                            on:click=move |_| on_open_backup.run(())
                        >
                            <IconCloudLock />
                            <span>"Encrypted Drive Vault"</span>
                        </button>
                    </div>
                </section>
            </div>

            <footer class="sidebar-footer">
                {move || match stats.get() {
                    Some(st) => view! {
                        <div class="vault-stats-ribbon">
                            <div class="vault-stat-cell">
                                <span class="stat-val">{st.current_streak_days}</span>
                                <span class="small-caps-meta">"Day Streak"</span>
                            </div>
                            <div class="vault-stat-cell">
                                <span class="stat-val">{st.total_words}</span>
                                <span class="small-caps-meta">"Words"</span>
                            </div>
                            <div class="vault-stat-cell">
                                <span class="stat-val">{st.total_entries}</span>
                                <span class="small-caps-meta">"Folios"</span>
                            </div>
                        </div>
                    }.into_any(),
                    None => view! {
                        <div class="vault-stats-ribbon">
                            <span class="small-caps-meta">"Local Markdown Vault"</span>
                        </div>
                    }.into_any(),
                }}
            </footer>
        </aside>
    }
}
