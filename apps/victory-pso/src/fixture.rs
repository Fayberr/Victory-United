//! Synthetic fixtures: small GFX10 programs of our own, wrapped the way the
//! console's shader compiler wraps its output (a DXBC container whose `DXIL`
//! part carries an `XDAT` table and the zlib-packed `FSSM` program record),
//! which is what the translator takes. Nothing here comes from a title.

use std::path::Path;

use anyhow::{Context, Result};
use victory_shader::FourCC;

const S_ENDPGM: u32 = 0xBF81_0000;
const VCC: u32 = 106;
const NULL: u32 = 125;
const EXEC: u32 = 126;
const LITERAL: u32 = 0xFF;

/// An inline integer operand, 0..=64.
fn inline(n: u32) -> u32 {
    0x80 + n
}

fn sop1(dst: u32, op: u32, src0: u32) -> u32 {
    0xBE80_0000 | dst << 16 | op << 8 | src0
}
fn sop2(dst: u32, op: u32, src0: u32, src1: u32) -> u32 {
    0x8000_0000 | op << 23 | dst << 16 | src1 << 8 | src0
}
fn sopp(op: u32, imm: u16) -> u32 {
    0xBF80_0000 | op << 16 | u32::from(imm)
}
fn vop1(dst: u32, op: u32, src0: u32) -> u32 {
    0x7E00_0000 | dst << 17 | op << 9 | src0
}
fn vop2(op: u32, dst: u32, src0: u32, vsrc1: u32) -> u32 {
    op << 25 | dst << 17 | vsrc1 << 9 | src0
}
fn vopc(op: u32, src0: u32, vsrc1: u32) -> u32 {
    0x7C00_0000 | op << 17 | vsrc1 << 9 | src0
}

/// Stores `v<data>` at dword `v0` (the thread id) of the raw buffer whose
/// descriptor is number `descriptor` of the view heap `s0` points to:
/// `s_mov_b32 s1, 0` (the heap address's high half), `s_load_dwordx4 s[8:11],
/// s[0:1], 32 * descriptor`, `v_lshlrev_b32 v15, 2, v0`, `buffer_store_dword`.
fn store_to_heap_buffer(data: u32, descriptor: u32) -> [u32; 6] {
    [
        sop1(1, 0x03, inline(0)),
        0xF408_0000 | 8 << 6,
        NULL << 25 | (32 * descriptor),
        vop2(0x1A, 15, inline(2), 0),
        0xE070_1000,
        15 | data << 8 | (8 >> 2) << 16 | inline(0) << 24,
    ]
}

/// A root parameter and the user SGPRs it arrives in.
pub(crate) struct Param {
    /// 0 root constants, 1 a root descriptor, 3 a descriptor table, 5 the
    /// view heap's address, 6 the sampler heap's.
    pub root: u32,
    pub sgprs: u32,
    pub user_data: u8,
    pub slot: u8,
}

/// Where each stage's program record offset sits in the `FSSM` payload.
const VERTEX: usize = 0x28;
const PIXEL: usize = 0x30;
pub(crate) const COMPUTE: usize = 0x60;

/// What a program's record says besides its code.
pub(crate) struct Record<'a> {
    pub stage: usize,
    pub params: &'a [Param],
    pub stage_data: &'a [u8],
    pub signatures: &'a [(FourCC, Vec<u8>)],
    pub vgprs: u32,
    /// 32 lanes per wave (else 64).
    pub wave32: bool,
}

/// A console shader container holding one native program.
fn container(
    stage: usize,
    code: &[u32],
    params: &[Param],
    stage_data: &[u8],
    signatures: &[(FourCC, Vec<u8>)],
) -> Vec<u8> {
    let code: Vec<u8> = code.iter().flat_map(|w| w.to_le_bytes()).collect();
    container_of(
        &code,
        &Record {
            stage,
            params,
            stage_data,
            signatures,
            vgprs: 32,
            wave32: false,
        },
    )
}

