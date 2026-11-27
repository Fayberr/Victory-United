//! File operations shared by snapshots and storage moves. No UI dependencies.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn now() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}

/// Holds the same OS lock while an engine or a storage action owns the data.
pub fn lock(path: &Path) -> Result<File> {
    fs::create_dir_all(path.parent().context("Activity lock needs a parent folder")?)?;
    let file = File::options().read(true).write(true).create(true).truncate(false).open(path)?;
    file.try_lock().context("Close the game or wait for the current storage operation to finish.")?;
    Ok(file)
}

pub fn metadata(path: &Path) -> Result<Option<Metadata>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| path.display().to_string()),
    };
    let link = meta.is_symlink();
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        link || meta.file_attributes() & 0x400 != 0
    };
    ensure!(!link && (meta.is_file() || meta.is_dir()), "Unsupported file or link: {}", path.display());
    Ok(Some(meta))
}

pub struct Entry {
    pub path: PathBuf,
    pub meta: Metadata,
}

/// Visit a tree without following links. The filter runs before opening an entry.
pub fn walk(
    root: &Path,
    filter: &impl Fn(&Path) -> bool,
    check: &mut impl FnMut() -> Result<()>,
    visit: &mut impl FnMut(Entry) -> Result<()>,
) -> Result<()> {
    if let Some(m) = metadata(root)? {
        ensure!(m.is_dir(), "Expected a folder: {}", root.display());
        walk_from(root, Path::new(""), filter, check, visit)?;
    }
    Ok(())
}

fn walk_from(
    root: &Path,
    relative: &Path,
    filter: &impl Fn(&Path) -> bool,
    check: &mut impl FnMut() -> Result<()>,
    visit: &mut impl FnMut(Entry) -> Result<()>,
) -> Result<()> {
    check()?;
    ensure!(relative.components().count() < 64, "Folder nesting is too deep: {}", relative.display());
    for item in fs::read_dir(root.join(relative))? {
        check()?;
        let path = relative.join(item?.file_name());
        if filter(&path) {
            let meta = metadata(&root.join(&path))?.context("A file disappeared while reading the folder")?;
            let is_dir = meta.is_dir();
            visit(Entry { path: path.clone(), meta })?;
            if is_dir {
                walk_from(root, &path, filter, check, visit)?;
            }
        }
    }
    Ok(())
}

/// Copy exactly the expected length and optionally hash in one pass. The callback
/// runs between chunks and can stop the operation before touching another file.
pub fn transfer(
    mut input: impl Read,
    mut output: impl Write,
    expected: u64,
    hash: bool,
    mut chunk: impl FnMut(u64) -> Result<()>,
) -> Result<Option<String>> {
    let mut digest = hash.then(Sha256::new);
    let mut bytes = 0u64;
    let mut buffer = vec![0; 256 * 1024];
    chunk(0)?;
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes = bytes.checked_add(n as u64).context("File size overflow")?;
        ensure!(bytes <= expected, "File exceeds its expected size");
        output.write_all(&buffer[..n])?;
        if let Some(digest) = &mut digest {
            digest.update(&buffer[..n]);
        }
        chunk(n as u64)?;
    }
    ensure!(bytes == expected, "File is shorter than expected");
    Ok(digest.map(|d| format!("{:x}", d.finalize())))
}

pub fn new_file(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(File::options().write(true).create_new(true).open(path)?)
}

pub fn digest(path: &Path, bytes: u64, check: impl FnMut(u64) -> Result<()>) -> Result<String> {
    let expected = metadata(path)?.with_context(|| format!("File missing: {}", path.display()))?;
    ensure!(expected.is_file() && expected.len() == bytes, "File size differs: {}", path.display());
    transfer(File::open(path)?, io::sink(), bytes, true, check)?
        .context("SHA-256 was requested")
}

