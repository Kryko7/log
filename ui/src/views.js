// Pure ES-module editorial views, tactile Polaroid cards, stamps/stickers,
// search modal, archive migration modal, and encrypted backup modal.

import { icon, weatherIcon } from "./icons.js";
import { resolveVaultUrl } from "./ipc.js";

function escapeHtml(str) {
  return String(str ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

const modalAbortMap = new WeakMap();

function bindModalEscape(container, backdrop, onClose) {
  const prev = modalAbortMap.get(container);
  if (prev) prev.abort();

  const ac = new AbortController();
  modalAbortMap.set(container, ac);

  const close = () => {
    ac.abort();
    if (modalAbortMap.get(container) === ac) {
      modalAbortMap.delete(container);
    }
    onClose?.();
  };

  window.addEventListener(
    "keydown",
    (ev) => {
      if (!container.isConnected || !backdrop.isConnected) {
        ac.abort();
        return;
      }
      if (ev.key === "Escape") {
        close();
      }
    },
    { signal: ac.signal }
  );

  return close;
}

function safeAssetUrl(rawUrl) {
  const s = String(rawUrl || "").trim();
  if (!s) return "";
  if (
    s.startsWith("/assets/") ||
    s.startsWith("assets/") ||
    s.startsWith("/vault-media/") ||
    s.startsWith("/vault-thumbs/") ||
    s.startsWith("vault://") ||
    s.startsWith("http://vault.localhost/")
  ) {
    return resolveVaultUrl(s);
  }
  return "";
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
}

/**
 * Strip inline Markdown image tags `![alt](url)` from prose.
 * @param {string} input
 * @returns {string}
 */
export function stripMarkdownImages(input) {
  const text = String(input ?? "");
  return text.replace(/!\[[^\n\]]*\]\([^\n)]*\)/g, "");
}

/**
 * Clean an entry excerpt for timeline and memory rows.
 * @param {string} text
 * @returns {string}
 */
export function cleanExcerptSnippet(text) {
  return stripMarkdownImages(text).replace(/\s+/g, " ").trim();
}

function normalizeProsePrefix(input) {
  const withoutImages = stripMarkdownImages(input);
  const lines = withoutImages.split(/\r?\n/);
  const parts = [];
  for (const rawLine of lines) {
    const trimmed = rawLine.trim().replace(/^[#*>\-\s]+/, "");
    const cleaned = trimmed.replace(/[*_`]/g, "").replace(/\s+/g, " ").trim().toLowerCase();
    if (cleaned) parts.push(cleaned);
  }
  return parts.join(" ").replace(/\s+/g, " ").trim();
}

/**
 * Returns true when an entry title is blank, a generic placeholder, an image tag
 * artifact, or auto-derived from the opening words of the entry body/snippet.
 * @param {string} title
 * @param {string} bodyOrSnippet
 * @returns {boolean}
 */
export function isAutoDerivedTitle(title, bodyOrSnippet) {
  const raw = String(title ?? "").trim();
  if (
    !raw ||
    /^untitled (entry|folio)$/i.test(raw) ||
    (raw.startsWith("![") && raw.includes("]("))
  ) {
    return true;
  }

  const strippedTitle = raw.replace(/(?:…|\.\.\.)+$/, "").trim();
  const normTitle = normalizeProsePrefix(strippedTitle);
  if (normTitle.length < 10) {
    return false;
  }

  const normBody = normalizeProsePrefix(bodyOrSnippet);
  if (!normBody) {
    return false;
  }

  if (normBody.startsWith(normTitle)) {
    return true;
  }

  const prefix = normTitle.slice(0, 24);
  return prefix.length >= 10 && normBody.startsWith(prefix);
}

function formatMonthYear(year, month) {
  const dt = new Date(year, month - 1, 1);
  return new Intl.DateTimeFormat("en-US", {
    month: "long",
    year: "numeric",
  }).format(dt);
}

function formatBytes(bytes) {
  const n = Number(bytes || 0);
  if (n >= 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(2)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
}

function formatDateParts(isoString) {
  const dt = new Date(isoString);
  if (Number.isNaN(dt.getTime())) {
    return { day: "--", sub: "---", time: "--:--", full: String(isoString || "") };
  }
  const day = String(dt.getDate()).padStart(2, "0");
  const mon = new Intl.DateTimeFormat("en-US", { month: "short" })
    .format(dt)
    .toUpperCase();
  const wk = new Intl.DateTimeFormat("en-US", { weekday: "short" })
    .format(dt)
    .toUpperCase();
  const hh = String(dt.getHours()).padStart(2, "0");
  const mm = String(dt.getMinutes()).padStart(2, "0");
  const full = new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "2-digit",
    year: "numeric",
  }).format(dt);
  return { day, sub: `${mon} · ${wk}`, time: `${hh}:${mm}`, full };
}

function buildTimelineRow(entry, notebookName, callbacks, subMode = "weekday") {
  const article = el("article", "timeline-entry-row");
  article.addEventListener("click", () => callbacks.onOpenEntry?.(entry.id));

  const parts = formatDateParts(entry.date);
  const dateCol = el("div", "timeline-date-col");
  dateCol.appendChild(el("span", "timeline-day-num", parts.day));
  const subText =
    subMode === "year"
      ? `${parts.sub.split(" · ")[0]} · ${new Date(entry.date).getFullYear()}`
      : parts.sub;
  dateCol.appendChild(el("span", "timeline-day-sub small-caps-meta", subText));
  dateCol.appendChild(el("span", "timeline-day-time", parts.time));
  article.appendChild(dateCol);

  const proseCol = el("div", "timeline-prose-col");
  const cleanSnippet = cleanExcerptSnippet(entry.snippet);
  const showTitle = !isAutoDerivedTitle(entry.title, cleanSnippet);

  if (showTitle) {
    proseCol.appendChild(el("h3", "timeline-row-title", entry.title));
  }
  if (cleanSnippet) {
    proseCol.appendChild(el("p", "timeline-row-excerpt", cleanSnippet));
  }

  const meta = el("div", "timeline-row-meta");
  if (notebookName) {
    meta.appendChild(el("span", "meta-text-item", notebookName));
  }
  if (entry.location?.place_name) {
    const locSpan = el("span", "meta-text-item");
    locSpan.innerHTML = icon("pin", "tiny-icon");
    locSpan.appendChild(el("span", "", entry.location.place_name));
    meta.appendChild(locSpan);
  }
  if (entry.weather) {
    const wSpan = el("span", "meta-text-item");
    wSpan.innerHTML = weatherIcon(entry.weather.icon_key, "tiny-icon");
    const temp =
      entry.weather.temperature_c !== null && entry.weather.temperature_c !== undefined
        ? ` ${Math.round(entry.weather.temperature_c)}°C`
        : "";
    wSpan.appendChild(el("span", "", `${entry.weather.condition}${temp}`));
    meta.appendChild(wSpan);
  }
  if (Array.isArray(entry.tags)) {
    for (const tag of entry.tags.slice(0, 4)) {
      const tagBtn = el("button", "inline-tag-link", `#${tag}`);
      tagBtn.type = "button";
      tagBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        callbacks.onSelectTag?.(tag);
      });
      meta.appendChild(tagBtn);
    }
  }
  if (entry.reading_minutes > 0) {
    meta.appendChild(el("span", "meta-text-item", `${entry.reading_minutes} min read`));
  }
  if (entry.has_audio) {
    const aSpan = el("span", "meta-text-item");
    aSpan.title = "Has voice note";
    aSpan.innerHTML = icon("mic", "tiny-icon");
    meta.appendChild(aSpan);
  }
  if (Array.isArray(entry.stamps) && entry.stamps.length > 0) {
    const sSpan = el("span", "meta-text-item");
    sSpan.title = "Has stamps";
    sSpan.innerHTML = icon("stamp", "tiny-icon");
    meta.appendChild(sSpan);
  }
  if (entry.starred) {
    const starSpan = el("span", "meta-star-mark");
    starSpan.title = "Starred";
    starSpan.innerHTML = icon("starFilled", "tiny-icon");
    meta.appendChild(starSpan);
  }

  proseCol.appendChild(meta);
  article.appendChild(proseCol);

  if (entry.cover_photo) {
    const thumbWrap = el("div", "timeline-photo-thumb");
    const img = document.createElement("img");
    img.src = resolveVaultUrl(entry.cover_photo);
    img.alt = "Entry photograph";
    img.loading = "lazy";
    img.decoding = "async";
    thumbWrap.appendChild(img);
    article.appendChild(thumbWrap);
  }

  if (callbacks.onDeleteEntry) {
    const actionsSlot = el("div", "timeline-row-actions");
    const renderDeleteTrigger = () => {
      actionsSlot.innerHTML = "";
      const delBtn = el("button", "timeline-row-delete-btn");
      delBtn.type = "button";
      delBtn.title = "Delete entry";
      delBtn.setAttribute("aria-label", "Delete entry");
      delBtn.innerHTML = icon("trash", "tiny-icon");
      delBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        renderDeleteConfirm();
      });
      actionsSlot.appendChild(delBtn);
    };
    const renderDeleteConfirm = () => {
      actionsSlot.innerHTML = "";
      const confirmWrap = el("div", "timeline-delete-confirm");
      confirmWrap.addEventListener("click", (ev) => ev.stopPropagation());

      const yesBtn = el("button", "btn-editorial btn-terracotta small", "Delete?");
      yesBtn.type = "button";
      const cancelBtn = el("button", "btn-editorial btn-ghost small", "Cancel");
      cancelBtn.type = "button";

      yesBtn.addEventListener("click", async (ev) => {
        ev.stopPropagation();
        yesBtn.disabled = true;
        cancelBtn.disabled = true;
        yesBtn.textContent = "Deleting...";
        try {
          await callbacks.onDeleteEntry(entry.id);
        } catch {
          if (actionsSlot.isConnected) renderDeleteTrigger();
        }
      });
      cancelBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        renderDeleteTrigger();
      });

      confirmWrap.append(yesBtn, cancelBtn);
      actionsSlot.appendChild(confirmWrap);
    };
    renderDeleteTrigger();
    article.appendChild(actionsSlot);
  }

  return article;
}

