<<<<<<< HEAD
# Contributing to Victory United

Thank you for helping. This project collects the best fixes for running GTA VI (Xbox Series build)
on Windows into one tree. Read [LEGAL.md](LEGAL.md) first. It is short and it is the part that
keeps this project alive.

## The few rules that matter

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
5. **Test it before you send it.** Build it and run it. "It compiles" is not enough.

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
- Update docs when behaviour changes.
- Do not commit builds, caches, logs, saves or dumps.

## Proposing a fix from another project

Open an issue titled `Include: <project> #<number>` with:

- the link to the pull request or commit and its licence,
- what it fixes and what you measured (before/after, same scene, same PC).

Accepted changes are merged as their own commit with the original author's credit, and added to
[PROVENANCE.md](PROVENANCE.md).
||||||| merged common ancestors
=======
# Como contribuir

Obrigado pelo interesse. Este fork do VIctory foca em placas de 8 GB (NVIDIA RTX 4060 Ti como
máquina de teste) e segue as regras abaixo.

## Antes de tudo

- **Nenhum arquivo do jogo** entra no repositório: nem executáveis, nem RPFs, nem shaders extraídos,
  nem capturas de tela do jogo. O `cargo xtask policy` barra binários e arquivos grandes.
- **Nada de código do xcl** (GPL-2.0-only, incompatível). O projeto é GPL-3.0, como o VIctory.
- Sem dependências baixadas na compilação (SDKs, URLs de git em `Cargo.toml`).

## Compilar e testar

```powershell
cargo build --profile diag -p victory --features diagnostics
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

O CI roda esses mesmos passos num Windows sem placa de vídeo (adaptador WARP); um teste que precisa de
placa de verdade deve pular o adaptador de software.

## Commits e pull requests

- Código, comentários e mensagens de commit em inglês; documentação do fork em português.
- Um commit por mudança, com uma primeira linha que diga o que mudou para quem roda
  (ex.: `GPU: placed textures keep their mips when the card is full`).
- Mudança no tradutor de shaders vem com um teste no estilo dos que já existem em `crates/victory-shader`.
- Se a mudança mexe no que aparece no jogo, diga no PR em que cena testou, a placa e o fps antes e depois.

## Reportar problemas

Use os modelos de issue. Inclua a placa, o driver, as variáveis `VICTORY_*` usadas e o trecho do log
(`victory.log` ou a saída do terminal) em volta do erro.
>>>>>>> vibr/main
