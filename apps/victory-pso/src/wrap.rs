//! `wrap`: RDNA2 machine code of our own, from any assembler or compiler,
//! wrapped the way the console's shader compiler wraps its output so that
//! the translator takes it. The use it is made for: test shaders compiled
//! by LLVM's AMD GPU back end (for example the `clang` AMD ships with its
//! Radeon GPU Analyzer), which gives a real compiler's code patterns.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::fixture::{COMPUTE, Compute, Param, Record, compute_stage, container_of};

/// A program description: where the code is and how the program is started.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    /// An ELF object (its `.text` is taken) or raw machine code, relative to
    /// the description.
    code: PathBuf,
    /// `compute`, the only stage wrapped so far.
    stage: Option<String>,
    threads: [u32; 3],
    /// SGPRs the root parameters arrive in (`s0` on).
    user_sgprs: u32,
    /// Which components of the thread group's id follow them in SGPRs.
    #[serde(default)]
    group_id: [bool; 3],
    /// Components of the thread's id in its group, in `v0` on (default 1).
    thread_id_components: Option<u32>,
    #[serde(default)]
    lds_bytes: u32,
    /// The code is written for 32 lanes per wave (default 64).
    #[serde(default)]
    wave32: bool,
    /// VGPRs the code uses (default 64).
    vgprs: Option<u32>,
    #[serde(default)]
    param: Vec<ParamDescription>,
}

/// A root parameter of the title's root signature and where it arrives.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParamDescription {
    /// `constants`, `descriptor`, `table`, `resource-heap` or `sampler-heap`.
    root: String,
    /// Its index in the root signature.
    slot: u8,
    /// The first user SGPR holding it.
    user_data: u8,
    /// How many user SGPRs hold it (default 1).
    sgprs: Option<u32>,
}

