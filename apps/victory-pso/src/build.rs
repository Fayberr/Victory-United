//! `build`: a pipeline description's shaders and state, translated offline.
//! Writes each stage's DXIL and a manifest of the bindings beside it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use victory_shader::emit::{
    HostShader, ROOT_CBV_SPACE, USER_DATA_SPACE, set_host_max_wave, translate,
};
use victory_shader::{HwStage, RootType};

use crate::manifest::{
    Description, Graphics, MANIFEST, Manifest, RootParameter, Stage, UserDataEntry,
};
use crate::state;

/// The host a 64-lane compute program is translated for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave64Mode {
    /// A host with 64-lane waves: the program keeps its waves.
    Native,
    /// A host whose widest wave has 32 lanes (NVIDIA): the translator's
    /// forms for two host waves per console wave.
    Wave32,
}

pub fn wave_name(mode: Wave64Mode) -> &'static str {
    match mode {
        Wave64Mode::Native => "native",
        Wave64Mode::Wave32 => "wave32",
    }
}

/// The host's widest wave, as the translator is told it.
fn host_max_wave(mode: Wave64Mode) -> u32 {
    match mode {
        Wave64Mode::Native => 64,
        Wave64Mode::Wave32 => 32,
    }
}

fn stage_name(stage: HwStage) -> &'static str {
    match stage {
        HwStage::Cs => "compute",
        HwStage::Vs => "vertex",
        HwStage::Ps => "pixel",
        HwStage::Ms => "mesh",
        _ => "tessellation or geometry",
    }
}

pub fn root_name(root: RootType) -> String {
    match root {
        RootType::Constants => "constants".into(),
        RootType::Descriptor => "descriptor".into(),
        RootType::Table => "table".into(),
        RootType::ImmediateSampler => "immediate-sampler".into(),
        RootType::ResourceHeap => "resource-heap".into(),
        RootType::SamplerHeap => "sampler-heap".into(),
        RootType::Other(n) => format!("other-{n}"),
    }
}

/// Checks translated DXIL with `dxil.dll`, where there is one.
#[cfg(windows)]
fn validate(dxil: &[u8]) -> Result<String, String> {
    use victory_shader::emit::validator::Validator;
    match Validator::load(Validator::default_path().as_deref()) {
        Ok(mut v) => v
            .validate(&mut dxil.to_vec())
            .map(|()| "dxil.dll: valid".to_owned()),
        Err(_) => Ok("dxil.dll not found: not validated".to_owned()),
    }
}

#[cfg(not(windows))]
fn validate(_: &[u8]) -> Result<String, String> {
    Ok("no dxil.dll on this system: not validated".to_owned())
}