/// A console shader container holding the native program `code`.
pub(crate) fn container_of(code: &[u8], r: &Record) -> Vec<u8> {
    const ABSENT: u64 = 0xDEAD_BEEF_DEAD_BEEF;
    const HEADER: usize = 0x68;
    const RECORD: usize = 0x54;
    let (stage, params, stage_data, signatures) = (r.stage, r.params, r.stage_data, r.signatures);
    let mut payload = vec![0u8; HEADER];
    // Six graphics slots, then the mesh and compute ones; all absent but ours.
    payload[..8].copy_from_slice(&6u64.to_le_bytes());
    for slot in [0x08, 0x10, 0x18, 0x20, 0x28, 0x30, 0x48, 0x60] {
        payload[slot..slot + 8].copy_from_slice(&ABSENT.to_le_bytes());
    }
    payload[stage..stage + 8].copy_from_slice(&(HEADER as u64).to_le_bytes());
    let code_at = (HEADER + RECORD + stage_data.len()) as u64;
    let layout_at = code_at + code.len() as u64;
    let mut record = [0u8; RECORD];
    record[..8].copy_from_slice(&code_at.to_le_bytes());
    record[8..16].copy_from_slice(&(code.len() as u64).to_le_bytes());
    // The root parameter table, and an empty table of used resources.
    record[0x28..0x30].copy_from_slice(&layout_at.to_le_bytes());
    record[0x30..0x38].copy_from_slice(&layout_at.to_le_bytes());
    record[0x38..0x3C].copy_from_slice(&(params.len() as u32).to_le_bytes());
    record[0x14..0x18].copy_from_slice(&(u32::from(r.wave32) << 5).to_le_bytes());
    // The VGPR count, and 31 SGPRs.
    record[0x50..0x54].copy_from_slice(&(r.vgprs.min(256) | 31 << 20).to_le_bytes());
    payload.extend(record);
    payload.extend(stage_data);
    payload.extend(code);
    for p in params {
        let mut entry = [0u8; 20];
        entry[..4].copy_from_slice(&p.root.to_le_bytes());
        entry[8..12].copy_from_slice(&p.sgprs.to_le_bytes());
        entry[16] = p.user_data;
        entry[17] = p.slot;
        payload.extend(entry);
    }

    let mut fssm = b"FSSM".to_vec();
    fssm.extend(0x101u32.to_le_bytes());
    fssm.extend((payload.len() as u64).to_le_bytes());
    fssm.extend(payload);
    let mut room = vec![0u8; zlib_rs::compress_bound(fssm.len())];
    let (packed, done) = zlib_rs::compress_slice(&mut room, &fssm, zlib_rs::DeflateConfig::new(6));
    assert_eq!(done, zlib_rs::ReturnCode::Ok, "room for every input");
    let mut xbch = b"XBCH".to_vec();
    xbch.extend(3u32.to_le_bytes());
    xbch.extend((packed.len() as u64).to_le_bytes());
    xbch.extend((fssm.len() as u64).to_le_bytes());
    xbch.extend_from_slice(packed);

    // The `DXIL` part: the program header's room, then `XDAT` naming one
    // native blob (kind 0x0d) by offset and size.
    let mut dxil = vec![0u8; 0x18];
    dxil.extend(b"XDAT");
    dxil.extend(1u32.to_le_bytes());
    dxil.extend(0x24u32.to_le_bytes());
    let blob_at = dxil.len() as u32 + 12;
    for v in [0x0D, blob_at, xbch.len() as u32] {
        dxil.extend(v.to_le_bytes());
    }
    dxil.extend(xbch);
    let mut parts: Vec<(FourCC, &[u8])> = vec![(FourCC::DXIL, &dxil)];
    parts.extend(signatures.iter().map(|(f, d)| (*f, d.as_slice())));
    dxbc(&parts)
}

