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
    gdrive_folder: "LogJournal",
    include_media: true,
    keep_local_snapshots: 7,
  },
};

let shellEl = null;
let sidebarRoot = null;
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
      recomputeFilteredEntries();
      renderSidebarOnly();
      if (state.activeView !== "editor") {
        renderMainView();
      }
    },
    onSelectNotebook: async (nbId) => {
      state.selectedNotebookId = nbId;
      recomputeFilteredEntries();
      renderSidebarOnly();
      if (state.activeView === "calendar") {
        await loadCalendarMonth(state.calendarYear, state.calendarMonth);
        renderMainView();
      } else if (state.activeView === "timeline") {
        renderMainView();
      }
    },
    onSelectTag: (tag) => {
      state.selectedTag = tag;
      recomputeFilteredEntries();
      renderSidebarOnly();
      if (state.activeView === "timeline") {
        renderMainView();
      }
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
      onDeleteEntry: async (id) => {
        await api.deleteEntry(id);
        await refreshVaultIndex();
        const next = state.filteredEntries[0] || state.entries[0] || null;
        if (next) {
          state.currentEntry = await api.getEntry(next.id);
        } else {
          state.currentEntry = createBlankEntry();
        }
        renderSidebarOnly();
        renderMainView();
      },
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
      onNewEntry: () => startNewEntry(),
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
    });
    return;
  }

  if (state.activeView === "on_this_day") {
    renderOnThisDayView(viewRoot, state, {
      onOpenEntry: (id) => openEntryById(id),
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
      state.backups = await api.listBackups();
    } catch (err) {
      state.backupError = err?.message || "Failed to list backups.";
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
      onSyncGdrive: async (passphrase) => {
        state.backupLoading = true;
        state.backupError = null;
        state.syncResult = null;
        renderModal();
        try {
          const res = await api.syncGdrive(passphrase || null, state.backupConfig);
          state.syncResult = res;
          state.backups = await api.listBackups();
        } catch (err) {
          state.backupError = err?.message || "Google Drive sync failed.";
        } finally {
          state.backupLoading = false;
          if (state.activeModal === "backup") renderModal();
        }
      },
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
            message: `Restored ${res.restored_files} files to ${res.target_dir}.`,
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
          state.backups = await api.listBackups();
        } catch (err) {
          state.backupError = err?.message || "Failed to refresh backups.";
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

  const mainEl = document.createElement("main");
  mainEl.className = "sanctuary-main";

  const reopenBtn = document.createElement("button");
  reopenBtn.type = "button";
  reopenBtn.className = "sidebar-reopen-btn";
  reopenBtn.title = "Open sidebar (Ctrl+\\)";
  const iconWrap = document.createElement("span");
  iconWrap.style.display = "inline-flex";
  iconWrap.innerHTML = icon("sidebar", "tiny-icon");
  reopenBtn.appendChild(iconWrap.firstElementChild || iconWrap);
  reopenBtn.appendChild(document.createTextNode("Sidebar"));
  reopenBtn.addEventListener("click", () => applySidebarCollapsed(false));

  viewRoot = document.createElement("div");
  viewRoot.id = "view-root";

  mainEl.append(reopenBtn, viewRoot);
  shellEl.append(sidebarRoot, mainEl);

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
