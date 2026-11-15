import { api } from "./ipc.js";
import { icon, weatherIcon } from "./icons.js";
import { renderMarkdownReadView, extractWikilinks } from "./markdown.js";
import { createVoiceRecorder, createWaveformPlayer } from "./audio.js";
import {
  isAutoDerivedTitle,
  createPolaroidElement,
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
    title: "Place Stamp or Sticker",
    sub: "Botanical seal or postmark",
    action: "stamp",
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

// Session-local optional cover banner selection keyed by entry ID
const sessionEntryCovers = new Map();

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
  let coverPopoverAbort = null;
  let promptDismissed = false;
  let activeInlineTool = null; // null | "voice" | "template" | "link" | "stamp"
  let activeRecorderCard = null;
  let activeWaveformRibbons = [];
  let slashState = null; // { query, start, end, selectedIndex }
  let locationEditing = false;
  let refocusTagInput = false;
  const lifecycleAbort = new AbortController();

  const coverKey = draft.id || "__draft__";
  let coverAsset = sessionEntryCovers.get(coverKey) || null;

  const sheet = document.createElement("article");
  sheet.className = "entry-page-sheet";
  if (state.pageTurnDir === "next") {
    sheet.classList.add("page-turn-next");
  } else if (state.pageTurnDir === "prev") {
    sheet.classList.add("page-turn-prev");
  }
  container.appendChild(sheet);

  // Stamp overlay (sits over sheet)
  const stampLayer = document.createElement("div");
  sheet.appendChild(stampLayer);

  const syncStampOverlay = () => {
    renderStampOverlay(
      stampLayer,
      draft.stamps,
      mode === "write",
      (updatedStamps) => {
        draft.stamps = updatedStamps;
        markDirtyAndScheduleSave();
      }
    );
  };

  // Optional cover banner container (populated only if user selects one)
  const coverContainer = document.createElement("div");
  sheet.appendChild(coverContainer);

  const renderCoverBanner = () => {
    coverContainer.innerHTML = "";
    if (!coverAsset) return;
    const banner = document.createElement("div");
    banner.className = "entry-cover-banner";
    const img = document.createElement("img");
    img.className = "entry-cover-img";
    img.src = coverAsset;
    img.alt = "Entry watercolor cover";

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "entry-cover-remove";
    removeBtn.textContent = "Remove cover";
    removeBtn.addEventListener("click", () => {
      coverAsset = null;
      sessionEntryCovers.delete(coverKey);
      renderCoverBanner();
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
      body: draft.body,
    };
  };

  const updateSaveIndicator = () => {
    if (!saveIndicatorEl) return;
    saveIndicatorEl.classList.toggle("is-dirty", dirty || saving);
    if (saving) {
      saveIndicatorEl.textContent = "Saving…";
    } else if (dirty) {
      saveIndicatorEl.textContent = "Unsaved";
    } else {
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
            if (hadNoId && coverAsset) {
              sessionEntryCovers.delete("__draft__");
              sessionEntryCovers.set(saved.id, coverAsset);
            }
          }
          savedVersion = targetVersion;
          dirty = editVersion > savedVersion;
        } catch (err) {
          saving = false;
          if (saveIndicatorEl) {
            saveIndicatorEl.classList.add("is-dirty");
            saveIndicatorEl.textContent = err?.message || "Save failed";
          }
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
    }, 750);
  };

  window.addEventListener(
    "beforeunload",
    () => {
      if (editVersion > savedVersion || dirty) {
        performSave().catch(() => {});
      }
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
    const list = Array.from(files || []).filter((f) =>
      String(f.type || "").startsWith("image/") ||
      /\.(png|jpe?g|webp|gif)$/i.test(f.name || "")
    );
    if (!list.length) return;
    for (const file of list) {
      try {
        const photo = await api.uploadPhoto(file, null);
        draft.photos.push(photo);
      } catch (err) {
        if (saveIndicatorEl) {
          saveIndicatorEl.classList.add("is-dirty");
          saveIndicatorEl.textContent = err?.message || "Photo upload failed";
        }
      }
    }
    renderPhotosSection();
    markDirtyAndScheduleSave(true);
  };

  photoInput.addEventListener("change", async () => {
    const files = photoInput.files;
    await handlePhotoFiles(files);
    photoInput.value = "";
  });

  // Drag & drop photo attach
  sheet.addEventListener("dragover", (ev) => {
    if (ev.dataTransfer?.types?.includes("Files")) {
      ev.preventDefault();
      sheet.classList.add("is-drag-over");
    }
  });
  sheet.addEventListener("dragleave", () => {
    sheet.classList.remove("is-drag-over");
  });
  sheet.addEventListener("drop", async (ev) => {
    sheet.classList.remove("is-drag-over");
    if (ev.dataTransfer?.files?.length) {
      ev.preventDefault();
      await handlePhotoFiles(ev.dataTransfer.files);
    }
  });

  // 1. Quiet Top Bar
  const topbar = document.createElement("div");
  topbar.className = "entry-topbar";
  column.appendChild(topbar);

  const renderTopbar = () => {
    topbar.innerHTML = "";
    const left = document.createElement("div");
    left.className = "entry-topbar-left";

    const list = state.filteredEntries ?? state.entries ?? [];
    const currentIdx = draft.id
      ? list.findIndex((e) => e.id === draft.id)
      : -1;
    const prevEntry =
      currentIdx >= 0 && currentIdx + 1 < list.length
        ? list[currentIdx + 1]
        : null;
    const nextEntry = currentIdx > 0 ? list[currentIdx - 1] : null;

    const prevBtn = document.createElement("button");
    prevBtn.type = "button";
    prevBtn.className = "topbar-ghost-btn";
    prevBtn.disabled = !prevEntry;
    prevBtn.title = "Previous entry";
    prevBtn.appendChild(iconNode("chevronLeft", "tiny-icon"));
    prevBtn.appendChild(document.createTextNode("Prev"));
    prevBtn.addEventListener("click", () => {
      if (prevEntry) callbacks.onOpenEntry?.(prevEntry.id, "prev");
    });

    const nextBtn = document.createElement("button");
    nextBtn.type = "button";
    nextBtn.className = "topbar-ghost-btn";
    nextBtn.disabled = !nextEntry;
    nextBtn.title = "Next entry";
    nextBtn.appendChild(document.createTextNode("Next"));
    nextBtn.appendChild(iconNode("chevronRight", "tiny-icon"));
    nextBtn.addEventListener("click", () => {
      if (nextEntry) callbacks.onOpenEntry?.(nextEntry.id, "next");
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

    left.append(prevBtn, nextBtn, nbSelect);

    const right = document.createElement("div");
    right.className = "entry-topbar-right";

    saveIndicatorEl = document.createElement("span");
    saveIndicatorEl.className = "entry-save-indicator";
    updateSaveIndicator();

    const modeSwitch = document.createElement("div");
    modeSwitch.className = "entry-mode-switch";
    const writeBtn = document.createElement("button");
    writeBtn.type = "button";
    writeBtn.className = `mode-switch-btn${mode === "write" ? " is-active" : ""}`;
    writeBtn.textContent = "Write";
    writeBtn.addEventListener("click", () => {
      if (mode !== "write") {
        mode = "write";
        callbacks.onModeChange?.("write");
        renderTopbar();
        renderTitleArea();
        renderProseRegion();
        syncStampOverlay();
      }
    });

    const readBtn = document.createElement("button");
    readBtn.type = "button";
    readBtn.className = `mode-switch-btn${mode === "read" ? " is-active" : ""}`;
    readBtn.textContent = "Read";
    readBtn.addEventListener("click", () => {
      if (mode !== "read") {
        mode = "read";
        callbacks.onModeChange?.("read");
        renderTopbar();
        renderTitleArea();
        renderProseRegion();
        syncStampOverlay();
      }
    });
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
    coverBtn.className = `topbar-icon-btn${coverAsset ? " is-active" : ""}`;
    coverBtn.title = "Watercolor cover wash";
    coverBtn.setAttribute("aria-label", "Watercolor cover wash");
    coverBtn.appendChild(iconNode("spark", "tiny-icon"));
    coverBtn.addEventListener("click", () => {
      coverPopoverOpen = !coverPopoverOpen;
      renderTopbar();
    });

    right.append(saveIndicatorEl, modeSwitch, starBtn, coverBtn);

    if (draft.id) {
      const divider = document.createElement("span");
      divider.className = "topbar-divider";
      right.appendChild(divider);

      if (!deleteConfirmOpen) {
        const delBtn = document.createElement("button");
        delBtn.type = "button";
        delBtn.className = "topbar-ghost-btn topbar-delete-btn";
        delBtn.title = "Delete entry";
        delBtn.setAttribute("aria-label", "Delete entry");
        delBtn.appendChild(iconNode("trash", "tiny-icon"));
        const delLabel = document.createElement("span");
        delLabel.textContent = "Delete";
        delBtn.appendChild(delLabel);
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
        confirmBtn.textContent = "Confirm Delete";
        confirmBtn.addEventListener("click", async () => {
          if (saveTimer) {
            clearTimeout(saveTimer);
            saveTimer = null;
          }
          dirty = false;
          editVersion = savedVersion;
          disposed = true;
          confirmBtn.disabled = true;
          confirmBtn.textContent = "Deleting...";
          if (inFlightSave) {
            await inFlightSave.catch(() => {});
          }
          if (draft.id) sessionEntryCovers.delete(draft.id);
          try {
            await callbacks.onDeleteEntry?.(draft.id);
          } catch (err) {
            disposed = false;
            deleteConfirmOpen = false;
            renderTopbar();
            if (saveIndicatorEl) {
              saveIndicatorEl.classList.add("is-dirty");
              saveIndicatorEl.textContent = err?.message || "Delete failed";
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

    if (coverPopoverAbort) {
      coverPopoverAbort.abort();
      coverPopoverAbort = null;
    }

    if (coverPopoverOpen) {
      const pop = document.createElement("div");
      pop.className = "cover-swatch-popover";
      const covers = state.manifest?.covers || [];
      for (const cov of covers) {
        const cBtn = document.createElement("button");
        cBtn.type = "button";
        cBtn.className = "cover-mini-btn";
        cBtn.title = cov.label || "Watercolor wash";
        const img = document.createElement("img");
        img.src = cov.path;
        img.alt = cov.label || "Cover wash";
        cBtn.appendChild(img);
        cBtn.addEventListener("click", () => {
          coverAsset = cov.path;
          sessionEntryCovers.set(coverKey, coverAsset);
          coverPopoverOpen = false;
          renderCoverBanner();
          renderTopbar();
        });
        pop.appendChild(cBtn);
      }
      if (coverAsset) {
        const clearCoverBtn = document.createElement("button");
        clearCoverBtn.type = "button";
        clearCoverBtn.className = "topbar-ghost-btn";
        clearCoverBtn.style.gridColumn = "1 / -1";
        clearCoverBtn.textContent = "Remove cover";
        clearCoverBtn.addEventListener("click", () => {
          coverAsset = null;
          sessionEntryCovers.delete(coverKey);
          coverPopoverOpen = false;
          renderCoverBanner();
          renderTopbar();
        });
        pop.appendChild(clearCoverBtn);
      }
      topbar.appendChild(pop);

      coverPopoverAbort = new AbortController();
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
        { signal: coverPopoverAbort.signal }
      );
      window.addEventListener(
        "keydown",
        (ev) => {
          if (ev.key === "Escape") closePop();
        },
        { signal: coverPopoverAbort.signal }
      );
    }
  };

  // 2. Entry Header (Dateline, Optional Title, Inline Metadata)
  const header = document.createElement("header");
  header.className = "entry-header";
  column.appendChild(header);

  const dateline = document.createElement("div");
  dateline.className = "entry-dateline";
  datelineTextEl = document.createElement("span");
  datelineTextEl.className = "small-caps-meta";
  datelineTextEl.textContent = formatEntryDateLine(draft.date, draft.body);
  dateline.appendChild(datelineTextEl);

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
  header.appendChild(dateline);

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
  header.appendChild(metaInlineEl);

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
      wxBtn.title = "Click to clear weather";
      wxBtn.addEventListener("click", () => {
        draft.weather = null;
        renderMetaInline();
        markDirtyAndScheduleSave();
      });
      wxWrap.appendChild(wxBtn);
    } else {
      wxWrap.appendChild(weatherIconNode("cloud", "tiny-icon"));
      const addWxBtn = document.createElement("button");
      addWxBtn.type = "button";
      addWxBtn.className = "meta-inline-btn";
      addWxBtn.textContent = "+ Weather";
      addWxBtn.addEventListener("click", () => {
        const hint = state.dailyContext?.weather_hint;
        draft.weather = hint
          ? { ...hint }
          : {
              condition: "Clear",
              temperature_c: 21,
              icon_key: "sun",
              humidity_pct: null,
              summary: "Clear sky",
            };
        renderMetaInline();
        markDirtyAndScheduleSave();
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
        state.manifest || { stickers: [], stamps: [] },
        (item) => {
          const id =
            typeof crypto !== "undefined" && crypto.randomUUID
              ? crypto.randomUUID()
              : `stamp-${Date.now()}`;
          const offset = (draft.stamps.length * 11) % 48;
          draft.stamps.push({
            id,
            asset_id: item.id,
            asset_path: item.path,
            x_pct: 78,
            y_pct: 14 + offset,
            rotation_deg: Math.round((Math.random() * 12 - 6) * 10) / 10,
            scale: 1.0,
            label: item.label || null,
          });
          syncStampOverlay();
          markDirtyAndScheduleSave();
        },
        () => {
          activeInlineTool = null;
          renderInlineTool();
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

    if (activeInlineTool === "template") {
      const grid = document.createElement("div");
      grid.className = "template-picker-grid";
      for (const tpl of state.templates || []) {
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
        grid.appendChild(btn);
      }
      card.appendChild(grid);
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
        label: "Stamp",
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

      textareaEl.addEventListener("keydown", (ev) => {
        if (!slashState) return;
        const items = getFilteredSlashItems();
        if (ev.key === "ArrowDown") {
          ev.preventDefault();
          if (items.length > 0) {
            slashState.selectedIndex =
              (slashState.selectedIndex + 1) % items.length;
            renderSlashPalette();
          }
        } else if (ev.key === "ArrowUp") {
          ev.preventDefault();
          if (items.length > 0) {
            slashState.selectedIndex =
              (slashState.selectedIndex - 1 + items.length) % items.length;
            renderSlashPalette();
          }
        } else if (ev.key === "Enter" && items.length > 0) {
          ev.preventDefault();
          applySlashCommand(items[slashState.selectedIndex] || items[0]);
        } else if (ev.key === "Escape") {
          ev.preventDefault();
          slashState = null;
          renderSlashPalette();
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
    isDirty: () => editVersion > savedVersion || dirty || Boolean(inFlightSave),
    toggleMode: () => {
      mode = mode === "write" ? "read" : "write";
      callbacks.onModeChange?.(mode);
      renderTopbar();
      renderTitleArea();
      renderProseRegion();
      syncStampOverlay();
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
      if (coverPopoverAbort) {
        coverPopoverAbort.abort();
        coverPopoverAbort = null;
      }
      lifecycleAbort.abort();
      destroyActiveRecorder();
      destroyActiveWaveforms();
    },
  };
}