pub fn run(description: &Path, out: &Path, wave64: Wave64Mode) -> Result<()> {
    victory_core::ensure_outside_repo(out)?;
    let text = std::fs::read_to_string(description)
        .with_context(|| format!("reading {}", description.display()))?;
    let desc: Description = toml::from_str(&text)
        .map_err(|e| anyhow!("{}: not a pipeline description: {e}", description.display()))?;
    let p = &desc.pipeline;
    // The stages the kind is made of, and the state it maps.
    let (wanted, graphics): (Vec<(&PathBuf, HwStage)>, Option<Graphics>) = match p.kind.as_str() {
        "compute" => {
            let stray = state::stray_graphics_keys(p);
            if !stray.is_empty() {
                bail!("a compute pipeline has no `{}`", stray.join("`, `"));
            }
            let cs = p
                .compute
                .as_ref()
                .context("a compute pipeline needs `compute = <container>`")?;
            (vec![(cs, HwStage::Cs)], None)
        }
        "graphics" => {
            if p.compute.is_some() {
                bail!("a graphics pipeline has no `compute`");
            }
            let vs = p
                .vertex
                .as_ref()
                .context("a graphics pipeline needs `vertex = <container>`")?;
            let ps = p
                .pixel
                .as_ref()
                .context("a graphics pipeline needs `pixel = <container>`")?;
            (
                vec![(vs, HwStage::Vs), (ps, HwStage::Ps)],
                Some(state::resolve(p)?),
            )
        }
        "mesh" => bail!(
            "unsupported pipeline kind `mesh`: this builder builds compute and vertex + pixel \
             pipelines"
        ),
        other => bail!("unsupported pipeline kind `{other}` (built: compute, graphics)"),
    };
    if let Some(run) = &desc.run {
        if graphics.is_some() {
            bail!("unsupported: `[run]` on a graphics pipeline (only compute pipelines are run)");
        }
        if let Some(v) = run.view.iter().find(|v| v.kind != "raw-uav") {
            bail!("unsupported view kind `{}` (bound: raw-uav)", v.kind);
        }
    }

    let dir = description.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let mut shaders: Vec<HostShader> = Vec::new();
    let mut stages = Vec::new();
    let mut root = Vec::new();
    // (the translator's setting is the process's: this one build is all it does)
    set_host_max_wave(host_max_wave(wave64));
    for &(path, stage) in &wanted {
        let source = dir.join(path);
        let bytes =
            std::fs::read(&source).with_context(|| format!("reading {}", source.display()))?;
        let shader = translate(&bytes).map_err(|e| anyhow!("{}: {e}", source.display()))?;
        if shader.stage != stage {
            bail!(
                "{}: holds a {} program, not the pipeline's {} shader",
                source.display(),
                stage_name(shader.stage),
                stage_name(stage)
            );
        }
        let validation = validate(&shader.dxil).map_err(|e| {
            anyhow!(
                "{}: the translated DXIL fails validation: {e}",
                source.display()
            )
        })?;
        let name = stage_name(stage);
        let file = format!("{name}.dxil");
        std::fs::write(out.join(&file), &shader.dxil).with_context(|| format!("writing {file}"))?;
        // User data of stage k is root parameter k.
        root.push(RootParameter {
            index: root.len() as u32,
            kind: "constants".into(),
            register: "b0".into(),
            space: USER_DATA_SPACE,
            dwords: shader.user_data_dwords().max(1),
            visibility: if stage == HwStage::Cs { "all" } else { name }.into(),
        });
        println!(
            "{name}: {} -> {file} ({} bytes; {validation})",
            source.display(),
            shader.dxil.len()
        );
        for u in &shader.user_data {
            println!(
                "  user data [{}] <- title root parameter {} ({}), dword {}",
                u.index,
                u.slot,
                root_name(u.root),
                u.dword
            );
        }
        stages.push(Stage {
            stage: name.into(),
            source: path.display().to_string(),
            dxil: file,
            dxil_bytes: shader.dxil.len(),
            validation,
            user_data_dwords: shader.user_data_dwords(),
            user_data: shader
                .user_data
                .iter()
                .map(|u| UserDataEntry {
                    index: u.index,
                    slot: u.slot,
                    root: root_name(u.root),
                    dword: u.dword,
                })
                .collect(),
            root_cbvs: shader.root_cbvs.clone(),
            root_srvs: shader.root_srvs.clone(),
            root_uavs: shader.root_uavs.clone(),
            samplers: shader.samplers.clone(),
            comparison_samplers: shader
                .static_samplers
                .iter()
                .map(|s| s.filter & 0x80 != 0)
                .collect(),
        });
        shaders.push(shader);
    }
    // Root views follow the user data, as the host root signature has them:
    // by slot, then CBV, SRV, UAV.
    let mut views: Vec<(u8, u8)> = shaders
        .iter()
        .flat_map(|s| {
            let cbvs = s.root_cbvs.iter().map(|&r| (r, 0));
            let srvs = s.root_srvs.iter().map(|&r| (r, 1));
            cbvs.chain(srvs).chain(s.root_uavs.iter().map(|&r| (r, 2)))
        })
        .collect();
    views.sort_unstable();
    views.dedup();
    for (slot, kind) in views {
        let (name, register) = [("cbv", 'b'), ("srv", 't'), ("uav", 'u')][usize::from(kind)];
        root.push(RootParameter {
            index: root.len() as u32,
            kind: name.into(),
            register: format!("{register}{slot}"),
            space: ROOT_CBV_SPACE,
            dwords: 0,
            visibility: "all".into(),
        });
    }
    for r in &root {
        println!(
            "root parameter {}: {} {}, space {} ({} dwords, visible to {})",
            r.index, r.kind, r.register, r.space, r.dwords, r.visibility
        );
    }
    let manifest = Manifest {
        kind: p.kind.clone(),
        wave64: wave_name(wave64).into(),
        stages,
        root_flags: vec![
            "ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT".into(),
            "CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED".into(),
            "SAMPLER_HEAP_DIRECTLY_INDEXED".into(),
        ],
        root,
        graphics,
        run: desc.run,
    };
    let path = out.join(MANIFEST);
    let text = toml::to_string_pretty(&manifest).context("writing the manifest")?;
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}
