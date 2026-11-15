import { icon } from "./icons.js";

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

function iconEl(name, cls = "log-icon") {
  const wrap = document.createElement("span");
  wrap.style.display = "inline-flex";
  wrap.style.alignItems = "center";
  wrap.innerHTML = icon(name, cls);
  return wrap.firstElementChild || wrap;
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
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `sidebar-nav-item${
      state.activeView === item.key ? " is-active" : ""
    }`;
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
    if (addNotebookOpen && !draftNbCover) {
      draftNbCover = state.manifest?.covers?.[0]?.path || null;
    }
    renderSidebar(container, state, callbacks);
  });

  nbHead.append(nbTitle, addNbBtn);
  nbSection.appendChild(nbHead);

  if (addNotebookOpen) {
    const form = document.createElement("form");
    form.className = "inline-notebook-form";

    const nameInput = document.createElement("input");
    nameInput.type = "text";
    nameInput.className = "editorial-input";
    nameInput.placeholder = "Notebook name";
    nameInput.value = draftNbName;
    nameInput.required = true;
    nameInput.addEventListener("input", () => {
      draftNbName = nameInput.value;
    });
    form.appendChild(nameInput);

    const covers = (state.manifest?.covers || []).slice(0, 4);
    if (covers.length > 0) {
      const coverRow = document.createElement("div");
      coverRow.className = "cover-swatch-row";
      for (const cov of covers) {
        const sw = document.createElement("button");
        sw.type = "button";
        sw.className = `cover-swatch-btn${
          draftNbCover === cov.path ? " selected" : ""
        }`;
        sw.title = cov.label || "Cover wash";
        const img = document.createElement("img");
        img.src = cov.path;
        img.alt = cov.label || "Cover";
        sw.appendChild(img);
        sw.addEventListener("click", () => {
          draftNbCover = cov.path;
          coverRow
            .querySelectorAll(".cover-swatch-btn")
            .forEach((b) => b.classList.remove("selected"));
          sw.classList.add("selected");
        });
        coverRow.appendChild(sw);
      }
      form.appendChild(coverRow);
    }

    const accentRow = document.createElement("div");
    accentRow.className = "accent-swatch-row";
    for (const color of ACCENT_PALETTE) {
      const dot = document.createElement("button");
      dot.type = "button";
      dot.className = `accent-dot-btn${
        draftNbAccent === color ? " selected" : ""
      }`;
      dot.style.background = color;
      dot.title = color;
      dot.addEventListener("click", () => {
        draftNbAccent = color;
        accentRow
          .querySelectorAll(".accent-dot-btn")
          .forEach((b) => b.classList.remove("selected"));
        dot.classList.add("selected");
      });
      accentRow.appendChild(dot);
    }
    form.appendChild(accentRow);

    const submitRow = document.createElement("div");
    submitRow.style.display = "flex";
    submitRow.style.gap = "0.4rem";

    const createBtn = document.createElement("button");
    createBtn.type = "submit";
    createBtn.className = "btn-editorial btn-sage small";
    createBtn.textContent = draftNbBusy ? "Creating..." : "Create";
    createBtn.disabled = draftNbBusy;

    const cancelNbBtn = document.createElement("button");
    cancelNbBtn.type = "button";
    cancelNbBtn.className = "btn-editorial btn-ghost small";
    cancelNbBtn.textContent = "Cancel";
    cancelNbBtn.addEventListener("click", () => {
      addNotebookOpen = false;
      draftNbName = "";
      renderSidebar(container, state, callbacks);
    });

    submitRow.append(createBtn, cancelNbBtn);
    form.appendChild(submitRow);

    form.addEventListener("submit", async (ev) => {
      ev.preventDefault();
      const trimmed = draftNbName.trim();
      if (!trimmed || draftNbBusy) return;
      draftNbBusy = true;
      createBtn.disabled = true;
      createBtn.textContent = "Creating...";
      try {
        await callbacks.onCreateNotebook?.({
          name: trimmed,
          description: null,
          cover_asset: draftNbCover,
          accent_color: draftNbAccent,
        });
        addNotebookOpen = false;
        draftNbName = "";
      } finally {
        draftNbBusy = false;
      }
    });

    nbSection.appendChild(form);
  }

  const nbList = document.createElement("div");
  nbList.className = "notebook-shelf-list";

  const allNbBtn = document.createElement("button");
  allNbBtn.type = "button";
  allNbBtn.className = `sidebar-notebook-item${
    !state.selectedNotebookId ? " is-active" : ""
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
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `sidebar-notebook-item${
      state.selectedNotebookId === nb.id ? " is-active" : ""
    }`;

    const dot = document.createElement("span");
    dot.className = "notebook-color-dot";
    dot.style.background = nb.accent_color || "var(--accent-primary)";

    const name = document.createElement("span");
    name.className = "notebook-name";
    name.textContent = nb.name;

    const count = document.createElement("span");
    count.className = "notebook-count";
    count.textContent = String(nb.entry_count ?? 0);

    btn.append(dot, name, count);
    btn.addEventListener("click", () => callbacks.onSelectNotebook?.(nb.id));
    nbList.appendChild(btn);
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

  const themeBtn = document.createElement("button");
  themeBtn.type = "button";
  themeBtn.className = "sidebar-icon-btn";
  themeBtn.title = state.nightMode
    ? "Switch to daylight paper"
    : "Switch to candlelit night";
  themeBtn.setAttribute("aria-label", themeBtn.title);
  themeBtn.appendChild(iconEl(state.nightMode ? "sun" : "moon", "log-icon"));
  themeBtn.addEventListener("click", () => callbacks.onToggleTheme?.());

  utilRow.append(importBtn, backupBtn, themeBtn);

  const statsLine = document.createElement("div");
  statsLine.className = "sidebar-stats-line";
  const totalWords = Number(state.stats?.total_words ?? 0);
  statsLine.textContent = `${totalEntries} ${
    totalEntries === 1 ? "entry" : "entries"
  } · ${totalWords.toLocaleString()} words`;

  footer.append(utilRow, statsLine);
  container.appendChild(footer);
}