const NOTEBOOK_ACCENT_PALETTE = [
  "#546E5A",
  "#B5563E",
  "#6D6157",
  "#4A6670",
  "#8C6B4A",
];

/**
 * Render the clean editorial river Timeline grouped by month.
 * @param {HTMLElement} container
 * @param {object} state
 * @param {object} callbacks
 */
export function renderTimelineView(container, state, callbacks = {}) {
  container.innerHTML = "";
  const entries = [...(state.filteredEntries ?? state.entries ?? [])];
  entries.sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime());
  const notebooks = state.notebooks ?? [];
  const nbMap = new Map(notebooks.map((nb) => [nb.id, nb.name]));

  const selectedNb = state.selectedNotebookId
    ? notebooks.find((nb) => nb.id === state.selectedNotebookId) || null
    : null;
  const selectedTag = state.selectedTag || null;
  const hasFilter = Boolean(state.selectedNotebookId || selectedTag);

  let headingTitle = "All Entries";
  let subtitlePrefix = "";
  let clearLabel = "Show all entries";
  if (selectedNb || state.selectedNotebookId) {
    headingTitle = selectedNb?.name || "Notebook";
    subtitlePrefix = selectedTag ? `Notebook · #${selectedTag}` : "Notebook";
    clearLabel = "Show all entries";
  } else if (selectedTag) {
    headingTitle = `#${selectedTag}`;
    subtitlePrefix = "Tag";
    clearLabel = "Clear filter";
  }

  const section = el("section", "timeline-river");
  section.setAttribute("aria-label", headingTitle);

  const header = el("header", "stage-header");
  const titleWrap = el("div");
  titleWrap.appendChild(el("h1", "stage-serif-title", headingTitle));
  const n = entries.length;
  const countLabel = `${n} ${n === 1 ? "entry" : "entries"}`;
  titleWrap.appendChild(
    el(
      "p",
      "stage-subtitle",
      subtitlePrefix ? `${subtitlePrefix} · ${countLabel}` : countLabel
    )
  );

  let panelMode = null;
  const panelSlot = el("div", "notebook-inline-panel-slot");
  panelSlot.hidden = true;

  function renderNotebookPanel() {
    panelSlot.innerHTML = "";
    if (!selectedNb || !panelMode) {
      panelSlot.hidden = true;
      return;
    }
    panelSlot.hidden = false;

    if (panelMode === "edit") {
      const card = el("form", "notebook-inline-card");
      let draftName = selectedNb.name || "";
      let draftCover = selectedNb.cover_asset || "";
      let draftAccent = selectedNb.accent_color || NOTEBOOK_ACCENT_PALETTE[0];
      let uploading = false;

      const topRow = el("div", "notebook-inline-card-head");
      topRow.appendChild(el("span", "small-caps-meta", "Edit Notebook"));
      card.appendChild(topRow);

      const nameInput = document.createElement("input");
      nameInput.type = "text";
      nameInput.className = "editorial-input";
      nameInput.placeholder = "Notebook name";
      nameInput.value = draftName;
      nameInput.required = true;
      nameInput.addEventListener("input", () => {
        draftName = nameInput.value;
      });
      card.appendChild(nameInput);

      const coverRow = el("div", "cover-swatch-row notebook-banner-swatches");
      const builtInCovers = (state.manifest?.covers || []).slice(0, 4);
      const renderSwatches = () => {
        coverRow.innerHTML = "";
        for (const cov of builtInCovers) {
          const sw = el(
            "button",
            `cover-swatch-btn${draftCover === cov.path ? " selected" : ""}`
          );
          sw.type = "button";
          sw.title = cov.label || "Cover wash";
          const img = document.createElement("img");
          img.src = resolveVaultUrl(cov.path);
          img.alt = cov.label || "Cover";
          img.decoding = "async";
          sw.appendChild(img);
          sw.addEventListener("click", () => {
            draftCover = cov.path;
            renderSwatches();
          });
          coverRow.appendChild(sw);
        }
        const isCustom =
          Boolean(draftCover) && !builtInCovers.some((c) => c.path === draftCover);
        if (isCustom) {
          const customSw = el("button", "cover-swatch-btn selected");
          customSw.type = "button";
          customSw.title = "Custom cover picture";
          const img = document.createElement("img");
          img.src = resolveVaultUrl(draftCover);
          img.alt = "Custom cover";
          img.decoding = "async";
          customSw.appendChild(img);
          coverRow.appendChild(customSw);
        }
        const noneSw = el(
          "button",
          `cover-swatch-btn is-none${!draftCover ? " selected" : ""}`
        );
        noneSw.type = "button";
        noneSw.title = "No cover";
        noneSw.setAttribute("aria-label", "No cover");
        noneSw.innerHTML = icon("close", "tiny-icon");
        noneSw.addEventListener("click", () => {
          draftCover = "";
          renderSwatches();
        });
        coverRow.appendChild(noneSw);
      };
      renderSwatches();
      card.appendChild(coverRow);

      const toolsRow = el("div", "notebook-inline-tools-row");
      const accentRow = el("div", "accent-swatch-row");
      for (const color of NOTEBOOK_ACCENT_PALETTE) {
        const dot = el(
          "button",
          `accent-dot-btn${draftAccent === color ? " selected" : ""}`
        );
        dot.type = "button";
        dot.style.background = color;
        dot.title = color;
        dot.addEventListener("click", () => {
          draftAccent = color;
          accentRow
            .querySelectorAll(".accent-dot-btn")
            .forEach((b) => b.classList.remove("selected"));
          dot.classList.add("selected");
        });
        accentRow.appendChild(dot);
      }
      toolsRow.appendChild(accentRow);

      const errEl = el("p", "form-error", "");
      errEl.hidden = true;

      if (callbacks.onUploadNotebookCover) {
        const uploadLabel = el("label", "btn-editorial btn-ghost small cover-upload-btn");
        uploadLabel.innerHTML = `${icon("image", "tiny-icon")}<span>Upload picture</span>`;
        const fileInput = document.createElement("input");
        fileInput.type = "file";
        fileInput.accept = "image/*";
        fileInput.className = "sr-only";
        fileInput.addEventListener("change", async () => {
          const file = fileInput.files?.[0];
          fileInput.value = "";
          if (!file || uploading) return;
          uploading = true;
          errEl.hidden = true;
          const span = uploadLabel.querySelector("span");
          uploadLabel.classList.add("is-busy");
          if (span) span.textContent = "Uploading...";
          try {
            const uploadedPath = await callbacks.onUploadNotebookCover(file);
            if (uploadedPath) {
              draftCover = uploadedPath;
              renderSwatches();
            }
          } catch (err) {
            errEl.textContent = String(err?.message || err || "Upload failed");
            errEl.hidden = false;
          } finally {
            uploading = false;
            uploadLabel.classList.remove("is-busy");
            if (span) span.textContent = "Upload picture";
          }
        });
        uploadLabel.appendChild(fileInput);
        toolsRow.appendChild(uploadLabel);
      }
      card.appendChild(toolsRow);
      card.appendChild(errEl);

      const actionRow = el("div", "inline-notebook-actions");
      const saveBtn = el("button", "btn-editorial btn-sage small", "Save");
      saveBtn.type = "submit";
      const cancelBtn = el("button", "btn-editorial btn-ghost small", "Cancel");
      cancelBtn.type = "button";
      cancelBtn.addEventListener("click", () => {
        panelMode = null;
        renderNotebookPanel();
      });
      actionRow.append(saveBtn, cancelBtn);
      card.appendChild(actionRow);

      card.addEventListener("submit", async (ev) => {
        ev.preventDefault();
        const trimmed = draftName.trim();
        if (!trimmed || uploading) return;
        errEl.hidden = true;
        saveBtn.disabled = true;
        saveBtn.textContent = "Saving...";
        try {
          await callbacks.onUpdateNotebook?.({
            id: selectedNb.id,
            name: trimmed,
            description: selectedNb.description ?? null,
            cover_asset: draftCover || "",
            accent_color: draftAccent,
          });
          panelMode = null;
        } catch (err) {
          errEl.textContent = String(err?.message || err || "Update failed");
          errEl.hidden = false;
        } finally {
          if (saveBtn.isConnected) {
            saveBtn.disabled = false;
            saveBtn.textContent = "Save";
          }
        }
      });

      panelSlot.appendChild(card);
    } else if (panelMode === "delete") {
      const card = el("div", "notebook-inline-card is-danger");
      const nbCount = Number(selectedNb.entry_count ?? entries.length ?? 0);
      card.appendChild(
        el(
          "p",
          "inline-notebook-delete-prompt",
          `Delete “${selectedNb.name}”?`
        )
      );
      const errEl = el("p", "form-error", "");
      errEl.hidden = true;
      card.appendChild(errEl);

      const btns = el("div", "inline-notebook-delete-actions");
      const runDelete = async (deleteEntries) => {
        errEl.hidden = true;
        btns.querySelectorAll("button").forEach((b) => {
          b.disabled = true;
        });
        try {
          await callbacks.onDeleteNotebook?.(selectedNb.id, deleteEntries);
        } catch (err) {
          errEl.textContent = String(err?.message || err || "Delete failed");
          errEl.hidden = false;
          btns.querySelectorAll("button").forEach((b) => {
            b.disabled = false;
          });
        }
      };

      if (nbCount > 0) {
        const keepBtn = el(
          "button",
          "btn-editorial small",
          `Keep ${nbCount} ${nbCount === 1 ? "entry" : "entries"}`
        );
        keepBtn.type = "button";
        keepBtn.addEventListener("click", () => runDelete(false));

        const wipeBtn = el(
          "button",
          "btn-editorial btn-terracotta small",
          `Delete + ${nbCount} ${nbCount === 1 ? "entry" : "entries"}`
        );
        wipeBtn.type = "button";
        wipeBtn.addEventListener("click", () => runDelete(true));

        btns.append(keepBtn, wipeBtn);
      } else {
        const delBtn = el("button", "btn-editorial btn-terracotta small", "Delete");
        delBtn.type = "button";
        delBtn.addEventListener("click", () => runDelete(false));
        btns.appendChild(delBtn);
      }

      const cancelBtn = el("button", "btn-editorial btn-ghost small", "Cancel");
      cancelBtn.type = "button";
      cancelBtn.addEventListener("click", () => {
        panelMode = null;
        renderNotebookPanel();
      });
      btns.appendChild(cancelBtn);
      card.appendChild(btns);
      panelSlot.appendChild(card);
    }
  }

  if (selectedNb) {
    const banner = el("div", "notebook-stage-banner");
    if (selectedNb.cover_asset) {
      const frame = el("button", "notebook-cover-frame");
      frame.type = "button";
      frame.style.borderColor = selectedNb.accent_color || "var(--border-strong)";
      frame.title = callbacks.onUpdateNotebook
        ? "Change notebook cover"
        : selectedNb.name;
      const covImg = document.createElement("img");
      covImg.src = resolveVaultUrl(selectedNb.cover_asset);
      covImg.alt = `${selectedNb.name} cover`;
      covImg.decoding = "async";
      frame.appendChild(covImg);
      if (callbacks.onUpdateNotebook) {
        frame.addEventListener("click", () => {
          panelMode = panelMode === "edit" ? null : "edit";
          renderNotebookPanel();
        });
      }
      banner.appendChild(frame);
    } else if (callbacks.onUpdateNotebook) {
      const placeholder = el("button", "notebook-cover-placeholder");
      placeholder.type = "button";
      placeholder.style.borderColor =
        selectedNb.accent_color || "var(--border-strong)";
      placeholder.title = "Add notebook cover picture";
      placeholder.innerHTML = `${icon("image", "tiny-icon")}<span>Add cover</span>`;
      placeholder.addEventListener("click", () => {
        panelMode = panelMode === "edit" ? null : "edit";
        renderNotebookPanel();
      });
      banner.appendChild(placeholder);
    }
    banner.appendChild(titleWrap);
    header.appendChild(banner);
  } else {
    header.appendChild(titleWrap);
  }

  const headerActions = el("div", "stage-header-actions");
  if (selectedNb && callbacks.onUpdateNotebook) {
    const editNbBtn = el("button", "btn-editorial btn-ghost small");
    editNbBtn.type = "button";
    editNbBtn.innerHTML = `${icon("edit", "tiny-icon")}<span>Edit Notebook</span>`;
    editNbBtn.addEventListener("click", () => {
      panelMode = panelMode === "edit" ? null : "edit";
      renderNotebookPanel();
    });
    headerActions.appendChild(editNbBtn);
  }
  if (selectedNb && callbacks.onDeleteNotebook) {
    const delNbBtn = el("button", "btn-editorial btn-ghost small is-danger");
    delNbBtn.type = "button";
    delNbBtn.innerHTML = `${icon("trash", "tiny-icon")}<span>Delete Notebook</span>`;
    delNbBtn.addEventListener("click", () => {
      panelMode = panelMode === "delete" ? null : "delete";
      renderNotebookPanel();
    });
    headerActions.appendChild(delNbBtn);
  }
  if (hasFilter && callbacks.onClearFilters) {
    const clearBtn = el("button", "btn-editorial btn-ghost", clearLabel);
    clearBtn.type = "button";
    clearBtn.addEventListener("click", () => callbacks.onClearFilters?.());
    headerActions.appendChild(clearBtn);
  }

  const onNew = callbacks.onNewEntry || callbacks.onNewFolio;
  if (onNew) {
    const newBtn = el("button", "btn-editorial btn-sage");
    newBtn.type = "button";
    newBtn.innerHTML = `${icon("feather")}<span>New Entry</span>`;
    newBtn.addEventListener("click", () => onNew());
    headerActions.appendChild(newBtn);
  }
  if (headerActions.childNodes.length > 0) {
    header.appendChild(headerActions);
  }
  section.appendChild(header);
  section.appendChild(panelSlot);

  if (entries.length === 0) {
    const empty = el("div", "editorial-empty-card");
    empty.innerHTML = icon("feather", "empty-emblem");
    empty.appendChild(el("h2", "empty-serif-heading", "No entries yet"));
    empty.appendChild(
      el(
        "p",
        "empty-prose-note",
        hasFilter
          ? "No entries match this filter yet."
          : "Start writing a new entry to begin this journal."
      )
    );
    const emptyActions = el("div", "stage-header-actions");
    if (hasFilter && callbacks.onClearFilters) {
      const clearEmptyBtn = el("button", "btn-editorial btn-ghost", clearLabel);
      clearEmptyBtn.type = "button";
      clearEmptyBtn.addEventListener("click", () => callbacks.onClearFilters?.());
      emptyActions.appendChild(clearEmptyBtn);
    }
    if (onNew) {
      const btn = el("button", "btn-editorial btn-sage");
      btn.type = "button";
      btn.innerHTML = `${icon("feather")}<span>New Entry</span>`;
      btn.addEventListener("click", () => onNew());
      emptyActions.appendChild(btn);
    }
    if (emptyActions.childNodes.length > 0) {
      empty.appendChild(emptyActions);
    }
    section.appendChild(empty);
    container.appendChild(section);
    return;
  }

  const TIMELINE_BATCH_SIZE = 80;
  let renderedCount = 0;
  let currentMonthKey = null;
  let currentRowsEl = null;
  let currentCountEl = null;
  let currentMonthCount = 0;

  const monthsList = el("div", "timeline-months-list");
  const loadMoreRow = el("div", "timeline-load-more-row");

  function appendNextBatch() {
    const slice = entries.slice(renderedCount, renderedCount + TIMELINE_BATCH_SIZE);
    if (slice.length === 0) return;

    const frag = document.createDocumentFragment();
    for (const entry of slice) {
      const dt = new Date(entry.date);
      const y = dt.getFullYear();
      const m = dt.getMonth() + 1;
      const key = `${y}-${String(m).padStart(2, "0")}`;

      if (key !== currentMonthKey || !currentRowsEl) {
        currentMonthKey = key;
        currentMonthCount = 0;

        const groupEl = el("section", "timeline-month-group");
        const divider = el("div", "timeline-month-divider");
        divider.appendChild(el("span", "timeline-month-heading", formatMonthYear(y, m)));
        currentCountEl = el("span", "timeline-month-count small-caps-meta", "");
        divider.appendChild(currentCountEl);
        groupEl.appendChild(divider);

        currentRowsEl = el("div", "timeline-rows");
        groupEl.appendChild(currentRowsEl);
        frag.appendChild(groupEl);
      }

      const nbName = entry.notebook_id ? nbMap.get(entry.notebook_id) : null;
      currentRowsEl.appendChild(buildTimelineRow(entry, nbName, callbacks, "weekday"));
      currentMonthCount += 1;
      if (currentCountEl) {
        currentCountEl.textContent = `${currentMonthCount} ${
          currentMonthCount === 1 ? "entry" : "entries"
        }`;
      }
    }

    monthsList.appendChild(frag);
    renderedCount += slice.length;

    const remaining = entries.length - renderedCount;
    loadMoreRow.innerHTML = "";
    if (remaining > 0) {
      const moreBtn = el(
        "button",
        "btn-editorial btn-ghost",
        `Show more (${remaining} older ${remaining === 1 ? "entry" : "entries"})`
      );
      moreBtn.type = "button";
      moreBtn.addEventListener("click", appendNextBatch);
      loadMoreRow.appendChild(moreBtn);
    }
  }

  appendNextBatch();
  section.appendChild(monthsList);
  section.appendChild(loadMoreRow);
  container.appendChild(section);
}

