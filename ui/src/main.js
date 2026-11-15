import { api } from "./ipc.js";
import { icon } from "./icons.js";
import { initPaperShader } from "./shader.js";
import { renderSidebar } from "./sidebar.js";
import { renderEntryEditor } from "./editor.js";
import {
  renderTimelineView,
  renderCalendarView,
  renderOnThisDayView,
  renderSearchModal,
  renderMigrationModal,
  renderBackupModal,
} from "./views.js";

const now = new Date();

const state = {
  nightMode: localStorage.getItem("log_theme") === "night",
  sidebarCollapsed: localStorage.getItem("log_sidebar_collapsed") === "true",
  activeView: "editor", // "editor" | "timeline" | "calendar" | "on_this_day"
  editorMode: "write", // "write" | "read"
  pageTurnDir: null,

  stats: null,
  notebooks: [],
  entries: [],
  filteredEntries: [],
  tags: [],
  manifest: { covers: [], stamps: [], stickers: [], washi: [] },
  templates: [],
  dailyContext: null,
  onThisDayGroups: [],
  onThisDayCount: 0,

  selectedNotebookId: null,
  selectedTag: null,
  currentEntry: null,

  calendarYear: now.getFullYear(),
  calendarMonth: now.getMonth() + 1,
  calendarDays: [],

  activeModal: null, // null | "search" | "migration" | "backup"
  searchQuery: "",
  searchResults: [],
  searchLoading: false,

  migrationLoading: false,
  migrationError: null,
  migrationReport: null,

  backupLoading: false,
  backupError: null,
  syncResult: null,
  backups: [],
  backupConfig: {
    destination_dir: null,
    gdrive_folder: "LogJournal",
    rclone_remote: null,
    auto_sync_enabled: false,
    keep_last_n: 3,
    has_passphrase_hint: null,
  },
  gdriveStatus: null,
  gdriveConnecting: false,
};

let shellEl = null;
let sidebarRoot = null;
let mainScrollEl = null;
let reopenBtnEl = null;
let viewRoot = null;
let modalRoot = null;
let paperShader = null;
let activeEditorHandle = null;
let searchDebounceTimer = null;
let entryLoadSeq = 0;
let calendarLoadSeq = 0;
let searchSeq = 0;
let refreshSeq = 0;
let statsDebounceTimer = null;

function applyTheme(isNight) {
  state.nightMode = Boolean(isNight);
  localStorage.setItem("log_theme", state.nightMode ? "night" : "day");
  document.documentElement.setAttribute(
    "data-theme",
    state.nightMode ? "night" : "day"
  );
  document.body.classList.toggle("night", state.nightMode);
  paperShader?.setNightMode(state.nightMode);
}

function applySidebarCollapsed(collapsed) {
  state.sidebarCollapsed = Boolean(collapsed);
  localStorage.setItem(
    "log_sidebar_collapsed",
    state.sidebarCollapsed ? "true" : "false"
  );
  shellEl?.classList.toggle("is-sidebar-collapsed", state.sidebarCollapsed);
  if (reopenBtnEl) {
    reopenBtnEl.hidden = !state.sidebarCollapsed;
  }
}

function resetMainScroll() {
  if (mainScrollEl) mainScrollEl.scrollTop = 0;
  if (viewRoot) viewRoot.scrollTop = 0;
  document.documentElement.scrollTop = 0;
  document.body.scrollTop = 0;
  window.scrollTo(0, 0);
}

function recomputeFilteredEntries() {
  state.filteredEntries = (state.entries || []).filter((e) => {
    if (
      state.selectedNotebookId &&
      e.notebook_id !== state.selectedNotebookId
    ) {
      return false;
    }
    if (
      state.selectedTag &&
      !(e.tags || []).includes(state.selectedTag)
    ) {
      return false;
    }
    return true;
  });
}

function createBlankEntry(dateStr = null) {
  const iso = dateStr
    ? new Date(`${dateStr}T12:00:00`).toISOString()
    : new Date().toISOString();
  const nbId =
    state.selectedNotebookId || state.notebooks?.[0]?.id || "daily";
  return {
    id: null,
    frontmatter: {
      id: null,
      title: "",
      date: iso,
      updated_at: iso,
      notebook_id: nbId,
      tags: state.selectedTag ? [state.selectedTag] : [],
      mood: null,
      starred: false,
      source: "native",
      source_id: null,
      weather: null,
      location: null,
      stamps: [],
      photos: [],
      audio: [],
      links: [],
    },
    body: "",
    html_preview: "",
    path: "",
    word_count: 0,
    reading_minutes: 0,
    backlinks: [],
  };
}

