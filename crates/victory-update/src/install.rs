//! Download, verify, install, switch, roll back, clean up.
use crate::{Engine, Error, Result, http::Http};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Downloads `engine.url` to `dest_zip`. Checks size and SHA-256 on the way; the file only gets its
/// final name when both match. `progress(done, total)` is called for every chunk, return false to
/// cancel. A leftover `.part` file is replaced.
pub fn download(http: &dyn Http, engine: &Engine, dest_zip: &Path, progress: &mut dyn FnMut(u64, u64) -> bool) -> Result<()> {
    let part = dest_zip.with_extension("part");
    if let Some(dir) = dest_zip.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::File::create(&part)?;
    let mut hash = Sha256::new();
    let mut done = 0u64;
    let mut io_err = None;
    let r = http.get(&engine.url, &mut |chunk, total| {
        hash.update(chunk);
        done += chunk.len() as u64;
        if let Err(e) = file.write_all(chunk) {
            io_err = Some(e);
            return false;
        }
        progress(done, total.unwrap_or(engine.size))
    });
    drop(file);
    let fail = |e: Error| {
        let _ = std::fs::remove_file(&part);
        Err(e)
    };
    if let Some(e) = io_err {
        return fail(e.into());
    }
    if let Err(e) = r {
        return fail(e);
    }
    if done != engine.size {
        return fail(Error::Corrupt(format!("expected {} bytes, got {done}", engine.size)));
    }
    let got = hex(&hash.finalize());
    if !got.eq_ignore_ascii_case(&engine.sha256) {
        return fail(Error::Corrupt(format!("SHA-256 is {got}, the manifest says {}", engine.sha256)));
    }
    std::fs::rename(&part, dest_zip)?;
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// SHA-256 of a file, lower case hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(hex(&h.finalize()));
        }
        h.update(&buf[..n]);
    }
}

/// Unpacks `zip_path` into `engine_root\<name>`, then makes it the active build. The ZIP must hold
/// `victory.exe` at its top level (or inside one single top folder, which is stripped). Order:
/// extract into `<name>.partial`, rename to `<name>`, write `previous.txt`, write `current.txt` last.
/// Whatever fails before the last step leaves `current.txt` as it was.
pub fn install(zip_path: &Path, engine_root: &Path, name: &str) -> Result<()> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return Err(Error::BadArchive(format!("bad folder name {name:?}")));
    }
    std::fs::create_dir_all(engine_root)?;
    let target = engine_root.join(name);
    let partial = engine_root.join(format!("{name}.partial"));
    if partial.exists() {
        std::fs::remove_dir_all(&partial)?;
    }
    if let Err(e) = extract(zip_path, &partial) {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(e);
    }
    if !partial.join("victory.exe").is_file() {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(Error::BadArchive("victory.exe is not in the package".into()));
    }
    if target.exists() {
        // Same build installed again: replace it (the old one may be the active one, which is fine
        // because the flip below names it again).
        std::fs::remove_dir_all(&target)?;
    }
    std::fs::rename(&partial, &target)?;
    set_current(engine_root, name)
}

fn extract(zip_path: &Path, dest: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?).map_err(|e| Error::BadArchive(e.to_string()))?;
    // One single top folder holding everything is stripped.
    let mut tops = std::collections::BTreeSet::new();
    let mut loose = false;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| Error::BadArchive(e.to_string()))?;
        match f.enclosed_name() {
            Some(p) => match p.components().count() {
                0 => {}
                1 if !f.is_dir() => loose = true,
                _ => {
                    tops.insert(p.components().next().unwrap().as_os_str().to_owned());
                }
            },
            None => return Err(Error::BadArchive(format!("unsafe path {:?}", f.name()))),
        }
    }
    let strip = !loose && tops.len() == 1;
    std::fs::create_dir_all(dest)?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| Error::BadArchive(e.to_string()))?;
        let rel = f.enclosed_name().ok_or_else(|| Error::BadArchive("unsafe path".into()))?;
        let rel: PathBuf = if strip { rel.components().skip(1).collect() } else { rel };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::io::copy(&mut f, &mut std::fs::File::create(&out)?)?;
        }
    }
    Ok(())
}

/// Makes `name` the active build and remembers the one it replaces. `current.txt` is replaced
/// atomically (write a temporary file, rename over it).
pub fn set_current(engine_root: &Path, name: &str) -> Result<()> {
    if !engine_root.join(name).join("victory.exe").is_file() {
        return Err(Error::BadArchive(format!("{name} is not an installed build")));
    }
    let current = engine_root.join("current.txt");
    if let Ok(old) = std::fs::read_to_string(&current) {
        let old = old.trim();
        if !old.is_empty() && old != name {
            std::fs::write(engine_root.join("previous.txt"), old)?;
        }
    }
    let tmp = engine_root.join("current.txt.new");
    std::fs::write(&tmp, name)?;
    std::fs::rename(&tmp, &current)?;
    Ok(())
}

/// Switches back to `to`, or to the build named in `previous.txt` when `to` is `None`.
/// Returns the name now active.
pub fn rollback(engine_root: &Path, to: Option<&str>) -> Result<String> {
    let name = match to {
        Some(n) => n.to_string(),
        None => std::fs::read_to_string(engine_root.join("previous.txt"))
            .map_err(|_| Error::BadArchive("there is no earlier build to go back to".into()))?
            .trim()
            .to_string(),
    };
    set_current(engine_root, &name)?;
    Ok(name)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub name: String,
    pub is_current: bool,
    pub is_previous: bool,
    pub modified: std::time::SystemTime,
    pub bytes: u64,
}

/// Installed builds (folders that hold `victory.exe`), newest first.
pub fn installed(engine_root: &Path) -> Vec<Installed> {
    let read = |f: &str| std::fs::read_to_string(engine_root.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
    let (cur, prev) = (read("current.txt"), read("previous.txt"));
    let mut out = Vec::new();
    for e in std::fs::read_dir(engine_root).into_iter().flatten().flatten() {
        let p = e.path();
        if !p.join("victory.exe").is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        out.push(Installed { is_current: name == cur, is_previous: name == prev, bytes: dir_size(&p), name, modified });
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
    out
}

fn dir_size(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Removes old builds, keeping the active one, the rollback target and the newest others up to
/// `keep` in all. Also removes leftover `.partial` folders. Returns the names removed.
pub fn cleanup(engine_root: &Path, keep: usize) -> Vec<String> {
    let mut removed = Vec::new();
    for e in std::fs::read_dir(engine_root).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.ends_with(".partial") && std::fs::remove_dir_all(e.path()).is_ok() {
            removed.push(n);
        }
    }
    let all = installed(engine_root);
    let mut kept = all.iter().filter(|b| b.is_current || b.is_previous).count();
    for b in &all {
        if b.is_current || b.is_previous {
            continue;
        }
        if kept < keep {
            kept += 1;
            continue;
        }
        if std::fs::remove_dir_all(engine_root.join(&b.name)).is_ok() {
            removed.push(b.name.clone());
        }
    }
    removed
}