/// Canonicalize the existing prefix so links and junctions cannot conceal containment.
pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    ensure!(!path.components().any(|c| c == std::path::Component::ParentDir), "Choose a path without parent traversal");
    let mut prefix = path.as_path();
    let mut tail = Vec::new();
    loop {
        match fs::canonicalize(prefix) {
            Ok(mut resolved) => {
                for name in tail.into_iter().rev() {
                    resolved.push(name);
                }
                return Ok(resolved);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                tail.push(prefix.file_name().context("Storage drive is unavailable")?);
                prefix = prefix.parent().context("Storage drive is unavailable")?;
            }
            Err(e) => return Err(e).with_context(|| format!("Could not resolve {}", prefix.display())),
        }
    }
}

pub fn within(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let path = path.to_string_lossy().to_lowercase();
        let root = root.to_string_lossy().to_lowercase();
        Path::new(&path).starts_with(Path::new(&root))
    }
    #[cfg(not(windows))]
    path.starts_with(root)
}

pub fn overlap(first: &Path, second: &Path) -> Result<bool> {
    let first = absolute(first)?;
    let second = absolute(second)?;
    Ok(within(&first, &second) || within(&second, &first))
}

use super::{saves, store::{self, Settings}};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Location {
    Cache,
    Data,
}

impl Location {
    pub fn default_path(self, base: &Path) -> PathBuf {
        base.join(match self { Self::Cache => "cache", Self::Data => "data" })
    }

    pub fn path(self, base: &Path, settings: &Settings) -> PathBuf {
        self.setting(settings).clone().unwrap_or_else(|| self.default_path(base))
    }

    fn setting(self, settings: &Settings) -> &Option<PathBuf> {
        match self {
            Self::Cache => &settings.storage.cache,
            Self::Data => &settings.storage.data,
        }
    }

    fn set(self, settings: &mut Settings, target: Option<PathBuf>) {
        match self {
            Self::Cache => settings.storage.cache = target,
            Self::Data => settings.storage.data = target,
        }
    }

    fn other(self) -> Self {
        match self { Self::Cache => Self::Data, Self::Data => Self::Cache }
    }
}

pub struct MoveContext<'a> {
    pub settings: &'a Path,
    pub engine: &'a Path,
    pub game: Option<&'a Path>,
}

impl MoveContext<'_> {
    fn base(&self) -> Result<&Path> {
        self.settings.parent().context("Settings need a parent folder")
    }

    fn journal(&self) -> Result<PathBuf> {
        Ok(self.base()?.join("move-journal.toml"))
    }

    fn activity_lock(&self) -> Result<File> {
        lock(&self.base()?.join("activity.lock"))
    }

    fn backups(&self, settings: &Settings) -> Result<PathBuf> {
        Ok(settings.backups.root.clone().unwrap_or(self.base()?.join("backups")))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MovePhase {
    Copying,
    Ready,
    Switched,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MoveJournal {
    pub location: Location,
    pub source: PathBuf,
    pub destination: PathBuf,
    stage: PathBuf,
    pub phase: MovePhase,
    hash: bool,
}

pub fn read_journal(path: &Path) -> Result<Option<MoveJournal>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(toml::from_str(&text).context("The move journal could not be read")?)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("The move journal could not be read"),
    }
}

fn journal_write(ctx: &MoveContext<'_>, journal: &MoveJournal) -> Result<()> {
    store::write_atomic(&ctx.journal()?, toml::to_string_pretty(journal)?.as_bytes())?;
    Ok(())
}

fn ensure_separate(first: &Path, other: &Path, what: &str) -> Result<()> {
    ensure!(!overlap(first, other)?, "Choose a folder separate from {what}: {}", other.display());
    Ok(())
}

fn validate_location(path: &Path, location: Location, ctx: &MoveContext<'_>, settings: &Settings) -> Result<()> {
    let resolved = absolute(path)?;
    ensure!(resolved.file_name().is_some(), "Choose a folder inside a drive.");
    for anc in resolved.ancestors() {
        ensure!(!anc.join(".victory-root").exists() && !anc.join(".git").exists(),
            "Storage must be outside source repositories.");
    }
    let base = ctx.base()?;
    ensure!(!within(&absolute(base)?, &resolved), "Choose a location outside the launcher settings folder itself.");
    ensure_separate(path, ctx.engine, "the launcher engine folder")?;
    if let Some(game) = ctx.game {
        ensure_separate(path, game, "the installed game's folder")?;
    }
    ensure_separate(path, &location.other().path(base, settings), "the other storage location")?;
    ensure_separate(path, &ctx.backups(settings)?, "the backups folder")?;
    for name in ["profiles", "logs", "reports", "downloads"] {
        ensure_separate(path, &base.join(name), "launcher files")?;
    }
    if let Some(m) = metadata(path)? {
        ensure!(m.is_dir(), "The location must be a folder: {}", path.display());
    }
    Ok(())
}

