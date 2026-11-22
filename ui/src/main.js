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
  renderInsightsModal,
  renderShortcutsModal,
} from "./views.js";

const now = new Date();

function emptySearchFilters() {
  return {
    starred_only: false,
    notebook_id: "",
    mood: "",
    tag: "",
    from_date: "",
    to_date: "",
  };
}

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
  starredOnly: false,
  currentEntry: null,

  calendarYear: now.getFullYear(),
  calendarMonth: now.getMonth() + 1,
  calendarDays: [],
  calendarSelectedDate: null,

  activeModal: null, // null | "search" | "migration" | "backup" | "insights" | "shortcuts"
  searchQuery: "",
  searchFilters: emptySearchFilters(),
  searchResults: [],
  searchLoading: false,
  vaultSyncing: false,

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
let toastRoot = null;
let paperShader = null;
let activeEditorHandle = null;
let searchDebounceTimer = null;
let entryLoadSeq = 0;
let calendarLoadSeq = 0;
let searchSeq = 0;
let refreshSeq = 0;
let statsDebounceTimer = null;

function showToast(message, tone = "info") {
  if (!toastRoot || !message) return;
  const toast = document.createElement("div");
  toast.className = `editorial-toast is-${tone}`;
  toast.setAttribute("role", "status");
  const icName =
    tone === "success" ? "check" : tone === "error" ? "close" : "spark";
  const icWrap = document.createElement("span");
  icWrap.className = "toast-icon-wrap";
  icWrap.innerHTML = icon(icName, "tiny-icon");
  const msg = document.createElement("span");
  msg.textContent = String(message);
  toast.append(icWrap, msg);
  toastRoot.appendChild(toast);

  const dismiss = () => {
    toast.classList.add("is-leaving");
    setTimeout(() => toast.remove(), 220);
  };
  toast.addEventListener("click", dismiss);
  setTimeout(dismiss, tone === "error" ? 4500 : 2800);
}

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
    if (state.starredOnly && !e.starred) {
      return false;
    }
    return true;
  });
}

function createBlankEntry(dateStr = null, overrides = null) {
  const iso = dateStr
    ? new Date(`${dateStr}T12:00:00`).toISOString()
    : new Date().toISOString();
  const nbId =
    overrides?.notebook_id ||
    state.selectedNotebookId ||
    state.notebooks?.[0]?.id ||
    "daily";
  return {
    id: null,
    frontmatter: {
      id: null,
      title: overrides?.title || "",
      date: iso,
      updated_at: iso,
      notebook_id: nbId,
      tags: Array.isArray(overrides?.tags)
        ? [...overrides.tags]
        : state.selectedTag
          ? [state.selectedTag]
          : [],
      mood: overrides?.mood ?? null,
      starred: Boolean(overrides?.starred),
      source: "log",
      source_id: null,
      weather: overrides?.weather ? { ...overrides.weather } : null,
      location: overrides?.location ? { ...overrides.location } : null,
      stamps: [],
      photos: [],
      audio: [],
      cover_asset: overrides?.cover_asset ?? null,
      links: [],
    },
    body: overrides?.body || "",
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
      api.listEntries({}),
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
    showToast(err?.message || "Failed to open entry", "error");
  }
}

