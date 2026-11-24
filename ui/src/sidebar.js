import { icon } from "./icons.js";
import { resolveVaultUrl } from "./ipc.js";
import { parseCoverAsset } from "./cover-fx.js";

const ACCENT_PALETTE = [
  "#546E5A",
  "#B5563E",
  "#6D6157",
  "#4A6670",
  "#8C6B4A",
];

let addNotebookOpen = false;
let draftNbName = "";
let draftNbCover = null;
let draftNbAccent = ACCENT_PALETTE[0];
let draftNbBusy = false;

let editingNotebookId = null;
let editNbName = "";
let editNbCover = "";
let editNbAccent = ACCENT_PALETTE[0];
let editNbBusy = false;

let deletingNotebookId = null;
let deleteNbBusy = false;

function iconEl(name, cls = "log-icon") {
  const wrap = document.createElement("span");
  wrap.style.display = "inline-flex";
  wrap.style.alignItems = "center";
  wrap.innerHTML = icon(name, cls);
  return wrap.firstElementChild || wrap;
}

function buildNotebookForm({
  state,
  callbacks,
  initialName,
  initialCover,
  initialAccent,
  submitLabel,
  busyLabel,
  isBusy,
  onNameChange,
  onCoverChange,
  onAccentChange,
  onSubmit,
  onCancel,
}) {
  let currentName = initialName || "";
  let currentCover = initialCover || "";
  let currentAccent = initialAccent || ACCENT_PALETTE[0];
  let uploading = false;

  const form = document.createElement("form");
  form.className = "inline-notebook-form";

  const nameInput = document.createElement("input");
  nameInput.type = "text";
  nameInput.className = "editorial-input";
  nameInput.placeholder = "Notebook name";
  nameInput.value = currentName;
  nameInput.required = true;
  nameInput.addEventListener("input", () => {
    currentName = nameInput.value;
    onNameChange?.(currentName);
  });
  form.appendChild(nameInput);

  const coverRow = document.createElement("div");
  coverRow.className = "cover-swatch-row";
  const builtInCovers = state.manifest?.covers || [];

  function renderCoverSwatches() {
    coverRow.innerHTML = "";
    for (const cov of builtInCovers) {
      const isAnimated =
        Array.isArray(cov.tags) && cov.tags.includes("animated");
      const sw = document.createElement("button");
      sw.type = "button";
      sw.className = `cover-swatch-btn${
        currentCover === cov.path ? " selected" : ""
      }${isAnimated ? " is-animated" : ""}`;
      sw.title = isAnimated
        ? `${cov.label || "Animated cover"} (Animated)`
        : cov.label || "Cover wash";
      const img = document.createElement("img");
      img.src = resolveVaultUrl(parseCoverAsset(cov.path).basePath || cov.path);
      img.alt = cov.label || "Cover";
      img.decoding = "async";
      sw.appendChild(img);
      if (isAnimated) {
        const dot = document.createElement("span");
        dot.className = "cover-live-dot";
        dot.setAttribute("aria-hidden", "true");
        sw.appendChild(dot);
      }
      sw.addEventListener("click", () => {
        currentCover = cov.path;
        onCoverChange?.(currentCover);
        renderCoverSwatches();
      });
      coverRow.appendChild(sw);
    }

    const isCustomCover =
      Boolean(currentCover) &&
      !builtInCovers.some((cov) => cov.path === currentCover);
    if (isCustomCover) {
      const customSw = document.createElement("button");
      customSw.type = "button";
      customSw.className = "cover-swatch-btn selected";
      customSw.title = "Custom cover picture";
      const img = document.createElement("img");
      img.src = resolveVaultUrl(currentCover);
      img.alt = "Custom notebook cover";
      img.decoding = "async";
      customSw.appendChild(img);
      customSw.addEventListener("click", () => {
        onCoverChange?.(currentCover);
        renderCoverSwatches();
      });
      coverRow.appendChild(customSw);
    }

    const noneSw = document.createElement("button");
    noneSw.type = "button";
    noneSw.className = `cover-swatch-btn is-none${
      !currentCover ? " selected" : ""
    }`;
    noneSw.title = "No cover";
    noneSw.setAttribute("aria-label", "No cover");
    noneSw.appendChild(iconEl("close", "tiny-icon"));
    noneSw.addEventListener("click", () => {
      currentCover = "";
      onCoverChange?.("");
      renderCoverSwatches();
    });
    coverRow.appendChild(noneSw);
  }
  renderCoverSwatches();
  form.appendChild(coverRow);

  const errorEl = document.createElement("p");
  errorEl.className = "form-error";
  errorEl.hidden = true;

  if (callbacks.onUploadNotebookCover) {
    const uploadWrap = document.createElement("label");
    uploadWrap.className = "btn-editorial btn-ghost small cover-upload-btn";
    const uploadIcon = iconEl("image", "tiny-icon");
    const uploadSpan = document.createElement("span");
    uploadSpan.textContent = "Upload picture";
    const fileInput = document.createElement("input");
    fileInput.type = "file";
    fileInput.accept = "image/*";
    fileInput.className = "sr-only";
    fileInput.addEventListener("change", async () => {
      const file = fileInput.files?.[0];
      fileInput.value = "";
      if (!file || uploading) return;
      uploading = true;
      errorEl.hidden = true;
      uploadWrap.classList.add("is-busy");
      uploadSpan.textContent = "Uploading...";
      try {
        const uploadedPath = await callbacks.onUploadNotebookCover(file);
        if (uploadedPath) {
          currentCover = uploadedPath;
          onCoverChange?.(currentCover);
          renderCoverSwatches();
        }
      } catch (err) {
        errorEl.textContent = String(err?.message || err || "Upload failed");
        errorEl.hidden = false;
      } finally {
        uploading = false;
        uploadWrap.classList.remove("is-busy");
        uploadSpan.textContent = "Upload picture";
      }
    });
    uploadWrap.append(uploadIcon, uploadSpan, fileInput);
    form.appendChild(uploadWrap);
  }

  const accentRow = document.createElement("div");
  accentRow.className = "accent-swatch-row";
  for (const color of ACCENT_PALETTE) {
    const dot = document.createElement("button");
    dot.type = "button";
    dot.className = `accent-dot-btn${
      currentAccent === color ? " selected" : ""
    }`;
    dot.style.background = color;
    dot.title = color;
    dot.addEventListener("click", () => {
      currentAccent = color;
      onAccentChange?.(color);
      accentRow
        .querySelectorAll(".accent-dot-btn")
        .forEach((b) => b.classList.remove("selected"));
      dot.classList.add("selected");
    });
    accentRow.appendChild(dot);
  }
  form.appendChild(accentRow);
  form.appendChild(errorEl);

  const submitRow = document.createElement("div");
  submitRow.className = "inline-notebook-actions";

  const saveBtn = document.createElement("button");
  saveBtn.type = "submit";
  saveBtn.className = "btn-editorial btn-sage small";
  saveBtn.textContent = isBusy ? busyLabel : submitLabel;
  saveBtn.disabled = Boolean(isBusy);

  const cancelBtn = document.createElement("button");
  cancelBtn.type = "button";
  cancelBtn.className = "btn-editorial btn-ghost small";
  cancelBtn.textContent = "Cancel";
  cancelBtn.addEventListener("click", () => onCancel?.());

  submitRow.append(saveBtn, cancelBtn);
  form.appendChild(submitRow);

  form.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const trimmed = currentName.trim();
    if (!trimmed || uploading) return;
    errorEl.hidden = true;
    saveBtn.disabled = true;
    saveBtn.textContent = busyLabel;
    try {
      await onSubmit?.({
        name: trimmed,
        cover_asset: currentCover || "",
        accent_color: currentAccent,
      });
    } catch (err) {
      errorEl.textContent = String(err?.message || err || "Action failed");
      errorEl.hidden = false;
    } finally {
      if (saveBtn.isConnected) {
        saveBtn.disabled = false;
        saveBtn.textContent = submitLabel;
      }
    }
  });

  return form;
}

