//! What each present showed (`VICTORY_PRESENT_TRACE=1`, off by default): the
//! title's resource, the back buffer, and a hash and the mean color of a few
//! rows of the picture, read once the card has run the copy. Only for finding
//! pictures shown again: nothing of it changes what is shown.

use std::collections::VecDeque;
use std::sync::Mutex;

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const ROWS: u32 = 16;

struct Shown {
    frame: u64,
    flown: u64,
    text: String,
    pitch: u32,
    width: u32,
    buffer: ID3D12Resource,
}

static SHOWN: Mutex<VecDeque<Shown>> = Mutex::new(VecDeque::new());
static TITLE: Mutex<String> = Mutex::new(String::new());

pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("VICTORY_PRESENT_TRACE").is_ok_and(|v| v == "1"))
}

/// What the title passed, for the line of the present that follows.
pub(crate) fn title(text: String) {
    *TITLE.lock().unwrap_or_else(|e| e.into_inner()) = text;
}

/// Records copies of a few rows of `texture` (8-bit color, in the
/// copy-source state) on `list`: the readback buffer, its row pitch and the
/// pixels of a row.
pub(crate) fn record(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    texture: &ID3D12Resource,
) -> Option<(ID3D12Resource, u32, u32)> {
    // SAFETY: a live resource.
    let d = unsafe { texture.GetDesc() };
    if !matches!(d.Format.0, 27..=29 | 87 | 90 | 91) {
        return None;
    }
    let (width, height) = (d.Width as u32, d.Height);
    let pitch = (width * 4).next_multiple_of(256);
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: u64::from(pitch) * u64::from(ROWS),
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
    let mut buffer: Option<ID3D12Resource> = None;
    // SAFETY: plain creation; an open list; locations naming live resources.
    unsafe {
        device
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut buffer,
            )
            .ok()?;
        let buffer = buffer?;
        for i in 0..ROWS {
            let y = (2 * i + 1) * height / (2 * ROWS);
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&buffer),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: u64::from(pitch) * u64::from(i),
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: d.Format,
                            Width: width,
                            Height: 1,
                            Depth: 1,
                            RowPitch: pitch,
                        },
                    },
                },
            };
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(texture),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            };
            let row = D3D12_BOX { left: 0, top: y, front: 0, right: width, bottom: y + 1, back: 1 };
            list.CopyTextureRegion(&to, 0, 0, 0, &from, Some(&row));
        }
        Some((buffer, pitch, width))
    }
}

/// Keeps a present's copy until the card has run flight `flown`.
pub(crate) fn keep(frame: u64, flown: u64, text: &str, copy: (ID3D12Resource, u32, u32)) {
    let title = TITLE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    SHOWN.lock().unwrap_or_else(|e| e.into_inner()).push_back(Shown {
        frame,
        flown,
        text: format!("{title} {text}"),
        pitch: copy.1,
        width: copy.2,
        buffer: copy.0,
    });
}

/// Writes the lines of the presents the card has run.
pub(crate) fn tell(finished: u64) {
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    while shown.front().is_some_and(|s| s.flown <= finished) {
        let Some(s) = shown.pop_front() else { break };
        let mut data = std::ptr::null_mut();
        // SAFETY: the copies have finished; the buffer holds `ROWS` rows.
        unsafe {
            if s.buffer.Map(0, None, Some(&mut data)).is_err() {
                continue;
            }
            let bytes = std::slice::from_raw_parts(data.cast::<u8>(), (s.pitch * ROWS) as usize);
            let (mut hash, mut sum, mut n) = (0xcbf2_9ce4_8422_2325u64, [0u64; 3], 0u64);
            for row in bytes.chunks(s.pitch as usize) {
                for p in row[..(s.width * 4) as usize].chunks_exact(4) {
                    let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                    hash = (hash ^ u64::from(v)).wrapping_mul(0x0100_0000_01b3);
                    for k in 0..3 {
                        sum[k] += u64::from(p[k]);
                    }
                    n += 1;
                }
            }
            s.buffer.Unmap(0, None);
            let n = n.max(1);
            tracing::info!(
                "present trace: frame {} {} hash {hash:016x} mean {} {} {}",
                s.frame,
                s.text,
                sum[0] / n,
                sum[1] / n,
                sum[2] / n
            );
        }
    }
}