/// The `.text` section of a little-endian 64-bit ELF object.
fn elf_text(elf: &[u8]) -> Result<&[u8]> {
    let u16_at = |at: usize| -> Result<usize> {
        let b = elf.get(at..at + 2).context("a truncated ELF object")?;
        Ok(usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let u32_at = |at: usize| -> Result<usize> {
        let b = elf.get(at..at + 4).context("a truncated ELF object")?;
        Ok(u32::from_le_bytes(b.try_into().expect("4 bytes")) as usize)
    };
    let u64_at = |at: usize| -> Result<usize> {
        let b = elf.get(at..at + 8).context("a truncated ELF object")?;
        usize::try_from(u64::from_le_bytes(b.try_into().expect("8 bytes")))
            .context("an ELF offset out of range")
    };
    if elf.get(4..6) != Some(&[2, 1]) {
        bail!("unsupported ELF object: not 64-bit little-endian");
    }
    let (table, size, count, names) = (u64_at(0x28)?, u16_at(0x3A)?, u16_at(0x3C)?, u16_at(0x3E)?);
    let section = |i: usize| -> Result<(usize, &[u8])> {
        let at = table + i * size;
        let (offset, len) = (u64_at(at + 0x18)?, u64_at(at + 0x20)?);
        let data = elf
            .get(
                offset
                    ..offset
                        .checked_add(len)
                        .context("an ELF section out of range")?,
            )
            .context("an ELF section out of range")?;
        Ok((u32_at(at)?, data))
    };
    let (_, strings) = section(names)?;
    for i in 0..count {
        let (name, data) = section(i)?;
        let name = strings.get(name..).unwrap_or(&[]);
        if name.starts_with(b".text\0") {
            return Ok(data);
        }
    }
    bail!("the ELF object has no .text section")
}

fn root_code(name: &str) -> Result<u32> {
    Ok(match name {
        "constants" => 0,
        "descriptor" => 1,
        "table" => 3,
        "resource-heap" => 5,
        "sampler-heap" => 6,
        other => bail!(
            "unsupported root parameter kind `{other}` (wrapped: constants, descriptor, table, \
             resource-heap, sampler-heap)"
        ),
    })
}

pub fn run(description: &Path, out: &Path) -> Result<()> {
    victory_core::ensure_outside_repo(out)?;
    let text = std::fs::read_to_string(description)
        .with_context(|| format!("reading {}", description.display()))?;
    let p: Program = toml::from_str(&text)
        .map_err(|e| anyhow!("{}: not a program description: {e}", description.display()))?;
    match p.stage.as_deref() {
        None | Some("compute") => {}
        Some(other) => bail!("unsupported stage `{other}` (wrapped: compute)"),
    }
    let source = description.parent().unwrap_or(Path::new(".")).join(&p.code);
    let bytes = std::fs::read(&source).with_context(|| format!("reading {}", source.display()))?;
    let code = if bytes.starts_with(b"\x7fELF") {
        elf_text(&bytes).with_context(|| source.display().to_string())?
    } else {
        &bytes[..]
    };
    if code.is_empty() || code.len() % 4 != 0 {
        bail!(
            "{}: {} bytes of code is not a whole number of instruction words",
            source.display(),
            code.len()
        );
    }
    if p.user_sgprs > 16 {
        bail!(
            "unsupported: {} user SGPRs (the console has 16)",
            p.user_sgprs
        );
    }
    if p.threads.contains(&0) {
        bail!("a thread group of {:?} has no threads", p.threads);
    }
    let params = p
        .param
        .iter()
        .map(|d| {
            let sgprs = d.sgprs.unwrap_or(1);
            if u32::from(d.user_data) + sgprs > p.user_sgprs {
                bail!(
                    "root parameter {} takes user SGPRs {} to {}, past the {} declared",
                    d.slot,
                    d.user_data,
                    u32::from(d.user_data) + sgprs - 1,
                    p.user_sgprs
                );
            }
            Ok(Param {
                root: root_code(&d.root)?,
                sgprs,
                user_data: d.user_data,
                slot: d.slot,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let stage_data = compute_stage(&Compute {
        user_sgprs: p.user_sgprs,
        group_id: p.group_id,
        thread_id_components: p.thread_id_components.unwrap_or(1),
        lds_bytes: p.lds_bytes,
        threads: p.threads,
    });
    let container = container_of(
        code,
        &Record {
            stage: COMPUTE,
            params: &params,
            stage_data: &stage_data,
            signatures: &[],
            vgprs: p.vgprs.unwrap_or(64),
            wave32: p.wave32,
        },
    );
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(out, &container).with_context(|| format!("writing {}", out.display()))?;
    println!(
        "wrapped {} ({} bytes of code, {} root parameters) -> {} ({} bytes)",
        source.display(),
        code.len(),
        params.len(),
        out.display(),
        container.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ELF64 object with a string table and a `.text` of `code`.
    fn object(code: &[u8]) -> Vec<u8> {
        let names = b"\0.text\0.shstrtab\0";
        let mut elf = vec![0u8; 0x40];
        elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
        let text_at = elf.len();
        elf.extend(code);
        let names_at = elf.len();
        elf.extend(names);
        let table = elf.len();
        let header = |name: u32, offset: usize, len: usize| {
            let mut h = [0u8; 0x40];
            h[..4].copy_from_slice(&name.to_le_bytes());
            h[0x18..0x20].copy_from_slice(&(offset as u64).to_le_bytes());
            h[0x20..0x28].copy_from_slice(&(len as u64).to_le_bytes());
            h
        };
        elf.extend([0u8; 0x40]);
        elf.extend(header(1, text_at, code.len()));
        elf.extend(header(7, names_at, names.len()));
        elf[0x28..0x30].copy_from_slice(&(table as u64).to_le_bytes());
        elf[0x3A..0x3C].copy_from_slice(&0x40u16.to_le_bytes());
        elf[0x3C..0x3E].copy_from_slice(&3u16.to_le_bytes());
        elf[0x3E..0x40].copy_from_slice(&2u16.to_le_bytes());
        elf
    }

    #[test]
    fn the_text_section_of_an_object_is_its_code() {
        let code = [0x00, 0x00, 0x81, 0xBF];
        assert_eq!(elf_text(&object(&code)).unwrap(), code);
        let mut cut = object(&code);
        cut.truncate(0x50);
        assert!(elf_text(&cut).is_err());
    }
}
