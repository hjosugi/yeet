use crate::platform;
use directories::ProjectDirs;
use gio::prelude::*;
use glib::types::StaticType;
use glib::value::ToValue;
use gtk::gdk;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use wayland_yeet::model::ShelfModel;
use wayland_yeet::settings::{Settings, Theme};

pub struct Ui {
    _hold: gio::ApplicationHoldGuard,
    app: gtk::Application,
    model: Rc<RefCell<ShelfModel>>,
    settings: Rc<RefCell<Settings>>,
    shelf: gtk::ApplicationWindow,
    list: gtk::ListBox,
    count: gtk::Label,
    edges: RefCell<Vec<gtk::Window>>,
    selected: RefCell<HashSet<PathBuf>>,
}

impl Ui {
    pub fn new(app: &gtk::Application) -> Rc<Self> {
        install_css();
        let settings = Settings::load();
        apply_theme(settings.theme);
        // Yeet owns always-available edge drop targets even while its shelf is
        // hidden, so the primary instance must not exit with no mapped shelf.
        let hold = app.hold();

        let shelf = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Yeet")
            .default_width(300)
            .default_height(520)
            .decorated(false)
            .resizable(true)
            .build();
        shelf.add_css_class("yeet-shelf");
        platform::configure_shelf(&shelf);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
        outer.set_margin_top(12);
        outer.set_margin_bottom(12);
        outer.set_margin_start(12);
        outer.set_margin_end(12);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::new(Some("YEET"));
        title.add_css_class("title");
        title.set_hexpand(true);
        title.set_halign(gtk::Align::Start);
        let count = gtk::Label::new(Some("0"));
        count.add_css_class("dim-label");
        let hide = gtk::Button::from_icon_name("window-minimize-symbolic");
        hide.add_css_class("flat");
        header.append(&title);
        header.append(&count);
        header.append(&hide);
        outer.append(&header);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Multiple);
        list.add_css_class("boxed-list");
        let empty = gtk::Label::new(Some("Drop files or text here"));
        empty.set_margin_top(60);
        empty.set_margin_bottom(60);
        list.set_placeholder(Some(&empty));
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        outer.append(&scroll);

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let mode = if platform::layer_shell_supported() {
            "Wayland layer shell"
        } else if cfg!(target_os = "windows") {
            "Windows native"
        } else {
            "Fallback window"
        };
        let mode_label = gtk::Label::new(Some(mode));
        mode_label.add_css_class("dim-label");
        mode_label.set_hexpand(true);
        mode_label.set_halign(gtk::Align::Start);
        let clear = gtk::Button::from_icon_name("edit-clear-all-symbolic");
        clear.add_css_class("flat");
        clear.set_tooltip_text(Some("Remove all unpinned items"));
        let clipboard = gtk::Button::from_icon_name("edit-paste-symbolic");
        clipboard.add_css_class("flat");
        clipboard.set_tooltip_text(Some("Capture clipboard"));
        let preferences = gtk::Button::from_icon_name("emblem-system-symbolic");
        preferences.add_css_class("flat");
        preferences.set_tooltip_text(Some("Settings"));
        footer.append(&mode_label);
        footer.append(&clipboard);
        footer.append(&preferences);
        footer.append(&clear);
        outer.append(&footer);
        shelf.set_child(Some(&outer));

        let state_path = ProjectDirs::from("io", "hjosugi", "Yeet")
            .map(|dirs| dirs.data_local_dir().join("shelf.json"))
            .unwrap_or_else(|| std::env::temp_dir().join("yeet/shelf.json"));
        let model = if settings.restore_shelf {
            ShelfModel::load(state_path.clone()).unwrap_or_else(|error| {
                eprintln!("yeet: could not restore shelf: {error}");
                ShelfModel::empty(state_path)
            })
        } else {
            ShelfModel::empty(state_path)
        };
        let ui = Rc::new(Self {
            _hold: hold,
            app: app.clone(),
            model: Rc::new(RefCell::new(model)),
            settings: Rc::new(RefCell::new(settings)),
            shelf,
            list,
            count,
            edges: RefCell::new(Vec::new()),
            selected: RefCell::new(HashSet::new()),
        });