function upsertEntrySummaryFromSaved(saved) {
  if (!saved || !saved.id) return;
  const fm = saved.frontmatter || {};
  const plainSnippet = String(saved.body || "")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 180);
  const summary = {
    id: saved.id,
    title: fm.title || "",
    date: fm.date,
    updated_at: fm.updated_at || fm.date,
    notebook_id: fm.notebook_id,
    tags: fm.tags || [],
    mood: fm.mood || null,
    starred: Boolean(fm.starred),
    snippet: plainSnippet,
    word_count: saved.word_count || 0,
    reading_minutes: saved.reading_minutes || 1,
    path: saved.path || "",
    cover_photo: fm.photos?.[0]?.thumbnail_url || fm.photos?.[0]?.url || null,
    has_audio: (fm.audio || []).length > 0,
    has_photos: (fm.photos || []).length > 0,
    weather: fm.weather || null,
    location: fm.location || null,
    stamps: fm.stamps || [],
  };

  const idx = state.entries.findIndex((e) => e.id === saved.id);
  const prevWords = idx >= 0 ? Number(state.entries[idx].word_count || 0) : 0;
  if (idx >= 0) {
    state.entries[idx] = summary;
  } else {
    state.entries.unshift(summary);
  }
  if (state.stats) {
    const delta = (summary.word_count || 0) - prevWords;
    state.stats.total_words = Math.max(0, (state.stats.total_words || 0) + delta);
    if (idx < 0) {
      state.stats.total_entries = (state.stats.total_entries || 0) + 1;
    }
  }
  recomputeFilteredEntries();
  renderSidebarOnly();

  if (statsDebounceTimer) clearTimeout(statsDebounceTimer);
  statsDebounceTimer = setTimeout(() => {
    statsDebounceTimer = null;
    api
      .getVaultStats()
      .then((s) => {
        if (s) {
          state.stats = s;
          renderSidebarOnly();
        }
      })
      .catch(() => {});
  }, 4000);
}

async function refreshVaultIndex() {
  const seq = ++refreshSeq;
  const [statsRes, nbRes, entriesRes, tagsRes, otdRes] =
    await Promise.allSettled([
      api.getVaultStats(),
      api.listNotebooks(),
      api.listEntries({ limit: 1000, offset: 0 }),
      api.listTags(),
      api.getOnThisDay(),
    ]);

  if (seq !== refreshSeq) return;

  if (statsRes.status === "fulfilled" && statsRes.value) {
    state.stats = statsRes.value;
  }
  if (nbRes.status === "fulfilled" && Array.isArray(nbRes.value)) {
    state.notebooks = nbRes.value;
  }
  if (entriesRes.status === "fulfilled" && Array.isArray(entriesRes.value)) {
    state.entries = entriesRes.value;
  }
  if (tagsRes.status === "fulfilled" && Array.isArray(tagsRes.value)) {
    state.tags = tagsRes.value;
  }
  if (otdRes.status === "fulfilled" && Array.isArray(otdRes.value)) {
    state.onThisDayGroups = otdRes.value;
    state.onThisDayCount = otdRes.value.reduce(
      (acc, g) => acc + (g.entries?.length || 0),
      0
    );
  }
  recomputeFilteredEntries();
  renderSidebarOnly();
}

async function loadCalendarMonth(year, month) {
  const seq = ++calendarLoadSeq;
  state.calendarYear = year;
  state.calendarMonth = month;
  try {
    const days = await api.getCalendarMonth(
      year,
      month,
      state.selectedNotebookId
    );
    if (seq !== calendarLoadSeq) return;
    state.calendarDays = days;
  } catch {
    if (seq !== calendarLoadSeq) return;
    state.calendarDays = [];
  }
}

async function openEntryById(id, turnDir = null) {
  if (!id) return;
  const seq = ++entryLoadSeq;
  if (state.currentEntry?.id !== id) {
    await activeEditorHandle?.flushSave();
    if (seq !== entryLoadSeq) return;
  }
  try {
    const entry = await api.getEntry(id);
    if (seq !== entryLoadSeq) return;
    state.currentEntry = entry;
    state.activeView = "editor";
    state.pageTurnDir = turnDir;
    renderSidebarOnly();
    renderMainView();
    state.pageTurnDir = null;
  } catch (err) {
    console.error("Failed to open entry:", err);
  }
}

