# Handoff for cloud agents

You are continuing a private, personal, educational project. It reverse engineers Assassin's Creed (2008) movement and
recreates it 1:1 in Rust/Bevy. Read `CLAUDE.md` (conventions), `README.txt` (educational use, no distribution) and
`RE/README.md` (the report index) first.

**The goal is an exact copy of the game.** Nothing invented: every behaviour must trace to the exe or the game data. Mark
anything unverified as **(hypothesis)**; mark port-only choices `PORT:`.

## What is NOT in this repo (and why)
| Missing | Why | Consequence |
|---|---|---|
| `bin/` (game exe + IDA database) | copyrighted program, kept on the owner's PC | you cannot decompile new functions here; work from the RE reports and the data dumps, and list any new exe addresses you need in your notes for a local session |
| The game install (`.forge` archives) | copyrighted assets; the port loads them at runtime from the owner's install | the install tests skip; the port runs without the model (capsule) and without animations. No screenshots of Altaïr can be made here |
| `port/target/` | build output | first build compiles Bevy (several minutes) |

## What you can do here
- **Logic and specs:** port behaviour already specified in `RE/*.md`. State machines, rules, timings, constants and the
  climb/ledge tables are all in `RE/03`–`RE/06`, `RE/11`, `RE/13`, and the `RE/data/*` dumps (decoded animation graph,
  climb/ledge table → action resolution, contact tracks, enums, reflection layouts).
- **Headless tests:** `cd port && cargo test` runs the simulation tests (`src/sim_tests.rs`) without any game data. Add a
  sim test for every behaviour you port.
- **Python tools** (`RE/tools/*.py`):
  - the graph tools that need the exe or the forge files will not run here;
  - the pure-data scripts and the `RE/data` dumps (`action_graph_movement.txt`, `climb_move_actions.txt`,
    `ledge_table_actions.txt`) are usable.
- **What remains:** `RE/12_remaining_movement_checklist.md`, with a suggested order at the bottom. Tick items
  (`[x]` / `[~]`) with a pointer to where they were done.

## Building on Linux
Bevy needs system packages: `sudo apt-get install -y pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.
Then `cd port && cargo test`.

## Rules (from CLAUDE.md and the owner)
- **Data:** never add game files, decompiled code dumps of the exe, or extracted assets to the repo.
- **This repo:** it is private; never make it public or share it.
- **Reports:** cite the function address or data file for every claim in `RE/*.md`; never convert number bases by hand.
- **Verification:** anything that needs the real model, animations or IDA goes in a "needs local verification" list in
  your summary, so the owner can run it on their PC. The port has `AC_SHOTS` multi-angle capture, `AC_IK_LOG` and
  `AC_ANIM_LOG` for that; see `port/README.md`.