pub fn validate_data_work(ctx: &MoveContext<'_>, settings: &Settings) -> Result<()> {
    if let Some(journal) = read_journal(&ctx.journal()?)? {
        ensure!(journal.phase == MovePhase::Switched, "Resume or cancel the interrupted move in Settings.");
    }
    let data = Location::Data.path(ctx.base()?, settings);
    let cache = Location::Cache.path(ctx.base()?, settings);
    ensure_separate(&data, &cache, "the cache location")?;
    Ok(())
}

pub fn validate_backup_root(path: &Path, ctx: &MoveContext<'_>, settings: &Settings) -> Result<PathBuf> {
    let resolved = absolute(path)?;
    ensure!(resolved.file_name().is_some(), "Choose a folder inside a drive.");
    ensure!(!within(&absolute(ctx.base()?)?, &resolved), "Choose a dedicated backup folder.");
    ensure!(!resolved.ancestors().any(|p| p.join(".victory-root").exists() || p.join(".git").exists()),
        "Backups must be outside source repositories.");
    ensure_separate(path, ctx.engine, "the launcher engine folder")?;
    if let Some(game) = ctx.game {
        ensure_separate(path, game, "the installed game's folder")?;
    }
    for location in [Location::Cache, Location::Data] {
        ensure_separate(path, &location.path(ctx.base()?, settings), "game storage")?;
    }
    for name in ["profiles", "logs", "reports", "downloads"] {
        ensure_separate(path, &ctx.base()?.join(name), "launcher files")?;
    }
    if let Some(m) = metadata(path)? {
        ensure!(m.is_dir(), "The backup location must be a folder");
    }
    Ok(std::path::absolute(path)?)
}

fn existing_dir(path: &Path) -> Result<PathBuf> {
    let path = absolute(path)?;
    path.ancestors().find(|p| p.is_dir()).map(Path::to_path_buf).context("The storage drive is unavailable")
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn free_space(path: &Path) -> Result<u64> {
    use windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::PCWSTR};
    let path = wide(&existing_dir(path)?);
    let mut available = 0;
    // The directory string is NUL-terminated and available is writable for the call.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut available), None, None)? };
    Ok(available)
}

#[cfg(unix)]
fn free_space(path: &Path) -> Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(existing_dir(path)?.as_os_str().as_bytes())?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // statvfs initializes the result on success.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let stats = unsafe { stats.assume_init() };
    Ok((stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64))
}

#[cfg(not(any(unix, windows)))]
fn free_space(_path: &Path) -> Result<u64> { anyhow::bail!("Drive free space is unavailable") }

pub fn network_drive(path: &Path) -> Result<bool> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        use windows::{Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW}, core::PCWSTR};
        if matches!(path.components().next(), Some(Component::Prefix(p))
            if matches!(p.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)))
        {
            return Ok(true);
        }
        let dir = existing_dir(path)?;
        if matches!(dir.components().next(), Some(Component::Prefix(p))
            if matches!(p.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)))
        {
            return Ok(true);
        }
        let existing = wide(&dir);
        let mut root = vec![0u16; 32_768];
        // Both buffers live through both calls. GetVolumePathNameW fills a NUL-terminated path.
        unsafe {
            GetVolumePathNameW(PCWSTR(existing.as_ptr()), &mut root)?;
            Ok(GetDriveTypeW(PCWSTR(root.as_ptr())) == 4) // DRIVE_REMOTE
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(false)
    }
}

#[derive(Clone, Copy)]
pub struct Usage {
    pub bytes: u64,
    pub free: u64,
    pub network: bool,
}

