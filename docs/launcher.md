# Victory Launcher

The launcher starts Victory United, picks settings that suit your graphics card, translates the game's
shaders the first time, and keeps logs. You bring your own copy of the game: the launcher never copies,
bundles or downloads game files.

## For players

1. Unpack the ZIP anywhere and run `victory-launcher.exe`.
2. On first start, choose the folder of your game. It is the folder with `MicrosoftGame.Config` in it, for
   example `D:\XboxGames\<title>\Content`. The launcher also looks in the usual Xbox app folders on your
   drives, and you can drop a folder onto the window.
3. Press Play.

The first start translates the game's shaders. That takes a while once; later starts are faster. You can
skip it, but the game then stutters more at first. The bar shows how far it is and how long it will
probably take.

The Play tab shows the preset it picked for your graphics card. "Automatic" is a good choice. If the
launcher warns about your PC (no mesh shader support, little memory), take that seriously: the game may not
run or may run badly.

When the game stops, the Play tab says how it ended and where the log is. If it stopped within 30 seconds
while you had extra flags set, it offers to start again without them.

Windows or antivirus may warn about an unsigned program. That is expected for a build that is not signed.

## Settings

Settings are saved in a profile. The top of the tab has the everyday things: preset, console model (Series
X or Series S), full screen, the performance panel (F3 hides it in the game), ray tracing, safe mode,
upscaling. Safe mode leaves out every way that is still on trial, so try it first when something is wrong.

Below that are the settings that matter most, with a description each: graphics, memory, threads and
debugging. Anything you changed from the preset is marked, with a Reset button for each, and "Reset to
preset" for all of them. "Final launch settings" shows exactly what the engine will get, and you can copy it.

DLSS and frame generation appear as unavailable until an engine build supports them. The launcher does not
include NVIDIA's files; when support arrives it says which files are needed and where to point it.

Profiles let you keep several sets of settings: create, duplicate, rename, delete, export to a `.toml` file
and import one.

## For developers and testers

**Flags tab.** Every `VICTORY_*` variable the code reads (about 550) with its type, default, a risk label
(safe, advanced, experimental) and a description where there is one. Add any flag by name and value; names
that are not in the list work too. Extra flags are saved in `launch-flags.txt` next to `settings.toml` and
are applied last, so they win over the preset and the Settings tab. A flag that is only checked for being
present is removed when you turn it off, never set to `0`.

"Start without my flags" starts once without any of them and deletes nothing.

**Order of values.** Preset, then the options of the profile, then the edits made in the Settings tab, then
extra flags. Later wins.

**Logs.** Each start writes `logs\run-<date>-<time>.log` with the engine's output and errors. The last 20
are kept. The warm-up writes its own `warmup-*.log`.

**Your own engine build.** In Settings, "Choose victory.exe..." selects a build you made yourself instead of
the installed one. "Use the installed engine" goes back.

**Bug report.** The tab builds a ZIP in `reports\` with the tail of the latest log, the flags, versions,
hardware and settings. You see every item first and can untick any. Your Windows user name and profile
folder are replaced by placeholders. Nothing is uploaded: attach the ZIP to an issue yourself. A ready
issue text goes to the clipboard.

**Where things are.** Settings, profiles, logs, caches and reports are in `%APPDATA%\Victory-United`. The
About tab has buttons that open them. Each game folder gets its own shader cache.

**Adding presets and flags.** Presets are in `crates/victory-launcher/data/presets.toml`, the flag list in
`crates/victory-launcher/data/flags.toml`. A test fails when the code reads a `VICTORY_*` variable that is
not in the list; `python3 crates/victory-launcher/tools/gen_flags.py` adds new names and keeps what is
already documented. `scripts/play.ps1` and `presets.toml` hold the same machine values: change both together.
