//! Windows Application User Model ID + relaunch properties so the shell can
//! pin a stable taskbar entry (not only Start).

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

/// Must match the Start Menu shortcut's System.AppUserModel.ID property.
pub const APP_USER_MODEL_ID: &str = "KeithBenefield.GrokBrowser";

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Call before creating any windows.
pub fn set_process_app_id() {
    #[link(name = "shell32")]
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
    }

    let wide = to_wide(APP_USER_MODEL_ID);
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
    }
}

/// After the HWND exists, attach AUMID + relaunch info so taskbar pin works.
///
/// `hwnd` is the value from `WindowExtWindows::hwnd()` (isize).
pub fn set_window_relaunch_props(hwnd: isize, exe_path: &std::path::Path) {
    if hwnd == 0 {
        return;
    }

    let exe = exe_path
        .canonicalize()
        .unwrap_or_else(|_| exe_path.to_path_buf());
    let exe_str = strip_unc_prefix(&exe);

    let relaunch_cmd = if exe_str.contains(' ') {
        format!("\"{exe_str}\"")
    } else {
        exe_str.clone()
    };
    let icon_resource = format!("{exe_str},0");

    unsafe {
        set_aumid_props(
            hwnd as *mut core::ffi::c_void,
            APP_USER_MODEL_ID,
            &relaunch_cmd,
            &icon_resource,
            "Grok",
        );
    }
}

fn strip_unc_prefix(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// Best-effort path to the running exe (for relaunch / shortcuts).
pub fn current_exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Grok.exe"))
}

// ---- COM property store (minimal, no extra crates) ----

#[repr(C)]
struct PropertyKey {
    fmtid: Guid,
    pid: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

// {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3} System.AppUserModel
const FMTID_APPUSERMODEL: Guid = Guid {
    data1: 0x9F4C_2855,
    data2: 0x9F79,
    data3: 0x4B39,
    data4: [0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3],
};

/// PROPVARIANT layout on Windows x64 (24 bytes).
#[repr(C)]
struct PropVariant {
    vt: u16,
    w_reserved1: u16,
    w_reserved2: u16,
    w_reserved3: u16,
    psz_val: *mut u16,
    _pad: usize,
}

const VT_LPWSTR: u16 = 31;

#[repr(C)]
struct IPropertyStoreVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut IPropertyStore) -> u32,
    get_count: usize,
    get_at: usize,
    get_value: usize,
    set_value: unsafe extern "system" fn(
        *mut IPropertyStore,
        *const PropertyKey,
        *const PropVariant,
    ) -> i32,
    commit: unsafe extern "system" fn(*mut IPropertyStore) -> i32,
}

#[repr(C)]
struct IPropertyStore {
    lp_vtbl: *const IPropertyStoreVtbl,
}

// {886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99}
const IID_IPROPERTY_STORE: Guid = Guid {
    data1: 0x886D_8EEB,
    data2: 0x8CF2,
    data3: 0x4446,
    data4: [0x8D, 0x02, 0xCD, 0xBA, 0x1D, 0xBD, 0xCF, 0x99],
};

#[link(name = "shell32")]
extern "system" {
    fn SHGetPropertyStoreForWindow(
        hwnd: *mut core::ffi::c_void,
        riid: *const Guid,
        ppv: *mut *mut IPropertyStore,
    ) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    fn PropVariantClear(pvar: *mut PropVariant) -> i32;
    fn CoTaskMemAlloc(cb: usize) -> *mut u8;
}

unsafe fn prop_variant_from_string(s: &str) -> PropVariant {
    let wide = to_wide(s);
    let byte_len = wide.len() * 2;
    let mem = CoTaskMemAlloc(byte_len);
    if !mem.is_null() {
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, mem, byte_len);
    }
    PropVariant {
        vt: VT_LPWSTR,
        w_reserved1: 0,
        w_reserved2: 0,
        w_reserved3: 0,
        psz_val: mem as *mut u16,
        _pad: 0,
    }
}

unsafe fn set_prop(store: *mut IPropertyStore, pid: u32, value: &str) {
    let key = PropertyKey {
        fmtid: FMTID_APPUSERMODEL,
        pid,
    };
    let mut pv = prop_variant_from_string(value);
    let vtbl = &*(*store).lp_vtbl;
    let _ = (vtbl.set_value)(store, &key, &pv);
    PropVariantClear(&mut pv);
}

unsafe fn set_aumid_props(
    hwnd: *mut core::ffi::c_void,
    aumid: &str,
    relaunch_cmd: &str,
    icon_resource: &str,
    display_name: &str,
) {
    let mut store: *mut IPropertyStore = std::ptr::null_mut();
    let hr = SHGetPropertyStoreForWindow(hwnd, &IID_IPROPERTY_STORE, &mut store);
    if hr < 0 || store.is_null() {
        return;
    }

    // pid: 2 RelaunchCommand, 3 Icon, 4 DisplayName, 5 ID
    set_prop(store, 5, aumid);
    set_prop(store, 2, relaunch_cmd);
    set_prop(store, 3, icon_resource);
    set_prop(store, 4, display_name);

    let vtbl = &*(*store).lp_vtbl;
    let _ = (vtbl.commit)(store);
    let _ = (vtbl.release)(store);
}