/**
 * Render the 7-column botanical Calendar view.
 * @param {HTMLElement} container
 * @param {object} state
 * @param {object} callbacks
 */
export function renderCalendarView(container, state, callbacks = {}) {
  container.innerHTML = "";
  const now = new Date();
  const year = state.calendarYear || now.getFullYear();
  const month = state.calendarMonth || now.getMonth() + 1;
  const days = state.calendarDays || [];
  const entries = state.entries || [];

  const dayMap = new Map(days.map((d) => [d.date, d]));
  const entryMap = new Map(entries.map((e) => [e.id, e]));

  const section = el("section", "calendar-stage");
  section.setAttribute("aria-label", "Calendar");

  const header = el("header", "stage-header");
  const titleWrap = el("div");
  titleWrap.appendChild(el("h1", "stage-serif-title", formatMonthYear(year, month)));
  titleWrap.appendChild(el("p", "stage-subtitle", "Calendar"));
  header.appendChild(titleWrap);

  const nav = el("div", "calendar-nav-controls");
  const prevBtn = el("button", "icon-btn");
  prevBtn.type = "button";
  prevBtn.title = "Previous month";
  prevBtn.setAttribute("aria-label", "Previous month");
  prevBtn.innerHTML = icon("chevronLeft");
  prevBtn.addEventListener("click", () => callbacks.onPrevMonth?.());

  const todayBtn = el("button", "btn-editorial small", "Today");
  todayBtn.type = "button";
  todayBtn.addEventListener("click", () =>
    callbacks.onSelectMonth?.(now.getFullYear(), now.getMonth() + 1)
  );

  const nextBtn = el("button", "icon-btn");
  nextBtn.type = "button";
  nextBtn.title = "Next month";
  nextBtn.setAttribute("aria-label", "Next month");
  nextBtn.innerHTML = icon("chevronRight");
  nextBtn.addEventListener("click", () => callbacks.onNextMonth?.());

  nav.appendChild(prevBtn);
  nav.appendChild(todayBtn);
  nav.appendChild(nextBtn);
  header.appendChild(nav);
  section.appendChild(header);

  const sheet = el("div", "calendar-mosaic-sheet");
  const wkRow = el("div", "calendar-weekday-row");
  for (const wd of ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]) {
    wkRow.appendChild(el("div", "calendar-weekday-cell small-caps-meta", wd));
  }
  sheet.appendChild(wkRow);

  const grid = el("div", "calendar-grid");
  const firstDay = new Date(year, month - 1, 1);
  const leadingEmpty = (firstDay.getDay() + 6) % 7;
  const daysInMonth = new Date(year, month, 0).getDate();
  const todayStr = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(
    now.getDate()
  ).padStart(2, "0")}`;

  for (let i = 0; i < leadingEmpty; i++) {
    const pad = el("div", "calendar-day-cell is-pad");
    pad.setAttribute("aria-hidden", "true");
    grid.appendChild(pad);
  }

  for (let day = 1; day <= daysInMonth; day++) {
    const dateStr = `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    const summary = dayMap.get(dateStr) || null;
    const primaryEntry = summary?.primary_entry_id
      ? entryMap.get(summary.primary_entry_id)
      : null;

    const cell = el("button", "calendar-day-cell");
    cell.type = "button";
    if (dateStr === todayStr) cell.classList.add("is-today");
    if (summary && summary.entry_count > 0) cell.classList.add("has-entries");
    if (primaryEntry?.cover_photo) cell.classList.add("has-photo");

    if (primaryEntry?.cover_photo) {
      const bg = el("div", "calendar-day-thumb-bg");
      const img = document.createElement("img");
      img.src = resolveVaultUrl(primaryEntry.cover_photo);
      img.alt = "";
      img.loading = "lazy";
      img.decoding = "async";
      bg.appendChild(img);
      bg.appendChild(el("div", "calendar-day-thumb-scrim"));
      cell.appendChild(bg);
    }

    const top = el("div", "calendar-day-top");
    top.appendChild(el("span", "calendar-day-number", String(day)));
    if (summary?.starred_count > 0) {
      const star = el("span", "calendar-day-star");
      star.innerHTML = icon("starFilled", "tiny-icon");
      top.appendChild(star);
    }
    cell.appendChild(top);

    const bottom = el("div", "calendar-day-bottom");
    const indicators = el("div", "calendar-day-indicators");
    if (summary?.has_photos) {
      const cam = el("span");
      cam.innerHTML = icon("camera", "tiny-icon");
      indicators.appendChild(cam);
    }
    if (summary?.has_audio) {
      const mic = el("span");
      mic.innerHTML = icon("mic", "tiny-icon");
      indicators.appendChild(mic);
    }
    bottom.appendChild(indicators);

    if (summary && summary.entry_count > 0) {
      const pips = el("div", "calendar-pips");
      const count = Math.min(summary.entry_count, 4);
      for (let p = 0; p < count; p++) {
        pips.appendChild(el("span", "calendar-pip"));
      }
      bottom.appendChild(pips);
    }
    cell.appendChild(bottom);

    cell.addEventListener("click", () => {
      if (callbacks.onSelectDate) {
        callbacks.onSelectDate(dateStr, summary);
      } else if (summary?.primary_entry_id) {
        callbacks.onOpenEntry?.(summary.primary_entry_id);
      } else {
        callbacks.onNewEntryForDate?.(dateStr);
      }
    });

    grid.appendChild(cell);
  }

  sheet.appendChild(grid);
  section.appendChild(sheet);
  container.appendChild(section);
}