export function renderSidebar(container, state, callbacks = {}) {
  const prevScroll =
    container.querySelector(".sidebar-scroll-region")?.scrollTop || 0;

  container.className = "sanctuary-sidebar";
  container.innerHTML = "";

  // Header
  const header = document.createElement("div");
  header.className = "sidebar-header";

  const wordmark = document.createElement("span");
  wordmark.className = "sidebar-wordmark";
  wordmark.textContent = "Log";

  const collapseBtn = document.createElement("button");
  collapseBtn.type = "button";
  collapseBtn.className = "sidebar-icon-btn";
  collapseBtn.title = "Collapse sidebar (Ctrl+\\)";
  collapseBtn.setAttribute("aria-label", "Collapse sidebar");
  collapseBtn.appendChild(iconEl("sidebar", "log-icon"));
  collapseBtn.addEventListener("click", () => callbacks.onToggleSidebar?.());

  header.append(wordmark, collapseBtn);
  container.appendChild(header);

  // Primary Actions
  const actions = document.createElement("div");
  actions.className = "sidebar-actions";

  const newBtn = document.createElement("button");
  newBtn.type = "button";
  newBtn.className = "sidebar-new-btn";
  newBtn.appendChild(iconEl("plus", "tiny-icon"));
  const newLabel = document.createElement("span");
  newLabel.textContent = "New Entry";
  newBtn.appendChild(newLabel);
  newBtn.addEventListener("click", () => callbacks.onNewEntry?.());

  const searchBtn = document.createElement("button");
  searchBtn.type = "button";
  searchBtn.className = "sidebar-search-btn";
  searchBtn.appendChild(iconEl("search", "tiny-icon"));
  const searchTxt = document.createElement("span");
  searchTxt.textContent = "Search";
  const kbd = document.createElement("kbd");
  kbd.textContent = "Ctrl K";
  searchBtn.append(searchTxt, kbd);
  searchBtn.addEventListener("click", () => callbacks.onOpenSearch?.());

  actions.append(newBtn, searchBtn);
  container.appendChild(actions);

  // Navigation
  const totalEntries =
    state.stats?.total_entries ?? state.entries?.length ?? 0;
  const starredCount = (state.entries || []).filter((e) => e.starred).length;
  const otdCount =
    state.onThisDayCount ??
    (state.onThisDayGroups || []).reduce(
      (acc, g) => acc + (g.entries?.length || 0),
      0
    );

  const navItems = [
    { key: "editor", label: "Editor", iconName: "feather", count: null },
    {
      key: "timeline",
      label: "All Entries",
      iconName: "book",
      count: totalEntries,
    },
    {
      key: "starred",
      label: "Starred",
      iconName: "star",
      count: starredCount > 0 ? starredCount : null,
    },
    { key: "calendar", label: "Calendar", iconName: "calendar", count: null },
    {
      key: "on_this_day",
      label: "On This Day",
      iconName: "clock",
      count: otdCount > 0 ? otdCount : null,
    },
  ];

  const nav = document.createElement("nav");
  nav.className = "sidebar-nav";
  nav.setAttribute("aria-label", "Journal views");

  for (const item of navItems) {
    const isNavActive =
      item.key === "timeline"
        ? state.activeView === "timeline" &&
          !state.selectedNotebookId &&
          !state.selectedTag &&
          !state.starredOnly
        : item.key === "starred"
          ? state.activeView === "timeline" &&
            Boolean(state.starredOnly) &&
            !state.selectedNotebookId &&
            !state.selectedTag
          : state.activeView === item.key;
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `sidebar-nav-item${isNavActive ? " is-active" : ""}`;
    btn.appendChild(iconEl(item.iconName, "log-icon"));
    const lbl = document.createElement("span");
    lbl.className = "sidebar-nav-label";
    lbl.textContent = item.label;
    btn.appendChild(lbl);
    if (item.count !== null) {
      const cnt = document.createElement("span");
      cnt.className = "sidebar-nav-count";
      cnt.textContent = String(item.count);
      btn.appendChild(cnt);
    }
    btn.addEventListener("click", () => callbacks.onNavigate?.(item.key));
    nav.appendChild(btn);
  }
  container.appendChild(nav);

  // Scroll region: Notebooks + Tags
  const scroll = document.createElement("div");
  scroll.className = "sidebar-scroll-region";

  // Notebooks section
  const nbSection = document.createElement("section");
  nbSection.className = "sidebar-section";

  const nbHead = document.createElement("div");
  nbHead.className = "sidebar-section-header";
  const nbTitle = document.createElement("span");
  nbTitle.className = "small-caps-meta";
  nbTitle.textContent = "Notebooks";

  const addNbBtn = document.createElement("button");
  addNbBtn.type = "button";
  addNbBtn.className = "sidebar-icon-btn";
  addNbBtn.title = addNotebookOpen ? "Cancel" : "New notebook";
  addNbBtn.setAttribute("aria-label", "New notebook");
  addNbBtn.appendChild(iconEl(addNotebookOpen ? "close" : "plus", "tiny-icon"));
  addNbBtn.addEventListener("click", () => {
    addNotebookOpen = !addNotebookOpen;
    if (addNotebookOpen) {
      editingNotebookId = null;
      deletingNotebookId = null;
      if (!draftNbCover) {
        draftNbCover = state.manifest?.covers?.[0]?.path || null;
      }
    }
    renderSidebar(container, state, callbacks);
  });

  nbHead.append(nbTitle, addNbBtn);
  nbSection.appendChild(nbHead);

  if (addNotebookOpen) {
    const form = buildNotebookForm({
      state,
      callbacks,
      initialName: draftNbName,
      initialCover: draftNbCover || "",
      initialAccent: draftNbAccent,
      submitLabel: "Create",
      busyLabel: "Creating...",
      isBusy: draftNbBusy,
      onNameChange: (val) => {
        draftNbName = val;
      },
      onCoverChange: (val) => {
        draftNbCover = val;
      },
      onAccentChange: (val) => {
        draftNbAccent = val;
      },
      onSubmit: async ({ name, cover_asset, accent_color }) => {
        if (draftNbBusy) return;
        draftNbBusy = true;
        try {
          await callbacks.onCreateNotebook?.({
            name,
            description: null,
            cover_asset: cover_asset || null,
            accent_color,
          });
          addNotebookOpen = false;
          draftNbName = "";
        } finally {
          draftNbBusy = false;
        }
      },
      onCancel: () => {
        addNotebookOpen = false;
        draftNbName = "";
        renderSidebar(container, state, callbacks);
      },
    });
    nbSection.appendChild(form);
  }

  const nbList = document.createElement("div");
  nbList.className = "notebook-shelf-list";

  const isTimelineView = state.activeView === "timeline";
  const allNbBtn = document.createElement("button");
  allNbBtn.type = "button";
  allNbBtn.className = `sidebar-notebook-item${
    isTimelineView && !state.selectedNotebookId && !state.selectedTag && !state.starredOnly
      ? " is-active"
      : ""
  }`;
  const allDot = document.createElement("span");
  allDot.className = "notebook-color-dot is-all";
  const allName = document.createElement("span");
  allName.className = "notebook-name";
  allName.textContent = "All Notebooks";
  const allCount = document.createElement("span");
  allCount.className = "notebook-count";
  allCount.textContent = String(totalEntries);
  allNbBtn.append(allDot, allName, allCount);
  allNbBtn.addEventListener("click", () => callbacks.onSelectNotebook?.(null));
  nbList.appendChild(allNbBtn);

  for (const nb of state.notebooks || []) {
    const isNbActive = isTimelineView && state.selectedNotebookId === nb.id;
    const row = document.createElement("div");
    row.className = `sidebar-notebook-row${isNbActive ? " is-active" : ""}`;

    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `sidebar-notebook-item${isNbActive ? " is-active" : ""}`;

    if (nb.cover_asset) {
      const thumb = document.createElement("span");
      thumb.className = "notebook-cover-thumb";
      thumb.style.borderColor = nb.accent_color || "var(--border-strong)";
      const thumbImg = document.createElement("img");
      thumbImg.src = resolveVaultUrl(
        parseCoverAsset(nb.cover_asset).basePath || nb.cover_asset
      );
      thumbImg.alt = "";
      thumbImg.loading = "lazy";
      thumbImg.decoding = "async";
      thumb.appendChild(thumbImg);
      btn.appendChild(thumb);
    } else {
      const dot = document.createElement("span");
      dot.className = "notebook-color-dot";
      dot.style.background = nb.accent_color || "var(--accent-primary)";
      btn.appendChild(dot);
    }

    const name = document.createElement("span");
    name.className = "notebook-name";
    name.textContent = nb.name;

    const count = document.createElement("span");
    count.className = "notebook-count";
    count.textContent = String(nb.entry_count ?? 0);

    btn.append(name, count);
    btn.addEventListener("click", () => callbacks.onSelectNotebook?.(nb.id));
    row.appendChild(btn);

    if (callbacks.onUpdateNotebook || callbacks.onDeleteNotebook) {
      const rowActions = document.createElement("div");
      rowActions.className = "sidebar-notebook-actions";

      if (callbacks.onUpdateNotebook) {
        const editBtn = document.createElement("button");
        editBtn.type = "button";
        editBtn.className = "sidebar-row-icon-btn";
        editBtn.title = "Edit notebook";
        editBtn.setAttribute("aria-label", `Edit ${nb.name}`);
        editBtn.appendChild(iconEl("edit", "tiny-icon"));
        editBtn.addEventListener("click", (ev) => {
          ev.stopPropagation();
          deletingNotebookId = null;
          if (editingNotebookId === nb.id) {
            editingNotebookId = null;
          } else {
            addNotebookOpen = false;
            editingNotebookId = nb.id;
            editNbName = nb.name || "";
            editNbCover = nb.cover_asset || "";
            editNbAccent = nb.accent_color || ACCENT_PALETTE[0];
          }
          renderSidebar(container, state, callbacks);
        });
        rowActions.appendChild(editBtn);
      }

      if (callbacks.onDeleteNotebook) {
        const delBtn = document.createElement("button");
        delBtn.type = "button";
        delBtn.className = "sidebar-row-icon-btn is-danger";
        delBtn.title = "Delete notebook";
        delBtn.setAttribute("aria-label", `Delete ${nb.name}`);
        delBtn.appendChild(iconEl("trash", "tiny-icon"));
        delBtn.addEventListener("click", (ev) => {
          ev.stopPropagation();
          editingNotebookId = null;
          if (deletingNotebookId === nb.id) {
            deletingNotebookId = null;
          } else {
            addNotebookOpen = false;
            deletingNotebookId = nb.id;
          }
          renderSidebar(container, state, callbacks);
        });
        rowActions.appendChild(delBtn);
      }

      row.appendChild(rowActions);
    }

    nbList.appendChild(row);

    if (editingNotebookId === nb.id) {
      const editForm = buildNotebookForm({
        state,
        callbacks,
        initialName: editNbName,
        initialCover: editNbCover,
        initialAccent: editNbAccent,
        submitLabel: "Save",
        busyLabel: "Saving...",
        isBusy: editNbBusy,
        onNameChange: (val) => {
          editNbName = val;
        },
        onCoverChange: (val) => {
          editNbCover = val;
        },
        onAccentChange: (val) => {
          editNbAccent = val;
        },
        onSubmit: async ({ name, cover_asset, accent_color }) => {
          if (editNbBusy) return;
          editNbBusy = true;
          try {
            await callbacks.onUpdateNotebook?.({
              id: nb.id,
              name,
              description: nb.description ?? null,
              cover_asset: cover_asset ?? "",
              accent_color,
            });
            editingNotebookId = null;
          } finally {
            editNbBusy = false;
          }
        },
        onCancel: () => {
          editingNotebookId = null;
          renderSidebar(container, state, callbacks);
        },
      });
      nbList.appendChild(editForm);
    }

    if (deletingNotebookId === nb.id) {
      const delPanel = document.createElement("div");
      delPanel.className = "inline-notebook-delete";

      const prompt = document.createElement("p");
      prompt.className = "inline-notebook-delete-prompt";
      prompt.textContent = `Delete “${nb.name}”?`;
      delPanel.appendChild(prompt);

      const errEl = document.createElement("p");
      errEl.className = "form-error";
      errEl.hidden = true;
      delPanel.appendChild(errEl);

      const delBtns = document.createElement("div");
      delBtns.className = "inline-notebook-delete-actions";

      const entryCount = Number(nb.entry_count ?? 0);
      const runDelete = async (deleteEntries) => {
        if (deleteNbBusy) return;
        deleteNbBusy = true;
        errEl.hidden = true;
        delPanel.querySelectorAll("button").forEach((b) => {
          b.disabled = true;
        });
        try {
          await callbacks.onDeleteNotebook?.(nb.id, deleteEntries);
          deletingNotebookId = null;
        } catch (err) {
          errEl.textContent = String(err?.message || err || "Delete failed");
          errEl.hidden = false;
          delPanel.querySelectorAll("button").forEach((b) => {
            b.disabled = false;
          });
        } finally {
          deleteNbBusy = false;
        }
      };

      if (entryCount > 0) {
        const keepBtn = document.createElement("button");
        keepBtn.type = "button";
        keepBtn.className = "btn-editorial small";
        keepBtn.textContent = `Keep ${entryCount} ${entryCount === 1 ? "entry" : "entries"}`;
        keepBtn.addEventListener("click", () => runDelete(false));

        const wipeBtn = document.createElement("button");
        wipeBtn.type = "button";
        wipeBtn.className = "btn-editorial btn-terracotta small";
        wipeBtn.textContent = `Delete + ${entryCount} ${entryCount === 1 ? "entry" : "entries"}`;
        wipeBtn.addEventListener("click", () => runDelete(true));

        delBtns.append(keepBtn, wipeBtn);
      } else {
        const confirmBtn = document.createElement("button");
        confirmBtn.type = "button";
        confirmBtn.className = "btn-editorial btn-terracotta small";
        confirmBtn.textContent = "Delete";
        confirmBtn.addEventListener("click", () => runDelete(false));
        delBtns.appendChild(confirmBtn);
      }

      const cancelBtn = document.createElement("button");
      cancelBtn.type = "button";
      cancelBtn.className = "btn-editorial btn-ghost small";
      cancelBtn.textContent = "Cancel";
      cancelBtn.addEventListener("click", () => {
        deletingNotebookId = null;
        renderSidebar(container, state, callbacks);
      });
      delBtns.appendChild(cancelBtn);

      delPanel.appendChild(delBtns);
      nbList.appendChild(delPanel);
    }
  }

  nbSection.appendChild(nbList);
  scroll.appendChild(nbSection);

  // Tags section (only when non-empty)
  const tags = state.tags || [];
  if (tags.length > 0) {
    const tagSection = document.createElement("section");
    tagSection.className = "sidebar-section";

    const tagHead = document.createElement("div");
    tagHead.className = "sidebar-section-header";
    const tagTitle = document.createElement("span");
    tagTitle.className = "small-caps-meta";
    tagTitle.textContent = "Tags";
    tagHead.appendChild(tagTitle);

    if (state.selectedTag) {
      const clearBtn = document.createElement("button");
      clearBtn.type = "button";
      clearBtn.className = "link-clear-btn";
      clearBtn.textContent = "Clear";
      clearBtn.addEventListener("click", () => callbacks.onSelectTag?.(null));
      tagHead.appendChild(clearBtn);
    }

    tagSection.appendChild(tagHead);

    const tagList = document.createElement("div");
    tagList.className = "sidebar-tags-list";

    for (const item of tags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = `sidebar-tag-item${
        state.selectedTag === item.tag ? " is-active" : ""
      }`;

      const tName = document.createElement("span");
      tName.className = "sidebar-tag-name";
      tName.textContent = `#${item.tag}`;

      const tCount = document.createElement("span");
      tCount.className = "tag-item-count";
      tCount.textContent = String(item.count ?? 0);

      btn.append(tName, tCount);
      btn.addEventListener("click", () => {
        const next = state.selectedTag === item.tag ? null : item.tag;
        callbacks.onSelectTag?.(next);
      });
      tagList.appendChild(btn);
    }

    tagSection.appendChild(tagList);
    scroll.appendChild(tagSection);
  }

  container.appendChild(scroll);
  scroll.scrollTop = prevScroll;

  // Footer
  const footer = document.createElement("footer");
  footer.className = "sidebar-footer";

  const utilRow = document.createElement("div");
  utilRow.className = "sidebar-utility-row";

  const importBtn = document.createElement("button");
  importBtn.type = "button";
  importBtn.className = "sidebar-utility-btn";
  importBtn.appendChild(iconEl("upload", "tiny-icon"));
  importBtn.appendChild(document.createTextNode("Import"));
  importBtn.addEventListener("click", () => callbacks.onOpenMigration?.());

  const backupBtn = document.createElement("button");
  backupBtn.type = "button";
  backupBtn.className = "sidebar-utility-btn";
  backupBtn.appendChild(iconEl("shield", "tiny-icon"));
  backupBtn.appendChild(document.createTextNode("Backup"));
  backupBtn.addEventListener("click", () => callbacks.onOpenBackup?.());

  const shortcutsBtn = document.createElement("button");
  shortcutsBtn.type = "button";
  shortcutsBtn.className = "sidebar-icon-btn";
  shortcutsBtn.title = "Keyboard shortcuts (Ctrl+/)";
  shortcutsBtn.setAttribute("aria-label", "Keyboard shortcuts");
  shortcutsBtn.appendChild(iconEl("keyboard", "log-icon"));
  shortcutsBtn.addEventListener("click", () => callbacks.onOpenShortcuts?.());

  const themeBtn = document.createElement("button");
  themeBtn.type = "button";
  themeBtn.className = "sidebar-icon-btn";
  themeBtn.title = state.nightMode
    ? "Switch to daylight paper"
    : "Switch to candlelit night";
  themeBtn.setAttribute("aria-label", themeBtn.title);
  themeBtn.appendChild(iconEl(state.nightMode ? "sun" : "moon", "log-icon"));
  themeBtn.addEventListener("click", () => callbacks.onToggleTheme?.());

  const utilIcons = document.createElement("div");
  utilIcons.className = "sidebar-utility-icons";
  utilIcons.append(shortcutsBtn, themeBtn);

  utilRow.append(importBtn, backupBtn, utilIcons);

  const statsBtn = document.createElement("button");
  statsBtn.type = "button";
  statsBtn.className = "sidebar-stats-btn";
  statsBtn.title = "Open Journal Insights & Vault tools";
  const totalWords = Number(state.stats?.total_words ?? 0);
  const streak = Number(state.stats?.current_streak_days ?? 0);
  const streakSuffix = streak > 0 ? ` · ${streak}d streak` : "";
  statsBtn.appendChild(iconEl("chart", "tiny-icon"));
  const statsText = document.createElement("span");
  statsText.textContent = `${totalEntries} ${
    totalEntries === 1 ? "entry" : "entries"
  } · ${totalWords.toLocaleString()} words${streakSuffix}`;
  statsBtn.appendChild(statsText);
  statsBtn.addEventListener("click", () => callbacks.onOpenInsights?.());

  footer.append(utilRow, statsBtn);
  container.appendChild(footer);
}