        {
            let ui = ui.clone();
            hide.connect_clicked(move |_| ui.hide());
        }
        {
            let ui = ui.clone();
            clear.connect_clicked(move |_| {
                if let Err(error) = ui.model.borrow_mut().clear_unpinned() {
                    eprintln!("yeet: {error:#}");
                }
                ui.selected.borrow_mut().clear();
                ui.refresh();
                ui.hide_if_empty();
            });
        }
        {
            let ui = ui.clone();
            clipboard.connect_clicked(move |_| ui.capture_clipboard());
        }
        {
            let ui = ui.clone();
            preferences.connect_clicked(move |_| ui.show_settings());
        }
        {
            let shelf = ui.shelf.clone();
            let ui = ui.clone();
            shelf.connect_close_request(move |_| {
                ui.hide();
                glib::Propagation::Stop
            });
        }
        {
            let ui = ui.clone();
            let list = ui.list.clone();
            list.connect_selected_rows_changed(move |list| {
                let model = ui.model.borrow();
                let mut selected = ui.selected.borrow_mut();
                selected.clear();
                for row in list.selected_rows() {
                    if let Some(item) = model.items().get(row.index() as usize) {
                        selected.insert(item.path.clone());
                    }
                }
            });
        }
        add_drag_source(&header, &ui, None);
        install_keyboard(&ui);

