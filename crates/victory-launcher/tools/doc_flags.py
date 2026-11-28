#!/usr/bin/env python3
"""Writes the hand-made documentation of the important flags into data/flags.toml.

Run after gen_flags.py:   python3 crates/victory-launcher/tools/doc_flags.py   (from the repository root)
The generator only guesses types; the entries below are what the code and scripts/play.ps1 say.
Fields: group (graphics, memory, threads, debug, paths) puts a flag on the Settings tab.
"""
import re, pathlib

OUT = pathlib.Path("crates/victory-launcher/data/flags.toml")

# name: (group, type, default, off, risk, extra, doc)
# off: "0" writes 0, "remove" takes the variable out. extra: dict of min/max/choices/presence.
S = "switch"; N = "number"; T = "text"; C = "choice"
DOCS = {
 # ---- graphics
 "FULLSCREEN": ("graphics", S, "", "remove", "safe", {}, "Fill the whole screen with no frame, as a console does. Off gives a normal window."),
 "PANEL": ("graphics", S, "", "remove", "safe", {}, "Show frame rate, frame times, GPU, CPU, video memory and RAM in the upper left corner. F3 hides it while playing."),
 "BVH_BUILDER": ("graphics", S, "", "remove", "advanced", {}, "Run the console's ray-tracing structure builder so the game's own ray-traced light works. Lit interiors, about a third slower."),
 "FSR_4K": ("graphics", C, "", "remove", "safe", {"choices": ["quality", "balanced", "performance"]}, "Upscale to 3840 by 2160 with AMD FidelityFX FSR. Needs amd_fidelityfx_upscaler_dx12.dll (see VICTORY_FIDELITYFX_DIR)."),
 "AMBIENT": ("graphics", T, "constant:0.1", "remove", "advanced", {}, "Stand-in for the indirect light the game traces rays for. A number is a share of the estimate (1 is all of it), constant:<level> is a fixed level, 0 is the game's own output."),
 "AMBIENT_AIR": ("graphics", N, "0.8", "remove", "advanced", {"min": 0, "max": 8}, "Upper limit for the light the air scatters, as a multiple of the light around. Stops a brown veil indoors without ray tracing. 0 keeps the game's own value."),
 "SHOW_AFTER": ("graphics", S, "1", "remove", "experimental", {}, "Wait until a texture's background upload has finished before a draw samples it, so a gray stand-in is not baked into the picture."),
 # ---- memory
 "CARD_BUFFERS_MB": ("memory", N, "", "remove", "advanced", {"min": 64, "max": 16384}, "Most video memory, in MB, used for copies of the game's buffers on the card. Unset: worked out from the card's budget."),
 "CARD_WINDOWS_MB": ("memory", N, "1280", "remove", "advanced", {"min": 0, "max": 8192}, "Most video memory, in MB, for copies of windows outside buffers that go to the card's memory. 0 turns them off."),
 "CARD_BUFFERS_MIN_KB": ("memory", N, "64", "remove", "advanced", {"min": 1, "max": 4096}, "Smallest buffer, in KB, that gets a copy in video memory. 64 includes every buffer the game uses."),
 "CARD_RESERVE_BUDGETS": ("memory", S, "", "remove", "advanced", {}, "Set the card buffer budgets aside first, so textures cannot take the memory they need."),
 "TEXTURE_ROOM_PERCENT": ("memory", N, "90", "remove", "advanced", {"min": 50, "max": 97}, "Share of the card's memory budget, in percent, that textures may fill. Lower it on 12 GB cards if the frame rate collapses when memory is full."),
 # ---- threads
 "THIN_THREADS": ("threads", N, "8", "remove", "advanced", {"min": 1, "max": 32}, "Threads that translate the game's command lists. More helps on CPUs with many cores."),
 "UNDER_THREADS": ("threads", N, "6", "remove", "advanced", {"min": 1, "max": 32}, "Threads that compare memory pages for changes the game made under the engine's feet."),
 "LAND_THREADS": ("threads", N, "", "remove", "advanced", {"min": 1, "max": 16}, "Threads that copy what shaders wrote back into the game's memory. Unset: a quarter of the logical processors, 1 to 4."),
 "MAP_THREADS": ("threads", T, "", "remove", "advanced", {}, "Threads that map pages while the game streams. A number, or auto. The newer default of 8 is replaced by 1 or auto in the presets."),
 "PIPELINE_WORKERS": ("threads", N, "", "remove", "advanced", {"min": 1, "max": 64}, "Workers that build graphics pipelines in the background. Unset: two thirds of the logical processors, 2 to 16."),
 "RELAY": ("threads", N, "2", "remove", "advanced", {"min": 0, "max": 2}, "Copy back what shaders wrote on a thread of its own instead of the submitting thread. 1 or 2 turns it on."),
 "WIDE_BACK": ("threads", S, "1", "remove", "advanced", {}, "Snapshot and compare shader-written ranges on several threads."),
 # ---- debug
 "LOG": ("debug", T, "", "remove", "safe", {}, "Log filter, for example victory_loader=trace,info. Debug and trace output exist only in diagnostics builds."),
 "D3D12_DRED": ("debug", C, "light", "remove", "safe", {"choices": ["light", "full"]}, "Keep a record of who owned which video memory, so a lost device can be explained."),
 "PROFILE": ("debug", T, "", "remove", "advanced", {}, "Sampling profiler: <from>,<to>,<ms>,<file>. Needs the engine's pdb file."),
 "WARM_SECONDS": ("debug", N, "", "remove", "safe", {"min": 1, "max": 86400}, "Stop translating shaders after this many seconds. What is done stays; the next warm-up goes on from there."),
 "SHADER_FRESH": ("debug", S, "", "remove", "advanced", {}, "Translate shaders again that an earlier translator build cached, and keep the new ones."),
 "PIPELINE_BOOK": ("debug", S, "", "remove", "advanced", {}, "Write down every pipeline the game creates so the warm command can build them ahead of a run."),
 # ---- performance helpers the presets set (shown in the Flags tab, not on Settings)
 "TEXTURE_NO_WAIT": (None, S, "", "remove", "experimental", {}, "Do not wait when a texture refresh finds the game remapping its memory."),
 "CARD_REHASH_RUNS": (None, S, "", "remove", "experimental", {}, "Hash the pieces sent to the card's copy in runs of touching pieces, each once."),
 "JOIN_COPIES": (None, S, "", "remove", "experimental", {}, "Join a buffer copy with the one recorded before it into a single step."),
 "INDIRECT_WINDOW": (None, S, "", "remove", "experimental", {}, "Clamp indirect draws that read record after record of one argument buffer a window at a time."),
 "INDIRECT_BATCH": (None, N, "2", "remove", "experimental", {"min": 0, "max": 2}, "Clamp indirect executions in batches. 0 does each on its own."),
 "RELAY_COPY": (None, S, "", "remove", "experimental", {}, "Fetch the relay's pieces over the card's copy queue instead of the drawing queue."),
 "CARD_STEPS": (None, N, "2", "remove", "experimental", {"min": 0, "max": 2}, "Do steps that read the game's memory on the card instead of the CPU."),
 "TITLE_PRIORITIES": (None, S, "", "remove", "advanced", {}, "Keep the game's own thread priorities except the lowest two, which become below normal. 0 keeps them all."),
 "CARD_AWAKE": (None, S, "", "remove", "advanced", {}, "Keep the card's clocks up between submissions. Helps on AMD cards whose driver lowers clocks when the card idles."),
 "PORT_CONSTANTS": (None, S, "", "remove", "experimental", {}, "Keep the game's 32-bit graphics heap as the card's own memory."),
 "THIN": (None, S, "", "remove", "experimental", {}, "Record the once-a-frame passes over the heap's views into a list that runs ahead of the first submission."),
 "JOIN_PARTS": (None, N, "8", "remove", "experimental", {"min": 1, "max": 64}, "How many parts of a submission are joined and refreshed together."),
 "UPLOAD_AHEAD": (None, S, "", "remove", "experimental", {}, "Upload textures in the background on the copy queue ahead of use."),
 "UNDER_WIDE": (None, S, "1", "0", "experimental", {}, "Compare memory pages on several threads. 0 uses the submitting thread only (safe mode)."),
 "LAND_ECHO": (None, S, "1", "0", "experimental", {}, "Skip sending up what a landing wrote itself. 0 copies and sends it as before (safe mode)."),
 "PLACED_ORDER": (None, S, "", "remove", "experimental", {}, "Fill the constant-buffer ring in list order. Used on NVIDIA cards."),
 "WAVE_WIDE_ONLY": (None, T, "", "remove", "experimental", {}, "Hashes of wave64 compute programs allowed to run in a wave-wide form on 32-lane cards."),
 # ---- paths (set by the launcher itself)
 "CONFIG": ("paths", T, "", "remove", "safe", {}, "Config file the engine reads instead of victory.toml. The launcher does not set it."),
 "GAME_DIR": ("paths", T, "", "remove", "safe", {}, "The game folder. The launcher sets it from your choice."),
 "CACHE_DIR": ("paths", T, "", "remove", "safe", {}, "Where shaders and pipelines are cached. The launcher sets one folder per game."),
 "DATA_DIR": ("paths", T, "", "remove", "safe", {}, "Where the game's saves and storage live. The launcher sets it."),
 "FIDELITYFX_DIR": ("paths", T, "", "remove", "safe", {}, "Folder with amd_fidelityfx_upscaler_dx12.dll, needed for FSR."),
 # ---- flags other workers add: documented ahead of the merge
 "STATS": ("debug", S, "1", "0", "safe", {}, "Publish live statistics for the launcher's monitor. 0 turns the block off."),
 "DLSS": ("graphics", C, "", "remove", "advanced", {"choices": ["quality", "balanced", "performance", "ultra-performance"]}, "NVIDIA DLSS super resolution mode. Only in engine builds that include DLSS."),
 "DLSS_DIR": ("paths", T, "", "remove", "safe", {}, "Folder with the DLSS files from NVIDIA. The launcher does not include them."),
 "FRAMEGEN": ("graphics", S, "", "remove", "experimental", {}, "DLSS frame generation. Only in engine builds that include it; RTX 40 natively."),
}