pub fn measure(path: &Path) -> Result<Usage> {
    let mut bytes = 0u64;
    walk(path, &|_| true, &mut || Ok(()), &mut |entry| {
        if entry.meta.is_file() {
            bytes = bytes.saturating_add(entry.meta.len());
        }
        Ok(())
    })?;
    Ok(Usage { bytes, free: free_space(path)?, network: network_drive(path)? })
}

fn empty_or_absent(path: &Path) -> Result<()> {
    if let Some(meta) = metadata(path)? {
        ensure!(meta.is_dir() && fs::read_dir(path)?.next().is_none(), "Choose an empty destination folder.");
    }
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Move paused. Resume it or cancel it from Storage.");
    Ok(())
}

pub fn start_move(
    ctx: &MoveContext<'_>,
    location: Location,
    wanted: &Path,
    build: &str,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<MoveJournal> {
    let _guard = ctx.activity_lock()?;
    ensure!(read_journal(&ctx.journal()?)?.is_none(), "Finish the previous move first.");
    let settings = Settings::load_from(ctx.settings);
    let source = std::path::absolute(location.path(ctx.base()?, &settings))?;
    let destination = std::path::absolute(wanted)?;
    validate_location(&source, location, ctx, &settings)?;
    validate_location(&destination, location, ctx, &settings)?;
    ensure_separate(&destination, &source, "the current location")?;
    empty_or_absent(&destination)?;
    check_cancel(cancel)?;
    let parent = destination.parent().context("Choose a destination folder")?;
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new().prefix(".victory-moving-").tempdir_in(parent)?;
    if location == Location::Data {
        saves::recover_restore(&source)?;
        check_cancel(cancel)?;
        saves::create(&source, &ctx.backups(&settings)?, saves::Create {
            kind: saves::Kind::BeforeMove, label: "", engine_build: build, deadline: None, force: false,
        })?;
    }
    let mut journal = MoveJournal {
        location, source, destination, stage: stage.path().to_path_buf(), phase: MovePhase::Copying,
        hash: location == Location::Data || settings.storage.verify_cache_by_hash,
    };
    journal_write(ctx, &journal)?;
    let _ = stage.keep();
    continue_move(ctx, &mut journal, cancel, progress)?;
    Ok(journal)
}

pub fn resume_move(ctx: &MoveContext<'_>, cancel: &AtomicBool, progress: impl FnMut(u64, u64)) -> Result<MoveJournal> {
    let _guard = ctx.activity_lock()?;
    let mut journal = read_journal(&ctx.journal()?)?.context("There is no move to resume")?;
    continue_move(ctx, &mut journal, cancel, progress)?;
    Ok(journal)
}

fn stage_for(journal: &MoveJournal) -> Result<&Path> {
    ensure!(journal.stage.parent() == journal.destination.parent()
        && journal.stage.file_name().and_then(|p| p.to_str()).is_some_and(|n| n.starts_with(".victory-moving-")),
        "Unsafe move staging folder");
    Ok(&journal.stage)
}

fn same_path(a: &Path, b: &Path) -> Result<bool> {
    let a = absolute(a)?;
    let b = absolute(b)?;
    Ok(within(&a, &b) && within(&b, &a))
}

fn continue_move(
    ctx: &MoveContext<'_>,
    journal: &mut MoveJournal,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    let settings = Settings::load_from(ctx.settings);
    let active = journal.location.path(ctx.base()?, &settings);
    if journal.phase == MovePhase::Switched || same_path(&active, &journal.destination)? {
        ensure!(same_path(&active, &journal.destination)?, "The selected location changed after the move.");
        journal.phase = MovePhase::Switched;
        return journal_write(ctx, journal);
    }
    ensure!(same_path(&active, &journal.source)?, "The current location changed since this move began.");
    validate_location(&journal.source, journal.location, ctx, &settings)?;
    validate_location(&journal.destination, journal.location, ctx, &settings)?;
    ensure_separate(&journal.destination, &journal.source, "the current location")?;
    let stage = stage_for(journal)?.to_path_buf();
    if journal.phase == MovePhase::Copying {
        empty_or_absent(&journal.destination)?;
        if metadata(&stage)?.is_some() {
            fs::remove_dir_all(&stage)?;
        }
        fs::create_dir(&stage)?;
        copy_tree(&journal.source, &stage, journal.hash, cancel, &mut progress)?;
        journal.phase = MovePhase::Ready;
        journal_write(ctx, journal)?;
    } else {
        let copied = if metadata(&stage)?.is_some() { &stage } else { &journal.destination };
        verify_tree(&journal.source, copied, journal.hash, cancel)?;
    }
    check_cancel(cancel)?;
    if metadata(&stage)?.is_some() {
        empty_or_absent(&journal.destination)?;
        if metadata(&journal.destination)?.is_some() {
            fs::remove_dir(&journal.destination)?;
        }
        fs::rename(&stage, &journal.destination).context("Could not publish the moved folder")?;
    }
    ensure!(metadata(&journal.destination)?.is_some_and(|m| m.is_dir()), "Moved folder is missing.");
    store::edit_settings(ctx.settings, |settings| {
        ensure!(same_path(&journal.location.path(ctx.base()?, settings), &journal.source)?,
            "The current location changed before the move could finish.");
        validate_location(&journal.destination, journal.location, ctx, settings)?;
        let default = journal.location.default_path(ctx.base()?);
        let path = (!same_path(&journal.destination, &default)?).then(|| journal.destination.clone());
        journal.location.set(settings, path);
        Ok(())
    })?;
    journal.phase = MovePhase::Switched;
    journal_write(ctx, journal)
}

fn inventory(root: &Path, cancel: &AtomicBool) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    walk(root, &|_| true, &mut || check_cancel(cancel), &mut |entry| {
        entries.push(entry);
        Ok(())
    })?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn copy_tree(
    source: &Path,
    target: &Path,
    hash: bool,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64, u64),
) -> Result<()> {
    let entries = inventory(source, cancel)?;
    let total = entries.iter().filter(|e| e.meta.is_file()).try_fold(0u64, |sum, entry| sum.checked_add(entry.meta.len()))
        .context("Storage size overflow")?;
    let free = free_space(target)?;
    ensure!(free >= total, "The destination drive needs {total} bytes, but only {free} are free.");
    let mut done = 0;
    progress(done, total);
    for entry in entries {
        check_cancel(cancel)?;
        let to = target.join(&entry.path);
        if entry.meta.is_dir() {
            fs::create_dir_all(to)?;
            continue;
        }
        let from = source.join(&entry.path);
        let file = new_file(&to)?;
        let expected = transfer(File::open(&from)?, &file, entry.meta.len(), hash, |size| {
            done += size;
            progress(done, total);
            check_cancel(cancel)
        }).with_context(|| format!("Could not copy {}", entry.path.display()))?;
        file.sync_all()?;
        let actual = metadata(&to)?.context("Copied file went missing")?;
        ensure!(actual.len() == entry.meta.len(), "Size differs: {}", entry.path.display());
        if let Some(expected) = expected {
            ensure!(digest(&to, entry.meta.len(), |_| check_cancel(cancel))? == expected,
                "SHA-256 differs: {}", entry.path.display());
        }
        let after = metadata(&from)?.context("Source file went missing")?;
        ensure!(after.len() == entry.meta.len() && after.modified().ok() == entry.meta.modified().ok(),
            "The source changed during copying: {}", entry.path.display());
    }
    Ok(())
}

