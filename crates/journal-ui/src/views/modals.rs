//! Archive Migration Studio (Day One Primary + Notion Delta) and Encrypted Sanctuary Backup modals.

use journal_types::{
    BackupConfig, BackupInfo, ImportDayOneRequest, ImportNotionRequest, MigrationReport,
    MigrationStatus, RestoreBackupRequest, RestoreBackupResponse, SyncResult,
};
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::bridge;
use crate::icons::{
    IconArchiveImport, IconCheck, IconClose, IconCloudLock, IconEye, IconRefresh, IconUpload,
};
use crate::views::{format_bytes, format_entry_date};

/// Archive Migration Studio modal for Day One `.zip` and Notion delta imports.
#[component]
pub fn MigrationModal(
    open: Signal<bool>,
    on_close: Callback<()>,
    #[prop(optional)] on_migrated: Option<Callback<MigrationReport>>,
) -> impl IntoView {
    let dayone_path = RwSignal::new(String::new());
    let notion_path = RwSignal::new(String::new());
    let dry_run = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error_msg = RwSignal::new(Option::<String>::None);
    let report = RwSignal::new(Option::<MigrationReport>::None);

    let handle_dayone_file = move |ev: leptos::ev::Event| {
        let Some(target) = ev.target() else { return };
        let Ok(input) = target.dyn_into::<HtmlInputElement>() else {
            return;
        };
        let Some(files) = input.files() else { return };
        let Some(file) = files.get(0) else { return };

        busy.set(true);
        error_msg.set(None);
        spawn_local(async move {
            match bridge::upload_dayone_zip(&file).await {
                Ok(rep) => {
                    busy.set(false);
                    report.set(Some(rep.clone()));
                    if let Some(cb) = on_migrated {
                        cb.run(rep);
                    }
                }
                Err(err) => {
                    busy.set(false);
                    error_msg.set(Some(err));
                }
            }
        });
    };

    let handle_notion_file = move |ev: leptos::ev::Event| {
        let Some(target) = ev.target() else { return };
        let Ok(input) = target.dyn_into::<HtmlInputElement>() else {
            return;
        };
        let Some(files) = input.files() else { return };
        let Some(file) = files.get(0) else { return };

        busy.set(true);
        error_msg.set(None);
        spawn_local(async move {
            match bridge::upload_notion_archive(&file).await {
                Ok(rep) => {
                    busy.set(false);
                    report.set(Some(rep.clone()));
                    if let Some(cb) = on_migrated {
                        cb.run(rep);
                    }
                }
                Err(err) => {
                    busy.set(false);
                    error_msg.set(Some(err));
                }
            }
        });
    };

    let run_dayone_path = move |_| {
        let zip_path = dayone_path.get_untracked().trim().to_string();
        if zip_path.is_empty() || busy.get_untracked() {
            return;
        }
        let is_dry = dry_run.get_untracked();
        busy.set(true);
        error_msg.set(None);
        spawn_local(async move {
            let req = ImportDayOneRequest {
                zip_path,
                notebook_id: None,
                dry_run: is_dry,
            };
            match bridge::import_dayone(req).await {
                Ok(rep) => {
                    busy.set(false);
                    report.set(Some(rep.clone()));
                    if let Some(cb) = on_migrated {
                        cb.run(rep);
                    }
                }
                Err(err) => {
                    busy.set(false);
                    error_msg.set(Some(err));
                }
            }
        });
    };

    let run_notion_path = move |_| {
        let source_path = notion_path.get_untracked().trim().to_string();
        if source_path.is_empty() || busy.get_untracked() {
            return;
        }
        let is_dry = dry_run.get_untracked();
        busy.set(true);
        error_msg.set(None);
        spawn_local(async move {
            let req = ImportNotionRequest {
                source_path,
                notebook_id: None,
                dry_run: is_dry,
            };
            match bridge::import_notion(req).await {
                Ok(rep) => {
                    busy.set(false);
                    report.set(Some(rep.clone()));
                    if let Some(cb) = on_migrated {
                        cb.run(rep);
                    }
                }
                Err(err) => {
                    busy.set(false);
                    error_msg.set(Some(err));
                }
            }
        });
    };

    view! {
        <Show when=move || open.get()>
            <div class="editorial-modal-backdrop" on:click=move |_| on_close.run(())>
                <div
                    class="editorial-modal-sheet wide"
                    role="dialog"
                    aria-modal="true"
                    aria-label="Archive Migration Studio"
                    on:click=move |ev| ev.stop_propagation()
                >
                    <header class="modal-header">
                        <div class="modal-title-group">
                            <IconArchiveImport class="modal-title-icon" />
                            <div>
                                <span class="small-caps-meta">"Archive Migration Studio"</span>
                                <h2 class="modal-serif-heading">"Day One & Notion Importer"</h2>
                            </div>
                        </div>
                        <button
                            type="button"
                            class="icon-btn subtle"
                            title="Close"
                            on:click=move |_| on_close.run(())
                        >
                            <IconClose />
                        </button>
                    </header>

                    <div class="modal-body">
                        <div class="migration-columns">
                            <section class="migration-panel">
                                <span class="small-caps-meta">"1 · Primary Archive"</span>
                                <h3 class="panel-serif-title">"Day One (.zip / .json)"</h3>
                                <p class="panel-note">
                                    "Preserves original timestamps, weather, GPS locations, starred marks, and embedded Polaroids."
                                </p>

                                <label class="btn-editorial btn-sage upload-btn-label">
                                    <IconUpload />
                                    <span>"Upload Day One .zip"</span>
                                    <input
                                        type="file"
                                        accept=".zip,.json"
                                        class="sr-only"
                                        disabled=move || busy.get()
                                        on:change=handle_dayone_file
                                    />
                                </label>

                                <div class="path-import-row">
                                    <input
                                        type="text"
                                        class="editorial-input"
                                        placeholder="/path/to/DayOne-Export.zip"
                                        prop:value=move || dayone_path.get()
                                        on:input=move |ev| dayone_path.set(event_target_value(&ev))
                                    />
                                    <button
                                        type="button"
                                        class="btn-editorial btn-ghost"
                                        disabled=move || busy.get() || dayone_path.get().trim().is_empty()
                                        on:click=run_dayone_path
                                    >
                                        "Import Path"
                                    </button>
                                </div>
                            </section>

                            <section class="migration-panel">
                                <span class="small-caps-meta">"2 · Delta Reconciliation"</span>
                                <h3 class="panel-serif-title">"Notion Export (.zip / .json)"</h3>
                                <p class="panel-note">
                                    "Day One Primary — Notion duplicates automatically skipped by source flag, date, and lead prose."
                                </p>

                                <label class="btn-editorial btn-terracotta upload-btn-label">
                                    <IconUpload />
                                    <span>"Upload Notion Archive"</span>
                                    <input
                                        type="file"
                                        accept=".zip,.json"
                                        class="sr-only"
                                        disabled=move || busy.get()
                                        on:change=handle_notion_file
                                    />
                                </label>

                                <div class="path-import-row">
                                    <input
                                        type="text"
                                        class="editorial-input"
                                        placeholder="/path/to/notion-journal.json"
                                        prop:value=move || notion_path.get()
                                        on:input=move |ev| notion_path.set(event_target_value(&ev))
                                    />
                                    <button
                                        type="button"
                                        class="btn-editorial btn-ghost"
                                        disabled=move || busy.get() || notion_path.get().trim().is_empty()
                                        on:click=run_notion_path
                                    >
                                        "Run Delta"
                                    </button>
                                </div>
                            </section>
                        </div>

                        <label class="dry-run-toggle">
                            <input
                                type="checkbox"
                                prop:checked=move || dry_run.get()
                                on:change=move |ev| dry_run.set(event_target_checked(&ev))
                            />
                            <span>"Dry-run audit only (preview matches without writing vault files)"</span>
                        </label>

                        {move || busy.get().then(|| view! {
                            <p class="modal-status-banner">"Auditing and importing archive into local vault..."</p>
                        })}

                        {move || error_msg.get().map(|err| view! {
                            <p class="form-error">{err}</p>
                        })}

                        {move || report.get().map(|rep| {
                            view! {
                                <div class="migration-report-box">
                                    <div class="report-kpi-grid">
                                        <div class="report-kpi">
                                            <span class="kpi-num">{rep.total_found}</span>
                                            <span class="small-caps-meta">"Found"</span>
                                        </div>
                                        <div class="report-kpi is-sage">
                                            <span class="kpi-num">{rep.imported_count}</span>
                                            <span class="small-caps-meta">"Imported"</span>
                                        </div>
                                        <div class="report-kpi">
                                            <span class="kpi-num">{rep.skipped_count}</span>
                                            <span class="small-caps-meta">"Skipped"</span>
                                        </div>
                                        <div class="report-kpi is-terracotta">
                                            <span class="kpi-num">{rep.failed_count}</span>
                                            <span class="small-caps-meta">"Failed"</span>
                                        </div>
                                        <div class="report-kpi">
                                            <span class="kpi-num">{rep.media_copied}</span>
                                            <span class="small-caps-meta">"Media"</span>
                                        </div>
                                    </div>

                                    {(!rep.items.is_empty()).then(|| {
                                        view! {
                                            <div class="audit-table-wrap">
                                                <table class="audit-table">
                                                    <thead>
                                                        <tr>
                                                            <th>"Title / Source"</th>
                                                            <th>"Date"</th>
                                                            <th>"Outcome"</th>
                                                            <th>"Note"</th>
                                                        </tr>
                                                    </thead>
                                                    <tbody>
                                                        {rep.items
                                                            .into_iter()
                                                            .map(|item| {
                                                                let (status_cls, status_lbl) = match item.status {
                                                                    MigrationStatus::Imported => ("status-pill is-imported", "Imported"),
                                                                    MigrationStatus::Skipped => ("status-pill is-skipped", "Skipped"),
                                                                    MigrationStatus::NeedsReview => ("status-pill is-review", "Review"),
                                                                    MigrationStatus::Failed => ("status-pill is-failed", "Failed"),
                                                                };
                                                                view! {
                                                                    <tr>
                                                                        <td>{item.title.unwrap_or(item.source_id)}</td>
                                                                        <td class="mono-cell">{item.date.unwrap_or_else(|| "—".to_string())}</td>
                                                                        <td><span class=status_cls>{status_lbl}</span></td>
                                                                        <td class="muted-cell">{item.reason.unwrap_or_default()}</td>
                                                                    </tr>
                                                                }
                                                            })
                                                            .collect_view()}
                                                    </tbody>
                                                </table>
                                            </div>
                                        }
                                    })}
                                </div>
                            }
                        })}
                    </div>
                </div>
            </div>
        </Show>
    }
}