async function startNewEntry(dateStr = null) {
  await activeEditorHandle?.flushSave();
  state.currentEntry = createBlankEntry(dateStr);
  state.activeView = "editor";
  state.editorMode = "write";
  state.pageTurnDir = null;
  renderSidebarOnly();
  renderMainView();
  activeEditorHandle?.focusEditor();
}

async function navigateToView(viewKey) {
  if (viewKey !== "editor") {
    await activeEditorHandle?.flushSave();
  }
  state.activeView = viewKey;
  if (viewKey === "timeline") {
    state.selectedNotebookId = null;
    state.selectedTag = null;
    recomputeFilteredEntries();
  }
  if (viewKey === "calendar") {
    await loadCalendarMonth(state.calendarYear, state.calendarMonth);
  } else if (viewKey === "on_this_day") {
    try {
      state.onThisDayGroups = await api.getOnThisDay();
      state.onThisDayCount = state.onThisDayGroups.reduce(
        (acc, g) => acc + (g.entries?.length || 0),
        0
      );
    } catch {
      // keep cached
    }
  }
  renderSidebarOnly();
  renderMainView();
}

async function handleUploadNotebookCover(file) {
  const photo = await api.uploadPhoto(file, "Notebook cover");
  return photo.thumbnail_url || photo.url;
}

async function handleUpdateNotebook(req) {
  await api.updateNotebook(req);
  await refreshVaultIndex();
  renderSidebarOnly();
  if (state.activeView === "timeline" || state.activeView === "editor") {
    renderMainView();
  }
}

async function handleDeleteNotebook(nbId, deleteEntries = false) {
  const editingTargetNb = state.currentEntry?.frontmatter?.notebook_id === nbId;
  if (state.activeView === "editor" && activeEditorHandle) {
    if (deleteEntries && editingTargetNb) {
      activeEditorHandle.destroy?.();
      activeEditorHandle = null;
    } else {
      await activeEditorHandle.flushSave?.();
    }
  }

  const deletedCurrentId =
    deleteEntries && editingTargetNb ? state.currentEntry?.id : null;
  const currentIdBefore = state.currentEntry?.id || null;

  const res = await api.deleteNotebook(nbId, deleteEntries);
  if (state.selectedNotebookId === nbId) {
    state.selectedNotebookId = null;
  }
  await refreshVaultIndex();

  if (editingTargetNb) {
    if (deleteEntries || (deletedCurrentId && currentIdBefore === deletedCurrentId)) {
      const next = state.filteredEntries[0] || state.entries[0] || null;
      if (next) {
        try {
          state.currentEntry = await api.getEntry(next.id);
        } catch {
          state.currentEntry = createBlankEntry();
        }
      } else {
        state.currentEntry = createBlankEntry();
      }
    } else if (currentIdBefore) {
      try {
        state.currentEntry = await api.getEntry(currentIdBefore);
      } catch {
        state.currentEntry = createBlankEntry();
      }
    } else if (state.currentEntry?.frontmatter) {
      state.currentEntry.frontmatter.notebook_id =
        res?.reassigned_to || state.notebooks?.[0]?.id || "daily";
    }
  }

  if (state.activeView === "calendar") {
    await loadCalendarMonth(state.calendarYear, state.calendarMonth);
  }
  renderSidebarOnly();
  renderMainView();
}

async function handleDeleteEntry(id) {
  if (!id) return;
  if (state.currentEntry?.id === id && activeEditorHandle) {
    activeEditorHandle.destroy?.();
    activeEditorHandle = null;
  }
  await api.deleteEntry(id);
  await refreshVaultIndex();
  if (state.currentEntry?.id === id) {
    const next = state.filteredEntries[0] || state.entries[0] || null;
    if (next) {
      try {
        state.currentEntry = await api.getEntry(next.id);
      } catch {
        state.currentEntry = createBlankEntry();
      }
    } else {
      state.currentEntry = createBlankEntry();
    }
  }
  if (state.activeView === "calendar") {
    await loadCalendarMonth(state.calendarYear, state.calendarMonth);
  }
  renderSidebarOnly();
  renderMainView();
}