fn verify_tree(source: &Path, target: &Path, hash: bool, cancel: &AtomicBool) -> Result<()> {
    ensure!(metadata(target)?.is_some_and(|meta| meta.is_dir()), "The copied folder is missing.");
    let left = inventory(source, cancel)?;
    let right = inventory(target, cancel)?;
    ensure!(left.len() == right.len(), "The copied file list differs.");
    for (a, b) in left.iter().zip(&right) {
        check_cancel(cancel)?;
        ensure!(a.path == b.path && a.meta.is_file() == b.meta.is_file(), "Unexpected file: {}", b.path.display());
        if a.meta.is_file() {
            let size = a.meta.len();
            ensure!(size == b.meta.len(), "Size differs: {}", a.path.display());
            if hash {
                let a_hash = digest(&source.join(&a.path), size, |_| check_cancel(cancel))?;
                let b_hash = digest(&target.join(&b.path), size, |_| check_cancel(cancel))?;
                ensure!(a_hash == b_hash, "SHA-256 differs: {}", a.path.display());
            }
        }
    }
    Ok(())
}

/// Before switching settings, discard the partial copy. After switching,
/// keep the old folder or remove it only on explicit request.
pub fn finish_move(ctx: &MoveContext<'_>, delete_old: bool) -> Result<()> {
    let _guard = ctx.activity_lock()?;
    let journal = read_journal(&ctx.journal()?)?.context("There is no move to finish")?;
    let settings = Settings::load_from(ctx.settings);
    let active = journal.location.path(ctx.base()?, &settings);
    if same_path(&active, &journal.destination)? {
        if delete_old {
            validate_location(&journal.source, journal.location, ctx, &settings)?;
            ensure_separate(&journal.source, &journal.destination, "the active location")?;
            ensure!(metadata(&journal.destination)?.is_some_and(|m| m.is_dir()), "The moved folder is missing.");
            if metadata(&journal.source)?.is_some() {
                fs::remove_dir_all(&journal.source).context("Could not remove the old folder")?;
            }
        }
    } else {
        ensure!(!delete_old && same_path(&active, &journal.source)?, "The active storage location changed.");
        let stage = stage_for(&journal)?;
        if metadata(stage)?.is_some() {
            fs::remove_dir_all(stage)?;
        }
        // Ready may already have published the destination just before an interruption.
        // Leave that verified copy on disk if it was published. The old path stays selected.
    }
    fs::remove_file(ctx.journal()?)?;
    Ok(())
}

