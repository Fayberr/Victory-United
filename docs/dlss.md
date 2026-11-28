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
Series X profile. Clear `VICTORY_FSR`, `VICTORY_FSR_4K`, `VICTORY_DLSS_4K`,
`VICTORY_FFX_SCALE`, `VICTORY_RENDER_SCALE`, and `VICTORY_UPSCALER` before
selecting Streamline. The 4K and Series S direct NGX examples in the README
apply to that backend.

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

## NVIDIA frame generation

Set `VICTORY_FRAMEGEN=dlss` to generate one frame between rendered frames.
Use `VICTORY_DLSS=1` on Series X. On Series S, frame generation can also use
the title's native FidelityFX or the PC FSR path. The existing
`VICTORY_FRAMEGEN=1` setting continues to select AMD frame generation.

1. Keep `sl.dlss_g.dll`, `sl.reflex.dll`, `sl.pcl.dll` and `nvngx_dlssg.dll`
   from the same Streamline SDK release in `VICTORY_STREAMLINE_DIR`. Enable
   Hardware-accelerated GPU scheduling in Windows.
2. For an RTX 30 or RTX 20, extract the [dlssg_for_sm86 release](https://github.com/sdli1995/dlssg_for_sm86/releases/tag/0.4.2)
   outside the checkout. Set `VICTORY_DLSSG_SM86_DIR` to the folder holding
   `version.dll`, `dlssg_sm86.ini` and the complete `dlssg_sm86\` directory.
   Victory United loads that proxy by its full path before Streamline starts.
   Leave the add-on variable unset for native support on RTX 40 and later.
   With the add-on, the host selects CUDA device 0 when no adapter is specified.
3. Use `VICTORY_THIN=1` or `VICTORY_ASYNC_SUBMIT=1`, with
   `VICTORY_LATE_WAIT=3` and the wait cuts enabled. This orders application
   submissions and presentation on the same thread.

Frame generation tags depth and motion from the submitted upscaler work.
Presenting without a fresh dispatch or on a history reset disables
interpolation. At matching plane sizes, Series S can supply the scene and HUD
separately. On Series X, the presenter uses a scene copy recorded just before
the known HUD composite. Set `VICTORY_FRAMEGEN_HUDLESS=0` to skip those copies.
An absent copy can leave the HUD inside the interpolated image.

Check the log for `DLSS Frame Generation: Streamline swap-chain proxy is active`,
`DLSS Frame Generation: tagged the first submitted depth and motion`,
`DLSS Frame Generation switched` with `on=true`, and status `0`. The periodic
`rendered` and `shown` counts distinguish real presents from displayed frames
including interpolation. At a low base frame rate, additional displayed frames
can come with more latency, visible interpolation errors, and extra VRAM use.

See the [NVIDIA DLSS-G integration guide](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.16/docs/ProgrammingGuideDLSS_G.md).
