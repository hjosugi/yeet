//! Yeet's installer and updater.
//!
//! One binary that installs, updates and removes Yeet on Linux, Windows and
//! macOS. It downloads the release archive published for the host, verifies it
//! against the release checksums, unpacks it into a prefix and records exactly
//! which files it wrote so a later update or uninstall touches nothing else.

mod archive;
mod fetch;
mod integrate;
mod layout;
mod release;

use std::path::PathBuf;

use layout::{Manifest, Scope};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const HELP: &str = "\
Usage: yeetup <COMMAND> [OPTIONS]

Install, update and remove Yeet.

Commands:
  install      Download and install Yeet
  update       Install the newest release over the current one
  uninstall    Remove the files this tool installed
  status       Show what is installed and whether it is current

Options:
  --version <V>   Install a specific version instead of the newest
  --prefix <DIR>  Install under DIR instead of the default for the scope
  --system        Install machine-wide (needs root or administrator rights)
  --help          Show this help
  --self-version  Show the yeetup version
";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("yeetup: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Default)]
struct Options {
    version: Option<String>,
    prefix: Option<PathBuf>,
    scope: Scope,
}

fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let Some(command) = arguments.next() else {
        print!("{HELP}");
        return Ok(());
    };
    if command == "--help" || command == "-h" {
        print!("{HELP}");
        return Ok(());
    }
    if command == "--self-version" {
        println!("yeetup {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut options = Options::default();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--version" => {
                options.version = Some(
                    arguments
                        .next()
                        .ok_or("--version needs a value, for example --version 0.5.3")?,
                );
            }
            "--prefix" => {
                options.prefix = Some(PathBuf::from(
                    arguments.next().ok_or("--prefix needs a directory")?,
                ));
            }
            "--system" => options.scope = Scope::System,
            "--user" => options.scope = Scope::User,
            other => return Err(format!("unknown option {other}").into()),
        }
    }

    match command.as_str() {
        "install" => install(options),
        "update" => update(options),
        "uninstall" => uninstall(),
        "status" => status(),
        other => Err(format!("unknown command {other}; run `yeetup --help`").into()),
    }
}

fn install(options: Options) -> Result<()> {
    let target = release::host_target()?;
    let agent = fetch::agent();
    let version = match options.version {
        Some(version) => version.trim_start_matches('v').to_owned(),
        None => {
            println!("Looking up the newest Yeet release…");
            release::latest_version(&agent)?
        }
    };
    let prefix = match options.prefix {
        Some(prefix) => prefix,
        None => layout::default_prefix(options.scope)?,
    };

    let archive_name = target.archive_name(&version);
    let checksum_name = target.checksum_name();
    println!("Installing Yeet {version} into {}", prefix.display());

    let checksums = fetch::get_text(&agent, &target.download_url(&version, &checksum_name))?;
    let expected = release::checksum_for(&checksums, &archive_name)?;

    let staging = tempdir()?;
    let archive_path = staging.path().join(&archive_name);
    println!("Downloading {archive_name}");
    let actual = fetch::download_to(
        &agent,
        &target.download_url(&version, &archive_name),
        &archive_path,
    )?;
    fetch::verify(&actual, &expected, &archive_name)?;
    println!("Checksum verified");

    let unpacked = staging.path().join("unpacked");
    archive::unpack(&archive_path, target.container, &unpacked)?;

    // Remove the previous install before laying down the new one so files that
    // a release dropped do not survive as orphans.
    if let Some(previous) = Manifest::load()?
        && previous.prefix == prefix
    {
        layout::remove_installed(&previous)?;
    }

    let files = layout::install_tree(&unpacked, &prefix)?;
    Manifest {
        version: version.clone(),
        scope: options.scope,
        prefix: prefix.clone(),
        files,
    }
    .save()?;

    println!(
        "Installed Yeet {version} ({})",
        layout::executable_path(&prefix).display()
    );
    integrate::finish(&prefix)?;
    Ok(())
}

fn update(mut options: Options) -> Result<()> {
    let Some(current) = Manifest::load()? else {
        return Err("Yeet was not installed by yeetup; run `yeetup install` first".into());
    };
    if options.version.is_none() {
        let latest = release::latest_version(&fetch::agent())?;
        if latest == current.version {
            println!("Yeet {} is already the newest release", current.version);
            return Ok(());
        }
        println!("Updating Yeet {} to {latest}", current.version);
        options.version = Some(latest);
    }
    // Keep the existing location unless the caller asked to move it.
    options.prefix = options.prefix.or(Some(current.prefix));
    options.scope = current.scope;
    install(options)
}

fn uninstall() -> Result<()> {
    let Some(manifest) = Manifest::load()? else {
        return Err("nothing to uninstall: no yeetup install record was found".into());
    };
    layout::remove_installed(&manifest)?;
    Manifest::forget()?;
    integrate::refresh_caches(&manifest.prefix);
    println!(
        "Removed Yeet {} from {}",
        manifest.version,
        manifest.prefix.display()
    );
    Ok(())
}

fn status() -> Result<()> {
    let Some(manifest) = Manifest::load()? else {
        println!("Yeet is not installed by yeetup.");
        return Ok(());
    };
    println!("Yeet {} ({:?} scope)", manifest.version, manifest.scope);
    println!("  prefix: {}", manifest.prefix.display());
    println!("  files:  {}", manifest.files.len());
    match release::latest_version(&fetch::agent()) {
        Ok(latest) if latest == manifest.version => {
            println!("  newest release: {latest} (current)")
        }
        Ok(latest) => println!("  newest release: {latest} — run `yeetup update`"),
        Err(error) => println!("  newest release: unknown ({error})"),
    }
    Ok(())
}

/// A self-deleting scratch directory for the download and unpack steps.
struct TempDir(PathBuf);

impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tempdir() -> Result<TempDir> {
    let base = std::env::temp_dir().join(format!("yeetup-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    Ok(TempDir(base))
}
