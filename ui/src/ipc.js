/**
 * Tauri v2 IPC bridge and vault:// media URL resolver for Log.
 */

export function isTauriRuntime() {
  return Boolean(
    typeof window !== "undefined" &&
      (window.__TAURI__ || window.__TAURI_INTERNALS__)
  );
}

function encodePathPreservingSlashes(path) {
  return encodeURI(path).replace(/#/g, "%23").replace(/\?/g, "%3F");
}

export function resolveVaultUrl(url) {
  if (!url) return "";
  const s = String(url).trim();
  if (!s) return "";
  if (
    s.startsWith("http://") ||
    s.startsWith("https://") ||
    s.startsWith("vault://") ||
    s.startsWith("asset://") ||
    s.startsWith("data:") ||
    s.startsWith("blob:")
  ) {
    return s;
  }
  const normalized = encodePathPreservingSlashes(s.startsWith("/") ? s : `/${s}`);
  if (
    isTauriRuntime() &&
    (normalized.startsWith("/vault-media/") ||
      normalized.startsWith("/vault-thumbs/"))
  ) {
    return `vault://localhost${normalized}`;
  }
  return normalized;
}

export function bytesToBase64(bytes) {
  const uint8 =
    bytes instanceof Uint8Array
      ? bytes
      : bytes instanceof ArrayBuffer
        ? new Uint8Array(bytes)
        : new Uint8Array(bytes || []);
  const chunkSize = 0x2000;
  const parts = [];
  for (let i = 0; i < uint8.length; i += chunkSize) {
    const end = Math.min(i + chunkSize, uint8.length);
    let chunkStr = "";
    for (let j = i; j < end; j += 1) {
      chunkStr += String.fromCharCode(uint8[j]);
    }
    parts.push(chunkStr);
  }
  return btoa(parts.join(""));
}

export function blobToBase64(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result || "");
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () =>
      reject(reader.error || new Error("Failed to read file as Base64"));
    reader.readAsDataURL(blob);
  });
}

async function inputToBase64(input) {
  if (typeof Blob !== "undefined" && input instanceof Blob) {
    return blobToBase64(input);
  }
  if (input instanceof Uint8Array || input instanceof ArrayBuffer) {
    return bytesToBase64(input);
  }
  if (input && typeof input.arrayBuffer === "function") {
    return bytesToBase64(new Uint8Array(await input.arrayBuffer()));
  }
  throw new Error("Expected Blob, File, ArrayBuffer, or Uint8Array");
}

function requireId(raw, label = "id") {
  const val =
    typeof raw === "object" && raw !== null ? raw.entry_id ?? raw.id : raw;
  if (val === null || val === undefined || String(val).trim() === "") {
    throw new Error(`Missing required ${label}`);
  }
  return String(val);
}

export async function invokeCmd(cmd, args = {}) {
  const tauriInvoke =
    window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke;
  if (typeof tauriInvoke !== "function") {
    throw new Error(
      `Tauri IPC unavailable for command "${cmd}". Launch Log via the Tauri desktop app.`
    );
  }
  try {
    return await tauriInvoke(cmd, args);
  } catch (err) {
    if (err instanceof Error) throw err;
    if (typeof err === "string") throw new Error(err);
    throw new Error(err?.message || JSON.stringify(err));
  }
}

