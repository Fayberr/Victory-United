# Pipelines without a title: `victory-pso`

A second, small program builds host pipelines from shader containers and a
description of the pipeline state, and checks them on an adapter of your
choice. It loads no title: no executable, launcher, asset or runtime, only
the translator (`victory-shader`).

```
victory-pso fixture <dir>
victory-pso wrap <program.toml> --out <container>
victory-pso build <pipeline.toml> --out <dir> [--wave64 native|wave32]
victory-pso verify <dir> [--adapter nvidia|warp|any] [--run]
victory-pso bench [--adapter nvidia|warp|any] [--only <groups>] [--out <report.toml>]
```

- `fixture` writes synthetic inputs (our own GFX10 code in the console's
  container format) with their pipeline descriptions: `bindings` (root
  constants and a buffer at a view-heap index), `skipped-ballot` (a wave
  count in skipped code, two 64-lane console waves) and `triangle` (a
  vertex and a pixel program with graphics state).
- `wrap` puts RDNA2 machine code of our own (an ELF object's `.text`, or
  raw code) into that container format, as a program description says
  (thread group, user SGPRs, root parameters). It is how test shaders
  compiled by LLVM's AMD GPU back end reach the translator: a real
  compiler's code, where `fixture`'s programs are assembled by hand.
- `build` translates the containers a description names, checks the DXIL
  with `dxil.dll` where there is one, maps the state (topology, target and
  depth formats, rasterizer, depth and blend state, input layout) and writes
  `<stage>.dxil` and `pipeline.toml`: each stage's user-data dwords with the
  title root parameter that fills them, and the host root signature's
  parameters. Anything it does not map is an error that names it.
- `verify` picks the adapter through DXGI, prints it with its shader model
  and wave lane counts, and creates the root signature and pipeline state.
  With `--run`, a compute pipeline is dispatched with the description's
  `[run]` bindings and its buffers are read back and compared.

- `bench` times what the host's D3D12 driver does with work of its own, no
  translated shader: draws with each kind of state change between them and
  split over submissions, copies on each queue, fences, pipeline creation
  (cold, again, from a blob, on threads), resources and barriers. Each row
  has the CPU's recording, submitting and waiting and the GPU's own time
  apart, and the results are checked, so a wrong driver fails. `--only
  present` opens a small window; `--debug-layer` checks the program's own
  use of D3D12 (its times then say nothing); `hooks` names overlays and
  capture tools inside the process, whose cost would read as the driver's.

`--wave64` names the host 64-lane compute programs are translated for:
`wave32` (the default) one whose widest wave has 32 lanes, as NVIDIA's,
`native` one with 64-lane waves. The translator has no form for other wave
sizes: on the software adapter (4-lane waves) only programs that do not
count over a wave or read other lanes run.

Exit codes: 0 done; 1 an error; 3 the adapter asked for is not there (for
example `--adapter nvidia` on a machine without one); 4 a run wrote
something else than expected. Output folders must be outside the source
tree, as for every tool here.

```powershell
cargo build -p victory-pso
target\debug\victory-pso.exe fixture D:\out\fixtures
target\debug\victory-pso.exe build D:\out\fixtures\bindings.toml --out D:\out\bindings
target\debug\victory-pso.exe verify D:\out\bindings --adapter nvidia --run
cargo test -p victory-pso          # the same, on WARP and on NVIDIA if present
```

`tests/llvm.rs` holds test shaders as LLVM IR. With `VICTORY_AMDGPU_CLANG`
set to a clang that has LLVM's AMD GPU back end, `cargo test -p victory-pso`
also compiles them for RDNA2 (`-mcpu=gfx1030`), wraps, translates and runs
them against values worked out from their source. A case also fails when
the translator only took one of its approximate forms for it (each host
wave counted as a wave of its own): such a translation validates and runs,
and writes other values than the console would. The official LLVM builds
for Windows lack that back end; the clang AMD ships with its Radeon GPU
Analyzer has it (`utils\lc\opencl\bin\clang.exe`). Without the variable
those tests are skipped.
