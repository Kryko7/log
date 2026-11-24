#!/usr/bin/env node
/**
 * Zero-dependency ES-module verification & anti-slop guardrail for Log UI.
 *
 * Checks:
 *  1. `node --check` syntax validation on all `ui/src/*.js` modules
 *  2. Relative `import { ... } from "./..."` parity against actual exports
 *  3. `icon("...")` key existence in `ui/src/icons.js`
 *  4. 1:1 parity between `ui/src/ipc.js` commands and `src-tauri/src/main.rs` handlers
 *  5. Existence of all asset paths declared in `ui/assets/manifest.json`
 *  6. Zero Unicode emoji characters in `ui/**` and `src-tauri/**`
 */

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const uiDir = path.join(repoRoot, "ui");
const uiSrcDir = path.join(uiDir, "src");
const tauriMainPath = path.join(repoRoot, "src-tauri", "src", "main.rs");
const manifestPath = path.join(uiDir, "assets", "manifest.json");

const failures = [];

function fail(msg) {
  failures.push(msg);
}

// 1. Collect all ui/src/*.js files and run `node --check`
const jsFiles = fs
  .readdirSync(uiSrcDir)
  .filter((f) => f.endsWith(".js"))
  .sort()
  .map((f) => path.join(uiSrcDir, f));

if (jsFiles.length === 0) {
  fail("No JS files found in ui/src/");
}

for (const file of jsFiles) {
  try {
    execFileSync(process.execPath, ["--check", file], { stdio: "pipe" });
  } catch (err) {
    fail(`Syntax check failed for ${path.relative(repoRoot, file)}: ${err.stderr?.toString() || err.message}`);
  }
}

// 2. Parse exports & relative imports across ui/src/*.js
const exportsByFile = new Map();
const sourcesByFile = new Map();

for (const file of jsFiles) {
  const src = fs.readFileSync(file, "utf8");
  sourcesByFile.set(file, src);

  const exported = new Set();
  for (const m of src.matchAll(/\bexport\s+(?:async\s+)?(?:function|const|let|var|class)\s+([A-Za-z_$][\w$]*)/g)) {
    exported.add(m[1]);
  }
  for (const m of src.matchAll(/\bexport\s*\{([^}]+)\}/g)) {
    for (const part of m[1].split(",")) {
      const cleaned = part.trim();
      if (!cleaned) continue;
      const asMatch = cleaned.match(/^([A-Za-z_$][\w$]*)\s+as\s+([A-Za-z_$][\w$]*)$/);
      exported.add(asMatch ? asMatch[2] : cleaned);
    }
  }
  exportsByFile.set(file, exported);
}

for (const [file, src] of sourcesByFile.entries()) {
  const relFile = path.relative(repoRoot, file);
  for (const m of src.matchAll(/\bimport\s*\{([^}]+)\}\s*from\s*["'](\.[^"']+)["']/g)) {
    const specifiers = m[1];
    const targetRel = m[2];
    const resolved = path.resolve(path.dirname(file), targetRel);
    const targetExports = exportsByFile.get(resolved);
    if (!targetExports) {
      fail(`${relFile}: imported module "${targetRel}" not found`);
      continue;
    }
    for (const part of specifiers.split(",")) {
      const cleaned = part.trim();
      if (!cleaned) continue;
      const asMatch = cleaned.match(/^([A-Za-z_$][\w$]*)\s+as\s+[A-Za-z_$][\w$]*$/);
      const name = asMatch ? asMatch[1] : cleaned;
      if (!targetExports.has(name)) {
        fail(`${relFile}: imported symbol "${name}" not exported by ${targetRel}`);
      }
    }
  }
}

// 3. Verify icon("...") string literal keys exist in ui/src/icons.js
const iconsFile = path.join(uiSrcDir, "icons.js");
const iconsSrc = sourcesByFile.get(iconsFile) || "";
const pathsBlockMatch = iconsSrc.match(/const\s+PATHS\s*=\s*\{([\s\S]*?)\n\};/);
const iconKeys = new Set();
if (!pathsBlockMatch) {
  fail("Unable to parse PATHS map in ui/src/icons.js");
} else {
  for (const m of pathsBlockMatch[1].matchAll(/^\s*([A-Za-z_$][\w$]*)\s*:/gm)) {
    iconKeys.add(m[1]);
  }
}

