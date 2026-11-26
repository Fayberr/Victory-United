//! The title's graphics state (as a pipeline description names it) to D3D12
//! values. Anything without a D3D12 form here is an error naming it.

use anyhow::{Result, bail};

use crate::manifest::{BlendState, Graphics, InputElement, Pipeline};

/// DXGI formats by name (`DXGI_FORMAT_` left out) and number.
const FORMATS: &[(&str, i32)] = &[
    ("UNKNOWN", 0),
    ("R32G32B32A32_FLOAT", 2),
    ("R32G32B32A32_UINT", 3),
    ("R32G32B32A32_SINT", 4),
    ("R32G32B32_FLOAT", 6),
    ("R32G32B32_UINT", 7),
    ("R16G16B16A16_FLOAT", 10),
    ("R16G16B16A16_UNORM", 11),
    ("R16G16B16A16_UINT", 12),
    ("R16G16B16A16_SNORM", 13),
    ("R32G32_FLOAT", 16),
    ("R32G32_UINT", 17),
    ("D32_FLOAT_S8X24_UINT", 20),
    ("R10G10B10A2_UNORM", 24),
    ("R11G11B10_FLOAT", 26),
    ("R8G8B8A8_UNORM", 28),
    ("R8G8B8A8_UNORM_SRGB", 29),
    ("R8G8B8A8_UINT", 30),
    ("R8G8B8A8_SNORM", 31),
    ("R16G16_FLOAT", 34),
    ("R16G16_UNORM", 35),
    ("R16G16_UINT", 36),
    ("R16G16_SNORM", 37),
    ("D32_FLOAT", 40),
    ("R32_FLOAT", 41),
    ("R32_UINT", 42),
    ("R32_SINT", 43),
    ("D24_UNORM_S8_UINT", 45),
    ("R8G8_UNORM", 49),
    ("R16_FLOAT", 54),
    ("D16_UNORM", 55),
    ("R16_UNORM", 56),
    ("R16_UINT", 57),
    ("R8_UNORM", 61),
    ("R8_UINT", 62),
    ("B8G8R8A8_UNORM", 87),
    ("B8G8R8A8_UNORM_SRGB", 91),
];

/// `D3D12_COMPARISON_FUNC` by name.
const COMPARISONS: &[(&str, i32)] = &[
    ("never", 1),
    ("less", 2),
    ("equal", 3),
    ("less_equal", 4),
    ("greater", 5),
    ("not_equal", 6),
    ("greater_equal", 7),
    ("always", 8),
];

/// `D3D12_BLEND` by name.
const BLENDS: &[(&str, i32)] = &[
    ("zero", 1),
    ("one", 2),
    ("src_color", 3),
    ("inv_src_color", 4),
    ("src_alpha", 5),
    ("inv_src_alpha", 6),
    ("dest_alpha", 7),
    ("inv_dest_alpha", 8),
    ("dest_color", 9),
    ("inv_dest_color", 10),
];

/// `D3D12_BLEND_OP` by name.
const BLEND_OPS: &[(&str, i32)] = &[
    ("add", 1),
    ("subtract", 2),
    ("rev_subtract", 3),
    ("min", 4),
    ("max", 5),
];

/// The number `name` has in `table`, or an error listing what is mapped.
fn named(what: &str, table: &[(&str, i32)], name: &str) -> Result<i32> {
    match table.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        Some(&(_, value)) => Ok(value),
        None => {
            let known: Vec<&str> = table.iter().map(|t| t.0).collect();
            bail!("unsupported {what} `{name}` (mapped: {})", known.join(", "))
        }
    }
}

fn format(name: &str) -> Result<i32> {
    named(
        "format",
        FORMATS,
        name.strip_prefix("DXGI_FORMAT_").unwrap_or(name),
    )
}

