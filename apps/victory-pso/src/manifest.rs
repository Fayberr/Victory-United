//! The two files of a pipeline: its description (what `build` reads) and its
//! manifest (what `build` writes beside the DXIL and `verify` reads).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The manifest's file name in a build folder.
pub const MANIFEST: &str = "pipeline.toml";

/// A pipeline description: the shader containers and the title's state.
/// Unknown keys are refused, so state this builder does not map is reported
/// rather than dropped.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Description {
    pub pipeline: Pipeline,
    pub run: Option<Run>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    /// `compute` or `graphics`.
    pub kind: String,
    /// Shader containers, relative to the description.
    pub compute: Option<PathBuf>,
    pub vertex: Option<PathBuf>,
    pub pixel: Option<PathBuf>,
    /// Graphics: `point`, `line`, `triangle` or `patch`.
    pub topology: Option<String>,
    /// Graphics: DXGI format names, one per render target.
    #[serde(default)]
    pub render_targets: Vec<String>,
    pub depth_stencil: Option<String>,
    pub sample_count: Option<u32>,
    pub rasterizer: Option<Rasterizer>,
    pub depth: Option<Depth>,
    /// One per render target; fewer leave the rest at D3D12's defaults.
    #[serde(default)]
    pub blend: Vec<Blend>,
    #[serde(default)]
    pub input: Vec<Input>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rasterizer {
    /// `solid` or `wireframe`.
    pub fill: Option<String>,
    /// `none`, `front` or `back`.
    pub cull: Option<String>,
    pub front_counter_clockwise: Option<bool>,
    pub depth_clip: Option<bool>,
    pub depth_bias: Option<i32>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Depth {
    pub enable: bool,
    pub write: Option<bool>,
    /// A comparison: `never`, `less`, `equal`, `less_equal`, `greater`,
    /// `not_equal`, `greater_equal`, `always`.
    pub func: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Blend {
    pub enable: Option<bool>,
    pub src: Option<String>,
    pub dst: Option<String>,
    pub op: Option<String>,
    pub src_alpha: Option<String>,
    pub dst_alpha: Option<String>,
    pub op_alpha: Option<String>,
    pub write_mask: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub semantic: String,
    pub index: Option<u32>,
    /// A DXGI format name.
    pub format: String,
    pub slot: Option<u32>,
    /// Byte offset in the slot; left out, the element follows the last.
    pub offset: Option<u32>,
    pub per_instance: Option<bool>,
    pub step: Option<u32>,
}

/// What `verify --run` binds, dispatches and expects (compute pipelines).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub groups: [u32; 3],
    /// Root constants: the title's root parameter index to its dwords.
    #[serde(default)]
    pub constants: BTreeMap<String, Vec<u32>>,
    /// Descriptor tables: the title's root parameter index to the view-heap
    /// index of the table's first descriptor.
    #[serde(default)]
    pub tables: BTreeMap<String, u32>,
    /// The view heap, in index order.
    #[serde(default)]
    pub view: Vec<View>,
    /// What views hold afterwards.
    #[serde(default)]
    pub expect: Vec<Expect>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// `raw-uav`: a raw buffer the shader may write, zero before the run.
    pub kind: String,
    pub dwords: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    pub view: usize,
    pub dwords: Vec<u32>,
    /// The dwords are floats, and may be off by this much (of the expected
    /// value where that is above 1); without it, exactly these bits.
    pub near: Option<f32>,
}

/// What `build` found: the translated stages, the host root signature their
/// bindings define, and the state as D3D12 values.
#[derive(Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub kind: String,
    /// The host 64-lane compute programs were translated for: `native`, `wave32`.
    pub wave64: String,
    pub stages: Vec<Stage>,
    pub root_flags: Vec<String>,
    pub root: Vec<RootParameter>,
    pub graphics: Option<Graphics>,
    pub run: Option<Run>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Stage {
    /// `compute`, `vertex` or `pixel`.
    pub stage: String,
    pub source: String,
    /// The DXIL container, beside the manifest.
    pub dxil: String,
    pub dxil_bytes: usize,
    /// What `dxil.dll` said, or that it was not found.
    pub validation: String,
    /// Size of the stage's user-data block at `b0` (root constants).
    pub user_data_dwords: u32,
    pub user_data: Vec<UserDataEntry>,
    /// Title root parameters read as root CBVs (`b<n>`), SRVs (`t<n>`) and
    /// UAVs (`u<n>`).
    pub root_cbvs: Vec<u8>,
    pub root_srvs: Vec<u8>,
    #[serde(default)]
    pub root_uavs: Vec<u8>,
    /// Immediate samplers (`s<n>`): the console's four sampler words.
    pub samplers: Vec<[u32; 4]>,
    /// Which of them the shader compares with.
    pub comparison_samplers: Vec<bool>,
}

/// One dword of a stage's user data and the title root parameter filling it.
#[derive(Debug, Deserialize, Serialize)]
pub struct UserDataEntry {
    pub index: u32,
    /// The title's root parameter index.
    pub slot: u8,
    /// `constants`, `table`, `resource-heap`, `sampler-heap`...
    pub root: String,
    /// Dword within the parameter.
    pub dword: u32,
}

/// One parameter of the host root signature.
#[derive(Debug, Deserialize, Serialize)]
pub struct RootParameter {
    pub index: u32,
    /// `constants`, `cbv` or `srv`.
    pub kind: String,
    pub register: String,
    pub space: u32,
    /// Root constants: dwords.
    pub dwords: u32,
    pub visibility: String,
}

/// Graphics state as D3D12 values (enumerations by their numbers).
#[derive(Debug, Deserialize, Serialize)]
pub struct Graphics {
    pub topology: i32,
    pub render_targets: Vec<i32>,
    pub depth_stencil: i32,
    pub sample_count: u32,
    pub fill: i32,
    pub cull: i32,
    pub front_counter_clockwise: bool,
    pub depth_clip: bool,
    pub depth_bias: i32,
    pub depth_enable: bool,
    pub depth_write: bool,
    pub depth_func: i32,
    pub blend: Vec<BlendState>,
    pub input: Vec<InputElement>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BlendState {
    pub enable: bool,
    pub src: i32,
    pub dst: i32,
    pub op: i32,
    pub src_alpha: i32,
    pub dst_alpha: i32,
    pub op_alpha: i32,
    pub write_mask: u8,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InputElement {
    pub semantic: String,
    pub index: u32,
    pub format: i32,
    pub slot: u32,
    /// `None`: appended after the slot's last element.
    pub offset: Option<u32>,
    pub per_instance: bool,
    pub step: u32,
}
