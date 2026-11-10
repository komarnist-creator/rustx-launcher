//! Системные мелочи: пути, оперативная память, ярлык, самоустановка.
use std::path::{Path, PathBuf};
use std::process::Command;

pub const OS_NAME: &str = if cfg!(windows) {
    "windows"
} else if cfg!(target_os = "macos") {
    "osx"
} else {
    "linux"
};

pub const OS_ARCH: &str = if cfg!(target_pointer_width = "64") { "x86_64" } else { "x86" };

pub const RUNTIME_PLATFORM: &str = if cfg!(windows) {
    if cfg!(target_pointer_width = "64") { "windows-x64" } else { "windows-x86" }
} else if cfg!(target_os = "macos") {
    "mac-os"
} else {
    "linux"
};

pub const NO_WINDOW: u32 = 0x0800_0000; // CREATE_NO_WINDOW

pub fn mc_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".minecraft")
}

pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("rustx")
}

pub fn app_exe() -> PathBuf {
    app_dir().join("rustx.exe")
}

/// (всего МБ, свободно МБ). На Windows это читает маленький C-модуль из папки native/
#[cfg(windows)]
pub fn ram_info() -> Option<(u64, u64)> {
    extern "C" {
        fn rustx_ram_mb(total: *mut u64, avail: *mut u64) -> i32;
    }
    let (mut total, mut avail) = (0u64, 0u64);
    let ok = unsafe { rustx_ram_mb(&mut total, &mut avail) };
    if ok == 0 {
        return None;
    }
    Some((total, avail))
}

#[cfg(not(windows))]
pub fn ram_info() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let grab = |key: &str| -> Option<u64> {
        s.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|n| n.parse::<u64>().ok())
            .map(|kb| kb / 1024)
    };
    Some((grab("MemTotal:")?, grab("MemAvailable:")?))
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

/// Копирует exe в постоянную папку, чтобы скачанный файл можно было удалить.
#[cfg(windows)]
pub fn install_self() -> bool {
    let run = || -> std::io::Result<bool> {
        let cur = std::env::current_exe()?;
        let dst = app_exe();
        std::fs::create_dir_all(app_dir())?;
        if same_path(&cur, &dst) {
            return Ok(false);
        }
        std::fs::copy(&cur, &dst)?;
        Ok(true)
    };
    run().unwrap_or(false)
}

#[cfg(not(windows))]
pub fn install_self() -> bool {
    let _ = same_path;
    false
}

/// Ярлык rustx на рабочем столе (только Windows).
#[cfg(windows)]
pub fn create_shortcut() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    let target = if app_exe().exists() {
        app_exe()
    } else {
        match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return false,
        }
    };
    let t = target.to_string_lossy().replace('\'', "''");
    let wd = target
        .parent()
        .map(|p| p.to_string_lossy().replace('\'', "''"))
        .unwrap_or_default();
    let ps = format!(
        "$d=[Environment]::GetFolderPath('Desktop');$s=(New-Object -ComObject WScript.Shell).CreateShortcut($d+'\\rustx.lnk');$s.TargetPath='{t}';$s.WorkingDirectory='{wd}';$s.IconLocation='{t},0';$s.Save()"
    );
    Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .creation_flags(NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn create_shortcut() -> bool {
    false
}

/// Окно "Открыть файл" для выбора картинки фона. Возвращает путь или None.
#[cfg(windows)]
pub fn pick_image() -> Option<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    let script = "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;\
Add-Type -AssemblyName System.Windows.Forms;\
$o=New-Object System.Windows.Forms.Form;$o.TopMost=$true;$o.ShowInTaskbar=$false;$o.Opacity=0;$o.StartPosition='CenterScreen';$o.Show();\
$f=New-Object System.Windows.Forms.OpenFileDialog;$f.Title='Выберите картинку для фона';\
$f.Filter='Картинки (png, jpg)|*.png;*.jpg;*.jpeg|Все файлы|*.*';\
if($f.ShowDialog($o) -eq 'OK'){Write-Output $f.FileName};$o.Close()";
    let out = Command::new("powershell")
        .args(["-NoProfile", "-STA", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(not(windows))]
pub fn pick_image() -> Option<String> {
    None
}

pub fn open_dir(p: &Path) {
    let _ = std::fs::create_dir_all(p);
    #[cfg(windows)]
    {
        let _ = Command::new("explorer").arg(p).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("xdg-open").arg(p).spawn();
    }
}