/// The description's graphics state as D3D12 values.
pub fn resolve(p: &Pipeline) -> Result<Graphics> {
    let topology = match p.topology.as_deref() {
        Some("point") => 1,
        Some("line") => 2,
        Some("triangle") => 3,
        Some("patch") => 4,
        // The console's own types: the runtime turns them into triangles as
        // it draws; a pipeline state has no form for them.
        Some(t @ ("rectangle" | "quad")) => bail!(
            "unsupported topology `{t}`: a console-only primitive type, which the runtime \
             expands when it draws; this builder maps point, line, triangle and patch"
        ),
        Some(t) => bail!("unsupported topology `{t}` (mapped: point, line, triangle, patch)"),
        None => bail!("a graphics pipeline needs `topology`"),
    };
    if p.render_targets.len() > 8 {
        bail!(
            "unsupported: {} render targets (D3D12 has 8)",
            p.render_targets.len()
        );
    }
    if p.blend.len() > p.render_targets.len().max(1) {
        bail!(
            "{} blend states for {} render targets",
            p.blend.len(),
            p.render_targets.len()
        );
    }
    let render_targets = p
        .render_targets
        .iter()
        .map(|f| format(f))
        .collect::<Result<Vec<_>>>()?;
    let depth_stencil = match &p.depth_stencil {
        Some(f) => format(f)?,
        None => 0,
    };
    let sample_count = p.sample_count.unwrap_or(1);
    if !matches!(sample_count, 1 | 2 | 4 | 8) {
        bail!("unsupported sample count {sample_count} (mapped: 1, 2, 4, 8)");
    }
    let r = p.rasterizer.as_ref();
    let fill = match r.and_then(|r| r.fill.as_deref()) {
        None | Some("solid") => 3,
        Some("wireframe") => 2,
        Some(f) => bail!("unsupported fill mode `{f}` (mapped: solid, wireframe)"),
    };
    let cull = match r.and_then(|r| r.cull.as_deref()) {
        None | Some("back") => 3,
        Some("front") => 2,
        Some("none") => 1,
        Some(c) => bail!("unsupported cull mode `{c}` (mapped: none, front, back)"),
    };
    let d = p.depth.as_ref();
    let depth_enable = d.is_some_and(|d| d.enable);
    if depth_enable && depth_stencil == 0 {
        bail!("the depth test is enabled without a `depth_stencil` format");
    }
    let depth_func = match d.and_then(|d| d.func.as_deref()) {
        Some(f) => named("comparison", COMPARISONS, f)?,
        None => 2,
    };
    let blend = p
        .blend
        .iter()
        .map(|b| {
            let factor = |f: &Option<String>, default| match f {
                Some(f) => named("blend factor", BLENDS, f),
                None => Ok(default),
            };
            let op = |o: &Option<String>| match o {
                Some(o) => named("blend operation", BLEND_OPS, o),
                None => Ok(1),
            };
            Ok(BlendState {
                enable: b.enable.unwrap_or(false),
                src: factor(&b.src, 2)?,
                dst: factor(&b.dst, 1)?,
                op: op(&b.op)?,
                src_alpha: factor(&b.src_alpha, 2)?,
                dst_alpha: factor(&b.dst_alpha, 1)?,
                op_alpha: op(&b.op_alpha)?,
                write_mask: b.write_mask.unwrap_or(0xf),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let input = p
        .input
        .iter()
        .map(|e| {
            let slot = e.slot.unwrap_or(0);
            if slot >= 32 {
                bail!(
                    "unsupported input slot {slot} of `{}` (D3D12 has 32)",
                    e.semantic
                );
            }
            if e.semantic.is_empty() || e.semantic.contains('\0') {
                bail!("an input element without a usable semantic name");
            }
            Ok(InputElement {
                semantic: e.semantic.clone(),
                index: e.index.unwrap_or(0),
                format: format(&e.format)?,
                slot,
                offset: e.offset,
                per_instance: e.per_instance.unwrap_or(false),
                step: e.step.unwrap_or(0),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Graphics {
        topology,
        render_targets,
        depth_stencil,
        sample_count,
        fill,
        cull,
        front_counter_clockwise: r.and_then(|r| r.front_counter_clockwise).unwrap_or(false),
        depth_clip: r.and_then(|r| r.depth_clip).unwrap_or(true),
        depth_bias: r.and_then(|r| r.depth_bias).unwrap_or(0),
        depth_enable,
        depth_write: d.and_then(|d| d.write).unwrap_or(depth_enable),
        depth_func,
        blend,
        input,
    })
}

/// Graphics keys set on a pipeline that is not one.
pub fn stray_graphics_keys(p: &Pipeline) -> Vec<&'static str> {
    let mut keys = Vec::new();
    let mut note = |set: bool, key| {
        if set {
            keys.push(key);
        }
    };
    note(p.vertex.is_some(), "vertex");
    note(p.pixel.is_some(), "pixel");
    note(p.topology.is_some(), "topology");
    note(!p.render_targets.is_empty(), "render_targets");
    note(p.depth_stencil.is_some(), "depth_stencil");
    note(p.sample_count.is_some(), "sample_count");
    note(p.rasterizer.is_some(), "rasterizer");
    note(p.depth.is_some(), "depth");
    note(!p.blend.is_empty(), "blend");
    note(!p.input.is_empty(), "input");
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graphics(topology: &str) -> Pipeline {
        Pipeline {
            kind: "graphics".into(),
            topology: Some(topology.into()),
            render_targets: vec!["R8G8B8A8_UNORM".into()],
            ..Default::default()
        }
    }

    #[test]
    fn state_maps_to_d3d12_numbers_with_its_defaults() {
        let g = resolve(&graphics("triangle")).unwrap();
        assert_eq!((g.topology, g.render_targets.as_slice()), (3, &[28][..]));
        // D3D12's defaults: solid, back-face culling, depth clip, no depth test.
        assert_eq!(
            (g.fill, g.cull, g.depth_clip, g.depth_enable),
            (3, 3, true, false)
        );
    }

    #[test]
    fn what_has_no_d3d12_form_is_named() {
        let e = resolve(&graphics("rectangle")).unwrap_err().to_string();
        assert!(e.contains("console-only"), "{e}");
        let mut p = graphics("triangle");
        p.render_targets = vec!["R9G9B9E5_SHAREDEXP".into()];
        let e = resolve(&p).unwrap_err().to_string();
        assert!(e.contains("unsupported format `R9G9B9E5_SHAREDEXP`"), "{e}");
    }
}
