//! Finding the game. A game folder is recognised by its `MicrosoftGame.Config` file, like the engine does
//! (`apps/victory/src/run.rs` reads it from the executable's folder). Auto-detection looks only at the
//! usual Xbox app install roots, one or two levels deep, never over whole drives.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub folder: PathBuf,
    pub exe: PathBuf,
    /// Executable name as the config names it (or the folder name when it does not).
    pub title: String,
}

const CONFIG_NAME: &str = "MicrosoftGame.Config";

/// The config file in `dir`, whatever its capitalisation.
pub fn find_config(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join(CONFIG_NAME);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(CONFIG_NAME))
    })
}

/// `Name` of the first `<Executable ...>` element, without a real XML parser (the file is small and the
/// attribute is all we need).
pub fn executable_from_config(text: &str) -> Option<String> {
    let at = text.find("<Executable ")?;
    let tag = &text[at..at + text[at..].find('>')?];
    let n = tag.find("Name=\"")? + 6;
    let end = tag[n..].find('"')?;
    let name = &tag[n..n + end];
    (!name.is_empty()).then(|| name.to_string())
}

fn folder_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Checks one folder: it must hold the config, and the executable must exist.
pub fn from_folder(dir: &Path) -> Option<Game> {
    let config = find_config(dir)?;
    let text = std::fs::read_to_string(&config).unwrap_or_default();
    let exe = match executable_from_config(&text) {
        Some(name) if dir.join(&name).is_file() => dir.join(name),
        _ => only_exe(dir)?,
    };
    Some(Game { folder: dir.to_path_buf(), title: exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| folder_name(dir)), exe })
}

fn only_exe(dir: &Path) -> Option<PathBuf> {
    let mut exes = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")));
    let first = exes.next()?;
    exes.next().is_none().then_some(first)
}

/// What the user picked: a game folder, an Xbox title folder one level above `Content`, or the executable.
pub fn from_pick(pick: &Path) -> Option<Game> {
    if pick.is_file() {
        let dir = pick.parent()?;
        let mut game = from_folder(dir)?;
        game.title = pick.file_stem()?.to_string_lossy().into_owned();
        game.exe = pick.to_path_buf();
        return Some(game);
    }
    from_folder(pick).or_else(|| from_folder(&pick.join("Content")))
}

/// Looks inside each root: the root itself, its children and their `Content` folders.
pub fn scan_roots(roots: &[PathBuf]) -> Vec<Game> {
    let mut found: Vec<Game> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        for entry in entries.flatten().take(400) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let game = from_folder(&path).or_else(|| from_folder(&path.join("Content")));
            if let Some(g) = game {
                if !found.iter().any(|f| f.folder == g.folder) {
                    found.push(g);
                }
            }
        }
    }
    found
}

/// Root folders worth looking at on a drive.
pub fn candidate_roots(drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for d in drives {
        for sub in ["XboxGames", "Program Files/ModifiableWindowsApps", "Games", "Program Files/Xbox Games"] {
            roots.push(d.join(sub));
        }
    }
    roots
}

/// Fixed and removable drives, by the Windows drive list (a plain bit mask, no disk access).
#[cfg(windows)]
pub fn drives() -> Vec<PathBuf> {
    use windows::{core::PCWSTR, Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives}};
    let mask = unsafe { GetLogicalDrives() };
    (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| {
            let root = format!("{}:\\", (b'A' + i as u8) as char);
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            // 2 removable, 3 fixed. Network and optical drives are skipped on purpose (they can stall).
            let kind = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
            (kind == 2 || kind == 3).then(|| PathBuf::from(root))
        })
        .collect()
}

#[cfg(not(windows))]
pub fn drives() -> Vec<PathBuf> {
    Vec::new()
}

pub fn auto_detect() -> Vec<Game> {
    scan_roots(&candidate_roots(&drives()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::tempdir;

    fn make_game(dir: &Path, exe: &str, config_name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(exe), b"MZ").unwrap();
        std::fs::write(
            dir.join(config_name),
            format!("<Game><ExecutableList><Executable Name=\"{exe}\" Id=\"Game\"/></ExecutableList></Game>"),
        )
        .unwrap();
    }

    #[test]
    fn parses_executable_name() {
        assert_eq!(executable_from_config("<Executable Name=\"a.exe\" Id=\"x\">"), Some("a.exe".into()));
        assert_eq!(executable_from_config("<Game/>"), None);
    }

    #[test]
    fn folder_exe_and_content_subfolder_are_recognised() {
        let t = tempdir("game");
        make_game(&t.join("One"), "one.exe", "MicrosoftGame.Config");
        let g = from_pick(&t.join("One")).unwrap();
        assert_eq!(g.exe, t.join("One").join("one.exe"));
        assert_eq!(from_pick(&t.join("One").join("one.exe")).unwrap().folder, t.join("One"));
        make_game(&t.join("Two").join("Content"), "two.exe", "microsoftgame.config");
        assert_eq!(from_pick(&t.join("Two")).unwrap().exe, t.join("Two").join("Content").join("two.exe"));
        std::fs::create_dir_all(t.join("Empty")).unwrap();
        assert!(from_pick(&t.join("Empty")).is_none());
    }

    #[test]
    fn scan_finds_titles_in_roots_without_going_deeper() {
        let t = tempdir("scan");
        let root = t.join("XboxGames");
        make_game(&root.join("A").join("Content"), "a.exe", "MicrosoftGame.Config");
        make_game(&root.join("B"), "b.exe", "MicrosoftGame.Config");
        make_game(&root.join("C").join("deep").join("deeper"), "c.exe", "MicrosoftGame.Config");
        let found = scan_roots(&[root, t.join("missing")]);
        let mut names: Vec<_> = found.iter().map(|g| g.title.clone()).collect();
        names.sort();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn config_without_exe_uses_the_single_exe() {
        let t = tempdir("noexe");
        std::fs::write(t.join("MicrosoftGame.Config"), "<Game/>").unwrap();
        std::fs::write(t.join("only.exe"), b"MZ").unwrap();
        assert_eq!(from_folder(&t).unwrap().exe, t.join("only.exe"));
    }
}