/**
 * Render the "On This Day" archival memories spread grouped by year.
 * @param {HTMLElement} container
 * @param {object} state
 * @param {object} callbacks
 */
export function renderOnThisDayView(container, state, callbacks = {}) {
  container.innerHTML = "";
  const groups = state.onThisDayGroups ?? [];
  const todayLabel = new Intl.DateTimeFormat("en-US", {
    month: "long",
    day: "2-digit",
  }).format(new Date());

  const section = el("section", "timeline-river");
  section.setAttribute("aria-label", "On This Day");

  const header = el("header", "stage-header");
  const titleWrap = el("div");
  titleWrap.appendChild(el("h1", "stage-serif-title", "On This Day"));
  titleWrap.appendChild(el("p", "stage-subtitle", todayLabel));
  header.appendChild(titleWrap);
  section.appendChild(header);

  if (groups.length === 0) {
    const empty = el("div", "editorial-empty-card");
    empty.innerHTML = icon("clock", "empty-emblem");
    empty.appendChild(el("h2", "empty-serif-heading", "No past entries on this date"));
    empty.appendChild(
      el(
        "p",
        "empty-prose-note",
        "Entries written on this calendar day in previous years will appear here."
      )
    );
    section.appendChild(empty);
    container.appendChild(section);
    return;
  }

  const listEl = el("div", "timeline-months-list");
  for (const group of groups) {
    const groupEl = el("section", "timeline-month-group");
    const label =
      group.years_ago === 1
        ? `1 year ago · ${group.year}`
        : `${group.years_ago} years ago · ${group.year}`;
    const count = (group.entries || []).length;

    const divider = el("div", "timeline-month-divider");
    divider.appendChild(el("span", "timeline-month-heading", label));
    divider.appendChild(
      el(
        "span",
        "timeline-month-count small-caps-meta",
        `${count} ${count === 1 ? "entry" : "entries"}`
      )
    );
    groupEl.appendChild(divider);

    const rowsEl = el("div", "timeline-rows");
    for (const entry of group.entries || []) {
      rowsEl.appendChild(buildTimelineRow(entry, null, callbacks, "year"));
    }
    groupEl.appendChild(rowsEl);
    listEl.appendChild(groupEl);
  }

  section.appendChild(listEl);
  container.appendChild(section);
}

