//! Turning a staged directory into a release container.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Result;

/// Write `<root>` as a gzipped tar whose single top-level entry is the
/// directory's own name, which is the shape `yeetup` unpacks.
pub fn tar_gz(root: &Path, archive: &Path) -> Result<()> {
    let name = root
        .file_name()
        .ok_or("the staging directory has no name")?
        .to_string_lossy()
        .into_owned();
    let file = File::create(archive)?;
    let encoder = flate2::write::GzEncoder::new(BufWriter::new(file), flate2::Compression::best());
    let mut builder = tar::Builder::new(encoder);
    builder.follow_symlinks(false);
    builder.append_dir_all(&name, root)?;
    builder.into_inner()?.finish()?;
    Ok(())
}

/// Write `<root>` as a zip with the same single top-level entry.
pub fn zip(root: &Path, archive: &Path) -> Result<()> {
    let name = root
        .file_name()
        .ok_or("the staging directory has no name")?
        .to_string_lossy()
        .into_owned();
    let file = File::create(archive)?;
    let mut writer = zip::ZipWriter::new(BufWriter::new(file));
    add_directory(&mut writer, root, Path::new(&name))?;
    writer.finish()?;
    Ok(())
}

fn add_directory<W: Write + std::io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    source: &Path,
    prefix: &Path,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(source)?.collect::<std::result::Result<_, _>>()?;
    // Deterministic ordering so the same tree always produces the same archive.
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = prefix.join(entry.file_name());
        let name = name.to_string_lossy().replace('\\', "/");
        if entry.file_type()?.is_dir() {
            writer.add_directory(format!("{name}/"), zip::write::SimpleFileOptions::default())?;
            add_directory(writer, &path, Path::new(&name))?;
            continue;
        }
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(mode_of(&path));
        writer.start_file(name, options)?;
        let mut reader = BufReader::new(File::open(&path)?);
        std::io::copy(&mut reader, writer)?;
    }
    Ok(())
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path).map_or(0o644, |metadata| metadata.permissions().mode())
}

#[cfg(not(unix))]
fn mode_of(_path: &Path) -> u32 {
    0o644
}

pub fn sha256(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Write a `sha256sum`-compatible manifest for `files`.
///
/// The format matters: `sha256sum -c` reads it during release verification and
/// `yeetup` parses it before installing anything.
pub fn write_checksums(files: &[std::path::PathBuf], manifest: &Path) -> Result<()> {
    let mut contents = String::new();
    for file in files {
        let name = file
            .file_name()
            .ok_or("checksum input has no file name")?
            .to_string_lossy();
        contents.push_str(&format!("{}  {name}\n", sha256(file)?));
    }
    fs::write(manifest, contents)?;
    Ok(())
}
