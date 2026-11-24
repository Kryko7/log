// Pure ES-module editorial views, tactile Polaroid cards, stamps/stickers,
// search modal, archive migration modal, and encrypted backup modal.

import { icon, weatherIcon } from "./icons.js";
import { resolveVaultUrl } from "./ipc.js";
import { parseCoverAsset, mountCoverCinemagraph } from "./cover-fx.js";

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
  } else if (!showTitle) {
    const fallbackLabel = entry.has_photos
      ? "Photo entry"
      : entry.has_audio
        ? "Voice note"
        : "Untitled entry";
    proseCol.appendChild(el("p", "timeline-row-excerpt is-muted", fallbackLabel));
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
  const baseEntries = [...(state.filteredEntries ?? state.entries ?? [])];
  const notebooks = state.notebooks ?? [];
  const nbMap = new Map(notebooks.map((nb) => [nb.id, nb.name]));

  const selectedNb = state.selectedNotebookId
    ? notebooks.find((nb) => nb.id === state.selectedNotebookId) || null
    : null;
  const selectedTag = state.selectedTag || null;
  const isStarredView = Boolean(state.starredOnly);
  const hasFilter = Boolean(state.selectedNotebookId || selectedTag || isStarredView);

  let headingTitle = "All Entries";
  let subtitlePrefix = "";
  let clearLabel = "Show all entries";
  if (isStarredView && !selectedNb && !selectedTag) {
    headingTitle = "Starred Entries";
    subtitlePrefix = "Favorites";
    clearLabel = "Show all entries";
  } else if (selectedNb || state.selectedNotebookId) {
    headingTitle = selectedNb?.name || "Notebook";
    subtitlePrefix = selectedTag
      ? `Notebook · #${selectedTag}`
      : isStarredView
        ? "Notebook · Starred"
        : "Notebook";
    clearLabel = "Show all entries";
  } else if (selectedTag) {
    headingTitle = `#${selectedTag}`;
    subtitlePrefix = isStarredView ? "Tag · Starred" : "Tag";
    clearLabel = "Clear filter";
  }

  const section = el("section", "timeline-river");
  section.setAttribute("aria-label", headingTitle);

  const header = el("header", "stage-header");
  const titleWrap = el("div");
  titleWrap.appendChild(el("h1", "stage-serif-title", headingTitle));
  const subtitleEl = el("p", "stage-subtitle", "");
  titleWrap.appendChild(subtitleEl);
  const updateSubtitleCount = (count) => {
    const countLabel = `${count} ${count === 1 ? "entry" : "entries"}`;
    subtitleEl.textContent = subtitlePrefix
      ? `${subtitlePrefix} · ${countLabel}`
      : countLabel;
  };
  updateSubtitleCount(baseEntries.length);

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
      const builtInCovers = state.manifest?.covers || [];
      const renderSwatches = () => {
        coverRow.innerHTML = "";
        for (const cov of builtInCovers) {
          const { basePath, fx } = parseCoverAsset(cov.path);
          const isAnim = Boolean(
            fx || (Array.isArray(cov.tags) && cov.tags.includes("animated"))
          );
          const sw = el(
            "button",
            `cover-swatch-btn${draftCover === cov.path ? " selected" : ""}${
              isAnim ? " is-animated" : ""
            }`
          );
          sw.type = "button";
          sw.title = isAnim
            ? `${cov.label || "Living cover"} (Animated)`
            : cov.label || "Cover wash";
          const img = document.createElement("img");
          img.src = resolveVaultUrl(basePath || cov.path);
          img.alt = cov.label || "Cover";
          img.decoding = "async";
          sw.appendChild(img);
          if (isAnim) {
            const dot = el("span", "cover-live-dot");
            dot.setAttribute("aria-hidden", "true");
            sw.appendChild(dot);
          }
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
      const nbCount = Number(selectedNb.entry_count ?? baseEntries.length ?? 0);
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
      const { basePath, fx } = parseCoverAsset(selectedNb.cover_asset);
      const frame = el(
        "button",
        `notebook-cover-frame${fx ? " is-animated-cover" : ""}`
      );
      frame.type = "button";
      frame.style.borderColor = selectedNb.accent_color || "var(--border-strong)";
      frame.title = callbacks.onUpdateNotebook
        ? "Change notebook cover"
        : selectedNb.name;
      const covImg = document.createElement("img");
      covImg.src = resolveVaultUrl(basePath || selectedNb.cover_asset);
      covImg.alt = `${selectedNb.name} cover`;
      covImg.decoding = "async";
      frame.appendChild(covImg);
      if (fx) {
        mountCoverCinemagraph(frame, fx, covImg);
      }
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

  // Quiet Timeline Filter & Sort Toolbar (only shown when baseEntries has entries)
  const localFilter = {
    text: "",
    starred: false,
    photos: false,
    audio: false,
    mood: "",
    sort: "newest",
  };

  const listRegion = el("div", "timeline-list-region");

  if (baseEntries.length > 0) {
    const filterBar = el("div", "timeline-filter-bar");

    const filterSearchWrap = el("div", "timeline-filter-search");
    filterSearchWrap.innerHTML = icon("search", "tiny-icon");
    const filterInput = document.createElement("input");
    filterInput.type = "text";
    filterInput.className = "timeline-filter-input";
    filterInput.placeholder = "Filter these entries…";
    filterInput.setAttribute("aria-label", "Filter entries");
    filterSearchWrap.appendChild(filterInput);
    filterBar.appendChild(filterSearchWrap);

    const pills = el("div", "timeline-filter-pills");

    const makeTogglePill = (label, icName, key) => {
      const btn = el("button", "filter-pill-btn");
      btn.type = "button";
      btn.innerHTML = `${icon(icName, "tiny-icon")}<span>${escapeHtml(label)}</span>`;
      btn.addEventListener("click", () => {
        localFilter[key] = !localFilter[key];
        btn.classList.toggle("is-active", localFilter[key]);
        renderFilteredRiver();
      });
      return btn;
    };

    if (!isStarredView) {
      pills.appendChild(makeTogglePill("Starred", "star", "starred"));
    }
    pills.appendChild(makeTogglePill("Photos", "camera", "photos"));
    pills.appendChild(makeTogglePill("Voice", "mic", "audio"));

    const availableMoods = Array.from(
      new Set(baseEntries.map((e) => e.mood).filter(Boolean))
    ).sort();
    if (availableMoods.length > 0) {
      const moodSel = document.createElement("select");
      moodSel.className = "filter-pill-select";
      moodSel.setAttribute("aria-label", "Filter by mood");
      const allM = document.createElement("option");
      allM.value = "";
      allM.textContent = "All moods";
      moodSel.appendChild(allM);
      for (const m of availableMoods) {
        const opt = document.createElement("option");
        opt.value = m;
        opt.textContent = m;
        moodSel.appendChild(opt);
      }
      moodSel.addEventListener("change", () => {
        localFilter.mood = moodSel.value;
        renderFilteredRiver();
      });
      pills.appendChild(moodSel);
    }

    const sortSel = document.createElement("select");
    sortSel.className = "filter-pill-select";
    sortSel.setAttribute("aria-label", "Sort entries");
    for (const [val, txt] of [
      ["newest", "Newest first"],
      ["oldest", "Oldest first"],
      ["longest", "Longest first"],
    ]) {
      const opt = document.createElement("option");
      opt.value = val;
      opt.textContent = txt;
      sortSel.appendChild(opt);
    }
    sortSel.addEventListener("change", () => {
      localFilter.sort = sortSel.value;
      renderFilteredRiver();
    });
    pills.appendChild(sortSel);

    filterBar.appendChild(pills);
    section.appendChild(filterBar);

    filterInput.addEventListener("input", () => {
      localFilter.text = filterInput.value.trim().toLowerCase();
      renderFilteredRiver();
    });
  }

  function renderFilteredRiver() {
    listRegion.innerHTML = "";
    const hasLocalFilter = Boolean(
      localFilter.text || localFilter.starred || localFilter.photos || localFilter.audio || localFilter.mood
    );
    const entries = baseEntries.filter((e) => {
      if (localFilter.starred && !e.starred) return false;
      if (localFilter.photos && !e.has_photos) return false;
      if (localFilter.audio && !e.has_audio) return false;
      if (localFilter.mood && e.mood !== localFilter.mood) return false;
      if (localFilter.text) {
        const hay = `${e.title || ""} ${e.snippet || ""} ${(e.tags || []).join(" ")} ${
          e.location?.place_name || ""
        }`.toLowerCase();
        if (!hay.includes(localFilter.text)) return false;
      }
      return true;
    });

    if (localFilter.sort === "oldest") {
      entries.sort((a, b) => new Date(a.date).getTime() - new Date(b.date).getTime());
    } else if (localFilter.sort === "longest") {
      entries.sort((a, b) => (b.word_count || 0) - (a.word_count || 0));
    } else {
      entries.sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime());
    }

    updateSubtitleCount(entries.length);

    if (entries.length === 0) {
      const empty = el("div", "editorial-empty-card");
      empty.innerHTML = icon("feather", "empty-emblem");
      empty.appendChild(
        el("h2", "empty-serif-heading", hasLocalFilter ? "No matching entries" : "No entries yet")
      );
      empty.appendChild(
        el(
          "p",
          "empty-prose-note",
          hasLocalFilter || hasFilter
            ? "No entries match this filter yet."
            : "Start writing a new entry to begin this journal."
        )
      );
      const emptyActions = el("div", "stage-header-actions");
      if (hasFilter && !hasLocalFilter && callbacks.onClearFilters) {
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
      listRegion.appendChild(empty);
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
    const groupByMonth = localFilter.sort !== "longest";

    function appendNextBatch() {
      const slice = entries.slice(renderedCount, renderedCount + TIMELINE_BATCH_SIZE);
      if (slice.length === 0) return;

      const frag = document.createDocumentFragment();
      for (const entry of slice) {
        if (groupByMonth) {
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
        } else if (!currentRowsEl) {
          const groupEl = el("section", "timeline-month-group");
          currentRowsEl = el("div", "timeline-rows");
          groupEl.appendChild(currentRowsEl);
          frag.appendChild(groupEl);
        }

        const nbName = entry.notebook_id ? nbMap.get(entry.notebook_id) : null;
        currentRowsEl.appendChild(
          buildTimelineRow(entry, nbName, callbacks, groupByMonth ? "weekday" : "year")
        );
        currentMonthCount += 1;
        if (groupByMonth && currentCountEl) {
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
          `Show more (${remaining} more ${remaining === 1 ? "entry" : "entries"})`
        );
        moreBtn.type = "button";
        moreBtn.addEventListener("click", appendNextBatch);
        loadMoreRow.appendChild(moreBtn);
      }
    }

    appendNextBatch();
    listRegion.appendChild(monthsList);
    listRegion.appendChild(loadMoreRow);
  }

  renderFilteredRiver();
  section.appendChild(listRegion);
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
  const notebooks = state.notebooks || [];
  const nbMap = new Map(notebooks.map((nb) => [nb.id, nb.name]));

  const dayMap = new Map(days.map((d) => [d.date, d]));
  const entryMap = new Map(entries.map((e) => [e.id, e]));

  const entriesForDate = (dateStr) =>
    entries
      .filter((e) => {
        if (state.selectedNotebookId && e.notebook_id !== state.selectedNotebookId) {
          return false;
        }
        return String(e.date || "").slice(0, 10) === dateStr;
      })
      .sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime());

  const section = el("section", "calendar-stage");
  section.setAttribute("aria-label", "Calendar");

  const header = el("header", "stage-header");
  const titleWrap = el("div");
  titleWrap.appendChild(el("h1", "stage-serif-title", formatMonthYear(year, month)));
  const activeNbName = state.selectedNotebookId
    ? nbMap.get(state.selectedNotebookId) || state.selectedNotebookId
    : "All Notebooks";
  const monthEntryTotal = days.reduce((acc, d) => acc + Number(d.entry_count || 0), 0);
  titleWrap.appendChild(
    el(
      "p",
      "stage-subtitle",
      `${activeNbName} · ${monthEntryTotal} ${monthEntryTotal === 1 ? "entry" : "entries"}`
    )
  );
  header.appendChild(titleWrap);

  const nav = el("div", "calendar-nav-controls");

  if (notebooks.length > 0 && callbacks.onChangeNotebookFilter) {
    const nbSel = document.createElement("select");
    nbSel.className = "filter-pill-select";
    nbSel.setAttribute("aria-label", "Filter calendar by notebook");
    const allOpt = document.createElement("option");
    allOpt.value = "";
    allOpt.textContent = "All Notebooks";
    nbSel.appendChild(allOpt);
    for (const nb of notebooks) {
      const opt = document.createElement("option");
      opt.value = nb.id;
      opt.textContent = nb.name;
      if (state.selectedNotebookId === nb.id) opt.selected = true;
      nbSel.appendChild(opt);
    }
    nbSel.addEventListener("change", () => {
      callbacks.onChangeNotebookFilter(nbSel.value || null);
    });
    nav.appendChild(nbSel);
  }

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

  let selectedDateStr = state.calendarSelectedDate || null;
  if (selectedDateStr && !selectedDateStr.startsWith(`${year}-${String(month).padStart(2, "0")}-`)) {
    selectedDateStr = null;
  }

  const dayDrawer = el("section", "calendar-day-drawer");
  dayDrawer.hidden = true;

  const renderDayDrawer = () => {
    dayDrawer.innerHTML = "";
    if (!selectedDateStr) {
      dayDrawer.hidden = true;
      return;
    }
    const dayEntries = entriesForDate(selectedDateStr);
    dayDrawer.hidden = false;

    const head = el("div", "calendar-day-drawer-head");
    const parsed = new Date(`${selectedDateStr}T12:00:00`);
    const humanDate = !Number.isNaN(parsed.getTime())
      ? parsed.toLocaleDateString(undefined, {
          weekday: "long",
          month: "long",
          day: "numeric",
          year: "numeric",
        })
      : selectedDateStr;

    const headTitle = el("div");
    headTitle.appendChild(el("h2", "calendar-drawer-heading", humanDate));
    headTitle.appendChild(
      el(
        "span",
        "small-caps-meta",
        `${dayEntries.length} ${dayEntries.length === 1 ? "entry" : "entries"}`
      )
    );

    const headActions = el("div", "stage-header-actions");
    const addDayBtn = el("button", "btn-editorial btn-sage small");
    addDayBtn.type = "button";
    addDayBtn.innerHTML = `${icon("plus", "tiny-icon")}<span>New entry on this day</span>`;
    addDayBtn.addEventListener("click", () => {
      callbacks.onNewEntryForDate?.(selectedDateStr);
    });
    headActions.appendChild(addDayBtn);
    head.append(headTitle, headActions);
    dayDrawer.appendChild(head);

    if (dayEntries.length === 0) {
      dayDrawer.appendChild(
        el("p", "empty-prose-note", "No entries recorded on this date yet.")
      );
      return;
    }

    const rowsEl = el("div", "timeline-rows");
    for (const entry of dayEntries) {
      const nbName = entry.notebook_id ? nbMap.get(entry.notebook_id) : null;
      rowsEl.appendChild(buildTimelineRow(entry, nbName, callbacks, "weekday"));
    }
    dayDrawer.appendChild(rowsEl);
  };

  for (let i = 0; i < leadingEmpty; i++) {
    const pad = el("div", "calendar-day-cell is-pad");
    pad.setAttribute("aria-hidden", "true");
    grid.appendChild(pad);
  }

  const cellNodes = new Map();

  for (let day = 1; day <= daysInMonth; day++) {
    const dateStr = `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    const summary = dayMap.get(dateStr) || null;
    const primaryEntry = summary?.primary_entry_id
      ? entryMap.get(summary.primary_entry_id)
      : null;

    const cell = el("button", "calendar-day-cell");
    cell.type = "button";
    if (dateStr === todayStr) cell.classList.add("is-today");
    if (dateStr === selectedDateStr) cell.classList.add("is-selected");
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
      const dayEntries = entriesForDate(dateStr);
      if (dayEntries.length === 0 && !summary?.entry_count) {
        callbacks.onNewEntryForDate?.(dateStr);
        return;
      }
      selectedDateStr = dateStr;
      state.calendarSelectedDate = dateStr;
      for (const [k, node] of cellNodes.entries()) {
        node.classList.toggle("is-selected", k === dateStr);
      }
      renderDayDrawer();
      dayDrawer.scrollIntoView({ behavior: "smooth", block: "nearest" });
    });

    cell.addEventListener("dblclick", () => {
      if (summary?.primary_entry_id) {
        callbacks.onOpenEntry?.(summary.primary_entry_id);
      } else {
        callbacks.onNewEntryForDate?.(dateStr);
      }
    });

    cellNodes.set(dateStr, cell);
    grid.appendChild(cell);
  }

  sheet.appendChild(grid);
  section.appendChild(sheet);
  renderDayDrawer();
  section.appendChild(dayDrawer);
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

export function openPolaroidLightbox(photosOrPhoto, startIndex = 0) {
  const photos = Array.isArray(photosOrPhoto) ? photosOrPhoto : [photosOrPhoto];
  if (photos.length === 0) return;
  let currentIdx = Math.max(0, Math.min(startIndex, photos.length - 1));

  const backdrop = el("div", "editorial-modal-backdrop");
  const sheet = el("div", "polaroid-lightbox-sheet");

  const closeBtn = el("button", "polaroid-lightbox-close");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close photograph");
  closeBtn.innerHTML = icon("close");

  const img = document.createElement("img");
  img.className = "polaroid-lightbox-img";
  img.decoding = "async";

  const captionEl = el("p", "polaroid-lightbox-caption", "");
  const metaEl = el("p", "polaroid-lightbox-meta small-caps-meta", "");

  let prevBtn = null;
  let nextBtn = null;
  if (photos.length > 1) {
    prevBtn = el("button", "polaroid-lightbox-nav is-prev");
    prevBtn.type = "button";
    prevBtn.setAttribute("aria-label", "Previous photograph");
    prevBtn.innerHTML = icon("chevronLeft");

    nextBtn = el("button", "polaroid-lightbox-nav is-next");
    nextBtn.type = "button";
    nextBtn.setAttribute("aria-label", "Next photograph");
    nextBtn.innerHTML = icon("chevronRight");
  }

  const renderCurrent = () => {
    const p = photos[currentIdx];
    img.src = resolveVaultUrl(p.url || p.thumbnail_url);
    img.alt = p.caption || "Journal photograph";
    captionEl.textContent = p.caption || "";
    captionEl.hidden = !p.caption;

    const metaParts = [];
    if (photos.length > 1) {
      metaParts.push(`${currentIdx + 1} / ${photos.length}`);
    }
    if (p.location?.place_name) {
      metaParts.push(p.location.place_name);
    }
    if (p.taken_at) {
      const dt = new Date(p.taken_at);
      if (!Number.isNaN(dt.getTime())) {
        metaParts.push(dt.toLocaleDateString());
      }
    }
    metaEl.textContent = metaParts.join(" · ");
    metaEl.hidden = metaParts.length === 0;
  };

  const step = (delta) => {
    if (photos.length <= 1) return;
    currentIdx = (currentIdx + delta + photos.length) % photos.length;
    renderCurrent();
  };

  sheet.appendChild(closeBtn);
  if (prevBtn) sheet.appendChild(prevBtn);
  sheet.appendChild(img);
  if (nextBtn) sheet.appendChild(nextBtn);
  sheet.append(captionEl, metaEl);
  renderCurrent();

  const ac = new AbortController();
  function close() {
    ac.abort();
    backdrop.remove();
  }
  closeBtn.addEventListener("click", close, { signal: ac.signal });
  prevBtn?.addEventListener("click", () => step(-1), { signal: ac.signal });
  nextBtn?.addEventListener("click", () => step(1), { signal: ac.signal });
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
      else if (ev.key === "ArrowLeft") step(-1);
      else if (ev.key === "ArrowRight") step(1);
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
    input.addEventListener("input", () => {
      callbacks.onUpdateCaption(photo.id, input.value);
    });
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
export function renderStampOverlay(
  container,
  stamps = [],
  editable = false,
  onChange = null,
  justPlacedId = null
) {
  if (container._stampOverlayAbort) {
    container._stampOverlayAbort.abort();
    container._stampOverlayAbort = null;
  }
  container.innerHTML = "";
  const list = Array.isArray(stamps) ? stamps.map((s) => ({ ...s })) : [];
  if (list.length === 0) return;

  let selectedId = null;
  const overlayAbort = new AbortController();
  container._stampOverlayAbort = overlayAbort;

  const layer = el("div", "stamp-sticker-layer");
  if (editable) layer.classList.add("is-editable");

  function commit(nextList) {
    onChange?.(nextList.map((s) => ({ ...s })));
  }

  function removeStampById(stampId) {
    const idx = list.findIndex((item) => item.id === stampId);
    if (idx < 0) return;
    list.splice(idx, 1);
    if (selectedId === stampId) selectedId = null;
    commit(list);
    redraw();
  }

  if (editable) {
    document.addEventListener(
      "mousedown",
      (ev) => {
        if (!layer.isConnected) {
          overlayAbort.abort();
          return;
        }
        if (selectedId && !ev.target?.closest?.(".placed-stamp-node")) {
          selectedId = null;
          redraw();
        }
      },
      { signal: overlayAbort.signal }
    );

    window.addEventListener(
      "keydown",
      (ev) => {
        if (!layer.isConnected) {
          overlayAbort.abort();
          return;
        }
        if (!selectedId) return;
        const active = document.activeElement;
        const inTextField =
          active &&
          (active.tagName === "INPUT" ||
            active.tagName === "TEXTAREA" ||
            active.isContentEditable);
        if (inTextField) return;

        if (ev.key === "Delete" || ev.key === "Backspace") {
          ev.preventDefault();
          removeStampById(selectedId);
        } else if (ev.key === "Escape") {
          ev.preventDefault();
          selectedId = null;
          redraw();
        }
      },
      { signal: overlayAbort.signal }
    );
  }

  function redraw() {
    layer.innerHTML = "";
    for (const st of list) {
      const pathStr = String(st.asset_path || "");
      const isPostmark = pathStr.includes("postmark-");
      const isWashi = pathStr.includes("washi");
      const kindClass = isWashi ? "is-washi" : isPostmark ? "is-postmark" : "is-diecut";
      const node = el(
        "div",
        `placed-stamp-node ${kindClass}${
          editable && selectedId === st.id ? " is-selected" : ""
        }${justPlacedId && st.id === justPlacedId ? " is-just-placed" : ""}`
      );
      node.style.left = `${st.x_pct}%`;
      node.style.top = `${st.y_pct}%`;
      node.style.setProperty("--stamp-rot", `${st.rotation_deg || 0}deg`);
      node.style.setProperty("--stamp-scale", String(st.scale || 1));
      node.style.transform = `translate(-50%, -50%) rotate(${st.rotation_deg || 0}deg) scale(${
        st.scale || 1
      })`;
      node.dataset.stampId = st.id;

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

      if (st.label && !isWashi) {
        node.appendChild(el("span", "placed-stamp-label", st.label));
      }

      if (editable) {
        node.tabIndex = 0;
        node.setAttribute("role", "group");
        node.setAttribute("aria-label", `Stamp: ${st.label || st.asset_id || "Sticker"}`);

        // Always-reachable one-click corner remove button (visible on hover, focus, or selection)
        const quickRemove = el("button", "stamp-quick-remove");
        quickRemove.type = "button";
        quickRemove.title = "Remove stamp";
        quickRemove.setAttribute("aria-label", `Remove ${st.label || st.asset_id || "stamp"}`);
        quickRemove.innerHTML = icon("close", "tiny-icon");
        quickRemove.addEventListener("mousedown", (ev) => {
          ev.stopPropagation();
        });
        quickRemove.addEventListener("click", (ev) => {
          ev.stopPropagation();
          ev.preventDefault();
          removeStampById(st.id);
        });
        node.appendChild(quickRemove);

        node.addEventListener("focus", () => {
          if (selectedId !== st.id) {
            selectedId = st.id;
            redraw();
          }
        });

        node.addEventListener("mousedown", (ev) => {
          if (
            ev.target.closest(".stamp-mini-controls") ||
            ev.target.closest(".stamp-quick-remove")
          ) {
            return;
          }
          ev.preventDefault();
          ev.stopPropagation();
          const wasSelected = selectedId === st.id;
          selectedId = st.id;
          layer.classList.add("is-dragging");
          if (!wasSelected) {
            redraw();
          }

          const rect = layer.getBoundingClientRect();
          const dragAc = new AbortController();
          let moved = false;
          function onMove(mv) {
            if (!layer.isConnected) {
              dragAc.abort();
              return;
            }
            if (rect.width <= 0 || rect.height <= 0) return;
            moved = true;
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
            if (moved) {
              commit(list);
            }
          }
          window.addEventListener("mousemove", onMove, { signal: dragAc.signal });
          window.addEventListener("mouseup", onUp, { signal: dragAc.signal });
        });

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
          del.title = "Remove stamp (Delete)";
          del.innerHTML = `${icon("close", "tiny-icon")}<span>Remove</span>`;
          del.addEventListener("click", (ev) => {
            ev.stopPropagation();
            removeStampById(st.id);
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
 * @param {Array} placedStamps
 * @param {function|null} onRemoveStamp
 * @param {function|null} onClearAllStamps
 */
export function renderStampPickerPopover(
  container,
  manifest,
  onPick,
  onClose,
  placedStamps = [],
  onRemoveStamp = null,
  onClearAllStamps = null,
  initialTab = "stickers",
  onTabChange = null
) {
  container.innerHTML = "";
  let activeTab = initialTab || "stickers";

  const pop = el("div", "stamp-picker-popover");
  const head = el("div", "stamp-picker-header");
  const titleWrap = el("div", "stamp-picker-title");
  titleWrap.innerHTML = icon("stamp", "popover-icon");
  titleWrap.appendChild(el("span", "small-caps-meta", "Stamps, Stickers & Washi"));
  head.appendChild(titleWrap);

  const closeBtn = el("button", "icon-btn");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close stamp picker");
  closeBtn.innerHTML = icon("close");
  closeBtn.addEventListener("click", () => onClose?.());
  head.appendChild(closeBtn);
  pop.appendChild(head);

  if (Array.isArray(placedStamps) && placedStamps.length > 0) {
    const placedSection = el("div", "stamp-placed-section");
    const placedHead = el("div", "stamp-placed-head");
    placedHead.appendChild(
      el("span", "small-caps-meta", `On This Page (${placedStamps.length})`)
    );
    if (onClearAllStamps) {
      const clearBtn = el("button", "stamp-clear-all-btn", "Remove all");
      clearBtn.type = "button";
      clearBtn.addEventListener("click", () => onClearAllStamps());
      placedHead.appendChild(clearBtn);
    }
    placedSection.appendChild(placedHead);

    const chips = el("div", "stamp-placed-list");
    for (const st of placedStamps) {
      const chip = el("span", "stamp-placed-chip");
      const thumbUrl = safeAssetUrl(st.asset_path);
      if (thumbUrl) {
        const thumb = document.createElement("img");
        thumb.className = "stamp-placed-thumb";
        thumb.src = thumbUrl;
        thumb.alt = "";
        chip.appendChild(thumb);
      }
      chip.appendChild(
        el("span", "stamp-placed-name", st.label || st.asset_id || "Stamp")
      );
      if (onRemoveStamp) {
        const rm = el("button", "stamp-placed-remove");
        rm.type = "button";
        rm.title = `Remove ${st.label || "stamp"}`;
        rm.setAttribute("aria-label", `Remove ${st.label || "stamp"}`);
        rm.innerHTML = icon("close", "tiny-icon");
        rm.addEventListener("click", () => onRemoveStamp(st.id));
        chip.appendChild(rm);
      }
      chips.appendChild(chip);
    }
    placedSection.appendChild(chips);
    pop.appendChild(placedSection);
  }

  const tabs = el("div", "stamp-picker-tabs");
  const stickerTab = el("button", "stamp-tab-btn is-active", "Stickers");
  stickerTab.type = "button";
  const postmarkTab = el("button", "stamp-tab-btn", "Postmarks");
  postmarkTab.type = "button";
  const washiTab = el("button", "stamp-tab-btn", "Washi");
  washiTab.type = "button";
  tabs.append(stickerTab, postmarkTab, washiTab);
  pop.appendChild(tabs);

  const grid = el("div", "stamp-choices-grid");

  function renderChoices() {
    grid.innerHTML = "";
    stickerTab.classList.toggle("is-active", activeTab === "stickers");
    postmarkTab.classList.toggle("is-active", activeTab === "stamps");
    washiTab.classList.toggle("is-active", activeTab === "washi");

    const items =
      activeTab === "stickers"
        ? manifest?.stickers || []
        : activeTab === "stamps"
          ? manifest?.stamps || []
          : manifest?.washi || [];

    for (const item of items) {
      const btn = el("button", "stamp-choice-card");
      btn.type = "button";
      const img = document.createElement("img");
      img.src = safeAssetUrl(item.path);
      img.loading = "lazy";
      img.decoding = "async";
      img.alt = item.label;
      img.className =
        activeTab === "stickers"
          ? "stamp-preview-diecut"
          : activeTab === "washi"
            ? "stamp-preview-washi"
            : "stamp-preview-ink";
      btn.appendChild(img);
      btn.appendChild(el("span", "stamp-choice-label", item.label));
      btn.addEventListener("click", () => {
        onPick?.(item);
      });
      grid.appendChild(btn);
    }
  }

  stickerTab.addEventListener("click", () => {
    activeTab = "stickers";
    onTabChange?.(activeTab);
    renderChoices();
  });
  postmarkTab.addEventListener("click", () => {
    activeTab = "stamps";
    onTabChange?.(activeTab);
    renderChoices();
  });
  washiTab.addEventListener("click", () => {
    activeTab = "washi";
    onTabChange?.(activeTab);
    renderChoices();
  });

  renderChoices();
  pop.appendChild(grid);
  container.appendChild(pop);
}

function decodeBasicHtmlEntities(str) {
  return String(str ?? "")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, "&");
}

function appendSafeHighlightedSnippet(parent, snippetHtml) {
  const raw = String(snippetHtml || "");
  const parts = raw.split(/(<mark>.*?<\/mark>)/gi);
  for (const part of parts) {
    const m = /^<mark>([\s\S]*)<\/mark>$/i.exec(part);
    if (m) {
      const mark = document.createElement("mark");
      mark.textContent = decodeBasicHtmlEntities(m[1]);
      parent.appendChild(mark);
    } else {
      parent.appendChild(document.createTextNode(decodeBasicHtmlEntities(part)));
    }
  }
}

/**
 * Render the FTS5 full-text Search modal with structured filters & keyboard navigation.
 * @param {HTMLElement} container
 * @param {object} state
 * @param {object} callbacks
 */
export function renderSearchModal(container, state, callbacks = {}) {
  container.__searchCallbacks = callbacks;
  const existing = container.querySelector('[data-log-modal="search"]');
  const hits = state.searchResults || [];
  const filters = state.searchFilters || {
    starred_only: false,
    notebook_id: "",
    mood: "",
    tag: "",
    from_date: "",
    to_date: "",
  };
  const hasAnyCriteria = Boolean(
    (state.searchQuery || "").trim() ||
      filters.starred_only ||
      filters.notebook_id ||
      filters.mood ||
      filters.tag ||
      filters.from_date ||
      filters.to_date
  );

  function fillResults(listEl) {
    listEl.innerHTML = "";
    container.__searchHits = hits;
    if (typeof container.__searchActiveIdx !== "number" || container.__searchActiveIdx >= hits.length) {
      container.__searchActiveIdx = 0;
    }

    if (state.searchLoading) {
      listEl.appendChild(el("div", "empty-prose-note", "Searching…"));
      return;
    }
    if (!hasAnyCriteria) {
      listEl.appendChild(
        el(
          "div",
          "empty-prose-note",
          "Type keywords or select a filter below. Use Up/Down arrows and Enter to jump to an entry."
        )
      );
      return;
    }
    if (!hits.length) {
      listEl.appendChild(el("div", "empty-prose-note", "No matching entries found."));
      return;
    }

    hits.forEach((hit, idx) => {
      const summary = hit.summary || hit;
      const card = el(
        "button",
        `search-hit-card${idx === container.__searchActiveIdx ? " is-selected" : ""}`
      );
      card.type = "button";
      card.dataset.hitIdx = String(idx);

      const head = el("div", "search-hit-head");
      const titleText = isAutoDerivedTitle(summary.title, summary.snippet)
        ? formatDateParts(summary.date).full
        : summary.title;
      head.appendChild(el("span", "search-hit-title", titleText));
      if (summary.starred) {
        const star = el("span", "meta-star-mark");
        star.innerHTML = icon("starFilled", "tiny-icon");
        head.appendChild(star);
      }
      head.appendChild(el("span", "small-caps-meta", formatDateParts(summary.date).full));
      card.appendChild(head);

      const snip = el("p", "search-hit-snippet");
      appendSafeHighlightedSnippet(
        snip,
        hit.highlighted_snippet || cleanExcerptSnippet(summary.snippet)
      );
      card.appendChild(snip);

      const hitMetaParts = [];
      const nbObj = (state.notebooks || []).find((n) => n.id === summary.notebook_id);
      if (nbObj?.name) hitMetaParts.push(nbObj.name);
      if (summary.mood) hitMetaParts.push(summary.mood);
      if (Array.isArray(summary.tags) && summary.tags.length > 0) {
        hitMetaParts.push(...summary.tags.slice(0, 3).map((t) => `#${t}`));
      }
      if (hitMetaParts.length > 0) {
        card.appendChild(el("div", "search-hit-meta small-caps-meta", hitMetaParts.join(" · ")));
      }

      card.addEventListener("mouseenter", () => {
        container.__searchActiveIdx = idx;
        listEl.querySelectorAll(".search-hit-card").forEach((c, i) => {
          c.classList.toggle("is-selected", i === idx);
        });
      });

      card.addEventListener("click", () => {
        container.__searchCallbacks?.onSelectEntry?.(summary.id);
        container.__searchCallbacks?.onClose?.();
      });
      listEl.appendChild(card);
    });
  }

  if (existing) {
    const existingInput = existing.querySelector(".search-modal-input");
    const existingResults = existing.querySelector(".search-results-list");
    if (existingInput && document.activeElement !== existingInput && existingInput.value !== (state.searchQuery || "")) {
      existingInput.value = state.searchQuery || "";
    }
    const starChip = existing.querySelector('[data-search-filter="starred"]');
    if (starChip) starChip.classList.toggle("is-active", Boolean(filters.starred_only));
    const nbSel = existing.querySelector('[data-search-filter="notebook"]');
    if (nbSel && nbSel.value !== (filters.notebook_id || "")) nbSel.value = filters.notebook_id || "";
    const moodSel = existing.querySelector('[data-search-filter="mood"]');
    if (moodSel && moodSel.value !== (filters.mood || "")) moodSel.value = filters.mood || "";
    const tagSel = existing.querySelector('[data-search-filter="tag"]');
    if (tagSel && tagSel.value !== (filters.tag || "")) tagSel.value = filters.tag || "";
    const fromIn = existing.querySelector('[data-search-filter="from_date"]');
    if (fromIn && fromIn.value !== (filters.from_date || "")) fromIn.value = filters.from_date || "";
    const toIn = existing.querySelector('[data-search-filter="to_date"]');
    if (toIn && toIn.value !== (filters.to_date || "")) toIn.value = filters.to_date || "";
    if (existingResults) fillResults(existingResults);
    return;
  }

  container.innerHTML = "";
  container.__searchActiveIdx = 0;

  const backdrop = el("div", "search-modal-backdrop");
  backdrop.dataset.logModal = "search";
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
  input.placeholder = "Search entries, places, tags…";
  input.value = state.searchQuery || "";
  input.addEventListener("input", () => {
    container.__searchActiveIdx = 0;
    container.__searchCallbacks?.onQueryChange?.(input.value);
  });
  bar.appendChild(input);

  const closeBtn = el("button", "icon-btn");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close search");
  closeBtn.innerHTML = icon("close");
  bar.appendChild(closeBtn);
  sheet.appendChild(bar);

  // Structured Filter Bar
  const filterRow = el("div", "search-filters-bar");

  const starBtn = el(
    "button",
    `filter-pill-btn${filters.starred_only ? " is-active" : ""}`
  );
  starBtn.type = "button";
  starBtn.dataset.searchFilter = "starred";
  starBtn.innerHTML = `${icon("star", "tiny-icon")}<span>Starred</span>`;
  starBtn.addEventListener("click", () => {
    container.__searchActiveIdx = 0;
    container.__searchCallbacks?.onFilterChange?.({
      starred_only: !state.searchFilters?.starred_only,
    });
  });
  filterRow.appendChild(starBtn);

  const nbSel = document.createElement("select");
  nbSel.className = "filter-pill-select";
  nbSel.dataset.searchFilter = "notebook";
  nbSel.setAttribute("aria-label", "Filter search by notebook");
  nbSel.appendChild(Object.assign(document.createElement("option"), { value: "", textContent: "All notebooks" }));
  for (const nb of state.notebooks || []) {
    const opt = document.createElement("option");
    opt.value = nb.id;
    opt.textContent = nb.name;
    if (filters.notebook_id === nb.id) opt.selected = true;
    nbSel.appendChild(opt);
  }
  nbSel.addEventListener("change", () => {
    container.__searchActiveIdx = 0;
    container.__searchCallbacks?.onFilterChange?.({ notebook_id: nbSel.value });
  });
  filterRow.appendChild(nbSel);

  const moodSel = document.createElement("select");
  moodSel.className = "filter-pill-select";
  moodSel.dataset.searchFilter = "mood";
  moodSel.setAttribute("aria-label", "Filter search by mood");
  moodSel.appendChild(Object.assign(document.createElement("option"), { value: "", textContent: "Any mood" }));
  for (const m of [
    "reflective",
    "calm",
    "grateful",
    "energized",
    "melancholic",
    "curious",
    "restless",
    "joyful",
  ]) {
    const opt = document.createElement("option");
    opt.value = m;
    opt.textContent = m;
    if (filters.mood === m) opt.selected = true;
    moodSel.appendChild(opt);
  }
  moodSel.addEventListener("change", () => {
    container.__searchActiveIdx = 0;
    container.__searchCallbacks?.onFilterChange?.({ mood: moodSel.value });
  });
  filterRow.appendChild(moodSel);

  if ((state.tags || []).length > 0) {
    const tagSel = document.createElement("select");
    tagSel.className = "filter-pill-select";
    tagSel.dataset.searchFilter = "tag";
    tagSel.setAttribute("aria-label", "Filter search by tag");
    tagSel.appendChild(Object.assign(document.createElement("option"), { value: "", textContent: "Any tag" }));
    for (const t of state.tags || []) {
      const opt = document.createElement("option");
      opt.value = t.tag;
      opt.textContent = `#${t.tag}`;
      if (filters.tag === t.tag) opt.selected = true;
      tagSel.appendChild(opt);
    }
    tagSel.addEventListener("change", () => {
      container.__searchActiveIdx = 0;
      container.__searchCallbacks?.onFilterChange?.({ tag: tagSel.value });
    });
    filterRow.appendChild(tagSel);
  }

  const fromInput = document.createElement("input");
  fromInput.type = "date";
  fromInput.className = "filter-pill-date";
  fromInput.dataset.searchFilter = "from_date";
  fromInput.title = "From date";
  fromInput.setAttribute("aria-label", "From date");
  fromInput.value = filters.from_date || "";
  fromInput.addEventListener("change", () => {
    container.__searchActiveIdx = 0;
    container.__searchCallbacks?.onFilterChange?.({ from_date: fromInput.value });
  });

  const toInput = document.createElement("input");
  toInput.type = "date";
  toInput.className = "filter-pill-date";
  toInput.dataset.searchFilter = "to_date";
  toInput.title = "To date";
  toInput.setAttribute("aria-label", "To date");
  toInput.value = filters.to_date || "";
  toInput.addEventListener("change", () => {
    container.__searchActiveIdx = 0;
    container.__searchCallbacks?.onFilterChange?.({ to_date: toInput.value });
  });

  const resetBtn = el("button", "link-clear-btn", "Reset");
  resetBtn.type = "button";
  resetBtn.addEventListener("click", () => {
    input.value = "";
    container.__searchActiveIdx = 0;
    container.__searchCallbacks?.onResetSearch?.();
  });

  filterRow.append(fromInput, toInput, resetBtn);
  sheet.appendChild(filterRow);

  const resultsList = el("div", "search-results-list");
  fillResults(resultsList);
  sheet.appendChild(resultsList);

  const close = bindModalEscape(container, backdrop, () =>
    container.__searchCallbacks?.onClose?.()
  );

  sheet.addEventListener("keydown", (ev) => {
    const curHits = container.__searchHits || [];
    if (!curHits.length) return;
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      container.__searchActiveIdx =
        ((container.__searchActiveIdx || 0) + 1) % curHits.length;
      const cards = resultsList.querySelectorAll(".search-hit-card");
      cards.forEach((c, i) => {
        const active = i === container.__searchActiveIdx;
        c.classList.toggle("is-selected", active);
        if (active) c.scrollIntoView({ block: "nearest" });
      });
    } else if (ev.key === "ArrowUp") {
      ev.preventDefault();
      container.__searchActiveIdx =
        ((container.__searchActiveIdx || 0) - 1 + curHits.length) % curHits.length;
      const cards = resultsList.querySelectorAll(".search-hit-card");
      cards.forEach((c, i) => {
        const active = i === container.__searchActiveIdx;
        c.classList.toggle("is-selected", active);
        if (active) c.scrollIntoView({ block: "nearest" });
      });
    } else if (
      ev.key === "Enter" &&
      document.activeElement?.tagName !== "BUTTON"
    ) {
      ev.preventDefault();
      const chosen = curHits[container.__searchActiveIdx || 0];
      const summary = chosen?.summary || chosen;
      if (summary?.id) {
        container.__searchCallbacks?.onSelectEntry?.(summary.id);
        close();
      }
    }
  });

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

  const optionsBar = el("div", "migration-options-bar");
  const nbField = el("label", "migration-target-field");
  nbField.appendChild(el("span", "small-caps-meta", "Target Notebook"));
  const nbSelect = document.createElement("select");
  nbSelect.className = "editorial-select";
  nbSelect.setAttribute("aria-label", "Target notebook for import");
  nbSelect.appendChild(
    Object.assign(document.createElement("option"), {
      value: "",
      textContent: "Default notebook",
    })
  );
  for (const nb of state.notebooks || []) {
    const opt = document.createElement("option");
    opt.value = nb.id;
    opt.textContent = nb.name;
    if (state.selectedNotebookId === nb.id) opt.selected = true;
    nbSelect.appendChild(opt);
  }
  nbField.appendChild(nbSelect);

  const dryLabel = el("label", "dry-run-toggle");
  const dryCheck = document.createElement("input");
  dryCheck.type = "checkbox";
  dryLabel.append(dryCheck, el("span", "", "Dry run (audit without writing files)"));
  optionsBar.append(nbField, dryLabel);

  const cols = el("div", "migration-columns");

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
  dFileLabel.innerHTML = `${icon("upload")}<span>Upload Day One .zip / .json</span>`;
  const dFileInput = document.createElement("input");
  dFileInput.type = "file";
  dFileInput.accept = ".zip,.json";
  dFileInput.className = "sr-only";
  dFileInput.addEventListener("change", () => {
    const file = dFileInput.files?.[0];
    dFileInput.value = "";
    if (file) {
      container.__migrationCallbacks?.onImportDayOneFile?.(
        file,
        dryCheck.checked,
        nbSelect.value || null
      );
    }
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
    if (p) {
      container.__migrationCallbacks?.onImportDayOnePath?.(
        p,
        dryCheck.checked,
        nbSelect.value || null
      );
    }
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
  nFileLabel.innerHTML = `${icon("upload")}<span>Upload Notion .zip / .json</span>`;
  const nFileInput = document.createElement("input");
  nFileInput.type = "file";
  nFileInput.accept = ".zip,.json";
  nFileInput.className = "sr-only";
  nFileInput.addEventListener("change", () => {
    const file = nFileInput.files?.[0];
    nFileInput.value = "";
    if (file) {
      container.__migrationCallbacks?.onImportNotionFile?.(
        file,
        dryCheck.checked,
        nbSelect.value || null
      );
    }
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
    if (p) {
      container.__migrationCallbacks?.onImportNotionPath?.(
        p,
        dryCheck.checked,
        nbSelect.value || null
      );
    }
  });
  nPathRow.append(nInput, nRunBtn);
  nPanel.appendChild(nPathRow);
  cols.appendChild(nPanel);

  body.appendChild(cols);
  body.appendChild(optionsBar);

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

function formatGdriveMethodLabel(method) {
  switch (method) {
    case "oauth":
      return "Google OAuth";
    case "rclone":
      return "rclone";
    case "token":
      return "Access Token";
    case "env_token":
      return "Env Token";
    case "gdrive_cli":
      return "gdrive CLI";
    default:
      return "Configured";
  }
}

function renderGdriveRcloneSlot(slot, state, getCallbacks, getSelectedRemote, setSelectedRemote) {
  slot.innerHTML = "";
  const status = state.gdriveStatus;
  const connecting = Boolean(state.gdriveConnecting);

  if (status?.has_rclone) {
    const remotes = status.rclone_remotes || [];
    if (remotes.length > 0) {
      const row = el("div", "gdrive-inline-row");
      const select = document.createElement("select");
      select.className = "editorial-select";
      select.setAttribute("aria-label", "rclone remote");
      const emptyOpt = document.createElement("option");
      emptyOpt.value = "";
      emptyOpt.textContent = "None (disabled)";
      select.appendChild(emptyOpt);
      for (const r of remotes) {
        const opt = document.createElement("option");
        opt.value = r;
        opt.textContent = r;
        select.appendChild(opt);
      }
      const curRemote = getSelectedRemote();
      if (curRemote && remotes.includes(curRemote)) {
        select.value = curRemote;
      } else if (curRemote === "") {
        select.value = "";
      } else if (state.backupConfig?.rclone_remote && remotes.includes(state.backupConfig.rclone_remote)) {
        select.value = state.backupConfig.rclone_remote;
        setSelectedRemote(select.value);
      } else {
        select.value = remotes[0] || "";
        setSelectedRemote(select.value);
      }
      select.addEventListener("change", () => {
        setSelectedRemote(select.value);
      });

      const signBtn = el("button", "btn-editorial btn-sage small");
      signBtn.type = "button";
      signBtn.disabled = connecting;
      signBtn.textContent = connecting
        ? "Waiting for browser..."
        : "Sign in New Google Account (rclone)";
      signBtn.addEventListener("click", () => {
        getCallbacks()?.onConnectRclone?.("logjournal");
      });
      row.append(select, signBtn);
      slot.appendChild(row);
    } else {
      const btn = el("button", "btn-editorial btn-sage small");
      btn.type = "button";
      btn.disabled = connecting;
      btn.innerHTML = `${icon("upload", "tiny-icon")}<span>${
        connecting
          ? "Waiting for browser sign-in..."
          : "Connect Google Drive in Browser (rclone)"
      }</span>`;
      btn.addEventListener("click", () => {
        getCallbacks()?.onConnectRclone?.("logjournal");
      });
      slot.appendChild(btn);
    }
  } else {
    slot.appendChild(
      el(
        "p",
        "panel-note",
        "Install rclone (sudo pacman -S rclone) then click Refresh for 1-click Google browser sign-in without a Client ID — or connect directly below."
      )
    );
  }
}

function renderGdriveStatusBar(bar, state, isConfigOpen, onToggleConfig, getCallbacks) {
  bar.innerHTML = "";
  const status = state.gdriveStatus;
  const connected = Boolean(status?.connected);

  const info = el("div", "gdrive-status-info");
  const titleRow = el("div", "gdrive-status-title-row");
  const dot = el(
    "span",
    `gdrive-status-dot ${connected ? "is-connected" : "is-disconnected"}`
  );
  titleRow.appendChild(dot);

  if (!status) {
    titleRow.appendChild(el("span", "gdrive-status-label", "Checking Google Drive…"));
  } else if (connected) {
    const methodLabel = formatGdriveMethodLabel(status.method);
    const hint = status.account_hint ? ` · ${status.account_hint}` : "";
    titleRow.appendChild(
      el("span", "gdrive-status-label", `Connected (${methodLabel}${hint})`)
    );
  } else {
    titleRow.appendChild(el("span", "gdrive-status-label", "Google Drive: Not connected"));
  }
  info.appendChild(titleRow);

  if (status?.detail) {
    info.appendChild(el("p", "panel-note", status.detail));
  }

  const actions = el("div", "gdrive-status-actions");
  if (connected && (status.method === "oauth" || status.method === "token")) {
    const discBtn = el("button", "btn-editorial btn-ghost small is-danger", "Disconnect");
    discBtn.type = "button";
    discBtn.disabled = Boolean(state.gdriveConnecting || state.backupLoading);
    discBtn.addEventListener("click", () => {
      getCallbacks()?.onDisconnectGdrive?.();
    });
    actions.appendChild(discBtn);
  }

  const cfgBtn = el(
    "button",
    "btn-editorial small",
    isConfigOpen ? "Hide Settings" : "Configure"
  );
  cfgBtn.type = "button";
  cfgBtn.addEventListener("click", onToggleConfig);
  actions.appendChild(cfgBtn);

  bar.append(info, actions);
}

function renderBackupFeedbackBanner(slot, state) {
  if (!slot) return;
  slot.innerHTML = "";
  if (state.backupError) {
    slot.appendChild(el("p", "form-error", state.backupError));
  }
  if (state.syncResult?.message) {
    const isWarning = state.syncResult.uploaded_remote === false;
    const isSuccess = Boolean(state.syncResult.success) && !isWarning;
    const bannerClass = isWarning
      ? "modal-status-banner is-warning"
      : isSuccess
      ? "modal-status-banner is-success"
      : "modal-status-banner";
    const banner = el("p", bannerClass);
    banner.innerHTML = `${icon(isWarning ? "shield" : "check")}<span>${escapeHtml(
      state.syncResult.message
    )}</span>`;
    slot.appendChild(banner);
  }
}

function renderBackupDynamicRegion(region, state, getPassphrase, getCallbacks) {
  region.innerHTML = "";

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

      if (b.location !== "gdrive" && getCallbacks()?.onSyncBackupById) {
        const upBtn = el("button", "btn-editorial small");
        upBtn.type = "button";
        upBtn.disabled = Boolean(state.backupLoading);
        upBtn.innerHTML = `${icon("upload", "tiny-icon")}<span>Upload to Drive</span>`;
        upBtn.addEventListener("click", () => {
          getCallbacks()?.onSyncBackupById?.(b.id);
        });
        acts.appendChild(upBtn);
      }

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

  const resolveConfigOpen = () => {
    if (typeof container.__backupConfigOpen === "boolean") {
      return container.__backupConfigOpen;
    }
    return Boolean(state.gdriveStatus && !state.gdriveStatus.connected);
  };

  if (existing) {
    const passInput = existing.querySelector('[data-role="backup-passphrase"]');
    const snapBtn = existing.querySelector('[data-role="backup-snap"]');
    const syncBtn = existing.querySelector('[data-role="backup-sync"]');
    const syncLabel = existing.querySelector('[data-role="backup-sync-label"]');
    if (snapBtn) snapBtn.disabled = Boolean(state.backupLoading);
    if (syncBtn) syncBtn.disabled = Boolean(state.backupLoading);
    if (syncLabel) syncLabel.textContent = `Sync to Google Drive (${gdriveFolder})`;

    const configOpen = resolveConfigOpen();
    const statusBar = existing.querySelector('[data-role="gdrive-status-bar"]');
    const configCard = existing.querySelector('[data-role="gdrive-config-card"]');
    if (statusBar) {
      renderGdriveStatusBar(
        statusBar,
        state,
        configOpen,
        () => {
          container.__backupConfigOpen = !resolveConfigOpen();
          renderBackupModal(container, state, container.__backupCallbacks);
        },
        () => container.__backupCallbacks
      );
    }
    if (configCard) {
      configCard.hidden = !configOpen;
      const rcloneSlot = configCard.querySelector('[data-role="gdrive-rclone-slot"]');
      if (rcloneSlot) {
        renderGdriveRcloneSlot(
          rcloneSlot,
          state,
          () => container.__backupCallbacks,
          () => container.__backupSelectedRemote ?? "",
          (v) => {
            container.__backupSelectedRemote = v;
          }
        );
      }

      const clientIdInput = configCard.querySelector('[data-role="gdrive-client-id"]');
      const oauthBtn = configCard.querySelector('[data-role="gdrive-oauth-btn"]');
      const tokenBtn = configCard.querySelector('[data-role="gdrive-token-btn"]');
      const folderInput = configCard.querySelector('[data-role="backup-cfg-folder"]');
      const destInput = configCard.querySelector('[data-role="backup-cfg-dest"]');
      const keepInput = configCard.querySelector('[data-role="backup-cfg-keep"]');
      const savePrefBtn = configCard.querySelector('[data-role="backup-cfg-save"]');

      if (clientIdInput && !clientIdInput.value && state.gdriveStatus?.saved_client_id) {
        clientIdInput.value = state.gdriveStatus.saved_client_id;
      }
      if (oauthBtn) {
        oauthBtn.disabled = Boolean(state.gdriveConnecting);
        oauthBtn.textContent = state.gdriveConnecting
          ? "Signing in..."
          : "Sign in with Browser";
      }
      if (tokenBtn) {
        tokenBtn.disabled = Boolean(state.gdriveConnecting);
      }
      if (savePrefBtn) {
        savePrefBtn.disabled = Boolean(state.backupLoading);
      }
      if (folderInput && document.activeElement !== folderInput) {
        folderInput.value = state.backupConfig?.gdrive_folder || "LogJournal";
      }
      if (destInput && document.activeElement !== destInput) {
        destInput.value = state.backupConfig?.destination_dir || "";
      }
      if (keepInput && document.activeElement !== keepInput) {
        keepInput.value = String(state.backupConfig?.keep_last_n ?? 3);
      }
    }

    const feedbackSlot = existing.querySelector('[data-region="backup-feedback"]');
    renderBackupFeedbackBanner(feedbackSlot, state);

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
  delete container.__backupConfigOpen;
  container.__backupSelectedRemote = state.backupConfig?.rclone_remote ?? null;
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
  syncBtn.title =
    "Leave passphrase empty to upload the latest existing .age snapshot without creating a duplicate archive.";
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
  passBox.appendChild(
    el(
      "p",
      "panel-note",
      "Tip: Leave passphrase empty and click Sync to upload your latest local .age snapshot without creating a duplicate archive."
    )
  );
  body.appendChild(passBox);

  // Google Drive Connection Status Bar & Collapsible Configure Panel
  const gdriveSection = el("section", "gdrive-section");
  const statusBar = el("div", "gdrive-status-bar");
  statusBar.dataset.role = "gdrive-status-bar";

  const configOpen = resolveConfigOpen();
  const configCard = el("div", "gdrive-config-card");
  configCard.dataset.role = "gdrive-config-card";
  configCard.hidden = !configOpen;

  renderGdriveStatusBar(
    statusBar,
    state,
    configOpen,
    () => {
      container.__backupConfigOpen = !resolveConfigOpen();
      const openNow = container.__backupConfigOpen;
      configCard.hidden = !openNow;
      renderGdriveStatusBar(
        statusBar,
        state,
        openNow,
        () => {
          container.__backupConfigOpen = !resolveConfigOpen();
          renderBackupModal(container, state, container.__backupCallbacks);
        },
        () => container.__backupCallbacks
      );
    },
    () => container.__backupCallbacks
  );
  gdriveSection.appendChild(statusBar);

  const feedbackSlot = document.createElement("div");
  feedbackSlot.dataset.region = "backup-feedback";
  feedbackSlot.style.display = "contents";
  renderBackupFeedbackBanner(feedbackSlot, state);
  gdriveSection.appendChild(feedbackSlot);

  // 1. rclone quick browser sign-in
  const rcloneGroup = el("div", "gdrive-config-group");
  rcloneGroup.appendChild(
    el(
      "span",
      "small-caps-meta",
      "1. Quick Browser Sign-In via rclone (Recommended on Linux — no Client ID needed)"
    )
  );
  const rcloneSlot = el("div", "gdrive-rclone-slot");
  rcloneSlot.dataset.role = "gdrive-rclone-slot";
  renderGdriveRcloneSlot(
    rcloneSlot,
    state,
    () => container.__backupCallbacks,
    () => container.__backupSelectedRemote ?? "",
    (v) => {
      container.__backupSelectedRemote = v;
    }
  );
  rcloneGroup.appendChild(rcloneSlot);
  configCard.appendChild(rcloneGroup);

  // 2. Direct Google OAuth 2.0 Browser Sign-In (Built-in PKCE)
  const oauthGroup = el("div", "gdrive-config-group");
  oauthGroup.appendChild(
    el(
      "span",
      "small-caps-meta",
      "2. Direct Google OAuth 2.0 Browser Sign-In (Built-in PKCE, no external CLI)"
    )
  );
  const oauthInputs = el("div", "gdrive-inline-row");
  const clientIdInput = document.createElement("input");
  clientIdInput.type = "text";
  clientIdInput.className = "editorial-input";
  clientIdInput.dataset.role = "gdrive-client-id";
  clientIdInput.placeholder = "Google OAuth Client ID (Desktop app)";
  clientIdInput.value = state.gdriveStatus?.saved_client_id || "";

  const clientSecretInput = document.createElement("input");
  clientSecretInput.type = "password";
  clientSecretInput.className = "editorial-input";
  clientSecretInput.placeholder = "Client Secret (required for Desktop OAuth)";

  const oauthBtn = el(
    "button",
    "btn-editorial small",
    state.gdriveConnecting ? "Signing in..." : "Sign in with Browser"
  );
  oauthBtn.type = "button";
  oauthBtn.dataset.role = "gdrive-oauth-btn";
  oauthBtn.disabled = Boolean(state.gdriveConnecting);
  oauthBtn.addEventListener("click", () => {
    container.__backupCallbacks?.onConnectOauth?.({
      client_id: clientIdInput.value.trim(),
      client_secret: clientSecretInput.value.trim() || null,
    });
  });
  oauthInputs.append(clientIdInput, clientSecretInput, oauthBtn);
  oauthGroup.appendChild(oauthInputs);
  oauthGroup.appendChild(
    el(
      "p",
      "panel-note",
      "Google Cloud Desktop app OAuth clients require both Client ID and Client Secret (loopback redirect on 127.0.0.1 is automatic)."
    )
  );
  configCard.appendChild(oauthGroup);

  // 3. Direct OAuth Access Token
  const tokenGroup = el("div", "gdrive-config-group");
  tokenGroup.appendChild(el("span", "small-caps-meta", "3. Direct OAuth Access Token"));
  const tokenRow = el("div", "gdrive-inline-row");
  const tokenInput = document.createElement("input");
  tokenInput.type = "password";
  tokenInput.className = "editorial-input";
  tokenInput.placeholder = "Paste Google Drive OAuth access token…";
  const tokenBtn = el("button", "btn-editorial small", "Save Token");
  tokenBtn.type = "button";
  tokenBtn.dataset.role = "gdrive-token-btn";
  tokenBtn.disabled = Boolean(state.gdriveConnecting);
  tokenBtn.addEventListener("click", () => {
    const t = tokenInput.value.trim();
    if (t) container.__backupCallbacks?.onSaveToken?.(t);
  });
  tokenRow.append(tokenInput, tokenBtn);
  tokenGroup.appendChild(tokenRow);
  configCard.appendChild(tokenGroup);

  // 4. Backup Preferences
  const prefGroup = el("div", "gdrive-config-group");
  prefGroup.appendChild(el("span", "small-caps-meta", "4. Backup Preferences"));
  const prefGrid = el("div", "backup-pref-grid");

  const folderField = el("label", "backup-pref-field");
  folderField.appendChild(el("span", "backup-pref-label", "Google Drive Folder"));
  const folderInput = document.createElement("input");
  folderInput.type = "text";
  folderInput.className = "editorial-input";
  folderInput.dataset.role = "backup-cfg-folder";
  folderInput.value = state.backupConfig?.gdrive_folder || "LogJournal";
  folderField.appendChild(folderInput);

  const destField = el("label", "backup-pref-field");
  destField.appendChild(
    el("span", "backup-pref-label", "Local / Cloud Mirror Directory")
  );
  const destInput = document.createElement("input");
  destInput.type = "text";
  destInput.className = "editorial-input";
  destInput.dataset.role = "backup-cfg-dest";
  destInput.placeholder = "~/GoogleDrive/LogJournal (optional)";
  destInput.value = state.backupConfig?.destination_dir || "";
  destField.appendChild(destInput);

  const keepField = el("label", "backup-pref-field is-narrow");
  keepField.appendChild(
    el("span", "backup-pref-label", "Keep Last N Local Snapshots")
  );
  const keepInput = document.createElement("input");
  keepInput.type = "number";
  keepInput.min = "1";
  keepInput.max = "50";
  keepInput.className = "editorial-input";
  keepInput.dataset.role = "backup-cfg-keep";
  keepInput.value = String(state.backupConfig?.keep_last_n ?? 3);
  keepField.appendChild(keepInput);

  prefGrid.append(folderField, destField, keepField);
  prefGroup.appendChild(prefGrid);

  const prefActions = el("div", "backup-pref-actions");
  const savePrefBtn = el("button", "btn-editorial btn-sage small", "Save Preferences");
  savePrefBtn.type = "button";
  savePrefBtn.dataset.role = "backup-cfg-save";
  savePrefBtn.disabled = Boolean(state.backupLoading);
  savePrefBtn.addEventListener("click", () => {
    const parsedKeep = Math.min(
      Math.max(Number.parseInt(keepInput.value || "3", 10) || 3, 1),
      50
    );
    const remoteVal =
      typeof container.__backupSelectedRemote === "string"
        ? container.__backupSelectedRemote.trim() || null
        : state.backupConfig?.rclone_remote ?? null;
    container.__backupCallbacks?.onSaveBackupConfig?.({
      destination_dir: destInput.value.trim() || null,
      gdrive_folder: folderInput.value.trim() || "LogJournal",
      auto_sync_enabled: Boolean(state.backupConfig?.auto_sync_enabled),
      keep_last_n: parsedKeep,
      has_passphrase_hint: state.backupConfig?.has_passphrase_hint ?? null,
      rclone_remote: remoteVal,
    });
  });
  prefActions.appendChild(savePrefBtn);
  prefGroup.appendChild(prefActions);
  configCard.appendChild(prefGroup);

  gdriveSection.appendChild(configCard);
  body.appendChild(gdriveSection);

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

/**
 * Render the Journal Insights, Writing Streaks, and Local Vault Maintenance modal.
 * @param {HTMLElement} container
 * @param {object} state
 * @param {object} callbacks
 */
export function renderInsightsModal(container, state, callbacks = {}) {
  container.innerHTML = "";

  const stats = state.stats || {};
  const entries = state.entries || [];

  const backdrop = el("div", "editorial-modal-backdrop");
  backdrop.dataset.logModal = "insights";
  const sheet = el("div", "editorial-modal-sheet wide");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Journal Insights & Vault");

  const header = el("header", "modal-header");
  const titleGroup = el("div", "modal-title-group");
  const titleIcon = el("span", "modal-title-icon");
  titleIcon.innerHTML = icon("chart");
  titleGroup.appendChild(titleIcon);

  const titleText = el("div");
  titleText.appendChild(el("span", "small-caps-meta", "Archive Ledger"));
  titleText.appendChild(el("h2", "modal-serif-heading", "Journal Insights & Vault"));
  titleGroup.appendChild(titleText);
  header.appendChild(titleGroup);

  const closeBtn = el("button", "icon-btn");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close insights");
  closeBtn.innerHTML = icon("close");
  header.appendChild(closeBtn);
  sheet.appendChild(header);

  const body = el("div", "modal-body");

  // KPI summary row
  const kpiGrid = el("div", "insights-kpi-grid");
  const kpis = [
    ["Current Streak", `${stats.current_streak_days ?? 0}d`, "is-sage"],
    ["Longest Streak", `${stats.longest_streak_days ?? 0}d`, ""],
    ["This Month", String(stats.entries_this_month ?? 0), ""],
    ["Total Entries", String(stats.total_entries ?? entries.length), ""],
    ["Total Words", Number(stats.total_words ?? 0).toLocaleString(), ""],
    [
      "Media",
      `${stats.total_photos ?? 0} photos · ${stats.total_audio_memos ?? 0} voice`,
      "",
    ],
  ];
  for (const [label, num, mod] of kpis) {
    const card = el("div", `report-kpi ${mod}`.trim());
    card.appendChild(el("span", "kpi-num", num));
    card.appendChild(el("span", "small-caps-meta", label));
    kpiGrid.appendChild(card);
  }
  body.appendChild(kpiGrid);

  // Mood distribution & Top Tags side by side
  const breakdownCols = el("div", "insights-breakdown-cols");

  // Moods
  const moodCounts = new Map();
  for (const e of entries) {
    if (e.mood) {
      moodCounts.set(e.mood, (moodCounts.get(e.mood) || 0) + 1);
    }
  }
  const sortedMoods = [...moodCounts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 6);
  const moodBox = el("section", "migration-panel");
  moodBox.appendChild(el("h3", "panel-serif-title", "Mood Cadence"));
  if (sortedMoods.length === 0) {
    moodBox.appendChild(el("p", "panel-note", "Add moods to entries to track emotional cadence over time."));
  } else {
    const maxM = sortedMoods[0][1] || 1;
    const bars = el("div", "insights-bars-list");
    for (const [mood, cnt] of sortedMoods) {
      const row = el("div", "insights-bar-row");
      const meta = el("div", "insights-bar-meta");
      meta.append(el("span", "", mood), el("span", "small-caps-meta", String(cnt)));
      const track = el("div", "insights-bar-track");
      const fill = el("div", "insights-bar-fill");
      fill.style.width = `${Math.max(8, Math.round((cnt / maxM) * 100))}%`;
      track.appendChild(fill);
      row.append(meta, track);
      bars.appendChild(row);
    }
    moodBox.appendChild(bars);
  }
  breakdownCols.appendChild(moodBox);

  // Top Tags
  const topTags = (stats.top_tags || state.tags || []).slice(0, 6);
  const tagBox = el("section", "migration-panel");
  tagBox.appendChild(el("h3", "panel-serif-title", "Recurring Themes"));
  if (topTags.length === 0) {
    tagBox.appendChild(el("p", "panel-note", "Tag your entries to surface recurring themes here."));
  } else {
    const maxT = topTags[0].count || 1;
    const bars = el("div", "insights-bars-list");
    for (const item of topTags) {
      const row = el("button", "insights-bar-row is-clickable");
      row.type = "button";
      const meta = el("div", "insights-bar-meta");
      meta.append(el("span", "", `#${item.tag}`), el("span", "small-caps-meta", String(item.count)));
      const track = el("div", "insights-bar-track");
      const fill = el("div", "insights-bar-fill is-terracotta");
      fill.style.width = `${Math.max(8, Math.round((item.count / maxT) * 100))}%`;
      track.appendChild(fill);
      row.append(meta, track);
      row.addEventListener("click", () => {
        callbacks.onSelectTag?.(item.tag);
        callbacks.onClose?.();
      });
      bars.appendChild(row);
    }
    tagBox.appendChild(bars);
  }
  breakdownCols.appendChild(tagBox);
  body.appendChild(breakdownCols);

  // Local Vault Maintenance section
  const vaultPanel = el("section", "migration-panel");
  vaultPanel.appendChild(el("h3", "panel-serif-title", "Local Markdown Vault"));
  vaultPanel.appendChild(
    el("p", "mono-cell muted-cell", stats.vault_path || "~/Personal/JournalVault")
  );
  vaultPanel.appendChild(
    el(
      "p",
      "panel-note",
      "Every entry is stored as plain UTF-8 Markdown with YAML frontmatter. If you edit files externally, refresh or rebuild the SQLite FTS5 search index below."
    )
  );

  const vaultActions = el("div", "backup-action-row");
  const openBtn = el("button", "btn-editorial small");
  openBtn.type = "button";
  openBtn.innerHTML = `${icon("folder", "tiny-icon")}<span>Open Vault Folder</span>`;
  openBtn.addEventListener("click", () => callbacks.onOpenVaultFolder?.());

  const syncBtn = el("button", "btn-editorial btn-sage small");
  syncBtn.type = "button";
  syncBtn.disabled = Boolean(state.vaultSyncing);
  syncBtn.innerHTML = `${icon("refresh", "tiny-icon")}<span>${
    state.vaultSyncing ? "Syncing…" : "Sync External Edits"
  }</span>`;
  syncBtn.addEventListener("click", () => callbacks.onSyncVault?.(false));

  const rebuildBtn = el("button", "btn-editorial btn-ghost small");
  rebuildBtn.type = "button";
  rebuildBtn.disabled = Boolean(state.vaultSyncing);
  rebuildBtn.innerHTML = `${icon("sliders", "tiny-icon")}<span>Rebuild Search Index</span>`;
  rebuildBtn.addEventListener("click", () => callbacks.onSyncVault?.(true));

  vaultActions.append(openBtn, syncBtn, rebuildBtn);
  vaultPanel.appendChild(vaultActions);
  body.appendChild(vaultPanel);

  sheet.appendChild(body);
  backdrop.appendChild(sheet);

  const close = bindModalEscape(container, backdrop, callbacks.onClose);
  closeBtn.addEventListener("click", close);
  backdrop.addEventListener("click", (ev) => {
    if (ev.target === backdrop) close();
  });

  container.appendChild(backdrop);
}

/**
 * Render the Keyboard Shortcuts reference modal.
 * @param {HTMLElement} container
 * @param {object} callbacks
 */
export function renderShortcutsModal(container, callbacks = {}) {
  container.innerHTML = "";

  const backdrop = el("div", "editorial-modal-backdrop");
  backdrop.dataset.logModal = "shortcuts";
  const sheet = el("div", "editorial-modal-sheet");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Keyboard Shortcuts");

  const header = el("header", "modal-header");
  const titleGroup = el("div", "modal-title-group");
  const titleIcon = el("span", "modal-title-icon");
  titleIcon.innerHTML = icon("keyboard");
  titleGroup.appendChild(titleIcon);

  const titleText = el("div");
  titleText.appendChild(el("span", "small-caps-meta", "Reference"));
  titleText.appendChild(el("h2", "modal-serif-heading", "Keyboard Shortcuts"));
  titleGroup.appendChild(titleText);
  header.appendChild(titleGroup);

  const closeBtn = el("button", "icon-btn");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close keyboard shortcuts");
  closeBtn.innerHTML = icon("close");
  header.appendChild(closeBtn);
  sheet.appendChild(header);

  const body = el("div", "modal-body");
  const groups = [
    {
      title: "Navigation & Workspace",
      items: [
        ["Ctrl + K", "Search & filter entries"],
        ["Ctrl + F", "Find in current entry"],
        ["Ctrl + N", "Start a new entry"],
        ["Ctrl + S", "Save current entry immediately"],
        ["Ctrl + E", "Toggle Write / Read mode"],
        ["Ctrl + Shift + F", "Toggle Zen Focus mode"],
        ["Ctrl + \\", "Toggle sidebar"],
        ["Alt + Left / Right", "Previous / Next entry"],
        ["Ctrl + 1 / 2 / 3 / 4", "Editor / All Entries / Calendar / On This Day"],
        ["Ctrl + /", "Show keyboard shortcuts"],
      ],
    },
    {
      title: "Writing & Markdown",
      items: [
        ["/", "Open inline slash command palette"],
        ["Ctrl + B", "Bold selection"],
        ["Ctrl + I", "Italic selection"],
        ["Ctrl + Shift + X", "Strikethrough selection"],
        ["Ctrl + Shift + C", "Inline code selection"],
        ["Ctrl + Shift + L", "Insert Markdown link"],
        ["Ctrl + Shift + T", "Toggle checklist item on line"],
        ["Tab / Shift + Tab", "Indent / outdent lines"],
        ["Ctrl + V (image)", "Paste image from clipboard as Polaroid"],
      ],
    },
  ];

  for (const grp of groups) {
    const sec = el("section", "shortcuts-group");
    sec.appendChild(el("span", "small-caps-meta", grp.title));
    const list = el("div", "shortcuts-list");
    for (const [keys, desc] of grp.items) {
      const row = el("div", "shortcut-row");
      row.appendChild(el("span", "shortcut-desc", desc));
      row.appendChild(el("kbd", "shortcut-kbd", keys));
      list.appendChild(row);
    }
    sec.appendChild(list);
    body.appendChild(sec);
  }

  sheet.appendChild(body);
  backdrop.appendChild(sheet);

  const close = bindModalEscape(container, backdrop, callbacks.onClose);
  closeBtn.addEventListener("click", close);
  backdrop.addEventListener("click", (ev) => {
    if (ev.target === backdrop) close();
  });

  container.appendChild(backdrop);
}