function openPolaroidLightbox(photo) {
  const backdrop = el("div", "editorial-modal-backdrop");
  const sheet = el("div", "polaroid-lightbox-sheet");

  const closeBtn = el("button", "polaroid-lightbox-close");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close photograph");
  closeBtn.innerHTML = icon("close");

  const img = document.createElement("img");
  img.className = "polaroid-lightbox-img";
  img.src = resolveVaultUrl(photo.url);
  img.alt = photo.caption || "Journal photograph";
  img.decoding = "async";

  sheet.appendChild(closeBtn);
  sheet.appendChild(img);
  if (photo.caption) {
    sheet.appendChild(el("p", "polaroid-lightbox-caption", photo.caption));
  }

  const ac = new AbortController();
  function close() {
    ac.abort();
    backdrop.remove();
  }
  closeBtn.addEventListener("click", close, { signal: ac.signal });
  backdrop.addEventListener(
    "click",
    (ev) => {
      if (ev.target === backdrop) close();
    },
    { signal: ac.signal }
  );
  window.addEventListener(
    "keydown",
    (ev) => {
      if (!backdrop.isConnected) {
        ac.abort();
        return;
      }
      if (ev.key === "Escape") close();
    },
    { signal: ac.signal }
  );

  backdrop.appendChild(sheet);
  document.body.appendChild(backdrop);
}

/**
 * Create an interactive 3D pointer-tilting Polaroid photograph card.
 * @param {object} photo
 * @param {number} figureNumber
 * @param {object} [callbacks]
 * @returns {HTMLElement}
 */
export function createPolaroidElement(photo, figureNumber, callbacks = {}) {
  const card = el("figure", "polaroid-card");
  const baseRot = Number(photo.rotation_deg || 0);
  card.style.transform = `perspective(760px) rotateX(0deg) rotateY(0deg) rotateZ(${baseRot}deg)`;

  const safeWashi = safeAssetUrl(photo.washi_asset);
  if (safeWashi) {
    const washi = document.createElement("img");
    washi.className = "polaroid-washi-strip";
    washi.src = safeWashi;
    washi.decoding = "async";
    washi.alt = "";
    washi.setAttribute("aria-hidden", "true");
    card.appendChild(washi);
  }

  if (callbacks.onRemove) {
    const rmBtn = el("button", "polaroid-remove-btn");
    rmBtn.type = "button";
    rmBtn.title = "Remove photograph";
    rmBtn.setAttribute("aria-label", "Remove photograph");
    rmBtn.innerHTML = icon("close");
    rmBtn.addEventListener("click", (ev) => {
      ev.stopPropagation();
      callbacks.onRemove(photo.id);
    });
    card.appendChild(rmBtn);
  }

  const frame = el("div", "polaroid-frame-window");
  const img = document.createElement("img");
  img.className = "polaroid-photo-img";
  img.src = resolveVaultUrl(photo.thumbnail_url || photo.url);
  img.alt = photo.caption || "Journal photograph";
  img.loading = "lazy";
  img.decoding = "async";
  frame.appendChild(img);

  const sheen = el("div", "polaroid-specular-sheen");
  frame.appendChild(sheen);
  frame.addEventListener("click", () => {
    if (callbacks.onOpenLightbox) {
      callbacks.onOpenLightbox(photo);
    } else {
      openPolaroidLightbox(photo);
    }
  });
  card.appendChild(frame);

  let tiltRaf = 0;
  let lastClientX = 0;
  let lastClientY = 0;

  card.addEventListener("mousemove", (ev) => {
    lastClientX = ev.clientX;
    lastClientY = ev.clientY;
    if (tiltRaf) return;
    tiltRaf = requestAnimationFrame(() => {
      tiltRaf = 0;
      if (!card.isConnected) return;
      const rect = card.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const nx = Math.min(Math.max((lastClientX - rect.left) / rect.width, 0), 1);
      const ny = Math.min(Math.max((lastClientY - rect.top) / rect.height, 0), 1);
      const tiltX = (0.5 - ny) * 10;
      const tiltY = (nx - 0.5) * 12;
      card.classList.add("is-hovered");
      card.style.setProperty("--sheen-x", `${Math.round(nx * 100)}%`);
      card.style.setProperty("--sheen-y", `${Math.round(ny * 100)}%`);
      card.style.transform = `perspective(760px) rotateX(${tiltX.toFixed(2)}deg) rotateY(${tiltY.toFixed(
        2
      )}deg) rotateZ(${baseRot}deg) translateY(-2px)`;
    });
  });

  card.addEventListener("mouseleave", () => {
    if (tiltRaf) {
      cancelAnimationFrame(tiltRaf);
      tiltRaf = 0;
    }
    card.classList.remove("is-hovered");
    card.style.transform = `perspective(760px) rotateX(0deg) rotateY(0deg) rotateZ(${baseRot}deg)`;
  });

  const captionArea = el("figcaption", "polaroid-caption-area");
  const metaRow = el("div", "polaroid-meta-row");
  metaRow.appendChild(
    el("span", "polaroid-fig-num", String(figureNumber).padStart(2, "0"))
  );
  if (photo.location?.place_name) {
    const locChip = el("span", "polaroid-loc-chip");
    locChip.innerHTML = icon("pin", "tiny-icon");
    locChip.appendChild(el("span", "", photo.location.place_name));
    metaRow.appendChild(locChip);
  }
  captionArea.appendChild(metaRow);

  if (callbacks.onUpdateCaption) {
    const input = document.createElement("input");
    input.type = "text";
    input.className = "polaroid-caption-input";
    input.value = photo.caption || "";
    input.placeholder = "Add a caption…";
    input.addEventListener("change", () => {
      callbacks.onUpdateCaption(photo.id, input.value);
    });
    captionArea.appendChild(input);
  } else if (photo.caption) {
    captionArea.appendChild(el("p", "polaroid-caption-text", photo.caption));
  }

  card.appendChild(captionArea);
  return card;
}

/**
 * Render draggable/rotatable/scalable botanical stickers & postal ink stamps.
 * @param {HTMLElement} container
 * @param {Array} stamps
 * @param {boolean} editable
 * @param {function} onChange
 */
