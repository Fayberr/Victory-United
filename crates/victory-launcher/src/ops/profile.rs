//! Sampling schedule and profile file operations shared by the launcher and other callers.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

pub type Result<T> = anyhow::Result<T>;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    pub windows: Vec<Window>,
    pub sample_ms: u32,
    pub threads: u32,
    pub filter: String,
    pub depth: u32,
    pub symbolize: bool,
    pub folder: String,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            enabled: false,
            windows: vec![Window {
                from: 20.0,
                to: 40.0,
            }],
            sample_ms: 5,
            threads: 28,
            filter: String::new(),
            depth: 40,
            symbolize: true,
            folder: String::new(),
        }
    }
}

impl Schedule {
    pub fn folder(&self, data: &Path) -> PathBuf {
        let folder = self.folder.trim();
        if folder.is_empty() {
            data.join("profiler")
        } else {
            data.join(folder)
        }
    }
}

pub fn launch_flags(
    schedule: &Schedule,
    data: &Path,
    run_id: u128,
) -> Result<BTreeMap<String, String>> {
    let mut flags = BTreeMap::new();
    if !schedule.enabled {
        return Ok(flags);
    }
    anyhow::ensure!(
        (1..=50).contains(&schedule.sample_ms),
        "Sample interval must be 1 to 50 ms."
    );
    anyhow::ensure!(
        (4..=28).contains(&schedule.threads),
        "Thread count must be 4 to 28."
    );
    anyhow::ensure!(
        (1..=40).contains(&schedule.depth),
        "Stack depth must be 1 to 40."
    );
    anyhow::ensure!(
        !schedule.filter.contains('\0'),
        "Thread filter contains a null character."
    );
    anyhow::ensure!(
        (1..=32).contains(&schedule.windows.len()),
        "Set one to 32 sampling windows."
    );
    let mut windows = schedule.windows.clone();
    for Window { from, to } in &windows {
        anyhow::ensure!(
            std::time::Duration::try_from_secs_f64(*from).is_ok()
                && std::time::Duration::try_from_secs_f64(*to).is_ok()
                && from < to,
            "Each window needs a start of zero or later and an end after its start."
        );
    }
    windows.sort_by(|a, b| a.from.total_cmp(&b.from));
    anyhow::ensure!(
        windows.windows(2).all(|w| w[0].to <= w[1].from),
        "Sampling windows overlap."
    );
    let folder = schedule.folder(data);
    let mut segments = Vec::with_capacity(windows.len());
    for (i, Window { from, to }) in windows.iter().enumerate() {
        let path = folder.join(format!("profile-{run_id:032}-{i:02}.txt"));
        let path = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("The profile folder must be valid Unicode."))?;
        anyhow::ensure!(
            !path.contains([';', '\0', '\n', '\r']),
            "The profile folder cannot contain semicolons or line breaks."
        );
        segments.push(format!("{from},{to},{},{path}", schedule.sample_ms));
    }
    flags.insert("VICTORY_PROFILE".into(), segments.join(";"));
    flags.insert(
        "VICTORY_PROFILE_THREADS".into(),
        schedule.threads.to_string(),
    );
    flags.insert("VICTORY_PROFILE_DEPTH".into(), schedule.depth.to_string());
    flags.insert(
        "VICTORY_PROFILE_FILTER".into(),
        schedule.filter.trim().into(),
    );
    flags.insert(
        "VICTORY_PROFILE_SYMBOLIZE".into(),
        u8::from(schedule.symbolize).to_string(),
    );
    Ok(flags)
}

pub fn prepare_launch(schedule: &Schedule, data: &Path) -> Result<BTreeMap<String, String>> {
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let flags = launch_flags(schedule, data, run_id)?;
    if schedule.enabled {
        std::fs::create_dir_all(schedule.folder(data))?;
    }
    Ok(flags)
}

pub fn latest(folder: &Path) -> Result<PathBuf> {
    std::fs::read_dir(folder)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name = name.to_str()?;
            if !name.starts_with("profile-") || !name.ends_with(".txt") {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            metadata
                .is_file()
                .then(|| (metadata.modified().ok(), entry.path()))
        })
        .max()
        .map(|(_, path)| path)
        .ok_or_else(|| anyhow::anyhow!("No completed profiles in {}", folder.display()))
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub id: u32,
    pub name: String,
    pub attempted: u64,
}

impl Thread {
    fn label(&self, merge: bool) -> String {
        match (self.name.is_empty(), merge) {
            (true, _) => format!("#{}", self.id),
            (false, true) => self.name.clone(),
            (false, false) => format!("{} · #{}", self.name, self.id),
        }
    }
}

#[derive(Clone, Debug)]
struct Stack {
    thread: u32,
    count: u64,
    frames: Vec<usize>,
}

#[derive(Default, Debug)]
pub struct Profile {
    pub rounds: u64,
    pub threads: BTreeMap<u32, Thread>,
    symbols: Vec<String>,
    stacks: Vec<Stack>,
}