        attach_drop_target(&ui.shelf, &ui, false, None);
        ui.refresh();
        ui.rebuild_edges(app);
        if let Some(display) = gdk::Display::default() {
            let monitors = display.monitors();
            let ui_for_change = ui.clone();
            let app = app.clone();
            monitors.connect_items_changed(move |_, _, _, _| {
                ui_for_change.rebuild_edges(&app);
            });
        }
        {
            let weak = Rc::downgrade(&ui);
            let last_press = Rc::new(Cell::new(None::<Instant>));
            platform::install_global_hotkey(move || {
                if let Some(ui) = weak.upgrade() {
                    let now = Instant::now();
                    let is_double = last_press
                        .get()
                        .is_some_and(|last| now.duration_since(last) <= Duration::from_millis(500));
                    last_press.set(Some(now));
                    if is_double {
                        ui.capture_clipboard();
                    } else {
                        ui.toggle();
                    }
                }
            });
        }
        ui
    }

    pub fn handle_arguments(
        self: &Rc<Self>,
        arguments: &[PathBuf],
        toggle: bool,
        clear: bool,
        hidden: bool,
    ) {
        if clear {
            if let Err(error) = self.model.borrow_mut().clear_unpinned() {
                eprintln!("yeet: {error:#}");
            }
            self.selected.borrow_mut().clear();
        }
        let added = match self.model.borrow_mut().add_paths(arguments.iter().cloned()) {
            Ok(added) => added > 0,
            Err(error) => {
                eprintln!("yeet: {error:#}");
                false
            }
        };
        self.refresh();
        if toggle {
            if self.shelf.is_visible() {
                self.hide();
            } else {
                self.show(None);
            }
        } else if added || (!hidden && !self.model.borrow().items().is_empty()) {
            self.show(None);
        } else {
            self.hide_if_empty();
        }
    }

    fn rebuild_edges(self: &Rc<Self>, app: &gtk::Application) {
        for window in self.edges.borrow_mut().drain(..) {
            window.close();
        }
        if cfg!(target_os = "linux") && !platform::layer_shell_supported() {
            return;
        }
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let monitors = display.monitors();
        let mut edges = self.edges.borrow_mut();
        for index in 0..monitors.n_items() {
            let Some(monitor) = monitors
                .item(index)
                .and_then(|item| item.downcast::<gdk::Monitor>().ok())
            else {
                continue;
            };
            let edge = gtk::Window::builder()
                .application(app)
                .title("Yeet edge")
                .decorated(false)
                .default_width(6)
                .focusable(false)
                .build();
            edge.add_css_class("yeet-edge");
            let strip_size = self.settings.borrow().strip_size;
            platform::configure_edge(&edge, &monitor, strip_size);
            attach_drop_target(&edge, self, true, Some(monitor));
            edge.set_visible(true);
            edges.push(edge);
        }
    }

    fn refresh(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let items = self.model.borrow().items().to_vec();
        let selected_snapshot = self.selected.borrow().clone();
        self.count.set_text(&items.len().to_string());
        for item in items {
            let row = gtk::ListBoxRow::new();
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            content.set_margin_top(7);
            content.set_margin_bottom(7);
            content.set_margin_start(8);
            content.set_margin_end(6);

            let icon = item_icon(&item.path);
            let name = gtk::Label::new(Some(&item.display_name()));
            name.set_hexpand(true);
            name.set_halign(gtk::Align::Start);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            let pin = gtk::Button::from_icon_name(if item.pinned {
                "view-pin-symbolic"
            } else {
                "window-pin-symbolic"
            });
            pin.add_css_class("flat");
            let preview = gtk::Button::from_icon_name("document-open-symbolic");
            preview.add_css_class("flat");
            let remove = gtk::Button::from_icon_name("window-close-symbolic");
            remove.add_css_class("flat");
            content.append(&icon);
            content.append(&name);
            content.append(&preview);
            content.append(&pin);
            content.append(&remove);
            row.set_child(Some(&content));
            if selected_snapshot.contains(&item.path) {
                self.list.select_row(Some(&row));
            }
            {
                let ui = self.clone();
                let path = item.path.clone();
                preview.connect_clicked(move |_| ui.preview_path(&path));
            }
            {
                let ui = self.clone();
                let path = item.path.clone();
                pin.connect_clicked(move |_| {
                    let index = ui
                        .model
                        .borrow()
                        .items()
                        .iter()
                        .position(|item| item.path == path);
                    let result = index
                        .map(|index| ui.model.borrow_mut().toggle_pinned(index))
                        .transpose();
                    if let Err(error) = result {
                        eprintln!("yeet: {error:#}");
                    }
                    ui.refresh();
                });
            }
            {
                let ui = self.clone();
                let path = item.path.clone();
                remove.connect_clicked(move |_| {
                    let index = ui
                        .model
                        .borrow()
                        .items()
                        .iter()
                        .position(|item| item.path == path);
                    let result = index
                        .map(|index| ui.model.borrow_mut().remove(index))
                        .transpose();
                    if let Err(error) = result {
                        eprintln!("yeet: {error:#}");
                    }
                    ui.selected.borrow_mut().remove(&path);
                    ui.refresh();
                    ui.hide_if_empty();
                });
            }
            attach_context_menu(&content, self, item.path.clone());
            add_drag_source(&content, self, Some(item.path));
            self.list.append(&row);
        }
    }

    fn show(&self, monitor: Option<&gdk::Monitor>) {
        if let Some(monitor) = monitor {
            platform::set_shelf_monitor(&self.shelf, monitor);
        }
        self.shelf.set_visible(true);
    }

    fn hide(&self) {
        self.shelf.set_visible(false);
    }

    fn toggle(&self) {
        if self.shelf.is_visible() {
            self.hide();
        } else {
            self.show(None);
        }
    }

    fn hide_if_empty(&self) {
        if self.settings.borrow().auto_hide && self.model.borrow().items().is_empty() {
            self.hide();
        }
    }

    fn remove_selected(self: &Rc<Self>) {
        let paths: Vec<PathBuf> = self.selected.borrow().iter().cloned().collect();
        let mut indices: Vec<usize> = self
            .model
            .borrow()
            .items()
            .iter()
            .enumerate()
            .filter_map(|(index, item)| paths.contains(&item.path).then_some(index))
            .collect();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for index in indices {
            if let Err(error) = self.model.borrow_mut().remove(index) {
                eprintln!("yeet: {error:#}");
            }
        }
        self.selected.borrow_mut().clear();
        self.refresh();
        self.hide_if_empty();
    }

    fn preview_path(&self, path: &Path) {
        if is_image(path) {
            let picture = gtk::Picture::for_filename(path);
            picture.set_can_shrink(true);
            picture.set_content_fit(gtk::ContentFit::Contain);
            let window = gtk::Window::builder()
                .title(
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Preview"),
                )
                .default_width(720)
                .default_height(520)
                .transient_for(&self.shelf)
                .child(&picture)
                .build();
            window.present();
            return;
        }
        if is_text(path) {
            match fs::read(path) {
                Ok(bytes) => {
                    let truncated = bytes.len() > 256 * 1024;
                    let bytes = &bytes[..bytes.len().min(256 * 1024)];
                    let mut text = String::from_utf8_lossy(bytes).into_owned();
                    if truncated {
                        text.push_str("\n\n— Preview truncated —");
                    }
                    let view = gtk::TextView::new();
                    view.set_editable(false);
                    view.set_cursor_visible(false);
                    view.set_wrap_mode(gtk::WrapMode::WordChar);
                    view.buffer().set_text(&text);
                    let scroll = gtk::ScrolledWindow::builder()
                        .min_content_width(640)
                        .min_content_height(480)
                        .child(&view)
                        .build();
                    let window = gtk::Window::builder()
                        .title(
                            path.file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("Preview"),
                        )
                        .default_width(720)
                        .default_height(520)
                        .transient_for(&self.shelf)
                        .child(&scroll)
                        .build();
                    window.present();
                }
                Err(error) => eprintln!("yeet: preview failed: {error}"),
            }
            return;
        }
        open_path(path);
    }

    fn flash_duplicate(&self) {
        self.shelf.add_css_class("duplicate");
        let shelf = self.shelf.clone();
        glib::timeout_add_local_once(Duration::from_millis(450), move || {
            shelf.remove_css_class("duplicate");
        });
    }

    fn capture_clipboard(self: &Rc<Self>) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let clipboard = display.clipboard();
        let ui = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(value) = clipboard
                .read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT)
                .await
                && let Ok(files) = value.get::<gdk::FileList>()
            {
                let paths = files.files().into_iter().filter_map(|file| file.path());
                if ui.model.borrow_mut().add_paths(paths).unwrap_or(0) > 0 {
                    ui.refresh();
                    ui.show(None);
                    return;
                }
            }
            if let Ok(Some(texture)) = clipboard.read_texture_future().await
                && let Ok(path) = ui.model.borrow().managed_path("png")
                && texture.save_to_png(&path).is_ok()
                && ui
                    .model
                    .borrow_mut()
                    .add_managed_path(path, "Clipboard image".to_owned())
                    .unwrap_or(false)
            {
                ui.refresh();
                ui.show(None);
                return;
            }
            if let Ok(Some(text)) = clipboard.read_text_future().await
                && ui.model.borrow_mut().add_text(&text).unwrap_or(false)
            {
                ui.refresh();
                ui.show(None);
            }
        });
    }

    fn show_settings(self: &Rc<Self>) {
        let window = gtk::Window::builder()
            .title("Yeet Settings")
            .default_width(380)
            .resizable(false)
            .transient_for(&self.shelf)
            .modal(true)
            .build();
        let grid = gtk::Grid::builder()
            .row_spacing(14)
            .column_spacing(18)
            .margin_top(20)
            .margin_bottom(20)
            .margin_start(20)
            .margin_end(20)
            .build();
        let settings = self.settings.borrow().clone();
        let auto_hide = gtk::Switch::builder().active(settings.auto_hide).build();
        let restore = gtk::Switch::builder()
            .active(settings.restore_shelf)
            .build();
        let autostart = gtk::Switch::builder().active(settings.autostart).build();
        let strip = gtk::SpinButton::with_range(3.0, 16.0, 1.0);
        strip.set_value(settings.strip_size.into());
        let theme = gtk::DropDown::from_strings(&["System", "Light", "Dark"]);
        theme.set_selected(match settings.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        });
        add_setting_row(&grid, 0, "Hide when empty", &auto_hide);
        add_setting_row(&grid, 1, "Restore shelf at launch", &restore);
        add_setting_row(&grid, 2, "Start with the session", &autostart);
        add_setting_row(&grid, 3, "Edge width", &strip);
        add_setting_row(&grid, 4, "Theme", &theme);
        window.set_child(Some(&grid));

        connect_setting(&auto_hide, self, |settings, value| {
            settings.auto_hide = value
        });
        connect_setting(&restore, self, |settings, value| {
            settings.restore_shelf = value
        });
        {
            let ui = self.clone();
            autostart.connect_active_notify(move |switch| {
                let enabled = switch.is_active();
                if let Err(error) = platform::set_autostart(enabled) {
                    eprintln!("yeet: autostart: {error}");
                    switch.set_active(!enabled);
                    return;
                }
                ui.settings.borrow_mut().autostart = enabled;
                ui.save_settings();
            });
        }
        {
            let ui = self.clone();
            strip.connect_value_changed(move |spin| {
                ui.settings.borrow_mut().strip_size = spin.value_as_int();
                ui.save_settings();
                ui.rebuild_edges(&ui.app);
            });
        }
        {
            let ui = self.clone();
            theme.connect_selected_notify(move |dropdown| {
                let value = match dropdown.selected() {
                    1 => Theme::Light,
                    2 => Theme::Dark,
                    _ => Theme::System,
                };
                ui.settings.borrow_mut().theme = value;
                apply_theme(value);
                ui.save_settings();
            });
        }
        window.present();
    }

    fn save_settings(&self) {
        if let Err(error) = self.settings.borrow().save() {
            eprintln!("yeet: settings: {error}");
        }
    }
}

