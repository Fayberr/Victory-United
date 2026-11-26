# KLBF Lab: beginner's guide (Windows)

Every step below is a command to copy and paste, in order. You only need your own copy of the title's Xbox Series files in a folder. KLBF Lab includes no game files.

In this guide the game files are in **`C:\Games\Title`** (the folder that holds the title's `.exe` and `shaders.rpf`). If yours are elsewhere, replace that path everywhere.

Open **PowerShell** (Start menu → type `PowerShell` → Enter) and paste each block. Wait for one block to finish before the next.

---

## 1. Check your PC (1 minute)

- A recent graphics card with DirectX 12 (tested on AMD Radeon RX 9070 XT only).
- **More than 8 GB of video memory** recommended.
- **32 GB of RAM**, or 16 GB plus a large page file (step 2).
- About 15 GB of free disk space for the tools and the shader cache.

## 2. Page file (only if you have 16 GB of RAM)

```powershell
SystemPropertiesPerformance.exe
```

Then: **Advanced** tab → **Virtual memory: Change…** → tick **Automatically manage paging file size for all drives** (or untick it, pick your fastest drive, **Custom size**, Initial `32768`, Maximum `49152`) → **Set** → **OK** → restart the PC.

## 3. Install the tools (10–20 minutes, once)

```powershell
winget install --id Git.Git -e --source winget
winget install --id Rustlang.Rustup -e --source winget
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --source winget --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

**Close PowerShell and open a new one** so the new tools are found.

## 4. Download and build KLBF Lab (5–15 minutes)

```powershell
cd $HOME
git clone -b beta-2 https://github.com/PastiiJay/klbf-lab
cd klbf-lab
cargo build --release
```

## 5. Get AMD's FidelityFX SDK (for Series S mode)

Download **FidelityFX SDK v2.4.0** from AMD's official page: https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/releases. Unzip it to **`C:\FidelityFX-SDK`**. Check that this file exists: `C:\FidelityFX-SDK\Kits\FidelityFX\signedbin\amd_fidelityfx_upscaler_dx12.dll`.

## 6. Pre-build the shaders (10–20 minutes, once per build)

```powershell
cd $HOME\klbf-lab
powershell -ExecutionPolicy Bypass -File .\play.ps1 -GameDir C:\Games\Title -Prebuild
```

If the graphics driver crashes, the script starts again by itself where it stopped and remembers what crashed. It's done when it prints `Pre-build done`.

## 7. Play

```powershell
cd $HOME\klbf-lab
powershell -ExecutionPolicy Bypass -File .\play.ps1 -GameDir C:\Games\Title
```

- **Plug in the controller before starting.** The account picker only answers the controller, not the keyboard ([#69](https://github.com/PastiiJay/klbf-lab/issues/69)).
- No FSR? Add `-SeriesX` (slower). Run step 6 again with `-SeriesX` too.
- The first minutes in a new area are slow while shaders finish loading.

## Updating later

```powershell
cd $HOME\klbf-lab
git pull
cargo build --release
```

Then step 6 again, and step 7.

## Something went wrong?

| What you see | What to do |
|---|---|
| `AMD FSR not found` | Step 5, or `-FfxDir <your signedbin folder>` |
| `Build first` | Step 4 |
| Crash about 3 seconds after start, black window | Not enough memory: step 2 (page file), close other programs |
| `X3506` or `vs_5_1` in the log, black window | Old Windows shader compiler: copy the x64 `d3dcompiler_47.dll` from a recent Windows SDK (`Redist\D3D\x64`) next to `target\release\victory.exe` |
| Crash while loading after `dstorage: request failed: zlib … hr=0x8007000d` | Some game files are incomplete or damaged: copy the game folder again from your source (thanks @Emaxoso) |

Still stuck? Open an [issue](https://github.com/PastiiJay/klbf-lab/issues) with your graphics card, driver version, Windows version and the last lines PowerShell printed. Text only, no game screenshots.

---

Thanks to [@HishamMuhammed](https://github.com/HishamMuhammed), whose questions shaped this guide.