export const api = {
  getVaultStats: () => invokeCmd("get_vault_stats", {}),

  listNotebooks: () => invokeCmd("list_notebooks", {}),

  createNotebook: (req = {}) =>
    invokeCmd("create_notebook", {
      name: req.name ?? "",
      description: req.description ?? null,
      cover_asset: req.cover_asset ?? null,
      accent_color: req.accent_color ?? null,
    }),

  listEntries: (params = {}) =>
    invokeCmd("list_entries", {
      notebook_id: params.notebook_id ?? null,
      tag: params.tag ?? null,
      limit: params.limit ?? null,
      offset: params.offset ?? null,
    }),

  getEntry: (id) =>
    invokeCmd("get_entry", {
      id: requireId(id, "entry id"),
    }),

  saveEntry: (req = {}) =>
    invokeCmd("save_entry", {
      id: req.id ?? null,
      title: req.title ?? "",
      date: req.date ?? null,
      notebook_id: req.notebook_id ?? null,
      tags: Array.isArray(req.tags) ? req.tags : [],
      mood: req.mood ?? null,
      starred: Boolean(req.starred),
      weather: req.weather ?? null,
      location: req.location ?? null,
      stamps: Array.isArray(req.stamps) ? req.stamps : [],
      photos: Array.isArray(req.photos) ? req.photos : [],
      audio: Array.isArray(req.audio) ? req.audio : [],
      body: req.body ?? "",
    }),

  deleteEntry: (id) =>
    invokeCmd("delete_entry", {
      id: requireId(id, "entry id"),
    }),

  searchEntries: (query = {}) =>
    invokeCmd("search_entries", {
      text: query.text ?? "",
      notebook_id: query.notebook_id ?? null,
      tag: query.tag ?? null,
      mood: query.mood ?? null,
      starred_only: Boolean(query.starred_only),
      from_date: query.from_date ?? null,
      to_date: query.to_date ?? null,
      limit: query.limit ?? 50,
      offset: query.offset ?? 0,
    }),

  listTags: () => invokeCmd("list_tags", {}),

  getCalendarMonth: (yearOrObj, month, notebookId = null) => {
    if (typeof yearOrObj === "object" && yearOrObj !== null) {
      return invokeCmd("get_calendar_month", {
        year: yearOrObj.year,
        month: yearOrObj.month,
        notebook_id: yearOrObj.notebook_id ?? null,
      });
    }
    return invokeCmd("get_calendar_month", {
      year: yearOrObj,
      month,
      notebook_id: notebookId ?? null,
    });
  },

  getOnThisDay: (monthOrObj = null, day = null) => {
    if (typeof monthOrObj === "object" && monthOrObj !== null) {
      return invokeCmd("get_on_this_day", {
        month: monthOrObj.month ?? null,
        day: monthOrObj.day ?? null,
      });
    }
    return invokeCmd("get_on_this_day", {
      month: monthOrObj ?? null,
      day: day ?? null,
    });
  },

  getBacklinks: (entryId) =>
    invokeCmd("get_backlinks", {
      entry_id: requireId(entryId, "entry_id"),
    }),

  listTemplates: () => invokeCmd("list_templates", {}),

  getDailyContext: (params = {}) =>
    invokeCmd("get_daily_context", {
      date: params?.date ?? null,
      lat: params?.lat ?? null,
      lng: params?.lng ?? null,
    }),

  getAssetManifest: () => invokeCmd("get_asset_manifest", {}),

  importDayone: (req = {}) =>
    invokeCmd("import_dayone", {
      zip_path: req.zip_path ?? "",
      notebook_id: req.notebook_id ?? null,
      dry_run: Boolean(req.dry_run),
    }),

  importNotion: (req = {}) =>
    invokeCmd("import_notion", {
      source_path: req.source_path ?? "",
      notebook_id: req.notebook_id ?? null,
      dry_run: Boolean(req.dry_run),
    }),

  listBackups: () => invokeCmd("list_backups", {}),

  createBackup: (passOrObj, uploadGdrive = false) => {
    if (typeof passOrObj === "object" && passOrObj !== null) {
      return invokeCmd("create_backup", {
        passphrase: passOrObj.passphrase ?? "",
        upload_gdrive: Boolean(passOrObj.upload_gdrive),
      });
    }
    return invokeCmd("create_backup", {
      passphrase: passOrObj ?? "",
      upload_gdrive: Boolean(uploadGdrive),
    });
  },

  restoreBackup: (req = {}) =>
    invokeCmd("restore_backup", {
      backup_id_or_path: req.backup_id_or_path ?? "",
      passphrase: req.passphrase ?? "",
      target_vault_dir: req.target_vault_dir ?? null,
      overwrite_existing: Boolean(req.overwrite_existing),
    }),

  syncGdrive: (passOrObj, config) => {
    if (
      typeof passOrObj === "object" &&
      passOrObj !== null &&
      ("config" in passOrObj || "passphrase" in passOrObj)
    ) {
      return invokeCmd("sync_gdrive", {
        passphrase: passOrObj.passphrase ?? null,
        config: passOrObj.config,
      });
    }
    return invokeCmd("sync_gdrive", {
      passphrase: passOrObj ?? null,
      config,
    });
  },

  uploadPhoto: async (fileOrBytes, caption = null, explicitName = null) => {
    const data_base64 = await inputToBase64(fileOrBytes);
    const filename =
      explicitName ||
      (fileOrBytes && typeof fileOrBytes.name === "string" && fileOrBytes.name) ||
      "photo.jpg";
    return invokeCmd("upload_photo", {
      filename,
      data_base64,
      caption: caption ?? null,
    });
  },

  uploadAudio: async (
    blobOrBytes,
    filename = "voice-memo.wav",
    durationSecs = null
  ) => {
    const data_base64 = await inputToBase64(blobOrBytes);
    const resolvedName =
      filename ||
      (blobOrBytes &&
        typeof blobOrBytes.name === "string" &&
        blobOrBytes.name) ||
      "voice-memo.wav";
    return invokeCmd("upload_audio", {
      filename: resolvedName,
      data_base64,
      duration_secs:
        typeof durationSecs === "number" && Number.isFinite(durationSecs)
          ? durationSecs
          : null,
    });
  },
};
