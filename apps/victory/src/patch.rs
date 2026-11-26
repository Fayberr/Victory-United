//! Byte patches for title builds the host works around, taken from
//! `VICTORY_PATCH`: `rva=hex[:expected],…`, for example
//! `0x28d0d60=31c0c3:ffc9f7c1`.
//!
//! A patch is only written when the bytes it lands on are still the ones it
//! was written for, so a different game build is refused instead of being
//! half-patched. The game's own files are never touched: this runs on the
//! prepared copy. The same variable carries the import keys the save
//! provider reads (`crates/victory-hle/src/modules/xgamesave.rs`).

use anyhow::{Context, Result};

use victory_loader::PreparedImage;

/// One `rva=hex[:expected]` item.
#[derive(Debug, PartialEq)]
pub struct Patch {
    pub rva: u32,
    pub new: Vec<u8>,
    pub expected: Option<Vec<u8>>,
}

fn hex(s: &str) -> Result<Vec<u8>> {
    anyhow::ensure!(s.len() % 2 == 0, "odd hex length: {s}");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("bad hex"))
        .collect()
}

/// Parse the whole switch. Empty items are skipped.
pub fn parse(spec: &str) -> Result<Vec<Patch>> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (at, rest) = item
                .split_once('=')
                .with_context(|| format!("item {item:?} needs rva=bytes"))?;
            let rva = u32::from_str_radix(at.trim().trim_start_matches("0x"), 16)
                .with_context(|| format!("bad rva in item {item:?}"))?;
            let rest = rest.trim();
            let (new, expected) = match rest.split_once(':') {
                Some((n, e)) => (hex(n.trim())?, Some(hex(e.trim())?)),
                None => (hex(rest)?, None),
            };
            anyhow::ensure!(!new.is_empty(), "item {item:?} has no bytes to write");
            Ok(Patch { rva, new, expected })
        })
        .collect()
}

/// Patch one image's bytes. `at` maps an RVA to the file offset holding it.
pub fn apply(
    mut at: impl FnMut(u32) -> Option<usize>,
    bytes: &mut [u8],
    patches: &[Patch],
) -> Result<()> {
    for p in patches {
        let off = at(p.rva).with_context(|| format!("rva {:#x} is not file-backed", p.rva))?;
        let len = p.new.len().max(p.expected.as_ref().map_or(0, Vec::len));
        let slot = bytes
            .get_mut(off..off + len)
            .with_context(|| format!("rva {:#x} runs past the image", p.rva))?;
        if let Some(e) = &p.expected {
            anyhow::ensure!(
                slot.starts_with(e),
                "rva {:#x}: found {:02x?}, expected {:02x?}; not patching",
                p.rva,
                &slot[..e.len()],
                e
            );
        }
        println!(
            "patch          {:#x}: {:02x?} -> {:02x?}",
            p.rva,
            &slot[..p.new.len()],
            p.new
        );
        slot[..p.new.len()].copy_from_slice(&p.new);
    }
    Ok(())
}

/// Apply `VICTORY_PATCH` when it is set.
pub fn apply_from_env(prepared: &mut PreparedImage) -> Result<()> {
    let Ok(spec) = std::env::var("VICTORY_PATCH") else {
        return Ok(());
    };
    let patches = parse(&spec)?;
    let info = prepared.info.clone();
    apply(
        |rva| info.rva_to_file_offset(rva).map(|o| o as usize),
        &mut prepared.bytes,
        &patches,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_without_expectation_parses() {
        assert_eq!(
            parse("0x10=90").unwrap(),
            [Patch {
                rva: 0x10,
                new: vec![0x90],
                expected: None
            }]
        );
    }

    #[test]
    fn the_expectation_is_kept() {
        let p = parse(" 0x28d0d60 = 31c0c3 : ffc9f7c1 ").unwrap();
        assert_eq!(p[0].rva, 0x28d0d60);
        assert_eq!(p[0].new, [0x31, 0xc0, 0xc3]);
        assert_eq!(
            p[0].expected.as_deref(),
            Some(&[0xff, 0xc9, 0xf7, 0xc1][..])
        );
    }

    #[test]
    fn several_items_and_empty_ones() {
        assert_eq!(parse("0x1=00,,0x2=01:02,").unwrap().len(), 2);
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn broken_items_are_named() {
        for bad in ["0x1", "1=9", "zz=90", "0x1=9g", "0x1="] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn matching_bytes_are_replaced() {
        let mut bytes = [0xaa; 8];
        bytes[2..4].copy_from_slice(&[0xff, 0xc9]);
        let patches = parse("0x2=31c0c3:ffc9").unwrap();
        apply(|rva| Some(rva as usize), &mut bytes, &patches).unwrap();
        assert_eq!(bytes, [0xaa, 0xaa, 0x31, 0xc0, 0xc3, 0xaa, 0xaa, 0xaa]);
    }

    #[test]
    fn other_bytes_are_refused_and_left_alone() {
        let mut bytes = [0xaa; 8];
        let patches = parse("0x2=31:ff").unwrap();
        let err = apply(|rva| Some(rva as usize), &mut bytes, &patches).unwrap_err();
        assert!(err.to_string().contains("expected"), "unexpected: {err}");
        assert_eq!(bytes, [0xaa; 8]);
    }

    #[test]
    fn an_unmapped_rva_is_an_error() {
        let mut bytes = [0u8; 4];
        let err = apply(|_| None, &mut bytes, &parse("0x100=00").unwrap()).unwrap_err();
        assert!(
            err.to_string().contains("not file-backed"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn a_patch_past_the_end_is_an_error() {
        let mut bytes = [0u8; 4];
        let err = apply(|_| Some(2), &mut bytes, &parse("0x0=00010203").unwrap()).unwrap_err();
        assert!(
            err.to_string().contains("past the image"),
            "unexpected: {err}"
        );
    }
}