export function renderStampOverlay(container, stamps = [], editable = false, onChange = null) {
  container.innerHTML = "";
  const list = Array.isArray(stamps) ? stamps.map((s) => ({ ...s })) : [];
  if (list.length === 0) return;

  let selectedId = null;
  const layer = el("div", "stamp-sticker-layer");
  if (editable) layer.classList.add("is-editable");

  function commit(nextList) {
    onChange?.(nextList.map((s) => ({ ...s })));
  }

  function redraw() {
    layer.innerHTML = "";
    for (const st of list) {
      const isPostmark = String(st.asset_path || "").includes("postmark-");
      const node = el(
        "div",
        `placed-stamp-node ${isPostmark ? "is-postmark" : "is-diecut"}${
          editable && selectedId === st.id ? " is-selected" : ""
        }`
      );
      node.style.left = `${st.x_pct}%`;
      node.style.top = `${st.y_pct}%`;
      node.style.transform = `translate(-50%, -50%) rotate(${st.rotation_deg || 0}deg) scale(${
        st.scale || 1
      })`;

      const stampUrl = safeAssetUrl(st.asset_path);
      if (stampUrl) {
        const img = document.createElement("img");
        img.className = "placed-stamp-img";
        img.src = stampUrl;
        img.decoding = "async";
        img.alt = st.label || st.asset_id || "Journal stamp";
        img.draggable = false;
        node.appendChild(img);
      }

      if (st.label) {
        node.appendChild(el("span", "placed-stamp-label", st.label));
      }

      if (editable) {
        node.addEventListener("mousedown", (ev) => {
          if (ev.target.closest(".stamp-mini-controls")) return;
          ev.preventDefault();
          ev.stopPropagation();
          selectedId = st.id;
          layer.classList.add("is-dragging");
          redraw();

          const rect = layer.getBoundingClientRect();
          const dragAc = new AbortController();
          function onMove(mv) {
            if (!layer.isConnected) {
              dragAc.abort();
              return;
            }
            if (rect.width <= 0 || rect.height <= 0) return;
            st.x_pct = Math.min(Math.max(((mv.clientX - rect.left) / rect.width) * 100, 3), 97);
            st.y_pct = Math.min(Math.max(((mv.clientY - rect.top) / rect.height) * 100, 3), 97);
            const activeNode = layer.querySelector(`[data-stamp-id="${CSS.escape(st.id)}"]`);
            if (activeNode) {
              activeNode.style.left = `${st.x_pct}%`;
              activeNode.style.top = `${st.y_pct}%`;
            }
          }
          function onUp() {
            dragAc.abort();
            layer.classList.remove("is-dragging");
            commit(list);
          }
          window.addEventListener("mousemove", onMove, { signal: dragAc.signal });
          window.addEventListener("mouseup", onUp, { signal: dragAc.signal });
        });

        node.dataset.stampId = st.id;

        if (selectedId === st.id) {
          const ctrls = el("div", "stamp-mini-controls");
          const rotL = el("button", "stamp-ctrl-btn", "-5°");
          rotL.type = "button";
          rotL.title = "Rotate counter-clockwise";
          rotL.addEventListener("click", (ev) => {
            ev.stopPropagation();
            st.rotation_deg = Math.max((st.rotation_deg || 0) - 5, -24);
            commit(list);
            redraw();
          });

          const rotR = el("button", "stamp-ctrl-btn", "+5°");
          rotR.type = "button";
          rotR.title = "Rotate clockwise";
          rotR.addEventListener("click", (ev) => {
            ev.stopPropagation();
            st.rotation_deg = Math.min((st.rotation_deg || 0) + 5, 24);
            commit(list);
            redraw();
          });

          const shrink = el("button", "stamp-ctrl-btn", "-");
          shrink.type = "button";
          shrink.title = "Smaller";
          shrink.addEventListener("click", (ev) => {
            ev.stopPropagation();
            st.scale = Math.max(Number((st.scale || 1) - 0.1), 0.6);
            commit(list);
            redraw();
          });

          const grow = el("button", "stamp-ctrl-btn", "+");
          grow.type = "button";
          grow.title = "Larger";
          grow.addEventListener("click", (ev) => {
            ev.stopPropagation();
            st.scale = Math.min(Number((st.scale || 1) + 0.1), 1.6);
            commit(list);
            redraw();
          });

          const del = el("button", "stamp-ctrl-btn danger");
          del.type = "button";
          del.title = "Remove stamp";
          del.innerHTML = icon("close", "tiny-icon");
          del.addEventListener("click", (ev) => {
            ev.stopPropagation();
            const idx = list.findIndex((item) => item.id === st.id);
            if (idx >= 0) list.splice(idx, 1);
            selectedId = null;
            commit(list);
            redraw();
          });

          ctrls.append(rotL, rotR, shrink, grow, del);
          node.appendChild(ctrls);
        }
      }

      layer.appendChild(node);
    }
  }

  redraw();
  container.appendChild(layer);
}

/**
 * Render the tactile stamp & sticker picker popover.
 * @param {HTMLElement} container
 * @param {object} manifest
 * @param {function} onPick
 * @param {function} onClose
 */
export function renderStampPickerPopover(container, manifest, onPick, onClose) {
  container.innerHTML = "";
  let activeTab = "stickers";

  const pop = el("div", "stamp-picker-popover");
  const head = el("div", "stamp-picker-header");
  const titleWrap = el("div", "stamp-picker-title");
  titleWrap.innerHTML = icon("stamp", "popover-icon");
  titleWrap.appendChild(el("span", "small-caps-meta", "Stamps & Stickers"));
  head.appendChild(titleWrap);

  const closeBtn = el("button", "icon-btn");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close stamp picker");
  closeBtn.innerHTML = icon("close");
  closeBtn.addEventListener("click", () => onClose?.());
  head.appendChild(closeBtn);
  pop.appendChild(head);

  const tabs = el("div", "stamp-picker-tabs");
  const stickerTab = el("button", "stamp-tab-btn is-active", "Stickers");
  stickerTab.type = "button";
  const postmarkTab = el("button", "stamp-tab-btn", "Postmarks");
  postmarkTab.type = "button";
  tabs.append(stickerTab, postmarkTab);
  pop.appendChild(tabs);

  const grid = el("div", "stamp-choices-grid");

  function renderChoices() {
    grid.innerHTML = "";
    stickerTab.classList.toggle("is-active", activeTab === "stickers");
    postmarkTab.classList.toggle("is-active", activeTab === "stamps");

    const items =
      activeTab === "stickers" ? manifest?.stickers || [] : manifest?.stamps || [];

    for (const item of items) {
      const btn = el("button", "stamp-choice-card");
      btn.type = "button";
      const img = document.createElement("img");
      img.src = safeAssetUrl(item.path);
      img.loading = "lazy";
      img.decoding = "async";
      img.alt = item.label;
      img.className = activeTab === "stickers" ? "stamp-preview-diecut" : "stamp-preview-ink";
      btn.appendChild(img);
      btn.appendChild(el("span", "stamp-choice-label", item.label));
      btn.addEventListener("click", () => {
        onPick?.(item);
        onClose?.();
      });
      grid.appendChild(btn);
    }
  }

  stickerTab.addEventListener("click", () => {
    activeTab = "stickers";
    renderChoices();
  });
  postmarkTab.addEventListener("click", () => {
    activeTab = "stamps";
    renderChoices();
  });

  renderChoices();
  pop.appendChild(grid);
  container.appendChild(pop);
}

function appendSafeHighlightedSnippet(parent, snippetHtml) {
  const raw = String(snippetHtml || "");
  const parts = raw.split(/(<mark>.*?<\/mark>)/gi);
  for (const part of parts) {
    const m = /^<mark>([\s\S]*)<\/mark>$/i.exec(part);
    if (m) {
      const mark = document.createElement("mark");
      mark.textContent = m[1];
      parent.appendChild(mark);
    } else {
      parent.appendChild(document.createTextNode(part));
    }
  }
}

/**
 * Render the FTS5 full-text Search modal.
 * @param {HTMLElement} container
 * @param {object} state
 * @param {object} callbacks
 */
export function renderSearchModal(container, state, callbacks = {}) {
  const existingInput = container.querySelector(".search-modal-input");
  const existingResults = container.querySelector(".search-results-list");
  const hits = state.searchResults || [];

  function fillResults(listEl) {
    listEl.innerHTML = "";
    if (state.searchLoading) {
      listEl.appendChild(el("div", "empty-prose-note", "Searching…"));
      return;
    }
    if (!hits.length) {
      listEl.appendChild(el("div", "empty-prose-note", "No matching entries found."));
      return;
    }

    for (const hit of hits) {
      const summary = hit.summary || hit;
      const card = el("button", "search-hit-card");
      card.type = "button";

      const head = el("div", "search-hit-head");
      const titleText = isAutoDerivedTitle(summary.title, summary.snippet)
        ? formatDateParts(summary.date).full
        : summary.title;
      head.appendChild(el("span", "search-hit-title", titleText));
      head.appendChild(el("span", "small-caps-meta", formatDateParts(summary.date).full));
      card.appendChild(head);

      const snip = el("p", "search-hit-snippet");
      appendSafeHighlightedSnippet(
        snip,
        hit.highlighted_snippet || cleanExcerptSnippet(summary.snippet)
      );
      card.appendChild(snip);

      card.addEventListener("click", () => {
        callbacks.onSelectEntry?.(summary.id);
        callbacks.onClose?.();
      });
      listEl.appendChild(card);
    }
  }

  if (existingInput && existingResults) {
    if (existingInput.value !== (state.searchQuery || "")) {
      existingInput.value = state.searchQuery || "";
    }
    fillResults(existingResults);
    return;
  }

  container.innerHTML = "";
  const backdrop = el("div", "search-modal-backdrop");
  const sheet = el("div", "search-modal-sheet");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Search entries");

  const bar = el("div", "search-input-bar");
  const iconWrap = el("span");
  iconWrap.innerHTML = icon("search");
  bar.appendChild(iconWrap);

  const input = document.createElement("input");
  input.type = "text";
  input.className = "search-modal-input";
  input.placeholder = "Search entries…";
  input.value = state.searchQuery || "";
  input.addEventListener("input", () => callbacks.onQueryChange?.(input.value));
  bar.appendChild(input);

  const closeBtn = el("button", "icon-btn");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close search");
  closeBtn.innerHTML = icon("close");
  bar.appendChild(closeBtn);
  sheet.appendChild(bar);

  const resultsList = el("div", "search-results-list");
  fillResults(resultsList);
  sheet.appendChild(resultsList);

  const close = bindModalEscape(container, backdrop, callbacks.onClose);
  closeBtn.addEventListener("click", close);
  backdrop.addEventListener("click", (ev) => {
    if (ev.target === backdrop) close();
  });

  backdrop.appendChild(sheet);
  container.appendChild(backdrop);
  requestAnimationFrame(() => input.focus());
}

