use crate::platform;
use crate::services::{DesktopAction, DesktopServices};
use directories::ProjectDirs;
use gio::prelude::*;
use glib::types::StaticType;
use glib::value::ToValue;
use gtk::gdk;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use uuid::Uuid;
use wayland_yeet::i18n::{Language, set_language, tr};
use wayland_yeet::model::{AddReport, ShelfItem, ShelfModel};
use wayland_yeet::settings::{HotkeyBinding, ScreenEdge, Settings, Theme};

thread_local! {
    static THUMBNAIL_CACHE: RefCell<HashMap<PathBuf, gdk::Texture>> = RefCell::new(HashMap::new());
}

pub struct Ui {
    _hold: gio::ApplicationHoldGuard,
    app: gtk::Application,
    model: Rc<RefCell<ShelfModel>>,
    settings: Rc<RefCell<Settings>>,
    shelf: gtk::ApplicationWindow,
    list: gtk::ListBox,
    count: gtk::Label,
    empty: gtk::Label,
    mode_label: gtk::Label,
    hide_button: gtk::Button,
    clear_button: gtk::Button,
    clipboard_button: gtk::Button,
    preferences_button: gtk::Button,
    revealer: gtk::Revealer,
    edges: RefCell<Vec<gtk::Window>>,
    selected: RefCell<HashSet<Uuid>>,
    global_hotkey: RefCell<Option<platform::GlobalHotkey>>,
    desktop_services: RefCell<Option<DesktopServices>>,
    drag_active: Cell<bool>,
}

impl Ui {
    pub fn new(app: &gtk::Application) -> Rc<Self> {
        install_css();
        let settings = Settings::load();
        set_language(settings.language);
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
        shelf.set_accessible_role(gtk::AccessibleRole::Dialog);
        shelf.update_property(&[
            gtk::accessible::Property::Label(tr("shelf_title")),
            gtk::accessible::Property::Description(tr("shelf_description")),
        ]);
        platform::configure_shelf(&shelf, settings.edge);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
        outer.set_margin_top(12);
        outer.set_margin_bottom(12);
        outer.set_margin_start(12);
        outer.set_margin_end(12);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.set_accessible_role(gtk::AccessibleRole::Banner);
        header.update_property(&[
            gtk::accessible::Property::Label("Drag all shelf items"),
            gtk::accessible::Property::Description(
                "Drag this header to move the complete stack as one group.",
            ),
        ]);
        let stack_icon = gtk::Image::from_icon_name("view-grid-symbolic");
        stack_icon.set_pixel_size(20);
        let title = gtk::Label::new(Some("YEET"));
        title.add_css_class("title");
        title.set_hexpand(true);
        title.set_halign(gtk::Align::Start);
        let count = gtk::Label::new(Some("0"));
        count.add_css_class("dim-label");
        count.set_accessible_role(gtk::AccessibleRole::Status);
        count.update_property(&[gtk::accessible::Property::Label("0 items on the shelf")]);
        let hide = gtk::Button::from_icon_name("window-minimize-symbolic");
        hide.add_css_class("flat");
        set_button_accessibility(&hide, tr("hide_shelf"), "Escape");
        header.append(&stack_icon);
        header.append(&title);
        header.append(&hide);
        outer.append(&header);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Multiple);
        list.set_focusable(true);
        list.set_accessible_role(gtk::AccessibleRole::ListBox);
        list.update_property(&[
            gtk::accessible::Property::Label(tr("shelf_items")),
            gtk::accessible::Property::Description(tr("shelf_items_help")),
            gtk::accessible::Property::MultiSelectable(true),
            gtk::accessible::Property::Orientation(gtk::Orientation::Vertical),
        ]);
        list.add_css_class("boxed-list");
        let empty = gtk::Label::new(Some(tr("drop_here")));
        empty.update_property(&[gtk::accessible::Property::Label(tr("empty_help"))]);
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
        footer.set_accessible_role(gtk::AccessibleRole::Toolbar);
        footer.update_property(&[gtk::accessible::Property::Label(tr("shelf_actions"))]);
        let mode = if platform::layer_shell_supported() {
            tr("wayland_mode")
        } else if cfg!(target_os = "windows") {
            tr("windows_mode")
        } else {
            tr("fallback_mode")
        };
        let mode_label = gtk::Label::new(Some(mode));
        mode_label.add_css_class("dim-label");
        mode_label.set_hexpand(true);
        mode_label.set_halign(gtk::Align::Start);
        let clear = gtk::Button::from_icon_name("edit-clear-all-symbolic");
        clear.add_css_class("flat");
        clear.set_tooltip_text(Some(tr("clear_unpinned")));
        set_button_accessibility(&clear, tr("clear_unpinned"), "");
        let clipboard = gtk::Button::from_icon_name("edit-paste-symbolic");
        clipboard.add_css_class("flat");
        clipboard.set_tooltip_text(Some(tr("capture_clipboard")));
        let clipboard_shortcut = format!("{} twice", settings.global_hotkey);
        set_button_accessibility(&clipboard, tr("capture_clipboard"), &clipboard_shortcut);
        let preferences = gtk::Button::from_icon_name("emblem-system-symbolic");
        preferences.add_css_class("flat");
        preferences.set_tooltip_text(Some(tr("settings")));
        set_button_accessibility(&preferences, tr("settings"), "");
        footer.append(&mode_label);
        footer.append(&count);
        footer.append(&clipboard);
        footer.append(&preferences);
        footer.append(&clear);
        for button in [&hide, &clipboard, &preferences, &clear] {
            button.add_css_class("touch-target");
        }
        outer.append(&footer);
        let revealer = gtk::Revealer::builder()
            .child(&outer)
            .reveal_child(true)
            .transition_type(if settings.edge == ScreenEdge::Right {
                gtk::RevealerTransitionType::SlideLeft
            } else {
                gtk::RevealerTransitionType::SlideRight
            })
            .transition_duration(if settings.reduced_motion { 0 } else { 180 })
            .build();
        shelf.set_child(Some(&revealer));