async function startNewEntry(dateStr = null, overrides = null) {
  await activeEditorHandle?.flushSave();
  state.currentEntry = createBlankEntry(dateStr, overrides);
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
  if (viewKey === "starred") {
    state.activeView = "timeline";
    state.selectedNotebookId = null;
    state.selectedTag = null;
    state.starredOnly = true;
    recomputeFilteredEntries();
  } else {
    state.activeView = viewKey;
    if (viewKey === "timeline") {
      state.selectedNotebookId = null;
      state.selectedTag = null;
      state.starredOnly = false;
      recomputeFilteredEntries();
    }
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
  showToast("Notebook deleted", "info");
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
      state.starredOnly = false;
      state.activeView = "timeline";
      recomputeFilteredEntries();
      renderSidebarOnly();
      renderMainView();
      showToast(`Created notebook "${nb.name}"`, "success");
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
      state.starredOnly = false;
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
      state.starredOnly = false;
      state.activeView = "timeline";
      recomputeFilteredEntries();
      renderSidebarOnly();
      renderMainView();
    },
    onOpenMigration: () => openModal("migration"),
    onOpenBackup: () => openModal("backup"),
    onOpenInsights: () => openModal("insights"),
    onOpenShortcuts: () => openModal("shortcuts"),
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
      onDuplicateEntry: async (seed) => {
        await activeEditorHandle?.flushSave();
        const saved = await api.saveEntry({
          id: null,
          title: seed.title || "",
          date: new Date().toISOString(),
          notebook_id: seed.notebook_id || state.selectedNotebookId || "daily",
          tags: Array.isArray(seed.tags) ? seed.tags : [],
          mood: seed.mood ?? null,
          starred: false,
          weather: seed.weather ?? null,
          location: seed.location ?? null,
          stamps: [],
          photos: [],
          audio: [],
          cover_asset: seed.cover_asset ?? null,
          body: seed.body || "",
        });
        state.currentEntry = saved;
        state.activeView = "editor";
        state.editorMode = "write";
        upsertEntrySummaryFromSaved(saved);
        refreshVaultIndex()
          .then(() => activeEditorHandle?.syncNav?.())
          .catch(() => {});
        renderSidebarOnly();
        renderMainView();
        showToast("Entry duplicated", "success");
      },
      onOpenEntry: (id, dir = null) => openEntryById(id, dir),
      onNavigate: (viewKey) => navigateToView(viewKey),
      onModeChange: (m) => {
        state.editorMode = m;
      },
      onSearchText: (query) => {
        state.searchQuery = query;
        state.searchFilters = emptySearchFilters();
        openModal("search");
      },
      onToast: (msg, tone) => showToast(msg, tone),
      onSaveTemplate: async (req) => {
        const saved = await api.saveTemplate(req);
        state.templates = await api.listTemplates();
        showToast(`Saved template "${saved.name}"`, "success");
        return saved;
      },
      onDeleteTemplate: async (id) => {
        await api.deleteTemplate(id);
        state.templates = await api.listTemplates();
        showToast("Template removed", "info");
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
        upsertEntrySummaryFromSaved(saved);
        if (meta?.structural) {
          refreshVaultIndex()
            .then(() => {
              activeEditorHandle?.syncNav?.();
            })
            .catch(() => {});
        }
        return saved;
      },
      onDeleteEntry: async (id) => {
        await handleDeleteEntry(id);
        showToast("Entry deleted", "info");
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
      onClearFilters: () => navigateToView("timeline"),
      onNewEntry: () => startNewEntry(),
      onUpdateNotebook: handleUpdateNotebook,
      onDeleteNotebook: handleDeleteNotebook,
      onUploadNotebookCover: handleUploadNotebookCover,
      onDeleteEntry: async (id) => {
        await handleDeleteEntry(id);
        showToast("Entry deleted", "info");
      },
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
        state.calendarSelectedDate = null;
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
        state.calendarSelectedDate = null;
        await loadCalendarMonth(y, m);
        renderMainView();
      },
      onSelectMonth: async (y, m) => {
        state.calendarSelectedDate = null;
        await loadCalendarMonth(y, m);
        renderMainView();
      },
      onChangeNotebookFilter: async (nbId) => {
        state.selectedNotebookId = nbId || null;
        state.calendarSelectedDate = null;
        recomputeFilteredEntries();
        await loadCalendarMonth(state.calendarYear, state.calendarMonth);
        renderSidebarOnly();
        renderMainView();
      },
      onOpenEntry: (id) => openEntryById(id),
      onNewEntryForDate: (dateStr) => startNewEntry(dateStr),
      onDeleteEntry: async (id) => {
        await handleDeleteEntry(id);
        showToast("Entry deleted", "info");
      },
    });
    return;
  }

  if (state.activeView === "on_this_day") {
    renderOnThisDayView(viewRoot, state, {
      onOpenEntry: (id) => openEntryById(id),
      onDeleteEntry: async (id) => {
        await handleDeleteEntry(id);
        showToast("Entry deleted", "info");
      },
    });
  }
}

function hasActiveSearchCriteria() {
  const f = state.searchFilters || {};
  return Boolean(
    (state.searchQuery || "").trim() ||
      f.starred_only ||
      f.notebook_id ||
      f.mood ||
      f.tag ||
      f.from_date ||
      f.to_date
  );
}

