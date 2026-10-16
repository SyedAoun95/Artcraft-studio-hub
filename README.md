# Studio Hub (working name)

One desktop app for the seven Craft apps, with one MCP server that AI tools (Claude Code, Codex,
Claude Desktop, Cursor, …) use to control them. The apps themselves are not modified: each still runs
as its own program and window, with all of its features.

```
Studio Hub window ── opens apps with their agent channel, agent ON/OFF, per-app switches, activity log
studio-hub mcp    ── the MCP gateway AI tools run; forwards each call to the app's own MCP server
```

## Use

```sh
cargo build --release
./target/release/studio-hub                 # the hub window
./target/release/studio-hub status          # JSON status of every app
./target/release/studio-hub open lightcraft # open one app with its agent channel
./target/release/studio-hub mcp             # MCP server on stdin/stdout (what AI tools start)
./target/release/studio-hub mcp --full      # also list every app tool as <app>__<tool> (~800 tools)
```

### Build the installer (.dmg)

```sh
# 1. each app's official packaging script builds and signs <Name>.app
for a in photocraft vectorcraft lightcraft designcraft filmcraft effectcraft pdfcraft; do
  ../$a/packaging/macos/package.sh --arch x86_64      # add --skip-build to reuse existing binaries
done
# 2. one .dmg with Studio Hub.app (carrying the seven CLIs) + the seven apps + licences
scripts/make-dmg.sh --arch x86_64                    # -> dist/Studio-Suite-<version>-macos-x86_64.dmg
```

Installed (from the .dmg), Studio Hub finds the apps next to itself, in /Applications or ~/Applications,
opens them with `open` (so they behave like normal Mac apps) and can bring an open app to the front (**Show**).
`scripts/make-app.sh` still builds a development-only `Studio Hub.app` that uses the apps in this folder.

In the window, **Connect** registers the gateway as `studio-hub` with Claude Code (user scope),
Codex (`~/.codex/config.toml`) or Claude Desktop. Each config file is backed up first
(`*.studio-hub-backup`). **Copy config** gives the JSON for any other MCP client.

## Agent tools (compact mode, the default)

| Tool | What it does |
|---|---|
| `suite_status` | apps installed/open, agent channel ready, agents allowed, work folder |
| `suite_launch_app` | open an app with its agent channel and wait until it is ready |
| `suite_list_tools` | an app's own tools, optionally filtered by a query |
| `suite_describe_tool` | one tool's description and input schema |
| `suite_call_tool` | call an app tool; the app's result (text, JSON, images) is returned unchanged |

Compact mode exists because the apps expose ~800 tools together (LightCraft alone lists 502 while open),
more than most AI tools handle well in one list.

## Settings and files

`~/Library/Application Support/Studio Hub/`: `config.json` (apps folder, work folder, token folder,
agent switches), `activity.jsonl` (every agent call), `logs/<app>.log` (app output when opened from the hub).
The hub and the gateway read the same settings, so switching agents off applies to the next call.

## Phase 1 limits (see the roadmap below)

- Five apps (FilmCraft, VectorCraft, EffectCraft, LightCraft, DesignCraft) have no password on their control
  port; the hub's switches stop calls that go through the gateway, not other local programs.
- PdfCraft's agent works on files headlessly; it does not drive PdfCraft's open window.
- Notifications and resources from the apps' MCP servers are not forwarded yet.
- Runs from this development layout (apps built next to this folder); no installer yet.

## Roadmap

1. **Phase 1 (this):** hub window, launcher, MCP gateway, connect buttons, agent switches, activity log.
2. **Phase 2:** per-launch tokens for every app (needs small upstream changes), approval prompts for
   destructive actions, crash recovery, forwarding of notifications/resources, closing apps safely.
3. **Phase 3:** a self-contained, signed and notarized macOS app (universal), then Windows and Linux,
   with auto-update; own name and icon (the ArtCraft name and logos are trademarks of the ArtCraft Team).

All seven apps are MIT OR Apache-2.0; a distributed build must include their LICENSE and NOTICE files.