fn add_setting_row(grid: &gtk::Grid, row: i32, text: &str, control: &impl IsA<gtk::Widget>) {
    let label = gtk::Label::new(Some(text));
    label.set_halign(gtk::Align::Start);
    label.set_hexpand(true);
    grid.attach(&label, 0, row, 1, 1);
    grid.attach(control, 1, row, 1, 1);
}

fn connect_setting(
    switch: &gtk::Switch,
    ui: &Rc<Ui>,
    update: impl Fn(&mut Settings, bool) + 'static,
) {
    let ui = ui.clone();
    switch.connect_active_notify(move |switch| {
        update(&mut ui.settings.borrow_mut(), switch.is_active());
        ui.save_settings();
    });
}

fn apply_theme(theme: Theme) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    // GTK 4.8 exposes a dark preference but not the three-state color-scheme
    // property added in later GTK releases. System and Light therefore clear
    // the explicit dark preference, while Dark opts in everywhere.
    settings.set_gtk_application_prefer_dark_theme(theme == Theme::Dark);
}

fn add_drag_source(widget: &impl IsA<gtk::Widget>, ui: &Rc<Ui>, source_path: Option<PathBuf>) {
    let source = gtk::DragSource::builder()
        .actions(gdk::DragAction::COPY | gdk::DragAction::MOVE)
        .build();
    let cancelled = Rc::new(Cell::new(false));
    let active_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));

    {
        let ui = ui.clone();
        let cancelled = cancelled.clone();
        let active_paths = active_paths.clone();
        let source_path = source_path.clone();
        source.connect_prepare(move |_, _, _| {
            cancelled.set(false);
            let selected = ui.selected.borrow();
            let paths: Vec<PathBuf> = if let Some(source_path) = &source_path {
                if selected.contains(source_path) {
                    ui.model
                        .borrow()
                        .items()
                        .iter()
                        .filter(|item| selected.contains(&item.path))
                        .map(|item| item.path.clone())
                        .collect()
                } else {
                    vec![source_path.clone()]
                }
            } else {
                ui.model
                    .borrow()
                    .items()
                    .iter()
                    .map(|item| item.path.clone())
                    .collect()
            };
            *active_paths.borrow_mut() = paths.clone();
            let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();
            if files.is_empty() {
                return None;
            }
            let file_list = gdk::FileList::from_array(&files);
            Some(gdk::ContentProvider::for_value(&file_list.to_value()))
        });
    }
    {
        let cancelled = cancelled.clone();
        source.connect_drag_cancel(move |_, _, _| {
            cancelled.set(true);
            false
        });
    }
    {
        let ui = ui.clone();
        source.connect_drag_end(move |_, drag, _delete_data| {
            let accepted = !cancelled.get() && !drag.selected_action().is_empty();
            let paths = active_paths.borrow().clone();
            if accepted {
                match ui.model.borrow_mut().remove_paths_after_drop(&paths) {
                    Ok(_) => {
                        for path in paths {
                            ui.selected.borrow_mut().remove(&path);
                        }
                    }
                    Err(error) => eprintln!("yeet: {error:#}"),
                }
            }
            ui.refresh();
            ui.hide_if_empty();
        });
    }
    widget.add_controller(source);
}

