use std::sync::mpsc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopAction {
    Toggle,
    Clear,
    Settings,
    CaptureClipboard,
    Quit,
}

pub struct DesktopServices {
    backend: backend::Backend,
}

impl DesktopServices {
    pub fn install(callback: impl Fn(DesktopAction) + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        let backend = backend::install(sender);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            while let Ok(action) = receiver.try_recv() {
                callback(action);
            }
            glib::ControlFlow::Continue
        });
        Self { backend }
    }

    pub fn update_count(&self, count: usize) {
        self.backend.update_count(count);
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use super::DesktopAction;
    use ksni::TrayMethods;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::Sender,
    };
    use yeet::i18n::tr;

    pub struct Backend {
        count: Arc<AtomicUsize>,
    }

    impl Backend {
        pub fn update_count(&self, count: usize) {
            self.count.store(count, Ordering::Relaxed);
        }
    }

    #[derive(Clone)]
    struct YeetTray {
        sender: Sender<DesktopAction>,
        count: Arc<AtomicUsize>,
    }

    impl ksni::Tray for YeetTray {
        fn id(&self) -> String {
            "io.github.hjosugi.Yeet".to_owned()
        }

        fn icon_name(&self) -> String {
            "io.github.hjosugi.Yeet".to_owned()
        }

        fn title(&self) -> String {
            format!("Yeet ({})", self.count.load(Ordering::Relaxed))
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            let count = self.count.load(Ordering::Relaxed);
            ksni::ToolTip {
                icon_name: self.icon_name(),
                title: "Yeet".to_owned(),
                description: format!("{count} item(s) on the shelf"),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.sender.send(DesktopAction::Toggle);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::{MenuItem, StandardItem};

            fn item(
                label: &str,
                icon_name: &str,
                action: DesktopAction,
            ) -> ksni::MenuItem<YeetTray> {
                StandardItem {
                    label: label.to_owned(),
                    icon_name: icon_name.to_owned(),
                    activate: Box::new(move |tray: &mut YeetTray| {
                        let _ = tray.sender.send(action);
                    }),
                    ..Default::default()
                }
                .into()
            }

            vec![
                item(
                    tr("show_hide"),
                    "view-reveal-symbolic",
                    DesktopAction::Toggle,
                ),
                item(
                    tr("capture_clipboard"),
                    "edit-paste-symbolic",
                    DesktopAction::CaptureClipboard,
                ),
                item(tr("clear"), "edit-clear-all-symbolic", DesktopAction::Clear),
                item(
                    tr("settings"),
                    "emblem-system-symbolic",
                    DesktopAction::Settings,
                ),
                MenuItem::Separator,
                item(tr("quit"), "application-exit-symbolic", DesktopAction::Quit),
            ]
        }
    }

    pub fn install(sender: Sender<DesktopAction>) -> Backend {
        let count = Arc::new(AtomicUsize::new(0));
        let thread_count = count.clone();
        std::thread::Builder::new()
            .name("yeet-desktop-services".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        eprintln!("yeet: desktop services runtime: {error}");
                        return;
                    }
                };
                runtime.block_on(async move {
                    let tray = YeetTray {
                        sender: sender.clone(),
                        count: thread_count,
                    };
                    let _tray_handle =
                        match tray.disable_dbus_name(ashpd::is_sandboxed()).spawn().await {
                            Ok(handle) => Some(handle),
                            Err(error) => {
                                eprintln!("yeet: status notifier unavailable: {error}");
                                None
                            }
                        };
                    std::future::pending::<()>().await;
                });
            })
            .expect("desktop services thread");
        Backend { count }
    }
}

