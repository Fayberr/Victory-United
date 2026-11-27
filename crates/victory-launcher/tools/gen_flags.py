#!/usr/bin/env python3
"""Builds or updates crates/victory-launcher/data/flags.toml from the source tree.

Usage: python3 crates/victory-launcher/tools/gen_flags.py   (run from the repository root)

Every VICTORY_* name that appears in crates/ or apps/ gets an entry. Entries that already exist are kept
as they are (documentation written by hand is never overwritten); new names are added with a guessed type
and an empty `doc`. The Rust test `flag_registry_covers_the_code` fails when a name is missing.
"""
import re, sys, pathlib, collections

ROOT = pathlib.Path(".")
OUT = ROOT / "crates/victory-launcher/data/flags.toml"
NAME = re.compile(r'VICTORY_[A-Z0-9_]+')

def sources():
    for base in ("crates", "apps"):
        for p in (ROOT / base).rglob("*.rs"):
            if "target" in p.parts or "victory-launcher" in p.parts:
                continue
            yield p

def guess(name, texts):
    """Guess the kind from how the code reads the variable."""
    kinds = collections.Counter()
    for t in texts:
        for m in re.finditer(re.escape(name) + r'"?\)?([^;{}]{0,120})', t):
            tail = m.group(1)
            if re.search(r'is_none_or\(\|\w+\| \w+ != "0"\)', tail):
                kinds["switch_on"] += 1
            elif re.search(r'is_some_and\(\|\w+\| \w+ == "1"\)', tail):
                kinds["switch_off"] += 1
            elif re.search(r'is_some_and\(\|\w+\| \w+ != "0"\)', tail):
                kinds["switch_off_presence"] += 1
            elif re.search(r'\.is_some\(\)|\.is_ok\(\)', tail):
                kinds["presence"] += 1
            elif re.search(r'parse::<(usize|u8|u16|u32|u64|i32|i64|isize)>', tail):
                kinds["int"] += 1
            elif re.search(r'parse::<f(32|64)>', tail):
                kinds["float"] += 1
    if not kinds:
        return "text", ""
    kind = kinds.most_common(1)[0][0]
    return {
        "switch_on": ("switch", "default-on"),
        "switch_off": ("switch", "off-by-default"),
        "switch_off_presence": ("switch", "off-by-default"),
        "presence": ("switch", "presence"),
        "int": ("number", ""),
        "float": ("number", ""),
    }[kind]

def main():
    texts = collections.defaultdict(list)
    names = set()
    for p in sources():
        t = p.read_text(encoding="utf-8", errors="replace")
        for n in set(NAME.findall(t)):
            names.add(n)
            texts[n].append(t)
    existing = {}
    if OUT.exists():
        cur = None
        for line in OUT.read_text(encoding="utf-8").splitlines():
            m = re.match(r'name = "(VICTORY_[A-Z0-9_]+)"', line)
            if m:
                cur = m.group(1)
                existing[cur] = []
            if cur:
                existing[cur].append(line)
            if line.strip() == "" and cur:
                cur = None
    out = ["# Registry of every VICTORY_* variable the code reads. Generated first by tools/gen_flags.py, then",
           "# documented by hand: entries are never overwritten by the script. `doc = \"\"` means not documented yet.",
           "# type: switch | number | text | choice.  risk: safe | advanced | experimental.",
           "# off: how a switch is turned off. `\"0\"` writes 0 (the engine treats presence as on only for names",
           "# marked `presence`), `remove` takes the variable out of the environment.", ""]
    added = 0
    for n in sorted(names | set(existing)):
        if n in existing:
            body = existing[n] if existing[n][-1] == "" else existing[n] + [""]
            out.extend(["[[flag]]"] + body)
            continue
        typ, how = guess(n, texts[n])
        out.append("[[flag]]")
        out.append(f'name = "{n}"')
        out.append(f'type = "{typ}"')
        if typ == "switch":
            if how == "default-on":
                out.append('default = "1"')
                out.append('off = "0"')
            elif how == "off-by-default":
                out.append('default = ""')
                out.append('off = "remove"')
            else:
                out.append('default = ""')
                out.append('off = "remove"')
                out.append('presence = true')
        else:
            out.append('default = ""')
        out.append('risk = "advanced"')
        out.append('doc = ""')
        out.append("")
        added += 1
    OUT.write_text("\n".join(out), encoding="utf-8")
    print(f"{len(names)} names in the code, {added} added, {len(existing)} kept")

main()
