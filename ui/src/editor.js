import { api, resolveVaultUrl } from "./ipc.js";
import { icon, weatherIcon } from "./icons.js";
import { renderMarkdownReadView, extractWikilinks } from "./markdown.js";
import { createVoiceRecorder, createWaveformPlayer } from "./audio.js";
import {
  isAutoDerivedTitle,
  createPolaroidElement,
  openPolaroidLightbox,
  renderStampOverlay,
  renderStampPickerPopover,
} from "./views.js";

const MOOD_OPTIONS = [
  "reflective",
  "calm",
  "grateful",
  "energized",
  "melancholic",
  "curious",
  "restless",
  "joyful",
];

const WEATHER_PRESETS = [
  { condition: "Clear", icon_key: "sun" },
  { condition: "Partly Cloudy", icon_key: "cloud" },
  { condition: "Overcast", icon_key: "cloud" },
  { condition: "Light Rain", icon_key: "rain" },
  { condition: "Heavy Rain", icon_key: "rain" },
  { condition: "Storm", icon_key: "storm" },
  { condition: "Fog", icon_key: "fog" },
  { condition: "Snow", icon_key: "snow" },
];

const STARTER_TEMPLATE_IDS = new Set([
  "evening-reflection",
  "morning-pages",
  "field-travel-notes",
  "quiet-gratitude",
]);

const SLASH_COMMANDS = [
  {
    cmd: "/h1",
    glyph: "H1",
    title: "Heading I",
    sub: "Large section title",
    action: "line",
    insert: "# ",
  },
  {
    cmd: "/h2",
    glyph: "H2",
    title: "Heading II",
    sub: "Sub-section heading",
    action: "line",
    insert: "## ",
  },
  {
    cmd: "/todo",
    glyph: "[ ]",
    title: "Checklist",
    sub: "Interactive task item",
    action: "line",
    insert: "- [ ] ",
  },
  {
    cmd: "/quote",
    glyph: "\"",
    title: "Pull-Quote",
    sub: "Indented literary excerpt",
    action: "line",
    insert: "> ",
  },
  {
    cmd: "/callout",
    glyph: "!",
    title: "Botanical Note",
    sub: "Framed aside note",
    action: "line",
    insert: "> [!NOTE] ",
  },
  {
    cmd: "/divider",
    glyph: "---",
    title: "Section Rule",
    sub: "Hairline horizontal break",
    action: "block",
    insert: "\n---\n\n",
  },
  {
    cmd: "/photo",
    glyph: "IMG",
    title: "Attach Photo",
    sub: "Polaroid print with caption",
    action: "photo",
  },
  {
    cmd: "/voice",
    glyph: "MIC",
    title: "Voice Note",
    sub: "Record or upload audio",
    action: "voice",
  },
  {
    cmd: "/stamp",
    glyph: "INK",
    title: "Place or Manage Stamps",
    sub: "Botanical seals, stickers & washi",
    action: "stamp",
  },
  {
    cmd: "/clear-stamps",
    glyph: "CLR",
    title: "Remove All Page Stamps",
    sub: "Clear all stamps placed on this entry",
    action: "clear_stamps",
  },
  {
    cmd: "/template",
    glyph: "TPL",
    title: "Editorial Template",
    sub: "Structured reflection prompts",
    action: "template",
  },
  {
    cmd: "/link",
    glyph: "[[ ]]",
    title: "Link Past Entry",
    sub: "Insert a [[wikilink]] connection",
    action: "link",
  },
];