        let state_path = ProjectDirs::from("io", "hjosugi", "Yeet")
            .map(|dirs| dirs.data_local_dir().join("shelf.json"))
            .unwrap_or_else(|| std::env::temp_dir().join("yeet/shelf.json"));
        let model = if settings.restore_shelf {
            ShelfModel::load_with_deduplication(state_path.clone(), settings.deduplicate_items)
                .unwrap_or_else(|error| {
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
            empty: empty.clone(),
            mode_label: mode_label.clone(),
            hide_button: hide.clone(),
            clear_button: clear.clone(),
            clipboard_button: clipboard.clone(),
            preferences_button: preferences.clone(),
            revealer,
            edges: RefCell::new(Vec::new()),
            selected: RefCell::new(HashSet::new()),
            global_hotkey: RefCell::new(None),
            desktop_services: RefCell::new(None),
            drag_active: Cell::new(false),
        });

        {
            let ui = ui.clone();
            hide.connect_clicked(move |_| ui.hide());
        }
        {
            let ui = ui.clone();
            clear.connect_clicked(move |_| {
                ui.clear_unpinned();
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
                        selected.insert(item.id);
                    }
                }
                update_selection_accessibility(list);
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
            let shortcut = ui.settings.borrow().global_hotkey.clone();
            let hotkey = platform::install_global_hotkey(&shortcut, move || {
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
            *ui.global_hotkey.borrow_mut() = Some(hotkey);
            ui.update_hotkey_accessibility();
        }
        {
            let weak = Rc::downgrade(&ui);
            let services = DesktopServices::install(move |action| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                match action {
                    DesktopAction::Toggle => ui.toggle(),
                    DesktopAction::Clear => ui.clear_unpinned(),
                    DesktopAction::Settings => ui.show_settings(),
                    DesktopAction::CaptureClipboard => ui.capture_clipboard(),
                    DesktopAction::Quit => ui.app.quit(),
                }
            });
            services.update_count(ui.model.borrow().items().len());
            *ui.desktop_services.borrow_mut() = Some(services);
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
        let deduplicate_items = self.settings.borrow().deduplicate_items;
        let added = match self
            .model
            .borrow_mut()
            .add_paths_report_with_deduplication(arguments.iter().cloned(), deduplicate_items)
        {
            Ok(report) => report.added > 0,
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
            if monitor.connector().is_some_and(|connector| {
                self.settings
                    .borrow()
                    .disabled_outputs
                    .iter()
                    .any(|disabled| disabled == connector.as_str())
            }) {
                continue;
            }
            let edge = gtk::Window::builder()
                .application(app)
                .title("Yeet edge")
                .decorated(false)
                .default_width(6)
                .focusable(false)
                .build();
            edge.add_css_class("yeet-edge");
            let strip_size = self.settings.borrow().strip_size;
            platform::configure_edge(&edge, &monitor, strip_size, self.settings.borrow().edge);
            attach_drop_target(&edge, self, true, Some(monitor));
            edge.set_visible(true);
            edges.push(edge);
        }
    }

    fn refresh(self: &Rc<Self>) {
        let focused_id = self.focused_id();
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let items = self.model.borrow().items().to_vec();
        let selected_snapshot = self.selected.borrow().clone();
        self.count.set_text(&items.len().to_string());
        if let Some(services) = self.desktop_services.borrow().as_ref() {
            services.update_count(items.len());
        }
        let count_description = format!(
            "{} {} on the shelf",
            items.len(),
            if items.len() == 1 { "item" } else { "items" }
        );
        self.count
            .update_property(&[gtk::accessible::Property::Label(&count_description)]);
        let item_count = items.len();
        for (index, item) in items.into_iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            row.add_css_class("shelf-row");
            row.set_focusable(true);
            row.set_activatable(true);
            row.set_accessible_role(gtk::AccessibleRole::Option);
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
            set_button_accessibility(
                &pin,
                if item.pinned {
                    tr("unpin_item")
                } else {
                    tr("pin_item")
                },
                "",
            );
            let preview = gtk::Button::from_icon_name("document-open-symbolic");
            preview.add_css_class("flat");
            set_button_accessibility(&preview, tr("preview_item"), "Space or Enter");
            let remove = gtk::Button::from_icon_name("window-close-symbolic");
            remove.add_css_class("flat");
            set_button_accessibility(&remove, tr("remove_item"), "Delete");
            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            actions.add_css_class("row-actions");
            actions.set_accessible_role(gtk::AccessibleRole::Group);
            actions.update_property(&[gtk::accessible::Property::Label(tr("item_actions"))]);
            for button in [&preview, &pin, &remove] {
                button.add_css_class("row-action");
                actions.append(button);
            }
            if self.settings.borrow().reduced_motion {
                actions.add_css_class("no-motion");
            }
            content.append(&icon);
            content.append(&name);
            content.append(&actions);
            row.set_child(Some(&content));
            attach_row_action_reveal(&row, &actions);
            let accessible_name = if item.pinned {
                format!("{}, pinned", item.display_name())
            } else {
                item.display_name()
            };
            let accessible_description = format!(
                "{}. Space or Enter previews, Ctrl+C copies, and Delete removes this item.",
                item.path.display()
            );
            row.update_property(&[
                gtk::accessible::Property::Label(&accessible_name),
                gtk::accessible::Property::Description(&accessible_description),
                gtk::accessible::Property::KeyShortcuts("Space Enter Ctrl+C Delete"),
            ]);
            row.update_relation(&[
                gtk::accessible::Relation::PosInSet((index + 1) as i32),
                gtk::accessible::Relation::SetSize(item_count as i32),
            ]);
            self.list.append(&row);
            if selected_snapshot.contains(&item.id) {
                self.list.select_row(Some(&row));
            }
            {
                let ui = self.clone();
                let path = item.path.clone();
                preview.connect_clicked(move |_| ui.preview_path(&path));
            }
            {
                let ui = self.clone();
                let id = item.id;
                pin.connect_clicked(move |_| {
                    let index = ui
                        .model
                        .borrow()
                        .items()
                        .iter()
                        .position(|item| item.id == id);
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
                let id = item.id;
                remove.connect_clicked(move |_| {
                    let index = ui
                        .model
                        .borrow()
                        .items()
                        .iter()
                        .position(|item| item.id == id);
                    let result = index
                        .map(|index| ui.model.borrow_mut().remove(index))
                        .transpose();
                    if let Err(error) = result {
                        eprintln!("yeet: {error:#}");
                    }
                    ui.selected.borrow_mut().remove(&id);
                    ui.refresh();
                    ui.hide_if_empty();
                });
            }
            attach_context_menu(&content, self, item.id, item.path.clone());
            add_drag_source(&content, self, Some(item.id));
        }
        update_selection_accessibility(&self.list);
        if self.shelf.is_visible() {
            let focus_index = focused_id
                .as_ref()
                .and_then(|id| {
                    self.model
                        .borrow()
                        .items()
                        .iter()
                        .position(|item| &item.id == id)
                })
                .or_else(|| self.first_selected_index())
                .unwrap_or(0);
            if selected_snapshot.is_empty() {
                self.focus_row(focus_index, false);
            } else {
                self.focus_row_without_selection(focus_index);
            }
        }
    }

    fn show(&self, monitor: Option<&gdk::Monitor>) {
        if let Some(monitor) = monitor {
            platform::set_shelf_monitor(&self.shelf, monitor, self.settings.borrow().edge);
        }
        self.shelf.set_visible(true);
        self.revealer.set_reveal_child(true);
        if let Some(index) = self.first_selected_index() {
            self.focus_row_without_selection(index);
        } else {
            self.focus_row(0, false);
        }
    }

    fn hide(&self) {
        if self.settings.borrow().reduced_motion {
            self.shelf.set_visible(false);
            return;
        }
        self.revealer.set_reveal_child(false);
        let shelf = self.shelf.clone();
        let revealer = self.revealer.clone();
        glib::timeout_add_local_once(Duration::from_millis(190), move || {
            if !revealer.reveals_child() {
                shelf.set_visible(false);
            }
        });
    }

    fn toggle(&self) {
        if self.shelf.is_visible() {
            self.hide();
        } else {
            self.show(None);
        }
    }

    fn hide_if_empty(&self) {
        if self.settings.borrow().auto_hide
            && self.model.borrow().items().is_empty()
            && !self.drag_active.get()
        {
            self.hide();
        }
    }

    fn remove_selected(self: &Rc<Self>) {
        let mut ids: Vec<Uuid> = self.selected.borrow().iter().copied().collect();
        if ids.is_empty()
            && let Some(id) = self.focused_id()
        {
            ids.push(id);
        }
        let mut indices: Vec<usize> = self
            .model
            .borrow()
            .items()
            .iter()
            .enumerate()
            .filter_map(|(index, item)| ids.contains(&item.id).then_some(index))
            .collect();
        let next_focus = indices.iter().min().copied().unwrap_or(0);
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for index in indices {
            if let Err(error) = self.model.borrow_mut().remove(index) {
                eprintln!("yeet: {error:#}");
            }
        }
        self.selected.borrow_mut().clear();
        self.refresh();
        self.focus_row(next_focus, false);
        self.hide_if_empty();
    }

    fn clear_unpinned(self: &Rc<Self>) {
        if let Err(error) = self.model.borrow_mut().clear_unpinned() {
            eprintln!("yeet: {error:#}");
        }
        self.selected.borrow_mut().clear();
        self.refresh();
        self.hide_if_empty();
    }

    fn copy_selected(&self) {
        let mut paths = self.selected_paths();
        if paths.is_empty()
            && let Some(path) = self.focused_path()
        {
            paths.push(path);
        }
        if paths.is_empty() {
            return;
        }
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();
        let file_list = gdk::FileList::from_array(&files);
        let file_provider = gdk::ContentProvider::for_value(&file_list.to_value());
        let text = paths_as_text(&paths);
        let text_provider = gdk::ContentProvider::for_value(&text.to_value());
        let provider = gdk::ContentProvider::new_union(&[file_provider, text_provider]);
        if let Err(error) = display.clipboard().set_content(Some(&provider)) {
            eprintln!("yeet: copy failed: {error}");
        }
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        let selected = self.selected.borrow();
        self.model
            .borrow()
            .items()
            .iter()
            .filter(|item| selected.contains(&item.id))
            .map(|item| item.path.clone())
            .collect()
    }

    fn first_selected_index(&self) -> Option<usize> {
        self.list
            .selected_rows()
            .into_iter()
            .map(|row| row.index() as usize)
            .min()
    }

    fn focused_row_index(&self) -> Option<usize> {
        let focus = gtk::prelude::GtkWindowExt::focus(&self.shelf)?;
        if let Ok(row) = focus.clone().downcast::<gtk::ListBoxRow>() {
            return usize::try_from(row.index()).ok();
        }
        focus
            .ancestor(gtk::ListBoxRow::static_type())
            .and_then(|widget| widget.downcast::<gtk::ListBoxRow>().ok())
            .and_then(|row| usize::try_from(row.index()).ok())
    }

    fn focused_path(&self) -> Option<PathBuf> {
        self.focused_row_index().and_then(|index| {
            self.model
                .borrow()
                .items()
                .get(index)
                .map(|item| item.path.clone())
        })
    }

    fn focused_id(&self) -> Option<Uuid> {
        self.focused_row_index()
            .and_then(|index| self.model.borrow().items().get(index).map(|item| item.id))
    }

    fn focus_row(&self, index: usize, extend_selection: bool) {
        let Some(row) = self.list.row_at_index(index as i32) else {
            self.list.grab_focus();
            return;
        };
        if !extend_selection {
            self.list.unselect_all();
        }
        self.list.select_row(Some(&row));
        row.grab_focus();
    }

    fn focus_row_without_selection(&self, index: usize) {
        if let Some(row) = self.list.row_at_index(index as i32) {
            row.grab_focus();
        } else {
            self.list.grab_focus();
        }
    }

    fn navigate_items(&self, navigation: Navigation, extend_selection: bool) {
        let item_count = self.model.borrow().items().len();
        let current = self
            .focused_row_index()
            .or_else(|| self.first_selected_index());
        if let Some(index) = navigation_target(current, item_count, navigation) {
            self.focus_row(index, extend_selection);
        }
    }

    fn preview_selected(&self) {
        let path = self
            .focused_path()
            .or_else(|| self.selected_paths().into_iter().next());
        if let Some(path) = path {
            self.preview_path(&path);
        }
    }

    fn preview_path(&self, path: &Path) {
        if is_image(path) {
            let picture = gtk::Picture::for_filename(path);
            picture.set_can_shrink(true);
            picture.set_content_fit(gtk::ContentFit::Contain);
            picture.update_property(&[gtk::accessible::Property::Label("Image preview")]);
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
            configure_preview_window(&window);
            window.present();
            return;
        }
        if is_pdf(path) {
            match render_pdf_first_page(path) {
                Ok(preview) => {
                    let picture = gtk::Picture::for_filename(&preview);
                    picture.set_can_shrink(true);
                    picture.set_content_fit(gtk::ContentFit::Contain);
                    picture.update_property(&[gtk::accessible::Property::Label(
                        "PDF first-page preview",
                    )]);
                    let window = gtk::Window::builder()
                        .title(
                            path.file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or(tr("preview")),
                        )
                        .default_width(720)
                        .default_height(520)
                        .transient_for(&self.shelf)
                        .child(&picture)
                        .build();
                    window.connect_destroy(move |_| cleanup_preview_file(&preview));
                    configure_preview_window(&window);
                    window.present();
                }
                Err(error) => {
                    eprintln!(
                        "yeet: PDF preview unavailable for {}: {error}; opening the default app",
                        path.display()
                    );
                    open_path(path);
                }
            }
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
                    view.update_property(&[
                        gtk::accessible::Property::Label("Text preview"),
                        gtk::accessible::Property::ReadOnly(true),
                        gtk::accessible::Property::MultiLine(true),
                    ]);
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
                    configure_preview_window(&window);
                    window.present();
                }
                Err(error) => eprintln!("yeet: preview failed: {error}"),
            }
            return;
        }
        open_path(path);
    }

    fn flash_duplicates(&self, ids: &[Uuid]) {
        self.shelf.add_css_class("duplicate");
        let shelf = self.shelf.clone();
        let rows: Vec<gtk::ListBoxRow> = ids
            .iter()
            .filter_map(|id| {
                self.model
                    .borrow()
                    .items()
                    .iter()
                    .position(|item| &item.id == id)
            })
            .filter_map(|index| self.list.row_at_index(index as i32))
            .collect();
        for row in &rows {
            row.add_css_class("duplicate");
        }
        if let Some(id) = ids.first()
            && let Some(index) = self
                .model
                .borrow()
                .items()
                .iter()
                .position(|item| &item.id == id)
        {
            self.focus_row_without_selection(index);
        }
        glib::timeout_add_local_once(Duration::from_millis(450), move || {
            shelf.remove_css_class("duplicate");
            for row in rows {
                row.remove_css_class("duplicate");
            }
        });
    }

    fn present_drop_report(self: &Rc<Self>, report: AddReport, monitor: Option<&gdk::Monitor>) {
        if report.rejected > 0 {
            eprintln!(
                "yeet: ignored {} unsupported or unavailable dropped URI(s)",
                report.rejected
            );
        }
        if report.added == 0 && report.duplicate_ids.is_empty() {
            return;
        }
        if !report.duplicate_ids.is_empty() {
            let mut selected = self.selected.borrow_mut();
            selected.clear();
            selected.extend(report.duplicate_ids.iter().copied());
        } else if self.settings.borrow().stack_multi_drop && report.added_ids.len() > 1 {
            let mut selected = self.selected.borrow_mut();
            selected.clear();
            selected.extend(report.added_ids.iter().copied());
        }
        self.refresh();
        self.show(monitor);
        if !report.duplicate_ids.is_empty() {
            self.flash_duplicates(&report.duplicate_ids);
        }
    }

    fn apply_language(&self) {
        self.shelf.update_property(&[
            gtk::accessible::Property::Label(tr("shelf_title")),
            gtk::accessible::Property::Description(tr("shelf_description")),
        ]);
        self.list.update_property(&[
            gtk::accessible::Property::Label(tr("shelf_items")),
            gtk::accessible::Property::Description(tr("shelf_items_help")),
        ]);
        self.empty.set_text(tr("drop_here"));
        self.empty
            .update_property(&[gtk::accessible::Property::Label(tr("empty_help"))]);
        self.mode_label
            .set_text(if platform::layer_shell_supported() {
                tr("wayland_mode")
            } else if cfg!(target_os = "windows") {
                tr("windows_mode")
            } else {
                tr("fallback_mode")
            });
        self.clear_button
            .set_tooltip_text(Some(tr("clear_unpinned")));
        self.clipboard_button
            .set_tooltip_text(Some(tr("capture_clipboard")));
        self.preferences_button
            .set_tooltip_text(Some(tr("settings")));
        set_button_accessibility(&self.hide_button, tr("hide_shelf"), "Escape");
        set_button_accessibility(&self.clear_button, tr("clear_unpinned"), "");
        self.update_hotkey_accessibility();
        set_button_accessibility(&self.preferences_button, tr("settings"), "");
    }

    fn update_hotkey_accessibility(&self) {
        let shortcut = format!("{} twice", self.settings.borrow().global_hotkey);
        set_button_accessibility(&self.clipboard_button, tr("capture_clipboard"), &shortcut);
    }

    fn capture_clipboard(self: &Rc<Self>) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let clipboard = display.clipboard();
        if clipboard_is_sensitive(&clipboard) {
            eprintln!("yeet: clipboard capture skipped because the source marked it as sensitive");
            return;
        }
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
                    .add_managed_path_with_mime(
                        path,
                        tr("clipboard_image").to_owned(),
                        Some("image/png".to_owned()),
                    )
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
            .title(tr("settings_title"))
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
        let deduplicate_items = gtk::Switch::builder()
            .active(settings.deduplicate_items)
            .build();
        let stack_multi_drop = gtk::Switch::builder()
            .active(settings.stack_multi_drop)
            .build();
        let autostart = gtk::Switch::builder().active(settings.autostart).build();
        let strip = gtk::SpinButton::with_range(3.0, 16.0, 1.0);
        strip.set_value(settings.strip_size.into());
        let theme = gtk::DropDown::from_strings(&[tr("system"), tr("light"), tr("dark")]);
        theme.set_selected(match settings.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        });
        let language = gtk::DropDown::from_strings(&[tr("system"), tr("english"), tr("japanese")]);
        language.set_selected(match settings.language {
            Language::System => 0,
            Language::English => 1,
            Language::Japanese => 2,
        });
        let reduced_motion = gtk::Switch::builder()
            .active(settings.reduced_motion)
            .build();
        let edge = gtk::DropDown::from_strings(&[tr("left"), tr("right")]);
        edge.set_selected(if settings.edge == ScreenEdge::Left {
            0
        } else {
            1
        });
        let disabled_outputs = gtk::Entry::new();
        disabled_outputs.set_text(&settings.disabled_outputs.join(", "));
        disabled_outputs.set_placeholder_text(Some("DP-1, HDMI-A-1"));
        let global_hotkey = gtk::Entry::new();
        global_hotkey.set_hexpand(true);
        global_hotkey.set_text(&settings.global_hotkey);
        global_hotkey.set_placeholder_text(Some("Ctrl+Alt+Y"));
        global_hotkey.set_tooltip_text(Some(tr("global_hotkey_hint")));
        let global_hotkey_error = gtk::Label::new(None);
        global_hotkey_error.set_halign(gtk::Align::Start);
        global_hotkey_error.set_wrap(true);
        global_hotkey_error.add_css_class("error");
        global_hotkey_error.set_visible(false);
        let apply_global_hotkey = gtk::Button::with_label(tr("apply"));
        apply_global_hotkey.set_sensitive(HotkeyBinding::parse(&settings.global_hotkey).is_ok());
        let global_hotkey_control = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        global_hotkey_control.append(&global_hotkey);
        global_hotkey_control.append(&apply_global_hotkey);
        if let Some(error) = self
            .global_hotkey
            .borrow()
            .as_ref()
            .and_then(platform::GlobalHotkey::registration_error)
        {
            global_hotkey_error.set_text(&global_hotkey_error_text(error));
            global_hotkey_error.set_visible(true);
        }
        add_setting_row(&grid, 0, tr("hide_when_empty"), &auto_hide);
        add_setting_row(&grid, 1, tr("restore_shelf"), &restore);
        add_setting_row(&grid, 2, tr("deduplicate_items"), &deduplicate_items);
        add_setting_row(&grid, 3, tr("stack_multi_drop"), &stack_multi_drop);
        add_setting_row(&grid, 4, tr("start_session"), &autostart);
        add_setting_row(&grid, 5, tr("edge_width"), &strip);
        add_setting_row(&grid, 6, tr("theme"), &theme);
        add_setting_row(&grid, 7, tr("language"), &language);
        add_setting_row(&grid, 8, tr("reduced_motion"), &reduced_motion);
        add_setting_row(&grid, 9, tr("screen_edge"), &edge);
        let disabled_outputs_row = if cfg!(target_os = "windows") {
            add_setting_row(&grid, 10, tr("global_hotkey"), &global_hotkey_control);
            grid.attach(&global_hotkey_error, 0, 11, 2, 1);
            12
        } else {
            10
        };
        add_setting_row(
            &grid,
            disabled_outputs_row,
            tr("disabled_outputs"),
            &disabled_outputs,
        );
        window.set_child(Some(&grid));