#[cfg(target_os = "windows")]
mod backend {
    use super::DesktopAction;
    use std::ffi::c_void;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
        mpsc::Sender,
    };
    use std::thread::JoinHandle;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NIM_SETVERSION, NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CREATESTRUCTW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW,
        GetWindowLongPtrW, HWND_MESSAGE, IDI_APPLICATION, LoadIconW, MENU_ITEM_FLAGS, MF_SEPARATOR,
        MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
        SetForegroundWindow, SetWindowLongPtrW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD,
        TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
        WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_NCCREATE, WM_NULL, WNDCLASSW,
    };
    use windows::core::{PCWSTR, w};
    use yeet::i18n::tr;

    const APP_ICON_RESOURCE_ID: usize = 1;
    const TRAY_ICON_ID: u32 = 1;
    const WM_TRAY: u32 = WM_APP + 1;
    const WM_UPDATE_COUNT: u32 = WM_APP + 2;
    const WM_SHUTDOWN: u32 = WM_APP + 3;
    const NIN_KEYSELECT: u32 = NIN_SELECT + 1;

    const MENU_TOGGLE: usize = 1;
    const MENU_CAPTURE_CLIPBOARD: usize = 2;
    const MENU_CLEAR: usize = 3;
    const MENU_SETTINGS: usize = 4;
    const MENU_QUIT: usize = 5;

    pub struct Backend {
        count: Arc<AtomicUsize>,
        window: Arc<AtomicPtr<c_void>>,
        stopping: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Backend {
        pub fn update_count(&self, count: usize) {
            self.count.store(count, Ordering::Relaxed);
            self.post(WM_UPDATE_COUNT);
        }

        fn post(&self, message: u32) {
            let window = self.window.load(Ordering::Acquire);
            if !window.is_null() {
                // SAFETY: The HWND is published only after CreateWindowExW succeeds and remains
                // owned by the tray thread until Backend::drop posts shutdown and joins it.
                let _ = unsafe { PostMessageW(Some(HWND(window)), message, WPARAM(0), LPARAM(0)) };
            }
        }
    }

    impl Drop for Backend {
        fn drop(&mut self) {
            self.stopping.store(true, Ordering::Release);
            self.post(WM_SHUTDOWN);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    struct TrayState {
        sender: Sender<DesktopAction>,
        count: Arc<AtomicUsize>,
        taskbar_created: u32,
    }

    pub fn install(sender: Sender<DesktopAction>) -> Backend {
        let count = Arc::new(AtomicUsize::new(0));
        let window = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_count = count.clone();
        let thread_window = window.clone();
        let thread_stopping = stopping.clone();
        let thread = std::thread::Builder::new()
            .name("yeet-windows-tray".to_owned())
            .spawn(move || {
                if let Err(error) = run_tray(sender, thread_count, thread_window, thread_stopping) {
                    eprintln!("yeet: Windows tray unavailable: {error}");
                }
            })
            .ok();

        Backend {
            count,
            window,
            stopping,
            thread,
        }
    }

    fn run_tray(
        sender: Sender<DesktopAction>,
        count: Arc<AtomicUsize>,
        window_handle: Arc<AtomicPtr<c_void>>,
        stopping: Arc<AtomicBool>,
    ) -> windows::core::Result<()> {
        // SAFETY: All Win32 objects created below live on this dedicated message-loop thread.
        unsafe {
            let module = GetModuleHandleW(None)?;
            let instance = HINSTANCE(module.0);
            let class_name = w!("YeetTrayMessageWindow");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(windows::core::Error::from_thread());
            }

            let mut state = Box::new(TrayState {
                sender,
                count,
                taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            });
            let state_ptr = (&mut *state as *mut TrayState).cast::<c_void>();
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("Yeet tray"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                Some(state_ptr.cast_const()),
            )?;
            window_handle.store(window.0, Ordering::Release);

            if stopping.load(Ordering::Acquire) {
                let _ = DestroyWindow(window);
            } else if let Err(error) = add_icon(window, state.count.load(Ordering::Relaxed)) {
                eprintln!("yeet: could not add the Windows tray icon: {error}");
            }

            let mut message = MSG::default();
            let mut message_error = None;
            loop {
                let result = GetMessageW(&mut message, None, 0, 0);
                if result.0 == -1 {
                    message_error = Some(windows::core::Error::from_thread());
                    break;
                }
                if result.0 == 0 {
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            window_handle.store(std::ptr::null_mut(), Ordering::Release);
            delete_icon(window);
            let _ = DestroyWindow(window);
            drop(state);
            match message_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_NCCREATE {
            // SAFETY: WM_NCCREATE receives the CREATESTRUCTW initialized by CreateWindowExW.
            let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
        }

        // SAFETY: GWLP_USERDATA is either null during early creation, or points to TrayState for
        // the entire window/message-loop lifetime.
        let state =
            unsafe { (GetWindowLongPtrW(window, GWLP_USERDATA) as *const TrayState).as_ref() };

        if let Some(state) = state {
            if state.taskbar_created != 0 && message == state.taskbar_created {
                let _ = unsafe { add_icon(window, state.count.load(Ordering::Relaxed)) };
                return LRESULT(0);
            }

            match message {
                WM_TRAY => {
                    let notification = lparam.0 as u32 & 0xffff;
                    if matches!(notification, NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP) {
                        let _ = state.sender.send(DesktopAction::Toggle);
                    } else if notification == WM_CONTEXTMENU {
                        unsafe { show_context_menu(window, &state.sender) };
                    }
                    return LRESULT(0);
                }
                WM_UPDATE_COUNT => {
                    unsafe { update_tooltip(window, state.count.load(Ordering::Relaxed)) };
                    return LRESULT(0);
                }
                WM_CLOSE | WM_SHUTDOWN => {
                    unsafe {
                        delete_icon(window);
                        let _ = DestroyWindow(window);
                    }
                    return LRESULT(0);
                }
                WM_DESTROY => {
                    unsafe { PostQuitMessage(0) };
                    return LRESULT(0);
                }
                _ => {}
            }
        }

        // SAFETY: Unhandled messages must be forwarded to the system default procedure.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    unsafe fn add_icon(window: HWND, count: usize) -> windows::core::Result<()> {
        let mut data = notify_data(window);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.hIcon = load_app_icon();
        copy_tooltip(&mut data.szTip, count);
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
            return Err(windows::core::Error::from_thread());
        }

        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        if !unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) }.as_bool() {
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
            return Err(windows::core::Error::from_thread());
        }
        Ok(())
    }

    unsafe fn update_tooltip(window: HWND, count: usize) {
        let mut data = notify_data(window);
        data.uFlags = NIF_TIP | NIF_SHOWTIP;
        copy_tooltip(&mut data.szTip, count);
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    unsafe fn delete_icon(window: HWND) {
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &notify_data(window)) };
    }

    fn notify_data(window: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: TRAY_ICON_ID,
            uCallbackMessage: WM_TRAY,
            ..Default::default()
        }
    }

    fn copy_tooltip(destination: &mut [u16; 128], count: usize) {
        let noun = if count == 1 { "item" } else { "items" };
        let tooltip = format!("Yeet — {count} {noun} on the shelf");
        let encoded = tooltip.encode_utf16().take(destination.len() - 1);
        for (target, value) in destination.iter_mut().zip(encoded) {
            *target = value;
        }
    }

    fn load_app_icon() -> windows::Win32::UI::WindowsAndMessaging::HICON {
        let module = unsafe { GetModuleHandleW(None) }
            .ok()
            .map(|module| HINSTANCE(module.0));
        // MAKEINTRESOURCEW encodes a numeric resource identifier in a pointer-sized value. The
        // Win32 API interprets it as an integer and never dereferences it as a Rust pointer.
        #[allow(clippy::manual_dangling_ptr)]
        let resource = PCWSTR(APP_ICON_RESOURCE_ID as *const u16);
        module
            .and_then(|instance| unsafe { LoadIconW(Some(instance), resource) }.ok())
            .or_else(|| unsafe { LoadIconW(None, IDI_APPLICATION) }.ok())
            .unwrap_or_default()
    }

    unsafe fn show_context_menu(window: HWND, sender: &Sender<DesktopAction>) {
        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };

        let items = [
            (MENU_TOGGLE, tr("show_hide")),
            (MENU_CAPTURE_CLIPBOARD, tr("capture_clipboard")),
            (MENU_CLEAR, tr("clear")),
            (MENU_SETTINGS, tr("settings")),
        ];
        for (id, label) in items {
            unsafe { append_menu_item(menu, MF_STRING, id, label) };
        }
        unsafe { append_menu_item(menu, MF_SEPARATOR, 0, "") };
        unsafe { append_menu_item(menu, MF_STRING, MENU_QUIT, tr("quit")) };

        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_ok() {
            let _ = unsafe { SetForegroundWindow(window) };
            let command = unsafe {
                TrackPopupMenu(
                    menu,
                    TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
                    cursor.x,
                    cursor.y,
                    None,
                    window,
                    None,
                )
            };
            let action = match command.0 as usize {
                MENU_TOGGLE => Some(DesktopAction::Toggle),
                MENU_CAPTURE_CLIPBOARD => Some(DesktopAction::CaptureClipboard),
                MENU_CLEAR => Some(DesktopAction::Clear),
                MENU_SETTINGS => Some(DesktopAction::Settings),
                MENU_QUIT => Some(DesktopAction::Quit),
                _ => None,
            };
            if let Some(action) = action {
                let _ = sender.send(action);
            }
            let _ = unsafe { PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0)) };
        }
        let _ = unsafe { DestroyMenu(menu) };
    }

    unsafe fn append_menu_item(
        menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
        flags: MENU_ITEM_FLAGS,
        id: usize,
        label: &str,
    ) {
        let mut label: Vec<u16> = label.encode_utf16().collect();
        label.push(0);
        let _ = unsafe { AppendMenuW(menu, flags, id, PCWSTR(label.as_ptr())) };
    }
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
mod backend {
    use super::DesktopAction;
    use std::sync::mpsc::Sender;

    pub struct Backend;

    impl Backend {
        pub fn update_count(&self, _count: usize) {}
    }

    pub fn install(_sender: Sender<DesktopAction>) -> Backend {
        Backend
    }
}
