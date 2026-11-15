//! Quiet editorial left sidebar: Navigation, Notebooks, Tags, Utilities & Theme Toggle.

use journal_types::{AssetManifest, CreateNotebookRequest, Notebook, TagCount, VaultStats};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::bridge;
use crate::icons::{
    IconArchiveImport, IconBookOpen, IconCalendar, IconChevronLeft, IconClockHistory, IconClose,
    IconCloudLock, IconFeather, IconLamp, IconNotebook, IconPlus, IconSearch, IconSun,
};

/// Top-level navigation sections inside the workspace.
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

fn format_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (idx, ch) in s.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// Quiet 236px editorial sidebar with navigation, notebooks, tags, and utility footer.
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
    #[prop(optional)] on_toggle_collapse: Option<Callback<()>>,
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
        <aside class="editorial-sidebar" aria-label="Journal navigation">
            <div class="sidebar-header">
                <span class="sidebar-wordmark">"Log"</span>
                {on_toggle_collapse.map(|cb| {
                    view! {
                        <button
                            type="button"
                            class="sidebar-icon-btn"
                            title="Collapse sidebar (Ctrl+\\)"
                            aria-label="Collapse sidebar"
                            on:click=move |_| cb.run(())
                        >
                            <IconChevronLeft />
                        </button>
                    }
                })}
            </div>

            <div class="sidebar-primary-actions">
                <button
                    type="button"
                    class="sidebar-new-btn"
                    on:click=move |_| on_new_folio.run(())
                >
                    <IconFeather />
                    <span>"New Entry"</span>
                </button>

                <button
                    type="button"
                    class="sidebar-search-btn"
                    title="Search entries (Ctrl+K)"
                    on:click=move |_| on_open_search.run(())
                >
                    <IconSearch />
                    <span>"Search"</span>
                    <kbd>"Ctrl K"</kbd>
                </button>
            </div>

            <nav class="sidebar-nav" aria-label="Primary views">
                <button
                    type="button"
                    class="sidebar-nav-item"
                    class:is-active=move || active_tab.get() == SanctuaryTab::Spread
                    on:click=move |_| on_select_tab.run(SanctuaryTab::Spread)
                >
                    <IconBookOpen />
                    <span class="sidebar-nav-label">"Editor"</span>
                </button>

                <button
                    type="button"
                    class="sidebar-nav-item"
                    class:is-active=move || active_tab.get() == SanctuaryTab::Timeline
                    on:click=move |_| on_select_tab.run(SanctuaryTab::Timeline)
                >
                    <IconNotebook />
                    <span class="sidebar-nav-label">"All Entries"</span>
                    {move || {
                        stats.get().map(|st| {
                            view! { <span class="sidebar-item-count">{st.total_entries}</span> }
                        })
                    }}
                </button>

                <button
                    type="button"
                    class="sidebar-nav-item"
                    class:is-active=move || active_tab.get() == SanctuaryTab::Calendar
                    on:click=move |_| on_select_tab.run(SanctuaryTab::Calendar)
                >
                    <IconCalendar />
                    <span class="sidebar-nav-label">"Calendar"</span>
                </button>

                <button
                    type="button"
                    class="sidebar-nav-item"
                    class:is-active=move || active_tab.get() == SanctuaryTab::OnThisDay
                    on:click=move |_| on_select_tab.run(SanctuaryTab::OnThisDay)
                >
                    <IconClockHistory />
                    <span class="sidebar-nav-label">"On This Day"</span>
                    {move || {
                        let count = on_this_day_count.get();
                        (count > 0).then(|| view! {
                            <span class="sidebar-item-count">{count}</span>
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
                            class="sidebar-icon-btn"
                            title="New notebook"
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
                                placeholder="Notebook name…"
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
                                                title=format!("Color {hex}")
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
                                {move || if nb_saving.get() { "Saving…" } else { "Create Notebook" }}
                            </button>
                        </form>
                    </Show>

                    <div class="notebook-shelf-list">
                        <button
                            type="button"
                            class="sidebar-notebook-item"
                            class:is-active=move || selected_notebook.get().is_none()
                            on:click=move |_| on_select_notebook.run(None)
                        >
                            <span class="notebook-color-dot is-all"></span>
                            <span class="notebook-name">"All Notebooks"</span>
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
                                            class="sidebar-notebook-item"
                                            class:is-active=move || selected_notebook.get().as_deref() == Some(nb_id_cmp.as_str())
                                            on:click=move |_| on_select_notebook.run(Some(nb_id_click.clone()))
                                        >
                                            <span
                                                class="notebook-color-dot"
                                                style=format!("background-color: {};", nb.accent_color)
                                            ></span>
                                            <span class="notebook-name">{nb.name}</span>
                                            <span class="sidebar-item-count">{nb.entry_count}</span>
                                        </button>
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                </section>

                <Show when=move || !tags.get().is_empty()>
                    <section class="sidebar-section">
                        <div class="sidebar-section-header">
                            <span class="small-caps-meta">"Tags"</span>
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

                        <div class="sidebar-tag-list">
                            {move || {
                                tags.get()
                                    .into_iter()
                                    .map(|tc| {
                                        let tag_name = tc.tag.clone();
                                        let tag_cmp = tag_name.clone();
                                        let tag_click = tag_name.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="sidebar-tag-item"
                                                class:is-active=move || selected_tag.get().as_deref() == Some(tag_cmp.as_str())
                                                on:click=move |_| {
                                                    if selected_tag.get_untracked().as_deref() == Some(tag_click.as_str()) {
                                                        on_select_tag.run(None);
                                                    } else {
                                                        on_select_tag.run(Some(tag_click.clone()));
                                                    }
                                                }
                                            >
                                                <span class="sidebar-tag-name">{format!("#{tag_name}")}</span>
                                                <span class="sidebar-item-count">{tc.count}</span>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </section>
                </Show>
            </div>

            <footer class="sidebar-footer">
                <div class="sidebar-utility-row">
                    <button
                        type="button"
                        class="sidebar-utility-link"
                        on:click=move |_| on_open_migration.run(())
                    >
                        <IconArchiveImport />
                        <span>"Import"</span>
                    </button>

                    <button
                        type="button"
                        class="sidebar-utility-link"
                        on:click=move |_| on_open_backup.run(())
                    >
                        <IconCloudLock />
                        <span>"Backup"</span>
                    </button>

                    <button
                        type="button"
                        class="sidebar-icon-btn"
                        title=move || if night_mode.get() { "Switch to Day Cream Paper" } else { "Switch to Candlelit Night" }
                        aria-label="Toggle theme"
                        on:click=toggle_lamp
                    >
                        <Show when=move || night_mode.get() fallback=|| view! { <IconLamp /> }>
                            <IconSun />
                        </Show>
                    </button>
                </div>

                {move || {
                    stats.get().map(|st| {
                        view! {
                            <div class="sidebar-stats-line">
                                {format!("{} entries · {} words", st.total_entries, format_thousands(st.total_words))}
                            </div>
                        }
                    })
                }}
            </footer>
        </aside>
    }
}
