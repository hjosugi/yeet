//! Which release artifact this machine needs, and where to get it.
//!
//! Asset names are a contract with the release workflow: `xtask` produces these
//! exact names and `yeetup` consumes them. Keeping the naming in one place on
//! each side means a new target is added by editing two tables, not by hunting
//! through shell scripts.

use crate::Result;

pub const REPOSITORY: &str = "hjosugi/yeet";
pub const USER_AGENT: &str = concat!("yeetup/", env!("CARGO_PKG_VERSION"));

/// Archive container for a target, which decides how it is unpacked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Container {
    TarGz,
    Zip,
}

impl Container {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::TarGz => "tar.gz",
            Self::Zip => "zip",
        }
    }
}

/// The release artifact matching the host this is running on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    /// Appears in the archive name, e.g. `linux-x86_64`.
    pub slug: &'static str,
    /// Groups targets that share a checksum file, e.g. `linux`.
    pub family: &'static str,
    pub container: Container,
}

impl Target {
    pub fn archive_name(&self, version: &str) -> String {
        format!(
            "yeet-{version}-{}.{}",
            self.slug,
            self.container.extension()
        )
    }

    pub fn checksum_name(&self) -> String {
        format!("SHA256SUMS-{}.txt", self.family)
    }

    pub fn download_url(&self, version: &str, file: &str) -> String {
        format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{file}")
    }
}

/// Resolve the current host to a release target.
///
/// An unsupported host is an error rather than a guess: downloading an archive
/// for the wrong architecture would fail later and less clearly.
pub fn host_target() -> Result<Target> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Target {
            slug: "linux-x86_64",
            family: "linux",
            container: Container::TarGz,
        },
        ("linux", "aarch64") => Target {
            slug: "linux-aarch64",
            family: "linux",
            container: Container::TarGz,
        },
        ("windows", "x86_64") => Target {
            slug: "windows-x64",
            family: "windows",
            container: Container::Zip,
        },
        ("macos", "aarch64") => Target {
            slug: "macos-aarch64",
            family: "macos",
            container: Container::TarGz,
        },
        ("macos", "x86_64") => Target {
            slug: "macos-x86_64",
            family: "macos",
            container: Container::TarGz,
        },
        (os, arch) => {
            return Err(format!(
                "no Yeet release is published for {os} on {arch}; build from source instead"
            )
            .into());
        }
    };
    Ok(target)
}

#[derive(serde::Deserialize)]
struct LatestRelease {
    tag_name: String,
}

/// Ask GitHub for the newest published release tag.
pub fn latest_version(agent: &ureq::Agent) -> Result<String> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let release: LatestRelease = agent
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_json()?;
    Ok(release.tag_name.trim_start_matches('v').to_owned())
}

/// Pull one file's hash out of a `sha256sum` manifest.
///
/// The manifest lists every artifact of a family, so the entry has to be
/// matched by name; taking the first line would install a Linux archive's hash
/// for a macOS download.
pub fn checksum_for(manifest: &str, file: &str) -> Result<String> {
    manifest
        .lines()
        .filter_map(|line| line.split_once("  ").or_else(|| line.split_once(" *")))
        .find(|(_, name)| name.trim() == file)
        .map(|(hash, _)| hash.trim().to_ascii_lowercase())
        .ok_or_else(|| format!("{file} is not listed in the published checksums").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_and_checksum_names_follow_the_release_workflow() {
        let linux = Target {
            slug: "linux-x86_64",
            family: "linux",
            container: Container::TarGz,
        };
        assert_eq!(
            linux.archive_name("0.5.3"),
            "yeet-0.5.3-linux-x86_64.tar.gz"
        );
        assert_eq!(linux.checksum_name(), "SHA256SUMS-linux.txt");
        assert_eq!(
            linux.download_url("0.5.3", "yeet-0.5.3-linux-x86_64.tar.gz"),
            "https://github.com/hjosugi/yeet/releases/download/v0.5.3/yeet-0.5.3-linux-x86_64.tar.gz"
        );

        let windows = Target {
            slug: "windows-x64",
            family: "windows",
            container: Container::Zip,
        };
        assert_eq!(windows.archive_name("0.5.3"), "yeet-0.5.3-windows-x64.zip");
        assert_eq!(windows.checksum_name(), "SHA256SUMS-windows.txt");
    }

    #[test]
    fn checksums_are_matched_by_file_name_not_by_position() {
        let manifest = "\
aaaa  yeet-0.5.3-macos-x86_64.tar.gz
bbbb  yeet-0.5.3-macos-aarch64.tar.gz
";
        assert_eq!(
            checksum_for(manifest, "yeet-0.5.3-macos-aarch64.tar.gz").unwrap(),
            "bbbb"
        );
        assert!(checksum_for(manifest, "yeet-0.5.3-linux-x86_64.tar.gz").is_err());
    }

    #[test]
    fn binary_mode_checksum_lines_are_understood() {
        let manifest = "cccc *yeet-0.5.3-windows-x64.zip\n";

        assert_eq!(
            checksum_for(manifest, "yeet-0.5.3-windows-x64.zip").unwrap(),
            "cccc"
        );
    }
}
