import { resolveVaultUrl } from "./ipc.js";
import { icon } from "./icons.js";

function isSafeHref(href) {
  if (!href) return false;
  const s = String(href).trim();
  if (!s) return false;
  if (s.startsWith("/") || s.startsWith("#")) return true;
  try {
    const u = new URL(s, "https://example.invalid");
    return (
      u.protocol === "http:" ||
      u.protocol === "https:" ||
      u.protocol === "mailto:"
    );
  } catch {
    return false;
  }
}

function isSafeImageSrc(rawSrc) {
  if (!rawSrc) return false;
  const s = String(rawSrc).trim();
  if (!s) return false;
  if (s.startsWith("/vault-media/") || s.startsWith("/vault-thumbs/") || s.startsWith("/assets/")) {
    return true;
  }
  if (/^data:image\/(png|jpeg|jpg|webp|gif);base64,/i.test(s)) {
    return true;
  }
  if (s.startsWith("/")) return true;
  try {
    const u = new URL(s, "https://example.invalid");
    return (
      u.protocol === "http:" ||
      u.protocol === "https:" ||
      u.protocol === "vault:" ||
      u.protocol === "asset:"
    );
  } catch {
    return false;
  }
}

function makeIconSpan(name, cls = "tiny-icon") {
  const wrap = document.createElement("span");
  wrap.className = "md-icon-wrap";
  wrap.innerHTML = icon(name, cls);
  return wrap.firstElementChild || wrap;
}

function buildEntryIndex(allEntries) {
  const byId = new Map();
  const byTitle = new Map();
  const list = [];
  if (!Array.isArray(allEntries)) {
    return { byId, byTitle, list };
  }
  for (const e of allEntries) {
    if (!e) continue;
    const idKey = String(e.id || "").toLowerCase();
    const titleKey = String(e.title || "").trim().toLowerCase();
    if (idKey && !byId.has(idKey)) byId.set(idKey, e);
    if (titleKey && !byTitle.has(titleKey)) byTitle.set(titleKey, e);
    list.push({ entry: e, titleKey });
  }
  return { byId, byTitle, list };
}

function resolveWikilinkTarget(target, entryIndex) {
  const needle = String(target || "").trim().toLowerCase();
  if (!needle || !entryIndex) return null;
  const exact = entryIndex.byId.get(needle) || entryIndex.byTitle.get(needle);
  if (exact) return exact;
  for (const item of entryIndex.list) {
    if (item.titleKey && item.titleKey.includes(needle)) {
      return item.entry;
    }
  }
  return null;
}

export function extractWikilinks(bodyText) {
  const found = [];
  const seen = new Set();
  const re = /\[\[([^[\]|]+)(?:\|([^[\]]+))?\]\]/g;
  const src = String(bodyText || "");
  let m;
  while ((m = re.exec(src)) !== null) {
    const target = m[1].trim();
    const label = (m[2] || m[1]).trim();
    const key = target.toLowerCase();
    if (target && !seen.has(key)) {
      seen.add(key);
      found.push({ target, label });
    }
  }
  return found;
}