/// A DXBC container: the header (no digest, version 1.0), the part offsets,
/// then each part as its four characters, size and data.
fn dxbc(parts: &[(FourCC, &[u8])]) -> Vec<u8> {
    const HEADER: usize = 0x20;
    let table = HEADER + 4 * parts.len();
    let total = table + parts.iter().map(|(_, d)| 8 + d.len()).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend(b"DXBC");
    out.extend([0; 16]);
    out.extend(1u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((parts.len() as u32).to_le_bytes());
    let mut offset = table;
    for (_, data) in parts {
        out.extend((offset as u32).to_le_bytes());
        offset += 8 + data.len();
    }
    for (fourcc, data) in parts {
        out.extend(fourcc.0);
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(*data);
    }
    out
}

/// A compute program's stage data: its user SGPR count and thread group.
fn compute_data(user_sgprs: u32, threads: u32) -> Vec<u8> {
    compute_stage(&Compute {
        user_sgprs,
        group_id: [false; 3],
        thread_id_components: 1,
        lds_bytes: 0,
        threads: [threads, 1, 1],
    })
}

/// How a compute program is started.
pub(crate) struct Compute {
    /// SGPRs the root parameters arrive in.
    pub user_sgprs: u32,
    /// Which components of the thread group's id arrive in the SGPRs after
    /// the user ones.
    pub group_id: [bool; 3],
    /// Components of the thread's id in the group, in `v0` on (1 to 3).
    pub thread_id_components: u32,
    pub lds_bytes: u32,
    pub threads: [u32; 3],
}

/// A compute program's stage data (`COMPUTE_PGM_RSRC2`, then the group size).
pub(crate) fn compute_stage(c: &Compute) -> Vec<u8> {
    let [x, y, z] = c.group_id.map(u32::from);
    let rsrc2 = (c.user_sgprs & 0x1F) << 1
        | x << 7
        | y << 8
        | z << 9
        | (c.thread_id_components.clamp(1, 3) - 1) << 11
        | c.lds_bytes.div_ceil(512) << 15;
    let mut data = vec![0u8; 0x34];
    data[0x24..0x28].copy_from_slice(&rsrc2.to_le_bytes());
    for (i, n) in c.threads.into_iter().enumerate() {
        data[0x28 + 4 * i..0x2C + 4 * i].copy_from_slice(&n.to_le_bytes());
    }
    data
}

/// A signature part with one four-component float row at register 0.
fn signature(name: &str, system_value: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(1u32.to_le_bytes());
    out.extend(8u32.to_le_bytes());
    // Stream, name offset, semantic index, system value, float, register.
    for v in [0, 8 + 32, 0, system_value, 3, 0] {
        out.extend(v.to_le_bytes());
    }
    out.extend([0xf, 0, 0, 0]);
    out.extend(0u32.to_le_bytes());
    out.extend(name.as_bytes());
    out.push(0);
    out
}

const HEAP: Param = Param {
    root: 5,
    sgprs: 1,
    user_data: 0,
    slot: 0,
};

/// `bindings`: 64 threads write `((thread + K0) << 8) | K1` to dword
/// `thread` of the buffer at view-heap index 2. K0 and K1 are the two dwords
/// of the title's root parameter 1 (root constants). A swapped constant, or
/// a wrong heap index, changes what lands where.
fn bindings() -> (Vec<u8>, String) {
    const K: [u32; 2] = [0x1234, 5];
    let mut code = vec![
        vop2(0x25, 1, 2, 0),         // v_add_nc_u32 v1, s2, v0
        vop2(0x1A, 1, inline(8), 1), // v_lshlrev_b32 v1, 8, v1
        vop2(0x1C, 1, 3, 1),         // v_or_b32 v1, s3, v1
    ];
    code.extend(store_to_heap_buffer(1, 2));
    code.push(S_ENDPGM);
    let constants = Param {
        root: 0,
        sgprs: 2,
        user_data: 2,
        slot: 1,
    };
    let bytes = container(
        COMPUTE,
        &code,
        &[HEAP, constants],
        &compute_data(4, 64),
        &[],
    );
    let expected: Vec<u32> = (0..64).map(|t| (t + K[0]) << 8 | K[1]).collect();
    let text = format!(
        "# 64 threads write ((thread + K0) << 8) | K1 to dword `thread` of the raw\n\
         # buffer at view-heap index 2; K0, K1 are root constants (title root\n\
         # parameter 1). The buffers at indices 0 and 1 must stay zero.\n\
         [pipeline]\n\
         kind = \"compute\"\n\
         compute = \"bindings.cs.bin\"\n\n\
         [run]\n\
         groups = [1, 1, 1]\n\n\
         [run.constants]\n\
         1 = {K:?}\n\n\
         [[run.view]]\nkind = \"raw-uav\"\ndwords = 64\n\n\
         [[run.view]]\nkind = \"raw-uav\"\ndwords = 64\n\n\
         [[run.view]]\nkind = \"raw-uav\"\ndwords = 64\n\n\
         [[run.expect]]\nview = 0\ndwords = {zeros:?}\n\n\
         [[run.expect]]\nview = 1\ndwords = {zeros:?}\n\n\
         [[run.expect]]\nview = 2\ndwords = {expected:?}\n",
        zeros = vec![0u32; 64],
    );
    (bytes, text)
}

/// `skipped-ballot`: 128 threads (two 64-lane console waves). The lanes
/// below 32 of each wave count the wave's running lanes whose thread id is
/// below 70, in code an `s_cbranch_execz` skips, and store the count; the
/// other lanes store 0. A 32-lane host has to keep the thread group together
/// through the skipped code to take that count.
fn skipped_ballot() -> (Vec<u8>, String) {
    let mut code = vec![
        vop2(0x1B, 2, inline(63), 0), // v_and_b32 v2, 63, v0: console lane
        vop1(1, 0x01, inline(0)),     // v_mov_b32 v1, 0
        sop1(4, 0x03, inline(0)),     // s_mov_b32 s4, 0
        vopc(0xC4, inline(32), 2),    // v_cmp_gt_u32 vcc, 32, v2: lane < 32
        sop1(6, 0x24, VCC),           // s_and_saveexec_b64 s[6:7], vcc
        sopp(0x08, 4),                // s_cbranch_execz +4
        vopc(0xC4, LITERAL, 0),       // v_cmp_gt_u32 vcc, 70, v0: thread < 70
        70,
        sop1(4, 0x10, VCC),        // s_bcnt1_i32_b64 s4, vcc
        vop1(1, 0x01, 4),          // v_mov_b32 v1, s4
        sop2(EXEC, 0x11, EXEC, 6), // s_or_b64 exec, exec, s[6:7]
    ];
    code.extend(store_to_heap_buffer(1, 0));
    code.push(S_ENDPGM);
    let bytes = container(COMPUTE, &code, &[HEAP], &compute_data(2, 128), &[]);
    let expected: Vec<u32> = (0..128u32)
        .map(|g| {
            let (first, lane) = (g & !63, g & 63);
            let counted = (0..32).filter(|l| first + l < 70).count() as u32;
            if lane < 32 { counted } else { 0 }
        })
        .collect();
    let text = format!(
        "# 128 threads, two 64-lane console waves: lanes below 32 store how many of\n\
         # their wave's running lanes have a thread id below 70 (32, then 6), the\n\
         # others 0. The count is taken in code an s_cbranch_execz skips.\n\
         [pipeline]\n\
         kind = \"compute\"\n\
         compute = \"skipped-ballot.cs.bin\"\n\n\
         [run]\n\
         groups = [1, 1, 1]\n\n\
         [[run.view]]\nkind = \"raw-uav\"\ndwords = 128\n\n\
         [[run.expect]]\nview = 0\ndwords = {expected:?}\n"
    );
    (bytes, text)
}

/// `triangle`: a vertex program exporting a constant position and a pixel
/// program exporting a constant colour, with state that exercises the input
/// layout, topology, target formats and fixed-function mapping.
fn triangle() -> (Vec<u8>, Vec<u8>, String) {
    let vs = [
        vop1(0, 0x01, inline(0)), // v_mov_b32 v0, 0
        vop1(1, 0x01, 0xF2),      // v_mov_b32 v1, 1.0
        0xF800_08CF,              // exp pos0 v0, v0, v0, v1 done
        0x0100_0000,
        S_ENDPGM,
    ];
    let ps = [
        vop1(0, 0x01, 0xF2), // v_mov_b32 v0, 1.0
        0xF800_180F,         // exp mrt0 v0, v0, v0, v0 done vm
        0x0000_0000,
        S_ENDPGM,
    ];
    // Vertex stage data: no fetched inputs, no parameter exports.
    let vs_data = vec![0u8; 0x18C];
    // Pixel stage data: no inputs, and no export patch for any target.
    const PATCHES: usize = 0x14C;
    const PATCH: usize = 4 + 10 * 10 * 4;
    let mut ps_data = vec![0u8; PATCHES + 8 * PATCH];
    for target in 0..8 {
        let at = PATCHES + target * PATCH;
        ps_data[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    }
    let position = [(FourCC::OSG1, signature("SV_Position", 1))];
    let target = [(FourCC::OSG1, signature("SV_Target", 64))];
    let text = "# A vertex program exporting a constant position and a pixel program\n\
                # exporting a constant colour; the state below is what gets mapped.\n\
                [pipeline]\n\
                kind = \"graphics\"\n\
                vertex = \"triangle.vs.bin\"\n\
                pixel = \"triangle.ps.bin\"\n\
                topology = \"triangle\"\n\
                render_targets = [\"R8G8B8A8_UNORM\"]\n\
                depth_stencil = \"D32_FLOAT\"\n\n\
                [pipeline.rasterizer]\n\
                cull = \"none\"\n\n\
                [pipeline.depth]\n\
                enable = true\n\
                func = \"less_equal\"\n\n\
                [[pipeline.blend]]\n\
                enable = true\n\
                src = \"src_alpha\"\n\
                dst = \"inv_src_alpha\"\n\n\
                [[pipeline.input]]\n\
                semantic = \"POSITION\"\n\
                format = \"R32G32B32_FLOAT\"\n\n\
                [[pipeline.input]]\n\
                semantic = \"TEXCOORD\"\n\
                format = \"R16G16_FLOAT\"\n"
        .to_owned();
    (
        container(VERTEX, &vs, &[], &vs_data, &position),
        container(PIXEL, &ps, &[], &ps_data, &target),
        text,
    )
}

/// Writes every fixture and its pipeline description into `dir`.
pub fn run(dir: &Path) -> Result<()> {
    victory_core::ensure_outside_repo(dir)?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let write = |name: &str, bytes: &[u8]| -> Result<()> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
        Ok(())
    };
    let (cs, text) = bindings();
    write("bindings.cs.bin", &cs)?;
    write("bindings.toml", text.as_bytes())?;
    let (cs, text) = skipped_ballot();
    write("skipped-ballot.cs.bin", &cs)?;
    write("skipped-ballot.toml", text.as_bytes())?;
    let (vs, ps, text) = triangle();
    write("triangle.vs.bin", &vs)?;
    write("triangle.ps.bin", &ps)?;
    write("triangle.toml", text.as_bytes())?;
    Ok(())
}