function renderMigrationDynamicRegion(region, state) {
  region.innerHTML = "";
  if (state.migrationLoading) {
    region.appendChild(el("p", "modal-status-banner", "Processing archive…"));
  }
  if (state.migrationError) {
    region.appendChild(el("p", "form-error", state.migrationError));
  }

  if (state.migrationReport) {
    const r = state.migrationReport;
    const box = el("div", "migration-report-box");
    const kpiGrid = el("div", "report-kpi-grid");

    const kpis = [
      ["Found", r.total_found, ""],
      ["Imported", r.imported_count, "is-sage"],
      ["Skipped", r.skipped_count, ""],
      ["Failed", r.failed_count, "is-terracotta"],
      ["Media", r.media_copied, ""],
    ];
    for (const [label, num, mod] of kpis) {
      const k = el("div", `report-kpi ${mod}`.trim());
      k.appendChild(el("span", "kpi-num", String(num)));
      k.appendChild(el("span", "small-caps-meta", label));
      kpiGrid.appendChild(k);
    }
    box.appendChild(kpiGrid);

    const wrap = el("div", "audit-table-wrap");
    const table = el("table", "audit-table");
    const thead = document.createElement("thead");
    const hr = document.createElement("tr");
    for (const col of ["Status", "Title / Source", "Date", "Note"]) {
      hr.appendChild(el("th", "", col));
    }
    thead.appendChild(hr);
    table.appendChild(thead);

    const tbody = document.createElement("tbody");
    for (const item of r.items || []) {
      const tr = document.createElement("tr");
      const statusCell = document.createElement("td");
      const pillClass =
        item.status === "imported"
          ? "is-imported"
          : item.status === "skipped"
          ? "is-skipped"
          : item.status === "needs_review"
          ? "is-review"
          : "is-failed";
      const pillText =
        item.status === "imported"
          ? "Imported"
          : item.status === "skipped"
          ? "Skipped"
          : item.status === "needs_review"
          ? "Review"
          : "Failed";
      statusCell.appendChild(el("span", `status-pill ${pillClass}`, pillText));

      tr.appendChild(statusCell);
      tr.appendChild(el("td", "", item.title || item.source_id));
      tr.appendChild(el("td", "mono-cell", item.date || "—"));
      tr.appendChild(el("td", "muted-cell", item.reason || item.target_path || ""));
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);
    wrap.appendChild(table);
    box.appendChild(wrap);
    region.appendChild(box);
  }
}

/**
 * Render the Day One & Notion Migration modal.
 * @param {HTMLElement} container
 * @param {object} state
 * @param {object} callbacks
 */
export function renderMigrationModal(container, state, callbacks = {}) {
  container.__migrationCallbacks = callbacks;
  const existing = container.querySelector('[data-log-modal="migration"]');
  if (existing) {
    const dBtn = existing.querySelector('[data-role="migration-dayone-run"]');
    const nBtn = existing.querySelector('[data-role="migration-notion-run"]');
    if (dBtn) dBtn.disabled = Boolean(state.migrationLoading);
    if (nBtn) nBtn.disabled = Boolean(state.migrationLoading);
    const region = existing.querySelector('[data-region="migration-dynamic"]');
    if (region) renderMigrationDynamicRegion(region, state);
    container.__migrationClose = bindModalEscape(container, existing, () =>
      container.__migrationCallbacks?.onClose?.()
    );
    return;
  }

  container.innerHTML = "";

  const backdrop = el("div", "editorial-modal-backdrop");
  backdrop.dataset.logModal = "migration";
  const sheet = el("div", "editorial-modal-sheet wide");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Import");

  const header = el("header", "modal-header");
  const titleGroup = el("div", "modal-title-group");
  const titleIcon = el("span", "modal-title-icon");
  titleIcon.innerHTML = icon("archive");
  titleGroup.appendChild(titleIcon);

  const titleText = el("div");
  titleText.appendChild(el("span", "small-caps-meta", "Import"));
  titleText.appendChild(el("h2", "modal-serif-heading", "Day One & Notion Import"));
  titleGroup.appendChild(titleText);
  header.appendChild(titleGroup);

  const closeBtn = el("button", "icon-btn");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close import modal");
  closeBtn.innerHTML = icon("close");
  header.appendChild(closeBtn);
  sheet.appendChild(header);

  const body = el("div", "modal-body");
  const cols = el("div", "migration-columns");

  const dryLabel = el("label", "dry-run-toggle");
  const dryCheck = document.createElement("input");
  dryCheck.type = "checkbox";
  dryLabel.append(dryCheck, el("span", "", "Dry run (audit without writing files)"));

  // Day One Panel
  const dPanel = el("section", "migration-panel");
  dPanel.appendChild(el("h3", "panel-serif-title", "Day One Export (.zip or folder)"));
  dPanel.appendChild(
    el(
      "p",
      "panel-note",
      "Primary journal source: imports entries, photos, audio, weather, and coordinates."
    )
  );

  const dFileLabel = el("label", "btn-editorial btn-sage upload-btn-label");
  dFileLabel.innerHTML = `${icon("upload")}<span>Upload Day One .zip</span>`;
  const dFileInput = document.createElement("input");
  dFileInput.type = "file";
  dFileInput.accept = ".zip";
  dFileInput.className = "sr-only";
  dFileInput.addEventListener("change", () => {
    const file = dFileInput.files?.[0];
    if (file) container.__migrationCallbacks?.onImportDayOneFile?.(file);
  });
  dFileLabel.appendChild(dFileInput);
  dPanel.appendChild(dFileLabel);

  const dPathRow = el("div", "path-import-row");
  const dInput = document.createElement("input");
  dInput.type = "text";
  dInput.className = "editorial-input";
  dInput.placeholder = "/path/to/dayone-export.zip";
  const dRunBtn = el("button", "btn-editorial small", "Import Path");
  dRunBtn.type = "button";
  dRunBtn.dataset.role = "migration-dayone-run";
  dRunBtn.disabled = Boolean(state.migrationLoading);
  dRunBtn.addEventListener("click", () => {
    const p = dInput.value.trim();
    if (p) container.__migrationCallbacks?.onImportDayOnePath?.(p, dryCheck.checked);
  });
  dPathRow.append(dInput, dRunBtn);
  dPanel.appendChild(dPathRow);
  cols.appendChild(dPanel);

  // Notion Panel
  const nPanel = el("section", "migration-panel");
  nPanel.appendChild(el("h3", "panel-serif-title", "Notion Export (.zip or folder)"));
  nPanel.appendChild(
    el(
      "p",
      "panel-note",
      "Day One Primary — Notion duplicates automatically skipped by source ID, timestamp, and content similarity."
    )
  );

  const nFileLabel = el("label", "btn-editorial btn-ghost upload-btn-label");
  nFileLabel.innerHTML = `${icon("upload")}<span>Upload Notion .zip</span>`;
  const nFileInput = document.createElement("input");
  nFileInput.type = "file";
  nFileInput.accept = ".zip";
  nFileInput.className = "sr-only";
  nFileInput.addEventListener("change", () => {
    const file = nFileInput.files?.[0];
    if (file) container.__migrationCallbacks?.onImportNotionFile?.(file);
  });
  nFileLabel.appendChild(nFileInput);
  nPanel.appendChild(nFileLabel);

  const nPathRow = el("div", "path-import-row");
  const nInput = document.createElement("input");
  nInput.type = "text";
  nInput.className = "editorial-input";
  nInput.placeholder = "/path/to/notion-export";
  const nRunBtn = el("button", "btn-editorial small", "Import Path");
  nRunBtn.type = "button";
  nRunBtn.dataset.role = "migration-notion-run";
  nRunBtn.disabled = Boolean(state.migrationLoading);
  nRunBtn.addEventListener("click", () => {
    const p = nInput.value.trim();
    if (p) container.__migrationCallbacks?.onImportNotionPath?.(p, dryCheck.checked);
  });
  nPathRow.append(nInput, nRunBtn);
  nPanel.appendChild(nPathRow);
  cols.appendChild(nPanel);

  body.appendChild(cols);
  body.appendChild(dryLabel);

  const dynRegion = document.createElement("div");
  dynRegion.dataset.region = "migration-dynamic";
  dynRegion.style.display = "contents";
  renderMigrationDynamicRegion(dynRegion, state);
  body.appendChild(dynRegion);

  sheet.appendChild(body);
  backdrop.appendChild(sheet);

  container.__migrationClose = bindModalEscape(container, backdrop, () =>
    container.__migrationCallbacks?.onClose?.()
  );
  closeBtn.addEventListener("click", () => container.__migrationClose?.());
  backdrop.addEventListener("click", (ev) => {
    if (ev.target === backdrop) container.__migrationClose?.();
  });

  container.appendChild(backdrop);
}

