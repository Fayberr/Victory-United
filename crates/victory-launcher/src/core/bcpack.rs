//! The BCPack tables file.
//!
//! Engine builds from c21 on need a file with the constant tables of the BCPack texture format to unpack
//! some textures (character skin and clothes, for example). Without it those textures are never filled and
//! look like coloured checkers. The file is not part of the launcher and not part of the repository
//! (docs/asset-policy.md): the player points the launcher at a copy they may use, or puts it next to the
//! launcher or in the engine folder as `bcpack-tables.bin`. The launcher hands its path to the engine in
//! `VICTORY_BCPACK_TABLES` and checks that the file looks right before it does.
use crate::i18n::tr;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

/// The variable the engine reads.
pub const TABLES_ENV: &str = "VICTORY_BCPACK_TABLES";
/// The name looked for next to the launcher and in the engine folders.
pub const FILE_NAME: &str = "bcpack-tables.bin";

const MODELS: usize = 128;
const BUCKETS: usize = 9;
/// Entries per format dictionary and bytes of one entry across the six formats (see the engine's `Tables::parse`).
const DICTIONARY: usize = 255;
const ENTRY_BYTES: usize = 6 + 16 + 8 + 32 + 2 + 16 + 4 + 32 + 25 + 33 + 50 + 66;
/// Exact size of the file.
pub const TABLES_SIZE: usize = MODELS * BUCKETS * 2 + 256 + 64 + 256 + 64 + 64 + 512 * 2 + DICTIONARY * ENTRY_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Unreadable,
    WrongSize,
    NotTables,
}

impl Problem {
    pub fn text(self) -> &'static str {
        match self {
            Problem::Unreadable => tr("bcpack.problem_unreadable"),
            Problem::WrongSize => tr("bcpack.problem_size"),
            Problem::NotTables => tr("bcpack.problem_content"),
        }
    }
}

/// The same checks the engine makes first: the size, then every model frequency table must have no empty
/// bucket and add up to 65536. A random file of the right size fails the second one.
pub fn check_bytes(bytes: &[u8]) -> Result<(), Problem> {
    if bytes.len() != TABLES_SIZE {
        return Err(Problem::WrongSize);
    }
    for model in 0..MODELS {
        let used = model / 16 + 2;
        let mut total = 0u32;
        for bucket in 0..used {
            let at = (model * BUCKETS + bucket) * 2;
            let freq = u32::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]));
            if freq == 0 {
                return Err(Problem::NotTables);
            }
            total += freq << bucket.saturating_sub(1);
        }
        if total != 1 << 16 {
            return Err(Problem::NotTables);
        }
    }
    Ok(())
}

pub fn check(path: &Path) -> Result<(), Problem> {
    let size = std::fs::metadata(path).map_err(|_| Problem::Unreadable)?.len();
    if size != TABLES_SIZE as u64 {
        return Err(Problem::WrongSize);
    }
    check_bytes(&std::fs::read(path).map_err(|_| Problem::Unreadable)?)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The file the player chose in Settings.
    Chosen(PathBuf),
    /// A file found next to the launcher or in the engine folder.
    Found(PathBuf),
    /// The player chose a file that cannot be used, and none was found elsewhere.
    Refused(PathBuf, Problem),
    Missing,
}

impl Status {
    /// The file the engine will get.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Status::Chosen(p) | Status::Found(p) => Some(p),
            Status::Refused(..) | Status::Missing => None,
        }
    }
}

/// Where the file is looked for besides the player's own choice: the engine's folder, the folder that holds
/// the engine builds, and the launcher's folder.
fn candidates(engine: Option<&Path>, app: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(build) = engine.and_then(Path::parent) {
        out.push(build.join(FILE_NAME));
        if let Some(root) = build.parent() {
            out.push(root.join(FILE_NAME));
        }
    }
    out.push(app.join(FILE_NAME));
    out
}

/// The file a start would use: the player's choice if it is still usable, else one found in the usual places.
pub fn locate(dirs: &BTreeMap<String, String>, engine: Option<&Path>, app: &Path) -> Status {
    let chosen = dirs.get(TABLES_ENV).map(|p| p.trim()).filter(|p| !p.is_empty()).map(PathBuf::from);
    let refused = match &chosen {
        Some(path) => match check(path) {
            Ok(()) => return Status::Chosen(path.clone()),
            Err(problem) => Some((path.clone(), problem)),
        },
        None => None,
    };
    if let Some(found) = candidates(engine, app).into_iter().find(|p| check(p).is_ok()) {
        return Status::Found(found);
    }
    match refused {
        Some((path, problem)) => Status::Refused(path, problem),
        None => Status::Missing,
    }
}

/// Text the engine carries when it has the tables built in (a debug line it logs when it uses them). The
/// engine's source says to keep it.
pub const BUILT_IN_MARK: &str = "bcpack tables: built in";

/// Does this engine build need the tables file from the player? Old builds (c20x) have the tables built in
/// and never read the variable. New builds read it but also carry the built-in tables, so a missing file is
/// no problem there either. Only a build that reads the variable and has no tables of its own (c21 and c22)
/// wants a file. Looks for both texts in the executable, once per file.
pub fn engine_wants_tables(engine: &Path) -> bool {
    type Memo = Option<(PathBuf, Option<SystemTime>, bool)>;
    static MEMO: Mutex<Memo> = Mutex::new(None);
    let stamp = std::fs::metadata(engine).and_then(|m| m.modified()).ok();
    {
        let memo = MEMO.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((path, when, answer)) = memo.as_ref()
            && path == engine && *when == stamp {
                return *answer;
            }
    }
    let found = file_contains(engine, &[TABLES_ENV.as_bytes(), BUILT_IN_MARK.as_bytes()]);
    let answer = found[0] && !found[1];
    *MEMO.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((engine.to_path_buf(), stamp, answer));
    answer
}

