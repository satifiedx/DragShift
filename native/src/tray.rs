//! Tray icon, the "Open a file…" dialog and dark context menus.

use crate::i18n::t;
use crate::util::{from_wide, wide};
use std::path::PathBuf;
use windows::core::{PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
}

fn set_tip(d: &mut NOTIFYICONDATAW, enabled: bool) {
    let tip = if enabled { "DragShift".to_string() } else { format!("DragShift ({})", t("paused")) };
    let w: Vec<u16> = tip.encode_utf16().take(d.szTip.len() - 1).collect();
    d.szTip[..w.len()].copy_from_slice(&w);
    d.szTip[w.len()] = 0;
}

pub fn add(hwnd: HWND, callback: u32) {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let icon = LoadImageW(hinst, PCWSTR(1 as *const u16), IMAGE_ICON, GetSystemMetrics(SM_CXSMICON),
                              GetSystemMetrics(SM_CYSMICON), LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0)).unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());
        let mut d = data(hwnd);
        d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        d.uCallbackMessage = callback;
        d.hIcon = icon;
        set_tip(&mut d, crate::settings::get().enabled);
        if !Shell_NotifyIconW(NIM_ADD, &d).as_bool() {
            // Explorer may still be starting at logon: try again in a moment
            let _ = Shell_NotifyIconW(NIM_DELETE, &d);
            if !Shell_NotifyIconW(NIM_ADD, &d).as_bool() {
                crate::logf!("tray: could not add the icon");
            }
        }
    }
}

pub fn update_tip(hwnd: HWND, enabled: bool) {
    let mut d = data(hwnd);
    d.uFlags = NIF_TIP | NIF_SHOWTIP;
    set_tip(&mut d, enabled);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

pub fn remove(hwnd: HWND) {
    let d = data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &d);
    }
}

/// Windows' own Open dialog, several files allowed.
pub fn pick_files(owner: HWND) -> Vec<PathBuf> {
    let mut buf = vec![0u16; 32 * 1024];
    let filter: Vec<u16> = format!("{}\0*.*\0", t("All supported files")).encode_utf16().chain([0]).collect();
    let title = wide(t("Choose files for DragShift"));
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_EXPLORER | OFN_ALLOWMULTISELECT | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY,
        ..Default::default()
    };
    unsafe {
        let _ = SetForegroundWindow(owner);
        if !GetOpenFileNameW(&mut ofn).as_bool() {
            return vec![];
        }
    }
    // "dir\0a\0b\0\0" for several files, "full\path\0\0" for one
    let parts: Vec<String> = buf.split(|&c| c == 0).take_while(|s| !s.is_empty()).map(from_wide).collect();
    match parts.len() {
        0 => vec![],
        1 => vec![PathBuf::from(&parts[0])],
        _ => parts[1..].iter().map(|f| PathBuf::from(&parts[0]).join(f)).collect(),
    }
}

/// Windows' own folder picker, opened at `start` when it still exists.
pub fn pick_folder(owner: HWND, start: &str) -> Option<PathBuf> {
    use windows::Win32::System::Com::*;
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let picked = folder_dialog(owner, start);
        if init {
            CoUninitialize(); // after the dialog's COM objects are gone
        }
        picked
    }
}

unsafe fn folder_dialog(owner: HWND, start: &str) -> Option<PathBuf> {
    use windows::Win32::System::Com::*;
    {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().ok()?;
        dlg.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).ok()?;
        let title = wide(t("Where to save the screenshot"));
        let _ = dlg.SetTitle(PCWSTR(title.as_ptr()));
        if !start.is_empty() && std::path::Path::new(start).is_dir() {
            let s = wide(start);
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(s.as_ptr()), None) {
                let _ = dlg.SetFolder(&item);
            }
        }
        let _ = SetForegroundWindow(owner);
        dlg.Show(owner).ok()?; // cancelled: an error
        let name = dlg.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        path.map(PathBuf::from)
    }
}

/// Follow the system's dark mode in context menus (uxtheme's unnamed SetPreferredAppMode).
pub fn allow_dark_menus() {
    unsafe {
        let name = wide("uxtheme.dll");
        let Ok(m) = LoadLibraryW(PCWSTR(name.as_ptr())) else { return };
        if let Some(f) = GetProcAddress(m, PCSTR(135 as *const u8)) {
            let set: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set(1); // AllowDark
        }
        if let Some(f) = GetProcAddress(m, PCSTR(136 as *const u8)) {
            let flush: extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}
