# Assassin's Creed (2008) movement — reverse engineering & recreation

Educational, personal-use project (see README.txt). Nothing from the game is redistributed.

## Goal
Reverse engineer Altaïr's movement (ground locomotion, jumping/falling, ledges, climbing,
beams, poles, ladders, ropes, wall-running/"walling", free-run, the world "guidance" data
that marks climbable geometry, player input → movement) well enough to **recreate it in a
new engine (likely Rust + Bevy/wgpu, iw4L-style: clean reimplementation that loads assets
from the user's own install at runtime)**. Findings must be engine-agnostic specs:
states, transitions, conditions, timings, speeds, detection rules, data layouts.

## Layout
- `bin/AssassinsCreed_Dx9.exe` — working copy (Steam v1.02 build 86610). Not packed.
- `bin/AssassinsCreed_Dx9.exe.i64` — IDA 9.3 database. `bin/backup/` — pristine backup.
- Game install (read-only, never modify): `C:\Users\benja\Desktop\Claude\Assassin's Creed\`
- `RE/*.md` — findings. `RE/README.md` is the index.

## Conventions
- IDA MCP headless session id: `ac1`.
- Function names: `Class__Method` (e.g. `HumanClimb__UpdateGrid`). Unknown-but-scoped:
  `HumanClimb__sub_DE4F80`. Keep `sub_` suffix only when purpose is unclear.
- Never convert number bases by hand — use `int_convert`.
- Every claim in RE docs cites the function address it came from. Mark guesses as
  **(hypothesis)**; mark verified facts plainly.
- Safe edits only: rename, comment, set_type, declare_type. No patching, undefining,
  deleting functions, or closing/saving the IDB from sub-agents.
