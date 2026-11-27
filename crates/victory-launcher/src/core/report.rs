//! Bug report tab: builds a ZIP in `reports\` from items the user can look at and untick first. Nothing is
//! uploaded. Text has the Windows user name and the user profile path replaced by placeholders.
use super::{
    actions,
    game::Game,
    hw::Hardware,
    launch::{ExtraFlag, Profile, Resolved},
    runner::{timestamp, RunState},
    store::Settings,
    widgets::{notice, open_folder, page_scroll, page_title},
    CoreState,
};
use crate::{app::Shared, i18n::{tr, trf}, paths, theme};
use eframe::egui;
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub const LOG_LINES: usize = 2000;

pub struct Item {
    pub file: &'static str,
    pub title: &'static str,
    pub text: String,
    pub include: bool,
}

#[derive(Default)]
pub struct ReportUi {
    pub items: Vec<Item>,
    pub built: bool,
    pub message: String,
    pub last_zip: Option<PathBuf>,
}

// ---------------------------------------------------------------- anonymizer

/// Who to hide: the Windows user name and the profile folder.
#[derive(Clone, Debug, Default)]
pub struct Identity {
    pub user: String,
    pub profile: String,
}

impl Identity {
    pub fn current() -> Self {
        Self { user: std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default(), profile: std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default() }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Replaces every case-insensitive whole-word occurrence of `needle`.
fn replace_word(text: &str, needle: &str, with: &str) -> String {
    if needle.chars().count() < 2 {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let n = needle.to_lowercase();
    // Lowercasing can change byte lengths for odd characters: then fall back to the plain text.
    if lower.len() != text.len() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(at) = lower[i..].find(&n) {
        let start = i + at;
        let end = start + n.len();
        let before_ok = text[..start].chars().next_back().is_none_or(|c| !is_word(c));
        let after_ok = text[end..].chars().next().is_none_or(|c| !is_word(c));
        out.push_str(&text[i..start]);
        out.push_str(if before_ok && after_ok { with } else { &text[start..end] });
        i = end;
    }
    out.push_str(&text[i..]);
    out
}

/// Profile path first (both slash styles), then the bare user name.
pub fn anonymize(text: &str, id: &Identity) -> String {
    let mut out = text.to_string();
    if id.profile.chars().count() > 3 {
        let back = id.profile.replace('/', "\\");
        let fwd = id.profile.replace('\\', "/");
        for form in [&back, &fwd, &id.profile] {
            out = replace_ci(&out, form, "%USERPROFILE%");
        }
    }
    if !id.user.is_empty() {
        out = replace_word(&out, &id.user, "<user>");
    }
    out
}

fn replace_ci(text: &str, needle: &str, with: &str) -> String {
    let lower = text.to_lowercase();
    let n = needle.to_lowercase();
    if n.is_empty() || lower.len() != text.len() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(at) = lower[i..].find(&n) {
        out.push_str(&text[i..i + at]);
        out.push_str(with);
        i += at + n.len();
    }
    out.push_str(&text[i..]);
    out
}

// ---------------------------------------------------------------- items

/// The last `lines` lines of a file, reading only its tail.
pub fn tail_lines(path: &Path, lines: usize) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let window = (lines as u64 * 400).min(len).max(1);
    f.seek(SeekFrom::Start(len - window.min(len)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let mut all: Vec<&str> = text.lines().collect();
    if window < len && !all.is_empty() {
        all.remove(0); // probably a cut line
    }
    let from = all.len().saturating_sub(lines);
    Ok(all[from..].join("\n"))
}

/// What a report is built from.
pub struct Inputs<'a> {
    pub settings: &'a Settings,
    pub profile: &'a Profile,
    pub extra: &'a [ExtraFlag],
    /// The final environment and the preset it came from.
    pub resolved: &'a Resolved,
    pub preset: &'a str,
    pub hardware: Option<&'a Hardware>,
    pub rendering_adapter: Option<u64>,
    pub game: Option<&'a Game>,
    pub engine: Option<PathBuf>,
    pub run: &'a RunState,
    /// The log to attach.
    pub log: Option<PathBuf>,
}

impl Inputs<'_> {
    fn engine_build(&self) -> String {
        Some(actions::build_name(self.engine.as_deref())).filter(|n| !n.is_empty()).unwrap_or_else(|| "none".into())
    }
}

pub fn build_items(input: &Inputs<'_>, id: &Identity) -> Vec<Item> {
    let mut items = Vec::new();
    let log_text = match &input.log {
        Some(p) => tail_lines(p, LOG_LINES).unwrap_or_else(|e| format!("(could not read {}: {e})", p.display())),
        None => "(no log yet: the game has not been started)".into(),
    };
    items.push(Item { file: "log-tail.txt", title: "Last lines of the latest log", text: anonymize(&log_text, id), include: true });
    let flags: String = input.resolved.env.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    items.push(Item { file: "flags.txt", title: "Launch flags (final environment)", text: anonymize(&flags, id), include: true });
    let versions = format!(
        "Launcher: {}\nEngine build: {}\nPreset: {}\nConsole: {}\n",
        env!("CARGO_PKG_VERSION"),
        input.engine_build(),
        input.preset,
        input.profile.options.console.arg()
    );
    items.push(Item { file: "versions.txt", title: "Launcher and engine version", text: versions, include: true });
    let hw = input.hardware.map(|h| h.summary_with_adapter(input.rendering_adapter)).unwrap_or_else(|| "(hardware check not finished)\n".into());
    items.push(Item { file: "hardware.txt", title: "Hardware and Windows build", text: anonymize(&hw, id), include: true });
    let mut settings = format!("Profile: {}\n{}", input.profile.name, toml::to_string_pretty(input.profile).unwrap_or_default());
    if !input.extra.is_empty() {
        settings.push_str("\nExtra flags:\n");
        settings.push_str(&super::launch::format_extra(input.extra));
    }
    if let Some(g) = input.game {
        settings.push_str(&format!("\nGame folder: {}\n", g.folder.display()));
    }
    settings.push_str(&format!("Engine: {}\n", if input.settings.engine_override.is_some() { "own build" } else { "installed" }));
    items.push(Item { file: "settings.txt", title: "Settings used", text: anonymize(&settings, id), include: true });
    items
}

/// Writes the checked items into a new ZIP in `dir` and returns its path.
pub fn write_zip(dir: &Path, items: &[Item]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("bug-report-{}.zip", timestamp()));
    let file = std::fs::File::create(&path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for item in items.iter().filter(|i| i.include) {
        zip.start_file(item.file, opts).map_err(std::io::Error::other)?;
        zip.write_all(item.text.as_bytes())?;
    }
    zip.finish().map_err(std::io::Error::other)?;
    Ok(path)
}

/// Short GitHub issue text: facts only, no raw log.
pub fn issue_text(input: &Inputs<'_>, id: &Identity, zip: Option<&Path>) -> String {
    let preset = input.preset;
    let outcome = match input.run {
        RunState::Ended(s) => match (s.error.as_ref(), s.code) {
            (Some(e), _) => format!("could not start ({e})"),
            (None, Some(0)) => "closed normally".to_string(),
            (None, Some(c)) => format!("ended with exit code {c} after {} s", s.secs),
            (None, None) => format!("ended after {} s", s.secs),
        },
        RunState::Running { .. } => "still running".to_string(),
        _ => "not started in this session".to_string(),
    };
    let hw = input.hardware;
    let mut t = String::new();
    t.push_str("Title: Describe the problem in one line\n\n");
    t.push_str("What happened:\n(write here)\n\n");
    t.push_str("Facts from the launcher:\n");
    t.push_str(&format!("- Launcher {}, engine {}\n", env!("CARGO_PKG_VERSION"), input.engine_build()));
    t.push_str(&format!("- Preset {preset}, console {}{}\n", input.profile.options.console.arg(), if input.profile.options.safe { ", safe mode" } else { "" }));
    let extra_on = input.extra.iter().filter(|e| e.enabled).count();
    if extra_on > 0 {
        t.push_str(&format!("- {extra_on} extra flag(s) set\n"));
    }
    if let Some(h) = hw {
        for (key, value) in h.capabilities(input.rendering_adapter) {
            t.push_str(&format!("- {}: {value}\n", tr(key)));
        }
        t.push_str(&format!("- {}, {} MB VRAM, driver {}\n- {} MB RAM, {}, Windows build {}\n", h.gpu_name, h.vram_mb, h.driver, h.ram_mb, h.cpu_name, h.windows_build));
    }
    t.push_str(&format!("- Last run: {outcome}\n"));
    if let Some(z) = zip {
        t.push_str(&format!("- Report ZIP: {} (attach it to this issue)\n", z.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    }
    anonymize(&t, id)
}

// ---------------------------------------------------------------- UI

/// The report inputs as the window sees them.
fn with_inputs<T>(core: &CoreState, work: impl FnOnce(&Inputs<'_>) -> T) -> T {
    // "Start without my flags" changes what ran, but the report lists everything that is set.
    let (resolved, preset) = core.resolved();
    work(&Inputs {
        settings: &core.settings,
        profile: &core.profile,
        extra: &core.extra,
        resolved: &resolved,
        preset: &preset,
        hardware: core.hw.as_ref(),
        rendering_adapter: core.rendering_adapter,
        game: core.game.as_ref(),
        engine: core.engine_exe(),
        run: &core.runner.state,
        log: actions::log_for_report(core.runner.log_file(), &paths::logs_dir()),
    })
}

pub fn show(core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    page_title(ui, tr("report.title"), tr("report.sub"));
    if !core.ui.report.built {
        core.ui.report.built = true;
        core.ui.report.items = with_inputs(core, |input| build_items(input, &Identity::current()));
    }
    let scroll = core.page_scroll.take();
    page_scroll(ui, scroll, |ui| {
        ui.set_max_width(820.0);
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::dim(tr("report.items_help")).size(12.5));
            ui.add_space(6.0);
            for item in core.ui.report.items.iter_mut() {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut item.include, "");
                    ui.label(theme::strong(item.title));
                    ui.label(theme::dim(format!("{} lines, {}", item.text.lines().count(), item.file)).size(12.0));
                });
                egui::CollapsingHeader::new(theme::dim(tr("report.preview")).size(12.5)).id_salt(item.file).default_open(core.ui.demo && item.file == "flags.txt").show(ui, |ui| {
                    let preview: String = item.text.lines().take(40).collect::<Vec<_>>().join("\n");
                    egui::Frame::new().fill(ui.visuals().extreme_bg_color).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new(preview).monospace().size(12.0));
                        if item.text.lines().count() > 40 {
                            ui.label(theme::dim(trf("report.more", &[("n", &(item.text.lines().count() - 40).to_string())])).size(12.0));
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if theme::primary_button(ui, tr("report.build")).clicked() {
                let id = Identity::current();
                if core.ui.demo {
                    core.ui.report.message = tr("report.demo").to_string();
                } else {
                    // Rebuild first so the log tail is current.
                    let checked: Vec<bool> = core.ui.report.items.iter().map(|i| i.include).collect();
                    let mut items = with_inputs(core, |input| build_items(input, &id));
                    for (item, on) in items.iter_mut().zip(checked) {
                        item.include = on;
                    }
                    match write_zip(&paths::reports_dir(), &items) {
                        Ok(p) => {
                            core.ui.report.message = trf("report.saved", &[("file", &p.to_string_lossy())]);
                            ui.ctx().copy_text(with_inputs(core, |input| issue_text(input, &id, Some(&p))));
                            core.ui.report.last_zip = Some(p);
                            core.ui.report.items = items;
                        }
                        Err(e) => core.ui.report.message = format!("{}: {e}", tr("report.failed")),
                    }
                }
            }
            if ui.button(tr("report.copy_issue")).clicked() {
                let zip = core.ui.report.last_zip.clone();
                let text = with_inputs(core, |input| issue_text(input, &Identity::current(), zip.as_deref()));
                ui.ctx().copy_text(text);
                core.ui.report.message = tr("report.copied").to_string();
            }
            if ui.button(tr("report.open_folder")).clicked() {
                open_folder(&paths::reports_dir());
            }
            if ui.button(tr("report.refresh")).clicked() {
                core.ui.report.built = false;
            }
        });
        if !core.ui.report.message.is_empty() {
            ui.add_space(8.0);
            notice(ui, &core.ui.report.message, false);
        }
        ui.add_space(8.0);
        ui.label(theme::dim(tr("report.privacy")).size(12.5));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::tempdir;

    fn id() -> Identity {
        Identity { user: "derfa".into(), profile: r"C:\Users\derfa".into() }
    }

    #[test]
    fn anonymizer_hides_profile_path_and_user_name() {
        let text = r"log in C:\Users\derfa\AppData\Roaming\x and c:/users/DERFA/y, user derfa ran; derfa2 and xderfa stay";
        let out = anonymize(text, &id());
        assert!(!out.contains("derfa\\") && !out.contains("DERFA/"), "{out}");
        assert!(out.contains(r"%USERPROFILE%\AppData\Roaming\x"));
        assert!(out.contains("%USERPROFILE%/y"));
        assert!(out.contains("user <user> ran"));
        assert!(out.contains("derfa2 and xderfa"), "only whole words are replaced: {out}");
        assert_eq!(anonymize("nothing here", &id()), "nothing here");
        // empty identity changes nothing, and a one letter name is left alone
        assert_eq!(anonymize("a b c", &Identity { user: "a".into(), profile: String::new() }), "a b c");
        assert_eq!(anonymize("x", &Identity::default()), "x");
    }

    #[test]
    fn tail_reads_the_last_lines_only() {
        let d = tempdir("tail");
        let f = d.join("run.log");
        let text: String = (0..5000).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&f, text).unwrap();
        let t = tail_lines(&f, 2000).unwrap();
        assert_eq!(t.lines().count(), 2000);
        assert!(t.lines().last().unwrap() == "line 4999" && t.lines().next().unwrap() == "line 3000");
        std::fs::write(&f, "a\nb\n").unwrap();
        assert_eq!(tail_lines(&f, 2000).unwrap(), "a\nb");
    }

    #[test]
    fn zip_holds_only_checked_items() {
        let d = tempdir("zip");
        let items = vec![
            Item { file: "a.txt", title: "A", text: "hello".into(), include: true },
            Item { file: "b.txt", title: "B", text: "secret".into(), include: false },
        ];
        let p = write_zip(&d, &items).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(p).unwrap()).unwrap();
        assert_eq!(z.len(), 1);
        let mut s = String::new();
        z.by_name("a.txt").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello");
    }
}
