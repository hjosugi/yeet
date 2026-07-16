use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShelfItem {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub managed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AddReport {
    pub added: usize,
    pub added_ids: Vec<Uuid>,
    pub duplicates: Vec<PathBuf>,
    pub duplicate_ids: Vec<Uuid>,
    pub rejected: usize,
}

impl AddReport {
    pub fn merge(&mut self, other: Self) {
        self.added += other.added;
        self.rejected += other.rejected;
        self.added_ids.extend(other.added_ids);
        for path in other.duplicates {
            if !self.duplicates.contains(&path) {
                self.duplicates.push(path);
            }
        }
        for id in other.duplicate_ids {
            if !self.duplicate_ids.contains(&id) {
                self.duplicate_ids.push(id);
            }
        }
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
        Self::load_with_deduplication(state_path, true)
    }

    pub fn load_with_deduplication(
        state_path: PathBuf,
        deduplicate_items: bool,
    ) -> io::Result<Self> {
        if !state_path.exists() {
            return Ok(Self {
                items: Vec::new(),
                state_path: Some(state_path),
            });
        }

        let data = fs::read(&state_path)?;
        let mut items: Vec<ShelfItem> = serde_json::from_slice(&data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let stored_count = items.len();
        items.retain(|item| item.path.exists());
        let missing_count = stored_count - items.len();
        if missing_count > 0 {
            eprintln!("yeet: removed {missing_count} missing persisted item(s)");
        }
        ensure_unique_ids(&mut items);
        if deduplicate_items {
            deduplicate(&mut items);
        }
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
        Ok(self.add_paths_report(paths)?.added)
    }

    pub fn add_paths_report<I>(&mut self, paths: I) -> io::Result<AddReport>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        self.add_paths_report_with_deduplication(paths, true)
    }

    pub fn add_paths_report_with_deduplication<I>(
        &mut self,
        paths: I,
        deduplicate_items: bool,
    ) -> io::Result<AddReport>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut known: HashSet<PathBuf> = if deduplicate_items {
            self.items
                .iter()
                .map(|item| comparison_path(&item.path))
                .collect()
        } else {
            HashSet::new()
        };
        let mut report = AddReport::default();

        for path in paths {
            let Some(path) = normalize_existing_path(&path) else {
                report.rejected += 1;
                continue;
            };
            let identity = comparison_path(&path);
            if deduplicate_items && !known.insert(identity.clone()) {
                if let Some(existing) = self
                    .items
                    .iter()
                    .find(|item| comparison_path(&item.path) == identity)
                {
                    report.duplicates.push(existing.path.clone());
                    report.duplicate_ids.push(existing.id);
                } else if !report.duplicates.contains(&path) {
                    // The duplicate may have been inserted earlier in this same batch.
                    report.duplicates.push(path);
                }
                continue;
            }
            let id = Uuid::new_v4();
            self.items.insert(
                0,
                ShelfItem {
                    id,
                    path,
                    name: None,
                    pinned: false,
                    managed: false,
                    source_uri: None,
                    mime_type: None,
                },
            );
            report.added += 1;
            report.added_ids.push(id);
        }

        if report.added > 0 {
            self.save()?;
        }
        Ok(report)
    }

    pub fn remove(&mut self, index: usize) -> io::Result<Option<ShelfItem>> {
        if index >= self.items.len() {
            return Ok(None);
        }
        let item = self.items[index].clone();
        if item.managed
            && let Err(error) = fs::remove_file(&item.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
        self.items.remove(index);
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

    pub fn remove_ids_after_drop(&mut self, ids: &[Uuid]) -> io::Result<usize> {
        let ids: HashSet<Uuid> = ids.iter().copied().collect();
        let indices: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| ids.contains(&item.id).then_some(index))
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
                id: Uuid::new_v4(),
                path,
                name: Some(name),
                pinned: false,
                managed: true,
                source_uri: None,
                mime_type: Some("text/plain".to_owned()),
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
        self.add_managed_path_with_mime(path, name, None)
    }

    pub fn add_managed_path_with_mime(
        &mut self,
        path: PathBuf,
        name: String,
        mime_type: Option<String>,
    ) -> io::Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        self.items.insert(
            0,
            ShelfItem {
                id: Uuid::new_v4(),
                path,
                name: Some(name),
                pinned: false,
                managed: true,
                source_uri: None,
                mime_type,
            },
        );
        self.save()?;
        Ok(true)
    }

    pub fn add_remote_uri(&mut self, uri: &str) -> io::Result<bool> {
        Ok(self.add_remote_uri_report(uri)?.added > 0)
    }

    pub fn add_remote_uri_report(&mut self, uri: &str) -> io::Result<AddReport> {
        self.add_remote_uri_report_with_deduplication(uri, true)
    }

    pub fn add_remote_uri_report_with_deduplication(
        &mut self,
        uri: &str,
        deduplicate_items: bool,
    ) -> io::Result<AddReport> {
        let uri = uri.trim();
        if !is_web_uri(uri) {
            return Ok(AddReport {
                rejected: 1,
                ..AddReport::default()
            });
        }
        if deduplicate_items
            && let Some(item) = self.items.iter().find(|item| remote_uri_matches(item, uri))
        {
            return Ok(AddReport {
                duplicates: vec![item.path.clone()],
                duplicate_ids: vec![item.id],
                ..AddReport::default()
            });
        }
        let path = self.managed_path("url")?;
        fs::write(&path, format!("[InternetShortcut]\nURL={uri}\n"))?;
        let name = uri.chars().take(80).collect();
        let id = Uuid::new_v4();
        self.items.insert(
            0,
            ShelfItem {
                id,
                path,
                name: Some(name),
                pinned: false,
                managed: true,
                source_uri: Some(uri.to_owned()),
                mime_type: None,
            },
        );
        self.save()?;
        Ok(AddReport {
            added: 1,
            added_ids: vec![id],
            ..AddReport::default()
        })
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

fn is_web_uri(uri: &str) -> bool {
    uri.get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || uri
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

fn remote_uri_matches(item: &ShelfItem, uri: &str) -> bool {
    if item.source_uri.as_deref() == Some(uri) {
        return true;
    }
    if !item.managed
        || !item
            .path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("url"))
    {
        return false;
    }
    fs::read_to_string(&item.path).is_ok_and(|contents| {
        contents.lines().any(|line| {
            line.strip_prefix("URL=")
                .is_some_and(|stored| stored.trim() == uri)
        })
    })
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
    let normalized = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    PathBuf::from(normalized.to_string_lossy().to_lowercase())
}

#[cfg(not(windows))]
fn comparison_path(path: &Path) -> PathBuf {
    path.to_owned()
}

fn deduplicate(items: &mut Vec<ShelfItem>) {
    let mut known = HashSet::new();
    items.retain(|item| {
        let identity = item
            .source_uri
            .as_ref()
            .map(|uri| format!("uri:{uri}"))
            .unwrap_or_else(|| format!("path:{}", comparison_path(&item.path).display()));
        known.insert(identity)
    });
}

fn ensure_unique_ids(items: &mut [ShelfItem]) {
    let mut known = HashSet::new();
    for item in items {
        while !known.insert(item.id) {
            item.id = Uuid::new_v4();
        }
    }
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
    fn add_report_identifies_duplicate_files_and_accepts_directories() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("one.txt");
        let folder = directory.path().join("folder");
        fs::write(&file, "one").unwrap();
        fs::create_dir(&folder).unwrap();
        let mut model = ShelfModel::in_memory();

        let report = model
            .add_paths_report([
                file.clone(),
                folder.clone(),
                file,
                directory.path().join("missing"),
            ])
            .unwrap();

        assert_eq!(report.added, 2);
        assert_eq!(report.duplicates.len(), 1);
        assert_eq!(report.rejected, 1);
        assert!(model.items().iter().any(|item| item.path.is_dir()));
    }

    #[test]
    fn remote_urls_are_managed_and_report_the_existing_item_on_duplicate() {
        let directory = tempfile::tempdir().unwrap();
        let mut model = ShelfModel::empty(directory.path().join("state.json"));

        let first = model
            .add_remote_uri_report("https://example.com/download?id=1")
            .unwrap();
        assert_eq!(first.added, 1);
        let path = model.items()[0].path.clone();
        assert_eq!(
            model.items()[0].source_uri.as_deref(),
            Some("https://example.com/download?id=1")
        );

        let duplicate = model
            .add_remote_uri_report("https://example.com/download?id=1")
            .unwrap();
        assert_eq!(duplicate.added, 0);
        assert_eq!(duplicate.duplicates, vec![path]);
        assert_eq!(model.items().len(), 1);

        let unsupported = model
            .add_remote_uri_report("ftp://example.com/file")
            .unwrap();
        assert_eq!(unsupported.rejected, 1);
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
    fn ids_are_stable_drag_identities_and_keep_pinned_items() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        let mut model = ShelfModel::in_memory();
        model.add_paths([first, second]).unwrap();
        model.toggle_pinned(0).unwrap();
        let ids: Vec<Uuid> = model.items().iter().map(|item| item.id).collect();

        assert_eq!(model.remove_ids_after_drop(&ids).unwrap(), 1);
        assert_eq!(model.items().len(), 1);
        assert!(model.items()[0].pinned);
    }

    #[test]
    fn repeated_paths_are_kept_when_deduplication_is_disabled() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("repeat.txt");
        fs::write(&file, "repeat").unwrap();
        let mut model = ShelfModel::in_memory();

        let first = model
            .add_paths_report_with_deduplication([file.clone()], false)
            .unwrap();
        let second = model
            .add_paths_report_with_deduplication([file], false)
            .unwrap();

        assert_eq!(first.added, 1);
        assert_eq!(second.added, 1);
        assert!(second.duplicates.is_empty());
        assert!(second.duplicate_ids.is_empty());
        assert_eq!(model.items().len(), 2);
        assert_ne!(model.items()[0].id, model.items()[1].id);
    }

    #[test]
    fn item_ids_survive_a_reload() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state.json");
        let file = directory.path().join("kept.txt");
        fs::write(&file, "kept").unwrap();
        let mut model = ShelfModel::load(state.clone()).unwrap();
        model.add_paths([file]).unwrap();
        let id = model.items()[0].id;

        let restored = ShelfModel::load(state).unwrap();

        assert_eq!(restored.items().len(), 1);
        assert_eq!(restored.items()[0].id, id);
    }

    #[test]
    fn persisted_items_sharing_an_id_are_given_unique_ids() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state.json");
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        let shared = Uuid::new_v4();
        let stored = serde_json::json!([
            {"id": shared, "path": first},
            {"id": shared, "path": second},
        ]);
        fs::write(&state, serde_json::to_vec(&stored).unwrap()).unwrap();

        let model = ShelfModel::load(state).unwrap();

        assert_eq!(model.items().len(), 2);
        assert_ne!(model.items()[0].id, model.items()[1].id);
    }

    #[test]
    fn managed_text_is_deleted_with_its_item() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state.json");
        let mut model = ShelfModel::load(state).unwrap();
        assert!(model.add_text("hello").unwrap());
        let snippet = model.items()[0].path.clone();
        assert!(snippet.exists());
        assert_eq!(model.items()[0].mime_type.as_deref(), Some("text/plain"));

        model.remove(0).unwrap();
        assert!(!snippet.exists());
    }

    #[test]
    fn managed_image_mime_is_persisted_and_file_is_collected() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state.json");
        let mut model = ShelfModel::load(state.clone()).unwrap();
        let image = model.managed_path("png").unwrap();
        fs::write(&image, b"png bytes").unwrap();

        assert!(
            model
                .add_managed_path_with_mime(
                    image.clone(),
                    "Image snippet".to_owned(),
                    Some("image/png".to_owned()),
                )
                .unwrap()
        );
        assert_eq!(model.items()[0].mime_type.as_deref(), Some("image/png"));
        assert_eq!(
            ShelfModel::load(state).unwrap().items()[0]
                .mime_type
                .as_deref(),
            Some("image/png")
        );

        model.remove(0).unwrap();
        assert!(!image.exists());
    }

    #[test]
    fn managed_gc_failure_keeps_the_item_for_a_retry() {
        let directory = tempfile::tempdir().unwrap();
        let managed_directory = directory.path().join("not-a-file");
        fs::create_dir(&managed_directory).unwrap();
        let mut model = ShelfModel::in_memory();
        assert!(
            model
                .add_managed_path(managed_directory.clone(), "Managed directory".to_owned())
                .unwrap()
        );

        assert!(model.remove(0).is_err());
        assert_eq!(model.items().len(), 1);
        assert!(managed_directory.exists());
    }
}
