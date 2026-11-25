# Contributing to Victory United

Thank you for helping. This project collects the best fixes for running GTA VI (Xbox Series build)
on Windows into one tree. Read [LEGAL.md](LEGAL.md) first. It is short and it is the part that
keeps this project alive.

## The rules that cannot be bent

1. **Nothing from the game goes into the repository.** No files, bytes, shaders, textures, text,
   screenshots, recordings, extracted tables. `cargo xtask policy` checks this and CI runs it.
   Allowed: your own code, build identities (hashes), per-build function offsets under names you
   choose, byte patterns that locate code, small interop constants you wrote.
2. **No leaked, stolen or pirated material**, and do not use any as a reference. If unsure where
   something came from, leave it out.
3. **No circumvention** of copy protection, encryption or licensing. No dumping or decryption
   guides, tools or keys.
4. **Licences:** your contribution is GPL-3.0-or-later. Code taken from another project needs a
   compatible licence, and its source and author must be named in the PR and in
   [PROVENANCE.md](PROVENANCE.md). "Reference only" licences (for example XCL's GPL-2.0-only) mean
   read, do not copy.
5. **Speed never comes from making the game look worse than on the Xbox.** Lower precision,
   skipped effects, dropped shadows or resolution cuts are rejected, even if they are
   opt-in defaults.
6. **Nothing goes in that has not been built and run.** "It compiles" is not enough.

## AI-assisted code is welcome

Using AI tools (Claude, Codex, Gemini, Copilot or anything else) to write, review or debug code is
completely fine here, and the maintainers use them heavily. **How the code was written does not
matter. How good it is does.** AI-written code is held to exactly the same bar as any other:

- **You are responsible for it.** If you submit it, you have read it, understood it and can explain
  it. Do not paste output you do not understand.
- **It must be tested on a real run**, not just compiled. A model saying "this should work" is not a
  test. Put the measurement in the PR (fps or ms per frame before and after, or the crash it fixes).
- **High quality, no slop.** Small focused changes, no unrelated refactors, no dead code, no
  speculative features, no comments that only restate the code. Follow the existing style
  and naming.
- **Safe.** Be careful with `unsafe` (every block needs a comment saying why it is sound), memory
  handling, integer overflow, threading and anything that parses data from a file. No new
  dependencies without a reason in the PR. No secrets, keys, personal paths or tokens.
- **Never feed the game's content to an AI tool you do not control and then commit what comes
  back.** Output that reproduces game data breaks rule 1 no matter who or what wrote it.
- You do not have to announce that AI was used, but if a tool did most of the work and you
  did not fully verify it, say so in the PR so the reviewer looks harder.

## Quality checklist (CI runs clippy, the tests and the policy check)

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked        # includes the asset policy check
cargo xtask policy
```

Also:

- Keep each PR to one topic, one reviewable commit series, with a clear message that says why.
- Add or update a test when behaviour can be tested without game files (synthetic data only).
- Keep any new diagnostics behind the existing diagnostic feature or an environment variable.
- Update docs when behaviour changes.
- Do not commit builds, caches, logs, saves or dumps.

## Proposing a fix from another project

Open an issue titled `Include: <project> #<number>` with:

- the link to the pull request or commit and its licence,
- what it fixes and what you measured (before/after, same scene, same PC),
- whether it touches visuals (it must not make them worse).

Maintainers merge each accepted change as its own commit, keep the original author's credit, and
add a line to [PROVENANCE.md](PROVENANCE.md). Changes that were tried and rejected are listed there
too, with the reason, so nobody repeats the work.

## Reviewing

Be kind and specific. Review the change, not the person. Measurements beat opinions.