function renderBackupDynamicRegion(region, state, getPassphrase, getCallbacks) {
  region.innerHTML = "";
  if (state.backupError) {
    region.appendChild(el("p", "form-error", state.backupError));
  }
  if (state.syncResult?.message) {
    const banner = el(
      "p",
      `modal-status-banner ${state.syncResult.success ? "is-success" : ""}`.trim()
    );
    banner.innerHTML = `${icon("check")}<span>${escapeHtml(state.syncResult.message)}</span>`;
    region.appendChild(banner);
  }

  const listSection = el("section", "backup-list-section");
  const listHead = el("div", "backup-list-header");
  listHead.appendChild(el("span", "small-caps-meta", "Available Backups"));
  const refreshBtn = el("button", "btn-editorial small");
  refreshBtn.type = "button";
  refreshBtn.innerHTML = `${icon("refresh")}<span>Refresh</span>`;
  refreshBtn.addEventListener("click", () => getCallbacks()?.onRefresh?.());
  listHead.appendChild(refreshBtn);
  listSection.appendChild(listHead);

  const backups = state.backups || [];
  if (backups.length === 0) {
    listSection.appendChild(
      el("p", "empty-prose-note", "No encrypted snapshots recorded yet.")
    );
  } else {
    const listWrap = el("div", "backup-items-list");
    for (const b of backups) {
      const row = el("div", "backup-item-row");
      const main = el("div", "backup-item-main");
      const titleLine = el("div", "backup-item-title-line");
      titleLine.appendChild(el("span", "backup-filename", b.filename));
      titleLine.appendChild(
        el("span", "small-caps-meta", b.encrypted ? "age-encrypted" : "plain")
      );
      main.appendChild(titleLine);

      const subText = `${formatDateParts(b.created_at).full} · ${formatBytes(b.size_bytes)} · ${
        b.entry_count
      } entries · sha256:${String(b.sha256 || "").slice(0, 10)}`;
      main.appendChild(el("span", "mono-cell muted-cell", subText));
      row.appendChild(main);

      const acts = el("div", "backup-item-actions");
      acts.appendChild(el("span", "small-caps-meta", b.location || "local"));
      const restoreBtn = el("button", "btn-editorial small", "Restore");
      restoreBtn.type = "button";
      restoreBtn.disabled = Boolean(state.backupLoading);
      restoreBtn.addEventListener("click", () => {
        getCallbacks()?.onRestoreBackup?.(b.id, getPassphrase());
      });
      acts.appendChild(restoreBtn);
      row.appendChild(acts);
      listWrap.appendChild(row);
    }
    listSection.appendChild(listWrap);
  }

  region.appendChild(listSection);
}

/**
 * Render the Encrypted Sanctuary Backup & Google Drive Sync modal.
 * @param {HTMLElement} container
 * @param {object} state
 * @param {object} callbacks
 */
export function renderBackupModal(container, state, callbacks = {}) {
  container.__backupCallbacks = callbacks;
  const gdriveFolder = state.backupConfig?.gdrive_folder || "LogJournal";
  const existing = container.querySelector('[data-log-modal="backup"]');
  if (existing) {
    const passInput = existing.querySelector('[data-role="backup-passphrase"]');
    const snapBtn = existing.querySelector('[data-role="backup-snap"]');
    const syncBtn = existing.querySelector('[data-role="backup-sync"]');
    const syncLabel = existing.querySelector('[data-role="backup-sync-label"]');
    if (snapBtn) snapBtn.disabled = Boolean(state.backupLoading);
    if (syncBtn) syncBtn.disabled = Boolean(state.backupLoading);
    if (syncLabel) syncLabel.textContent = `Sync to Google Drive (${gdriveFolder})`;
    const region = existing.querySelector('[data-region="backup-dynamic"]');
    if (region) {
      renderBackupDynamicRegion(
        region,
        state,
        () => passInput?.value || "",
        () => container.__backupCallbacks
      );
    }
    container.__backupClose = bindModalEscape(container, existing, () =>
      container.__backupCallbacks?.onClose?.()
    );
    return;
  }

  container.innerHTML = "";
  let showPassphrase = false;

  const backdrop = el("div", "editorial-modal-backdrop");
  backdrop.dataset.logModal = "backup";
  const sheet = el("div", "editorial-modal-sheet wide");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Backup");

  const header = el("header", "modal-header");
  const titleGroup = el("div", "modal-title-group");
  const titleIcon = el("span", "modal-title-icon");
  titleIcon.innerHTML = icon("shield");
  titleGroup.appendChild(titleIcon);

  const titleText = el("div");
  titleText.appendChild(el("span", "small-caps-meta", "Backup"));
  titleText.appendChild(
    el("h2", "modal-serif-heading", "Encrypted Backup & Google Drive")
  );
  titleGroup.appendChild(titleText);
  header.appendChild(titleGroup);

  const closeBtn = el("button", "icon-btn");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close backup modal");
  closeBtn.innerHTML = icon("close");
  header.appendChild(closeBtn);
  sheet.appendChild(header);

  const body = el("div", "modal-body");

  const passBox = el("div", "backup-passphrase-box");
  passBox.appendChild(el("label", "small-caps-meta", "Encryption Passphrase (age)"));
  const passRow = el("div", "passphrase-input-row");
  const passInput = document.createElement("input");
  passInput.type = "password";
  passInput.className = "editorial-input";
  passInput.dataset.role = "backup-passphrase";
  passInput.placeholder = "Enter passphrase to encrypt or restore…";

  const revealBtn = el("button", "btn-editorial small", "Show");
  revealBtn.type = "button";
  revealBtn.addEventListener("click", () => {
    showPassphrase = !showPassphrase;
    passInput.type = showPassphrase ? "text" : "password";
    revealBtn.textContent = showPassphrase ? "Hide" : "Show";
  });
  passRow.append(passInput, revealBtn);
  passBox.appendChild(passRow);

  const actionRow = el("div", "backup-action-row");
  const snapBtn = el("button", "btn-editorial btn-sage");
  snapBtn.type = "button";
  snapBtn.dataset.role = "backup-snap";
  snapBtn.disabled = Boolean(state.backupLoading);
  snapBtn.innerHTML = `${icon("shield")}<span>Create .age Encrypted Snapshot</span>`;
  snapBtn.addEventListener("click", () => {
    container.__backupCallbacks?.onCreateBackup?.(passInput.value, false);
  });

  const syncBtn = el("button", "btn-editorial");
  syncBtn.type = "button";
  syncBtn.dataset.role = "backup-sync";
  syncBtn.disabled = Boolean(state.backupLoading);
  syncBtn.innerHTML = `${icon("upload")}<span data-role="backup-sync-label">Sync to Google Drive (${escapeHtml(
    gdriveFolder
  )})</span>`;
  syncBtn.addEventListener("click", () => {
    const cbs = container.__backupCallbacks;
    if (cbs?.onSyncGdrive) {
      cbs.onSyncGdrive(passInput.value);
    } else {
      cbs?.onCreateBackup?.(passInput.value, true);
    }
  });

  actionRow.append(snapBtn, syncBtn);
  passBox.appendChild(actionRow);
  body.appendChild(passBox);

  const dynRegion = document.createElement("div");
  dynRegion.dataset.region = "backup-dynamic";
  dynRegion.style.display = "contents";
  renderBackupDynamicRegion(
    dynRegion,
    state,
    () => passInput.value,
    () => container.__backupCallbacks
  );
  body.appendChild(dynRegion);

  sheet.appendChild(body);
  backdrop.appendChild(sheet);

  container.__backupClose = bindModalEscape(container, backdrop, () =>
    container.__backupCallbacks?.onClose?.()
  );
  closeBtn.addEventListener("click", () => container.__backupClose?.());
  backdrop.addEventListener("click", (ev) => {
    if (ev.target === backdrop) container.__backupClose?.();
  });

  container.appendChild(backdrop);
}
