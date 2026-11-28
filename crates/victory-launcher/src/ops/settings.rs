//! The ops half's own small settings file (`ops.toml` next to `settings.toml`): in-game panel switch, monitor layout, update checks.
use crate::paths;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OpsSettings {
    /// Show the engine's in-game panel while playing (`VICTORY_PANEL=1`).
    pub panel: bool,
    pub monitor: MonitorSettings,
    pub profiler: super::profile::Schedule,
    pub profile_view: super::profile::View,
    /// `stable` or `dev`.
    pub channel: String,
    /// Which engine build to download: `release` or `diagnostics`.
    pub engine_kind: String,
}

impl Default for OpsSettings {
    fn default() -> Self {
        Self {
            panel: false,
            monitor: MonitorSettings::default(),
            profiler: super::profile::Schedule::default(),
            profile_view: super::profile::View::default(),
            channel: "stable".into(),
            engine_kind: "release".into(),
        }
    }
}

impl OpsSettings {
    pub fn file() -> std::path::PathBuf {
        paths::data_dir().join("ops.toml")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::file())
    }

    pub fn load_from(file: &std::path::Path) -> Self {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::file())
    }

    pub fn save_to(&self, file: &std::path::Path) -> anyhow::Result<()> {
        crate::core::store::write_atomic(file, toml::to_string_pretty(self)?.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_defaults() {
        let s = OpsSettings {
            panel: true,
            ..Default::default()
        };
        let t = toml::to_string_pretty(&s).unwrap();
        assert_eq!(toml::from_str::<OpsSettings>(&t).unwrap(), s);
        // Missing keys fall back to defaults; unknown keys do not break loading.
        assert_eq!(
            toml::from_str::<OpsSettings>("panel = true\nfuture = 1")
                .unwrap()
                .channel,
            "stable"
        );
    }

    #[test]
    fn old_overlay_keys_are_ignored() {
        let old = "overlay = true\noverlay_corner = \"top_right\"\nchannel = \"dev\"\n\
                   [overlay_options]\nmode = \"window\"\nhotkey = \"Ctrl+F9\"\nscale_percent = 150\n";
        let loaded = toml::from_str::<OpsSettings>(old).unwrap();
        assert!(!loaded.panel);
        assert_eq!(loaded.channel, "dev");
    }

    #[test]
    fn the_panel_switch_sets_the_engine_flag_only_when_on() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = OpsSettings::default();
        assert!(engine_options(&config, temp.path()).unwrap().is_empty());
        config.panel = true;
        assert_eq!(
            engine_options(&config, temp.path()).unwrap()["VICTORY_PANEL"],
            "1"
        );
        assert_eq!(panel_flag(&OpsSettings::default()), None);
    }
}

/// Display choices use stable serialized names so old settings can be upgraded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Fps,
    LowFps,
    FrameMs,
    GpuEstimate,
    GpuIdleCpu,
    CpuMs,
    GpuMs,
    GpuLoad,
    GpuTemp,
    CpuLoad,
    Vram,
    Ram,
    SystemRam,
    CopyTo,
    CopyFrom,
    InPlaceBytes,
    InPlacePages,
    Draws,
    DrawsIssued,
    Dispatches,
    DispatchesIssued,
    PipelineDrops,
    IndirectDrops,
    PresentMs,
    Pipelines,
    PipelineWait,
    TextureRoom,
    TextureTileRoom,
    TextureUploads,
    WriteWatch,
    Frame,
    Missed,
    Occluded,
    Rejected,
}

