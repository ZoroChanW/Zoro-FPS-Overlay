//! Raw Win32 declarations (no extra crates needed).
use std::ffi::c_void;

// Win32 constants
pub(crate) const WS_POPUP: u32 = 0x8000_0000;
pub(crate) const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub(crate) const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub(crate) const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub(crate) const WS_EX_LAYERED: u32 = 0x0008_0000;
pub(crate) const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub(crate) const WM_PAINT: u32 = 0x000F;
pub(crate) const WM_DESTROY: u32 = 0x0002;
pub(crate) const WM_TIMER: u32 = 0x0113;
pub(crate) const WM_HOTKEY: u32 = 0x0312;
pub(crate) const WM_NCHITTEST: u32 = 0x0084;
pub(crate) const HTTRANSPARENT: isize = -1;
pub(crate) const SW_HIDE: i32 = 0;
pub(crate) const SW_SHOWNOACTIVATE: i32 = 4;
pub(crate) const SWP_NOACTIVATE: u32 = 0x0010;
pub(crate) const MOD_NOREPEAT: u32 = 0x4000;
pub(crate) const VK_F8: u32 = 0x77;
pub(crate) const VK_F10: u32 = 0x79;
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub(crate) const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
pub(crate) const SYNCHRONIZE: u32 = 0x0010_0000;
pub(crate) const WAIT_TIMEOUT_RESULT: u32 = 258;
pub(crate) const TIMER_ID: usize = 1;
pub(crate) const SM_CXSCREEN: i32 = 0;
pub(crate) const SM_CYSCREEN: i32 = 1;

// DrawText flags
pub(crate) const DT_LEFT: u32 = 0x0000;
pub(crate) const DT_CENTER: u32 = 0x0001;
pub(crate) const DT_RIGHT: u32 = 0x0002;
pub(crate) const DT_SINGLELINE: u32 = 0x0020;
pub(crate) const DT_NOCLIP: u32 = 0x0100;
pub(crate) const DT_NOPREFIX: u32 = 0x0800;


// PDH (performance counters) used for the GPU usage reading.
pub(crate) const PDH_FMT_DOUBLE: u32 = 0x0000_0200;
pub(crate) const PDH_MORE_DATA: u32 = 0x8000_07D2;

// Win32 aliases and structures. Using raw FFI keeps this project dependency-free.
pub(crate) type BOOL = i32;
pub(crate) type BYTE = u8;
pub(crate) type WORD = u16;
pub(crate) type DWORD = u32;
pub(crate) type UINT = u32;
pub(crate) type WPARAM = usize;
pub(crate) type LPARAM = isize;
pub(crate) type LRESULT = isize;
pub(crate) type HANDLE = *mut c_void;
pub(crate) type HWND = HANDLE;
pub(crate) type HINSTANCE = HANDLE;
pub(crate) type HICON = HANDLE;
pub(crate) type HCURSOR = HANDLE;
pub(crate) type HBRUSH = HANDLE;
pub(crate) type HGDIOBJ = HANDLE;
pub(crate) type HDC = HANDLE;
pub(crate) type HFONT = HANDLE;
pub(crate) type LPCWSTR = *const u16;
pub(crate) type LPWSTR = *mut u16;
pub(crate) type WNDPROC = Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>;