function appendInline(parent, text, ctx) {
  const src = String(text ?? "");
  let i = 0;
  let plain = "";

  const flushPlain = () => {
    if (plain) {
      parent.appendChild(document.createTextNode(plain));
      plain = "";
    }
  };

  while (i < src.length) {
    // Inline code: `...`
    if (src[i] === "`") {
      const end = src.indexOf("`", i + 1);
      if (end > i + 1) {
        flushPlain();
        const code = document.createElement("code");
        code.className = "entry-inline-code";
        code.textContent = src.slice(i + 1, end);
        parent.appendChild(code);
        i = end + 1;
        continue;
      }
    }

    // Inline image: ![alt](url)
    if (src[i] === "!" && src[i + 1] === "[") {
      const closeBracket = src.indexOf("]", i + 2);
      if (closeBracket >= i + 2 && src[closeBracket + 1] === "(") {
        const closeParen = src.indexOf(")", closeBracket + 2);
        if (closeParen > closeBracket + 2) {
          const alt = src.slice(i + 2, closeBracket);
          const rawSrc = src.slice(closeBracket + 2, closeParen).trim();
          flushPlain();
          if (isSafeImageSrc(rawSrc)) {
            const img = document.createElement("img");
            img.src = resolveVaultUrl(rawSrc);
            img.alt = alt || "Journal figure";
            img.loading = "lazy";
            img.decoding = "async";
            img.style.maxWidth = "100%";
            img.style.maxHeight = "480px";
            img.style.borderRadius = "4px";
            img.style.border = "1px solid var(--border-subtle)";
            img.style.display = "block";
            img.style.margin = "0.75rem 0";
            parent.appendChild(img);
          } else {
            parent.appendChild(document.createTextNode(alt || "[blocked image]"));
          }
          i = closeParen + 1;
          continue;
        }
      }
    }

    // Wikilink: [[Target]] or [[Target|Label]]
    if (src[i] === "[" && src[i + 1] === "[") {
      const end = src.indexOf("]]", i + 2);
      if (end > i + 2) {
        const inner = src.slice(i + 2, end);
        if (!inner.includes("\n")) {
          flushPlain();
          const pipe = inner.indexOf("|");
          const target = (pipe >= 0 ? inner.slice(0, pipe) : inner).trim();
          const label = (pipe >= 0 ? inner.slice(pipe + 1) : inner).trim();
          const match = resolveWikilinkTarget(target, ctx.entryIndex);

          const btn = document.createElement("button");
          btn.type = "button";
          btn.className = "entry-wikilink";
          btn.title = match
            ? `Open "${match.title || target}"`
            : `Entry "${target}"`;
          btn.appendChild(makeIconSpan("link", "tiny-icon"));
          btn.appendChild(document.createTextNode(label || target));
          btn.addEventListener("click", (ev) => {
            ev.stopPropagation();
            if (typeof ctx.onOpenEntry === "function") {
              ctx.onOpenEntry(match ? match.id : null, target);
            }
          });
          parent.appendChild(btn);
          i = end + 2;
          continue;
        }
      }
    }

    // Markdown link: [label](url)
    if (src[i] === "[" && (i === 0 || src[i - 1] !== "!")) {
      const closeBracket = src.indexOf("]", i + 1);
      if (closeBracket > i + 1 && src[closeBracket + 1] === "(") {
        const closeParen = src.indexOf(")", closeBracket + 2);
        if (closeParen > closeBracket + 2) {
          const label = src.slice(i + 1, closeBracket);
          const href = src.slice(closeBracket + 2, closeParen).trim();
          flushPlain();
          if (isSafeHref(href)) {
            const a = document.createElement("a");
            a.className = "entry-external-link";
            a.href = href;
            a.target = "_blank";
            a.rel = "noopener noreferrer";
            a.textContent = label;
            parent.appendChild(a);
          } else {
            parent.appendChild(document.createTextNode(label));
          }
          i = closeParen + 1;
          continue;
        }
      }
    }

    // Bold: **...**
    if (src[i] === "*" && src[i + 1] === "*") {
      const end = src.indexOf("**", i + 2);
      if (end > i + 2) {
        flushPlain();
        const strong = document.createElement("strong");
        appendInline(strong, src.slice(i + 2, end), ctx);
        parent.appendChild(strong);
        i = end + 2;
        continue;
      }
    }

    // Italic: *...*
    if (src[i] === "*" && src[i + 1] !== "*" && src[i + 1] !== " ") {
      const end = src.indexOf("*", i + 1);
      if (end > i + 1 && src[end - 1] !== " ") {
        flushPlain();
        const em = document.createElement("em");
        appendInline(em, src.slice(i + 1, end), ctx);
        parent.appendChild(em);
        i = end + 1;
        continue;
      }
    }

    plain += src[i];
    i += 1;
  }

  flushPlain();
}