/// Which of the needles occur in the file, read in 1 MB chunks (a needle may straddle two chunks).
fn file_contains(path: &Path, needles: &[&[u8]]) -> Vec<bool> {
    let mut found = vec![false; needles.len()];
    let Ok(mut file) = std::fs::File::open(path) else { return found };
    let keep = needles.iter().map(|n| n.len()).max().unwrap_or(1).saturating_sub(1);
    let mut chunk = vec![0u8; 1 << 20];
    let mut window: Vec<u8> = Vec::new();
    loop {
        let Ok(n) = file.read(&mut chunk) else { return found };
        if n == 0 {
            return found;
        }
        window.extend_from_slice(&chunk[..n]);
        for (flag, needle) in found.iter_mut().zip(needles) {
            if !*flag && window.windows(needle.len()).any(|w| w == *needle) {
                *flag = true;
            }
        }
        if found.iter().all(|f| *f) {
            return found;
        }
        let from = window.len().saturating_sub(keep);
        window.drain(..from);
    }
}

/// Bytes that pass `check_bytes` (made up, not the real tables): bucket 0 takes what the others leave of 65536.
#[cfg(test)]
pub(crate) fn sample_bytes() -> Vec<u8> {
    let mut bytes = vec![0u8; TABLES_SIZE];
    for model in 0..MODELS {
        let used = model / 16 + 2;
        let rest: u32 = (1..used).map(|b| 1u32 << (b - 1)).sum();
        for bucket in 0..used {
            let freq: u16 = if bucket == 0 { (65536 - rest) as u16 } else { 1 };
            let at = (model * BUCKETS + bucket) * 2;
            bytes[at..at + 2].copy_from_slice(&freq.to_le_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vu-bcpack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dirs(path: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([(TABLES_ENV.to_string(), path.to_string_lossy().into_owned())])
    }

    #[test]
    fn the_size_matches_the_engines_file() {
        assert_eq!(TABLES_SIZE, 77_982);
    }

    #[test]
    fn a_good_file_passes_and_wrong_ones_are_named() {
        assert_eq!(check_bytes(&sample_bytes()), Ok(()));
        assert_eq!(check_bytes(&sample_bytes()[1..]), Err(Problem::WrongSize));
        assert_eq!(check_bytes(&vec![0; TABLES_SIZE]), Err(Problem::NotTables));
        let mut off = sample_bytes();
        off[0] = off[0].wrapping_add(1);
        assert_eq!(check_bytes(&off), Err(Problem::NotTables), "a model that does not add up to 65536");
    }

    #[test]
    fn the_players_choice_wins_over_a_file_next_to_the_launcher() {
        let root = temp("choice");
        let app = root.join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join(FILE_NAME), sample_bytes()).unwrap();
        let mine = root.join("mine.bin");
        std::fs::write(&mine, sample_bytes()).unwrap();
        assert_eq!(locate(&dirs(&mine), None, &app), Status::Chosen(mine));
        assert_eq!(locate(&BTreeMap::new(), None, &app), Status::Found(app.join(FILE_NAME)));
    }

    #[test]
    fn engine_folders_are_searched_and_a_bad_choice_falls_back() {
        let root = temp("search");
        let build = root.join("app").join("engine").join("b1");
        std::fs::create_dir_all(&build).unwrap();
        let engine = build.join("victory.exe");
        std::fs::write(root.join("app").join("engine").join(FILE_NAME), sample_bytes()).unwrap();
        let bad = root.join("bad.bin");
        std::fs::write(&bad, b"nope").unwrap();
        let app = root.join("app");
        assert_eq!(locate(&BTreeMap::new(), Some(&engine), &app), Status::Found(app.join("engine").join(FILE_NAME)));
        assert_eq!(locate(&dirs(&bad), Some(&engine), &app), Status::Found(app.join("engine").join(FILE_NAME)));
    }

    #[test]
    fn nothing_found_is_missing_and_a_refused_choice_says_why() {
        let root = temp("none");
        let bad = root.join("bad.bin");
        std::fs::write(&bad, b"nope").unwrap();
        assert_eq!(locate(&BTreeMap::new(), None, &root), Status::Missing);
        assert_eq!(locate(&dirs(&bad), None, &root), Status::Refused(bad, Problem::WrongSize));
        assert_eq!(locate(&dirs(&root.join("gone.bin")), None, &root), Status::Refused(root.join("gone.bin"), Problem::Unreadable));
        assert_eq!(Status::Missing.path(), None);
    }

    #[test]
    fn only_engines_that_read_the_variable_and_lack_the_tables_want_the_file() {
        let root = temp("wants");
        let write = |name: &str, texts: &[&str]| {
            let mut body = vec![7u8; (1 << 20) - 5];
            for text in texts {
                body.extend_from_slice(text.as_bytes()); // the first one straddles the first read
                body.extend_from_slice(&[9u8; 100]);
            }
            let path = root.join(name);
            std::fs::write(&path, &body).unwrap();
            path
        };
        let reads_only = write("c22.exe", &[TABLES_ENV]);
        let built_in = write("c23.exe", &[TABLES_ENV, BUILT_IN_MARK]);
        let built_in_first = write("c23b.exe", &[BUILT_IN_MARK, TABLES_ENV]);
        let old = root.join("old.exe");
        std::fs::write(&old, vec![7u8; 3 << 20]).unwrap();
        assert!(engine_wants_tables(&reads_only));
        assert!(!engine_wants_tables(&built_in));
        assert!(!engine_wants_tables(&built_in_first));
        assert!(!engine_wants_tables(&old));
        assert!(!engine_wants_tables(&root.join("missing.exe")));
    }
}