impl Field {
    pub fn label(self) -> &'static str {
        use Field::*;
        crate::i18n::tr(match self {
            Fps => "monitor.fps",
            LowFps => "monitor.low",
            FrameMs => "monitor.frame",
            GpuEstimate => "monitor.gpu_estimate",
            GpuIdleCpu => "monitor.gpu_idle_cpu",
            CpuMs => "monitor.cpu",
            GpuMs => "monitor.gpu",
            GpuLoad => "monitor.gpu_load",
            GpuTemp => "monitor.gpu_temp",
            CpuLoad => "monitor.cpu_load",
            Vram => "monitor.vram",
            Ram => "monitor.ram",
            SystemRam => "monitor.system_ram",
            CopyTo => "monitor.copy_to",
            CopyFrom => "monitor.copy_from",
            InPlaceBytes => "monitor.in_place_bytes",
            InPlacePages => "monitor.in_place_pages",
            Draws => "monitor.draws",
            DrawsIssued => "monitor.draws_issued",
            Dispatches => "monitor.dispatch",
            DispatchesIssued => "monitor.dispatch_issued",
            PipelineDrops => "monitor.pipeline_drops",
            IndirectDrops => "monitor.indirect_drops",
            PresentMs => "monitor.present_ms",
            Pipelines => "monitor.pipelines",
            PipelineWait => "monitor.pipeline_wait",
            TextureRoom => "monitor.texture_room",
            TextureTileRoom => "monitor.texture_tile_room",
            TextureUploads => "monitor.texture_uploads",
            WriteWatch => "monitor.write_watch",
            Frame => "monitor.frame_number",
            Missed => "monitor.missed",
            Occluded => "monitor.occluded_count",
            Rejected => "monitor.rejected_count",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Card {
    Overview,
    Chart,
    Activity,
    Stages,
    Memory,
    Commands,
    Preparation,
    Hardware,
    Footer,
}

impl Card {
    pub const ALL: &[Self] = &[
        Self::Overview,
        Self::Chart,
        Self::Activity,
        Self::Stages,
        Self::Memory,
        Self::Commands,
        Self::Preparation,
        Self::Hardware,
        Self::Footer,
    ];

    pub fn label(self) -> &'static str {
        use Card::*;
        crate::i18n::tr(match self {
            Overview => "monitor.overview",
            Chart => "monitor.chart",
            Activity => "monitor.activity",
            Stages => "monitor.stages",
            Memory => "monitor.memory",
            Commands => "monitor.commands",
            Preparation => "monitor.prep",
            Hardware => "monitor.hardware",
            Footer => "monitor.footer",
        })
    }

    pub fn fields(self) -> &'static [Field] {
        use Field::*;
        match self {
            Self::Overview => &[Fps, FrameMs, GpuEstimate, GpuIdleCpu, LowFps],
            Self::Memory => &[
                Vram,
                Ram,
                SystemRam,
                CopyTo,
                CopyFrom,
                InPlaceBytes,
                InPlacePages,
            ],
            Self::Commands => &[
                DrawsIssued,
                Draws,
                DispatchesIssued,
                Dispatches,
                PipelineDrops,
                IndirectDrops,
                PresentMs,
            ],
            Self::Preparation => &[
                Pipelines,
                PipelineWait,
                TextureRoom,
                TextureTileRoom,
                TextureUploads,
                WriteWatch,
            ],
            Self::Hardware => &[GpuLoad, GpuTemp, CpuLoad],
            Self::Footer => &[Frame, Missed, Occluded, Rejected],
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardChoice {
    pub card: Card,
    pub visible: bool,
    pub rows: Vec<RowChoice>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RowChoice {
    pub field: Field,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Minimal,
    Standard,
    Everything,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rate {
    Quarter,
    Half,
    #[default]
    Second,
    TwoSeconds,
}

impl Rate {
    pub const ALL: &[Self] = &[Self::Quarter, Self::Half, Self::Second, Self::TwoSeconds];

    pub fn duration(self) -> std::time::Duration {
        std::time::Duration::from_millis(match self {
            Self::Quarter => 250,
            Self::Half => 500,
            Self::Second => 1_000,
            Self::TwoSeconds => 2_000,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quarter => "0.25 s",
            Self::Half => "0.5 s",
            Self::Second => "1 s",
            Self::TwoSeconds => "2 s",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorSettings {
    pub cards: Vec<CardChoice>,
    pub chart_seconds: u32,
    pub fixed_scale_ms: Option<u32>,
    pub target_ms: f32,
    pub rate: Rate,
    pub csv_seconds: u32,
    pub live_sensors: bool,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            cards: Self::preset(Preset::Standard),
            chart_seconds: 30,
            fixed_scale_ms: None,
            target_ms: 1000.0 / 60.0,
            rate: Rate::Second,
            csv_seconds: 60,
            live_sensors: true,
        }
    }
}

impl MonitorSettings {
    pub fn preset(preset: Preset) -> Vec<CardChoice> {
        Card::ALL
            .iter()
            .map(|&card| {
                let visible = match preset {
                    Preset::Minimal => matches!(card, Card::Overview | Card::Chart),
                    Preset::Standard => true,
                    Preset::Everything => true,
                };
                let rows = card
                    .fields()
                    .iter()
                    .map(|&field| RowChoice {
                        field,
                        visible: match preset {
                            Preset::Everything => true,
                            Preset::Standard => !matches!(
                                field,
                                Field::InPlacePages | Field::TextureTileRoom | Field::Rejected
                            ),
                            Preset::Minimal => matches!(
                                field,
                                Field::Fps
                                    | Field::FrameMs
                                    | Field::GpuEstimate
                                    | Field::GpuIdleCpu
                            ),
                        },
                    })
                    .collect();
                CardChoice {
                    card,
                    visible,
                    rows,
                }
            })
            .collect()
    }
}

pub fn engine_options(
    cfg: &OpsSettings,
    data: &std::path::Path,
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    Ok(with_panel(cfg, super::profile::prepare_launch(&cfg.profiler, data)?))
}

/// Predicts the same flags without creating a profiler directory or using a real run id.
pub fn preview_engine_options(
    cfg: &OpsSettings,
    data: &std::path::Path,
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    Ok(with_panel(cfg, super::profile::launch_flags(&cfg.profiler, data, 0)?))
}

fn with_panel(
    cfg: &OpsSettings,
    mut flags: std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    if let Some(panel) = panel_flag(cfg) {
        flags.insert("VICTORY_PANEL".into(), panel.into());
    }
    flags
}

/// `Some("1")` when the launcher setting turns the in-game panel on. When it is off the launch profile decides.
pub fn panel_flag(cfg: &OpsSettings) -> Option<&'static str> {
    cfg.panel.then_some("1")
}