#[repr(C)]
pub(crate) struct POINT { pub(crate) x: i32, pub(crate) y: i32 }
#[repr(C)]
pub(crate) struct RECT { pub(crate) left: i32, pub(crate) top: i32, pub(crate) right: i32, pub(crate) bottom: i32 }
#[repr(C)]
pub(crate) struct SIZE { pub(crate) cx: i32, pub(crate) cy: i32 }
#[repr(C)]
pub(crate) struct MSG { pub(crate) hwnd: HWND, pub(crate) message: UINT, pub(crate) wParam: WPARAM, pub(crate) lParam: LPARAM, pub(crate) time: DWORD, pub(crate) pt: POINT, pub(crate) lPrivate: DWORD }
#[repr(C)]
pub(crate) struct PAINTSTRUCT {
    pub(crate) hdc: HDC,
    pub(crate) fErase: BOOL,
    pub(crate) rcPaint: RECT,
    pub(crate) fRestore: BOOL,
    pub(crate) fIncUpdate: BOOL,
    pub(crate) rgbReserved: [BYTE; 32],
}
#[repr(C)]
pub(crate) struct WNDCLASSEXW {
    pub(crate) cbSize: UINT,
    pub(crate) style: UINT,
    pub(crate) lpfnWndProc: WNDPROC,
    pub(crate) cbClsExtra: i32,
    pub(crate) cbWndExtra: i32,
    pub(crate) hInstance: HINSTANCE,
    pub(crate) hIcon: HICON,
    pub(crate) hCursor: HCURSOR,
    pub(crate) hbrBackground: HBRUSH,
    pub(crate) lpszMenuName: LPCWSTR,
    pub(crate) lpszClassName: LPCWSTR,
    pub(crate) hIconSm: HICON,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FILETIME { pub(crate) low: u32, pub(crate) high: u32 }
#[repr(C)]
pub(crate) struct MEMORYSTATUSEX {
    pub(crate) dwLength: u32,
    pub(crate) dwMemoryLoad: u32,
    pub(crate) ullTotalPhys: u64,
    pub(crate) ullAvailPhys: u64,
    pub(crate) ullTotalPageFile: u64,
    pub(crate) ullAvailPageFile: u64,
    pub(crate) ullTotalVirtual: u64,
    pub(crate) ullAvailVirtual: u64,
    pub(crate) ullAvailExtendedVirtual: u64,
}
// PDH_FMT_COUNTERVALUE_ITEM_W with the double member of the value union.
#[repr(C)]
pub(crate) struct PdhCounterValueItem { pub(crate) name: *const u16, pub(crate) status: DWORD, pub(crate) double_value: f64 }

#[repr(C)]
pub(crate) struct BITMAPINFOHEADER {
    pub(crate) bi_size: u32, pub(crate) bi_width: i32, pub(crate) bi_height: i32,
    pub(crate) bi_planes: u16, pub(crate) bi_bit_count: u16, pub(crate) bi_compression: u32,
    pub(crate) bi_size_image: u32, pub(crate) bi_x_pels: i32, pub(crate) bi_y_pels: i32,
    pub(crate) bi_clr_used: u32, pub(crate) bi_clr_important: u32,
}
#[repr(C)]
pub(crate) struct BITMAPINFO { pub(crate) header: BITMAPINFOHEADER, pub(crate) colors: [u32; 1] }
#[repr(C)]
pub(crate) struct BLENDFUNCTION {
    pub(crate) blend_op: u8, pub(crate) blend_flags: u8,
    pub(crate) source_constant_alpha: u8, pub(crate) alpha_format: u8,
}
pub(crate) const ULW_ALPHA: u32 = 0x0000_0002;
pub(crate) const AC_SRC_ALPHA: u8 = 0x01;
pub(crate) const DIB_RGB_COLORS: u32 = 0;
pub(crate) type HBITMAP = HANDLE;

#[link(name = "pdh")]
extern "system" {
    pub(crate) fn PdhOpenQueryW(data_source: LPCWSTR, user_data: usize, query: *mut HANDLE) -> i32;
    pub(crate) fn PdhAddEnglishCounterW(query: HANDLE, counter_path: LPCWSTR, user_data: usize, counter: *mut HANDLE) -> i32;
    pub(crate) fn PdhCollectQueryData(query: HANDLE) -> i32;
    pub(crate) fn PdhGetFormattedCounterArrayW(counter: HANDLE, format: DWORD, buffer_size: *mut DWORD, item_count: *mut DWORD, items: *mut c_void) -> i32;
    pub(crate) fn PdhCloseQuery(query: HANDLE) -> i32;
}

#[link(name = "user32")]
extern "system" {
    pub(crate) fn SetProcessDpiAwarenessContext(value: HANDLE) -> BOOL;
    pub(crate) fn RegisterClassExW(wnd_class: *const WNDCLASSEXW) -> WORD;
    pub(crate) fn FindWindowW(class_name: LPCWSTR, window_name: LPCWSTR) -> HWND;
    pub(crate) fn GetSystemMetrics(index: i32) -> i32;
    pub(crate) fn CreateWindowExW(ex_style: DWORD, class_name: LPCWSTR, window_name: LPCWSTR,
        style: DWORD, x: i32, y: i32, width: i32, height: i32,
        parent: HWND, menu: HANDLE, instance: HINSTANCE, param: *mut c_void) -> HWND;
    pub(crate) fn DefWindowProcW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub(crate) fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
    pub(crate) fn SetTimer(hwnd: HWND, id: usize, interval_ms: UINT, callback: Option<unsafe extern "system" fn(HWND, UINT, usize, DWORD)>) -> usize;
    pub(crate) fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: UINT, max: UINT) -> BOOL;
    pub(crate) fn TranslateMessage(msg: *const MSG) -> BOOL;
    pub(crate) fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub(crate) fn PostQuitMessage(exit_code: i32);
    pub(crate) fn GetForegroundWindow() -> HWND;
    pub(crate) fn GetWindowThreadProcessId(hwnd: HWND, process_id: *mut DWORD) -> DWORD;
    pub(crate) fn RegisterHotKey(hwnd: HWND, id: i32, modifiers: UINT, key: UINT) -> BOOL;
    pub(crate) fn UnregisterHotKey(hwnd: HWND, id: i32) -> BOOL;
    pub(crate) fn SetLayeredWindowAttributes(hwnd: HWND, color_key: DWORD, alpha: BYTE, flags: DWORD) -> BOOL;
    pub(crate) fn SetWindowPos(hwnd: HWND, insert_after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: UINT) -> BOOL;
    pub(crate) fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HDC;
    pub(crate) fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> BOOL;
    pub(crate) fn UpdateLayeredWindow(hwnd: HWND, dst_dc: HDC, dst_pos: *const POINT, size: *const SIZE,
        src_dc: HDC, src_pos: *const POINT, color_key: DWORD, blend: *const BLENDFUNCTION, flags: DWORD) -> BOOL;
    pub(crate) fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub(crate) fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    pub(crate) fn LoadCursorW(instance: HINSTANCE, cursor_name: LPCWSTR) -> HCURSOR;
    pub(crate) fn MessageBoxW(hwnd: HWND, text: LPCWSTR, caption: LPCWSTR, kind: UINT) -> i32;
    pub(crate) fn GetDC(hwnd: HWND) -> HDC;
    pub(crate) fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
    pub(crate) fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub(crate) fn DrawTextW(hdc: HDC, text: LPCWSTR, count: i32, rect: *mut RECT, format: UINT) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub(crate) fn GetModuleHandleW(module_name: LPCWSTR) -> HINSTANCE;
    pub(crate) fn GetCurrentProcessId() -> DWORD;
    pub(crate) fn OpenProcess(access: DWORD, inherit: BOOL, process_id: DWORD) -> HANDLE;
    pub(crate) fn QueryFullProcessImageNameW(process: HANDLE, flags: DWORD, image_name: LPWSTR, size: *mut DWORD) -> BOOL;
    pub(crate) fn WaitForSingleObject(handle: HANDLE, milliseconds: DWORD) -> DWORD;
    pub(crate) fn CloseHandle(handle: HANDLE) -> BOOL;
    pub(crate) fn GetSystemTimes(idle: *mut FILETIME, kernel: *mut FILETIME, user: *mut FILETIME) -> BOOL;
    pub(crate) fn GlobalMemoryStatusEx(buffer: *mut MEMORYSTATUSEX) -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    pub(crate) fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    pub(crate) fn SetTextColor(hdc: HDC, color: DWORD) -> DWORD;
    pub(crate) fn CreateFontW(height: i32, width: i32, escapement: i32, orientation: i32,
        weight: i32, italic: DWORD, underline: DWORD, strike_out: DWORD,
        charset: DWORD, output_precision: DWORD, clip_precision: DWORD,
        quality: DWORD, pitch_and_family: DWORD, face_name: LPCWSTR) -> HFONT;
    pub(crate) fn SelectObject(hdc: HDC, object: HGDIOBJ) -> HGDIOBJ;
    pub(crate) fn CreateCompatibleDC(hdc: HDC) -> HDC;
    pub(crate) fn DeleteDC(hdc: HDC) -> BOOL;
    pub(crate) fn CreateDIBSection(hdc: HDC, info: *const BITMAPINFO, usage: UINT, bits: *mut *mut c_void,
        section: HANDLE, offset: DWORD) -> HBITMAP;
    pub(crate) fn DeleteObject(object: HGDIOBJ) -> BOOL;
    pub(crate) fn GetStockObject(object: i32) -> HGDIOBJ;
    pub(crate) fn GetTextExtentPoint32W(hdc: HDC, text: LPCWSTR, count: i32, size: *mut SIZE) -> BOOL;
}