        {
            let ui = self.clone();
            auto_hide.connect_active_notify(move |switch| {
                ui.settings.borrow_mut().auto_hide = switch.is_active();
                ui.save_settings();
                if switch.is_active() {
                    ui.hide_if_empty();
                }
            });
        }
        connect_setting(&restore, self, |settings, value| {
            settings.restore_shelf = value
        });
        connect_setting(&deduplicate_items, self, |settings, value| {
            settings.deduplicate_items = value
        });
        connect_setting(&stack_multi_drop, self, |settings, value| {
            settings.stack_multi_drop = value
        });
        {
            let ui = self.clone();
            reduced_motion.connect_active_notify(move |switch| {
                let reduced = switch.is_active();
                ui.settings.borrow_mut().reduced_motion = reduced;
                ui.revealer
                    .set_transition_duration(if reduced { 0 } else { 180 });
                ui.save_settings();
                ui.refresh();
            });
        }
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
            edge.connect_selected_notify(move |dropdown| {
                ui.settings.borrow_mut().edge = if dropdown.selected() == 0 {
                    ScreenEdge::Left
                } else {
                    ScreenEdge::Right
                };
                ui.revealer.set_transition_type(
                    if ui.settings.borrow().edge == ScreenEdge::Right {
                        gtk::RevealerTransitionType::SlideLeft
                    } else {
                        gtk::RevealerTransitionType::SlideRight
                    },
                );
                ui.save_settings();
                platform::update_shelf_placement(&ui.shelf, ui.settings.borrow().edge);
                ui.rebuild_edges(&ui.app);
            });
        }
        {
            let ui = self.clone();
            disabled_outputs.connect_changed(move |entry| {
                ui.settings.borrow_mut().disabled_outputs = entry
                    .text()
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
                ui.settings.borrow_mut().normalize();
                ui.save_settings();
                ui.rebuild_edges(&ui.app);
            });
        }
        if cfg!(target_os = "windows") {
            let error_label = global_hotkey_error.clone();
            let apply_button = apply_global_hotkey.clone();
            global_hotkey.connect_changed(move |entry| {
                if let Err(error) = HotkeyBinding::parse(entry.text().as_str()) {
                    apply_button.set_sensitive(false);
                    error_label.set_text(&format!("{}: {error}", tr("global_hotkey_invalid")));
                    error_label.set_visible(true);
                } else {
                    apply_button.set_sensitive(true);
                    error_label.set_text("");
                    error_label.set_visible(false);
                }
            });

            let ui = self.clone();
            let error_label = global_hotkey_error.clone();
            let entry = global_hotkey.clone();
            apply_global_hotkey.connect_clicked(move |_| {
                let result = ui
                    .global_hotkey
                    .borrow_mut()
                    .as_mut()
                    .ok_or_else(|| {
                        platform::GlobalHotkeyError::Unavailable(
                            "hotkey service was not initialized".to_owned(),
                        )
                    })
                    .and_then(|hotkey| hotkey.rebind(entry.text().as_str()));
                match result {
                    Ok(normalized) => {
                        error_label.set_visible(false);
                        error_label.set_text("");
                        if ui.settings.borrow().global_hotkey != normalized {
                            ui.settings.borrow_mut().global_hotkey = normalized.clone();
                            ui.save_settings();
                            ui.update_hotkey_accessibility();
                        }
                        if entry.text().as_str() != normalized {
                            entry.set_text(&normalized);
                        }
                    }
                    Err(error) => {
                        error_label.set_text(&global_hotkey_error_text(&error));
                        error_label.set_visible(true);
                    }
                }
            });
            let apply_global_hotkey = apply_global_hotkey.clone();
            global_hotkey.connect_activate(move |_| apply_global_hotkey.emit_clicked());
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
                ui.refresh_native_theme();
                ui.save_settings();
            });
        }
        {
            let ui = self.clone();
            let window = window.clone();
            language.connect_selected_notify(move |dropdown| {
                let value = match dropdown.selected() {
                    1 => Language::English,
                    2 => Language::Japanese,
                    _ => Language::System,
                };
                if ui.settings.borrow().language == value {
                    return;
                }
                ui.settings.borrow_mut().language = value;
                set_language(value);
                ui.save_settings();
                ui.apply_language();
                ui.refresh();
                window.close();
                ui.show_settings();
            });
        }
        window.present();
    }

    fn save_settings(&self) {
        if let Err(error) = self.settings.borrow().save() {
            eprintln!("yeet: settings: {error}");
        }
    }

    fn refresh_native_theme(&self) {
        platform::refresh_window_theme(self.shelf.upcast_ref());
        for edge in self.edges.borrow().iter() {
            platform::refresh_window_theme(edge);
        }
    }
}