function getSidebarCallbacks() {
  return {
    onToggleSidebar: () => applySidebarCollapsed(!state.sidebarCollapsed),
    onNewEntry: () => startNewEntry(),
    onOpenSearch: () => openModal("search"),
    onNavigate: (viewKey) => navigateToView(viewKey),
    onCreateNotebook: async (req) => {
      const nb = await api.createNotebook(req);
      await refreshVaultIndex();
      state.selectedNotebookId = nb.id;
      state.selectedTag = null;
      state.activeView = "timeline";
      recomputeFilteredEntries();
      renderSidebarOnly();
      renderMainView();
    },
    onUpdateNotebook: handleUpdateNotebook,
    onDeleteNotebook: handleDeleteNotebook,
    onUploadNotebookCover: handleUploadNotebookCover,
    onSelectNotebook: async (nbId) => {
      if (state.activeView === "editor") {
        await activeEditorHandle?.flushSave();
      }
      state.selectedNotebookId = nbId;
      state.selectedTag = null;
      state.activeView = "timeline";
      recomputeFilteredEntries();
      renderSidebarOnly();
      renderMainView();
    },
    onSelectTag: async (tag) => {
      if (state.activeView === "editor") {
        await activeEditorHandle?.flushSave();
      }
      state.selectedTag = tag;
      state.activeView = "timeline";
      recomputeFilteredEntries();
      renderSidebarOnly();
      renderMainView();
    },
    onOpenMigration: () => openModal("migration"),
    onOpenBackup: () => openModal("backup"),
    onToggleTheme: () => {
      applyTheme(!state.nightMode);
      renderSidebarOnly();
    },
  };
}

function renderSidebarOnly() {
  if (!sidebarRoot) return;
  renderSidebar(sidebarRoot, state, getSidebarCallbacks());
}

function renderMainView() {
  if (!viewRoot) return;
  if (activeEditorHandle) {
    activeEditorHandle.destroy?.();
    activeEditorHandle = null;
  }
  resetMainScroll();

  if (state.activeView === "editor") {
    if (!state.currentEntry) {
      state.currentEntry = createBlankEntry();
    }
    activeEditorHandle = renderEntryEditor(viewRoot, state, {
      onNewEntry: () => startNewEntry(),
      onOpenEntry: (id, dir = null) => openEntryById(id, dir),
      onNavigate: (viewKey) => navigateToView(viewKey),
      onModeChange: (m) => {
        state.editorMode = m;
      },
      onSearchText: (query) => {
        state.searchQuery = query;
        openModal("search");
      },
      onSaveEntry: async (payload, meta = {}) => {
        const saved = await api.saveEntry(payload);
        if (
          !state.currentEntry ||
          state.currentEntry.id === null ||
          state.currentEntry.id === saved.id
        ) {
          state.currentEntry = saved;
        }
        if (meta?.structural) {
          refreshVaultIndex().catch(() => {});
        } else {
          upsertEntrySummaryFromSaved(saved);
        }
        return saved;
      },
      onDeleteEntry: (id) => handleDeleteEntry(id),
    });
    return;
  }

  if (state.activeView === "timeline") {
    renderTimelineView(viewRoot, state, {
      onOpenEntry: (id) => openEntryById(id),
      onSelectTag: (tag) => {
        state.selectedTag = tag;
        recomputeFilteredEntries();
        renderSidebarOnly();
        renderMainView();
      },
      onClearFilters: () => navigateToView("timeline"),
      onNewEntry: () => startNewEntry(),
      onUpdateNotebook: handleUpdateNotebook,
      onDeleteNotebook: handleDeleteNotebook,
      onUploadNotebookCover: handleUploadNotebookCover,
      onDeleteEntry: (id) => handleDeleteEntry(id),
    });
    return;
  }

  if (state.activeView === "calendar") {
    renderCalendarView(viewRoot, state, {
      onPrevMonth: async () => {
        let y = state.calendarYear;
        let m = state.calendarMonth - 1;
        if (m < 1) {
          m = 12;
          y -= 1;
        }
        await loadCalendarMonth(y, m);
        renderMainView();
      },
      onNextMonth: async () => {
        let y = state.calendarYear;
        let m = state.calendarMonth + 1;
        if (m > 12) {
          m = 1;
          y += 1;
        }
        await loadCalendarMonth(y, m);
        renderMainView();
      },
      onSelectMonth: async (y, m) => {
        await loadCalendarMonth(y, m);
        renderMainView();
      },
      onSelectDate: (dateStr, summary) => {
        if (summary?.primary_entry_id) {
          openEntryById(summary.primary_entry_id);
        } else {
          startNewEntry(dateStr);
        }
      },
      onOpenEntry: (id) => openEntryById(id),
      onNewEntryForDate: (dateStr) => startNewEntry(dateStr),
      onDeleteEntry: (id) => handleDeleteEntry(id),
    });
    return;
  }

  if (state.activeView === "on_this_day") {
    renderOnThisDayView(viewRoot, state, {
      onOpenEntry: (id) => openEntryById(id),
      onDeleteEntry: (id) => handleDeleteEntry(id),
    });
  }
}