pub fn open(path: &Path) -> Result<Profile> {
    const MAX_BYTES: u64 = 64 << 20;
    let file = std::fs::File::open(path)?;
    anyhow::ensure!(
        file.metadata()?.len() <= MAX_BYTES,
        "The profile exceeds 64 MB."
    );
    parse(BufReader::new(file.take(MAX_BYTES + 1)))
}

pub fn parse(reader: impl BufRead) -> Result<Profile> {
    let mut profile = Profile::default();
    let mut symbols = HashMap::new();
    let mut has_rounds = false;
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let result = (|| -> Result<()> {
            let (record, payload) = line
                .split_once(' ')
                .ok_or_else(|| anyhow::anyhow!("Missing record fields"))?;
            match record {
                "rounds" => {
                    profile.rounds = payload.parse()?;
                    has_rounds = true;
                }
                "phase" => {
                    let (phase, rounds) = payload
                        .split_once(' ')
                        .ok_or_else(|| anyhow::anyhow!("Missing phase count"))?;
                    phase.parse::<u32>()?;
                    rounds.parse::<u64>()?;
                }
                "thread" => {
                    let mut fields = payload.splitn(5, ' ');
                    let id = field(&mut fields)?.parse()?;
                    fields.next(); // legacy engines leave the start address empty when unavailable
                    field(&mut fields)?.parse::<u64>()?; // CPU time
                    let attempted = field(&mut fields)?.parse()?;
                    let name = fields.next().unwrap_or("").trim().to_owned();
                    profile.threads.insert(
                        id,
                        Thread {
                            id,
                            name,
                            attempted,
                        },
                    );
                }
                "stack" => {
                    let mut fields = payload.splitn(4, ' ');
                    let thread = field(&mut fields)?.parse()?;
                    field(&mut fields)?.parse::<u32>()?; // phase
                    let count = field(&mut fields)?.parse::<u64>()?;
                    let frames = field(&mut fields)?
                        .split(';')
                        .filter(|s| !s.is_empty())
                        .map(|name| {
                            *symbols.entry(name.to_owned()).or_insert_with(|| {
                                let i = profile.symbols.len();
                                profile.symbols.push(name.to_owned());
                                i
                            })
                        })
                        .collect::<Vec<_>>();
                    anyhow::ensure!(!frames.is_empty() && count != 0, "Empty stack");
                    profile.stacks.push(Stack {
                        thread,
                        count,
                        frames,
                    });
                }
                _ => anyhow::bail!("Unknown record {record}"),
            }
            Ok(())
        })();
        result.map_err(|error| anyhow::anyhow!("Profile line {}: {error}", index + 1))?;
    }
    anyhow::ensure!(has_rounds, "This file has no profiler header.");
    Ok(profile)
}

fn field<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Result<&'a str> {
    fields
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing record field"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Thread,
    #[default]
    Function,
    Module,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    pub group: Group,
    pub top: usize,
    pub minimum_percent: f64,
    pub merge_threads: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            group: Group::Function,
            top: 20,
            minimum_percent: 0.0,
            merge_threads: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub self_samples: u64,
    pub inclusive_samples: u64,
    pub self_percent: f64,
    pub inclusive_percent: f64,
}