function toLocalDateInputValue(isoDate) {
  const d = isoDate ? new Date(isoDate) : new Date();
  const valid = !Number.isNaN(d.getTime()) ? d : new Date();
  const y = valid.getFullYear();
  const m = String(valid.getMonth() + 1).padStart(2, "0");
  const day = String(valid.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function toLocalTimeInputValue(isoDate) {
  const d = isoDate ? new Date(isoDate) : new Date();
  const valid = !Number.isNaN(d.getTime()) ? d : new Date();
  const hh = String(valid.getHours()).padStart(2, "0");
  const mm = String(valid.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

function buildExportMarkdown(draft) {
  const title =
    draft.titleInput.trim() ||
    draft.hiddenAutoTitle ||
    deriveFallbackTitle(draft.body, draft.date);
  const lines = ["---"];
  lines.push(`title: "${title.replace(/"/g, '\\"')}"`);
  lines.push(`date: "${draft.date}"`);
  if (draft.notebook_id) lines.push(`notebook: "${draft.notebook_id}"`);
  if (draft.mood) lines.push(`mood: "${draft.mood}"`);
  if (draft.starred) lines.push("starred: true");
  if (draft.location?.place_name) {
    lines.push(`location: "${draft.location.place_name.replace(/"/g, '\\"')}"`);
  }
  if (draft.weather?.condition) {
    const temp =
      typeof draft.weather.temperature_c === "number"
        ? ` (${Math.round(draft.weather.temperature_c)}°C)`
        : "";
    lines.push(`weather: "${draft.weather.condition}${temp}"`);
  }
  if (draft.tags.length > 0) {
    lines.push(`tags: [${draft.tags.map((t) => `"${t}"`).join(", ")}]`);
  }
  lines.push("---", "");
  if (title && !isAutoDerivedTitle(title, draft.body)) {
    lines.push(`# ${title}`, "");
  }
  lines.push(draft.body || "");
  return lines.join("\n");
}

function downloadTextFile(filename, text, mime = "text/markdown;charset=utf-8") {
  const blob = new Blob([text], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function iconNode(name, cls = "tiny-icon") {
  const wrap = document.createElement("span");
  wrap.style.display = "inline-flex";
  wrap.style.alignItems = "center";
  wrap.innerHTML = icon(name, cls);
  return wrap.firstElementChild || wrap;
}

function weatherIconNode(iconKey, cls = "tiny-icon") {
  const wrap = document.createElement("span");
  wrap.style.display = "inline-flex";
  wrap.style.alignItems = "center";
  wrap.innerHTML = weatherIcon(iconKey, cls);
  return wrap.firstElementChild || wrap;
}

function countWords(text) {
  const s = String(text || "")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .trim();
  if (!s) return 0;
  return s.split(/\s+/).length;
}

function formatEntryDateLine(isoDate, body) {
  const d = isoDate ? new Date(isoDate) : new Date();
  const valid = !Number.isNaN(d.getTime()) ? d : new Date();
  const datePart = valid.toLocaleDateString(undefined, {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
  });
  const timePart = valid.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
  const words = countWords(body);
  const mins = Math.max(1, Math.ceil(words / 220));
  return words > 0
    ? `${datePart} · ${timePart} · ${mins} min read`
    : `${datePart} · ${timePart}`;
}

function countNewlines(str) {
  let count = 0;
  let pos = -1;
  while ((pos = str.indexOf("\n", pos + 1)) !== -1) {
    count += 1;
  }
  return count;
}

function measureTextareaHeight(el) {
  if (!el) return;
  const scroller = el.closest(".sanctuary-main") || document.scrollingElement;
  const prevScroll = scroller ? scroller.scrollTop : 0;
  const prevWinY = window.scrollY;
  el.style.height = "auto";
  el.style.height = `${Math.max(480, el.scrollHeight)}px`;
  if (scroller && scroller.scrollTop !== prevScroll) {
    scroller.scrollTop = prevScroll;
  }
  if (window.scrollY !== prevWinY) {
    window.scrollTo(window.scrollX, prevWinY);
  }
}

function deriveFallbackTitle(body, dateIso) {
  const cleaned = String(body || "")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/^#+\s*/gm, "")
    .replace(/^>\s*(\[![A-Z]+\]\s*)?/gm, "")
    .replace(/\s+/g, " ")
    .trim();
  if (cleaned) {
    return cleaned.length > 60 ? `${cleaned.slice(0, 60).trim()}…` : cleaned;
  }
  const d = dateIso ? new Date(dateIso) : new Date();
  if (!Number.isNaN(d.getTime())) {
    return d.toLocaleDateString(undefined, {
      month: "long",
      day: "numeric",
      year: "numeric",
    });
  }
  return "Untitled Entry";
}

function detectSlashToken(textarea) {
  const pos = textarea.selectionStart ?? 0;
  const before = textarea.value.slice(0, pos);
  const lineStart = before.lastIndexOf("\n") + 1;
  const lineBefore = before.slice(lineStart);
  const m = /(?:^|\s)(\/[a-z0-9_-]*)$/i.exec(lineBefore);
  if (!m) return null;
  const token = m[1];
  const tokenStart = pos - token.length;
  return {
    query: token.slice(1).toLowerCase(),
    start: tokenStart,
    end: pos,
  };
}

export function renderEntryEditor(container, state, callbacks = {}) {
  container.innerHTML = "";

  const entry = state.currentEntry;
  if (!entry) {
    const emptyCard = document.createElement("div");
    emptyCard.className = "editorial-empty-card";
    const h = document.createElement("h2");
    h.className = "empty-serif-heading";
    h.textContent = "A fresh sheet awaits";
    const p = document.createElement("p");
    p.className = "empty-prose-note";
    p.textContent =
      "Create a new entry or choose an earlier page from the index.";
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "btn-editorial btn-sage";
    btn.textContent = "New Entry";
    btn.addEventListener("click", () => callbacks.onNewEntry?.());
    emptyCard.append(h, p, btn);
    container.appendChild(emptyCard);
    return { flushSave: async () => {}, focusEditor: () => {} };
  }

  const fm = entry.frontmatter || {};
  const initialRawTitle = String(fm.title || "");
  const initialBody = String(entry.body || "");
  const titleWasAuto = isAutoDerivedTitle(initialRawTitle, initialBody);

  const draft = {
    id: entry.id || fm.id || null,
    titleInput: titleWasAuto ? "" : initialRawTitle,
    hiddenAutoTitle: titleWasAuto ? initialRawTitle : "",
    date: fm.date || new Date().toISOString(),
    notebook_id:
      fm.notebook_id ||
      state.selectedNotebookId ||
      state.notebooks?.[0]?.id ||
      "daily",
    tags: Array.isArray(fm.tags) ? [...fm.tags] : [],
    mood: fm.mood || null,
    starred: Boolean(fm.starred),
    weather: fm.weather ? { ...fm.weather } : null,
    location: fm.location ? { ...fm.location } : null,
    stamps: Array.isArray(fm.stamps) ? fm.stamps.map((s) => ({ ...s })) : [],
    photos: Array.isArray(fm.photos) ? fm.photos.map((p) => ({ ...p })) : [],
    audio: Array.isArray(fm.audio) ? fm.audio.map((a) => ({ ...a })) : [],
    cover_asset: fm.cover_asset || null,
    body: initialBody,
    path: entry.path || "",
    backlinks: Array.isArray(entry.backlinks) ? [...entry.backlinks] : [],
  };

  let mode = state.editorMode === "read" ? "read" : "write";
  let dirty = false;
  let saving = false;
  let editVersion = 0;
  let savedVersion = 0;
  let inFlightSave = null;
  let pendingStructuralRefresh = false;
  let disposed = false;
  let saveTimer = null;
  let ledgerTimer = null;
  let shrinkRaf = null;
  let lastBodyLen = initialBody.length;
  let lastNewlineCount = countNewlines(initialBody);
  let deleteConfirmOpen = false;
  let coverPopoverOpen = false;
  let moreMenuOpen = false;
  let datePopoverOpen = false;
  let weatherPopoverOpen = false;
  let topbarPopoverAbort = null;
  let datePopoverAbort = null;
  let weatherPopoverAbort = null;
  let promptDismissed = false;
  let activeInlineTool = null; // null | "voice" | "template" | "link" | "stamp" | "save_template"
  let stampPickerTab = "stickers";
  let activeRecorderCard = null;
  let activeWaveformRibbons = [];
  let slashState = null; // { query, start, end, selectedIndex }
  let locationEditing = false;
  let refocusTagInput = false;
  let dragDepth = 0;
  const lifecycleAbort = new AbortController();

  const sheet = document.createElement("article");
  sheet.className = "entry-page-sheet";
  if (state.pageTurnDir === "next") {
    sheet.classList.add("page-turn-next");
  } else if (state.pageTurnDir === "prev") {
    sheet.classList.add("page-turn-prev");
  }
  container.appendChild(sheet);

  // Whisper-thin Reading Progress Ink Hairline
  const progressHairline = document.createElement("div");
  progressHairline.className = "entry-progress-hairline";
  progressHairline.setAttribute("aria-hidden", "true");
  const progressFill = document.createElement("div");
  progressFill.className = "entry-progress-fill";
  progressHairline.appendChild(progressFill);
  sheet.appendChild(progressHairline);

  let progressRaf = 0;
  const updateReadingProgress = () => {
    progressRaf = 0;
    const scroller = container.closest?.(".sanctuary-main");
    if (!scroller) {
      progressHairline.classList.remove("is-visible");
      return;
    }
    const maxScroll = scroller.scrollHeight - scroller.clientHeight;
    if (maxScroll < 140) {
      progressHairline.classList.remove("is-visible");
      progressFill.style.transform = "scaleX(0)";
      return;
    }
    const ratio = Math.min(Math.max(scroller.scrollTop / maxScroll, 0), 1);
    progressHairline.classList.toggle("is-visible", ratio > 0.015);
    progressFill.style.transform = `scaleX(${ratio.toFixed(4)})`;
  };
  const scheduleReadingProgress = () => {
    if (progressRaf || disposed) return;
    progressRaf = requestAnimationFrame(updateReadingProgress);
  };
  const scrollHost = container.closest?.(".sanctuary-main");
  if (scrollHost) {
    scrollHost.addEventListener("scroll", scheduleReadingProgress, {
      passive: true,
      signal: lifecycleAbort.signal,
    });
  }
  window.addEventListener("resize", scheduleReadingProgress, {
    passive: true,
    signal: lifecycleAbort.signal,
  });

  // Stamp overlay (sits over sheet)
  const stampLayer = document.createElement("div");
  sheet.appendChild(stampLayer);

  let justPlacedStampId = null;

  const syncStampOverlay = () => {
    const animateId = justPlacedStampId;
    justPlacedStampId = null;
    renderStampOverlay(
      stampLayer,
      draft.stamps,
      mode === "write",
      (updatedStamps) => {
        draft.stamps = updatedStamps;
        if (typeof renderInsertBar === "function") renderInsertBar();
        if (typeof renderLedger === "function") renderLedger();
        if (activeInlineTool === "stamp" && typeof renderInlineTool === "function") {
          renderInlineTool();
        }
        markDirtyAndScheduleSave();
      },
      animateId
    );
  };

  const removeStampById = (stampId) => {
    const before = draft.stamps.length;
    draft.stamps = draft.stamps.filter((s) => s.id !== stampId);
    if (draft.stamps.length === before) return;
    syncStampOverlay();
    renderInsertBar();
    renderLedger();
    if (activeInlineTool === "stamp") renderInlineTool();
    markDirtyAndScheduleSave();
  };

  const clearAllStamps = () => {
    if (draft.stamps.length === 0) return;
    draft.stamps = [];
    syncStampOverlay();
    renderInsertBar();
    renderLedger();
    if (activeInlineTool === "stamp") renderInlineTool();
    markDirtyAndScheduleSave();
    callbacks.onToast?.("Cleared all stamps from entry", "info");
  };

  // Optional cover banner container (populated only if user selects one)
  const coverContainer = document.createElement("div");
  sheet.appendChild(coverContainer);

  const renderCoverBanner = () => {
    coverContainer.innerHTML = "";
    if (!draft.cover_asset) return;
    const matchedCover = (state.manifest?.covers || []).find(
      (c) => c.path === draft.cover_asset
    );
    const isAnimated = Boolean(
      (Array.isArray(matchedCover?.tags) &&
        matchedCover.tags.includes("animated")) ||
        String(draft.cover_asset).includes("/anim-")
    );
    const banner = document.createElement("div");
    banner.className = `entry-cover-banner${
      isAnimated ? " is-animated-cover" : ""
    }`;
    const img = document.createElement("img");
    img.className = "entry-cover-img";
    img.src = resolveVaultUrl(draft.cover_asset);
    img.alt = matchedCover?.label || "Entry cover";

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "entry-cover-remove";
    removeBtn.textContent = "Remove cover";
    removeBtn.addEventListener("click", () => {
      draft.cover_asset = null;
      renderCoverBanner();
      renderTopbar();
      markDirtyAndScheduleSave();
    });

    banner.append(img, removeBtn);
    coverContainer.appendChild(banner);
  };

  // Inner column
  const column = document.createElement("div");
  column.className = "entry-column";
  sheet.appendChild(column);

  // Elements we update in-place so typing never resets focus
  let saveIndicatorEl = null;
  let datelineTextEl = null;
  let ledgerContainer = null;
  let connectionsContainer = null;
  let photosContainer = null;
  let voiceContainer = null;
  let proseRegionEl = null;
  let textareaEl = null;
  let slashContainer = null;
  let inlineToolContainer = null;
  let metaInlineEl = null;
  let blankPromptEl = null;
  let titleWrapEl = null;

  const buildSavePayload = () => {
    const cleanTitle = draft.titleInput.trim();
    const resolvedTitle =
      cleanTitle ||
      draft.hiddenAutoTitle ||
      deriveFallbackTitle(draft.body, draft.date);
    return {
      id: draft.id || null,
      title: resolvedTitle,
      date: draft.date,
      notebook_id: draft.notebook_id,
      tags: draft.tags,
      mood: draft.mood || null,
      starred: Boolean(draft.starred),
      weather: draft.weather || null,
      location: draft.location || null,
      stamps: draft.stamps,
      photos: draft.photos,
      audio: draft.audio,
      cover_asset: draft.cover_asset || null,
      body: draft.body,
    };
  };

  const updateSaveIndicator = () => {
    if (!saveIndicatorEl) return;
    saveIndicatorEl.classList.toggle("is-dirty", dirty || saving);
    saveIndicatorEl.title = "";
    if (saving) {
      saveIndicatorEl.dataset.status = "saving";
      saveIndicatorEl.textContent = "Saving…";
    } else if (dirty) {
      saveIndicatorEl.dataset.status = "unsaved";
      saveIndicatorEl.textContent = "Unsaved";
    } else {
      saveIndicatorEl.dataset.status = "saved";
      saveIndicatorEl.textContent = "Saved";
    }
  };

  const destroyActiveRecorder = () => {
    if (activeRecorderCard && typeof activeRecorderCard.destroy === "function") {
      activeRecorderCard.destroy();
    }
    activeRecorderCard = null;
  };

  const destroyActiveWaveforms = () => {
    for (const r of activeWaveformRibbons) {
      if (r && typeof r.destroy === "function") {
        r.destroy();
      }
    }
    activeWaveformRibbons = [];
  };

  const resizeTextareaForEdit = (el, forceMeasure = false) => {
    if (!el) return;
    const val = el.value;
    const nextLen = val.length;
    const nextNewlines = countNewlines(val);
    if (forceMeasure) {
      if (shrinkRaf) {
        cancelAnimationFrame(shrinkRaf);
        shrinkRaf = null;
      }
      measureTextareaHeight(el);
    } else if (nextLen >= lastBodyLen && nextNewlines >= lastNewlineCount) {
      const needed = Math.max(480, el.scrollHeight);
      if (needed > el.clientHeight) {
        el.style.height = `${needed}px`;
      }
    } else if (!shrinkRaf) {
      shrinkRaf = requestAnimationFrame(() => {
        shrinkRaf = null;
        if (!disposed && textareaEl === el) {
          measureTextareaHeight(el);
        }
      });
    }
    lastBodyLen = nextLen;
    lastNewlineCount = nextNewlines;
  };

  const updateDatelineAndLedger = (immediate = false) => {
    if (immediate) {
      if (ledgerTimer) {
        clearTimeout(ledgerTimer);
        ledgerTimer = null;
      }
      if (datelineTextEl) {
        datelineTextEl.textContent = formatEntryDateLine(draft.date, draft.body);
      }
      if (ledgerContainer) renderLedger();
      if (connectionsContainer) renderConnections();
      return;
    }
    if (ledgerTimer) clearTimeout(ledgerTimer);
    ledgerTimer = setTimeout(() => {
      ledgerTimer = null;
      if (disposed) return;
      if (datelineTextEl) {
        datelineTextEl.textContent = formatEntryDateLine(draft.date, draft.body);
      }
      if (ledgerContainer) renderLedger();
      if (connectionsContainer) renderConnections();
    }, 180);
  };

  const performSave = async () => {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
    if (inFlightSave) {
      await inFlightSave;
      if (editVersion <= savedVersion) return;
    }
    if (editVersion === savedVersion && !dirty) return;

    const runSaveLoop = async () => {
      while (editVersion > savedVersion || (dirty && !saving)) {
        const targetVersion = editVersion;
        const structural = pendingStructuralRefresh || !draft.id;
        pendingStructuralRefresh = false;
        saving = true;
        let assignedNewId = false;
        updateSaveIndicator();
        try {
          const saved = await callbacks.onSaveEntry?.(buildSavePayload(), {
            structural,
          });
          if (saved) {
            const hadNoId = !draft.id;
            draft.id = saved.id;
            draft.path = saved.path || draft.path;
            if (Array.isArray(saved.backlinks)) {
              draft.backlinks = saved.backlinks;
            }
            if (hadNoId && saved.id) {
              assignedNewId = true;
            }
          }
          savedVersion = targetVersion;
          dirty = editVersion > savedVersion;
        } catch (err) {
          saving = false;
          if (saveIndicatorEl) {
            saveIndicatorEl.classList.add("is-dirty");
            saveIndicatorEl.dataset.status = "error";
            saveIndicatorEl.textContent = "Error";
            saveIndicatorEl.title = err?.message || "Save failed";
          }
          callbacks.onToast?.(err?.message || "Save failed", "error");
          throw err;
        }
        saving = false;
        updateSaveIndicator();
        if (assignedNewId && !disposed) {
          renderTopbar();
        }
        if (!disposed) {
          updateDatelineAndLedger(true);
        }
      }
    };

    inFlightSave = runSaveLoop().finally(() => {
      inFlightSave = null;
    });
    return inFlightSave;
  };

  const markDirtyAndScheduleSave = (structural = false) => {
    editVersion += 1;
    dirty = true;
    if (structural) pendingStructuralRefresh = true;
    updateSaveIndicator();
    updateDatelineAndLedger(false);
    if (disposed) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      performSave().catch(() => {});
    }, 500);
  };

  const flushIfDirty = () => {
    if (!disposed && (editVersion > savedVersion || dirty)) {
      performSave().catch(() => {});
    }
  };

  window.addEventListener("beforeunload", flushIfDirty, {
    signal: lifecycleAbort.signal,
  });
  window.addEventListener("blur", flushIfDirty, {
    signal: lifecycleAbort.signal,
  });
  document.addEventListener(
    "visibilitychange",
    () => {
      if (document.hidden) flushIfDirty();
    },
    { signal: lifecycleAbort.signal }
  );

  window.addEventListener(
    "resize",
    () => {
      if (!textareaEl || mode !== "write" || shrinkRaf) return;
      shrinkRaf = requestAnimationFrame(() => {
        shrinkRaf = null;
        if (!disposed && textareaEl && mode === "write") {
          measureTextareaHeight(textareaEl);
        }
      });
    },
    { signal: lifecycleAbort.signal, passive: true }
  );

  // Hidden photo file picker
  const photoInput = document.createElement("input");
  photoInput.type = "file";
  photoInput.accept = "image/*";
  photoInput.multiple = true;
  photoInput.hidden = true;
  column.appendChild(photoInput);

  const handlePhotoFiles = async (files) => {
    const all = Array.from(files || []);
    const images = all.filter(
      (f) =>
        String(f.type || "").startsWith("image/") ||
        /\.(png|jpe?g|webp|gif)$/i.test(f.name || "")
    );
    const audios = all.filter(
      (f) =>
        String(f.type || "").startsWith("audio/") ||
        /\.(wav|mp3|ogg|opus|m4a|webm|flac)$/i.test(f.name || "")
    );
    if (!images.length && !audios.length) return;

    let addedPhotos = 0;
    for (const file of images) {
      try {
        const photo = await api.uploadPhoto(file, null);
        draft.photos.push(photo);
        addedPhotos += 1;
      } catch (err) {
        callbacks.onToast?.(err?.message || "Photo upload failed", "error");
        if (saveIndicatorEl) {
          saveIndicatorEl.classList.add("is-dirty");
          saveIndicatorEl.dataset.status = "error";
          saveIndicatorEl.textContent = "Error";
          saveIndicatorEl.title = err?.message || "Photo upload failed";
        }
      }
    }

    let addedAudio = 0;
    for (const file of audios) {
      try {
        const memo = await api.uploadAudio(file, file.name, null);
        draft.audio.push(memo);
        addedAudio += 1;
      } catch (err) {
        callbacks.onToast?.(err?.message || "Audio upload failed", "error");
      }
    }

    if (addedPhotos > 0) {
      renderPhotosSection();
      callbacks.onToast?.(
        addedPhotos === 1 ? "Photo attached" : `${addedPhotos} photos attached`,
        "success"
      );
    }
    if (addedAudio > 0) {
      renderVoiceSection();
      callbacks.onToast?.(
        addedAudio === 1 ? "Audio note attached" : `${addedAudio} audio notes attached`,
        "success"
      );
    }
    if (addedPhotos > 0 || addedAudio > 0) {
      markDirtyAndScheduleSave(true);
    }
  };

  photoInput.addEventListener("change", async () => {
    const files = photoInput.files;
    await handlePhotoFiles(files);
    photoInput.value = "";
  });

  // Drag & drop photo/audio attach (depth-tracked to avoid child hover flicker)
  sheet.addEventListener("dragenter", (ev) => {
    if (ev.dataTransfer?.types?.includes("Files")) {
      ev.preventDefault();
      dragDepth += 1;
      sheet.classList.add("is-drag-over");
    }
  });
  sheet.addEventListener("dragover", (ev) => {
    if (ev.dataTransfer?.types?.includes("Files")) {
      ev.preventDefault();
      sheet.classList.add("is-drag-over");
    }
  });
  sheet.addEventListener("dragleave", () => {
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) {
      sheet.classList.remove("is-drag-over");
    }
  });
  sheet.addEventListener("drop", async (ev) => {
    dragDepth = 0;
    sheet.classList.remove("is-drag-over");
    if (ev.dataTransfer?.files?.length) {
      ev.preventDefault();
      await handlePhotoFiles(ev.dataTransfer.files);
    }
  });

  // Clipboard image paste (Ctrl+V screenshot or copied image)
  sheet.addEventListener("paste", async (ev) => {
    const items = Array.from(ev.clipboardData?.items || []);
    const imgFiles = items
      .filter((it) => it.kind === "file" && String(it.type || "").startsWith("image/"))
      .map((it) => it.getAsFile())
      .filter(Boolean);
    if (imgFiles.length > 0) {
      ev.preventDefault();
      await handlePhotoFiles(imgFiles);
    }
  });

  // 1. Quiet Top Bar + Inline Find-in-Entry Bar
  const topbar = document.createElement("div");
  topbar.className = "entry-topbar";
  column.appendChild(topbar);

  const findBarSlot = document.createElement("div");
  findBarSlot.className = "entry-find-slot";
  findBarSlot.hidden = true;
  column.appendChild(findBarSlot);

  let prevBtnEl = null;
  let nextBtnEl = null;
  let prevEntryRef = null;
  let nextEntryRef = null;

  const updateAdjacentNavState = () => {
    const list = state.filteredEntries ?? state.entries ?? [];
    const currentIdx = draft.id
      ? list.findIndex((e) => e.id === draft.id)
      : -1;
    prevEntryRef =
      currentIdx >= 0 && currentIdx + 1 < list.length
        ? list[currentIdx + 1]
        : null;
    nextEntryRef = currentIdx > 0 ? list[currentIdx - 1] : null;
    if (prevBtnEl) prevBtnEl.disabled = !prevEntryRef;
    if (nextBtnEl) nextBtnEl.disabled = !nextEntryRef;
  };

  let findOpen = false;
  let findQuery = "";
  let findMatches = [];
  let findIndex = 0;

  const clearReadModeHighlights = () => {
    if (mode !== "read" || !proseRegionEl) return;
    const marks = proseRegionEl.querySelectorAll("mark.entry-find-match");
    if (marks.length === 0) return;
    renderProseRegion();
  };

  const applyFindHighlights = (shouldFocusMatch = false) => {
    findMatches = [];
    const q = findQuery.trim();
    const countEl = findBarSlot.querySelector(".entry-find-count");

    if (!q) {
      clearReadModeHighlights();
      if (countEl) countEl.textContent = "";
      return;
    }

    const qLower = q.toLowerCase();
    if (mode === "write" && textareaEl) {
      const hay = textareaEl.value.toLowerCase();
      let pos = 0;
      while ((pos = hay.indexOf(qLower, pos)) !== -1) {
        findMatches.push({ start: pos, end: pos + q.length });
        pos += Math.max(1, q.length);
      }
      if (findMatches.length === 0) {
        findIndex = 0;
        if (countEl) countEl.textContent = "0 matches";
        return;
      }
      findIndex = ((findIndex % findMatches.length) + findMatches.length) % findMatches.length;
      if (countEl) countEl.textContent = `${findIndex + 1} / ${findMatches.length}`;
      if (shouldFocusMatch) {
        const m = findMatches[findIndex];
        const findInputEl = findBarSlot.querySelector(".entry-find-input");
        const keepFindFocus = Boolean(
          findInputEl && document.activeElement === findInputEl
        );
        textareaEl.focus();
        textareaEl.setSelectionRange(m.start, m.end);
        if (keepFindFocus) {
          findInputEl.focus({ preventScroll: true });
        }
      }
      return;
    }

    if (mode === "read" && proseRegionEl) {
      renderProseRegion();
      const readEl = proseRegionEl.querySelector(".entry-reading-prose");
      if (!readEl) return;
      const textNodes = [];
      const walk = (node) => {
        for (const child of [...node.childNodes]) {
          if (child.nodeType === 3) {
            if (child.textContent && child.textContent.toLowerCase().includes(qLower)) {
              textNodes.push(child);
            }
          } else if (child.nodeType === 1 && child.tagName !== "SCRIPT" && child.tagName !== "STYLE") {
            walk(child);
          }
        }
      };
      walk(readEl);

      for (const tn of textNodes) {
        const raw = tn.textContent;
        const lower = raw.toLowerCase();
        const frag = document.createDocumentFragment();
        let last = 0;
        let pos = 0;
        while ((pos = lower.indexOf(qLower, last)) !== -1) {
          if (pos > last) {
            frag.appendChild(document.createTextNode(raw.slice(last, pos)));
          }
          const mk = document.createElement("mark");
          mk.className = "entry-find-match";
          mk.textContent = raw.slice(pos, pos + q.length);
          frag.appendChild(mk);
          findMatches.push(mk);
          last = pos + q.length;
        }
        if (last < raw.length) {
          frag.appendChild(document.createTextNode(raw.slice(last)));
        }
        if (tn.parentNode) {
          if (typeof tn.parentNode.replaceChild === "function") {
            tn.parentNode.replaceChild(frag, tn);
          } else {
            const idx = tn.parentNode.childNodes.indexOf(tn);
            if (idx >= 0) {
              const inserted = [...frag.childNodes];
              for (const n of inserted) n.parentNode = tn.parentNode;
              tn.parentNode.childNodes.splice(idx, 1, ...inserted);
              tn.parentNode = null;
            }
          }
        }
      }

      if (findMatches.length === 0) {
        findIndex = 0;
        if (countEl) countEl.textContent = "0 matches";
        return;
      }
      findIndex = ((findIndex % findMatches.length) + findMatches.length) % findMatches.length;
      findMatches.forEach((mk, i) => {
        mk.classList.toggle("is-active", i === findIndex);
      });
      if (countEl) countEl.textContent = `${findIndex + 1} / ${findMatches.length}`;
      findMatches[findIndex]?.scrollIntoView?.({ behavior: "smooth", block: "center" });
    }
  };

  const closeFindBar = (focusMatchInEditor = false) => {
    const activeMatch =
      focusMatchInEditor && mode === "write" && findMatches.length > 0
        ? findMatches[findIndex]
        : null;
    findOpen = false;
    findBarSlot.hidden = true;
    findBarSlot.innerHTML = "";
    findMatches = [];
    clearReadModeHighlights();
    if (activeMatch && textareaEl) {
      textareaEl.focus();
      textareaEl.setSelectionRange(activeMatch.start, activeMatch.end);
    }
  };

  const renderFindBar = () => {
    if (!findOpen) {
      findBarSlot.hidden = true;
      findBarSlot.innerHTML = "";
      return;
    }
    findBarSlot.hidden = false;
    findBarSlot.innerHTML = "";

    const bar = document.createElement("div");
    bar.className = "entry-find-bar";
    bar.appendChild(iconNode("search", "tiny-icon"));

    const input = document.createElement("input");
    input.type = "text";
    input.className = "entry-find-input";
    input.placeholder = "Find in entry…";
    input.value = findQuery;
    input.setAttribute("aria-label", "Find in entry");

    const countEl = document.createElement("span");
    countEl.className = "entry-find-count small-caps-meta";

    const stepMatch = (delta) => {
      if (!findQuery.trim()) return;
      findIndex += delta;
      applyFindHighlights(true);
    };

    const prevM = document.createElement("button");
    prevM.type = "button";
    prevM.className = "topbar-icon-btn";
    prevM.title = "Previous match (Shift+Enter)";
    prevM.setAttribute("aria-label", "Previous match");
    prevM.appendChild(iconNode("chevronLeft", "tiny-icon"));
    prevM.addEventListener("click", () => stepMatch(-1));

    const nextM = document.createElement("button");
    nextM.type = "button";
    nextM.className = "topbar-icon-btn";
    nextM.title = "Next match (Enter)";
    nextM.setAttribute("aria-label", "Next match");
    nextM.appendChild(iconNode("chevronRight", "tiny-icon"));
    nextM.addEventListener("click", () => stepMatch(1));

    const closeM = document.createElement("button");
    closeM.type = "button";
    closeM.className = "topbar-icon-btn";
    closeM.title = "Close find bar (Esc)";
    closeM.setAttribute("aria-label", "Close find bar");
    closeM.appendChild(iconNode("close", "tiny-icon"));
    closeM.addEventListener("click", closeFindBar);

    input.addEventListener("input", () => {
      findQuery = input.value;
      findIndex = 0;
      applyFindHighlights(false);
    });
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") {
        ev.preventDefault();
        stepMatch(ev.shiftKey ? -1 : 1);
      } else if (ev.key === "Escape") {
        ev.preventDefault();
        closeFindBar(true);
      }
    });

    bar.append(input, countEl, prevM, nextM, closeM);
    findBarSlot.appendChild(bar);
    applyFindHighlights(false);
    queueMicrotask(() => {
      input.focus();
      if (input.value && typeof input.select === "function") input.select();
    });
  };

  const renderTopbar = () => {
    topbar.innerHTML = "";
    const left = document.createElement("div");
    left.className = "entry-topbar-left";

    updateAdjacentNavState();

    prevBtnEl = document.createElement("button");
    prevBtnEl.type = "button";
    prevBtnEl.className = "topbar-ghost-btn";
    prevBtnEl.disabled = !prevEntryRef;
    prevBtnEl.title = "Previous entry";
    prevBtnEl.appendChild(iconNode("chevronLeft", "tiny-icon"));
    prevBtnEl.appendChild(document.createTextNode("Prev"));
    prevBtnEl.addEventListener("click", () => {
      if (prevEntryRef) callbacks.onOpenEntry?.(prevEntryRef.id, "prev");
    });

    nextBtnEl = document.createElement("button");
    nextBtnEl.type = "button";
    nextBtnEl.className = "topbar-ghost-btn";
    nextBtnEl.disabled = !nextEntryRef;
    nextBtnEl.title = "Next entry";
    nextBtnEl.appendChild(document.createTextNode("Next"));
    nextBtnEl.appendChild(iconNode("chevronRight", "tiny-icon"));
    nextBtnEl.addEventListener("click", () => {
      if (nextEntryRef) callbacks.onOpenEntry?.(nextEntryRef.id, "next");
    });

    const nbSelect = document.createElement("select");
    nbSelect.className = "topbar-notebook-select";
    nbSelect.setAttribute("aria-label", "Notebook");
    const nbs = state.notebooks || [];
    if (nbs.length === 0) {
      const opt = document.createElement("option");
      opt.value = draft.notebook_id || "daily";
      opt.textContent = "Journal";
      nbSelect.appendChild(opt);
    } else {
      for (const nb of nbs) {
        const opt = document.createElement("option");
        opt.value = nb.id;
        opt.textContent = nb.name;
        if (nb.id === draft.notebook_id) opt.selected = true;
        nbSelect.appendChild(opt);
      }
    }
    nbSelect.addEventListener("change", () => {
      draft.notebook_id = nbSelect.value;
      markDirtyAndScheduleSave(true);
    });

    left.append(prevBtnEl, nextBtnEl, nbSelect);

    const right = document.createElement("div");
    right.className = "entry-topbar-right";

    saveIndicatorEl = document.createElement("span");
    saveIndicatorEl.className = "entry-save-indicator";
    updateSaveIndicator();

    const setEditorMode = (nextMode) => {
      if (mode === nextMode) return;
      mode = nextMode;
      datePopoverOpen = false;
      weatherPopoverOpen = false;
      coverPopoverOpen = false;
      moreMenuOpen = false;
      renderDatePopover();
      renderWeatherPopover();
      callbacks.onModeChange?.(mode);
      renderTopbar();
      renderTitleArea();
      renderProseRegion();
      syncStampOverlay();
      if (findOpen) applyFindHighlights(false);
    };

    const modeSwitch = document.createElement("div");
    modeSwitch.className = "entry-mode-switch";
    const writeBtn = document.createElement("button");
    writeBtn.type = "button";
    writeBtn.className = `mode-switch-btn${mode === "write" ? " is-active" : ""}`;
    writeBtn.textContent = "Write";
    writeBtn.addEventListener("click", () => setEditorMode("write"));

    const readBtn = document.createElement("button");
    readBtn.type = "button";
    readBtn.className = `mode-switch-btn${mode === "read" ? " is-active" : ""}`;
    readBtn.textContent = "Read";
    readBtn.addEventListener("click", () => setEditorMode("read"));
    modeSwitch.append(writeBtn, readBtn);

    const starBtn = document.createElement("button");
    starBtn.type = "button";
    starBtn.className = `topbar-icon-btn${draft.starred ? " is-starred" : ""}`;
    starBtn.title = draft.starred ? "Unstar entry" : "Star entry";
    starBtn.setAttribute("aria-label", starBtn.title);
    starBtn.appendChild(
      iconNode(draft.starred ? "starFilled" : "star", "tiny-icon")
    );
    starBtn.addEventListener("click", () => {
      draft.starred = !draft.starred;
      renderTopbar();
      markDirtyAndScheduleSave(true);
    });

    const coverBtn = document.createElement("button");
    coverBtn.type = "button";
    coverBtn.className = `topbar-icon-btn${
      draft.cover_asset || coverPopoverOpen ? " is-active" : ""
    }`;
    coverBtn.title = "Page appearance, paper ruling & covers";
    coverBtn.setAttribute("aria-label", "Page appearance and covers");
    coverBtn.appendChild(iconNode("spark", "tiny-icon"));
    coverBtn.addEventListener("click", () => {
      moreMenuOpen = false;
      coverPopoverOpen = !coverPopoverOpen;
      renderTopbar();
    });

    const focusBtn = document.createElement("button");
    focusBtn.type = "button";
    focusBtn.className = `topbar-icon-btn${state.focusZen ? " is-active" : ""}`;
    focusBtn.title = state.focusZen
      ? "Exit Zen Focus mode (Ctrl+Shift+F)"
      : "Zen Focus mode (Ctrl+Shift+F)";
    focusBtn.setAttribute("aria-label", focusBtn.title);
    focusBtn.dataset.role = "zen-focus-btn";
    focusBtn.appendChild(iconNode("focus", "tiny-icon"));
    focusBtn.addEventListener("click", () => {
      callbacks.onToggleFocusZen?.();
    });

    const moreBtn = document.createElement("button");
    moreBtn.type = "button";
    moreBtn.className = `topbar-icon-btn${moreMenuOpen ? " is-active" : ""}`;
    moreBtn.title = "Entry actions (Export, Print, Duplicate, Save as Template)";
    moreBtn.setAttribute("aria-label", "Entry actions");
    moreBtn.appendChild(iconNode("moreHorizontal", "tiny-icon"));
    moreBtn.addEventListener("click", () => {
      coverPopoverOpen = false;
      moreMenuOpen = !moreMenuOpen;
      renderTopbar();
    });

    right.append(saveIndicatorEl, modeSwitch, starBtn, coverBtn, focusBtn, moreBtn);

    if (draft.id) {
      const divider = document.createElement("span");
      divider.className = "topbar-divider";
      right.appendChild(divider);

      if (!deleteConfirmOpen) {
        const delBtn = document.createElement("button");
        delBtn.type = "button";
        delBtn.className = "topbar-icon-btn topbar-delete-btn";
        delBtn.title = "Delete entry";
        delBtn.setAttribute("aria-label", "Delete entry");
        delBtn.appendChild(iconNode("trash", "tiny-icon"));
        delBtn.addEventListener("click", () => {
          deleteConfirmOpen = true;
          renderTopbar();
        });
        right.appendChild(delBtn);
      } else {
        const confirmWrap = document.createElement("div");
        confirmWrap.className = "topbar-delete-confirm";
        const confirmBtn = document.createElement("button");
        confirmBtn.type = "button";
        confirmBtn.className = "topbar-danger-btn";
        confirmBtn.textContent = "Confirm";
        confirmBtn.title = "Permanently delete this entry";
        confirmBtn.addEventListener("click", async () => {
          if (saveTimer) {
            clearTimeout(saveTimer);
            saveTimer = null;
          }
          dirty = false;
          editVersion = savedVersion;
          disposed = true;
          confirmBtn.disabled = true;
          confirmBtn.textContent = "Deleting…";
          if (inFlightSave) {
            await inFlightSave.catch(() => {});
          }
          try {
            await callbacks.onDeleteEntry?.(draft.id);
          } catch (err) {
            disposed = false;
            deleteConfirmOpen = false;
            renderTopbar();
            callbacks.onToast?.(err?.message || "Delete failed", "error");
            if (saveIndicatorEl) {
              saveIndicatorEl.classList.add("is-dirty");
              saveIndicatorEl.dataset.status = "error";
              saveIndicatorEl.textContent = "Error";
              saveIndicatorEl.title = err?.message || "Delete failed";
            }
          }
        });
        const cancelDelBtn = document.createElement("button");
        cancelDelBtn.type = "button";
        cancelDelBtn.className = "topbar-ghost-btn";
        cancelDelBtn.textContent = "Cancel";
        cancelDelBtn.addEventListener("click", () => {
          deleteConfirmOpen = false;
          renderTopbar();
        });
        confirmWrap.append(confirmBtn, cancelDelBtn);
        right.appendChild(confirmWrap);
      }
    }

    topbar.append(left, right);

    if (topbarPopoverAbort) {
      topbarPopoverAbort.abort();
      topbarPopoverAbort = null;
    }

    if (coverPopoverOpen) {
      const pop = document.createElement("div");
      pop.className = "cover-swatch-popover page-appearance-popover";

      // 1) Paper Ruling selector
      const rulingGroup = document.createElement("div");
      rulingGroup.className = "appearance-group";
      const rulingLabel = document.createElement("span");
      rulingLabel.className = "small-caps-meta";
      rulingLabel.textContent = "Paper Ruling";
      const rulingRow = document.createElement("div");
      rulingRow.className = "appearance-seg-row";
      const rulings = [
        { id: "plain", label: "Plain" },
        { id: "ruled", label: "Ruled" },
        { id: "dots", label: "Dot Grid" },
        { id: "margin", label: "Margin" },
      ];
      for (const r of rulings) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = `appearance-seg-btn${
          (state.paperRuling || "plain") === r.id ? " is-active" : ""
        }`;
        b.dataset.paperRuling = r.id;
        b.textContent = r.label;
        b.addEventListener("click", () => {
          state.paperRuling = r.id;
          callbacks.onSetPaperRuling?.(r.id);
          renderTopbar();
        });
        rulingRow.appendChild(b);
      }
      rulingGroup.append(rulingLabel, rulingRow);
      pop.appendChild(rulingGroup);

      // 2) Prose Scale selector
      const scaleGroup = document.createElement("div");
      scaleGroup.className = "appearance-group";
      const scaleLabel = document.createElement("span");
      scaleLabel.className = "small-caps-meta";
      scaleLabel.textContent = "Typography Scale";
      const scaleRow = document.createElement("div");
      scaleRow.className = "appearance-seg-row";
      const scales = [
        { id: "compact", label: "Compact" },
        { id: "editorial", label: "Editorial" },
        { id: "large", label: "Large" },
      ];
      for (const sc of scales) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = `appearance-seg-btn${
          (state.proseScale || "editorial") === sc.id ? " is-active" : ""
        }`;
        b.dataset.proseScale = sc.id;
        b.textContent = sc.label;
        b.addEventListener("click", () => {
          state.proseScale = sc.id;
          callbacks.onSetProseScale?.(sc.id);
          renderTopbar();
        });
        scaleRow.appendChild(b);
      }
      scaleGroup.append(scaleLabel, scaleRow);
      pop.appendChild(scaleGroup);

      const covers = state.manifest?.covers || [];
      const animatedCovers = covers.filter(
        (c) => Array.isArray(c.tags) && c.tags.includes("animated")
      );
      const staticCovers = covers.filter(
        (c) => !(Array.isArray(c.tags) && c.tags.includes("animated"))
      );

      const buildCoverGridSection = (headingText, list, isAnimSection) => {
        if (list.length === 0) return;
        const grp = document.createElement("div");
        grp.className = "appearance-group";
        const lbl = document.createElement("span");
        lbl.className = "small-caps-meta";
        lbl.textContent = headingText;
        const grid = document.createElement("div");
        grid.className = "cover-swatch-grid";
        for (const cov of list) {
          const cBtn = document.createElement("button");
          cBtn.type = "button";
          cBtn.className = `cover-mini-btn${
            draft.cover_asset === cov.path ? " is-selected" : ""
          }${isAnimSection ? " is-animated" : ""}`;
          cBtn.title = cov.label || "Cover wash";
          const img = document.createElement("img");
          img.src = resolveVaultUrl(cov.path);
          img.alt = cov.label || "Cover wash";
          cBtn.appendChild(img);
          if (isAnimSection) {
            const badge = document.createElement("span");
            badge.className = "cover-live-badge";
            badge.textContent = "Live";
            cBtn.appendChild(badge);
          }
          cBtn.addEventListener("click", () => {
            draft.cover_asset = cov.path;
            coverPopoverOpen = false;
            renderCoverBanner();
            renderTopbar();
            markDirtyAndScheduleSave();
          });
          grid.appendChild(cBtn);
        }
        grp.append(lbl, grid);
        pop.appendChild(grp);
      };

      buildCoverGridSection("Animated Scenes", animatedCovers, true);
      buildCoverGridSection("Watercolor Washes", staticCovers, false);

      const uploadLabel = document.createElement("label");
      uploadLabel.className = "topbar-ghost-btn cover-pop-full";
      uploadLabel.appendChild(iconNode("image", "tiny-icon"));
      uploadLabel.appendChild(document.createTextNode("Upload custom cover"));
      const coverFileInput = document.createElement("input");
      coverFileInput.type = "file";
      coverFileInput.accept = "image/*";
      coverFileInput.className = "sr-only";
      coverFileInput.addEventListener("change", async () => {
        const file = coverFileInput.files?.[0];
        coverFileInput.value = "";
        if (!file) return;
        try {
          const uploaded = await api.uploadPhoto(file, "Entry cover");
          draft.cover_asset = uploaded.url;
          coverPopoverOpen = false;
          renderCoverBanner();
          renderTopbar();
          markDirtyAndScheduleSave();
          callbacks.onToast?.("Entry cover updated", "success");
        } catch (err) {
          callbacks.onToast?.(err?.message || "Cover upload failed", "error");
        }
      });
      uploadLabel.appendChild(coverFileInput);
      pop.appendChild(uploadLabel);

      if (draft.cover_asset) {
        const clearCoverBtn = document.createElement("button");
        clearCoverBtn.type = "button";
        clearCoverBtn.className = "topbar-ghost-btn cover-pop-full";
        clearCoverBtn.textContent = "Remove cover";
        clearCoverBtn.addEventListener("click", () => {
          draft.cover_asset = null;
          coverPopoverOpen = false;
          renderCoverBanner();
          renderTopbar();
          markDirtyAndScheduleSave();
        });
        pop.appendChild(clearCoverBtn);
      }
      topbar.appendChild(pop);

      topbarPopoverAbort = new AbortController();
      const closePop = () => {
        coverPopoverOpen = false;
        renderTopbar();
      };
      document.addEventListener(
        "pointerdown",
        (ev) => {
          if (!pop.contains(ev.target) && !coverBtn.contains(ev.target)) {
            closePop();
          }
        },
        { signal: topbarPopoverAbort.signal }
      );
      window.addEventListener(
        "keydown",
        (ev) => {
          if (ev.key === "Escape") closePop();
        },
        { signal: topbarPopoverAbort.signal }
      );
    } else if (moreMenuOpen) {
      const menu = document.createElement("div");
      menu.className = "entry-more-popover";

      const menuItems = [
        {
          label: "Copy Markdown",
          ic: "copy",
          run: async () => {
            const md = buildExportMarkdown(draft);
            try {
              await navigator.clipboard.writeText(md);
              callbacks.onToast?.("Markdown copied to clipboard", "success");
            } catch {
              callbacks.onToast?.("Clipboard unavailable", "error");
            }
          },
        },
        {
          label: "Export .md file",
          ic: "download",
          run: () => {
            const md = buildExportMarkdown(draft);
            const ymd = toLocalDateInputValue(draft.date);
            const titleSlug = (draft.titleInput.trim() || "journal-entry")
              .toLowerCase()
              .replace(/[^a-z0-9]+/g, "-")
              .replace(/^-+|-+$/g, "")
              .slice(0, 48) || "entry";
            downloadTextFile(`${ymd}-${titleSlug}.md`, md);
            callbacks.onToast?.("Markdown file exported", "success");
          },
        },
        {
          label: "Print / Save PDF",
          ic: "printer",
          run: () => {
            window.print();
          },
        },
        {
          label: "Duplicate entry",
          ic: "bookmark",
          run: () => {
            callbacks.onDuplicateEntry?.({
              title: draft.titleInput ? `${draft.titleInput} (Copy)` : "",
              notebook_id: draft.notebook_id,
              tags: [...draft.tags],
              mood: draft.mood,
              weather: draft.weather ? { ...draft.weather } : null,
              location: draft.location ? { ...draft.location } : null,
              cover_asset: draft.cover_asset,
              body: draft.body,
            });
          },
        },
        {
          label: "Save as Template",
          ic: "feather",
          run: () => {
            if (mode !== "write") {
              mode = "write";
              callbacks.onModeChange?.("write");
              renderTitleArea();
              renderProseRegion();
              syncStampOverlay();
            }
            activeInlineTool = "save_template";
            renderInlineTool();
            inlineToolContainer.scrollIntoView({ behavior: "smooth", block: "nearest" });
          },
        },
      ];

      for (const item of menuItems) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "entry-more-item";
        btn.appendChild(iconNode(item.ic, "tiny-icon"));
        btn.appendChild(document.createTextNode(item.label));
        btn.addEventListener("click", () => {
          moreMenuOpen = false;
          renderTopbar();
          item.run();
        });
        menu.appendChild(btn);
      }

      topbar.appendChild(menu);

      topbarPopoverAbort = new AbortController();
      const closeMenu = () => {
        moreMenuOpen = false;
        renderTopbar();
      };
      document.addEventListener(
        "pointerdown",
        (ev) => {
          if (!menu.contains(ev.target) && !moreBtn.contains(ev.target)) {
            closeMenu();
          }
        },
        { signal: topbarPopoverAbort.signal }
      );
      window.addEventListener(
        "keydown",
        (ev) => {
          if (ev.key === "Escape") closeMenu();
        },
        { signal: topbarPopoverAbort.signal }
      );
    }
  };

  // 2. Entry Header (Dateline with Date/Time Picker, Optional Title, Inline Metadata)
  const header = document.createElement("header");
  header.className = "entry-header";
  column.appendChild(header);

  const dateline = document.createElement("div");
  dateline.className = "entry-dateline";

  const datelineBtn = document.createElement("button");
  datelineBtn.type = "button";
  datelineBtn.className = "entry-dateline-btn";
  datelineBtn.title = "Change entry date or time";
  datelineTextEl = document.createElement("span");
  datelineTextEl.className = "small-caps-meta";
  datelineTextEl.textContent = formatEntryDateLine(draft.date, draft.body);
  datelineBtn.append(datelineTextEl, iconNode("calendar", "tiny-icon dateline-edit-icon"));
  dateline.appendChild(datelineBtn);

  const datePopoverSlot = document.createElement("div");
  datePopoverSlot.className = "inline-popover-anchor";

  const renderDatePopover = () => {
    if (datePopoverAbort) {
      datePopoverAbort.abort();
      datePopoverAbort = null;
    }
    datePopoverSlot.innerHTML = "";
    if (!datePopoverOpen) return;

    const pop = document.createElement("form");
    pop.className = "entry-date-popover";

    const head = document.createElement("div");
    head.className = "inline-picker-header";
    const lbl = document.createElement("span");
    lbl.className = "small-caps-meta";
    lbl.textContent = "Entry Date & Time";
    head.appendChild(lbl);
    pop.appendChild(head);

    const row = document.createElement("div");
    row.className = "date-popover-inputs";
    const dateInput = document.createElement("input");
    dateInput.type = "date";
    dateInput.className = "editorial-input";
    dateInput.value = toLocalDateInputValue(draft.date);
    dateInput.required = true;

    const timeInput = document.createElement("input");
    timeInput.type = "time";
    timeInput.className = "editorial-input";
    timeInput.value = toLocalTimeInputValue(draft.date);
    timeInput.required = true;
    row.append(dateInput, timeInput);
    pop.appendChild(row);

    const actions = document.createElement("div");
    actions.className = "date-popover-actions";
    const applyBtn = document.createElement("button");
    applyBtn.type = "submit";
    applyBtn.className = "btn-editorial btn-sage small";
    applyBtn.textContent = "Apply";

    const nowBtn = document.createElement("button");
    nowBtn.type = "button";
    nowBtn.className = "btn-editorial btn-ghost small";
    nowBtn.textContent = "Set to now";
    nowBtn.addEventListener("click", () => {
      draft.date = new Date().toISOString();
      datePopoverOpen = false;
      renderDatePopover();
      updateDatelineAndLedger(true);
      markDirtyAndScheduleSave(true);
    });

    const cancelBtn = document.createElement("button");
    cancelBtn.type = "button";
    cancelBtn.className = "btn-editorial btn-ghost small";
    cancelBtn.textContent = "Cancel";
    cancelBtn.addEventListener("click", () => {
      datePopoverOpen = false;
      renderDatePopover();
    });

    actions.append(applyBtn, nowBtn, cancelBtn);
    pop.appendChild(actions);

    pop.addEventListener("submit", (ev) => {
      ev.preventDefault();
      if (!dateInput.value) return;
      const t = timeInput.value || "12:00";
      const parsed = new Date(`${dateInput.value}T${t}:00`);
      if (!Number.isNaN(parsed.getTime())) {
        draft.date = parsed.toISOString();
        datePopoverOpen = false;
        renderDatePopover();
        updateDatelineAndLedger(true);
        markDirtyAndScheduleSave(true);
      }
    });

    datePopoverSlot.appendChild(pop);
    datePopoverAbort = new AbortController();
    document.addEventListener(
      "pointerdown",
      (ev) => {
        if (!pop.contains(ev.target) && !datelineBtn.contains(ev.target)) {
          datePopoverOpen = false;
          renderDatePopover();
        }
      },
      { signal: datePopoverAbort.signal }
    );
    window.addEventListener(
      "keydown",
      (ev) => {
        if (ev.key === "Escape") {
          datePopoverOpen = false;
          renderDatePopover();
        }
      },
      { signal: datePopoverAbort.signal }
    );
  };

  datelineBtn.addEventListener("click", () => {
    datePopoverOpen = !datePopoverOpen;
    renderDatePopover();
  });

  const otdTotal =
    state.onThisDayCount ??
    (state.onThisDayGroups || []).reduce(
      (acc, g) => acc + (g.entries?.length || 0),
      0
    );
  if (otdTotal > 0) {
    const otdBtn = document.createElement("button");
    otdBtn.type = "button";
    otdBtn.className = "entry-otd-btn";
    otdBtn.textContent = `On This Day (${otdTotal})`;
    otdBtn.addEventListener("click", () => callbacks.onNavigate?.("on_this_day"));
    dateline.appendChild(otdBtn);
  }
  header.append(dateline, datePopoverSlot);

  titleWrapEl = document.createElement("div");
  header.appendChild(titleWrapEl);

  const renderTitleArea = () => {
    titleWrapEl.innerHTML = "";
    if (mode === "write") {
      const input = document.createElement("input");
      input.type = "text";
      input.className = "entry-title-input";
      input.placeholder = "Title (optional)";
      input.value = draft.titleInput;
      input.addEventListener("input", () => {
        draft.titleInput = input.value;
        if (input.value.trim()) {
          draft.hiddenAutoTitle = "";
        }
        markDirtyAndScheduleSave();
      });
      input.addEventListener("keydown", (ev) => {
        if (ev.key === "Enter") {
          ev.preventDefault();
          textareaEl?.focus();
        }
      });
      titleWrapEl.appendChild(input);
    } else {
      const showTitle =
        draft.titleInput.trim() &&
        !isAutoDerivedTitle(draft.titleInput, draft.body);
      if (showTitle) {
        const h1 = document.createElement("h1");
        h1.className = "entry-title-heading";
        h1.textContent = draft.titleInput.trim();
        titleWrapEl.appendChild(h1);
      }
    }
  };

  metaInlineEl = document.createElement("div");
  metaInlineEl.className = "entry-meta-inline";
  const weatherPopoverSlot = document.createElement("div");
  weatherPopoverSlot.className = "inline-popover-anchor";
  header.append(metaInlineEl, weatherPopoverSlot);

  const renderWeatherPopover = () => {
    if (weatherPopoverAbort) {
      weatherPopoverAbort.abort();
      weatherPopoverAbort = null;
    }
    weatherPopoverSlot.innerHTML = "";
    if (!weatherPopoverOpen) return;

    const hint = state.dailyContext?.weather_hint;
    let selectedIcon = draft.weather?.icon_key || hint?.icon_key || "sun";
    let conditionVal = draft.weather?.condition || hint?.condition || "Clear";
    let tempVal =
      typeof draft.weather?.temperature_c === "number"
        ? String(Math.round(draft.weather.temperature_c))
        : typeof hint?.temperature_c === "number"
          ? String(Math.round(hint.temperature_c))
          : "";

    const pop = document.createElement("form");
    pop.className = "entry-weather-popover";

    const head = document.createElement("div");
    head.className = "inline-picker-header";
    const lbl = document.createElement("span");
    lbl.className = "small-caps-meta";
    lbl.textContent = "Weather Conditions";
    head.appendChild(lbl);
    pop.appendChild(head);

    const presets = document.createElement("div");
    presets.className = "weather-presets-grid";

    const condInput = document.createElement("input");
    condInput.type = "text";
    condInput.className = "editorial-input";
    condInput.placeholder = "Condition (e.g. Light Rain)";
    condInput.value = conditionVal;

    const tempInput = document.createElement("input");
    tempInput.type = "number";
    tempInput.step = "1";
    tempInput.className = "editorial-input weather-temp-input";
    tempInput.placeholder = "°C";
    tempInput.value = tempVal;

    const renderPresetBtns = () => {
      presets.innerHTML = "";
      for (const p of WEATHER_PRESETS) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = `weather-preset-btn${
          conditionVal.toLowerCase() === p.condition.toLowerCase() ? " is-active" : ""
        }`;
        b.append(weatherIconNode(p.icon_key, "tiny-icon"), document.createTextNode(p.condition));
        b.addEventListener("click", () => {
          conditionVal = p.condition;
          selectedIcon = p.icon_key;
          condInput.value = p.condition;
          renderPresetBtns();
        });
        presets.appendChild(b);
      }
    };
    renderPresetBtns();
    pop.appendChild(presets);

    condInput.addEventListener("input", () => {
      conditionVal = condInput.value;
    });

    const fieldsRow = document.createElement("div");
    fieldsRow.className = "weather-fields-row";
    fieldsRow.append(condInput, tempInput);
    pop.appendChild(fieldsRow);

    const actions = document.createElement("div");
    actions.className = "date-popover-actions";
    const applyBtn = document.createElement("button");
    applyBtn.type = "submit";
    applyBtn.className = "btn-editorial btn-sage small";
    applyBtn.textContent = "Save";

    actions.appendChild(applyBtn);

    if (draft.weather) {
      const clearBtn = document.createElement("button");
      clearBtn.type = "button";
      clearBtn.className = "btn-editorial btn-ghost small is-danger";
      clearBtn.textContent = "Clear";
      clearBtn.addEventListener("click", () => {
        draft.weather = null;
        weatherPopoverOpen = false;
        renderWeatherPopover();
        renderMetaInline();
        markDirtyAndScheduleSave();
      });
      actions.appendChild(clearBtn);
    }

    const cancelBtn = document.createElement("button");
    cancelBtn.type = "button";
    cancelBtn.className = "btn-editorial btn-ghost small";
    cancelBtn.textContent = "Cancel";
    cancelBtn.addEventListener("click", () => {
      weatherPopoverOpen = false;
      renderWeatherPopover();
    });
    actions.appendChild(cancelBtn);
    pop.appendChild(actions);

    pop.addEventListener("submit", (ev) => {
      ev.preventDefault();
      const cond = condInput.value.trim();
      if (!cond) {
        draft.weather = null;
      } else {
        const parsedTemp =
          tempInput.value.trim() !== "" && Number.isFinite(Number(tempInput.value))
            ? Number(tempInput.value)
            : null;
        draft.weather = {
          condition: cond,
          temperature_c: parsedTemp,
          icon_key: selectedIcon || "cloud",
          humidity_pct: draft.weather?.humidity_pct ?? null,
          summary: cond,
        };
      }
      weatherPopoverOpen = false;
      renderWeatherPopover();
      renderMetaInline();
      markDirtyAndScheduleSave();
    });

    weatherPopoverSlot.appendChild(pop);
    weatherPopoverAbort = new AbortController();
    document.addEventListener(
      "pointerdown",
      (ev) => {
        if (!pop.contains(ev.target) && !metaInlineEl.contains(ev.target)) {
          weatherPopoverOpen = false;
          renderWeatherPopover();
        }
      },
      { signal: weatherPopoverAbort.signal }
    );
    window.addEventListener(
      "keydown",
      (ev) => {
        if (ev.key === "Escape") {
          weatherPopoverOpen = false;
          renderWeatherPopover();
        }
      },
      { signal: weatherPopoverAbort.signal }
    );
  };

  const renderMetaInline = () => {
    metaInlineEl.innerHTML = "";

    // Location
    const locWrap = document.createElement("span");
    locWrap.className = "meta-inline-item";
    locWrap.appendChild(iconNode("pin", "tiny-icon"));
    if (locationEditing) {
      const locInput = document.createElement("input");
      locInput.type = "text";
      locInput.className = "meta-inline-input";
      locInput.placeholder = "City or place";
      locInput.value = draft.location?.place_name || "";
      let locCommitted = false;
      const commitLoc = () => {
        if (locCommitted) return;
        locCommitted = true;
        const val = locInput.value.trim();
        locationEditing = false;
        if (val) {
          draft.location = {
            ...(draft.location || {}),
            place_name: val,
            city: draft.location?.city ?? null,
            country: draft.location?.country ?? null,
            latitude: draft.location?.latitude ?? null,
            longitude: draft.location?.longitude ?? null,
          };
        } else {
          draft.location = null;
        }
        renderMetaInline();
        markDirtyAndScheduleSave();
      };
      locInput.addEventListener("keydown", (ev) => {
        if (ev.key === "Enter") {
          ev.preventDefault();
          commitLoc();
        } else if (ev.key === "Escape") {
          locCommitted = true;
          locationEditing = false;
          renderMetaInline();
        }
      });
      locInput.addEventListener("blur", commitLoc);
      locWrap.appendChild(locInput);
      queueMicrotask(() => locInput.focus());
    } else if (draft.location?.place_name) {
      const locBtn = document.createElement("button");
      locBtn.type = "button";
      locBtn.className = "meta-inline-btn";
      locBtn.textContent = draft.location.place_name;
      locBtn.title = "Edit location";
      locBtn.addEventListener("click", () => {
        locationEditing = true;
        renderMetaInline();
      });
      locWrap.appendChild(locBtn);
    } else {
      const addLocBtn = document.createElement("button");
      addLocBtn.type = "button";
      addLocBtn.className = "meta-inline-btn";
      addLocBtn.textContent = "+ Add location";
      addLocBtn.addEventListener("click", () => {
        locationEditing = true;
        renderMetaInline();
      });
      locWrap.appendChild(addLocBtn);
    }
    metaInlineEl.appendChild(locWrap);

    // Weather
    const wxWrap = document.createElement("span");
    wxWrap.className = "meta-inline-item";
    if (draft.weather) {
      wxWrap.appendChild(
        weatherIconNode(draft.weather.icon_key || "sun", "tiny-icon")
      );
      const wxBtn = document.createElement("button");
      wxBtn.type = "button";
      wxBtn.className = "meta-inline-btn";
      const tempPart =
        typeof draft.weather.temperature_c === "number"
          ? ` · ${Math.round(draft.weather.temperature_c)}°C`
          : "";
      wxBtn.textContent = `${draft.weather.condition}${tempPart}`;
      wxBtn.title = "Edit or clear weather";
      wxBtn.addEventListener("click", () => {
        weatherPopoverOpen = !weatherPopoverOpen;
        renderWeatherPopover();
      });
      wxWrap.appendChild(wxBtn);
    } else {
      wxWrap.appendChild(weatherIconNode("cloud", "tiny-icon"));
      const addWxBtn = document.createElement("button");
      addWxBtn.type = "button";
      addWxBtn.className = "meta-inline-btn";
      addWxBtn.textContent = "+ Weather";
      addWxBtn.title = "Add weather conditions";
      addWxBtn.addEventListener("click", () => {
        weatherPopoverOpen = !weatherPopoverOpen;
        renderWeatherPopover();
      });
      wxWrap.appendChild(addWxBtn);
    }
    metaInlineEl.appendChild(wxWrap);

    // Mood select
    const moodWrap = document.createElement("span");
    moodWrap.className = "meta-inline-item";
    const moodSel = document.createElement("select");
    moodSel.className = "meta-inline-select";
    moodSel.setAttribute("aria-label", "Mood");
    const blankOpt = document.createElement("option");
    blankOpt.value = "";
    blankOpt.textContent = "+ Mood";
    moodSel.appendChild(blankOpt);
    const moodList =
      draft.mood && !MOOD_OPTIONS.includes(draft.mood)
        ? [draft.mood, ...MOOD_OPTIONS]
        : MOOD_OPTIONS;
    for (const m of moodList) {
      const opt = document.createElement("option");
      opt.value = m;
      opt.textContent = m;
      if (draft.mood === m) opt.selected = true;
      moodSel.appendChild(opt);
    }
    moodSel.addEventListener("change", () => {
      draft.mood = moodSel.value || null;
      markDirtyAndScheduleSave();
    });
    moodWrap.appendChild(moodSel);
    metaInlineEl.appendChild(moodWrap);

    // Tags
    for (const tag of draft.tags) {
      const tagItem = document.createElement("span");
      tagItem.className = "meta-tag-item";
      const label = document.createElement("span");
      label.textContent = `#${tag}`;
      const rem = document.createElement("button");
      rem.type = "button";
      rem.className = "meta-tag-remove";
      rem.title = `Remove #${tag}`;
      rem.setAttribute("aria-label", `Remove ${tag}`);
      rem.appendChild(iconNode("close", "tiny-icon"));
      rem.addEventListener("click", () => {
        draft.tags = draft.tags.filter((t) => t !== tag);
        renderMetaInline();
        markDirtyAndScheduleSave(true);
      });
      tagItem.append(label, rem);
      metaInlineEl.appendChild(tagItem);
    }

    const tagInput = document.createElement("input");
    tagInput.type = "text";
    tagInput.className = "meta-tag-input";
    tagInput.placeholder = "+ tag";
    tagInput.setAttribute("aria-label", "Add tag");
    const addTagValue = (keepFocus = false) => {
      const cleaned = tagInput.value
        .replace(/^#+/, "")
        .replace(/,/g, "")
        .trim()
        .toLowerCase();
      tagInput.value = "";
      if (cleaned && !draft.tags.includes(cleaned)) {
        draft.tags.push(cleaned);
        refocusTagInput = keepFocus;
        renderMetaInline();
        markDirtyAndScheduleSave(true);
      }
    };
    tagInput.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" || ev.key === ",") {
        ev.preventDefault();
        addTagValue(true);
      }
    });
    tagInput.addEventListener("blur", () => addTagValue(false));
    metaInlineEl.appendChild(tagInput);
    if (refocusTagInput) {
      refocusTagInput = false;
      queueMicrotask(() => tagInput.focus());
    }
  };

  // 3. Blank-Entry-Only Contextual Prompt
  blankPromptEl = document.createElement("div");
  column.appendChild(blankPromptEl);

  const renderBlankPrompt = () => {
    blankPromptEl.innerHTML = "";
    const dc = state.dailyContext;
    const events = dc?.events || [];
    const prompts = dc?.suggested_prompts || [];
    const shouldShow =
      mode === "write" &&
      !draft.body.trim() &&
      !promptDismissed &&
      (events.length > 0 || prompts.length > 0);
    if (!shouldShow) {
      blankPromptEl.hidden = true;
      return;
    }
    blankPromptEl.hidden = false;
    blankPromptEl.className = "entry-blank-prompt";

    blankPromptEl.appendChild(iconNode("spark", "tiny-icon"));
    const items = document.createElement("div");
    items.className = "blank-prompt-items";

    const chips = [
      ...events.slice(0, 2).map((ev) => ({
        label: ev.title,
        text: ev.suggested_prompt || `Reflecting on ${ev.title} — `,
      })),
      ...prompts.slice(0, 2).map((p) => ({
        label: p,
        text: `${p}\n\n`,
      })),
    ];

    for (const chip of chips) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "blank-prompt-chip";
      btn.textContent = chip.label;
      btn.addEventListener("click", () => {
        draft.body = chip.text;
        if (!draft.weather && dc?.weather_hint) {
          draft.weather = { ...dc.weather_hint };
          renderMetaInline();
        }
        promptDismissed = true;
        renderBlankPrompt();
        if (textareaEl) {
          textareaEl.value = draft.body;
          resizeTextareaForEdit(textareaEl, true);
          textareaEl.focus();
          textareaEl.setSelectionRange(draft.body.length, draft.body.length);
        }
        markDirtyAndScheduleSave();
      });
      items.appendChild(btn);
    }

    const dismiss = document.createElement("button");
    dismiss.type = "button";
    dismiss.className = "blank-prompt-dismiss";
    dismiss.title = "Dismiss prompt";
    dismiss.setAttribute("aria-label", "Dismiss prompt");
    dismiss.appendChild(iconNode("close", "tiny-icon"));
    dismiss.addEventListener("click", () => {
      promptDismissed = true;
      renderBlankPrompt();
    });

    blankPromptEl.append(items, dismiss);
  };

  // 4. Main Prose Region + Slash Palette + Quiet Insert Bar
  proseRegionEl = document.createElement("div");
  proseRegionEl.className = "entry-prose-region";
  column.appendChild(proseRegionEl);

  slashContainer = document.createElement("div");
  column.appendChild(slashContainer);

  const insertBar = document.createElement("div");
  insertBar.className = "entry-insert-bar";
  column.appendChild(insertBar);

  inlineToolContainer = document.createElement("div");
  column.appendChild(inlineToolContainer);

  const insertTextIntoBody = (insertion, replaceStart = null, replaceEnd = null) => {
    if (!textareaEl) {
      draft.body = `${draft.body}${draft.body.endsWith("\n") || !draft.body ? "" : "\n"}${insertion}`;
      markDirtyAndScheduleSave();
      return;
    }
    const start = replaceStart ?? textareaEl.selectionStart ?? draft.body.length;
    const end = replaceEnd ?? textareaEl.selectionEnd ?? draft.body.length;
    const nextVal =
      draft.body.slice(0, start) + insertion + draft.body.slice(end);
    draft.body = nextVal;
    textareaEl.value = nextVal;
    const caret = start + insertion.length;
    textareaEl.focus();
    textareaEl.setSelectionRange(caret, caret);
    resizeTextareaForEdit(textareaEl, true);
    renderBlankPrompt();
    markDirtyAndScheduleSave();
  };

  const applySlashCommand = (item) => {
    const start = slashState ? slashState.start : null;
    const end = slashState ? slashState.end : null;
    slashState = null;
    renderSlashPalette();

    if (item.action === "line" || item.action === "block") {
      insertTextIntoBody(item.insert, start, end);
      return;
    }

    if (start !== null && end !== null && textareaEl) {
      const cleared = draft.body.slice(0, start) + draft.body.slice(end);
      draft.body = cleared;
      textareaEl.value = cleared;
      textareaEl.setSelectionRange(start, start);
      resizeTextareaForEdit(textareaEl, true);
      markDirtyAndScheduleSave();
    }

    if (item.action === "photo") {
      photoInput.click();
    } else if (item.action === "voice") {
      activeInlineTool = "voice";
      renderInlineTool();
    } else if (item.action === "stamp") {
      activeInlineTool = "stamp";
      renderInlineTool();
    } else if (item.action === "clear_stamps") {
      clearAllStamps();
    } else if (item.action === "template") {
      activeInlineTool = "template";
      renderInlineTool();
    } else if (item.action === "link") {
      activeInlineTool = "link";
      renderInlineTool();
    }
  };

  const getFilteredSlashItems = () => {
    if (!slashState) return [];
    const q = slashState.query;
    if (!q) return SLASH_COMMANDS;
    return SLASH_COMMANDS.filter(
      (item) =>
        item.cmd.toLowerCase().includes(q) ||
        item.title.toLowerCase().includes(q) ||
        item.sub.toLowerCase().includes(q)
    );
  };

  const renderSlashPalette = () => {
    slashContainer.innerHTML = "";
    if (!slashState || mode !== "write") return;

    const items = getFilteredSlashItems();
    const pop = document.createElement("div");
    pop.className = "slash-palette-popover";

    const head = document.createElement("div");
    head.className = "slash-palette-header";
    const title = document.createElement("span");
    title.className = "small-caps-meta";
    title.textContent = slashState.query
      ? `Insert /${slashState.query}`
      : "Insert block or artifact";
    const closeBtn = document.createElement("button");
    closeBtn.type = "button";
    closeBtn.className = "topbar-icon-btn";
    closeBtn.setAttribute("aria-label", "Close slash menu");
    closeBtn.appendChild(iconNode("close", "tiny-icon"));
    closeBtn.addEventListener("click", () => {
      slashState = null;
      renderSlashPalette();
    });
    head.append(title, closeBtn);
    pop.appendChild(head);

    if (items.length === 0) {
      const empty = document.createElement("div");
      empty.className = "slash-empty";
      empty.textContent = "No matching commands.";
      pop.appendChild(empty);
      slashContainer.appendChild(pop);
      return;
    }

    if (slashState.selectedIndex >= items.length) {
      slashState.selectedIndex = 0;
    }

    const list = document.createElement("div");
    list.className = "slash-palette-list";
    items.forEach((item, idx) => {
      const row = document.createElement("button");
      row.type = "button";
      row.className = `slash-palette-item${
        idx === slashState.selectedIndex ? " is-selected" : ""
      }`;

      const glyph = document.createElement("span");
      glyph.className = "slash-item-glyph";
      glyph.textContent = item.glyph;

      const textWrap = document.createElement("div");
      textWrap.className = "slash-item-text";
      const tEl = document.createElement("span");
      tEl.className = "slash-item-title";
      tEl.textContent = item.title;
      const subEl = document.createElement("span");
      subEl.className = "slash-item-sub";
      subEl.textContent = item.sub;
      textWrap.append(tEl, subEl);

      const cmdEl = document.createElement("span");
      cmdEl.className = "slash-item-cmd";
      cmdEl.textContent = item.cmd;

      row.append(glyph, textWrap, cmdEl);
      row.addEventListener("mousedown", (ev) => {
        ev.preventDefault();
        applySlashCommand(item);
      });
      list.appendChild(row);
    });

    pop.appendChild(list);
    slashContainer.appendChild(pop);
  };

  const renderInlineTool = () => {
    destroyActiveRecorder();
    inlineToolContainer.innerHTML = "";
    if (!activeInlineTool || mode !== "write") return;

    if (activeInlineTool === "voice") {
      const recorder = createVoiceRecorder(
        (memo) => {
          draft.audio.push(memo);
          activeInlineTool = null;
          renderInlineTool();
          renderVoiceSection();
          markDirtyAndScheduleSave(true);
        },
        () => {
          activeInlineTool = null;
          renderInlineTool();
        }
      );
      activeRecorderCard = recorder;
      inlineToolContainer.appendChild(recorder);
      return;
    }

    if (activeInlineTool === "stamp") {
      renderStampPickerPopover(
        inlineToolContainer,
        state.manifest || { stickers: [], stamps: [], washi: [] },
        (item) => {
          const id =
            typeof crypto !== "undefined" && crypto.randomUUID
              ? crypto.randomUUID()
              : `stamp-${Date.now()}`;
          const idx = draft.stamps.length;
          const offset = (idx * 13) % 52;
          draft.stamps.push({
            id,
            asset_id: item.id,
            asset_path: item.path,
            x_pct: idx % 2 === 0 ? 89 : 11,
            y_pct: 14 + offset,
            rotation_deg: Math.round((Math.random() * 12 - 6) * 10) / 10,
            scale: 1.0,
            label: item.label || null,
          });
          justPlacedStampId = id;
          syncStampOverlay();
          renderInsertBar();
          renderLedger();
          renderInlineTool();
          markDirtyAndScheduleSave();
        },
        () => {
          activeInlineTool = null;
          renderInlineTool();
        },
        draft.stamps,
        removeStampById,
        clearAllStamps,
        stampPickerTab,
        (nextTab) => {
          stampPickerTab = nextTab;
        }
      );
      return;
    }

    const card = document.createElement("div");
    card.className = "inline-picker-card";

    const head = document.createElement("div");
    head.className = "inline-picker-header";
    const heading = document.createElement("span");
    heading.className = "small-caps-meta";
    heading.textContent =
      activeInlineTool === "template"
        ? "Apply Editorial Template"
        : activeInlineTool === "save_template"
          ? "Save Entry as Reusable Template"
          : "Link Past Entry";
    const closeBtn = document.createElement("button");
    closeBtn.type = "button";
    closeBtn.className = "topbar-icon-btn";
    closeBtn.appendChild(iconNode("close", "tiny-icon"));
    closeBtn.addEventListener("click", () => {
      activeInlineTool = null;
      renderInlineTool();
    });
    head.append(heading, closeBtn);
    card.appendChild(head);

    if (activeInlineTool === "save_template") {
      const form = document.createElement("form");
      form.className = "save-template-form";

      const nameInput = document.createElement("input");
      nameInput.type = "text";
      nameInput.className = "editorial-input";
      nameInput.placeholder = "Template name (e.g. Weekly Review)";
      nameInput.value = draft.titleInput.trim() || "";
      nameInput.required = true;

      const descInput = document.createElement("input");
      descInput.type = "text";
      descInput.className = "editorial-input";
      descInput.placeholder = "Short description (optional)";

      const catInput = document.createElement("input");
      catInput.type = "text";
      catInput.className = "editorial-input";
      catInput.placeholder = "Category (e.g. Daily, Review, Travel)";
      catInput.value = "Custom";

      const errEl = document.createElement("p");
      errEl.className = "form-error";
      errEl.hidden = true;

      const actionsRow = document.createElement("div");
      actionsRow.className = "date-popover-actions";
      const submitBtn = document.createElement("button");
      submitBtn.type = "submit";
      submitBtn.className = "btn-editorial btn-sage small";
      submitBtn.textContent = "Save Template";
      const cancelBtn = document.createElement("button");
      cancelBtn.type = "button";
      cancelBtn.className = "btn-editorial btn-ghost small";
      cancelBtn.textContent = "Cancel";
      cancelBtn.addEventListener("click", () => {
        activeInlineTool = null;
        renderInlineTool();
      });
      actionsRow.append(submitBtn, cancelBtn);

      form.append(nameInput, descInput, catInput, errEl, actionsRow);
      form.addEventListener("submit", async (ev) => {
        ev.preventDefault();
        const nm = nameInput.value.trim();
        if (!nm) return;
        errEl.hidden = true;
        submitBtn.disabled = true;
        submitBtn.textContent = "Saving...";
        try {
          await callbacks.onSaveTemplate?.({
            name: nm,
            description: descInput.value.trim() || null,
            category: catInput.value.trim() || "Custom",
            default_tags: [...draft.tags],
            default_mood: draft.mood || null,
            body_markdown: draft.body || "",
            prompts: [],
          });
          activeInlineTool = null;
          renderInlineTool();
        } catch (err) {
          errEl.textContent = err?.message || "Failed to save template";
          errEl.hidden = false;
          submitBtn.disabled = false;
          submitBtn.textContent = "Save Template";
        }
      });
      card.appendChild(form);
      queueMicrotask(() => nameInput.focus());
    } else if (activeInlineTool === "template") {
      const grid = document.createElement("div");
      grid.className = "template-picker-grid";
      for (const tpl of state.templates || []) {
        const cardWrap = document.createElement("div");
        cardWrap.className = "template-choice-wrap";

        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "template-choice-card";

        const name = document.createElement("span");
        name.className = "template-choice-name";
        name.textContent = tpl.name;
        const desc = document.createElement("span");
        desc.className = "template-choice-desc";
        desc.textContent = tpl.description;
        const cat = document.createElement("span");
        cat.className = "template-choice-cat small-caps-meta";
        cat.textContent = tpl.category;

        btn.append(name, desc, cat);
        btn.addEventListener("click", () => {
          const sep = draft.body.trim() ? "\n\n" : "";
          insertTextIntoBody(`${sep}${tpl.body_markdown || ""}`);
          if (Array.isArray(tpl.default_tags)) {
            for (const t of tpl.default_tags) {
              const clean = String(t).trim().toLowerCase();
              if (clean && !draft.tags.includes(clean)) {
                draft.tags.push(clean);
              }
            }
            renderMetaInline();
          }
          if (!draft.mood && tpl.default_mood) {
            draft.mood = tpl.default_mood;
            renderMetaInline();
          }
          activeInlineTool = null;
          renderInlineTool();
        });
        cardWrap.appendChild(btn);

        if (!STARTER_TEMPLATE_IDS.has(tpl.id) && callbacks.onDeleteTemplate) {
          const delTplBtn = document.createElement("button");
          delTplBtn.type = "button";
          delTplBtn.className = "template-delete-btn";
          delTplBtn.title = `Delete "${tpl.name}" template`;
          delTplBtn.setAttribute("aria-label", `Delete ${tpl.name}`);
          delTplBtn.appendChild(iconNode("trash", "tiny-icon"));
          delTplBtn.addEventListener("click", async (ev) => {
            ev.stopPropagation();
            await callbacks.onDeleteTemplate?.(tpl.id);
            renderInlineTool();
          });
          cardWrap.appendChild(delTplBtn);
        }

        grid.appendChild(cardWrap);
      }
      card.appendChild(grid);

      if (callbacks.onSaveTemplate) {
        const footerRow = document.createElement("div");
        footerRow.className = "template-picker-footer";
        const saveCurBtn = document.createElement("button");
        saveCurBtn.type = "button";
        saveCurBtn.className = "btn-editorial btn-ghost small";
        saveCurBtn.append(
          iconNode("plus", "tiny-icon"),
          document.createTextNode("Save current entry as template")
        );
        saveCurBtn.addEventListener("click", () => {
          activeInlineTool = "save_template";
          renderInlineTool();
        });
        footerRow.appendChild(saveCurBtn);
        card.appendChild(footerRow);
      }
    } else if (activeInlineTool === "link") {
      const searchInput = document.createElement("input");
      searchInput.type = "text";
      searchInput.className = "editorial-input";
      searchInput.placeholder = "Filter entries by title or date…";
      card.appendChild(searchInput);

      const listEl = document.createElement("div");
      listEl.className = "wikilink-picker-list";
      card.appendChild(listEl);

      const fillChoices = () => {
        listEl.innerHTML = "";
        const q = searchInput.value.trim().toLowerCase();
        const matches = (state.entries || [])
          .filter((e) => e.id !== draft.id)
          .filter((e) => {
            if (!q) return true;
            return (
              String(e.title || "").toLowerCase().includes(q) ||
              String(e.date || "").toLowerCase().includes(q) ||
              String(e.snippet || "").toLowerCase().includes(q)
            );
          })
          .slice(0, 8);

        if (matches.length === 0) {
          const empty = document.createElement("div");
          empty.className = "slash-empty";
          empty.textContent = "No matching entries.";
          listEl.appendChild(empty);
          return;
        }

        for (const m of matches) {
          const row = document.createElement("button");
          row.type = "button";
          row.className = "wikilink-choice-row";
          const linkTarget =
            m.title && !isAutoDerivedTitle(m.title, m.snippet)
              ? m.title
              : `${String(m.date || "").slice(0, 10)}`;
          const tSpan = document.createElement("span");
          tSpan.className = "wikilink-choice-title";
          tSpan.textContent = linkTarget;
          const dSpan = document.createElement("span");
          dSpan.className = "small-caps-meta";
          dSpan.textContent = String(m.date || "").slice(0, 10);
          row.append(tSpan, dSpan);
          row.addEventListener("click", () => {
            insertTextIntoBody(`[[${linkTarget}]]`);
            activeInlineTool = null;
            renderInlineTool();
          });
          listEl.appendChild(row);
        }
      };

      searchInput.addEventListener("input", fillChoices);
      fillChoices();
      queueMicrotask(() => searchInput.focus());
    }

    inlineToolContainer.appendChild(card);
  };

  const renderInsertBar = () => {
    insertBar.innerHTML = "";
    if (mode !== "write") {
      insertBar.hidden = true;
      return;
    }
    insertBar.hidden = false;

    const actions = [
      {
        label: "Photo",
        ic: "camera",
        onClick: () => photoInput.click(),
      },
      {
        label: "Voice",
        ic: "mic",
        onClick: () => {
          activeInlineTool = activeInlineTool === "voice" ? null : "voice";
          renderInlineTool();
        },
      },
      {
        label: draft.stamps.length > 0 ? `Stamp (${draft.stamps.length})` : "Stamp",
        ic: "stamp",
        onClick: () => {
          activeInlineTool = activeInlineTool === "stamp" ? null : "stamp";
          renderInlineTool();
        },
      },
      {
        label: "Template",
        ic: "feather",
        onClick: () => {
          activeInlineTool =
            activeInlineTool === "template" ? null : "template";
          renderInlineTool();
        },
      },
      {
        label: "Link entry",
        ic: "link",
        onClick: () => {
          activeInlineTool = activeInlineTool === "link" ? null : "link";
          renderInlineTool();
        },
      },
      {
        label: "Slash menu",
        ic: "plus",
        onClick: () => {
          if (slashState) {
            slashState = null;
            renderSlashPalette();
          } else {
            const pos = textareaEl?.selectionStart ?? draft.body.length;
            slashState = { query: "", start: pos, end: pos, selectedIndex: 0 };
            renderSlashPalette();
          }
        },
      },
    ];

    for (const a of actions) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "insert-action-btn";
      btn.appendChild(iconNode(a.ic, "tiny-icon"));
      btn.appendChild(document.createTextNode(a.label));
      btn.addEventListener("click", a.onClick);
      insertBar.appendChild(btn);
    }
  };

  const renderProseRegion = () => {
    proseRegionEl.innerHTML = "";
    renderInsertBar();
    renderBlankPrompt();

    if (mode === "write") {
      textareaEl = document.createElement("textarea");
      textareaEl.className = "entry-prose-textarea";
      textareaEl.placeholder =
        "Begin writing… Type / for blocks, media, or templates";
      textareaEl.value = draft.body;
      textareaEl.spellcheck = true;

      textareaEl.addEventListener("input", () => {
        draft.body = textareaEl.value;
        resizeTextareaForEdit(textareaEl, false);
        renderBlankPrompt();

        const detected = detectSlashToken(textareaEl);
        if (detected) {
          const prevSel = slashState?.selectedIndex ?? 0;
          slashState = { ...detected, selectedIndex: prevSel };
        } else {
          slashState = null;
        }
        renderSlashPalette();
        markDirtyAndScheduleSave();
      });

      const wrapSelection = (before, after = before, placeholder = "text") => {
        if (!textareaEl) return;
        const start = textareaEl.selectionStart ?? 0;
        const end = textareaEl.selectionEnd ?? start;
        const val = textareaEl.value;
        const selected = val.slice(start, end);

        // Unwrap if already wrapped with identical markers
        if (
          start >= before.length &&
          val.slice(start - before.length, start) === before &&
          val.slice(end, end + after.length) === after
        ) {
          const next =
            val.slice(0, start - before.length) +
            selected +
            val.slice(end + after.length);
          draft.body = next;
          textareaEl.value = next;
          textareaEl.setSelectionRange(start - before.length, end - before.length);
          resizeTextareaForEdit(textareaEl, true);
          markDirtyAndScheduleSave();
          return;
        }

        const inner = selected || placeholder;
        const next = val.slice(0, start) + before + inner + after + val.slice(end);
        draft.body = next;
        textareaEl.value = next;
        textareaEl.setSelectionRange(
          start + before.length,
          start + before.length + inner.length
        );
        resizeTextareaForEdit(textareaEl, true);
        renderBlankPrompt();
        markDirtyAndScheduleSave();
      };

      textareaEl.addEventListener("keydown", (ev) => {
        if (slashState) {
          const items = getFilteredSlashItems();
          if (ev.key === "ArrowDown") {
            ev.preventDefault();
            if (items.length > 0) {
              slashState.selectedIndex =
                (slashState.selectedIndex + 1) % items.length;
              renderSlashPalette();
            }
            return;
          }
          if (ev.key === "ArrowUp") {
            ev.preventDefault();
            if (items.length > 0) {
              slashState.selectedIndex =
                (slashState.selectedIndex - 1 + items.length) % items.length;
              renderSlashPalette();
            }
            return;
          }
          if (ev.key === "Enter" && items.length > 0) {
            ev.preventDefault();
            applySlashCommand(items[slashState.selectedIndex] || items[0]);
            return;
          }
          if (ev.key === "Escape") {
            ev.preventDefault();
            slashState = null;
            renderSlashPalette();
            return;
          }
        }

        const mod = ev.ctrlKey || ev.metaKey;
        if (mod && !ev.altKey) {
          const key = ev.key.toLowerCase();
          if (key === "b" && !ev.shiftKey) {
            ev.preventDefault();
            wrapSelection("**", "**", "bold text");
            return;
          }
          if (key === "i" && !ev.shiftKey) {
            ev.preventDefault();
            wrapSelection("*", "*", "italic text");
            return;
          }
          if (key === "x" && ev.shiftKey) {
            ev.preventDefault();
            wrapSelection("~~", "~~", "strikethrough");
            return;
          }
          if (key === "c" && ev.shiftKey) {
            ev.preventDefault();
            wrapSelection("`", "`", "code");
            return;
          }
          if (key === "l" && ev.shiftKey) {
            ev.preventDefault();
            wrapSelection("[", "](https://)", "link text");
            return;
          }
          if (key === "t" && ev.shiftKey) {
            ev.preventDefault();
            const pos = textareaEl.selectionStart ?? 0;
            const val = textareaEl.value;
            const lineStart = val.lastIndexOf("\n", pos - 1) + 1;
            const lineEndIdx = val.indexOf("\n", pos);
            const lineEnd = lineEndIdx === -1 ? val.length : lineEndIdx;
            const line = val.slice(lineStart, lineEnd);
            let nextLine = line;
            let delta = 0;
            if (/^(\s*[-*]\s+)\[ \]\s?/.test(line)) {
              nextLine = line.replace(/^(\s*[-*]\s+)\[ \]/, "$1[x]");
            } else if (/^(\s*[-*]\s+)\[[xX]\]\s?/.test(line)) {
              nextLine = line.replace(/^(\s*[-*]\s+)\[[xX]\]\s?/, "");
              delta = nextLine.length - line.length;
            } else {
              nextLine = `- [ ] ${line}`;
              delta = 6;
            }
            const nextVal = val.slice(0, lineStart) + nextLine + val.slice(lineEnd);
            draft.body = nextVal;
            textareaEl.value = nextVal;
            const nextPos = Math.max(lineStart, pos + delta);
            textareaEl.setSelectionRange(nextPos, nextPos);
            resizeTextareaForEdit(textareaEl, true);
            markDirtyAndScheduleSave();
            return;
          }
        }

        // Smart Tab / Shift+Tab indentation inside prose
        if (ev.key === "Tab") {
          ev.preventDefault();
          const start = textareaEl.selectionStart ?? 0;
          const end = textareaEl.selectionEnd ?? start;
          const val = textareaEl.value;
          if (start === end && !ev.shiftKey) {
            insertTextIntoBody("  ", start, end);
            return;
          }
          const lineStart = val.lastIndexOf("\n", start - 1) + 1;
          const selectedBlock = val.slice(lineStart, end);
          const lines = selectedBlock.split("\n");
          let firstDelta = 0;
          let totalDelta = 0;
          const updated = lines.map((ln, idx) => {
            if (ev.shiftKey) {
              const m = /^( {1,2}|\t)/.exec(ln);
              if (m) {
                if (idx === 0) firstDelta = -m[1].length;
                totalDelta -= m[1].length;
                return ln.slice(m[1].length);
              }
              return ln;
            }
            if (idx === 0) firstDelta = 2;
            totalDelta += 2;
            return `  ${ln}`;
          });
          const nextVal = val.slice(0, lineStart) + updated.join("\n") + val.slice(end);
          draft.body = nextVal;
          textareaEl.value = nextVal;
          textareaEl.setSelectionRange(
            Math.max(lineStart, start + firstDelta),
            Math.max(lineStart, end + totalDelta)
          );
          resizeTextareaForEdit(textareaEl, true);
          markDirtyAndScheduleSave();
          return;
        }

        // Smart Enter continuation for checklists, bullet lists, numbered lists, and quotes
        if (ev.key === "Enter" && !ev.shiftKey && !mod && !ev.altKey) {
          const start = textareaEl.selectionStart ?? 0;
          const end = textareaEl.selectionEnd ?? start;
          if (start !== end) return;
          const val = textareaEl.value;
          const lineStart = val.lastIndexOf("\n", start - 1) + 1;
          const beforeCaret = val.slice(lineStart, start);

          const todoMatch = /^(\s*)([-*])\s+\[([ xX])\]\s+(.*)$/.exec(beforeCaret);
          if (todoMatch) {
            ev.preventDefault();
            if (!todoMatch[4].trim()) {
              // Empty checklist item -> exit list cleanly
              const nextVal = val.slice(0, lineStart) + val.slice(start);
              draft.body = nextVal;
              textareaEl.value = nextVal;
              textareaEl.setSelectionRange(lineStart, lineStart);
              resizeTextareaForEdit(textareaEl, true);
              markDirtyAndScheduleSave();
            } else {
              insertTextIntoBody(`\n${todoMatch[1]}${todoMatch[2]} [ ] `, start, start);
            }
            return;
          }

          const bulletMatch = /^(\s*)([-*])\s+(.*)$/.exec(beforeCaret);
          if (bulletMatch) {
            ev.preventDefault();
            if (!bulletMatch[3].trim()) {
              const nextVal = val.slice(0, lineStart) + val.slice(start);
              draft.body = nextVal;
              textareaEl.value = nextVal;
              textareaEl.setSelectionRange(lineStart, lineStart);
              resizeTextareaForEdit(textareaEl, true);
              markDirtyAndScheduleSave();
            } else {
              insertTextIntoBody(`\n${bulletMatch[1]}${bulletMatch[2]} `, start, start);
            }
            return;
          }

          const numMatch = /^(\s*)(\d+)\.\s+(.*)$/.exec(beforeCaret);
          if (numMatch) {
            ev.preventDefault();
            if (!numMatch[3].trim()) {
              const nextVal = val.slice(0, lineStart) + val.slice(start);
              draft.body = nextVal;
              textareaEl.value = nextVal;
              textareaEl.setSelectionRange(lineStart, lineStart);
              resizeTextareaForEdit(textareaEl, true);
              markDirtyAndScheduleSave();
            } else {
              const nextNum = Number.parseInt(numMatch[2], 10) + 1;
              insertTextIntoBody(`\n${numMatch[1]}${nextNum}. `, start, start);
            }
            return;
          }

          const quoteMatch = /^(\s*>\s+)(.*)$/.exec(beforeCaret);
          if (quoteMatch && !/^\[!/.test(quoteMatch[2])) {
            ev.preventDefault();
            if (!quoteMatch[2].trim()) {
              const nextVal = val.slice(0, lineStart) + val.slice(start);
              draft.body = nextVal;
              textareaEl.value = nextVal;
              textareaEl.setSelectionRange(lineStart, lineStart);
              resizeTextareaForEdit(textareaEl, true);
              markDirtyAndScheduleSave();
            } else {
              insertTextIntoBody(`\n${quoteMatch[1]}`, start, start);
            }
          }
        }
      });

      proseRegionEl.appendChild(textareaEl);
      requestAnimationFrame(() => resizeTextareaForEdit(textareaEl, true));
    } else {
      textareaEl = null;
      slashState = null;
      renderSlashPalette();
      renderInlineTool();

      const readDiv = document.createElement("div");
      readDiv.className = "entry-reading-prose";
      renderMarkdownReadView(
        readDiv,
        draft.body,
        state.entries || [],
        (targetId, targetTitle) => {
          if (targetId) {
            callbacks.onOpenEntry?.(targetId);
          } else if (targetTitle) {
            callbacks.onSearchText?.(targetTitle);
          }
        },
        (lineIdx) => {
          const lines = draft.body.replace(/\r\n/g, "\n").split("\n");
          const line = lines[lineIdx];
          if (typeof line !== "string") return;
          if (/^(\s*[-*]\s+)\[ \]/.test(line)) {
            lines[lineIdx] = line.replace(/^(\s*[-*]\s+)\[ \]/, "$1[x]");
          } else if (/^(\s*[-*]\s+)\[[xX]\]/.test(line)) {
            lines[lineIdx] = line.replace(/^(\s*[-*]\s+)\[[xX]\]/, "$1[ ]");
          }
          draft.body = lines.join("\n");
          renderProseRegion();
          markDirtyAndScheduleSave();
        },
        (rawSrc, alt) => {
          openPolaroidLightbox([{ url: rawSrc, caption: alt || null }], 0);
        }
      );
      proseRegionEl.appendChild(readDiv);
    }
  };

  // 5. Below-the-Fold Sections
  const belowFold = document.createElement("div");
  belowFold.className = "entry-below-fold";
  column.appendChild(belowFold);

  photosContainer = document.createElement("section");
  voiceContainer = document.createElement("section");
  ledgerContainer = document.createElement("section");
  connectionsContainer = document.createElement("section");
  belowFold.append(
    photosContainer,
    voiceContainer,
    ledgerContainer,
    connectionsContainer
  );

  const renderPhotosSection = () => {
    photosContainer.innerHTML = "";
    if (draft.photos.length === 0) {
      photosContainer.hidden = true;
      return;
    }
    photosContainer.hidden = false;
    photosContainer.className = "entry-detail-section";

    const head = document.createElement("div");
    head.className = "detail-section-header";
    const titleWrap = document.createElement("div");
    titleWrap.className = "detail-section-title";
    const h = document.createElement("h3");
    h.className = "detail-section-heading";
    h.textContent = "Photographs";
    const cnt = document.createElement("span");
    cnt.className = "small-caps-meta";
    cnt.textContent = String(draft.photos.length);
    titleWrap.append(h, cnt);

    const addBtn = document.createElement("button");
    addBtn.type = "button";
    addBtn.className = "insert-action-btn";
    addBtn.appendChild(iconNode("plus", "tiny-icon"));
    addBtn.appendChild(document.createTextNode("Add photo"));
    addBtn.addEventListener("click", () => photoInput.click());

    head.append(titleWrap, addBtn);
    photosContainer.appendChild(head);

    const grid = document.createElement("div");
    grid.className = `entry-photos-grid${
      draft.photos.length === 1 ? " is-single" : ""
    }`;

    draft.photos.forEach((photo, idx) => {
      const card = createPolaroidElement(photo, idx + 1, {
        onOpenLightbox: () => {
          openPolaroidLightbox(draft.photos, idx);
        },
        onRemove: (photoId) => {
          draft.photos = draft.photos.filter((p) => p.id !== photoId);
          renderPhotosSection();
          markDirtyAndScheduleSave(true);
        },
        onUpdateCaption: (photoId, newCaption) => {
          const target = draft.photos.find((p) => p.id === photoId);
          if (target) {
            target.caption = newCaption || null;
            markDirtyAndScheduleSave();
          }
        },
      });
      grid.appendChild(card);
    });
    photosContainer.appendChild(grid);
  };

  const renderVoiceSection = () => {
    destroyActiveWaveforms();
    voiceContainer.innerHTML = "";
    if (draft.audio.length === 0) {
      voiceContainer.hidden = true;
      return;
    }
    voiceContainer.hidden = false;
    voiceContainer.className = "entry-detail-section";

    const head = document.createElement("div");
    head.className = "detail-section-header";
    const titleWrap = document.createElement("div");
    titleWrap.className = "detail-section-title";
    const h = document.createElement("h3");
    h.className = "detail-section-heading";
    h.textContent = "Voice Notes";
    const cnt = document.createElement("span");
    cnt.className = "small-caps-meta";
    cnt.textContent = String(draft.audio.length);
    titleWrap.append(h, cnt);

    const addBtn = document.createElement("button");
    addBtn.type = "button";
    addBtn.className = "insert-action-btn";
    addBtn.appendChild(iconNode("mic", "tiny-icon"));
    addBtn.appendChild(document.createTextNode("Record"));
    addBtn.addEventListener("click", () => {
      if (mode !== "write") {
        mode = "write";
        callbacks.onModeChange?.("write");
        renderTopbar();
        renderTitleArea();
        renderProseRegion();
      }
      activeInlineTool = "voice";
      renderInlineTool();
      inlineToolContainer.scrollIntoView({ behavior: "smooth", block: "nearest" });
    });

    head.append(titleWrap, addBtn);
    voiceContainer.appendChild(head);

    const stack = document.createElement("div");
    stack.className = "voice-ribbons-stack";
    for (const memo of draft.audio) {
      const player = createWaveformPlayer(memo, (memoId) => {
        draft.audio = draft.audio.filter((m) => m.id !== memoId);
        renderVoiceSection();
        markDirtyAndScheduleSave(true);
      });
      activeWaveformRibbons.push(player);
      stack.appendChild(player);
    }
    voiceContainer.appendChild(stack);
  };

  const renderLedger = () => {
    ledgerContainer.innerHTML = "";
    ledgerContainer.className = "entry-detail-section";

    const head = document.createElement("div");
    head.className = "detail-section-header";
    const h = document.createElement("h3");
    h.className = "detail-section-heading";
    h.textContent = "Entry Details";
    head.appendChild(h);
    ledgerContainer.appendChild(head);

    const dl = document.createElement("dl");
    dl.className = "entry-ledger-grid";

    const words = countWords(draft.body);
    const mins = Math.max(1, Math.ceil(words / 220));
    const nb = (state.notebooks || []).find((n) => n.id === draft.notebook_id);
    const parsedDate = new Date(draft.date);
    const isoHuman = !Number.isNaN(parsedDate.getTime())
      ? parsedDate.toLocaleString()
      : draft.date;

    const cells = [
      {
        term: "Date & Time",
        val: isoHuman,
        sub: draft.date,
      },
      {
        term: "Length",
        val: `${words.toLocaleString()} words · ${mins} min read`,
        sub: `${draft.photos.length} photos · ${draft.audio.length} voice notes`,
      },
      {
        term: "Notebook",
        val: nb?.name || draft.notebook_id,
        sub: draft.mood ? `Mood: ${draft.mood}` : null,
      },
      {
        term: "Vault File",
        val: draft.path || "Unsaved draft",
        sub: draft.id ? `ID: ${draft.id}` : null,
      },
      {
        term: "Place & Weather",
        val: draft.location?.place_name || "—",
        sub: draft.weather
          ? `${draft.weather.condition}${
              typeof draft.weather.temperature_c === "number"
                ? ` (${Math.round(draft.weather.temperature_c)}°C)`
                : ""
            }`
          : null,
      },
    ];

    for (const c of cells) {
      const cell = document.createElement("div");
      cell.className = "ledger-cell";
      const dt = document.createElement("dt");
      dt.className = "ledger-term";
      dt.textContent = c.term;
      const dd = document.createElement("dd");
      dd.className = "ledger-val";
      dd.textContent = c.val;
      if (c.sub) {
        const sub = document.createElement("span");
        sub.className = "ledger-sub";
        sub.textContent = c.sub;
        dd.appendChild(sub);
      }
      cell.append(dt, dd);
      dl.appendChild(cell);
    }

    ledgerContainer.appendChild(dl);

    if (draft.stamps.length > 0) {
      const stampsBar = document.createElement("div");
      stampsBar.className = "ledger-stamps-bar";

      const stHead = document.createElement("div");
      stHead.className = "stamp-placed-head";
      const stTitle = document.createElement("span");
      stTitle.className = "small-caps-meta";
      stTitle.textContent = `Page Stamps & Stickers (${draft.stamps.length})`;
      const clearBtn = document.createElement("button");
      clearBtn.type = "button";
      clearBtn.className = "stamp-clear-all-btn";
      clearBtn.textContent = "Remove all";
      clearBtn.addEventListener("click", clearAllStamps);
      stHead.append(stTitle, clearBtn);
      stampsBar.appendChild(stHead);

      const listEl = document.createElement("div");
      listEl.className = "stamp-placed-list";
      for (const st of draft.stamps) {
        const chip = document.createElement("span");
        chip.className = "stamp-placed-chip";
        if (st.asset_path) {
          const thumb = document.createElement("img");
          thumb.className = "stamp-placed-thumb";
          thumb.src = resolveVaultUrl(st.asset_path);
          thumb.alt = "";
          chip.appendChild(thumb);
        }
        const nm = document.createElement("span");
        nm.className = "stamp-placed-name";
        nm.textContent = st.label || st.asset_id || "Stamp";
        const rm = document.createElement("button");
        rm.type = "button";
        rm.className = "stamp-placed-remove";
        rm.title = `Remove ${nm.textContent}`;
        rm.setAttribute("aria-label", `Remove ${nm.textContent}`);
        rm.appendChild(iconNode("close", "tiny-icon"));
        rm.addEventListener("click", () => removeStampById(st.id));
        chip.append(nm, rm);
        listEl.appendChild(chip);
      }
      stampsBar.appendChild(listEl);
      ledgerContainer.appendChild(stampsBar);
    }
  };

  const renderConnections = () => {
    connectionsContainer.innerHTML = "";
    const outgoing = extractWikilinks(draft.body);
    const incoming = draft.backlinks || [];
    if (outgoing.length === 0 && incoming.length === 0) {
      connectionsContainer.hidden = true;
      return;
    }
    connectionsContainer.hidden = false;
    connectionsContainer.className = "entry-detail-section";

    const head = document.createElement("div");
    head.className = "detail-section-header";
    const h = document.createElement("h3");
    h.className = "detail-section-heading";
    h.textContent = "Connections";
    head.appendChild(h);
    connectionsContainer.appendChild(head);

    if (outgoing.length > 0) {
      const group = document.createElement("div");
      group.className = "connections-group";
      const sub = document.createElement("span");
      sub.className = "connections-sublabel";
      sub.textContent = "Links in this entry";
      const bar = document.createElement("div");
      bar.className = "outgoing-links-bar";
      for (const link of outgoing) {
        const match = (state.entries || []).find(
          (e) =>
            String(e.title || "").trim().toLowerCase() ===
              link.target.toLowerCase() ||
            String(e.id || "").toLowerCase() === link.target.toLowerCase()
        );
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "entry-wikilink";
        btn.appendChild(iconNode("link", "tiny-icon"));
        btn.appendChild(document.createTextNode(link.label || link.target));
        btn.addEventListener("click", () => {
          if (match) {
            callbacks.onOpenEntry?.(match.id);
          } else {
            callbacks.onSearchText?.(link.target);
          }
        });
        bar.appendChild(btn);
      }
      group.append(sub, bar);
      connectionsContainer.appendChild(group);
    }

    if (incoming.length > 0) {
      const group = document.createElement("div");
      group.className = "connections-group";
      const sub = document.createElement("span");
      sub.className = "connections-sublabel";
      sub.textContent = "Referenced in";
      const list = document.createElement("div");
      list.className = "backlinks-list";
      for (const bl of incoming) {
        const card = document.createElement("button");
        card.type = "button";
        card.className = "backlink-card";
        const cardHead = document.createElement("div");
        cardHead.className = "backlink-card-head";
        const t = document.createElement("span");
        t.className = "backlink-title";
        t.textContent = bl.title || "Untitled entry";
        const d = document.createElement("span");
        d.className = "small-caps-meta";
        d.textContent = String(bl.date || "").slice(0, 10);
        cardHead.append(t, d);
        const snip = document.createElement("p");
        snip.className = "backlink-snippet";
        snip.textContent = bl.snippet || "";
        card.append(cardHead, snip);
        card.addEventListener("click", () =>
          callbacks.onOpenEntry?.(bl.entry_id)
        );
        list.appendChild(card);
      }
      group.append(sub, list);
      connectionsContainer.appendChild(group);
    }
  };

  renderCoverBanner();
  renderTopbar();
  renderTitleArea();
  renderMetaInline();
  renderProseRegion();
  renderPhotosSection();
  renderVoiceSection();
  renderLedger();
  renderConnections();
  syncStampOverlay();

  return {
    flushSave: async () => {
      if (editVersion > savedVersion || dirty || inFlightSave) {
        await performSave();
      }
    },
    saveNow: async () => {
      const hasAnyContent = Boolean(
        draft.id ||
          dirty ||
          editVersion > savedVersion ||
          draft.body.trim() ||
          draft.titleInput.trim() ||
          draft.photos.length > 0 ||
          draft.audio.length > 0 ||
          draft.tags.length > 0 ||
          draft.cover_asset ||
          draft.mood ||
          draft.location ||
          draft.weather
      );
      if (!hasAnyContent) {
        return false;
      }
      if (editVersion === savedVersion && !dirty) {
        editVersion += 1;
        dirty = true;
      }
      await performSave();
      return true;
    },
    syncNav: () => {
      if (!disposed) {
        updateAdjacentNavState();
      }
    },
    syncFocusState: () => {
      if (disposed) return;
      const btn = topbar.querySelector('[data-role="zen-focus-btn"]');
      if (btn) {
        btn.classList.toggle("is-active", Boolean(state.focusZen));
        btn.title = state.focusZen
          ? "Exit Zen Focus mode (Ctrl+Shift+F)"
          : "Zen Focus mode (Ctrl+Shift+F)";
        btn.setAttribute("aria-label", btn.title);
      }
    },
    openFind: () => {
      if (disposed) return;
      findOpen = true;
      renderFindBar();
    },
    goAdjacent: (dir) => {
      updateAdjacentNavState();
      if (dir === "prev" && prevEntryRef) {
        callbacks.onOpenEntry?.(prevEntryRef.id, "prev");
      } else if (dir === "next" && nextEntryRef) {
        callbacks.onOpenEntry?.(nextEntryRef.id, "next");
      }
    },
    isDirty: () => editVersion > savedVersion || dirty || Boolean(inFlightSave),
    toggleMode: () => {
      mode = mode === "write" ? "read" : "write";
      datePopoverOpen = false;
      weatherPopoverOpen = false;
      coverPopoverOpen = false;
      moreMenuOpen = false;
      renderDatePopover();
      renderWeatherPopover();
      callbacks.onModeChange?.(mode);
      renderTopbar();
      renderTitleArea();
      renderProseRegion();
      syncStampOverlay();
      if (findOpen) applyFindHighlights(false);
    },
    focusEditor: () => {
      textareaEl?.focus();
    },
    destroy: () => {
      disposed = true;
      if (saveTimer) {
        clearTimeout(saveTimer);
        saveTimer = null;
      }
      if (ledgerTimer) {
        clearTimeout(ledgerTimer);
        ledgerTimer = null;
      }
      if (shrinkRaf) {
        cancelAnimationFrame(shrinkRaf);
        shrinkRaf = null;
      }
      if (progressRaf) {
        cancelAnimationFrame(progressRaf);
        progressRaf = 0;
      }
      if (topbarPopoverAbort) {
        topbarPopoverAbort.abort();
        topbarPopoverAbort = null;
      }
      if (datePopoverAbort) {
        datePopoverAbort.abort();
        datePopoverAbort = null;
      }
      if (weatherPopoverAbort) {
        weatherPopoverAbort.abort();
        weatherPopoverAbort = null;
      }
      lifecycleAbort.abort();
      if (stampLayer._stampOverlayAbort) {
        stampLayer._stampOverlayAbort.abort();
        stampLayer._stampOverlayAbort = null;
      }
      destroyActiveRecorder();
      destroyActiveWaveforms();
    },
  };
}
