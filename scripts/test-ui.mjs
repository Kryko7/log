#!/usr/bin/env node
/**
 * Zero-dependency ES-module DOM integration test suite for Log UI.
 * Exercises actual ui/src/*.js modules end-to-end under a deterministic DOM shim
 * and verifies CSS, manifest, and animated asset invariants.
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const uiDir = path.join(repoRoot, "ui");
const uiSrcUrl = (file) => pathToFileURL(path.join(uiDir, "src", file)).href;

// Minimal deterministic DOM shim sufficient to execute all ui/src/*.js modules end-to-end
class FakeClassList {
  constructor(el) {
    this.el = el;
    this.set = new Set();
  }
  _syncFromAttr(v) {
    this.set = new Set(String(v || "").split(/\s+/).filter(Boolean));
  }
  _syncToAttr() {
    this.el._className = [...this.set].join(" ");
  }
  add(...names) {
    for (const n of names) if (n) this.set.add(n);
    this._syncToAttr();
  }
  remove(...names) {
    for (const n of names) this.set.delete(n);
    this._syncToAttr();
  }
  toggle(n, force) {
    const on = force === undefined ? !this.set.has(n) : Boolean(force);
    if (on) this.set.add(n);
    else this.set.delete(n);
    this._syncToAttr();
    return on;
  }
  contains(n) {
    return this.set.has(n);
  }
}

class FakeNode {
  constructor(nodeType, text = "") {
    this.nodeType = nodeType;
    this._text = text;
    this.parentNode = null;
    this.childNodes = [];
    this._listeners = new Map();
  }
  get isConnected() {
    let cur = this;
    while (cur) {
      if (cur === document.body || cur === document.documentElement) return true;
      cur = cur.parentNode;
    }
    return false;
  }
  get textContent() {
    if (this.nodeType === 3) return this._text;
    return this.childNodes.map((c) => c.textContent).join("");
  }
  set textContent(v) {
    if (this.nodeType === 3) {
      this._text = String(v ?? "");
      return;
    }
    this.childNodes = [];
    if (v !== "" && v !== null && v !== undefined) {
      this.appendChild(new FakeNode(3, String(v)));
    }
  }
  appendChild(child) {
    if (child.nodeType === 11) {
      for (const c of [...child.childNodes]) this.appendChild(c);
      child.childNodes = [];
      return child;
    }
    if (child.parentNode) child.remove();
    child.parentNode = this;
    this.childNodes.push(child);
    return child;
  }
  append(...items) {
    for (const it of items) {
      if (typeof it === "string") this.appendChild(new FakeNode(3, it));
      else if (it) this.appendChild(it);
    }
  }
  remove() {
    if (!this.parentNode) return;
    const idx = this.parentNode.childNodes.indexOf(this);
    if (idx >= 0) this.parentNode.childNodes.splice(idx, 1);
    this.parentNode = null;
  }
  replaceChild(newChild, oldChild) {
    const idx = this.childNodes.indexOf(oldChild);
    if (idx < 0) return oldChild;
    if (newChild.nodeType === 11) {
      const items = [...newChild.childNodes];
      newChild.childNodes = [];
      for (const n of items) n.parentNode = this;
      this.childNodes.splice(idx, 1, ...items);
    } else {
      if (newChild.parentNode) newChild.remove();
      newChild.parentNode = this;
      this.childNodes.splice(idx, 1, newChild);
    }
    oldChild.parentNode = null;
    return oldChild;
  }
  contains(other) {
    let cur = other;
    while (cur) {
      if (cur === this) return true;
      cur = cur.parentNode;
    }
    return false;
  }
  addEventListener(type, fn, opts = {}) {
    if (!this._listeners.has(type)) this._listeners.set(type, []);
    const arr = this._listeners.get(type);
    arr.push(fn);
    if (opts?.signal) {
      opts.signal.addEventListener(
        "abort",
        () => this.removeEventListener(type, fn),
        { once: true }
      );
    }
  }
  removeEventListener(type, fn) {
    const arr = this._listeners.get(type);
    if (!arr) return;
    const idx = arr.indexOf(fn);
    if (idx >= 0) arr.splice(idx, 1);
  }
  dispatchEvent(ev) {
    ev.target = ev.target || this;
    ev.currentTarget = this;
    ev.defaultPrevented = false;
    ev.preventDefault = () => {
      ev.defaultPrevented = true;
    };
    ev.stopPropagation = () => {
      ev._stopped = true;
    };
    const arr = [...(this._listeners.get(ev.type) || [])];
    for (const fn of arr) fn(ev);
    if (!ev._stopped && this.parentNode) this.parentNode.dispatchEvent(ev);
    return !ev.defaultPrevented;
  }
  click() {
    if (this.disabled) return;
    this.dispatchEvent({ type: "click" });
    if (this.tagName === "BUTTON" && this.type === "submit") {
      let p = this.parentNode;
      while (p && p.tagName !== "FORM") p = p.parentNode;
      if (p) p.dispatchEvent({ type: "submit" });
    }
  }
}

class FakeElement extends FakeNode {
  constructor(tagName) {
    super(1);
    this.tagName = String(tagName).toUpperCase();
    this._className = "";
    this.classList = new FakeClassList(this);
    this.style = {
      setProperty(k, v) {
        this[k] = v;
      },
    };
    this.attributes = {};
    this.dataset = {};
    this.hidden = false;
    this.disabled = false;
    this.value = "";
    this.checked = false;
    this.selectionStart = 0;
    this.selectionEnd = 0;
    this.scrollTop = 0;
    this.scrollHeight = 500;
    this.clientHeight = 500;
    this._innerHTML = "";
  }
  get className() {
    return this._className;
  }
  set className(v) {
    this._className = String(v || "");
    this.classList._syncFromAttr(this._className);
  }
  get firstElementChild() {
    return this.childNodes.find((c) => c.nodeType === 1) || null;
  }
  get innerHTML() {
    return this._innerHTML;
  }
  set innerHTML(v) {
    this._innerHTML = String(v || "");
    this.childNodes = [];
    if (this._innerHTML.includes("<svg")) {
      const svg = new FakeElement("svg");
      svg._innerHTML = this._innerHTML;
      this.appendChild(svg);
    }
    const stripped = this._innerHTML
      .replace(/<svg[\s\S]*?<\/svg>/gi, "")
      .replace(/<[^>]+>/g, "");
    if (stripped) {
      const sp = new FakeElement("span");
      sp.textContent = stripped;
      this.appendChild(sp);
    }
  }
  setAttribute(k, v) {
    this.attributes[k] = String(v);
    if (k === "class") this.className = v;
  }
  getAttribute(k) {
    return this.attributes[k] ?? null;
  }
  focus() {
    document.activeElement = this;
  }
  setSelectionRange(a, b) {
    this.selectionStart = a;
    this.selectionEnd = b;
  }
  scrollIntoView() {}
  getBoundingClientRect() {
    return { left: 0, top: 0, width: 300, height: 200 };
  }
  closest(sel) {
    let cur = this;
    while (cur && cur.nodeType === 1) {
      if (matchesSimpleSelector(cur, sel)) return cur;
      cur = cur.parentNode;
    }
    return null;
  }
  querySelector(sel) {
    return this.querySelectorAll(sel)[0] || null;
  }
  querySelectorAll(sel) {
    const out = [];
    const walk = (n) => {
      for (const c of n.childNodes) {
        if (c.nodeType === 1) {
          if (matchesSimpleSelector(c, sel)) out.push(c);
          walk(c);
        }
      }
    };
    walk(this);
    return out;
  }
}

function matchesSimpleSelector(el, sel) {
  if (!sel) return false;
  const attrM = /^\[([a-z0-9-]+)=["']([^"']+)["']\]$/i.exec(sel);
  if (attrM) {
    const [, k, v] = attrM;
    if (k.startsWith("data-")) {
      const camel = k.slice(5).replace(/-([a-z])/g, (_, ch) => ch.toUpperCase());
      return el.dataset[camel] === v || el.attributes[k] === v;
    }
    return el.attributes[k] === v;
  }
  if (sel.includes(" ")) {
    const parts = sel.trim().split(/\s+/);
    const last = parts[parts.length - 1];
    if (!matchesSimpleSelector(el, last)) return false;
    let cur = el.parentNode;
    for (let i = parts.length - 2; i >= 0; i -= 1) {
      let found = false;
      while (cur && cur.nodeType === 1) {
        if (matchesSimpleSelector(cur, parts[i])) {
          found = true;
          cur = cur.parentNode;
          break;
        }
        cur = cur.parentNode;
      }
      if (!found) return false;
    }
    return true;
  }
  if (sel.startsWith(".")) {
    return sel.slice(1).split(".").every((cls) => el.classList.contains(cls));
  }
  if (sel.includes(".")) {
    const [tag, ...classes] = sel.split(".");
    return (
      el.tagName === tag.toUpperCase() &&
      classes.every((cls) => el.classList.contains(cls))
    );
  }
  return el.tagName === sel.toUpperCase();
}

const winListeners = new Map();
globalThis.window = {
  __TAURI_INTERNALS__: { invoke: async () => null },
  scrollX: 0,
  scrollY: 0,
  scrollTo() {},
  addEventListener(t, fn, opts = {}) {
    if (!winListeners.has(t)) winListeners.set(t, []);
    winListeners.get(t).push(fn);
    if (opts?.signal) {
      opts.signal.addEventListener("abort", () => this.removeEventListener(t, fn), {
        once: true,
      });
    }
  },
  removeEventListener(t, fn) {
    const arr = winListeners.get(t);
    if (!arr) return;
    const idx = arr.indexOf(fn);
    if (idx >= 0) arr.splice(idx, 1);
  },
  dispatchEvent(ev) {
    ev.preventDefault = () => {
      ev.defaultPrevented = true;
    };
    for (const fn of [...(winListeners.get(ev.type) || [])]) fn(ev);
  },
  print() {
    window.__printed = true;
  },
};
globalThis.document = {
  documentElement: new FakeElement("html"),
  body: new FakeElement("body"),
  activeElement: null,
  visibilityState: "visible",
  createElement: (tag) => new FakeElement(tag),
  createTextNode: (t) => new FakeNode(3, t),
  createDocumentFragment: () => new FakeNode(11),
  addEventListener(t, fn, opts) {
    window.addEventListener(`doc:${t}`, fn, opts);
  },
  removeEventListener(t, fn) {
    window.removeEventListener(`doc:${t}`, fn);
  },
  dispatchEvent(ev) {
    window.dispatchEvent({ ...ev, type: `doc:${ev.type}` });
  },
};
document.documentElement.appendChild(document.body);
globalThis.requestAnimationFrame = (cb) => {
  cb(performance.now());
  return 1;
};
globalThis.cancelAnimationFrame = () => {};
globalThis.CSS = { escape: (s) => String(s) };
globalThis.localStorage = { getItem: () => null, setItem() {} };

const { renderMarkdownReadView, extractWikilinks } = await import(
  uiSrcUrl("markdown.js")
);
const {
  renderTimelineView,
  renderCalendarView,
  renderSearchModal,
  renderMigrationModal,
  renderInsightsModal,
  renderShortcutsModal,
  openPolaroidLightbox,
  createPolaroidElement,
} = await import(uiSrcUrl("views.js"));
const { renderEntryEditor } = await import(uiSrcUrl("editor.js"));
const { renderSidebar } = await import(uiSrcUrl("sidebar.js"));
const { parseCoverAsset, mountCoverCinemagraph } = await import(
  uiSrcUrl("cover-fx.js")
);

const manifest = JSON.parse(
  fs.readFileSync(path.join(uiDir, "assets", "manifest.json"), "utf8")
);

let passed = 0;
function test(name, fn) {
  return Promise.resolve()
    .then(fn)
    .then(() => {
      passed += 1;
      console.log(`  PASS  ${name}`);
    });
}

// 1. Markdown Read View & Wikilink Extraction
await test("markdown.js renders h1-h6, table, strikethrough, highlight, underscore italic, preserves snake_case, extracts wikilinks, and blocks unsafe URLs", () => {
  const root = document.createElement("div");
  document.body.appendChild(root);
  let openedImg = null;
  let toggledLine = null;
  let externalOpened = null;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    if (cmd === "open_external_url") externalOpened = args.url;
    return null;
  };

  const md = [
    "# Title",
    "#### Deep Heading",
    "Check **bold**, *star*, _under_, snake_case_var, ~~old~~, ==fresh==, ***both***, and [[Autumn Walk|Walk]].",
    "[Safe](https://example.com) and [Bad](javascript:alert(1))",
    "- [ ] First task",
    "![Rain photo](/vault-media/photos/a.jpg)",
    "| ColA | ColB |",
    "| --- | --- |",
    "| One | ~~Two~~ |",
  ].join("\n");

  const wikis = extractWikilinks(md);
  assert.equal(wikis.length, 1);
  assert.equal(wikis[0].target, "Autumn Walk");
  assert.equal(wikis[0].label, "Walk");

  renderMarkdownReadView(
    root,
    md,
    [],
    null,
    (idx) => {
      toggledLine = idx;
    },
    (src, alt) => {
      openedImg = { src, alt };
    }
  );

  assert.equal(root.querySelectorAll("h1").length, 1);
  assert.equal(root.querySelectorAll("h4").length, 1);
  assert.equal(root.querySelectorAll("del").length, 2);
  assert.equal(root.querySelectorAll("mark").length, 1);
  assert.equal(root.querySelectorAll("table").length, 1);
  assert.equal(root.querySelectorAll("th").length, 2);
  assert.equal(root.querySelectorAll("td").length, 2);
  assert.ok(root.textContent.includes("snake_case_var"));

  const links = root.querySelectorAll("a");
  assert.equal(links.length, 1);
  assert.equal(links[0].href, "https://example.com");
  links[0].click();
  assert.equal(externalOpened, "https://example.com");

  const cb = root.querySelector("input");
  cb.dispatchEvent({ type: "change" });
  assert.equal(toggledLine, 4);

  const img = root.querySelector("img");
  img.click();
  assert.deepEqual(openedImg, {
    src: "/vault-media/photos/a.jpg",
    alt: "Rain photo",
  });
  root.remove();
});

// 2. Search Modal entity decoding, metadata & keyboard navigation
await test("renderSearchModal decodes HTML entities safely without XSS, shows metadata, and supports ArrowDown+Enter + filters", () => {
  const modal = document.createElement("div");
  document.body.appendChild(modal);
  let selectedId = null;
  let filterPatched = null;

  const state = {
    searchQuery: "tea",
    searchFilters: {
      starred_only: true,
      notebook_id: "",
      mood: "",
      tag: "",
      from_date: "",
      to_date: "",
    },
    searchLoading: false,
    notebooks: [{ id: "daily", name: "Daily Journal" }],
    tags: [{ tag: "tea", count: 2 }],
    searchResults: [
      {
        summary: {
          id: "e-1",
          title: "Tea & Herbs",
          date: "2026-11-22T10:00:00Z",
          notebook_id: "daily",
          mood: "calm",
          tags: ["oolong"],
          snippet: "Oolong & green",
          starred: true,
        },
        highlighted_snippet: "Drinking <mark>tea</mark> &amp; herbs &lt;warm&gt;",
      },
      {
        summary: {
          id: "e-2",
          title: "Second Hit",
          date: "2026-11-21T10:00:00Z",
          notebook_id: "daily",
          snippet: "Another",
          starred: true,
        },
        highlighted_snippet: "Another <mark>tea</mark> note",
      },
    ],
  };

  renderSearchModal(modal, state, {
    onFilterChange: (p) => {
      filterPatched = p;
    },
    onSelectEntry: (id) => {
      selectedId = id;
    },
    onClose: () => {},
  });

  const snippets = modal.querySelectorAll(".search-hit-snippet");
  assert.equal(snippets.length, 2);
  assert.equal(snippets[0].textContent, "Drinking tea & herbs <warm>");
  assert.ok(!snippets[0].textContent.includes("&amp;"));
  const metaText = modal.querySelector(".search-hit-meta")?.textContent || "";
  assert.ok(metaText.includes("Daily Journal"));
  assert.ok(metaText.includes("calm"));
  assert.ok(metaText.includes("#oolong"));

  const input = modal.querySelector(".search-modal-input");
  const sheet = modal.querySelector(".search-modal-sheet");
  input.focus();
  sheet.dispatchEvent({ type: "keydown", key: "ArrowDown" });
  assert.ok(
    modal.querySelectorAll(".search-hit-card")[1].classList.contains("is-selected")
  );
  sheet.dispatchEvent({ type: "keydown", key: "Enter" });
  assert.equal(selectedId, "e-2");

  modal.querySelector('[data-search-filter="starred"]').click();
  assert.deepEqual(filterPatched, { starred_only: false });
  modal.remove();
});

// 3. Calendar multi-entry day drawer, UTC day matching & notebook filter
await test("renderCalendarView opens Selected Day Drawer on multi-entry days, matches UTC date 1:1, and filters by notebook", () => {
  const root = document.createElement("div");
  document.body.appendChild(root);
  let nbChanged = null;
  let newDate = null;
  let openedId = null;

  const state = {
    calendarYear: 2026,
    calendarMonth: 11,
    selectedNotebookId: null,
    notebooks: [
      { id: "daily", name: "Daily" },
      { id: "travel", name: "Travel" },
    ],
    calendarDays: [
      {
        date: "2026-11-22",
        entry_count: 2,
        starred_count: 1,
        moods: ["calm"],
        primary_entry_id: "e-1",
        has_photos: true,
        has_audio: false,
      },
    ],
    entries: [
      {
        id: "e-1",
        title: "Morning Entry",
        date: "2026-11-22T09:00:00Z",
        notebook_id: "daily",
        tags: [],
        snippet: "Morning",
        reading_minutes: 1,
      },
      {
        id: "e-2",
        title: "Late Night UTC Entry",
        date: "2026-11-22T23:55:00Z",
        notebook_id: "daily",
        tags: [],
        snippet: "Late",
        reading_minutes: 1,
      },
    ],
  };

  renderCalendarView(root, state, {
    onChangeNotebookFilter: (nb) => {
      nbChanged = nb;
    },
    onNewEntryForDate: (d) => {
      newDate = d;
    },
    onOpenEntry: (id) => {
      openedId = id;
    },
  });

  const cells = root.querySelectorAll(".calendar-day-cell");
  const day22 = cells.find(
    (c) =>
      !c.classList.contains("is-pad") &&
      c.querySelector(".calendar-day-number")?.textContent === "22"
  );
  const day20 = cells.find(
    (c) =>
      !c.classList.contains("is-pad") &&
      c.querySelector(".calendar-day-number")?.textContent === "20"
  );

  day22.click();
  const drawer = root.querySelector(".calendar-day-drawer");
  assert.equal(drawer.hidden, false);
  const drawerRows = drawer.querySelectorAll(".timeline-entry-row");
  assert.equal(drawerRows.length, 2);
  drawerRows[1].click();
  assert.equal(openedId, "e-1");

  day20.click();
  assert.equal(newDate, "2026-11-20");

  const nbSel = root.querySelector(".filter-pill-select");
  nbSel.value = "travel";
  nbSel.dispatchEvent({ type: "change" });
  assert.equal(nbChanged, "travel");
  root.remove();
});

// 4. Timeline filter, sort & media-only fallback labels
await test("renderTimelineView filters, sorts, and renders muted fallbacks for untitled media-only entries", () => {
  const root = document.createElement("div");
  document.body.appendChild(root);
  const state = {
    selectedNotebookId: null,
    selectedTag: null,
    starredOnly: false,
    notebooks: [{ id: "daily", name: "Daily" }],
    filteredEntries: [
      {
        id: "1",
        title: "Short Newer",
        date: "2026-11-22T10:00:00Z",
        notebook_id: "daily",
        tags: [],
        mood: "calm",
        starred: true,
        has_photos: true,
        has_audio: false,
        word_count: 10,
        reading_minutes: 1,
        snippet: "alpha",
      },
      {
        id: "2",
        title: "Long Older",
        date: "2026-11-10T10:00:00Z",
        notebook_id: "daily",
        tags: [],
        mood: "grateful",
        starred: false,
        has_photos: false,
        has_audio: true,
        word_count: 500,
        reading_minutes: 3,
        snippet: "beta",
      },
      {
        id: "3",
        title: "Untitled entry",
        date: "2026-11-09T10:00:00Z",
        notebook_id: "daily",
        tags: [],
        has_photos: true,
        has_audio: false,
        word_count: 0,
        reading_minutes: 0,
        snippet: "",
      },
      {
        id: "4",
        title: "",
        date: "2026-11-08T10:00:00Z",
        notebook_id: "daily",
        tags: [],
        has_photos: false,
        has_audio: true,
        word_count: 0,
        reading_minutes: 0,
        snippet: "",
      },
    ],
  };

  renderTimelineView(root, state, {});
  assert.equal(root.querySelectorAll(".timeline-entry-row").length, 4);
  const muted = root
    .querySelectorAll(".timeline-row-excerpt.is-muted")
    .map((e) => e.textContent);
  assert.deepEqual(muted, ["Photo entry", "Voice note"]);

  const photosPill = root
    .querySelectorAll(".filter-pill-btn")
    .find((b) => b.textContent.includes("Photos"));
  photosPill.click();
  assert.equal(root.querySelectorAll(".timeline-entry-row").length, 2);
  photosPill.click();
  assert.equal(root.querySelectorAll(".timeline-entry-row").length, 4);

  const selects = root.querySelectorAll(".filter-pill-select");
  const sortSel = selects[selects.length - 1];
  sortSel.value = "longest";
  sortSel.dispatchEvent({ type: "change" });
  const titles = root
    .querySelectorAll(".timeline-row-title")
    .map((t) => t.textContent);
  assert.deepEqual(titles, ["Long Older", "Short Newer"]);
  root.remove();
});

// 5. Lightbox gallery & Polaroid instant caption input
await test("openPolaroidLightbox navigates multiple photos and Polaroid caption updates immediately on input", () => {
  let latestCaption = null;
  const card = createPolaroidElement(
    { id: "p1", url: "/assets/covers/rainy-tea-desk.png", caption: "Old" },
    1,
    {
      onUpdateCaption: (_id, cap) => {
        latestCaption = cap;
      },
    }
  );
  const capInput = card.querySelector(".polaroid-caption-input");
  capInput.value = "Updated immediately";
  capInput.dispatchEvent({ type: "input" });
  assert.equal(latestCaption, "Updated immediately");

  openPolaroidLightbox(
    [
      { url: "/assets/covers/rainy-tea-desk.png", caption: "First" },
      { url: "/assets/covers/pressed-wildflowers.png", caption: "Second" },
    ],
    0
  );
  const lb = document.body.querySelector(".polaroid-lightbox-sheet");
  assert.ok(lb);
  assert.ok(
    lb.querySelector(".polaroid-lightbox-meta").textContent.includes("1 / 2")
  );
  lb.querySelector(".polaroid-lightbox-nav.is-next").click();
  assert.ok(
    lb.querySelector(".polaroid-lightbox-meta").textContent.includes("2 / 2")
  );
  assert.equal(
    lb.querySelector(".polaroid-lightbox-caption").textContent,
    "Second"
  );
  window.dispatchEvent({ type: "keydown", key: "Escape" });
  assert.equal(document.body.querySelector(".polaroid-lightbox-sheet"), null);
});

// 6. Editor cover persistence, date/weather popovers, Markdown shortcuts, list continuation & Tab indent
await test("renderEntryEditor persists cover_asset, edits date & weather, supports Markdown shortcuts, Tab indent, and list continuation", async () => {
  const root = document.createElement("div");
  document.body.appendChild(root);
  let savedPayload = null;

  const state = {
    editorMode: "write",
    notebooks: [{ id: "daily", name: "Daily" }],
    entries: [],
    filteredEntries: [],
    templates: [],
    manifest,
    currentEntry: {
      id: "e-edit",
      frontmatter: {
        id: "e-edit",
        title: "Test Page",
        date: "2026-11-22T10:00:00.000Z",
        notebook_id: "daily",
        tags: [],
        mood: null,
        starred: false,
        weather: { condition: "Clear", temperature_c: 21, icon_key: "sun" },
        location: null,
        stamps: [],
        photos: [],
        audio: [],
        cover_asset: "/assets/covers/botanical-fern-wash.png",
      },
      body: "hello world",
      path: "entries/daily/2026/11/test.md",
      backlinks: [],
    },
  };

  const handle = renderEntryEditor(root, state, {
    onSaveEntry: async (payload) => {
      savedPayload = payload;
      return {
        id: "e-edit",
        frontmatter: payload,
        body: payload.body,
        path: "entries/daily/2026/11/test.md",
        backlinks: [],
      };
    },
  });

  assert.ok(root.querySelector(".entry-cover-banner"));

  const ta = root.querySelector(".entry-prose-textarea");
  ta.setSelectionRange(0, 5);
  ta.dispatchEvent({ type: "keydown", key: "b", ctrlKey: true });
  assert.equal(ta.value, "**hello** world");

  // Tab indents selected line, Shift+Tab outdents
  ta.value = "line one";
  ta.dispatchEvent({ type: "input" });
  ta.setSelectionRange(0, 8);
  ta.dispatchEvent({ type: "keydown", key: "Tab" });
  assert.equal(ta.value, "  line one");
  ta.setSelectionRange(0, ta.value.length);
  ta.dispatchEvent({ type: "keydown", key: "Tab", shiftKey: true });
  assert.equal(ta.value, "line one");

  // Smart Enter list continuation
  ta.value = "- first item";
  ta.dispatchEvent({ type: "input" });
  ta.setSelectionRange(12, 12);
  ta.dispatchEvent({ type: "keydown", key: "Enter" });
  assert.equal(ta.value, "- first item\n- ");

  // Weather popover opens on click instead of deleting immediately
  const wxBtn = root
    .querySelectorAll(".meta-inline-btn")
    .find((b) => b.textContent.includes("Clear"));
  wxBtn.click();
  const wxPop = root.querySelector(".entry-weather-popover");
  assert.ok(wxPop);
  wxPop
    .querySelectorAll(".weather-preset-btn")
    .find((b) => b.textContent.includes("Storm"))
    .click();
  wxPop.querySelector(".weather-temp-input").value = "17";
  wxPop.dispatchEvent({ type: "submit" });

  // Date popover updates date
  root.querySelector(".entry-dateline-btn").click();
  const datePop = root.querySelector(".entry-date-popover");
  assert.ok(datePop);
  const dateInputs = datePop.querySelectorAll(".editorial-input");
  dateInputs[0].value = "2026-10-15";
  dateInputs[1].value = "08:30";
  datePop.dispatchEvent({ type: "submit" });

  await handle.saveNow();
  assert.ok(savedPayload);
  assert.equal(savedPayload.cover_asset, "/assets/covers/botanical-fern-wash.png");
  assert.equal(savedPayload.weather.condition, "Storm");
  assert.equal(savedPayload.weather.temperature_c, 17);
  assert.equal(savedPayload.weather.icon_key, "storm");
  assert.ok(savedPayload.date.startsWith("2026-10-15"));

  handle.destroy();
  root.remove();
});

// 7. Editor Find-in-Entry, Title Enter jump, in-place syncNav, blur flush & bounded save-error indicator
await test("editor supports Find-in-Entry (Write & Read), Title Enter jump, in-place syncNav, blur flush, and bounded save Error label", async () => {
  const root = document.createElement("div");
  document.body.appendChild(root);
  let saveCalls = 0;
  let shouldFailSave = false;
  let lastToast = null;

  const state = {
    editorMode: "write",
    notebooks: [{ id: "daily", name: "Daily" }],
    entries: [
      { id: "e-next", title: "Next" },
      { id: "e-find", title: "Find Test" },
      { id: "e-prev", title: "Prev" },
    ],
    filteredEntries: [{ id: "e-find", title: "Find Test" }],
    templates: [],
    manifest,
    currentEntry: {
      id: "e-find",
      frontmatter: {
        id: "e-find",
        title: "Find Test",
        date: "2026-11-22T10:00:00.000Z",
        notebook_id: "daily",
        tags: [],
        mood: null,
        starred: false,
        weather: null,
        location: null,
        stamps: [],
        photos: [],
        audio: [],
        cover_asset: null,
      },
      body: "Rainy tea morning and afternoon tea ritual.",
      path: "entries/daily/2026/11/find.md",
      backlinks: [],
    },
  };

  const handle = renderEntryEditor(root, state, {
    onToast: (msg, tone) => {
      lastToast = { msg, tone };
    },
    onSaveEntry: async (payload) => {
      saveCalls += 1;
      if (shouldFailSave) {
        throw new Error("Simulated very long disk write failure message from vault");
      }
      return {
        id: "e-find",
        frontmatter: payload,
        body: payload.body,
        path: "entries/daily/2026/11/find.md",
        backlinks: [],
      };
    },
  });

  const titleInput = root.querySelector(".entry-title-input");
  const ta = root.querySelector(".entry-prose-textarea");
  titleInput.focus();
  titleInput.dispatchEvent({ type: "keydown", key: "Enter" });
  assert.equal(document.activeElement, ta);

  // syncNav updates Prev/Next in-place without destroying nbSelect
  const nbSelectBefore = root.querySelector(".topbar-notebook-select");
  const prevBtnBefore = root
    .querySelectorAll(".topbar-ghost-btn")
    .find((b) => b.textContent.includes("Prev"));
  assert.equal(prevBtnBefore.disabled, true);
  state.filteredEntries = [...state.entries];
  handle.syncNav();
  assert.equal(root.querySelector(".topbar-notebook-select"), nbSelectBefore);
  assert.equal(prevBtnBefore.disabled, false);

  // Find-in-Entry Write mode
  handle.openFind();
  const findInput = root.querySelector(".entry-find-input");
  findInput.focus();
  findInput.value = "tea";
  findInput.dispatchEvent({ type: "input" });
  assert.equal(root.querySelector(".entry-find-count").textContent, "1 / 2");

  findInput.dispatchEvent({ type: "keydown", key: "Enter" });
  assert.equal(root.querySelector(".entry-find-count").textContent, "2 / 2");
  assert.equal(document.activeElement, findInput);
  assert.equal(ta.value.slice(ta.selectionStart, ta.selectionEnd), "tea");

  // Read mode marks + Escape cleanup
  handle.toggleMode();
  const marks = root.querySelectorAll("mark.entry-find-match");
  assert.equal(marks.length, 2);
  assert.ok(marks[1].classList.contains("is-active"));
  findInput.dispatchEvent({ type: "keydown", key: "Escape" });
  assert.equal(root.querySelector(".entry-find-slot").hidden, true);
  assert.equal(root.querySelectorAll("mark.entry-find-match").length, 0);

  // Blur flushes dirty edits
  handle.toggleMode();
  const ta2 = root.querySelector(".entry-prose-textarea");
  ta2.value = "Updated on blur";
  ta2.dispatchEvent({ type: "input" });
  assert.equal(root.querySelector(".entry-save-indicator").textContent, "Unsaved");
  window.dispatchEvent({ type: "blur" });
  await Promise.resolve();
  assert.equal(saveCalls, 1);

  // Save failure keeps indicator compact ("Error") and shows toast
  shouldFailSave = true;
  ta2.value = "Trigger failure";
  ta2.dispatchEvent({ type: "input" });
  await assert.rejects(() => handle.saveNow());
  const ind = root.querySelector(".entry-save-indicator");
  assert.equal(ind.textContent, "Error");
  assert.ok(ind.title.includes("Simulated very long disk write failure"));
  assert.equal(lastToast?.tone, "error");

  handle.destroy();
  root.remove();
});

// 8. Stamp & Sticker placement, quick-remove, keyboard Delete, Picker & Ledger management, and Washi tab
await test("stamps support quick-remove button, keyboard Delete (not while typing), Picker & Ledger management, and Washi tab", () => {
  const root = document.createElement("div");
  document.body.appendChild(root);

  const state = {
    editorMode: "write",
    notebooks: [{ id: "daily", name: "Daily" }],
    entries: [],
    filteredEntries: [],
    templates: [],
    manifest,
    currentEntry: {
      id: "e-stamps",
      frontmatter: {
        id: "e-stamps",
        title: "Scrapbook Page",
        date: "2026-11-22T10:00:00.000Z",
        notebook_id: "daily",
        tags: [],
        mood: null,
        starred: false,
        weather: null,
        location: null,
        stamps: [],
        photos: [],
        audio: [],
        cover_asset: null,
      },
      body: "Writing under botanical stamps.",
      path: "entries/daily/2026/11/stamps.md",
      backlinks: [],
    },
  };

  const handle = renderEntryEditor(root, state, {
    onSaveEntry: async (payload) => ({
      id: "e-stamps",
      frontmatter: payload,
      body: payload.body,
      path: "entries/daily/2026/11/stamps.md",
      backlinks: [],
    }),
  });

  root
    .querySelectorAll(".insert-action-btn")
    .find((b) => b.textContent.includes("Stamp"))
    .click();

  const washiTab = root
    .querySelectorAll(".stamp-tab-btn")
    .find((b) => b.textContent === "Washi");
  washiTab.click();
  assert.equal(root.querySelectorAll(".stamp-choice-card").length, 5);
  root.querySelectorAll(".stamp-choice-card")[0].click();

  assert.ok(
    root
      .querySelectorAll(".stamp-tab-btn")
      .find((b) => b.textContent === "Washi")
      .classList.contains("is-active")
  );
  assert.equal(root.querySelectorAll(".placed-stamp-node").length, 1);
  assert.ok(root.querySelector(".placed-stamp-node").classList.contains("is-washi"));
  assert.ok(
    root
      .querySelectorAll(".insert-action-btn")
      .some((b) => b.textContent.includes("Stamp (1)"))
  );
  assert.equal(
    root.querySelectorAll(".stamp-picker-popover .stamp-placed-chip").length,
    1
  );
  assert.equal(
    root.querySelectorAll(".ledger-stamps-bar .stamp-placed-chip").length,
    1
  );

  // Switch to Stickers tab & add 2 more
  root
    .querySelectorAll(".stamp-tab-btn")
    .find((b) => b.textContent === "Stickers")
    .click();
  root.querySelectorAll(".stamp-choice-card")[2].click();
  root.querySelectorAll(".stamp-choice-card")[3].click();
  assert.equal(root.querySelectorAll(".placed-stamp-node").length, 3);

  // One-click corner quick-remove
  root.querySelector(".stamp-quick-remove").click();
  assert.equal(root.querySelectorAll(".placed-stamp-node").length, 2);

  // Delete inside textarea must NOT delete selected stamp; Delete on stamp removes it
  const firstNode = root.querySelector(".placed-stamp-node");
  firstNode.dispatchEvent({ type: "mousedown" });
  root.querySelector(".entry-prose-textarea").focus();
  window.dispatchEvent({ type: "keydown", key: "Delete" });
  assert.equal(root.querySelectorAll(".placed-stamp-node").length, 2);
  firstNode.focus();
  window.dispatchEvent({ type: "keydown", key: "Delete" });
  assert.equal(root.querySelectorAll(".placed-stamp-node").length, 1);

  // Remove from Entry Details ledger
  root.querySelector(".ledger-stamps-bar .stamp-placed-remove").click();
  assert.equal(root.querySelectorAll(".placed-stamp-node").length, 0);

  // Remove all from Picker
  root.querySelectorAll(".stamp-choice-card")[0].click();
  root.querySelectorAll(".stamp-choice-card")[1].click();
  root.querySelector(".stamp-picker-popover .stamp-clear-all-btn").click();
  assert.equal(root.querySelectorAll(".placed-stamp-node").length, 0);

  handle.destroy();
  root.remove();
});

// 9. Animated covers, Page Appearance popover, Zen Focus, reading progress & all-14 notebook covers
await test("animated covers, Page Appearance popover, Zen Focus toggle, reading progress hairline, and all-14 notebook covers work end-to-end", () => {
  const root = document.createElement("div");
  const sb = document.createElement("aside");
  document.body.append(root, sb);

  let rulingChosen = null;
  let scaleChosen = null;
  let focusToggled = 0;

  const state = {
    editorMode: "write",
    paperRuling: "plain",
    proseScale: "editorial",
    focusZen: false,
    notebooks: [{ id: "daily", name: "Daily", entry_count: 1 }],
    entries: [],
    filteredEntries: [],
    templates: [],
    tags: [],
    manifest,
    currentEntry: {
      id: "e-anim",
      frontmatter: {
        id: "e-anim",
        title: "Living Cover",
        date: "2026-11-22T10:00:00.000Z",
        notebook_id: "daily",
        tags: [],
        mood: null,
        starred: false,
        weather: null,
        location: null,
        stamps: [],
        photos: [],
        audio: [],
        cover_asset: null,
      },
      body: "Quiet morning tea by the window.",
      path: "entries/daily/2026/11/anim.md",
      backlinks: [],
    },
  };

  const handle = renderEntryEditor(root, state, {
    onSetPaperRuling: (r) => {
      rulingChosen = r;
    },
    onSetProseScale: (s) => {
      scaleChosen = s;
    },
    onToggleFocusZen: () => {
      focusToggled += 1;
      state.focusZen = !state.focusZen;
      handle.syncFocusState();
    },
    onSaveEntry: async (payload) => ({
      id: "e-anim",
      frontmatter: payload,
      body: payload.body,
      path: "entries/daily/2026/11/anim.md",
      backlinks: [],
    }),
  });

  assert.ok(root.querySelector(".entry-progress-hairline"));

  const focusBtn = root.querySelector('[data-role="zen-focus-btn"]');
  focusBtn.click();
  assert.equal(focusToggled, 1);
  assert.ok(focusBtn.classList.contains("is-active"));

  root.querySelector('[aria-label="Page appearance and covers"]').click();
  const pop = root.querySelector(".page-appearance-popover");
  assert.ok(pop);
  assert.equal(pop.querySelectorAll(".cover-live-badge").length, 6);

  pop.querySelector('[data-paper-ruling="dots"]').click();
  assert.equal(rulingChosen, "dots");
  root.querySelector('[data-prose-scale="large"]').click();
  assert.equal(scaleChosen, "large");

  root.querySelector(".cover-mini-btn.is-animated").click();
  assert.ok(root.querySelector(".entry-cover-banner.is-animated-cover"));
  assert.ok(root.querySelector(".entry-cover-fx-canvas"));

  root
    .querySelectorAll(".insert-action-btn")
    .find((b) => b.textContent.includes("Stamp"))
    .click();
  root.querySelector(".stamp-choice-card").click();
  assert.ok(root.querySelector(".placed-stamp-node.is-just-placed"));

  const delBtn = root.querySelector(".topbar-delete-btn");
  assert.ok(delBtn && delBtn.classList.contains("topbar-icon-btn"));

  renderSidebar(sb, state, {});
  sb.querySelector('[aria-label="New notebook"]').click();
  assert.equal(sb.querySelectorAll(".cover-swatch-btn").length, 15);
  assert.equal(sb.querySelectorAll(".cover-live-dot").length, 6);

  handle.destroy();
  root.remove();
  sb.remove();
});

// 10. Sidebar, Insights, Shortcuts & Migration modals
await test("sidebar, Insights modal, Shortcuts modal, and Migration modal render and wire callbacks cleanly", () => {
  const sb = document.createElement("aside");
  const modal = document.createElement("div");
  document.body.append(sb, modal);

  let navTo = null;
  let insightsOpened = false;
  let shortcutsOpened = false;

  const state = {
    activeView: "editor",
    nightMode: false,
    stats: {
      total_entries: 4,
      total_words: 1200,
      current_streak_days: 3,
      longest_streak_days: 9,
      entries_this_month: 4,
      total_photos: 2,
      total_audio_memos: 1,
      top_tags: [{ tag: "tea", count: 2 }],
      vault_path: "/tmp/vault",
    },
    entries: [
      { id: "1", starred: true, mood: "calm" },
      { id: "2", starred: false, mood: "calm" },
    ],
    notebooks: [{ id: "daily", name: "Daily", entry_count: 2 }],
    tags: [{ tag: "tea", count: 2 }],
    onThisDayCount: 1,
  };

  renderSidebar(sb, state, {
    onNavigate: (k) => {
      navTo = k;
    },
    onOpenInsights: () => {
      insightsOpened = true;
    },
    onOpenShortcuts: () => {
      shortcutsOpened = true;
    },
  });

  const iconGroup = sb.querySelector(".sidebar-utility-icons");
  assert.ok(iconGroup);
  assert.equal(iconGroup.querySelectorAll(".sidebar-icon-btn").length, 2);

  sb.querySelectorAll(".sidebar-nav-item")
    .find((b) => b.textContent.includes("Starred"))
    .click();
  assert.equal(navTo, "starred");

  const statsBtn = sb.querySelector(".sidebar-stats-btn");
  assert.ok(statsBtn.textContent.includes("3d streak"));
  statsBtn.click();
  assert.equal(insightsOpened, true);

  sb.querySelector('[aria-label="Keyboard shortcuts"]').click();
  assert.equal(shortcutsOpened, true);

  let syncedFull = null;
  renderInsightsModal(modal, state, {
    onSyncVault: (full) => {
      syncedFull = full;
    },
    onClose: () => {},
  });
  modal
    .querySelectorAll("button")
    .find((b) => b.textContent.includes("Rebuild Search Index"))
    .click();
  assert.equal(syncedFull, true);

  renderShortcutsModal(modal, { onClose: () => {} });
  assert.ok(modal.textContent.includes("Ctrl + K"));
  assert.ok(modal.textContent.includes("Ctrl + F"));
  assert.ok(modal.textContent.includes("Ctrl + Shift + F"));

  let importedArgs = null;
  renderMigrationModal(modal, state, {
    onImportDayOnePath: (p, dry, nb) => {
      importedArgs = { p, dry, nb };
    },
    onClose: () => {},
  });
  modal.querySelector(".editorial-select").value = "daily";
  modal.querySelector(".dry-run-toggle input").checked = true;
  modal.querySelector(".path-import-row input").value = "/tmp/dayone.zip";
  modal.querySelector('[data-role="migration-dayone-run"]').click();
  assert.deepEqual(importedArgs, { p: "/tmp/dayone.zip", dry: true, nb: "daily" });

  sb.remove();
  modal.remove();
});

// 11. Manifest asset integrity, cinemagraph cover FX parsing & CSS layout stability invariants
await test("manifest assets, cinemagraph cover-fx presets, legacy anim mapping, and CSS topbar/dark-mode invariants hold", () => {
  assert.equal(manifest.covers.length, 14);
  assert.equal(manifest.stamps.length, 7);
  assert.equal(manifest.stickers.length, 7);
  assert.equal(manifest.washi.length, 5);

  const animCovers = manifest.covers.filter(
    (c) => Array.isArray(c.tags) && c.tags.includes("animated")
  );
  assert.equal(animCovers.length, 6);
  const fxSeen = new Set();
  for (const cov of animCovers) {
    const parsed = parseCoverAsset(cov.path);
    assert.ok(parsed.fx, `${cov.id} must resolve a valid cinemagraph fx key`);
    fxSeen.add(parsed.fx);
    assert.ok(
      fs.existsSync(path.join(uiDir, parsed.basePath.replace(/^\/+/, ""))),
      `backdrop image for ${cov.id} must exist on disk`
    );
  }
  assert.equal(fxSeen.size, 6, "all 6 animated covers must use distinct fx scenes");

  // Legacy SVG animated cover paths transparently upgrade to painted backdrop + live FX
  const legacy = parseCoverAsset("/assets/covers/anim-rainy-window.svg");
  assert.equal(legacy.fx, "rain");
  assert.equal(legacy.basePath, "/assets/covers/rainy-tea-desk.png");

  // mountCoverCinemagraph cleanly creates & destroys its canvas
  const host = document.createElement("div");
  const handleFx = mountCoverCinemagraph(host, "fireflies");
  assert.ok(host.querySelector(".entry-cover-fx-canvas"));
  handleFx.destroy();
  assert.equal(host.querySelector(".entry-cover-fx-canvas"), null);

  const cssSrc = fs.readFileSync(path.join(uiDir, "styles", "main.css"), "utf8");
  const opens = (cssSrc.match(/\{/g) || []).length;
  const closes = (cssSrc.match(/\}/g) || []).length;
  assert.equal(opens, closes, "main.css must have balanced braces");
  assert.ok(
    cssSrc.includes("flex-wrap: nowrap;"),
    "entry-topbar must stay single-line on desktop"
  );
  assert.ok(
    cssSrc.includes("width: 66px;\n  min-width: 66px;"),
    "save indicator must keep fixed width"
  );
  assert.ok(cssSrc.includes("color-scheme: dark;"));
  assert.ok(cssSrc.includes("option {"));
});

console.log(`\nALL ${passed} UI INTEGRATION TESTS PASSED!`);