pub fn overview(
    profile: &Profile,
    view: &View,
    thread: Option<u32>,
    query: &str,
) -> Result<Vec<Row>> {
    anyhow::ensure!((1..=250).contains(&view.top), "Top rows must be 1 to 250.");
    anyhow::ensure!(
        (0.0..=100.0).contains(&view.minimum_percent),
        "Minimum share must be 0 to 100 percent."
    );
    let query = query.trim().to_lowercase();
    let mut tally = HashMap::<String, [u64; 2]>::new();
    let mut total = 0u64;
    for stack in profile
        .stacks
        .iter()
        .filter(|s| thread.is_none_or(|id| id == s.thread))
    {
        total = total.saturating_add(stack.count);
        if view.group == Group::Thread {
            let name = profile.threads.get(&stack.thread).map_or_else(
                || format!("#{}", stack.thread),
                |t| t.label(view.merge_threads),
            );
            let sums = tally.entry(name).or_default();
            sums[0] = sums[0].saturating_add(stack.count);
            sums[1] = sums[0];
            continue;
        }
        let mut seen = HashSet::new();
        for (i, frame) in stack.frames.iter().enumerate() {
            let symbol = &profile.symbols[*frame];
            let name = if view.group == Group::Module {
                module(symbol)
            } else {
                symbol
            };
            if seen.insert(name) {
                let sums = tally.entry(name.to_owned()).or_default();
                sums[1] = sums[1].saturating_add(stack.count);
                if i == 0 {
                    sums[0] = sums[0].saturating_add(stack.count);
                }
            }
        }
    }
    let share = |count: u64| 100.0 * count as f64 / total.max(1) as f64;
    let mut rows = tally
        .into_iter()
        .filter_map(|(name, [own, inclusive])| {
            (share(inclusive) >= view.minimum_percent
                && (query.is_empty() || name.to_lowercase().contains(&query)))
            .then(|| Row {
                name,
                self_samples: own,
                inclusive_samples: inclusive,
                self_percent: share(own),
                inclusive_percent: share(inclusive),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_unstable_by(|a, b| {
        b.self_samples
            .cmp(&a.self_samples)
            .then(b.inclusive_samples.cmp(&a.inclusive_samples))
            .then(a.name.cmp(&b.name))
    });
    rows.truncate(view.top);
    Ok(rows)
}

fn module(symbol: &str) -> &str {
    symbol
        .split_once('!')
        .or_else(|| symbol.rsplit_once("+0x"))
        .map_or("(unknown module)", |(name, _)| name)
}

pub fn as_text(rows: &[Row]) -> Result<String> {
    use std::fmt::Write;
    let mut text = String::from("Name\tSelf %\tInclusive %\tSelf samples\tInclusive samples\n");
    for r in rows {
        writeln!(
            text,
            "{}\t{:.2}\t{:.2}\t{}\t{}",
            r.name, r.self_percent, r.inclusive_percent, r.self_samples, r.inclusive_samples
        )?;
    }
    Ok(text)
}

pub fn export_csv(path: &Path, rows: &[Row]) -> Result<()> {
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    writeln!(
        out,
        "name,self_percent,inclusive_percent,self_samples,inclusive_samples"
    )?;
    for r in rows {
        writeln!(
            out,
            "\"{}\",{:.4},{:.4},{},{}",
            r.name.replace('"', "\"\""),
            r.self_percent,
            r.inclusive_percent,
            r.self_samples,
            r.inclusive_samples
        )?;
    }
    out.flush()?;
    Ok(())
}

pub const DEMO: &str = "\
rounds 4000
phase 4 4000
thread 221 victory.exe+0x114c0 19580 4000 Victory render
thread 222 victory.exe+0x1cc70 15461 4000 Victory copy
thread 223 victory.exe+0x17f00 15149 4000 Victory pipeline
stack 221 4 2100 victory.exe!prepare_submission;victory.exe!submit;victory.exe!render_thread;
stack 221 4 1300 D3D12Core.dll!ExecuteCommandLists;victory.exe!submit;victory.exe!render_thread;
stack 222 4 1850 victory.exe!refresh_buffers;victory.exe!copy_worker;
stack 222 4 1050 victory.exe!compare_readback;victory.exe!copy_worker;
stack 223 4 1720 nvwgf2umx.dll+0x62f3;victory.exe!build_pipeline;
stack 223 4 830 ntdll.dll!NtWaitForSingleObject;victory.exe!pipeline_worker;
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_is_sorted_and_creates_a_folder_only_when_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let schedule = Schedule {
            enabled: true,
            windows: vec![Window { from: 4., to: 6. }, Window { from: 1., to: 3. }],
            ..Default::default()
        };
        let flags = prepare_launch(&schedule, temp.path()).unwrap();
        assert!(schedule.folder(temp.path()).is_dir());
        assert!(flags["VICTORY_PROFILE"].starts_with("1,3,5,"));
        assert!(flags["VICTORY_PROFILE"].contains(";4,6,5,"));
        assert_eq!(flags["VICTORY_PROFILE_THREADS"], "28");
        let mut bad = schedule;
        bad.windows[1].to = 5.;
        assert!(launch_flags(&bad, temp.path(), 1).is_err());
        bad.windows[1].to = f64::INFINITY;
        assert!(launch_flags(&bad, temp.path(), 1).is_err());
    }

    #[test]
    fn profile_file_aggregates_recursion_and_drills_down() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profile-0001.txt");
        std::fs::write(&path, "rounds 5\nthread 1 x 1 4 Same worker\nthread 2 x 1 4 Same worker\nstack 1 0 3 x!a;x!b;x!a;\nstack 2 0 1 y+0x10;x!a;\n").unwrap();
        let profile = open(&latest(temp.path()).unwrap()).unwrap();
        let rows = overview(&profile, &View::default(), None, "").unwrap();
        assert_eq!((rows[0].self_samples, rows[0].inclusive_samples), (3, 4));
        let rows = overview(
            &profile,
            &View {
                group: Group::Module,
                ..Default::default()
            },
            Some(1),
            "x",
        )
        .unwrap();
        assert_eq!(
            (rows[0].self_percent, rows[0].inclusive_percent),
            (100., 100.)
        );
        let rows = overview(
            &profile,
            &View {
                group: Group::Thread,
                merge_threads: true,
                ..Default::default()
            },
            None,
            "",
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(as_text(&rows).unwrap().contains("Same worker\t100.00"));
        let csv = temp.path().join("profile.csv");
        export_csv(&csv, &rows).unwrap();
        assert!(
            std::fs::read_to_string(csv)
                .unwrap()
                .contains("\"Same worker\",100.0000")
        );
        assert!(parse("rounds 5\nstack 1 stupid".as_bytes()).is_err());
    }
}
