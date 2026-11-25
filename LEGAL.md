# Legal notice and disclaimer

Please read this before using, forking or contributing to this repository.

## What this project is

Victory United is independent, community-written software. It is a compatibility layer: it lets a
Windows PC run a program that was built for another platform, the same way an emulator or Wine does.
All of the code here was written by the project's contributors (see [CREDITS in the README](README.md#credits)
and [PROVENANCE.md](PROVENANCE.md)).

## What this project is not

- **Not affiliated with, endorsed by, sponsored by or connected to** Rockstar Games, Take-Two
  Interactive, Microsoft, Xbox, or any other game developer, publisher or platform holder.
- **Not a way to get the game.** It contains no game, and it does not include, link to, or help you
  find one.
- All names, logos and trademarks (for example "Grand Theft Auto", "GTA", "Rockstar Games",
  "Xbox", "DirectX", "Windows") belong to their owners. They are used here only to describe what the
  software works with (nominative use), never as branding for this project.

## Hard rules for everything in this repository

1. **No game material, ever.** No game files, executables, code, textures, models, audio, video,
   text, shaders, tables, screenshots, recordings or anything extracted or derived from them. This is
   enforced by `cargo xtask policy` and CI. See [docs/asset-policy.md](docs/asset-policy.md).
2. **No leaked or stolen material.** No source code, assets, documents, internal tools, keys or
   unreleased builds that came from a leak, a hack or a breach (including the 2022 Rockstar leak). If
   you have seen such material, do not use it as a reference for anything you contribute. Anything
   suspected to come from such material is removed on sight, history rewritten if needed.
3. **No circumvention.** No tools, keys, instructions or links that break copy protection,
   encryption, licensing or console security, and no help dumping or decrypting a game. You supply
   your own legally obtained copy, already in a form this software can read. Questions that involve
   piracy, cracked copies or stolen keys are closed without an answer.
4. **No third-party code under an incompatible licence.** Code from another project is only taken
   if its licence allows it (GPL-3.0-compatible), with credit kept. Code from projects with no
   licence, or a "reference only" licence, is read for ideas at most, never copied.
5. **No online, anti-cheat or account features.** This project does not touch game servers,
   accounts, online services or anti-cheat, and it will not accept changes that do.

## Why a project like this is lawful (short version, not legal advice)

Writing software that is compatible with another platform, using only your own legally obtained
copy and your own original code, is generally recognised as lawful. Examples are the US court
decisions on interoperability (Sega v. Accolade, Sony v. Connectix, Google v. Oracle), the US DMCA
exemption for interoperability (17 U.S.C. 1201(f)) and the EU Software Directive (2009/24/EC, Art. 6).
That protection depends on the rules above, which is why they are strict. **This is a description of
the project's intent, not legal advice, and the law differs by country.** Use at your own risk and
check your local law.

## No warranty

The software is provided "as is" under the GPL-3.0 licence in [LICENSE](LICENSE), without warranty of
any kind. Running it may crash your system or lose data. The maintainers are not responsible for any
damage, or for how you obtained the game you run with it.

## One known third-party table

`crates/victory-formats/src/bcpack-tables.bin` (about 78 KB) holds constant tables of the BCPack
texture format, which is a Microsoft format and not Rockstar's. It was inherited from upstream VIctory
and is not covered by this repository's licence (see [docs/asset-policy.md](docs/asset-policy.md)).
It is the only such file. If its owner objects, it will be removed (see below).

## Takedown requests and rights holders

If you are a rights holder and believe something here infringes your rights, please
[open an issue](https://github.com/Fayberr/Victory-United/issues/new?template=rights-holder.yml)
or contact the repository owner through the GitHub profile
[@Fayberr](https://github.com/Fayberr), naming the file or commit and what the concern is.

- We respond in good faith, **and remove the named material promptly** while the question is
  looked at. Genuine mistakes are fixed first and discussed after.
- The project's rule 1 means a valid concern should be about a few specific files, which are easy to
  remove. If something got in that should not have, we rewrite the history, not just add a
  follow-up commit.
- Contributors keep their own responsibility for what they submit. By opening a pull request you
  confirm the checklist in [CONTRIBUTING.md](CONTRIBUTING.md).

## Reporting a security problem

Use GitHub's private "Report a vulnerability" button on the Security tab, not a public issue.