def esc(s):
    return s.replace("\\", "\\\\").replace('"', '\\"')

def entry(n, spec):
    group, typ, default, off, risk, extra, doc = spec
    lines = ["[[flag]]", f'name = "VICTORY_{n}"', f'type = "{typ}"', f'default = "{esc(default)}"']
    if typ == S:
        lines.append(f'off = "{off}"')
    if group:
        lines.append(f'group = "{group}"')
    if "min" in extra:
        lines.append(f'min = {extra["min"]}')
    if "max" in extra:
        lines.append(f'max = {extra["max"]}')
    if "choices" in extra:
        lines.append("choices = [" + ", ".join(f'"{c}"' for c in extra["choices"]) + "]")
    lines += [f'risk = "{risk}"', f'doc = "{esc(doc)}"', ""]
    return lines

def main():
    text = OUT.read_text(encoding="utf-8")
    head, _, body = text.partition("[[flag]]")
    blocks = ("[[flag]]" + body).split("[[flag]]")[1:]
    out_blocks = {}
    for b in blocks:
        m = re.search(r'name = "(VICTORY_[A-Z0-9_]+)"', b)
        out_blocks[m.group(1)] = "[[flag]]" + b.rstrip() + "\n"
    n_doc = 0
    for n, spec in DOCS.items():
        if spec is None:
            continue
        out_blocks["VICTORY_" + n] = "\n".join(entry(n, spec))
        n_doc += 1
    OUT.write_text(head + "\n".join(out_blocks[k].rstrip() + "\n" for k in sorted(out_blocks)), encoding="utf-8")
    print(f"{n_doc} documented, {len(out_blocks)} entries")

main()
