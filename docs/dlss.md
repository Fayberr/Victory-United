# NVIDIA Streamline

Set `VICTORY_DLSS=1` for Streamline Super Resolution on Windows. The title
renders a smaller scene and passes its FidelityFX colour, depth, motion,
exposure and reactive textures to DLSS on the same D3D12 command list.
The default launch uses the title's own renderer. Existing DLSS preset values
(`VICTORY_DLSS=quality`, `balanced`, `performance`, or `VICTORY_DLSS_4K`)
continue to select the direct NGX path.

## Setup

Download `streamline-sdk-v2.14.16.zip` from the [NVIDIA Streamline SDK release](https://github.com/NVIDIA-RTX/Streamline/releases/tag/v2.14.16).
Extract it outside the checkout. Point `VICTORY_STREAMLINE_DIR` at
`bin\x64`, which must contain `sl.interposer.dll`, `sl.common.dll`,
`sl.dlss.dll` and `nvngx_dlss.dll` from that SDK. Victory United loads the
interposer at runtime after checking its Authenticode signature.

| Variable | Values |
| --- | --- |
| `VICTORY_DLSS` | `1` selects Streamline Super Resolution. |
| `VICTORY_DLSS_MODE` | `quality` (default), `balanced`, `performance`. |
| `VICTORY_STREAMLINE_DIR` | SDK library folder, required. |
| `VICTORY_STREAMLINE_LOG` | Optional folder for verbose Streamline logs. |
| `VICTORY_DLSS_PRESET` | `default`, `j`, `k`, `l`, `m`. |
| `VICTORY_DLSS_JITTER`, `VICTORY_DLSS_MV` | Optional `x,y` multipliers. |
| `VICTORY_DLSS_REACTIVE` | `0` omits the reactive mask. |

For Series X at 1440p, set `VICTORY_OUTPUT=2560x1440` and launch with the
Series X profile. Clear any separate FSR, direct DLSS, or FidelityFX scale
and renderer overrides before selecting Streamline. The 4K and Series S
direct NGX examples in the README apply to that backend.

On successful startup, look for `Streamline loaded`, `Streamline: DLSS Super
Resolution loaded` and `Streamline DLSS: selected the FidelityFX renderer`.
On the first gameplay dispatch, look for `Streamline DLSS: recorded the
title's upscaling` with input and output sizes. For Quality at 1440p expect
approximately 1706 by 960 input and 2560 by 1440 output.

Missing files, unsupported adapters and failed initialization leave the
title's upscaler selected. If DLSS fails after initialization, the FidelityFX
bridge can switch to the optional PC FSR provider in
`VICTORY_FIDELITYFX_DIR` (FSR 4.1.2 on Series X or 3.1.6 on Series S).
Without that provider, check the log for `DLSS: stopped for this upscaler
context` and restart with `VICTORY_DLSS` unset to return to the title renderer.

The manual integration follows NVIDIA's [DLSS guide](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.16/docs/ProgrammingGuideDLSS.md)
and [manual hooking guide](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.16/docs/ProgrammingGuideManualHooking.md).
