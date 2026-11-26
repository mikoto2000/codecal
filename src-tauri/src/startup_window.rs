//! A Win32 spinner with its own message pump, available before WebView2 exists.
use std::{
    cell::Cell,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

const DISMISS: u32 = WM_APP + 1;
struct Control {
    hwnd: AtomicUsize,
    shown_ms: AtomicU64,
    dismissed: AtomicBool,
    cancelled: AtomicBool,
    app: Mutex<Option<tauri::AppHandle>>,
}
#[derive(Clone)]
pub(crate) struct NativeStartup(Arc<Control>);
struct PaintData {
    control: Arc<Control>,
    step: Cell<u32>,
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
impl NativeStartup {
    pub(crate) fn new(began: Instant) -> Self {
        // Match Tao's per-monitor DPI policy before the first native HWND.
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let control = Arc::new(Control {
            hwnd: AtomicUsize::new(0),
            shown_ms: AtomicU64::new(0),
            dismissed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            app: Mutex::new(None),
        });
        let thread_control = control.clone();
        let (ready, wait) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("startup-window".into())
            .spawn(move || {
                // All HWND/GDI access and PaintData lifetime are owned by this thread.
                unsafe {
                    let class = wide("CodeCalStartup");
                    let instance = GetModuleHandleW(null());
                    let wc = WNDCLASSW {
                        lpfnWndProc: Some(window_proc),
                        hInstance: instance,
                        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                        hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                        lpszClassName: class.as_ptr(),
                        ..std::mem::zeroed()
                    };
                    if RegisterClassW(&wc) == 0 {
                        let _ = ready.send(());
                        return;
                    }
                    let data = Box::new(PaintData {
                        control: thread_control.clone(),
                        step: Cell::new(0),
                    });
                    let dpi = GetDpiForSystem().max(96) as i32;
                    let width = 360 * dpi / 96;
                    let height = 180 * dpi / 96;
                    let title = wide("CodeCal - 起動中");
                    let hwnd = CreateWindowExW(
                        WS_EX_TOOLWINDOW,
                        class.as_ptr(),
                        title.as_ptr(),
                        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
                        (GetSystemMetrics(SM_CXSCREEN) - width) / 2,
                        (GetSystemMetrics(SM_CYSCREEN) - height) / 2,
                        width,
                        height,
                        null_mut(),
                        null_mut(),
                        instance,
                        (&*data as *const PaintData).cast(),
                    );
                    if hwnd.is_null() {
                        UnregisterClassW(class.as_ptr(), instance);
                        let _ = ready.send(());
                        return;
                    }
                    thread_control.hwnd.store(hwnd as usize, Ordering::SeqCst);
                    if thread_control.dismissed.load(Ordering::SeqCst) {
                        DestroyWindow(hwnd);
                    } else {
                        ShowWindow(hwnd, SW_SHOWNORMAL);
                        // STARTF_USESHOWWINDOW/SW_HIDE can override the first
                        // ShowWindow call when a launcher hides its console.
                        SetWindowPos(
                            hwnd,
                            HWND_TOP,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
                        );
                        UpdateWindow(hwnd);
                        if IsWindowVisible(hwnd) != 0 {
                            thread_control
                                .shown_ms
                                .store(began.elapsed().as_millis().max(1) as u64, Ordering::SeqCst);
                        }
                        let mut animate: i32 = 1;
                        SystemParametersInfoW(
                            SPI_GETCLIENTAREAANIMATION,
                            0,
                            (&mut animate as *mut i32).cast(),
                            0,
                        );
                        if animate != 0 {
                            SetTimer(hwnd, 1, 80, None);
                        }
                    }
                    let _ = ready.send(());
                    let mut message = std::mem::zeroed();
                    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    // Also detach user data if the message pump ended with an error.
                    if IsWindow(hwnd) != 0 {
                        DestroyWindow(hwnd);
                    }
                    // WM_NCDESTROY clears the pointer before this Box is dropped.
                    UnregisterClassW(class.as_ptr(), instance);
                }
            });
        // A failed/slow optional splash must never prevent the HTML fallback.
        let _ = wait.recv_timeout(Duration::from_millis(500));
        Self(control)
    }
    pub(crate) fn bind(&self, app: &tauri::AppHandle) -> bool {
        *self.0.app.lock().unwrap_or_else(|e| e.into_inner()) = Some(app.clone());
        if self.0.cancelled.load(Ordering::SeqCst) {
            app.exit(0);
            false
        } else {
            true
        }
    }
    pub(crate) fn shown_ms(&self) -> Option<u64> {
        let ms = self.0.shown_ms.load(Ordering::SeqCst);
        (ms > 0).then_some(ms)
    }
    pub(crate) fn close(&self) {
        self.0.dismissed.store(true, Ordering::SeqCst);
        let hwnd = self.0.hwnd.load(Ordering::SeqCst) as HWND;
        if !hwnd.is_null() {
            unsafe {
                PostMessageW(hwnd, DISMISS, 0, 0);
            }
        } else {
            // Creation can fail or finish late; do not retain the AppHandle
            // cycle when there is no WM_DESTROY callback to release it.
            self.0.app.lock().unwrap_or_else(|e| e.into_inner()).take();
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let data = (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const PaintData).as_ref();
    if let Some(data) = data {
        match message {
            WM_PAINT => {
                paint(hwnd, data.step.get());
                return 0;
            }
            WM_PRINTCLIENT => {
                draw(hwnd, w as HDC, data.step.get());
                return 0;
            }
            WM_TIMER => {
                data.step.set((data.step.get() + 1) % 12);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_CLOSE => {
                data.control.cancelled.store(true, Ordering::SeqCst);
                let app = data
                    .control
                    .app
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Some(app) = app {
                    app.exit(0);
                }
                DestroyWindow(hwnd);
                return 0;
            }
            DISMISS => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_DESTROY => {
                KillTimer(hwnd, 1);
                data.control.hwnd.store(0, Ordering::SeqCst);
                data.control
                    .app
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
                PostQuitMessage(0);
                return 0;
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            WM_DPICHANGED => {
                if let Some(rect) = (l as *const windows_sys::Win32::Foundation::RECT).as_ref() {
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    );
                }
                return 0;
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, message, w, l)
}

unsafe fn paint(hwnd: HWND, step: u32) {
    let mut ps = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    if dc.is_null() {
        return;
    }
    draw(hwnd, dc, step);
    EndPaint(hwnd, &ps);
}

unsafe fn draw(hwnd: HWND, dc: HDC, step: u32) {
    if dc.is_null() {
        return;
    }
    let mut rect = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    FillRect(dc, &rect, GetSysColorBrush(COLOR_WINDOW));
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, GetSysColor(COLOR_WINDOWTEXT));
    SetTextAlign(dc, TA_CENTER | TA_TOP);
    let old_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
    let text = wide("CodeCalを起動しています…");
    let scale = GetDpiForWindow(hwnd).max(96) as i32;
    let cx = rect.right / 2;
    TextOutW(
        dc,
        cx,
        30 * scale / 96,
        text.as_ptr(),
        (text.len() - 1) as i32,
    );
    SelectObject(dc, old_font);
    let cy = 90 * scale / 96;
    let r = 13 * scale / 96;
    let brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
    for (color, segment) in [(0x00e5e5e5, false), (0x0088940d, true)] {
        let pen = CreatePen(PS_SOLID, (3 * scale / 96).max(1), color);
        if !pen.is_null() {
            let old = SelectObject(dc, pen);
            if segment {
                let a = f64::from(step) * std::f64::consts::PI / 6.0;
                let b = a + std::f64::consts::PI / 2.0;
                Arc(
                    dc,
                    cx - r,
                    cy - r,
                    cx + r,
                    cy + r,
                    cx + (f64::from(r) * a.cos()) as i32,
                    cy + (f64::from(r) * a.sin()) as i32,
                    cx + (f64::from(r) * b.cos()) as i32,
                    cy + (f64::from(r) * b.sin()) as i32,
                );
            } else {
                Ellipse(dc, cx - r, cy - r, cx + r, cy + r);
            }
            SelectObject(dc, old);
            DeleteObject(pen);
        }
    }
    SelectObject(dc, brush);
}
