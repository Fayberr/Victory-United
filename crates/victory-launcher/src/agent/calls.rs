//! What agents did: tool name, time and outcome, never arguments or results. The last calls are kept
//! in memory for the Settings tab and in `logs\agent.log`.
use super::lock;
use std::{collections::VecDeque, io::Write, path::PathBuf, sync::Mutex};

pub const KEPT: usize = 50;
const FILE_LINES: usize = 300;
const FILE_LIMIT: u64 = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Error,
    /// A destructive tool without `confirm`: it only said what it would do.
    Asked,
}

impl Outcome {
    pub fn word(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Error => "error",
            Outcome::Asked => "asked",
        }
    }
    fn parse(word: &str) -> Option<Self> {
        [Outcome::Ok, Outcome::Error, Outcome::Asked].into_iter().find(|o| o.word() == word)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// Seconds since 1970, UTC.
    pub time: u64,
    pub tool: String,
    pub outcome: Outcome,
}

impl Call {
    fn line(&self) -> String {
        format!("{} {} {} {}", self.time, crate::core::runner::timestamp_at(self.time), self.tool, self.outcome.word())
    }
    fn parse(line: &str) -> Option<Self> {
        let mut words = line.split(' ');
        let time = words.next()?.parse().ok()?;
        let _readable = words.next()?;
        let tool = words.next()?.to_string();
        Some(Self { time, tool, outcome: Outcome::parse(words.next()?)? })
    }
}

pub struct CallLog {
    file: PathBuf,
    ring: Mutex<VecDeque<Call>>,
}

impl CallLog {
    pub fn open(file: PathBuf) -> Self {
        let old = std::fs::read_to_string(&file).unwrap_or_default();
        let mut ring: VecDeque<Call> = old.lines().rev().filter_map(Call::parse).take(KEPT).collect();
        ring.make_contiguous().reverse();
        Self { file, ring: Mutex::new(ring) }
    }

    /// `tool` must be a name from the tool table, never text a client sent.
    pub fn add(&self, tool: &'static str, outcome: Outcome) {
        let call = Call { time: crate::core::storage::now().as_secs(), tool: tool.into(), outcome };
        let mut ring = lock(&self.ring);
        // The ring lock also keeps two request threads from writing the file at once.
        let _ = self.write(&call);
        if ring.len() == KEPT {
            ring.pop_front();
        }
        ring.push_back(call);
    }

    fn write(&self, call: &Call) -> std::io::Result<()> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::metadata(&self.file).is_ok_and(|m| m.len() > FILE_LIMIT) {
            let old = std::fs::read_to_string(&self.file)?;
            let lines: Vec<&str> = old.lines().collect();
            let mut kept = lines[lines.len().saturating_sub(FILE_LINES)..].join("\n");
            kept.push('\n');
            crate::core::store::write_atomic(&self.file, kept.as_bytes())?;
        }
        let mut file = std::fs::File::options().append(true).create(true).open(&self.file)?;
        writeln!(file, "{}", call.line())
    }

    /// Newest first.
    pub fn recent(&self) -> Vec<Call> {
        lock(&self.ring).iter().rev().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_are_bounded_and_survive_a_restart() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("logs/agent.log");
        let log = CallLog::open(file.clone());
        for _ in 0..2000 {
            log.add("status", Outcome::Ok);
        }
        log.add("play", Outcome::Error);
        assert_eq!(log.recent().len(), KEPT);
        assert_eq!(log.recent()[0].tool, "play");
        assert!(std::fs::metadata(&file).unwrap().len() < FILE_LIMIT + 200);
        let again = CallLog::open(file);
        assert_eq!(again.recent()[0].outcome, Outcome::Error);
        assert_eq!(again.recent().len(), KEPT);
    }
}