/// Encrypted Sanctuary Backup & Google Drive (`LogJournal`) sync modal.
#[component]
pub fn BackupSyncModal(
    open: Signal<bool>,
    on_close: Callback<()>,
    #[prop(optional)] on_restored: Option<Callback<RestoreBackupResponse>>,
) -> impl IntoView {
    let passphrase = RwSignal::new(String::new());
    let show_pass = RwSignal::new(false);
    let backups = RwSignal::new(Vec::<BackupInfo>::new());
    let busy = RwSignal::new(false);
    let error_msg = RwSignal::new(Option::<String>::None);
    let status_msg = RwSignal::new(Option::<String>::None);
    let restore_target = RwSignal::new(Option::<String>::None);

    let refresh_backups = move || {
        spawn_local(async move {
            if let Ok(list) = bridge::list_backups().await {
                backups.set(list);
            }
        });
    };

    Effect::new(move |_| {
        if open.get() {
            error_msg.set(None);
            refresh_backups();
        }
    });

    let create_snapshot = move |_| {
        let pass = passphrase.get_untracked();
        if pass.trim().is_empty() {
            error_msg.set(Some(
                "Enter an encryption passphrase before creating an .age archive.".to_string(),
            ));
            return;
        }
        busy.set(true);
        error_msg.set(None);
        status_msg.set(None);

        spawn_local(async move {
            match bridge::create_backup(pass, false).await {
                Ok(info) => {
                    busy.set(false);
                    status_msg.set(Some(format!(
                        "Encrypted snapshot {} ({}) created locally.",
                        info.filename,
                        format_bytes(info.size_bytes)
                    )));
                    refresh_backups();
                }
                Err(err) => {
                    busy.set(false);
                    error_msg.set(Some(err));
                }
            }
        });
    };

    let sync_drive = move |_| {
        let pass = passphrase.get_untracked();
        let pass_opt = if pass.trim().is_empty() {
            None
        } else {
            Some(pass)
        };
        busy.set(true);
        error_msg.set(None);
        status_msg.set(None);

        spawn_local(async move {
            let cfg = BackupConfig::default();
            match bridge::sync_gdrive(pass_opt, cfg).await {
                Ok(SyncResult {
                    message,
                    uploaded_remote,
                    ..
                }) => {
                    busy.set(false);
                    let prefix = if uploaded_remote {
                        "Synced to Google Drive (LogJournal): "
                    } else {
                        "Local encrypted vault ready: "
                    };
                    status_msg.set(Some(format!("{prefix}{message}")));
                    refresh_backups();
                }
                Err(err) => {
                    busy.set(false);
                    error_msg.set(Some(err));
                }
            }
        });
    };

    let confirm_restore = move |backup_id: String| {
        let pass = passphrase.get_untracked();
        if pass.trim().is_empty() {
            error_msg.set(Some(
                "Enter the archive's decryption passphrase above to restore.".to_string(),
            ));
            return;
        }
        busy.set(true);
        error_msg.set(None);
        status_msg.set(None);

        spawn_local(async move {
            let req = RestoreBackupRequest {
                backup_id_or_path: backup_id,
                passphrase: pass,
                target_vault_dir: None,
                overwrite_existing: true,
            };
            match bridge::restore_backup(req).await {
                Ok(res) => {
                    busy.set(false);
                    restore_target.set(None);
                    status_msg.set(Some(format!(
                        "Restored {} entries & {} media files — {}",
                        res.entries_restored, res.media_restored, res.message
                    )));
                    if let Some(cb) = on_restored {
                        cb.run(res);
                    }
                }
                Err(err) => {
                    busy.set(false);
                    error_msg.set(Some(err));
                }
            }
        });
    };

    view! {
        <Show when=move || open.get()>
            <div class="editorial-modal-backdrop" on:click=move |_| on_close.run(())>
                <div
                    class="editorial-modal-sheet wide"
                    role="dialog"
                    aria-modal="true"
                    aria-label="Encrypted Sanctuary Backup"
                    on:click=move |ev| ev.stop_propagation()
                >
                    <header class="modal-header">
                        <div class="modal-title-group">
                            <IconCloudLock class="modal-title-icon" />
                            <div>
                                <span class="small-caps-meta">"Zero-Knowledge Sync"</span>
                                <h2 class="modal-serif-heading">"Encrypted Sanctuary Backup & Drive"</h2>
                            </div>
                        </div>
                        <button
                            type="button"
                            class="icon-btn subtle"
                            title="Close"
                            on:click=move |_| on_close.run(())
                        >
                            <IconClose />
                        </button>
                    </header>

                    <div class="modal-body">
                        <div class="backup-passphrase-box">
                            <label class="small-caps-meta" for="backup-passphrase-input">
                                "Client-Side age Encryption Passphrase"
                            </label>
                            <div class="passphrase-input-row">
                                <input
                                    id="backup-passphrase-input"
                                    type=move || if show_pass.get() { "text" } else { "password" }
                                    class="editorial-input"
                                    placeholder="Enter sanctuary passphrase (never transmitted or stored)"
                                    prop:value=move || passphrase.get()
                                    on:input=move |ev| passphrase.set(event_target_value(&ev))
                                />
                                <button
                                    type="button"
                                    class="icon-btn subtle"
                                    title="Toggle passphrase visibility"
                                    on:click=move |_| show_pass.update(|v| *v = !*v)
                                >
                                    <IconEye />
                                </button>
                            </div>
                            <p class="panel-note">
                                "Archives are compressed and encrypted client-side with age before leaving your machine for Google Drive folder LogJournal."
                            </p>

                            <div class="backup-action-row">
                                <button
                                    type="button"
                                    class="btn-editorial btn-sage"
                                    disabled=move || busy.get()
                                    on:click=create_snapshot
                                >
                                    <IconCloudLock />
                                    <span>"Create .age Encrypted Snapshot"</span>
                                </button>

                                <button
                                    type="button"
                                    class="btn-editorial btn-terracotta"
                                    disabled=move || busy.get()
                                    on:click=sync_drive
                                >
                                    <IconRefresh />
                                    <span>"Sync to Google Drive (LogJournal)"</span>
                                </button>
                            </div>
                        </div>

                        {move || status_msg.get().map(|msg| view! {
                            <p class="modal-status-banner is-success">
                                <IconCheck class="tiny-icon" />
                                <span>{msg}</span>
                            </p>
                        })}

                        {move || error_msg.get().map(|err| view! {
                            <p class="form-error">{err}</p>
                        })}

                        <section class="backup-list-section">
                            <div class="backup-list-header">
                                <span class="small-caps-meta">"Verified Snapshots"</span>
                                <button
                                    type="button"
                                    class="link-clear-btn"
                                    on:click=move |_| refresh_backups()
                                >
                                    "Refresh"
                                </button>
                            </div>

                            {move || {
                                let list = backups.get();
                                if list.is_empty() {
                                    return view! {
                                        <p class="muted-empty-note">"No encrypted .age snapshots created yet."</p>
                                    }
                                        .into_any();
                                }

                                view! {
                                    <div class="backup-items-list">
                                        {list
                                            .into_iter()
                                            .map(|b| {
                                                let id_stored = StoredValue::new(b.id.clone());
                                                let short_sha: String = b.sha256.chars().take(12).collect();
                                                let date_str = format_entry_date(&b.created_at);
                                                let size_str = format_bytes(b.size_bytes);
                                                let is_armed = move || {
                                                    restore_target.get().as_deref() == Some(id_stored.get_value().as_str())
                                                };

                                                view! {
                                                    <div class="backup-item-row">
                                                        <div class="backup-item-main">
                                                            <div class="backup-item-title-line">
                                                                <span class="backup-filename">{b.filename}</span>
                                                                {b.encrypted.then(|| view! { <span class="status-pill is-imported">"age"</span> })}
                                                                <span class="status-pill is-skipped">{b.location}</span>
                                                            </div>
                                                            <div class="backup-item-meta">
                                                                <span class="small-caps-meta">{date_str}</span>
                                                                <span>{size_str}</span>
                                                                <span>{format!("{} entries", b.entry_count)}</span>
                                                                <code class="sha-badge">{format!("sha256:{short_sha}")}</code>
                                                            </div>
                                                        </div>

                                                        <div class="backup-item-actions">
                                                            <Show
                                                                when=is_armed
                                                                fallback=move || {
                                                                    view! {
                                                                        <button
                                                                            type="button"
                                                                            class="btn-editorial btn-ghost"
                                                                            disabled=move || busy.get()
                                                                            on:click=move |_| restore_target.set(Some(id_stored.get_value()))
                                                                        >
                                                                            "Restore"
                                                                        </button>
                                                                    }
                                                                }
                                                            >
                                                                <div class="restore-confirm-pair">
                                                                    <button
                                                                        type="button"
                                                                        class="btn-editorial btn-terracotta"
                                                                        disabled=move || busy.get()
                                                                        on:click=move |_| confirm_restore(id_stored.get_value())
                                                                    >
                                                                        "Confirm Restore"
                                                                    </button>
                                                                    <button
                                                                        type="button"
                                                                        class="btn-editorial btn-ghost"
                                                                        on:click=move |_| restore_target.set(None)
                                                                    >
                                                                        "Cancel"
                                                                    </button>
                                                                </div>
                                                            </Show>
                                                        </div>
                                                    </div>
                                                }
                                            })
                                            .collect_view()}
                                    </div>
                                }
                                    .into_any()
                            }}
                        </section>
                    </div>
                </div>
            </div>
        </Show>
    }
}
