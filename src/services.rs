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
    use wayland_yeet::i18n::tr;

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
    use std::sync::mpsc::Sender;
    use tray_icon::{
        Icon, TrayIcon, TrayIconBuilder,
        menu::{Menu, MenuEvent, MenuItem},
    };
    use wayland_yeet::i18n::tr;

    pub struct Backend {
        tray: Option<TrayIcon>,
    }

    impl Backend {
        pub fn update_count(&self, count: usize) {
            if let Some(tray) = &self.tray {
                let _ = tray.set_tooltip(Some(format!("Yeet — {count} item(s)")));
            }
        }
    }

    pub fn install(sender: Sender<DesktopAction>) -> Backend {
        let menu = Menu::new();
        let items = [
            MenuItem::with_id("toggle", tr("show_hide"), true, None),
            MenuItem::with_id("capture", tr("capture_clipboard"), true, None),
            MenuItem::with_id("clear", tr("clear"), true, None),
            MenuItem::with_id("settings", tr("settings"), true, None),
            MenuItem::with_id("quit", tr("quit"), true, None),
        ];
        if let Err(error) =
            menu.append_items(&[&items[0], &items[1], &items[2], &items[3], &items[4]])
        {
            eprintln!("yeet: tray menu: {error}");
        }
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = match event.id.0.as_str() {
                "toggle" => DesktopAction::Toggle,
                "capture" => DesktopAction::CaptureClipboard,
                "clear" => DesktopAction::Clear,
                "settings" => DesktopAction::Settings,
                "quit" => DesktopAction::Quit,
                _ => return,
            };
            let _ = sender.send(action);
        }));
        let tray = TrayIconBuilder::new()
            .with_id("io.github.hjosugi.Yeet")
            .with_tooltip("Yeet")
            .with_menu(Box::new(menu))
            .with_icon(tray_icon())
            .build()
            .map_err(|error| eprintln!("yeet: tray icon: {error}"))
            .ok();
        Backend { tray }
    }

    fn tray_icon() -> Icon {
        let mut rgba = vec![0_u8; 32 * 32 * 4];
        for y in 0_usize..32 {
            for x in 0_usize..32 {
                let offset = (y * 32 + x) * 4;
                let white_y = (y < 15 && (x.abs_diff(8 + y / 2) < 2 || x.abs_diff(24 - y / 2) < 2))
                    || (y >= 14 && x.abs_diff(16) < 2);
                let color = if white_y {
                    [255, 255, 255, 255]
                } else {
                    [111, 66, 193, 255]
                };
                rgba[offset..offset + 4].copy_from_slice(&color);
            }
        }
        Icon::from_rgba(rgba, 32, 32).expect("valid built-in tray icon")
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