async function runSearchQuery(text) {
  const seq = ++searchSeq;
  state.searchQuery = text;
  const trimmed = text.trim();
  if (!trimmed) {
    state.searchResults = [];
    state.searchLoading = false;
    renderModal();
    return;
  }
  state.searchLoading = true;
  renderModal();
  try {
    const hits = await api.searchEntries({
      text: trimmed,
      limit: 40,
      offset: 0,
    });
    if (seq !== searchSeq) return;
    state.searchResults = hits;
  } catch {
    if (seq !== searchSeq) return;
    state.searchResults = [];
  } finally {
    if (seq === searchSeq) {
      state.searchLoading = false;
      if (state.activeModal === "search") {
        renderModal();
      }
    }
  }
}

async function openModal(modalName) {
  state.activeModal = modalName;
  if (modalName === "search") {
    renderModal();
    if (state.searchQuery.trim()) {
      runSearchQuery(state.searchQuery);
    }
    return;
  }
  if (modalName === "migration") {
    state.migrationError = null;
    renderModal();
    return;
  }
  if (modalName === "backup") {
    state.backupError = null;
    state.syncResult = null;
    state.backupLoading = true;
    renderModal();
    try {
      const [backupsRes, cfgRes, statusRes] = await Promise.allSettled([
        api.listBackups(),
        api.getBackupConfig(),
        api.getGdriveStatus(),
      ]);
      if (backupsRes.status === "fulfilled" && Array.isArray(backupsRes.value)) {
        state.backups = backupsRes.value;
      } else if (backupsRes.status === "rejected") {
        state.backupError =
          backupsRes.reason?.message || "Failed to list backups.";
      }
      if (cfgRes.status === "fulfilled" && cfgRes.value) {
        state.backupConfig = {
          ...state.backupConfig,
          ...cfgRes.value,
        };
      }
      if (statusRes.status === "fulfilled" && statusRes.value) {
        state.gdriveStatus = statusRes.value;
      }
    } finally {
      state.backupLoading = false;
      if (state.activeModal === "backup") renderModal();
    }
  }
}

function closeModal() {
  state.activeModal = null;
  if (modalRoot) modalRoot.innerHTML = "";
}

