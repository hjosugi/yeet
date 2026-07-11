use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShelfItem {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub managed: bool,
}

impl ShelfItem {
    pub fn display_name(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

#[derive(Debug, Default)]
pub struct ShelfModel {
    items: Vec<ShelfItem>,
    state_path: Option<PathBuf>,
}

impl ShelfModel {
    pub fn empty(state_path: PathBuf) -> Self {
        Self {
            items: Vec::new(),
            state_path: Some(state_path),
        }
    }

    pub fn load(state_path: PathBuf) -> io::Result<Self> {
        if !state_path.exists() {
            return Ok(Self {
                items: Vec::new(),
                state_path: Some(state_path),
            });
        }

        let data = fs::read(&state_path)?;
        let mut items: Vec<ShelfItem> = serde_json::from_slice(&data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        items.retain(|item| item.path.exists());
        deduplicate(&mut items);
        Ok(Self {
            items,
            state_path: Some(state_path),
        })
    }

    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn items(&self) -> &[ShelfItem] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn add_paths<I>(&mut self, paths: I) -> io::Result<usize>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut known: HashSet<PathBuf> = self
            .items
            .iter()
            .map(|item| comparison_path(&item.path))
            .collect();
        let mut added = 0;

        for path in paths {
            let Some(path) = normalize_existing_path(&path) else {
                continue;
            };
            if !known.insert(comparison_path(&path)) {
                continue;
            }
            self.items.insert(
                0,
                ShelfItem {
                    path,
                    name: None,
                    pinned: false,
                    managed: false,
                },
            );
            added += 1;
        }

        if added > 0 {
            self.save()?;
        }
        Ok(added)
    }

    pub fn remove(&mut self, index: usize) -> io::Result<Option<ShelfItem>> {
        if index >= self.items.len() {
            return Ok(None);
        }
        let item = self.items.remove(index);
        if item.managed {
            let _ = fs::remove_file(&item.path);
        }
        self.save()?;
        Ok(Some(item))
    }

    pub fn remove_after_drop(&mut self, indices: &[usize]) -> io::Result<usize> {
        let mut indices: Vec<usize> = indices.to_vec();
        indices.sort_unstable();
        indices.dedup();

        let mut removed = 0;
        for index in indices.into_iter().rev() {
            if self.items.get(index).is_some_and(|item| !item.pinned) {
                self.remove(index)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn remove_paths_after_drop(&mut self, paths: &[PathBuf]) -> io::Result<usize> {
        let paths: HashSet<PathBuf> = paths.iter().map(|path| comparison_path(path)).collect();
        let indices: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                paths
                    .contains(&comparison_path(&item.path))
                    .then_some(index)
            })
            .collect();
        self.remove_after_drop(&indices)
    }

    pub fn clear_unpinned(&mut self) -> io::Result<usize> {
        let indices: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (!item.pinned).then_some(index))
            .collect();
        self.remove_after_drop(&indices)
    }

    pub fn clear_all(&mut self) -> io::Result<usize> {
        let count = self.items.len();
        while !self.items.is_empty() {
            self.remove(self.items.len() - 1)?;
        }
        Ok(count)
    }

    pub fn add_text(&mut self, text: &str) -> io::Result<bool> {
        if text.trim().is_empty() {
            return Ok(false);
        }
        let base = self
            .state_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("yeet"));
        let snippets = base.join("snippets");
        fs::create_dir_all(&snippets)?;
        let path = snippets.join(format!("snippet-{}.txt", Uuid::new_v4()));
        fs::write(&path, text)?;
        let name = text
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim().chars().take(64).collect())
            .unwrap_or_else(|| "Text snippet".to_owned());
        self.items.insert(
            0,
            ShelfItem {
                path,
                name: Some(name),
                pinned: false,
                managed: true,
            },
        );
        self.save()?;
        Ok(true)
    }