pub fn clear(ctx: &MoveContext<'_>, location: Location, build: &str) -> Result<()> {
    let _guard = ctx.activity_lock()?;
    ensure!(read_journal(&ctx.journal()?)?.is_none_or(|j| j.phase == MovePhase::Switched),
        "Finish the move before clearing a folder.");
    let settings = Settings::load_from(ctx.settings);
    let path = location.path(ctx.base()?, &settings);
    validate_location(&path, location, ctx, &settings)?;
    if location == Location::Data {
        saves::recover_restore(&path)?;
        saves::create(&path, &ctx.backups(&settings)?, saves::Create {
            kind: saves::Kind::BeforeMove, label: "", engine_build: build, deadline: None, force: true,
        })?;
    }
    if metadata(&path)?.is_some() {
        let staged = tempfile::Builder::new().prefix(".victory-clear-").tempdir_in(
            path.parent().context("Storage needs a parent folder")?
        )?;
        let discarded = staged.path().join("cleared");
        fs::rename(&path, &discarded)?;
        staged.close().context("The location was cleared, but the old files could not be removed")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(root: &Path) -> (PathBuf, PathBuf) {
        (root.join("launcher/settings.toml"), root.join("portable/engine"))
    }

    #[test]
    fn canceled_copy_resumes_and_preserves_the_old_folder_until_confirmed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (settings, engine) = setup(root);
        let base = settings.parent().unwrap();
        let ctx = MoveContext { settings: &settings, engine: &engine, game: None };
        let source = base.join("data");
        let target = root.join("other-drive/game data");
        let slot = Path::new("DEADBEEF/saves/slot");
        fs::create_dir_all(source.join(slot).parent().unwrap()).unwrap();
        fs::create_dir_all(source.join("empty")).unwrap();
        fs::write(source.join(slot), [126; 1024]).unwrap();
        let stop = AtomicBool::new(false);
        let failed = start_move(&ctx, Location::Data, &target, "test", &stop, |done, _| {
            if done > 0 { stop.store(true, Ordering::Relaxed); }
        });
        assert!(failed.is_err());
        assert_eq!(read_journal(&ctx.journal().unwrap()).unwrap().unwrap().phase, MovePhase::Copying);
        assert!(Settings::load_from(&settings).storage.data.is_none());
        assert!(source.join(slot).is_file());
        assert!(!target.exists());
        assert_eq!(saves::list(&base.join("backups")).unwrap()[0].manifest.kind, saves::Kind::BeforeMove);

        let fresh = AtomicBool::new(false);
        let moved = resume_move(&ctx, &fresh, |_, _| {}).unwrap();
        assert_eq!(moved.phase, MovePhase::Switched);
        assert_eq!(Settings::load_from(&settings).storage.data.as_ref(), Some(&target));
        assert_eq!(fs::read(target.join(slot)).unwrap(), [126; 1024]);
        assert!(target.join("empty").is_dir());
        assert!(source.join(slot).is_file());
        assert!(measure(&target).unwrap().free > 0);
        finish_move(&ctx, true).unwrap();
        assert!(!source.exists());
        assert!(target.join(slot).exists());
        assert!(read_journal(&ctx.journal().unwrap()).unwrap().is_none());
    }

    #[test]
    fn resume_after_destination_rename_checks_hash_before_switching() {
        let temp = tempfile::tempdir().unwrap();
        let (settings, engine) = setup(temp.path());
        let ctx = MoveContext { settings: &settings, engine: &engine, game: None };
        let source = settings.parent().unwrap().join("data");
        let destination = temp.path().join("new-data");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("new-file"), b"same").unwrap();
        fs::write(destination.join("new-file"), b"bad!").unwrap();
        let journal = MoveJournal { location: Location::Data, source: source.clone(), destination: destination.clone(),
            stage: temp.path().join(".victory-moving-crash"), phase: MovePhase::Ready, hash: true };
        journal_write(&ctx, &journal).unwrap();
        let cancel = AtomicBool::new(false);
        assert!(resume_move(&ctx, &cancel, |_, _| {}).unwrap_err().to_string().contains("SHA-256 differs"));
        assert!(Settings::load_from(&settings).storage.data.is_none());
        assert_eq!(fs::read(source.join("new-file")).unwrap(), b"same");
        fs::write(destination.join("new-file"), b"same").unwrap();
        assert_eq!(resume_move(&ctx, &cancel, |_, _| {}).unwrap().phase, MovePhase::Switched);
        finish_move(&ctx, false).unwrap();
        assert!(source.exists());
    }

    #[test]
    fn guarded_paths_and_game_lock_are_checked_before_a_move() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (settings, engine) = setup(root);
        let base = settings.parent().unwrap();
        let game = root.join("game");
        let repo = root.join("checkout");
        let ctx = MoveContext { settings: &settings, engine: &engine, game: Some(&game) };
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join(".victory-root"), []).unwrap();
        let not_empty = root.join("occupied");
        fs::create_dir_all(&not_empty).unwrap();
        fs::write(not_empty.join("keep"), []).unwrap();
        for target in [base.join("cache/inner"), base.join("data"), engine.join("storage"), game.clone(), game.join("folder"), repo.join("cache"), not_empty, base.into()] {
            assert!(start_move(&ctx, Location::Cache, &target, "", &AtomicBool::new(false), |_, _| {}).is_err(), "{target:?}");
        }
        let guard = ctx.activity_lock().unwrap();
        assert!(start_move(&ctx, Location::Cache, &root.join("elsewhere"), "", &AtomicBool::new(false), |_, _| {}).is_err());
        drop(guard);
        assert!(read_journal(&ctx.journal().unwrap()).unwrap().is_none());
    }

    #[test]
    fn data_clear_creates_a_recovery_snapshot_first() {
        let temp = tempfile::tempdir().unwrap();
        let (settings, engine) = setup(temp.path());
        let base = settings.parent().unwrap();
        let data = base.join("data");
        let path = data.join("DEADBEEF/saves/slot");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"persistent").unwrap();
        let ctx = MoveContext { settings: &settings, engine: &engine, game: None };
        let root = base.join("backups");
        fs::write(&root, b"cannot create a backup here").unwrap();
        assert!(clear(&ctx, Location::Data, "test").is_err());
        assert!(path.exists());
        fs::remove_file(&root).unwrap();
        clear(&ctx, Location::Data, "test").unwrap();
        assert!(!data.exists());
        let snap = saves::list(&root).unwrap().remove(0);
        assert_eq!(snap.manifest.kind, saves::Kind::BeforeMove);
        assert!(saves::verify(&root, &snap.id).is_ok());
    }
}
