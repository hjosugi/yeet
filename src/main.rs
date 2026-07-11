mod platform;
mod ui;

use gio::ApplicationFlags;
use gio::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use ui::Ui;

const APP_ID: &str = "io.github.hjosugi.Yeet";
const HELP: &str = "Usage: yeet [OPTIONS] [FILE...]\n\n\
Native drag-and-drop shelf for Wayland and Windows.\n\n\
Options:\n  --toggle   Show or hide the shelf\n  --clear    Remove every item\n  --hidden   Start without showing the shelf\n  --help     Show this help\n  --version  Show the version\n";

fn main() -> glib::ExitCode {
    let local_arguments: Vec<_> = std::env::args_os().collect();
    if local_arguments.iter().any(|argument| argument == "--help") {
        print!("{HELP}");
        return glib::ExitCode::SUCCESS;
    }
    if local_arguments
        .iter()
        .any(|argument| argument == "--version")
    {
        println!("Yeet {}", env!("CARGO_PKG_VERSION"));
        return glib::ExitCode::SUCCESS;
    }

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let ui: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));

    {
        let ui = ui.clone();
        app.connect_activate(move |app| {
            if ui.borrow().is_none() {
                *ui.borrow_mut() = Some(Ui::new(app));
            }
        });
    }
    {
        let ui = ui.clone();
        app.connect_command_line(move |app, command| {
            let arguments = command.arguments();
            app.activate();
            let cwd = command.cwd().unwrap_or_else(|| PathBuf::from("."));
            let mut toggle = false;
            let mut clear = false;
            let mut hidden = false;
            let mut paths = Vec::new();
            for argument in arguments.iter().skip(1) {
                if argument == "--toggle" {
                    toggle = true;
                } else if argument == "--clear" {
                    clear = true;
                } else if argument == "--hidden" {
                    hidden = true;
                } else if !argument.to_string_lossy().starts_with('-') {
                    let path = PathBuf::from(argument);
                    paths.push(if path.is_absolute() {
                        path
                    } else {
                        cwd.join(path)
                    });
                }
            }
            if let Some(ui) = ui.borrow().as_ref() {
                ui.handle_arguments(&paths, toggle, clear, hidden);
            }
            glib::ExitCode::SUCCESS
        });
    }

    app.run()
}
