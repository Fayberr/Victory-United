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
