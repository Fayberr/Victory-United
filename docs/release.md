# Making a release

A release is one GitHub release with these files:

| File | What it is |
|---|---|
| `victory-united-<version>-windows-x64.zip` | What a player downloads: `victory-launcher.exe`, `engine\` with the current build, notices |
| `engine-<commit>-<kind>.zip` | The engine alone. The launcher's updater downloads this one |
| `manifest.json` | Describes the engine builds of the release (see below) |
| `SHA256SUMS.txt` | SHA-256 of the files above |

## Steps

1. Build the engine on Windows (`cargo build --release -p victory`, and with `--features diagnostics` for the
   diagnostics build) and the launcher (`cargo build --release -p victory-launcher`).
2. Run `scripts\release\package.ps1` once per engine kind:

       scripts\release\package.ps1 -EngineDir target\release -LauncherExe target\release\victory-launcher.exe `
           -Kind release -Version 0.1.0 -Channel stable -Out dist -BaseUrl https://github.com/Fayberr/Victory-United/releases/download/v0.1.0

   `-DryRun` works on any folder that contains a `victory.exe`, without a launcher exe.
   `scripts\release\test-package.ps1` runs a dry run on a fake folder and checks the output.
3. Create the GitHub release by hand and attach the files from `dist\`. The script does not upload anything
   and does not create tags.

Everything that the engine needs sits next to `victory.exe`, so the whole folder goes into the engine ZIP, except
`.pdb` files (symbols stay out; the profiler needs them, so keep them from the build you profile).

## Channels

- `stable`: the latest normal release. The launcher asks `releases/latest`.
- `dev`: the newest build of main, published as a pre-release. The launcher asks the release list and takes
  the newest pre-release.

The launcher picks the channel in its Updates tab. A manifest may list both a `release` and a `diagnostics` engine.

## manifest.json

```json
{
  "schema": 1,
  "launcher_version": "0.1.0",
  "launcher_url": "https://github.com/Fayberr/Victory-United/releases/tag/v0.1.0",
  "channels": {
    "stable": {
      "engines": [
        { "build": "<commit>", "translator": "<version>", "kind": "release",
          "url": "<download url of the engine ZIP>", "sha256": "<hex>", "size": 12345, "notes": "<text>" }
      ]
    }
  }
}
```

`schema` is bumped when the format changes in a way an older launcher cannot read; the launcher then says it
needs a newer version. The updater checks size and SHA-256 before anything is installed, unpacks into a new
`engine\<commit8>-<kind>` folder, and switches `engine\current.txt` last. The previous build is kept for
rollback, and the last three builds are kept in all.

## What is not in the ZIP

- The game. It is never copied, bundled or downloaded (docs/asset-policy.md). The player points the launcher to
  their own installation.
- NVIDIA DLSS and Streamline files (`nvngx_dlss.dll`, `sl.*.dll` and similar). Whether they may be
  redistributed has not been checked, so the launcher tells the user where to get them and `package.ps1` refuses to
  package them.

## Unsigned executables

The executables are not code signed. Windows SmartScreen may show "Windows protected your PC" on the first
start (More info, then Run anyway), and some antivirus programs flag new unsigned programs, especially ones that
translate and run other software's code. The SHA-256 list lets anyone compare the download with what was
published. If a program is blocked or deleted, add the folder to its exclusions or report the false positive to
the vendor.