function makeFigure(alt, rawSrc) {
  if (!isSafeImageSrc(rawSrc)) {
    const p = document.createElement("p");
    p.className = "entry-paragraph muted-empty-note";
    p.textContent = alt ? `[Blocked image: ${alt}]` : "[Blocked unsafe image source]";
    return p;
  }

  const fig = document.createElement("figure");
  fig.className = "entry-inline-figure";

  const img = document.createElement("img");
  img.src = resolveVaultUrl(rawSrc);
  img.alt = alt || "Journal figure";
  img.loading = "lazy";
  img.decoding = "async";
  fig.appendChild(img);

  if (alt && alt.trim()) {
    const cap = document.createElement("figcaption");
    cap.textContent = alt.trim();
    fig.appendChild(cap);
  }
  return fig;
}

export function renderMarkdownReadView(
  container,
  bodyText,
  allEntries = [],
  onOpenEntry = null,
  onToggleTodo = null
) {
  container.innerHTML = "";
  const ctx = {
    entryIndex: buildEntryIndex(allEntries),
    onOpenEntry,
    onToggleTodo,
  };
  const text = String(bodyText || "").replace(/\r\n/g, "\n");
  const lines = text.split("\n");

  if (!text.trim()) {
    const empty = document.createElement("p");
    empty.className = "entry-paragraph muted-empty-note";
    empty.textContent = "Nothing written on this page yet.";
    container.appendChild(empty);
    return;
  }

  let i = 0;
  while (i < lines.length) {
    const raw = lines[i];
    const trimmed = raw.trim();

    if (!trimmed) {
      i += 1;
      continue;
    }

    // Fenced code block
    if (trimmed.startsWith("```")) {
      const codeLines = [];
      i += 1;
      while (i < lines.length && !lines[i].trim().startsWith("```")) {
        codeLines.push(lines[i]);
        i += 1;
      }
      if (i < lines.length) i += 1;
      const pre = document.createElement("pre");
      pre.className = "entry-inline-code";
      pre.style.display = "block";
      pre.style.padding = "0.85rem 1rem";
      pre.style.margin = "1.25rem 0";
      pre.style.whiteSpace = "pre-wrap";
      pre.textContent = codeLines.join("\n");
      container.appendChild(pre);
      continue;
    }

    // Divider
    if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) {
      const hr = document.createElement("hr");
      hr.className = "entry-section-rule";
      container.appendChild(hr);
      i += 1;
      continue;
    }

    // Standalone image: ![alt](src)
    const imgMatch = trimmed.match(/^!\[([^\]]*)\]\(([^)]+)\)$/);
    if (imgMatch) {
      container.appendChild(makeFigure(imgMatch[1], imgMatch[2].trim()));
      i += 1;
      continue;
    }

    // Headings
    const hMatch = raw.match(/^(#{1,3})\s+(.*)$/);
    if (hMatch) {
      const level = hMatch[1].length;
      const el = document.createElement(`h${level}`);
      el.className = `entry-h${level}`;
      appendInline(el, hMatch[2], ctx);
      container.appendChild(el);
      i += 1;
      continue;
    }

    // Callout: > [!NOTE] ...
    const calloutMatch = trimmed.match(/^>\s*\[!(NOTE|TIP|IMPORTANT|CALLOUT)\]\s*(.*)$/i);
    if (calloutMatch) {
      const firstLine = calloutMatch[2] || "";
      const noteLines = firstLine ? [firstLine] : [];
      i += 1;
      while (i < lines.length && /^\s*>/.test(lines[i])) {
        noteLines.push(lines[i].replace(/^\s*>\s?/, ""));
        i += 1;
      }
      const aside = document.createElement("aside");
      aside.className = "entry-callout-note";
      aside.appendChild(makeIconSpan("leaf", "log-icon"));
      const bodySpan = document.createElement("div");
      noteLines.forEach((nl, idx) => {
        if (idx > 0) bodySpan.appendChild(document.createElement("br"));
        appendInline(bodySpan, nl, ctx);
      });
      aside.appendChild(bodySpan);
      container.appendChild(aside);
      continue;
    }

    // Pull quote: > ...
    if (trimmed.startsWith(">")) {
      const quoteLines = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) {
        quoteLines.push(lines[i].replace(/^\s*>\s?/, ""));
        i += 1;
      }
      const bq = document.createElement("blockquote");
      bq.className = "entry-pull-quote";
      quoteLines.forEach((ql, idx) => {
        if (idx > 0) bq.appendChild(document.createElement("br"));
        appendInline(bq, ql, ctx);
      });
      container.appendChild(bq);
      continue;
    }

    // Checklist: - [ ] or - [x]
    const todoMatch = raw.match(/^\s*[-*]\s+\[([ xX])\]\s*(.*)$/);
    if (todoMatch) {
      const lineIdx = i;
      const done = todoMatch[1].toLowerCase() === "x";
      const row = document.createElement("label");
      row.className = `entry-checklist-row${done ? " is-done" : ""}`;

      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.checked = done;
      cb.addEventListener("change", () => {
        if (typeof ctx.onToggleTodo === "function") {
          ctx.onToggleTodo(lineIdx);
        }
      });
      row.appendChild(cb);

      const labelSpan = document.createElement("span");
      appendInline(labelSpan, todoMatch[2], ctx);
      row.appendChild(labelSpan);

      container.appendChild(row);
      i += 1;
      continue;
    }

    // Unordered list
    if (/^\s*[-*]\s+/.test(raw)) {
      const ul = document.createElement("ul");
      ul.style.margin = "0 0 1.25rem";
      ul.style.paddingLeft = "1.35rem";
      while (
        i < lines.length &&
        /^\s*[-*]\s+/.test(lines[i]) &&
        !/^\s*[-*]\s+\[[ xX]\]/.test(lines[i])
      ) {
        const li = document.createElement("li");
        li.style.marginBottom = "0.28rem";
        appendInline(li, lines[i].replace(/^\s*[-*]\s+/, ""), ctx);
        ul.appendChild(li);
        i += 1;
      }
      container.appendChild(ul);
      continue;
    }

    // Ordered list
    if (/^\s*\d+\.\s+/.test(raw)) {
      const ol = document.createElement("ol");
      ol.style.margin = "0 0 1.25rem";
      ol.style.paddingLeft = "1.35rem";
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) {
        const li = document.createElement("li");
        li.style.marginBottom = "0.28rem";
        appendInline(li, lines[i].replace(/^\s*\d+\.\s+/, ""), ctx);
        ol.appendChild(li);
        i += 1;
      }
      container.appendChild(ol);
      continue;
    }

    // Paragraph (accumulate consecutive plain lines)
    const paraLines = [];
    while (i < lines.length) {
      const cur = lines[i];
      const curTrim = cur.trim();
      if (
        !curTrim ||
        curTrim.startsWith("```") ||
        /^(-{3,}|\*{3,}|_{3,})$/.test(curTrim) ||
        /^!\[([^\]]*)\]\(([^)]+)\)$/.test(curTrim) ||
        /^#{1,3}\s+/.test(cur) ||
        curTrim.startsWith(">") ||
        /^\s*[-*]\s+/.test(cur) ||
        /^\s*\d+\.\s+/.test(cur)
      ) {
        break;
      }
      paraLines.push(cur);
      i += 1;
    }
    const p = document.createElement("p");
    p.className = "entry-paragraph";
    paraLines.forEach((pl, idx) => {
      if (idx > 0) p.appendChild(document.createElement("br"));
      appendInline(p, pl, ctx);
    });
    container.appendChild(p);
  }
}