fn attach_drop_target(
    widget: &impl IsA<gtk::Widget>,
    ui: &Rc<Ui>,
    reveal_on_enter: bool,
    monitor: Option<gdk::Monitor>,
) {
    let target = gtk::DropTarget::new(
        glib::Type::INVALID,
        gdk::DragAction::COPY | gdk::DragAction::MOVE,
    );
    target.set_types(&[
        gdk::FileList::static_type(),
        String::static_type(),
        gdk::Texture::static_type(),
    ]);
    if reveal_on_enter {
        let ui = ui.clone();
        let monitor = monitor.clone();
        target.connect_enter(move |_, _, _| {
            ui.show(monitor.as_ref());
            gdk::DragAction::COPY
        });
    }
    {
        let ui = ui.clone();
        target.connect_drop(move |_, value, _, _| {
            let result = if let Ok(files) = value.get::<gdk::FileList>() {
                let paths = files
                    .files()
                    .into_iter()
                    .filter_map(|file| file.path())
                    .collect::<Vec<_>>();
                ui.model.borrow_mut().add_paths(paths)
            } else if let Ok(text) = value.get::<String>() {
                if text.starts_with("https://") || text.starts_with("http://") {
                    ui.model.borrow_mut().add_remote_uri(&text).map(usize::from)
                } else {
                    ui.model.borrow_mut().add_text(&text).map(usize::from)
                }
            } else if let Ok(texture) = value.get::<gdk::Texture>() {
                let path = match ui.model.borrow().managed_path("png") {
                    Ok(path) => path,
                    Err(error) => {
                        eprintln!("yeet: {error}");
                        return false;
                    }
                };
                if let Err(error) = texture.save_to_png(&path) {
                    eprintln!("yeet: image drop failed: {error}");
                    return false;
                }
                ui.model
                    .borrow_mut()
                    .add_managed_path(path, "Image snippet".to_owned())
                    .map(usize::from)
            } else {
                return false;
            };
            match result {
                Ok(changed) => {
                    if changed > 0 {
                        ui.refresh();
                        ui.show(None);
                    } else {
                        ui.flash_duplicate();
                    }
                    true
                }
                Err(error) => {
                    eprintln!("yeet: {error:#}");
                    false
                }
            }
        });
    }
    widget.add_controller(target);
}