fn global_hotkey_error_text(error: &platform::GlobalHotkeyError) -> String {
    match error {
        platform::GlobalHotkeyError::Invalid(detail) => {
            format!("{}: {detail}", tr("global_hotkey_invalid"))
        }
        platform::GlobalHotkeyError::Conflict {
            shortcut,
            previous_restored,
            ..
        } => format!(
            "{}: {shortcut} {}",
            tr("global_hotkey_conflict"),
            if *previous_restored {
                tr("global_hotkey_restored")
            } else {
                tr("global_hotkey_rollback_failed")
            }
        ),
        platform::GlobalHotkeyError::Unavailable(detail) => {
            format!("{} {detail}", tr("global_hotkey_unavailable"))
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
    platform::set_theme(theme);
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    // GTK 4.8 exposes a dark preference but not the three-state color-scheme
    // property added in later GTK releases. System and Light therefore clear
    // the explicit dark preference, while Dark opts in everywhere.
    settings.set_gtk_application_prefer_dark_theme(theme == Theme::Dark);
}

fn add_drag_source(widget: &impl IsA<gtk::Widget>, ui: &Rc<Ui>, source_id: Option<Uuid>) {
    let source = gtk::DragSource::builder()
        .actions(gdk::DragAction::COPY | gdk::DragAction::MOVE)
        .build();
    let cancelled = Rc::new(Cell::new(false));
    let active_items = Rc::new(RefCell::new(Vec::<(Uuid, PathBuf)>::new()));

    {
        let ui = ui.clone();
        let cancelled = cancelled.clone();
        let active_items = active_items.clone();
        source.connect_prepare(move |_, _, _| {
            cancelled.set(false);
            let selected = ui.selected.borrow();
            let model = ui.model.borrow();
            let items: Vec<(Uuid, PathBuf)> = match source_id {
                Some(source_id) if selected.contains(&source_id) => model
                    .items()
                    .iter()
                    .filter(|item| selected.contains(&item.id))
                    .map(|item| (item.id, item.path.clone()))
                    .collect(),
                Some(source_id) => model
                    .items()
                    .iter()
                    .find(|item| item.id == source_id)
                    .map(|item| (item.id, item.path.clone()))
                    .into_iter()
                    .collect(),
                None => model
                    .items()
                    .iter()
                    .map(|item| (item.id, item.path.clone()))
                    .collect(),
            };
            *active_items.borrow_mut() = items.clone();
            let files: Vec<gio::File> = items
                .iter()
                .map(|(_, path)| gio::File::for_path(path))
                .collect();
            if files.is_empty() {
                return None;
            }
            ui.drag_active.set(true);
            let file_list = gdk::FileList::from_array(&files);
            let mut providers = vec![gdk::ContentProvider::for_value(&file_list.to_value())];
            if let [(id, _)] = items.as_slice() {
                let item = model.items().iter().find(|item| item.id == *id);
                if let Some(item) = item
                    && let Ok(Some(payload)) = snippet_drag_payload(item)
                {
                    let bytes = glib::Bytes::from_owned(payload.bytes);
                    providers.push(gdk::ContentProvider::for_bytes(&payload.mime_type, &bytes));
                    if payload.mime_type.starts_with("text/")
                        && let Ok(text) = std::str::from_utf8(bytes.as_ref())
                    {
                        providers.push(gdk::ContentProvider::for_value(&text.to_value()));
                    } else if payload.mime_type.starts_with("image/")
                        && let Ok(texture) = gdk::Texture::from_bytes(&bytes)
                    {
                        providers.push(gdk::ContentProvider::for_value(&texture.to_value()));
                    }
                }
            }
            Some(gdk::ContentProvider::new_union(&providers))
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
        let active_items = active_items.clone();
        source.connect_drag_begin(move |_, drag| {
            let icon = gtk::Image::from_icon_name("text-x-generic-symbolic");
            icon.set_pixel_size(32);
            let count = gtk::Label::new(Some(&active_items.borrow().len().to_string()));
            count.add_css_class("drag-count");
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            content.add_css_class("drag-preview");
            content.append(&icon);
            content.append(&count);
            gtk::DragIcon::for_drag(drag).set_child(Some(&content));
        });
    }
    {
        let ui = ui.clone();
        source.connect_drag_end(move |_, drag, _delete_data| {
            ui.drag_active.set(false);
            let accepted = !cancelled.get() && !drag.selected_action().is_empty();
            let ids: Vec<Uuid> = active_items.borrow().iter().map(|(id, _)| *id).collect();
            if accepted {
                match ui.model.borrow_mut().remove_ids_after_drop(&ids) {
                    Ok(_) => {
                        let mut selected = ui.selected.borrow_mut();
                        for id in ids {
                            selected.remove(&id);
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
    {
        let ui = ui.clone();
        let monitor = monitor.clone();
        let drop_widget = widget.as_ref().clone();
        target.connect_enter(move |_, _, _| {
            drop_widget.add_css_class("drop-active");
            if reveal_on_enter {
                ui.show(monitor.as_ref());
            }
            gdk::DragAction::COPY
        });
    }
    {
        let drop_widget = widget.as_ref().clone();
        target.connect_leave(move |_| drop_widget.remove_css_class("drop-active"));
    }
    {
        let ui = ui.clone();
        let drop_widget = widget.as_ref().clone();
        let monitor = monitor.clone();
        target.connect_drop(move |_, value, _, _| {
            drop_widget.remove_css_class("drop-active");
            let deduplicate_items = ui.settings.borrow().deduplicate_items;
            let result = if let Ok(files) = value.get::<gdk::FileList>() {
                let payload = DropPayload::from_files(files.files());
                add_drop_payload(&mut ui.model.borrow_mut(), payload, deduplicate_items)
            } else if let Ok(text) = value.get::<String>() {
                if let Some(payload) = DropPayload::from_uri_list(&text) {
                    add_drop_payload(&mut ui.model.borrow_mut(), payload, deduplicate_items)
                } else {
                    ui.model
                        .borrow_mut()
                        .add_text(&text)
                        .map(|added| AddReport {
                            added: usize::from(added),
                            ..AddReport::default()
                        })
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
                    .add_managed_path_with_mime(
                        path,
                        tr("image_snippet").to_owned(),
                        Some("image/png".to_owned()),
                    )
                    .map(|added| AddReport {
                        added: usize::from(added),
                        ..AddReport::default()
                    })
            } else {
                return false;
            };
            match result {
                Ok(report) => {
                    ui.present_drop_report(report, monitor.as_ref());
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

#[derive(Debug, Default, Eq, PartialEq)]
struct DropPayload {
    paths: Vec<PathBuf>,
    remote_uris: Vec<String>,
    rejected: usize,
}

impl DropPayload {
    fn from_files(files: Vec<gio::File>) -> Self {
        let mut payload = Self::default();
        for file in files {
            // GTK resolves completed file promises to local paths. Browser HTTP(S)
            // references remain lightweight shortcuts; other remote schemes are
            // counted as rejected instead of being silently treated as local files.
            if let Some(path) = file.path() {
                payload.paths.push(path);
                continue;
            }
            payload.push_uri(file.uri().as_str());
        }
        payload
    }

    fn from_uri_list(text: &str) -> Option<Self> {
        let mut payload = Self::default();
        let mut found_uri = false;
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !looks_like_uri(line) {
                return None;
            }
            found_uri = true;
            payload.push_uri(line);
        }
        found_uri.then_some(payload)
    }

    fn push_uri(&mut self, uri: &str) {
        if uri
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
        {
            if let Some(path) = gio::File::for_uri(uri).path() {
                self.paths.push(path);
            } else {
                self.rejected += 1;
            }
        } else if is_web_uri(uri) {
            self.remote_uris.push(uri.to_owned());
        } else {
            self.rejected += 1;
        }
    }
}

fn add_drop_payload(
    model: &mut ShelfModel,
    payload: DropPayload,
    deduplicate_items: bool,
) -> std::io::Result<AddReport> {
    let mut report = model.add_paths_report_with_deduplication(payload.paths, deduplicate_items)?;
    report.rejected += payload.rejected;
    for uri in payload.remote_uris {
        report.merge(model.add_remote_uri_report_with_deduplication(&uri, deduplicate_items)?);
    }
    Ok(report)
}

fn looks_like_uri(value: &str) -> bool {
    let Some((scheme, remainder)) = value.split_once(':') else {
        return false;
    };
    !remainder.is_empty()
        && !scheme.is_empty()
        && scheme.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'A'..=b'Z' => true,
            b'0'..=b'9' | b'+' | b'-' | b'.' => index > 0,
            _ => false,
        })
}

fn is_web_uri(uri: &str) -> bool {
    uri.get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || uri
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

fn install_keyboard(ui: &Rc<Ui>) {
    let keys = gtk::EventControllerKey::new();
    let ui_for_keys = ui.clone();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let Some(action) = keyboard_action(key, modifiers) else {
            return glib::Propagation::Proceed;
        };
        match action {
            KeyboardAction::Hide => ui_for_keys.hide(),
            KeyboardAction::Remove => ui_for_keys.remove_selected(),
            KeyboardAction::Preview => ui_for_keys.preview_selected(),
            KeyboardAction::Copy => ui_for_keys.copy_selected(),
            KeyboardAction::SelectAll => {
                ui_for_keys.list.select_all();
                update_selection_accessibility(&ui_for_keys.list);
            }
            KeyboardAction::Navigate(navigation) => ui_for_keys.navigate_items(
                navigation,
                modifiers.contains(gdk::ModifierType::SHIFT_MASK),
            ),
        }
        glib::Propagation::Stop
    });
    ui.shelf.add_controller(keys);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Navigation {
    Previous,
    Next,
    First,
    Last,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyboardAction {
    Hide,
    Remove,
    Preview,
    Copy,
    SelectAll,
    Navigate(Navigation),
}

fn keyboard_action(key: gdk::Key, modifiers: gdk::ModifierType) -> Option<KeyboardAction> {
    let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
    if control && matches!(key, gdk::Key::a | gdk::Key::A) {
        return Some(KeyboardAction::SelectAll);
    }
    if control && matches!(key, gdk::Key::c | gdk::Key::C) {
        return Some(KeyboardAction::Copy);
    }
    if control
        || modifiers.intersects(
            gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::META_MASK
                | gdk::ModifierType::SUPER_MASK,
        )
    {
        return None;
    }
    match key {
        gdk::Key::Escape => Some(KeyboardAction::Hide),
        gdk::Key::Delete | gdk::Key::KP_Delete | gdk::Key::BackSpace => {
            Some(KeyboardAction::Remove)
        }
        gdk::Key::space | gdk::Key::Return | gdk::Key::KP_Enter => Some(KeyboardAction::Preview),
        gdk::Key::Up | gdk::Key::KP_Up => Some(KeyboardAction::Navigate(Navigation::Previous)),
        gdk::Key::Down | gdk::Key::KP_Down => Some(KeyboardAction::Navigate(Navigation::Next)),
        gdk::Key::Home | gdk::Key::KP_Home => Some(KeyboardAction::Navigate(Navigation::First)),
        gdk::Key::End | gdk::Key::KP_End => Some(KeyboardAction::Navigate(Navigation::Last)),
        _ => None,
    }
}

fn navigation_target(
    current: Option<usize>,
    item_count: usize,
    navigation: Navigation,
) -> Option<usize> {
    if item_count == 0 {
        return None;
    }
    let last = item_count - 1;
    Some(match navigation {
        Navigation::Previous => current.unwrap_or(0).min(last).saturating_sub(1),
        Navigation::Next => current.map_or(0, |index| index.saturating_add(1).min(last)),
        Navigation::First => 0,
        Navigation::Last => last,
    })
}

fn paths_as_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n")
}

fn set_button_accessibility(button: &gtk::Button, label: &str, shortcuts: &str) {
    button.set_accessible_role(gtk::AccessibleRole::Button);
    if shortcuts.is_empty() {
        button.update_property(&[gtk::accessible::Property::Label(label)]);
    } else {
        button.update_property(&[
            gtk::accessible::Property::Label(label),
            gtk::accessible::Property::KeyShortcuts(shortcuts),
        ]);
    }
}

fn attach_row_action_reveal(row: &gtk::ListBoxRow, actions: &gtk::Box) {
    let hovered = Rc::new(Cell::new(false));
    let focused = Rc::new(Cell::new(false));

    let motion = gtk::EventControllerMotion::new();
    {
        let actions = actions.clone();
        let hovered = hovered.clone();
        motion.connect_enter(move |_, _, _| {
            hovered.set(true);
            actions.add_css_class("actions-revealed");
        });
    }
    {
        let actions = actions.clone();
        let hovered = hovered.clone();
        let focused = focused.clone();
        motion.connect_leave(move |_| {
            hovered.set(false);
            if !focused.get() {
                actions.remove_css_class("actions-revealed");
            }
        });
    }
    row.add_controller(motion);

    let focus = gtk::EventControllerFocus::new();
    {
        let actions = actions.clone();
        let focused = focused.clone();
        focus.connect_enter(move |_| {
            focused.set(true);
            actions.add_css_class("actions-revealed");
        });
    }
    {
        let actions = actions.clone();
        focus.connect_leave(move |_| {
            focused.set(false);
            if !hovered.get() {
                actions.remove_css_class("actions-revealed");
            }
        });
    }
    row.add_controller(focus);
}

fn configure_preview_window(window: &gtk::Window) {
    window.set_accessible_role(gtk::AccessibleRole::Dialog);
    window.update_property(&[
        gtk::accessible::Property::Label("Item preview"),
        gtk::accessible::Property::Description("Press Escape to close the preview."),
        gtk::accessible::Property::KeyShortcuts("Escape"),
    ]);
    let keys = gtk::EventControllerKey::new();
    let window_for_keys = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            window_for_keys.close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);
}

fn update_selection_accessibility(list: &gtk::ListBox) {
    let selected: HashSet<i32> = list
        .selected_rows()
        .into_iter()
        .map(|row| row.index())
        .collect();
    let mut child = list.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Ok(row) = widget.downcast::<gtk::ListBoxRow>() {
            row.update_state(&[gtk::accessible::State::Selected(Some(
                selected.contains(&row.index()),
            ))]);
        }
    }
}

fn item_icon(path: &Path) -> gtk::Widget {
    if is_image(path) {
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_size_request(38, 38);
        let path = path.to_path_buf();
        if let Some(texture) = THUMBNAIL_CACHE.with(|cache| cache.borrow().get(&path).cloned()) {
            picture.set_paintable(Some(&texture));
        } else {
            let file = gio::File::for_path(&path);
            let within_cap = file
                .query_info(
                    "standard::size",
                    gio::FileQueryInfoFlags::NONE,
                    gio::Cancellable::NONE,
                )
                .is_ok_and(|info| info.size() <= 20 * 1024 * 1024);
            if within_cap {
                let picture = picture.clone();
                glib::spawn_future_local(async move {
                    let Ok((bytes, _)) = file.load_bytes_future().await else {
                        return;
                    };
                    let Ok(texture) = gdk::Texture::from_bytes(&bytes) else {
                        return;
                    };
                    picture.set_paintable(Some(&texture));
                    THUMBNAIL_CACHE.with(|cache| {
                        let mut cache = cache.borrow_mut();
                        if cache.len() >= 128 {
                            cache.clear();
                        }
                        cache.insert(path, texture);
                    });
                });
            }
        }
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

fn attach_context_menu(widget: &impl IsA<gtk::Widget>, ui: &Rc<Ui>, id: Uuid, path: PathBuf) {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(true);
    popover.set_parent(widget);
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let open = gtk::Button::with_label(tr("open"));
    let reveal = gtk::Button::with_label(tr("reveal"));
    let copy = gtk::Button::with_label(tr("copy_path"));
    let remove = gtk::Button::with_label(tr("remove"));
    let clear = gtk::Button::with_label(tr("clear_unpinned"));
    for button in [&open, &reveal, &copy, &remove, &clear] {
        button.add_css_class("flat");
        button.add_css_class("context-action");
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
        remove.connect_clicked(move |_| {
            let index = ui
                .model
                .borrow()
                .items()
                .iter()
                .position(|item| item.id == id);
            if let Some(index) = index
                && let Err(error) = ui.model.borrow_mut().remove(index)
            {
                eprintln!("yeet: {error}");
            }
            ui.selected.borrow_mut().remove(&id);
            ui.refresh();
            ui.hide_if_empty();
        });
    }
    {
        let ui = ui.clone();
        clear.connect_clicked(move |_| ui.clear_unpinned());
    }
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_SECONDARY);
    let popover_for_click = popover.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        popover_for_click.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover_for_click.popup();
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    widget.add_controller(click);
    let long_press = gtk::GestureLongPress::new();
    long_press.connect_pressed(move |gesture, x, y| {
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    widget.add_controller(long_press);
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

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

#[derive(Debug, Eq, PartialEq)]
struct MimePayload {
    mime_type: String,
    bytes: Vec<u8>,
}

fn snippet_drag_payload(item: &ShelfItem) -> std::io::Result<Option<MimePayload>> {
    let mime_type = item
        .mime_type
        .clone()
        .or_else(|| legacy_snippet_mime_type(item));
    let Some(mime_type) = mime_type else {
        return Ok(None);
    };
    Ok(Some(MimePayload {
        mime_type,
        bytes: fs::read(&item.path)?,
    }))
}

fn legacy_snippet_mime_type(item: &ShelfItem) -> Option<String> {
    if !item.managed {
        return None;
    }
    let extension = item.path.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("txt") {
        Some("text/plain".to_owned())
    } else if extension.eq_ignore_ascii_case("png") {
        Some("image/png".to_owned())
    } else {
        None
    }
}

fn render_pdf_first_page(path: &Path) -> std::io::Result<PathBuf> {
    render_pdf_first_page_with(path, |path, base| {
        std::process::Command::new("pdftoppm")
            .args([
                "-f",
                "1",
                "-l",
                "1",
                "-singlefile",
                "-scale-to",
                "1400",
                "-png",
            ])
            .arg(path)
            .arg(base)
            .status()
            .map(|status| status.success())
    })
}

fn render_pdf_first_page_with(
    path: &Path,
    renderer: impl FnOnce(&Path, &Path) -> std::io::Result<bool>,
) -> std::io::Result<PathBuf> {
    let directory = std::env::temp_dir().join("yeet/previews");
    fs::create_dir_all(&directory)?;
    let base = directory.join(format!("preview-{}", Uuid::new_v4()));
    let preview = base.with_extension("png");
    let rendered = renderer(path, &base);
    match rendered {
        Ok(true) if preview.is_file() => Ok(preview),
        Ok(true) => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "PDF renderer succeeded without producing a preview image",
        )),
        Ok(false) => {
            cleanup_preview_file(&preview);
            Err(std::io::Error::other("PDF renderer exited unsuccessfully"))
        }
        Err(error) => {
            cleanup_preview_file(&preview);
            Err(error)
        }
    }
}

fn cleanup_preview_file(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "yeet: could not remove preview file {}: {error}",
            path.display()
        );
    }
}

fn clipboard_is_sensitive(clipboard: &gdk::Clipboard) -> bool {
    clipboard.formats().mime_types().iter().any(|mime| {
        let mime = mime.to_ascii_lowercase();
        mime.contains("password")
            || mime.contains("secret")
            || mime.contains("keepass")
            || mime.contains("1password")
            || mime.contains("bitwarden")
            || mime.contains("concealed")
    })
}

fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".yeet-shelf { background: alpha(@window_bg_color, 0.96); border: 1px solid alpha(@accent_color, 0.55); border-radius: 12px; }\n\
         .yeet-edge { background: alpha(@accent_color, 0.04); }\n\
         .yeet-edge:drop(active) { background: alpha(@accent_color, 0.65); }\n\
         .yeet-shelf.drop-active { border: 3px solid @accent_color; background: alpha(@accent_bg_color, 0.16); }\n\
         .yeet-shelf.duplicate { border: 3px solid @warning_color; }\n\
         .boxed-list row.duplicate { background: alpha(@warning_color, 0.30); outline: 3px solid @warning_color; outline-offset: -3px; }\n\
         .title { font-weight: 800; letter-spacing: 2px; }\n\
         .drag-preview { padding: 8px; border-radius: 10px; background: @theme_bg_color; border: 1px solid @theme_selected_bg_color; }\n\
         .drag-count { min-width: 20px; min-height: 20px; border-radius: 10px; color: @theme_selected_fg_color; background: @theme_selected_bg_color; font-weight: bold; }\n\
         .boxed-list row { border-radius: 8px; margin-bottom: 5px; transition: 160ms ease-in-out; }\n\
         .row-actions { opacity: 0.42; transition: opacity 160ms ease-in-out; }\n\
         .row-actions.actions-revealed, .boxed-list row:selected .row-actions { opacity: 1; }\n\
         .row-actions.no-motion { transition: none; }\n\
         .row-action, .touch-target { min-width: 44px; min-height: 44px; padding: 8px; }\n\
         .context-action { min-height: 44px; padding: 8px 12px; }\n\
         .boxed-list row:selected { background: @theme_selected_bg_color; color: @theme_selected_fg_color; }\n\
         .boxed-list row:focus-visible, button:focus-visible, switch:focus-visible, spinbutton:focus-visible, dropdown:focus-visible { outline: 3px solid @theme_selected_bg_color; outline-offset: -3px; }\n\
         .boxed-list row:selected:focus-visible { outline-color: @theme_selected_fg_color; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_shortcuts_cover_all_shelf_operations() {
        let none = gdk::ModifierType::empty();
        let control = gdk::ModifierType::CONTROL_MASK;

        assert_eq!(
            keyboard_action(gdk::Key::Down, none),
            Some(KeyboardAction::Navigate(Navigation::Next))
        );
        assert_eq!(
            keyboard_action(gdk::Key::Up, none),
            Some(KeyboardAction::Navigate(Navigation::Previous))
        );
        assert_eq!(
            keyboard_action(gdk::Key::Delete, none),
            Some(KeyboardAction::Remove)
        );
        assert_eq!(
            keyboard_action(gdk::Key::c, control),
            Some(KeyboardAction::Copy)
        );
        assert_eq!(
            keyboard_action(gdk::Key::space, none),
            Some(KeyboardAction::Preview)
        );
        assert_eq!(
            keyboard_action(gdk::Key::Escape, none),
            Some(KeyboardAction::Hide)
        );
        assert_eq!(
            keyboard_action(gdk::Key::a, control),
            Some(KeyboardAction::SelectAll)
        );
    }

    #[test]
    fn navigation_stops_at_boundaries_and_handles_an_empty_shelf() {
        assert_eq!(navigation_target(None, 0, Navigation::Next), None);
        assert_eq!(navigation_target(None, 3, Navigation::Next), Some(0));
        assert_eq!(navigation_target(Some(0), 3, Navigation::Previous), Some(0));
        assert_eq!(navigation_target(Some(0), 3, Navigation::Last), Some(2));
        assert_eq!(navigation_target(Some(2), 3, Navigation::Next), Some(2));
        assert_eq!(navigation_target(Some(2), 3, Navigation::First), Some(0));
    }

    #[test]
    fn copied_paths_keep_model_order_and_are_line_separated() {
        let paths = [PathBuf::from("first.txt"), PathBuf::from("second.txt")];
        assert_eq!(paths_as_text(&paths), "first.txt\nsecond.txt");
    }

    #[test]
    fn uri_list_keeps_multiple_files_directories_and_remote_urls() {
        let first = gio::File::for_path("/tmp/first file.txt").uri();
        let directory = gio::File::for_path("/tmp/a-directory").uri();
        let text = format!(
            "# text/uri-list comment\r\n{first}\r\n{directory}\r\nhttps://example.com/file.pdf\r\nftp://example.com/unsupported\r\n"
        );

        let payload = DropPayload::from_uri_list(&text).unwrap();

        assert_eq!(payload.paths.len(), 2);
        assert_eq!(payload.paths[0], PathBuf::from("/tmp/first file.txt"));
        assert_eq!(payload.paths[1], PathBuf::from("/tmp/a-directory"));
        assert_eq!(payload.remote_uris, vec!["https://example.com/file.pdf"]);
        assert_eq!(payload.rejected, 1);
    }

    #[test]
    fn ordinary_text_containing_a_url_remains_a_text_snippet() {
        assert!(DropPayload::from_uri_list("Download this:\nhttps://example.com/file").is_none());
    }

    #[test]
    fn file_like_browser_drop_preserves_remote_uri_without_fetching_it() {
        let payload =
            DropPayload::from_files(vec![gio::File::for_uri("https://example.com/archive.zip")]);

        assert!(payload.paths.is_empty());
        assert_eq!(payload.remote_uris, vec!["https://example.com/archive.zip"]);
        assert_eq!(payload.rejected, 0);
    }

    #[test]
    fn managed_text_drag_payload_preserves_its_mime_and_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snippet.txt");
        fs::write(&path, "hello from Yeet").unwrap();
        let item = ShelfItem {
            id: Uuid::new_v4(),
            path,
            name: Some("hello from Yeet".to_owned()),
            pinned: false,
            managed: true,
            source_uri: None,
            mime_type: Some("text/plain".to_owned()),
        };

        let payload = snippet_drag_payload(&item).unwrap().unwrap();

        assert_eq!(payload.mime_type, "text/plain");
        assert_eq!(payload.bytes, b"hello from Yeet");
    }

    #[test]
    fn legacy_managed_images_still_drag_as_png() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snippet.png");
        fs::write(&path, b"png bytes").unwrap();
        let item = ShelfItem {
            id: Uuid::new_v4(),
            path,
            name: Some("Image snippet".to_owned()),
            pinned: false,
            managed: true,
            source_uri: None,
            mime_type: None,
        };

        let payload = snippet_drag_payload(&item).unwrap().unwrap();

        assert_eq!(payload.mime_type, "image/png");
        assert_eq!(payload.bytes, b"png bytes");
    }

    #[test]
    fn pdf_preview_renderer_returns_and_cleans_a_generated_page() {
        let generated = Rc::new(RefCell::new(None));
        let generated_for_renderer = generated.clone();

        let preview = render_pdf_first_page_with(Path::new("document.pdf"), move |_, base| {
            let output = base.with_extension("png");
            fs::write(&output, b"preview")?;
            *generated_for_renderer.borrow_mut() = Some(output);
            Ok(true)
        })
        .unwrap();

        assert_eq!(generated.borrow().as_ref(), Some(&preview));
        assert!(preview.is_file());
        cleanup_preview_file(&preview);
        assert!(!preview.exists());
    }

    #[test]
    fn failed_pdf_preview_removes_partial_output() {
        let generated = Rc::new(RefCell::new(None));
        let generated_for_renderer = generated.clone();

        let result = render_pdf_first_page_with(Path::new("document.pdf"), move |_, base| {
            let output = base.with_extension("png");
            fs::write(&output, b"partial preview")?;
            *generated_for_renderer.borrow_mut() = Some(output);
            Ok(false)
        });

        assert!(result.is_err());
        assert!(!generated.borrow().as_ref().unwrap().exists());
    }

    #[test]
    fn unavailable_pdf_renderer_is_reported_for_default_app_fallback() {
        let result = render_pdf_first_page_with(Path::new("DOCUMENT.PDF"), |_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "pdftoppm missing",
            ))
        });

        assert!(is_pdf(Path::new("DOCUMENT.PDF")));
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }
}
