//! The ops half's own small settings file (`ops.toml` next to `settings.toml`): overlay, update checks.
use crate::paths;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];
    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Corner::TopLeft => tr("ops.corner.tl"),
            Corner::TopRight => tr("ops.corner.tr"),
            Corner::BottomLeft => tr("ops.corner.bl"),
            Corner::BottomRight => tr("ops.corner.br"),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OpsSettings {
    /// The small always-on-top fps overlay.
    pub overlay: bool,
    pub overlay_corner: Corner,
    pub monitor: MonitorSettings,
    pub overlay_options: OverlaySettings,
    pub profiler: super::profile::Schedule,
    pub profile_view: super::profile::View,
    /// `stable` or `dev`.
    pub channel: String,
    /// Which engine build to download: `release` or `diagnostics`.
    pub engine_kind: String,
    /// A developer's own victory.exe (Developer tab). Empty: the installed build is used.
    pub own_engine: String,
}

impl Default for OpsSettings {
    fn default() -> Self {
        Self {
            overlay: false,
            overlay_corner: Corner::TopLeft,
            monitor: MonitorSettings::default(),
            overlay_options: OverlaySettings::default(),
            profiler: super::profile::Schedule::default(),
            profile_view: super::profile::View::default(),
            channel: "stable".into(),
            engine_kind: "release".into(),
            own_engine: String::new(),
        }
    }
}

impl OpsSettings {
    pub fn file() -> std::path::PathBuf {
        paths::data_dir().join("ops.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::file())
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(paths::data_dir())?;
        crate::core::store::write_atomic(&Self::file(), toml::to_string_pretty(self)?.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_defaults() {
        let s = OpsSettings {
            overlay: true,
            overlay_corner: Corner::BottomRight,
            ..Default::default()
        };
        let t = toml::to_string_pretty(&s).unwrap();
        assert_eq!(toml::from_str::<OpsSettings>(&t).unwrap(), s);
        // Missing keys fall back to defaults; unknown keys do not break loading.
        assert_eq!(
            toml::from_str::<OpsSettings>("overlay = true\nfuture = 1")
                .unwrap()
                .channel,
            "stable"
        );
    }

    #[test]
    fn desktop_and_engine_use_exclusive_overlay_flags() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = OpsSettings::default();
        assert!(engine_options(&config, temp.path()).unwrap().is_empty());
        config.overlay = true;
        assert_eq!(
            engine_options(&config, temp.path()).unwrap()["VICTORY_PANEL"],
            "0"
        );
        config.overlay_options.mode = OverlayMode::Engine;
        assert_eq!(
            engine_options(&config, temp.path()).unwrap()["VICTORY_PANEL"],
            "1"
        );
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
    pub const OVERLAY: &[Self] = &[
        Self::Fps,
        Self::LowFps,
        Self::FrameMs,
        Self::CpuMs,
        Self::GpuMs,
        Self::GpuLoad,
        Self::GpuTemp,
        Self::Vram,
        Self::Ram,
        Self::Draws,
        Self::Pipelines,
    ];

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMode {
    #[default]
    Window,
    Engine,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayLayout {
    #[default]
    Line,
    Vertical,
    Fps,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextTone {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub mode: OverlayMode,
    pub fields: Vec<Field>,
    pub layout: OverlayLayout,
    pub margin: u32,
    pub scale_percent: u32,
    pub opacity_percent: u32,
    pub tone: TextTone,
    pub rate: Rate,
    pub sparkline: bool,
    pub only_game: bool,
    pub hide_focused: bool,
    pub hotkey: String,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            mode: OverlayMode::Window,
            fields: vec![Field::Fps, Field::FrameMs, Field::Vram],
            layout: OverlayLayout::Line,
            margin: 16,
            scale_percent: 100,
            opacity_percent: 82,
            tone: TextTone::Light,
            rate: Rate::Second,
            sparkline: false,
            only_game: true,
            hide_focused: false,
            hotkey: String::new(),
        }
    }
}

pub fn engine_options(
    cfg: &OpsSettings,
    data: &std::path::Path,
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let mut flags = super::profile::prepare_launch(&cfg.profiler, data)?;
    if cfg.overlay {
        let engine_panel = cfg.overlay_options.mode == OverlayMode::Engine;
        flags.insert("VICTORY_PANEL".into(), u8::from(engine_panel).to_string());
    }
    Ok(flags)
}
