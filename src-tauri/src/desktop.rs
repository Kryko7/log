//! Freedesktop `.desktop` launcher, hicolor icon, and binary installer for Linux.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use serde::Serialize;

const APP_ID: &str = "dev.logjournal.app";
const BIN_NAME: &str = "log-journal";

const ICON_32: &[u8] = include_bytes!("../icons/32x32.png");
const ICON_128: &[u8] = include_bytes!("../icons/128x128.png");
const ICON_256: &[u8] = include_bytes!("../icons/128x128@2x.png");
const ICON_512: &[u8] = include_bytes!("../icons/icon.png");

const HICOLOR_ICONS: [(&str, &[u8]); 4] = [
    ("32x32", ICON_32),
    ("128x128", ICON_128),
    ("256x256", ICON_256),
    ("512x512", ICON_512),
];

/// Summary emitted by `log-journal install-desktop`.
#[derive(Debug, Serialize)]
pub struct DesktopInstallReport {
    pub binary_path: String,
    pub desktop_file: String,
    pub icon_files: Vec<String>,
}

/// Summary emitted by `log-journal uninstall-desktop`.
#[derive(Debug, Serialize)]
pub struct DesktopUninstallReport {
    pub binary_path: String,
    pub desktop_file: String,
    pub icon_files: Vec<String>,
    pub removed_files: Vec<String>,
}

fn resolve_xdg_dir(
    override_dir: Option<PathBuf>,
    env_var: &str,
    fallback_rel: &str,
) -> Result<PathBuf> {
    if let Some(dir) = override_dir {
        return Ok(dir);
    }
    if let Some(val) = std::env::var_os(env_var).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(val));
    }
    let home = dirs::home_dir()
        .context("could not determine user home directory for XDG install paths")?;
    Ok(home.join(fallback_rel))
}

pub(crate) fn format_desktop_exec(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let needs_quotes = raw.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '\\'
                    | '$'
                    | '`'
                    | '>'
                    | '<'
                    | '~'
                    | '|'
                    | '&'
                    | ';'
                    | '*'
                    | '?'
                    | '#'
                    | '('
                    | ')'
            )
    });
    if !needs_quotes {
        return raw.replace('%', "%%");
    }
    let mut escaped = String::with_capacity(raw.len() + 4);
    escaped.push('"');
    for ch in raw.chars() {
        if ch == '%' {
            escaped.push_str("%%");
            continue;
        }
        if matches!(ch, '"' | '\\' | '$' | '`') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped.push('"');
    escaped
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn copy_binary_atomically(source: &Path, target: &Path) -> Result<()> {
    let parent = target
        .parent()
        .context("target binary path must have a parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create bin directory {}", parent.display()))?;

    let tmp_name = format!(".{BIN_NAME}.tmp.{}", std::process::id());
    let tmp_path = parent.join(tmp_name);

    let copy_res = (|| -> Result<()> {
        fs::copy(source, &tmp_path).with_context(|| {
            format!(
                "failed to copy executable from {} to {}",
                source.display(),
                tmp_path.display()
            )
        })?;
        set_mode(&tmp_path, 0o755)?;
        fs::rename(&tmp_path, target).with_context(|| {
            format!(
                "failed to move {} into {}",
                tmp_path.display(),
                target.display()
            )
        })?;
        Ok(())
    })();

    if copy_res.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    copy_res
}