    pub fn managed_path(&self, extension: &str) -> io::Result<PathBuf> {
        let base = self
            .state_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("yeet"));
        let snippets = base.join("snippets");
        fs::create_dir_all(&snippets)?;
        Ok(snippets.join(format!(
            "snippet-{}.{}",
            Uuid::new_v4(),
            extension.trim_start_matches('.')
        )))
    }

    pub fn add_managed_path(&mut self, path: PathBuf, name: String) -> io::Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        self.items.insert(
            0,
            ShelfItem {
                path,
                name: Some(name),
                pinned: false,
                managed: true,
            },
        );
        self.save()?;
        Ok(true)
    }

    pub fn add_remote_uri(&mut self, uri: &str) -> io::Result<bool> {
        if !(uri.starts_with("https://") || uri.starts_with("http://")) {
            return Ok(false);
        }
        let path = self.managed_path("url")?;
        fs::write(&path, format!("[InternetShortcut]\nURL={uri}\n"))?;
        let name = uri.chars().take(80).collect();
        self.add_managed_path(path, name)
    }

    pub fn toggle_pinned(&mut self, index: usize) -> io::Result<bool> {
        let Some(item) = self.items.get_mut(index) else {
            return Ok(false);
        };
        item.pinned = !item.pinned;
        self.save()?;
        Ok(true)
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let data = serde_json::to_vec_pretty(&self.items).map_err(io::Error::other)?;
        fs::write(&temporary, data)?;
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(temporary, path)
    }
}

fn normalize_existing_path(path: &Path) -> Option<PathBuf> {
    path.exists().then(|| {
        fs::canonicalize(path).unwrap_or_else(|_| {
            if path.is_absolute() {
                path.to_owned()
            } else {
                std::env::current_dir().unwrap_or_default().join(path)
            }
        })
    })
}

#[cfg(windows)]
fn comparison_path(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

#[cfg(not(windows))]
fn comparison_path(path: &Path) -> PathBuf {
    path.to_owned()
}

fn deduplicate(items: &mut Vec<ShelfItem>) {
    let mut known = HashSet::new();
    items.retain(|item| known.insert(comparison_path(&item.path)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_rejects_missing_and_duplicate_paths() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("one.txt");
        fs::write(&file, "one").unwrap();
        let mut model = ShelfModel::in_memory();

        let added = model
            .add_paths([file.clone(), file.clone(), directory.path().join("missing")])
            .unwrap();

        assert_eq!(added, 1);
        assert_eq!(model.items().len(), 1);
    }

    #[test]
    fn pinned_items_survive_drop_and_clear() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        let mut model = ShelfModel::in_memory();
        model.add_paths([first, second]).unwrap();
        model.toggle_pinned(0).unwrap();

        assert_eq!(model.remove_after_drop(&[0, 1]).unwrap(), 1);
        assert_eq!(model.clear_unpinned().unwrap(), 0);
        assert_eq!(model.items().len(), 1);
        assert!(model.items()[0].pinned);
    }

    #[test]
    fn state_is_restored_and_missing_entries_are_removed() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state.json");
        let file = directory.path().join("kept.txt");
        fs::write(&file, "kept").unwrap();
        let mut model = ShelfModel::load(state.clone()).unwrap();
        model.add_paths([file.clone()]).unwrap();

        let restored = ShelfModel::load(state.clone()).unwrap();
        assert_eq!(restored.items().len(), 1);

        fs::remove_file(file).unwrap();
        let restored = ShelfModel::load(state).unwrap();
        assert!(restored.is_empty());
    }

    #[test]
    fn paths_are_stable_drag_identities() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        let mut model = ShelfModel::in_memory();
        model.add_paths([first.clone(), second.clone()]).unwrap();
        model.toggle_pinned(0).unwrap();

        assert_eq!(model.remove_paths_after_drop(&[first, second]).unwrap(), 1);
        assert_eq!(model.items().len(), 1);
        assert!(model.items()[0].pinned);
    }

    #[test]
    fn managed_text_is_deleted_with_its_item() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state.json");
        let mut model = ShelfModel::load(state).unwrap();
        assert!(model.add_text("hello").unwrap());
        let snippet = model.items()[0].path.clone();
        assert!(snippet.exists());

        model.remove(0).unwrap();
        assert!(!snippet.exists());
    }
}