function renderModal() {
  if (!modalRoot) return;
  if (!state.activeModal) {
    modalRoot.innerHTML = "";
    return;
  }

  if (state.activeModal === "search") {
    renderSearchModal(modalRoot, state, {
      onQueryChange: (q) => {
        state.searchQuery = q;
        if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
        searchDebounceTimer = setTimeout(() => {
          runSearchQuery(q);
        }, 160);
      },
      onSelectEntry: (id) => openEntryById(id),
      onClose: closeModal,
    });
    return;
  }

  if (state.activeModal === "migration") {
    renderMigrationModal(modalRoot, state, {
      onImportDayOnePath: async (zipPath, dryRun) => {
        state.migrationLoading = true;
        state.migrationError = null;
        renderModal();
        try {
          state.migrationReport = await api.importDayone({
            zip_path: zipPath,
            notebook_id: state.selectedNotebookId,
            dry_run: dryRun,
          });
          if (!dryRun) {
            await refreshVaultIndex();
            if (state.activeView === "timeline") renderMainView();
          }
        } catch (err) {
          state.migrationError = err?.message || "Day One import failed.";
        } finally {
          state.migrationLoading = false;
          if (state.activeModal === "migration") renderModal();
        }
      },
      onImportNotionPath: async (sourcePath, dryRun) => {
        state.migrationLoading = true;
        state.migrationError = null;
        renderModal();
        try {
          state.migrationReport = await api.importNotion({
            source_path: sourcePath,
            notebook_id: state.selectedNotebookId,
            dry_run: dryRun,
          });
          if (!dryRun) {
            await refreshVaultIndex();
            if (state.activeView === "timeline") renderMainView();
          }
        } catch (err) {
          state.migrationError = err?.message || "Notion import failed.";
        } finally {
          state.migrationLoading = false;
          if (state.activeModal === "migration") renderModal();
        }
      },
      onImportDayOneFile: async (file, dryRun = false) => {
        if (file?.path) {
          state.migrationLoading = true;
          state.migrationError = null;
          renderModal();
          try {
            state.migrationReport = await api.importDayone({
              zip_path: file.path,
              notebook_id: state.selectedNotebookId,
              dry_run: Boolean(dryRun),
            });
            if (!dryRun) {
              await refreshVaultIndex();
              if (state.activeView === "timeline") renderMainView();
            }
          } catch (err) {
            state.migrationError = err?.message || "Import failed.";
          } finally {
            state.migrationLoading = false;
            if (state.activeModal === "migration") renderModal();
          }
          return;
        }
        state.migrationError =
          "Enter the full local path to your Day One .zip or folder below and click Import Path.";
        renderModal();
      },
      onImportNotionFile: async (file, dryRun = false) => {
        if (file?.path) {
          state.migrationLoading = true;
          state.migrationError = null;
          renderModal();
          try {
            state.migrationReport = await api.importNotion({
              source_path: file.path,
              notebook_id: state.selectedNotebookId,
              dry_run: Boolean(dryRun),
            });
            if (!dryRun) {
              await refreshVaultIndex();
              if (state.activeView === "timeline") renderMainView();
            }
          } catch (err) {
            state.migrationError = err?.message || "Import failed.";
          } finally {
            state.migrationLoading = false;
            if (state.activeModal === "migration") renderModal();
          }
          return;
        }
        state.migrationError =
          "Enter the full local path to your Notion export .zip or folder below and click Import Path.";
        renderModal();
      },
      onClose: closeModal,
    });
    return;
  }

  if (state.activeModal === "backup") {
    const runSync = async ({ passphrase = null, backup_id = null } = {}) => {
      state.backupLoading = true;
      state.backupError = null;
      state.syncResult = null;
      renderModal();
      try {
        const res = await api.syncGdrive({
          passphrase: passphrase || null,
          backup_id: backup_id || null,
          config: state.backupConfig,
        });
        state.syncResult = res;
        state.backups = await api.listBackups();
      } catch (err) {
        state.backupError = err?.message || "Google Drive sync failed.";
      } finally {
        state.backupLoading = false;
        if (state.activeModal === "backup") renderModal();
      }
    };

    const runGdriveConnect = async (action, fallbackErr) => {
      state.gdriveConnecting = true;
      state.backupError = null;
      state.syncResult = null;
      renderModal();
      try {
        await action();
      } catch (err) {
        state.backupError = err?.message || fallbackErr;
      } finally {
        state.gdriveConnecting = false;
        if (state.activeModal === "backup") renderModal();
      }
    };

    renderBackupModal(modalRoot, state, {
      onCreateBackup: async (passphrase, uploadGdrive) => {
        if (!passphrase) {
          state.backupError = "Enter an encryption passphrase first.";
          renderModal();
          return;
        }
        state.backupLoading = true;
        state.backupError = null;
        state.syncResult = null;
        renderModal();
        try {
          const info = await api.createBackup(passphrase, uploadGdrive);
          state.backups = await api.listBackups();
          state.syncResult = {
            success: true,
            message: `Encrypted snapshot created: ${info.filename}`,
          };
        } catch (err) {
          state.backupError = err?.message || "Backup creation failed.";
        } finally {
          state.backupLoading = false;
          if (state.activeModal === "backup") renderModal();
        }
      },
      onSyncGdrive: (passphrase) =>
        runSync({ passphrase: passphrase || null, backup_id: null }),
      onSyncBackupById: (backupId) =>
        runSync({ passphrase: null, backup_id: backupId }),
      onSaveBackupConfig: async (nextCfg) => {
        state.backupLoading = true;
        state.backupError = null;
        state.syncResult = null;
        renderModal();
        try {
          const saved = await api.saveBackupConfig(nextCfg);
          if (saved) state.backupConfig = saved;
          state.gdriveStatus = await api.getGdriveStatus(state.backupConfig);
          state.syncResult = {
            success: true,
            message: "Backup preferences saved.",
          };
        } catch (err) {
          state.backupError = err?.message || "Failed to save preferences.";
        } finally {
          state.backupLoading = false;
          if (state.activeModal === "backup") renderModal();
        }
      },
      onConnectRclone: (remoteName) =>
        runGdriveConnect(async () => {
          state.gdriveStatus = await api.connectGdriveRclone(remoteName);
          const cfg = await api.getBackupConfig();
          if (cfg) state.backupConfig = cfg;
          state.syncResult = {
            success: true,
            message:
              state.gdriveStatus?.detail ||
              "Connected Google Drive via rclone.",
          };
        }, "rclone Google Drive connection failed."),
      onConnectOauth: ({ client_id, client_secret }) =>
        runGdriveConnect(async () => {
          state.gdriveStatus = await api.connectGdriveOauth({
            client_id,
            client_secret,
          });
          state.syncResult = {
            success: true,
            message:
              state.gdriveStatus?.detail ||
              "Connected Google Drive via Google OAuth.",
          };
        }, "Google OAuth sign-in failed."),
      onSaveToken: (token) =>
        runGdriveConnect(async () => {
          state.gdriveStatus = await api.saveGdriveToken(token);
          state.syncResult = {
            success: true,
            message:
              state.gdriveStatus?.detail ||
              "Google Drive access token verified and saved.",
          };
        }, "Failed to save Google Drive token."),
      onDisconnectGdrive: () =>
        runGdriveConnect(async () => {
          state.gdriveStatus = await api.disconnectGdrive();
        }, "Failed to disconnect Google Drive."),
      onRestoreBackup: async (backupId, passphrase) => {
        if (!passphrase) {
          state.backupError = "Enter the backup passphrase to restore.";
          renderModal();
          return;
        }
        state.backupLoading = true;
        state.backupError = null;
        renderModal();
        try {
          const res = await api.restoreBackup({
            backup_id_or_path: backupId,
            passphrase,
            overwrite_existing: false,
          });
          state.syncResult = {
            success: true,
            message:
              res.message ||
              `Restored ${res.entries_restored ?? 0} entries and ${res.media_restored ?? 0} media files to ${res.target_vault_dir}.`,
          };
          await refreshVaultIndex();
        } catch (err) {
          state.backupError = err?.message || "Restore failed.";
        } finally {
          state.backupLoading = false;
          if (state.activeModal === "backup") renderModal();
        }
      },
      onRefresh: async () => {
        state.backupLoading = true;
        state.backupError = null;
        renderModal();
        try {
          const [backupsRes, statusRes] = await Promise.allSettled([
            api.listBackups(),
            api.getGdriveStatus(state.backupConfig),
          ]);
          if (backupsRes.status === "fulfilled" && Array.isArray(backupsRes.value)) {
            state.backups = backupsRes.value;
          } else if (backupsRes.status === "rejected") {
            state.backupError =
              backupsRes.reason?.message || "Failed to refresh backups.";
          }
          if (statusRes.status === "fulfilled" && statusRes.value) {
            state.gdriveStatus = statusRes.value;
          }
        } finally {
          state.backupLoading = false;
          if (state.activeModal === "backup") renderModal();
        }
      },
      onClose: closeModal,
    });
  }
}

