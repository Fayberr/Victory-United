//! Versioned save snapshots, ZIP exchange, integrity checks and recoverable restore.
//! Snapshot payloads contain title saves and persistent local storage only.
use super::{runner, storage, store};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Instant, UNIX_EPOCH},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MANIFEST: &str = "manifest.json";
const FILES: &str = "files";
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;
const MAX_BYTES: u64 = 25 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
const MAX_LABEL: usize = 120;
const DAY_SECS: u64 = 86_400;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    BeforeLaunch,
    AfterExit,
    Manual,
    BeforeRestore,
    BeforeMove,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Self::BeforeLaunch => "before-launch",
            Self::AfterExit => "after-exit",
            Self::Manual => "manual",
            Self::BeforeRestore => "before-restore",
            Self::BeforeMove => "before-move",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct Retention {
    pub keep_recent: u32,
    pub keep_days: u32,
    pub max_gib: u32,
}

impl Default for Retention {
    fn default() -> Self {
        Self { keep_recent: 8, keep_days: 14, max_gib: 5 }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SavedFile {
    pub path: String,
    pub size: u64,
    pub modified_ns: Option<u64>,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub schema: u32,
    pub created: u64,
    pub kind: Kind,
    pub engine_build: String,
    pub launcher_version: String,
    pub label: String,
    pub total_size: u64,
    pub files: Vec<SavedFile>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    pub manifest: Manifest,
}

fn persistent(path: &Path) -> bool {
    let mut parts = path.iter();
    let title = parts.next().and_then(|p| p.to_str()).unwrap_or_default();
    title.len() == 8 && title.bytes().all(|b| b.is_ascii_hexdigit())
        && parts.next().is_none_or(|p| p == "saves" || p == "storage")
}

/// A ZIP name is treated as a Windows pathname even when imported on Linux.
fn safe_path(path: &str) -> Result<PathBuf> {
    ensure!(path.len() <= 1024, "Snapshot path is too long");
    let mut clean = PathBuf::new();
    for name in path.split('/') {
        let stem = name.split('.').next().unwrap_or_default().to_uppercase();
        let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
            || (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.get(3..), Some("0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));
        ensure!(
            !name.is_empty() && !name.ends_with(['.', ' ']) && !device
                && !name.chars().any(|c| c.is_control() || "\\:<>|\"?*".contains(c)),
            "Unsafe snapshot path: {path}"
        );
        clean.push(name);
    }
    ensure!((3..64).contains(&clean.components().count()) && persistent(&clean), "Not a persistent title file: {path}");
    Ok(clean)
}

fn path_text(path: &Path) -> Result<String> {
    let parts = path.iter().map(|p| p.to_str().context("Save file path is not Unicode")).collect::<Result<Vec<_>>>()?;
    let text = parts.join("/");
    safe_path(&text)?;
    Ok(text)
}

fn label_text(label: &str) -> Result<String> {
    let label = label.trim();
    ensure!(label.chars().count() <= MAX_LABEL && !label.chars().any(char::is_control), "Use at most 120 characters for the label.");
    Ok(label.to_string())
}

fn validate(manifest: &Manifest) -> Result<()> {
    ensure!(manifest.schema == 1, "Unsupported backup manifest version");
    ensure!(!manifest.files.is_empty() && manifest.files.len() <= MAX_FILES, "Invalid number of backup files");
    label_text(&manifest.label)?;
    ensure!(manifest.engine_build.len() <= 256 && manifest.launcher_version.len() <= 64, "Invalid build or launcher version");
    let mut paths = BTreeSet::new();
    let mut total = 0u64;
    for file in &manifest.files {
        safe_path(&file.path)?;
        ensure!(paths.insert(file.path.to_lowercase()), "Duplicate backup path: {}", file.path);
        ensure!(!file.path.is_empty() && file.sha256.len() == 64 && file.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid SHA-256: {}", file.path);
        total = total.checked_add(file.size).context("Backup size overflow")?;
    }
    ensure!(total == manifest.total_size && total <= MAX_BYTES, "Backup size does not match the manifest or exceeds 25 GiB");
    Ok(())
}

fn read_manifest(reader: impl Read) -> Result<Manifest> {
    let mut raw = Vec::new();
    reader.take(MAX_MANIFEST + 1).read_to_end(&mut raw)?;
    ensure!(raw.len() as u64 <= MAX_MANIFEST, "Backup manifest is too large");
    let manifest = serde_json::from_slice(&raw).context("Invalid backup manifest")?;
    validate(&manifest)?;
    Ok(manifest)
}

fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<()> {
    let raw = serde_json::to_vec_pretty(manifest)?;
    ensure!(raw.len() as u64 <= MAX_MANIFEST, "Backup manifest is too large");
    store::write_atomic(&dir.join(MANIFEST), &raw)?;
    Ok(())
}

fn id_is_safe(id: &str) -> bool {
    id.len() <= 100 && id.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && id.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-')
}

fn snapshot_dir(root: &Path, id: &str) -> Result<PathBuf> {
    ensure!(id_is_safe(id), "Invalid snapshot name");
    let dir = root.join(id);
    ensure!(storage::metadata(&dir)?.is_some_and(|m| m.is_dir()), "Snapshot folder is missing");
    Ok(dir)
}

fn read_snapshot(root: &Path, id: &str) -> Result<Snapshot> {
    let dir = snapshot_dir(root, id)?;
    let manifest_path = dir.join(MANIFEST);
    ensure!(storage::metadata(&manifest_path)?.is_some_and(|m| m.is_file()), "Snapshot manifest is missing");
    Ok(Snapshot { id: id.into(), manifest: read_manifest(File::open(manifest_path)?)? })
}

pub fn list(root: &Path) -> Result<Vec<Snapshot>> {
    list_checked(root, &|| Ok(()))
}

fn list_checked(root: &Path, check: &impl Fn() -> Result<()>) -> Result<Vec<Snapshot>> {
    let mut snapshots = Vec::new();
    if let Some(m) = storage::metadata(root)? {
        ensure!(m.is_dir(), "Backup location is not a folder");
        for entry in fs::read_dir(root)? {
            check()?;
            let entry = entry?;
            let name = entry.file_name();
            if let Some(id) = name.to_str().filter(|id| id_is_safe(id)) {
                // A damaged or incomplete backup must never be selected by automatic cleanup.
                if let Ok(snapshot) = read_snapshot(root, id) {
                    snapshots.push(snapshot);
                }
            }
        }
    }
    snapshots.sort_by(|a, b| (b.manifest.created, &b.id).cmp(&(a.manifest.created, &a.id)));
    Ok(snapshots)
}

fn scan(data: &Path, mut check: impl FnMut() -> Result<()>) -> Result<BTreeMap<String, SavedFile>> {
    let mut files = BTreeMap::new();
    let mut bytes = 0u64;
    storage::walk(data, &persistent, &mut check, &mut |entry| {
        if entry.meta.is_file() {
            let name = path_text(&entry.path)?;
            let size = entry.meta.len();
            bytes = bytes.checked_add(size).context("Backup size overflow")?;
            ensure!(files.len() < MAX_FILES && bytes <= MAX_BYTES, "Save data exceeds the backup size limit");
            let modified_ns = entry.meta.modified().ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .and_then(|d| u64::try_from(d.as_nanos()).ok());
            files.insert(name.clone(), SavedFile { path: name, size, modified_ns, sha256: String::new() });
        }
        Ok(())
    })?;
    Ok(files)
}

fn same_data(data: &Path, files: &BTreeMap<String, SavedFile>, previous: &Manifest, check: &impl Fn() -> Result<()>) -> Result<bool> {
    if previous.files.len() != files.len() {
        return Ok(false);
    }
    for old in &previous.files {
        check()?;
        let Some(new) = files.get(&old.path) else { return Ok(false) };
        if new.size != old.size {
            return Ok(false);
        }
        if (new.modified_ns.is_none() || new.modified_ns != old.modified_ns)
            && storage::digest(&data.join(safe_path(&new.path)?), new.size, |_| check())? != old.sha256
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub struct Create<'a> {
    pub kind: Kind,
    pub label: &'a str,
    pub engine_build: &'a str,
    pub deadline: Option<Instant>,
    pub force: bool,
}

pub fn create(data: &Path, root: &Path, options: Create<'_>) -> Result<Option<Snapshot>> {
    let label = label_text(options.label)?;
    let check = || {
        ensure!(options.deadline.is_none_or(|end| Instant::now() < end), "Save backup timed out");
        Ok(())
    };
    check()?;
    ensure!(!storage::overlap(data, root)?, "Save data and backups must use separate folders");
    let files = scan(data, check)?;
    if files.is_empty() {
        return Ok(None);
    }
    if !options.force && label.is_empty() {
        if let Some(previous) = list_checked(root, &check)?.first() {
            if same_data(data, &files, &previous.manifest, &check)? {
                return Ok(None);
            }
        }
    }
    fs::create_dir_all(root)?;
    let stage = tempfile::Builder::new().prefix(".snapshot-").tempdir_in(root)?;
    let created = storage::now().as_secs();
    let mut manifest = Manifest {
        schema: 1,
        created,
        kind: options.kind,
        engine_build: options.engine_build.into(),
        launcher_version: env!("CARGO_PKG_VERSION").into(),
        label,
        total_size: 0,
        files: Vec::with_capacity(files.len()),
    };
    for (_, mut file) in files {
        check()?;
        let relative = safe_path(&file.path)?;
        let dest = storage::new_file(&stage.path().join(FILES).join(&relative))?;
        file.sha256 = storage::transfer(File::open(data.join(relative))?, &dest, file.size, true, |_| check())?
            .context("Missing backup SHA-256")?;
        dest.sync_all()?;
        manifest.total_size += file.size;
        manifest.files.push(file);
    }
    validate(&manifest)?;
    write_manifest(stage.path(), &manifest)?;
    check()?;
    publish(root, stage, manifest).map(Some)
}

fn publish(root: &Path, stage: tempfile::TempDir, manifest: Manifest) -> Result<Snapshot> {
    let nonce = stage.path().file_name().and_then(|p| p.to_str()).context("Invalid staging folder")?
        .trim_start_matches(".snapshot-").to_ascii_lowercase();
    let id = format!("{}-{}-{}", runner::timestamp(), nonce, manifest.kind.id());
    fs::rename(stage.path(), root.join(&id)).context("Could not publish the backup")?;
    Ok(Snapshot { id, manifest })
}

fn verify_file(source: &Path, file: &SavedFile) -> Result<PathBuf> {
    let path = safe_path(&file.path)?;
    let actual = storage::digest(&source.join(&path), file.size, |_| Ok(()))
        .with_context(|| format!("{} differs", file.path))?;
    ensure!(actual.eq_ignore_ascii_case(&file.sha256), "SHA-256 differs: {}", file.path);
    Ok(path)
}

pub fn verify(root: &Path, id: &str) -> Result<Snapshot> {
    let snap = read_snapshot(root, id)?;
    let source = snapshot_dir(root, id)?.join(FILES);
    let mut expected: BTreeSet<_> = snap.manifest.files.iter().map(|f| f.path.as_str()).collect();
    storage::walk(&source, &|_| true, &mut || Ok(()), &mut |entry| {
        if entry.meta.is_file() {
            let path = path_text(&entry.path)?;
            ensure!(expected.remove(path.as_str()), "Unexpected file: {path}");
        }
        Ok(())
    })?;
    if let Some(path) = expected.first() {
        bail!("File missing: {path}");
    }
    for file in &snap.manifest.files {
        verify_file(&source, file)?;
    }
    Ok(snap)
}

pub fn rename(root: &Path, id: &str, label: &str) -> Result<()> {
    let mut snap = read_snapshot(root, id)?;
    snap.manifest.label = label_text(label)?;
    write_manifest(&snapshot_dir(root, id)?, &snap.manifest)
}

pub fn delete(root: &Path, id: &str) -> Result<()> {
    let snap = read_snapshot(root, id)?;
    let other = list(root)?.into_iter().any(|other| other.id != snap.id && verify(root, &other.id).is_ok());
    ensure!(other, "Keep at least one verified backup.");
    fs::remove_dir_all(snapshot_dir(root, id)?)?;
    Ok(())
}

/// Return the exact automatic removals for review. Most recent copies win when a cap applies.
pub fn retention_plan(snapshots: &[Snapshot], policy: Retention, now: u64) -> Result<Vec<String>> {
    let mut snapshots = snapshots.iter().collect::<Vec<_>>();
    snapshots.sort_by(|a, b| (b.manifest.created, &b.id).cmp(&(a.manifest.created, &a.id)));
    let mut days = BTreeSet::new();
    let mut automatic = 0;
    let mut keep = BTreeSet::new();
    let mut size = 0u64;
    for snap in &snapshots {
        let protected = snap.manifest.kind == Kind::Manual || !snap.manifest.label.is_empty();
        let recent = if protected { false } else {
            automatic += 1;
            automatic <= policy.keep_recent
        };
        let day = snap.manifest.created / DAY_SECS;
        let daily = day <= now / DAY_SECS && (now / DAY_SECS - day) < u64::from(policy.keep_days) && days.insert(day);
        if protected || recent || daily {
            keep.insert(snap.id.as_str());
            size = size.saturating_add(snap.manifest.total_size);
        }
    }
    if let Some(newest) = snapshots.first() {
        if keep.insert(&newest.id) {
            size = size.saturating_add(newest.manifest.total_size);
        }
        if policy.max_gib > 0 {
            let cap = u64::from(policy.max_gib) << 30;
            for snap in snapshots.iter().rev() {
                if size <= cap {
                    break;
                }
                if snap.id != newest.id && snap.manifest.kind != Kind::Manual && snap.manifest.label.is_empty()
                    && keep.remove(snap.id.as_str())
                {
                    size -= snap.manifest.total_size;
                }
            }
        }
    }
    Ok(snapshots.into_iter().filter(|s| !keep.contains(s.id.as_str())).map(|s| s.id.clone()).collect())
}

pub fn cleanup(root: &Path, policy: Retention, approved: &[String]) -> Result<usize> {
    if approved.is_empty() {
        return Ok(0);
    }
    let snapshots = list(root)?;
    let planned = retention_plan(&snapshots, policy, storage::now().as_secs())?;
    ensure!(approved.iter().all(|id| planned.contains(id)), "The backup list changed. Review the cleanup again.");
    let proof = snapshots.iter().find(|s| !approved.contains(&s.id) && verify(root, &s.id).is_ok());
    ensure!(proof.is_some(), "Keep at least one verified backup.");
    for id in approved {
        fs::remove_dir_all(snapshot_dir(root, id)?)?;
    }
    Ok(approved.len())
}

pub fn auto_cleanup(root: &Path, policy: Retention) -> Result<usize> {
    let removals = retention_plan(&list(root)?, policy, storage::now().as_secs())?;
    cleanup(root, policy, &removals)
}

pub fn export(root: &Path, id: &str, out: &Path) -> Result<()> {
    let snap = verify(root, id)?;
    let source = snapshot_dir(root, id)?.join(FILES);
    ensure!(!storage::overlap(root, out)?, "Export the ZIP outside the backup folder");
    let parent = out.parent().filter(|p| !p.as_os_str().is_empty()).context("Choose an export folder")?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    let mut zip = ZipWriter::new(tmp.as_file_mut());
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(MANIFEST, options)?;
    zip.write_all(&serde_json::to_vec_pretty(&snap.manifest)?)?;
    for file in &snap.manifest.files {
        let full = source.join(safe_path(&file.path)?);
        zip.start_file(format!("{FILES}/{}", file.path), options.large_file(file.size >= u64::from(u32::MAX)))?;
        let digest = storage::transfer(File::open(full)?, &mut zip, file.size, true, |_| Ok(()))?;
        ensure!(digest.is_some_and(|h| h.eq_ignore_ascii_case(&file.sha256)), "SHA-256 differs: {}", file.path);
    }
    zip.finish()?;
    tmp.as_file().sync_all()?;
    tmp.persist_noclobber(out).context("The export file already exists or could not be saved")?;
    Ok(())
}

pub fn import(root: &Path, source: &Path) -> Result<Snapshot> {
    let source = File::open(source)?;
    ensure!(source.metadata()?.len() <= MAX_BYTES + MAX_MANIFEST, "ZIP is too large");
    let mut zip = ZipArchive::new(source).context("Invalid backup ZIP")?;
    ensure!(zip.len() <= MAX_FILES + 1, "Too many entries in the ZIP");
    let mut manifest = read_manifest(zip.by_name(MANIFEST)?)?;
    ensure!(zip.len() == manifest.files.len() + 1, "The ZIP file list does not match its manifest");
    fs::create_dir_all(root)?;
    let stage = tempfile::Builder::new().prefix(".snapshot-").tempdir_in(root)?;
    let mut expected: BTreeMap<_, _> = manifest.files.iter().map(|f| (format!("{FILES}/{}", f.path), f)).collect();
    let mut manifest_seen = false;
    for i in 0..zip.len() {
        let mut item = zip.by_index(i)?;
        ensure!(item.is_file() && !item.is_symlink(), "Unsupported ZIP entry: {}", item.name());
        if item.name() == MANIFEST {
            ensure!(!manifest_seen, "Duplicate ZIP manifest");
            manifest_seen = true;
            continue;
        }
        let entry = expected.remove(item.name()).with_context(|| format!("Unexpected ZIP entry: {}", item.name()))?;
        ensure!(item.size() == entry.size, "ZIP size differs: {}", entry.path);
        let dest = storage::new_file(&stage.path().join(FILES).join(safe_path(&entry.path)?))?;
        let digest = storage::transfer(&mut item, &dest, entry.size, true, |_| Ok(()))?;
        ensure!(digest.is_some_and(|h| h.eq_ignore_ascii_case(&entry.sha256)), "SHA-256 differs: {}", entry.path);
        dest.sync_all()?;
    }
    ensure!(manifest_seen && expected.is_empty(), "ZIP entries are missing");
    manifest.kind = Kind::Manual;
    write_manifest(stage.path(), &manifest)?;
    publish(root, stage, manifest)
}

fn restore_old(data: &Path) -> Result<PathBuf> {
    let name = data.file_name().context("Data folder must have a name")?.to_string_lossy();
    Ok(data.with_file_name(format!(".{name}.victory-restore-old")))
}

/// Recover a rename interrupted between preserving the original and publishing the replacement.
pub fn recover_restore(data: &Path) -> Result<()> {
    let old = restore_old(data)?;
    if storage::metadata(&old)?.is_some() && storage::metadata(data)?.is_none() {
        fs::rename(&old, data).with_context(|| format!("Recover the original data from {}", old.display()))?;
    }
    Ok(())
}

pub fn restore(data: &Path, root: &Path, id: &str, engine_build: &str) -> Result<()> {
    recover_restore(data)?;
    let snap = verify(root, id)?;
    ensure!(!storage::overlap(data, root)?, "Save data and backups must use separate folders");
    let old = restore_old(data)?;
    if storage::metadata(&old)?.is_some() {
        fs::remove_dir_all(&old)?;
    }
    create(data, root, Create { kind: Kind::BeforeRestore, label: "", engine_build, deadline: None, force: true })?;
    let parent = data.parent().context("Data folder must have a parent")?;
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new().prefix(".victory-restore-").tempdir_in(parent)?;
    let source = snapshot_dir(root, id)?.join(FILES);
    for file in snap.manifest.files {
        let path = safe_path(&file.path)?;
        let dest = storage::new_file(&stage.path().join(&path))?;
        let digest = storage::transfer(File::open(source.join(path))?, &dest, file.size, true, |_| Ok(()))?;
        ensure!(digest.is_some_and(|h| h.eq_ignore_ascii_case(&file.sha256)), "SHA-256 differs: {}", file.path);
        dest.sync_all()?;
    }
    swap_data(data, &old, stage.path(), |from, to| fs::rename(from, to))
}

fn swap_data(data: &Path, old: &Path, stage: &Path, rename: impl Fn(&Path, &Path) -> io::Result<()>) -> Result<()> {
    let has_original = storage::metadata(data)?.is_some();
    if has_original {
        rename(data, old).context("Could not preserve the current data")?;
    }
    if let Err(error) = rename(stage, data) {
        if has_original {
            fs::rename(old, data).with_context(|| format!("Restore stopped. The original data is at {}", old.display()))?;
        }
        return Err(error).context("Could not switch to the restored data");
    }
    if has_original {
        fs::remove_dir_all(old).context("The restore succeeded, but the old data folder could not be removed")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOT: &str = "DEADBEEF/saves/one.bin";
    const LOCAL: &str = "DEADBEEF/storage/options.dat";

    fn write(root: &Path, name: &str, bytes: &[u8]) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn create_save(data: &Path, root: &Path) -> Snapshot {
        create(data, root, Create { kind: Kind::Manual, label: "Checkpoint", engine_build: "01234567", deadline: None, force: false })
            .unwrap().unwrap()
    }

    #[test]
    fn snapshot_zip_restore_round_trip_includes_only_persistent_data() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let root = temp.path().join("backups");
        let exported = temp.path().join("save.zip");
        let imported_root = temp.path().join("imported");
        write(&data, SLOT, b"saved");
        write(&data, LOCAL, b"options");
        write(&data, "DEADBEEF/temp/work", b"temporary");
        write(&data, "cache/shader", b"shader");
        let snapshot = create_save(&data, &root);
        let paths = snapshot.manifest.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>();
        assert_eq!(paths, [SLOT, LOCAL]);
        assert_eq!(snapshot.manifest.total_size, 12);
        export(&root, &snapshot.id, &exported).unwrap();
        let imported = import(&imported_root, &exported).unwrap();
        assert_eq!(imported.manifest.kind, Kind::Manual);
        assert_eq!(imported.manifest.label, "Checkpoint");
        write(&data, SLOT, b"newer");
        fs::remove_file(data.join(LOCAL)).unwrap();
        restore(&data, &imported_root, &imported.id, "new-build").unwrap();
        assert_eq!(fs::read(data.join(SLOT)).unwrap(), b"saved");
        assert_eq!(fs::read(data.join(LOCAL)).unwrap(), b"options");
        assert!(!restore_old(&data).unwrap().exists());
        let old = list(&imported_root).unwrap().into_iter().find(|s| s.manifest.kind == Kind::BeforeRestore).unwrap();
        assert_eq!(fs::read(imported_root.join(old.id).join(FILES).join(SLOT)).unwrap(), b"newer");
    }

    #[test]
    fn unchanged_skips_and_same_length_edit_is_detected() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let root = temp.path().join("backups");
        write(&data, SLOT, b"first");
        let before = || Create { kind: Kind::BeforeLaunch, label: "", engine_build: "test", deadline: None, force: false };
        let snap = create(&data, &root, before()).unwrap().unwrap();
        assert!(create(&data, &root, before()).unwrap().is_none());
        // A different sampled mtime forces hashing even though the length is unchanged.
        write(&data, SLOT, b"other");
        let file = File::open(data.join(SLOT)).unwrap();
        file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(100)).unwrap();
        assert!(create(&data, &root, before()).unwrap().is_some());
        write(&root.join(&snap.id).join(FILES), SLOT, b"wrong");
        assert!(format!("{:#}", verify(&root, &snap.id).unwrap_err()).contains(SLOT));
    }

    #[test]
    fn corrupt_and_traversing_zips_leave_no_published_backup() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let root = temp.path().join("backups");
        let out = temp.path().join("imported");
        let zip_path = temp.path().join("untrusted.zip");
        write(&data, SLOT, b"good");
        let snap = create_save(&data, &root);
        fs::write(&zip_path, b"broken zip").unwrap();
        assert!(import(&out, &zip_path).is_err());
        for (path, payload) in [
            (SLOT, b"bad!" as &[u8]),
            ("DEADBEEF/saves/../../../../escape", b"good"),
            ("DEADBEEF/saves/CON.txt", b"good"),
            ("DEADBEEF/saves/alternate:stream", b"good"),
        ] {
            let mut manifest = snap.manifest.clone();
            manifest.files[0].path = path.into();
            let mut zip = ZipWriter::new(File::create(&zip_path).unwrap());
            zip.start_file(MANIFEST, SimpleFileOptions::default()).unwrap();
            zip.write_all(&serde_json::to_vec(&manifest).unwrap()).unwrap();
            zip.start_file(format!("{FILES}/{path}"), SimpleFileOptions::default()).unwrap();
            zip.write_all(payload).unwrap();
            zip.finish().unwrap();
            assert!(import(&out, &zip_path).is_err(), "{path}");
            assert!(list(&out).unwrap().is_empty());
        }
    }

    #[test]
    fn retention_protects_labels_manual_backups_and_the_last_copy() {
        let snap = |id: &str, day, kind, label: &str| Snapshot {
            id: id.into(),
            manifest: Manifest { schema: 1, created: day * DAY_SECS, kind, engine_build: String::new(),
                launcher_version: String::new(), label: label.into(), total_size: 1 << 30, files: vec![] },
        };
        let samples = [
            snap("5-newest", 10, Kind::BeforeLaunch, ""),
            snap("4-same-day", 10, Kind::AfterExit, ""),
            snap("3-previous-day", 9, Kind::AfterExit, ""),
            snap("2-labeled", 4, Kind::BeforeLaunch, "keep"),
            snap("1-manual", 1, Kind::Manual, ""),
            snap("0-expired", 0, Kind::BeforeLaunch, ""),
        ];
        for (recent, days, cap, expected) in [
            (1, 2, 0, vec!["4-same-day", "0-expired"]),
            (2, 0, 0, vec!["3-previous-day", "0-expired"]),
            (1, 2, 3, vec!["4-same-day", "3-previous-day", "0-expired"]),
            (0, 0, 1, vec!["4-same-day", "3-previous-day", "0-expired"]),
        ] {
            let policy = Retention { keep_recent: recent, keep_days: days, max_gib: cap };
            assert_eq!(retention_plan(&samples, policy, 10 * DAY_SECS).unwrap(), expected);
            assert!(retention_plan(&samples[..1], policy, 10 * DAY_SECS).unwrap().is_empty());
        }
    }

    #[test]
    fn failed_restore_keeps_original_and_interrupted_rename_is_recoverable() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let stage = temp.path().join("stage");
        let root = temp.path().join("backups");
        let old = restore_old(&data).unwrap();
        write(&data, SLOT, b"original");
        write(&stage, SLOT, b"replacement");
        let snap = create_save(&stage, &root);
        write(&root.join(&snap.id).join(FILES), SLOT, b"tampered");
        assert!(restore(&data, &root, &snap.id, "").is_err());
        assert_eq!(fs::read(data.join(SLOT)).unwrap(), b"original");
        let rename = |from: &Path, to: &Path| {
            if from == stage { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "simulated failure")); }
            fs::rename(from, to)
        };
        assert!(swap_data(&data, &old, &stage, rename).is_err());
        assert_eq!(fs::read(data.join(SLOT)).unwrap(), b"original");
        fs::rename(&data, &old).unwrap();
        recover_restore(&data).unwrap();
        assert_eq!(fs::read(data.join(SLOT)).unwrap(), b"original");
    }

    #[test]
    fn expired_snapshot_never_publishes_and_last_backup_cannot_be_deleted() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let root = temp.path().join("backups");
        write(&data, SLOT, b"original");
        let options = Create { kind: Kind::BeforeLaunch, label: "", engine_build: "", deadline: Some(Instant::now()), force: false };
        assert!(create(&data, &root, options).is_err());
        assert!(!root.exists());
        let one = create_save(&data, &root);
        assert!(delete(&root, &one.id).is_err());
    }
}