fn install_keyboard(ui: &Rc<Ui>) {
    let keys = gtk::EventControllerKey::new();
    let ui_for_keys = ui.clone();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if key == gdk::Key::Escape {
            ui_for_keys.hide();
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Delete {
            ui_for_keys.remove_selected();
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::space {
            if let Some(path) = ui_for_keys.selected.borrow().iter().next().cloned() {
                ui_for_keys.preview_path(&path);
            }
            return glib::Propagation::Stop;
        }
        if modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            && matches!(key, gdk::Key::a | gdk::Key::A)
        {
            ui_for_keys.list.select_all();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    ui.shelf.add_controller(keys);
}

fn item_icon(path: &Path) -> gtk::Widget {
    if is_image(path) {
        let picture = gtk::Picture::for_filename(path);
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_size_request(38, 38);
        return picture.upcast();
    }
    let file = gio::File::for_path(path);
    let image = file
        .query_info(
            "standard::icon",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|info| info.icon())
        .map(|icon| gtk::Image::from_gicon(&icon))
        .unwrap_or_else(|| {
            gtk::Image::from_icon_name(if path.is_dir() {
                "folder-symbolic"
            } else {
                "text-x-generic-symbolic"
            })
        });
    image.set_pixel_size(32);
    image.upcast()
}

fn attach_context_menu(widget: &impl IsA<gtk::Widget>, ui: &Rc<Ui>, path: PathBuf) {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(true);
    popover.set_parent(widget);
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let open = gtk::Button::with_label("Open");
    let reveal = gtk::Button::with_label("Reveal in file manager");
    let copy = gtk::Button::with_label("Copy path");
    let remove = gtk::Button::with_label("Remove");
    for button in [&open, &reveal, &copy, &remove] {
        button.add_css_class("flat");
        menu.append(button);
    }
    popover.set_child(Some(&menu));

    {
        let path = path.clone();
        open.connect_clicked(move |_| open_path(&path));
    }
    {
        let path = path.clone();
        reveal.connect_clicked(move |_| {
            if let Some(parent) = path.parent() {
                open_path(parent);
            }
        });
    }
    {
        let path = path.clone();
        copy.connect_clicked(move |_| {
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_text(&path.to_string_lossy());
            }
        });
    }
    {
        let ui = ui.clone();
        let path = path.clone();
        remove.connect_clicked(move |_| {
            let index = ui
                .model
                .borrow()
                .items()
                .iter()
                .position(|item| item.path == path);
            if let Some(index) = index
                && let Err(error) = ui.model.borrow_mut().remove(index)
            {
                eprintln!("yeet: {error}");
            }
            ui.refresh();
            ui.hide_if_empty();
        });
    }
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_SECONDARY);
    click.connect_pressed(move |gesture, _, x, y| {
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    widget.add_controller(click);
}

fn open_path(path: &Path) {
    let uri = gio::File::for_path(path).uri();
    if let Err(error) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
        eprintln!("yeet: open failed: {error}");
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "svg"
            )
        })
}

fn is_text(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "txt"
                    | "md"
                    | "json"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "rs"
                    | "py"
                    | "go"
                    | "c"
                    | "h"
                    | "cpp"
                    | "js"
                    | "ts"
                    | "css"
                    | "html"
                    | "log"
                    | "url"
            )
        })
}

fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".yeet-shelf { background: alpha(@window_bg_color, 0.96); border: 1px solid alpha(@accent_color, 0.55); border-radius: 12px; }\n\
         .yeet-edge { background: alpha(@accent_color, 0.04); }\n\
         .yeet-edge:drop(active) { background: alpha(@accent_color, 0.65); }\n\
         .yeet-shelf.duplicate { border: 3px solid @warning_color; }\n\
         .title { font-weight: 800; letter-spacing: 2px; }\n\
         .boxed-list row { border-radius: 8px; margin-bottom: 5px; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