function mountShell() {
  const app = document.getElementById("app");
  if (!app) return;
  app.innerHTML = "";

  const root = document.createElement("div");
  root.className = "log-root";

  shellEl = document.createElement("div");
  shellEl.className = `sanctuary-shell${
    state.sidebarCollapsed ? " is-sidebar-collapsed" : ""
  }`;

  sidebarRoot = document.createElement("aside");
  sidebarRoot.id = "sidebar-root";

  mainScrollEl = document.createElement("main");
  mainScrollEl.className = "sanctuary-main";

  reopenBtnEl = document.createElement("button");
  reopenBtnEl.type = "button";
  reopenBtnEl.className = "sidebar-reopen-btn";
  reopenBtnEl.title = "Open sidebar (Ctrl+\\)";
  reopenBtnEl.hidden = !state.sidebarCollapsed;
  const iconWrap = document.createElement("span");
  iconWrap.style.display = "inline-flex";
  iconWrap.innerHTML = icon("sidebar", "tiny-icon");
  reopenBtnEl.appendChild(iconWrap.firstElementChild || iconWrap);
  reopenBtnEl.appendChild(document.createTextNode("Sidebar"));
  reopenBtnEl.addEventListener("click", () => applySidebarCollapsed(false));

  viewRoot = document.createElement("div");
  viewRoot.id = "view-root";

  mainScrollEl.append(reopenBtnEl, viewRoot);
  shellEl.append(sidebarRoot, mainScrollEl);

  modalRoot = document.createElement("div");
  modalRoot.id = "modal-root";

  root.append(shellEl, modalRoot);
  app.appendChild(root);
}

