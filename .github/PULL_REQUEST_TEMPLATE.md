## What and why

<!-- One topic per PR. Say why, not only what. -->

## Source

<!-- Your own work, or taken from another project/PR? Link, author and licence. -->

## Measured on a real run

<!-- PC (CPU/GPU), scene, before and after: fps or ms per frame, or the crash it fixes. "It compiles" is not a test. -->

## Checklist (all required)

- [ ] This contains **no game material** (no files, bytes, shaders, textures, text, screenshots, recordings, extracted tables). `cargo xtask policy` passes.
- [ ] No leaked, stolen or pirated material, and I did not use any as a reference.
- [ ] No copy-protection, encryption or licence circumvention.
- [ ] Any code from elsewhere has a compatible licence and is credited here and in PROVENANCE.md.
- [ ] It does **not** make the game look worse than on the Xbox.
- [ ] I built it and ran it, and I have read and understand every line (including any AI-written code).
- [ ] `cargo fmt`, `cargo clippy -D warnings` and `cargo test` pass.
- [ ] Every new `unsafe` block has a comment explaining why it is sound.
- [ ] No secrets, tokens or personal paths.