fn refresh_desktop_caches(apps_dir: &Path, hicolor_dir: &Path) {
    let _ = Command::new("update-desktop-database")
        .arg(apps_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let _ = Command::new("gtk-update-icon-cache")
        .args(["-q", "-t", "-f"])
        .arg(hicolor_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn remove_if_exists(path: &Path, removed: &mut Vec<String>) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {
            removed.push(path.to_string_lossy().into_owned());
            Ok(())
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("failed to remove {}", path.display())),
    }
}

/// Install the `log-journal` binary, hicolor icons, and `.desktop` launcher entry.
pub fn install_desktop(
    bin_dir_override: Option<PathBuf>,
    data_dir_override: Option<PathBuf>,
) -> Result<DesktopInstallReport> {
    let bin_dir = resolve_xdg_dir(bin_dir_override, "XDG_BIN_HOME", ".local/bin")?;
    let data_dir = resolve_xdg_dir(data_dir_override, "XDG_DATA_HOME", ".local/share")?;

    let current_exe =
        std::env::current_exe().context("failed to locate running log-journal executable")?;
    let target_bin = bin_dir.join(BIN_NAME);
    copy_binary_atomically(&current_exe, &target_bin)?;

    let hicolor_dir = data_dir.join("icons/hicolor");
    let mut icon_files = Vec::with_capacity(HICOLOR_ICONS.len());
    for (size, bytes) in HICOLOR_ICONS {
        let icon_dir = hicolor_dir.join(size).join("apps");
        fs::create_dir_all(&icon_dir)
            .with_context(|| format!("failed to create icon directory {}", icon_dir.display()))?;
        let icon_path = icon_dir.join(format!("{APP_ID}.png"));
        fs::write(&icon_path, bytes)
            .with_context(|| format!("failed to write icon {}", icon_path.display()))?;
        set_mode(&icon_path, 0o644)?;
        icon_files.push(icon_path.to_string_lossy().into_owned());
    }

    let apps_dir = data_dir.join("applications");
    fs::create_dir_all(&apps_dir)
        .with_context(|| format!("failed to create applications dir {}", apps_dir.display()))?;
    let desktop_file = apps_dir.join(format!("{APP_ID}.desktop"));
    let exec_line = format_desktop_exec(&target_bin);
    let try_exec = target_bin.to_string_lossy();
    let desktop_contents = format!(
        "[Desktop Entry]\n\
         Version=1.0\n\
         Type=Application\n\
         Name=Log\n\
         GenericName=Personal Journal\n\
         Comment=Local-first personal Markdown journal\n\
         Exec={exec_line}\n\
         TryExec={try_exec}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Utility;TextEditor;\n\
         Keywords=journal;diary;markdown;notes;writing;\n\
         StartupNotify=true\n\
         StartupWMClass={APP_ID}\n"
    );
    fs::write(&desktop_file, desktop_contents)
        .with_context(|| format!("failed to write {}", desktop_file.display()))?;
    set_mode(&desktop_file, 0o644)?;

    refresh_desktop_caches(&apps_dir, &hicolor_dir);

    Ok(DesktopInstallReport {
        binary_path: target_bin.to_string_lossy().into_owned(),
        desktop_file: desktop_file.to_string_lossy().into_owned(),
        icon_files,
    })
}

/// Uninstall the `log-journal` binary, hicolor icons, and `.desktop` launcher entry.
pub fn uninstall_desktop(
    bin_dir_override: Option<PathBuf>,
    data_dir_override: Option<PathBuf>,
) -> Result<DesktopUninstallReport> {
    let bin_dir = resolve_xdg_dir(bin_dir_override, "XDG_BIN_HOME", ".local/bin")?;
    let data_dir = resolve_xdg_dir(data_dir_override, "XDG_DATA_HOME", ".local/share")?;

    let target_bin = bin_dir.join(BIN_NAME);
    let apps_dir = data_dir.join("applications");
    let desktop_file = apps_dir.join(format!("{APP_ID}.desktop"));
    let hicolor_dir = data_dir.join("icons/hicolor");

    let mut removed_files = Vec::new();
    let mut icon_files = Vec::with_capacity(HICOLOR_ICONS.len());

    remove_if_exists(&desktop_file, &mut removed_files)?;
    for (size, _) in HICOLOR_ICONS {
        let icon_path = hicolor_dir
            .join(size)
            .join("apps")
            .join(format!("{APP_ID}.png"));
        icon_files.push(icon_path.to_string_lossy().into_owned());
        remove_if_exists(&icon_path, &mut removed_files)?;
    }
    remove_if_exists(&target_bin, &mut removed_files)?;

    refresh_desktop_caches(&apps_dir, &hicolor_dir);

    Ok(DesktopUninstallReport {
        binary_path: target_bin.to_string_lossy().into_owned(),
        desktop_file: desktop_file.to_string_lossy().into_owned(),
        icon_files,
        removed_files,
    })
}
