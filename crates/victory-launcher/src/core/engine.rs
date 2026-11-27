//! Which engine the launcher runs.
//!
//! The engine is part of the launcher: it is installed next to it (`engine\<build>\victory.exe`) and
//! updated together with it. A developer can point the launcher at another build; that choice is checked
//! here so a wrong file (the launcher itself, any other program) can never be started as "the engine".
use std::path::Path;

pub const ENGINE_FILE: &str = "victory.exe";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Missing,
    /// The file is not called `victory.exe`.
    WrongName,
    /// The file is this launcher. Starting it would open a second launcher window.
    ThisLauncher,
}

impl Problem {
    pub fn text(self) -> &'static str {
        match self {
            Problem::Missing => crate::i18n::tr("engine.problem_missing"),
            Problem::WrongName => crate::i18n::tr("engine.problem_name"),
            Problem::ThisLauncher => crate::i18n::tr("engine.problem_launcher"),
        }
    }
}

/// Is `path` something the launcher may start as the engine?
pub fn check(path: &Path) -> Result<(), Problem> {
    check_against(path, std::env::current_exe().ok().as_deref())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn check_against(path: &Path, this_launcher: Option<&Path>) -> Result<(), Problem> {
    if !path.is_file() {
        return Err(Problem::Missing);
    }
    if this_launcher.is_some_and(|me| same_file(path, me)) {
        return Err(Problem::ThisLauncher);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.to_ascii_lowercase().contains("launcher") {
        return Err(Problem::ThisLauncher);
    }
    if !name.eq_ignore_ascii_case(ENGINE_FILE) {
        return Err(Problem::WrongName);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::tempdir;

    #[test]
    fn only_a_real_victory_exe_is_accepted() {
        let t = tempdir("engine-check");
        let ok = t.join("Victory.EXE");
        std::fs::write(&ok, b"MZ").unwrap();
        assert_eq!(check_against(&ok, None), Ok(()));
        assert_eq!(check_against(&t.join("missing.exe"), None), Err(Problem::Missing));
        let other = t.join("notepad.exe");
        std::fs::write(&other, b"MZ").unwrap();
        assert_eq!(check_against(&other, None), Err(Problem::WrongName));
        assert_eq!(check_against(&t, None), Err(Problem::Missing));
    }

    #[test]
    fn the_launcher_itself_is_never_the_engine() {
        let t = tempdir("engine-self");
        let me = t.join("victory-launcher.exe");
        std::fs::write(&me, b"MZ").unwrap();
        assert_eq!(check_against(&me, Some(&me)), Err(Problem::ThisLauncher));
        // Another copy of the launcher (a different folder, or the installed one when running from a download).
        let copy = t.join("Victory United Launcher.exe");
        std::fs::write(&copy, b"MZ").unwrap();
        assert_eq!(check_against(&copy, Some(&me)), Err(Problem::ThisLauncher));
        assert_eq!(check_against(&t.join("Victory-Launcher.EXE"), None), Err(Problem::Missing));
    }
}