for (const [file, src] of sourcesByFile.entries()) {
  const relFile = path.relative(repoRoot, file);
  for (const m of src.matchAll(/\bicon\(\s*["']([A-Za-z0-9_-]+)["']/g)) {
    if (!iconKeys.has(m[1])) {
      fail(`${relFile}: unknown icon key "${m[1]}" (not in ui/src/icons.js PATHS)`);
    }
  }
}

// 4. Verify 1:1 parity between ui/src/ipc.js invokeCmd(...) and src-tauri/src/main.rs handlers
const ipcFile = path.join(uiSrcDir, "ipc.js");
const ipcSrc = sourcesByFile.get(ipcFile) || "";
const ipcCommands = new Set();
for (const m of ipcSrc.matchAll(/\binvokeCmd\(\s*["']([a-z0-9_]+)["']/g)) {
  ipcCommands.add(m[1]);
}

if (!fs.existsSync(tauriMainPath)) {
  fail("Missing src-tauri/src/main.rs");
} else {
  const rustSrc = fs.readFileSync(tauriMainPath, "utf8");
  const tauriCmdDefs = new Set();
  for (const m of rustSrc.matchAll(/#\[tauri::command[^\]]*\]\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g)) {
    tauriCmdDefs.add(m[1]);
  }
  const genMatch = rustSrc.match(/tauri::generate_handler!\s*\[([\s\S]*?)\]/);
  const tauriRegistered = new Set();
  if (!genMatch) {
    fail("Missing tauri::generate_handler![...] in src-tauri/src/main.rs");
  } else {
    for (const token of genMatch[1].split(",")) {
      const name = token.trim().replace(/\/\/.*$/gm, "").trim();
      if (name) tauriRegistered.add(name);
    }
  }

  for (const cmd of ipcCommands) {
    if (!tauriCmdDefs.has(cmd)) {
      fail(`IPC command "${cmd}" in ui/src/ipc.js is missing #[tauri::command] fn in src-tauri/src/main.rs`);
    }
    if (!tauriRegistered.has(cmd)) {
      fail(`IPC command "${cmd}" in ui/src/ipc.js is missing from tauri::generate_handler![]`);
    }
  }
  for (const cmd of tauriRegistered) {
    if (!ipcCommands.has(cmd)) {
      fail(`Tauri handler "${cmd}" in src-tauri/src/main.rs is not wrapped in ui/src/ipc.js`);
    }
  }
}

// 5. Verify all paths in ui/assets/manifest.json exist under ui/
if (!fs.existsSync(manifestPath)) {
  fail("Missing ui/assets/manifest.json");
} else {
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  for (const group of ["covers", "stamps", "stickers", "washi"]) {
    const items = Array.isArray(manifest[group]) ? manifest[group] : [];
    for (const item of items) {
      const rel = String(item.path || "")
        .replace(/[?#].*$/, "")
        .replace(/^\/+/, "");
      const fullPath = path.join(uiDir, rel);
      if (!rel || !fs.existsSync(fullPath)) {
        fail(`Manifest asset missing on disk (${group}/${item.id}): ${item.path}`);
      }
    }
  }
}

// 6. Zero-emoji scan across ui/** and src-tauri/**
const emojiRegex = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{26FF}\u{2700}-\u{27BF}]/u;
const textExts = new Set([".js", ".mjs", ".css", ".html", ".svg", ".json", ".rs", ".toml"]);

function walk(dir) {
  const results = [];
  if (!fs.existsSync(dir)) return results;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "gen" || entry.name === "target" || entry.name === "node_modules") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...walk(full));
    } else if (textExts.has(path.extname(entry.name).toLowerCase())) {
      results.push(full);
    }
  }
  return results;
}

for (const file of [...walk(uiDir), ...walk(path.join(repoRoot, "src-tauri"))]) {
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
  lines.forEach((line, idx) => {
    const match = line.match(emojiRegex);
    if (match) {
      fail(`Emoji "${match[0]}" found at ${path.relative(repoRoot, file)}:${idx + 1}`);
    }
  });
}

if (failures.length > 0) {
  console.error("UI / Tauri guardrail check FAILED:");
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
}

console.log(
  `check-ui OK: ${jsFiles.length} JS modules checked, ${iconKeys.size} icons, ${ipcCommands.size} IPC commands matched 1:1, manifest assets & 0-emoji verified.`
);