function wireGlobalShortcuts() {
  window.addEventListener("keydown", (ev) => {
    const mod = ev.ctrlKey || ev.metaKey;
    if (mod && ev.key.toLowerCase() === "k") {
      ev.preventDefault();
      if (state.activeModal === "search") {
        closeModal();
      } else {
        openModal("search");
      }
      return;
    }
    if (mod && ev.key === "\\") {
      ev.preventDefault();
      applySidebarCollapsed(!state.sidebarCollapsed);
      return;
    }
    if (
      mod &&
      ev.key.toLowerCase() === "e" &&
      state.activeView === "editor" &&
      !state.activeModal
    ) {
      ev.preventDefault();
      activeEditorHandle?.toggleMode();
    }
  });
}

async function bootstrap() {
  applyTheme(state.nightMode);
  mountShell();
  applySidebarCollapsed(state.sidebarCollapsed);
  wireGlobalShortcuts();

  const canvas = document.getElementById("paper-canvas");
  if (canvas) {
    paperShader = initPaperShader(canvas, state.nightMode);
  }

  renderSidebarOnly();
  renderMainView();

  const [
    statsRes,
    nbRes,
    entriesRes,
    tagsRes,
    manifestRes,
    tplRes,
    dcRes,
    otdRes,
  ] = await Promise.allSettled([
    api.getVaultStats(),
    api.listNotebooks(),
    api.listEntries({ limit: 1000, offset: 0 }),
    api.listTags(),
    api.getAssetManifest(),
    api.listTemplates(),
    api.getDailyContext(),
    api.getOnThisDay(),
  ]);

  if (statsRes.status === "fulfilled" && statsRes.value) {
    state.stats = statsRes.value;
  }
  if (nbRes.status === "fulfilled" && Array.isArray(nbRes.value)) {
    state.notebooks = nbRes.value;
  }
  if (entriesRes.status === "fulfilled" && Array.isArray(entriesRes.value)) {
    state.entries = entriesRes.value;
  }
  if (tagsRes.status === "fulfilled" && Array.isArray(tagsRes.value)) {
    state.tags = tagsRes.value;
  }
  if (manifestRes.status === "fulfilled" && manifestRes.value) {
    state.manifest = manifestRes.value;
  } else {
    try {
      const resp = await fetch("/assets/manifest.json");
      if (resp.ok) state.manifest = await resp.json();
    } catch {
      // ignore
    }
  }
  if (tplRes.status === "fulfilled" && Array.isArray(tplRes.value)) {
    state.templates = tplRes.value;
  }
  if (dcRes.status === "fulfilled" && dcRes.value) {
    state.dailyContext = dcRes.value;
  }
  if (otdRes.status === "fulfilled" && Array.isArray(otdRes.value)) {
    state.onThisDayGroups = otdRes.value;
    state.onThisDayCount = otdRes.value.reduce(
      (acc, g) => acc + (g.entries?.length || 0),
      0
    );
  }

  recomputeFilteredEntries();

  const userStartedTyping =
    Boolean(activeEditorHandle?.isDirty?.()) ||
    Boolean(state.currentEntry?.id);

  if (!userStartedTyping && state.entries.length > 0 && state.activeView === "editor") {
    try {
      const loaded = await api.getEntry(state.entries[0].id);
      if (!activeEditorHandle?.isDirty?.()) {
        state.currentEntry = loaded;
        renderSidebarOnly();
        renderMainView();
        return;
      }
    } catch {
      // keep existing blank draft
    }
  }

  renderSidebarOnly();
  if (!userStartedTyping) {
    renderMainView();
  }
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", bootstrap, { once: true });
} else {
  bootstrap();
}
