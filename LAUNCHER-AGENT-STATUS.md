# Victory United Launcher agent interface

Branch `launcher/agent`. The implementation and Settings polish are pushed. See
[the agent setup and generated reference](docs/launcher-agent.md) for the 56 tools.

## Verified

- 82 launcher tests, 8 stats tests, 8 updater tests, and both library doctest runs pass on Linux.
- Both Windows workspace compile gates passed. The launcher-only check also passed after the
  Settings polish.
- A loopback server on an OS-selected port answered `tools/list` and `status`. Tests cover bearer
  authentication, Host and Origin checks, JSON-RPC errors and batches, request size, confirmation,
  settings validation, and the generated tool reference.
- The Linux launcher starts headless with no display and exits from the confirmed shutdown tool or
  Ctrl+C. Stdio forwarded to an existing endpoint and started a headless instance when it was alone.
- In the real Linux Settings window, turning agent control on opened a window-owned endpoint and
  showed a recent call. **Turn off** closed it. The setting persisted and stdio refused calls while
  the window was open with agent control off.
- Reviewed dark and light captures in both states at 980 x 660 using Xvfb. The enabled card shows
  the port, token file path, three sample calls and the **Turn off** button.
- A 60-second Linux debug sample after startup measured 0.00% of one core for idle Play with
  agent control off (126.2 MiB RSS) and for the idle headless endpoint (7.6 MiB RSS).
- The repository's three known xtask failures still reproduce in CI configuration, asset policy and
  older documented switches. The agent changes added no new failures.

## Still to verify

- The WinPC release build, native captures and release idle CPU measurements await the serialized
  WinPC job slot. Linux captures and measurements describe the Linux build only.
- The live game and production engine paths remain untested. The brief excludes starting them.