async function runSearchQuery(text = state.searchQuery) {
  const seq = ++searchSeq;
  state.searchQuery = text;
  const trimmed = String(text || "").trim();
  const f = state.searchFilters || {};
  if (!hasActiveSearchCriteria()) {
    state.searchResults = [];
    state.searchLoading = false;
    if (state.activeModal === "search") renderModal();
    return;
  }
  state.searchLoading = true;
  if (state.activeModal === "search") renderModal();
  try {
    const hits = await api.searchEntries({
      text: trimmed || null,
      notebook_id: f.notebook_id || null,
      tag: f.tag || null,
      mood: f.mood || null,
      starred_only: Boolean(f.starred_only),
      from_date: f.from_date || null,
      to_date: f.to_date || null,
      limit: 50,
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
    if (hasActiveSearchCriteria()) {
      runSearchQuery(state.searchQuery);
    }
    return;
  }
  if (modalName === "migration") {
    state.migrationError = null;
    renderModal();
    return;
  }
  if (modalName === "shortcuts") {
    renderModal();
    return;
  }
  if (modalName === "insights") {
    renderModal();
    api
      .getVaultStats()
      .then((s) => {
        if (s) {
          state.stats = s;
          renderSidebarOnly();
          if (state.activeModal === "insights") renderModal();
        }
      })
      .catch(() => {});
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
      onFilterChange: (patch) => {
        state.searchFilters = {
          ...(state.searchFilters || emptySearchFilters()),
          ...patch,
        };
        runSearchQuery(state.searchQuery);
      },
      onResetSearch: () => {
        state.searchQuery = "";
        state.searchFilters = emptySearchFilters();
        state.searchResults = [];
        renderModal();
      },
      onSelectEntry: (id) => openEntryById(id),
      onClose: closeModal,
    });
    return;
  }

  if (state.activeModal === "insights") {
    renderInsightsModal(modalRoot, state, {
      onSelectTag: async (tag) => {
        if (state.activeView === "editor") {
          await activeEditorHandle?.flushSave();
        }
        state.selectedTag = tag;
        state.starredOnly = false;
        state.activeView = "timeline";
        recomputeFilteredEntries();
        renderSidebarOnly();
        renderMainView();
      },
      onOpenVaultFolder: async () => {
        try {
          await api.openVaultFolder();
        } catch (err) {
          showToast(err?.message || "Failed to open vault folder", "error");
        }
      },
      onSyncVault: async (fullRebuild) => {
        state.vaultSyncing = true;
        renderModal();
        try {
          const res = await api.syncVault(fullRebuild);
          await refreshVaultIndex();
          state.templates = await api.listTemplates().catch(() => state.templates);
          if (state.activeView === "editor" && state.currentEntry?.id) {
            try {
              state.currentEntry = await api.getEntry(state.currentEntry.id);
              renderMainView();
            } catch {
              // ignore if deleted externally
            }
          } else if (state.activeView !== "editor") {
            if (state.activeView === "calendar") {
              await loadCalendarMonth(state.calendarYear, state.calendarMonth);
            }
            renderMainView();
          }
          showToast(
            res.full_rebuild
              ? `Rebuilt index (${res.indexed_count} entries)`
              : res.indexed_count > 0
                ? `Synced ${res.indexed_count} external changes`
                : "Vault index is up to date",
            "success"
          );
        } catch (err) {
          showToast(err?.message || "Vault sync failed", "error");
        } finally {
          state.vaultSyncing = false;
          if (state.activeModal === "insights") renderModal();
        }
      },
      onClose: closeModal,
    });
    return;
  }

  if (state.activeModal === "shortcuts") {
    renderShortcutsModal(modalRoot, {
      onClose: closeModal,
    });
    return;
  }

  if (state.activeModal === "migration") {
    const runImportTask = async (fn, dryRun, fallbackErr) => {
      state.migrationLoading = true;
      state.migrationError = null;
      renderModal();
      try {
        state.migrationReport = await fn();
        if (!dryRun) {
          await refreshVaultIndex();
          if (state.activeView === "timeline") renderMainView();
          else if (state.activeView === "calendar") {
            await loadCalendarMonth(state.calendarYear, state.calendarMonth);
            renderMainView();
          }
          showToast(
            `Imported ${state.migrationReport?.imported_count ?? 0} entries`,
            "success"
          );
        }
      } catch (err) {
        state.migrationError = err?.message || fallbackErr;
      } finally {
        state.migrationLoading = false;
        if (state.activeModal === "migration") renderModal();
      }
    };

    renderMigrationModal(modalRoot, state, {
      onImportDayOnePath: (zipPath, dryRun, notebookId = null) =>
        runImportTask(
          () =>
            api.importDayone({
              zip_path: zipPath,
              notebook_id: notebookId ?? state.selectedNotebookId,
              dry_run: Boolean(dryRun),
            }),
          dryRun,
          "Day One import failed."
        ),
      onImportNotionPath: (sourcePath, dryRun, notebookId = null) =>
        runImportTask(
          () =>
            api.importNotion({
              source_path: sourcePath,
              notebook_id: notebookId ?? state.selectedNotebookId,
              dry_run: Boolean(dryRun),
            }),
          dryRun,
          "Notion import failed."
        ),
      onImportDayOneFile: (file, dryRun = false, notebookId = null) =>
        runImportTask(
          () =>
            file?.path
              ? api.importDayone({
                  zip_path: file.path,
                  notebook_id: notebookId ?? state.selectedNotebookId,
                  dry_run: Boolean(dryRun),
                })
              : api.importArchiveUpload("dayone", file, {
                  notebook_id: notebookId ?? state.selectedNotebookId,
                  dry_run: Boolean(dryRun),
                }),
          dryRun,
          "Day One archive import failed."
        ),
      onImportNotionFile: (file, dryRun = false, notebookId = null) =>
        runImportTask(
          () =>
            file?.path
              ? api.importNotion({
                  source_path: file.path,
                  notebook_id: notebookId ?? state.selectedNotebookId,
                  dry_run: Boolean(dryRun),
                })
              : api.importArchiveUpload("notion", file, {
                  notebook_id: notebookId ?? state.selectedNotebookId,
                  dry_run: Boolean(dryRun),
                }),
          dryRun,
          "Notion archive import failed."
        ),
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

  toastRoot = document.createElement("div");
  toastRoot.id = "toast-root";
  toastRoot.className = "editorial-toast-stack";
  toastRoot.setAttribute("aria-live", "polite");

  root.append(shellEl, modalRoot, toastRoot);
  app.appendChild(root);
}

function wireGlobalShortcuts() {
  window.addEventListener("keydown", (ev) => {
    const mod = ev.ctrlKey || ev.metaKey;
    const key = ev.key.toLowerCase();

    if (mod && key === "k") {
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
    if (mod && ev.key === "/") {
      ev.preventDefault();
      if (state.activeModal === "shortcuts") {
        closeModal();
      } else {
        openModal("shortcuts");
      }
      return;
    }
    if (mod && key === "n" && !ev.shiftKey && !state.activeModal) {
      ev.preventDefault();
      startNewEntry();
      return;
    }
    if (mod && key === "f" && !ev.shiftKey && !ev.altKey) {
      if (state.activeView === "editor" && !state.activeModal) {
        ev.preventDefault();
        activeEditorHandle?.openFind?.();
      }
      return;
    }
    if (mod && key === "s" && !ev.shiftKey) {
      ev.preventDefault();
      if (
        !state.activeModal &&
        state.activeView === "editor" &&
        activeEditorHandle?.saveNow
      ) {
        activeEditorHandle
          .saveNow()
          .then((saved) => {
            if (saved) showToast("Entry saved", "success");
          })
          .catch((err) => {
            showToast(err?.message || "Save failed", "error");
          });
      }
      return;
    }
    if (
      mod &&
      key === "e" &&
      state.activeView === "editor" &&
      !state.activeModal
    ) {
      ev.preventDefault();
      activeEditorHandle?.toggleMode();
      return;
    }
    if (mod && !ev.shiftKey && !ev.altKey && !state.activeModal) {
      if (ev.key === "1") {
        ev.preventDefault();
        navigateToView("editor");
        return;
      }
      if (ev.key === "2") {
        ev.preventDefault();
        navigateToView("timeline");
        return;
      }
      if (ev.key === "3") {
        ev.preventDefault();
        navigateToView("calendar");
        return;
      }
      if (ev.key === "4") {
        ev.preventDefault();
        navigateToView("on_this_day");
        return;
      }
    }
    const activeTag = document.activeElement?.tagName;
    const isEditingField =
      activeTag === "INPUT" ||
      activeTag === "TEXTAREA" ||
      activeTag === "SELECT" ||
      Boolean(document.activeElement?.isContentEditable);

    if (
      ev.altKey &&
      !mod &&
      !isEditingField &&
      state.activeView === "editor" &&
      !state.activeModal
    ) {
      if (ev.key === "ArrowLeft") {
        ev.preventDefault();
        activeEditorHandle?.goAdjacent?.("prev");
      } else if (ev.key === "ArrowRight") {
        ev.preventDefault();
        activeEditorHandle?.goAdjacent?.("next");
      }
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
    api.listEntries({}),
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
