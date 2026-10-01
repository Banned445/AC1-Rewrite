# Shared brief for RE sub-agents (AC1 movement)

Read `CLAUDE.md` in the project root first.

## Tools
- IDA MCP tools are deferred: load them with ToolSearch, e.g.
  `select:mcp__plugin_ida-pro-mcp_idalib__decompile,mcp__plugin_ida-pro-mcp_idalib__disasm,mcp__plugin_ida-pro-mcp_idalib__rename,mcp__plugin_ida-pro-mcp_idalib__set_comments,mcp__plugin_ida-pro-mcp_idalib__append_comments,mcp__plugin_ida-pro-mcp_idalib__xrefs_to,mcp__plugin_ida-pro-mcp_idalib__callees,mcp__plugin_ida-pro-mcp_idalib__int_convert,mcp__plugin_ida-pro-mcp_idalib__func_query,mcp__plugin_ida-pro-mcp_idalib__lookup_funcs,mcp__plugin_ida-pro-mcp_idalib__get_string,mcp__plugin_ida-pro-mcp_idalib__find_regex,mcp__plugin_ida-pro-mcp_idalib__find_bytes,mcp__plugin_ida-pro-mcp_idalib__set_type,mcp__plugin_ida-pro-mcp_idalib__declare_type,mcp__plugin_ida-pro-mcp_idalib__analyze_function,mcp__plugin_ida-pro-mcp_idalib__get_bytes`
- Database session id: **`ac1`** (already open, shared by several agents — calls are serialized,
  so prefer fewer, bigger calls; decompile with `include_addresses:false` to save tokens).
- Static data helpers (Python, reads the exe directly by VA — fast for tables):
  - Python: `C:\Users\benja\AppData\Roaming\uv\python\cpython-3.11-windows-x86_64-none\python.exe`
  - Set `PYTHONPATH` to `C:\Users\benja\AppData\Local\Temp\claude\C--Users-benja-Desktop-Claude-Game-Reversal-And-Recreation\20943491-d831-49f3-9582-75f9347e730d\scratchpad`
  - `from pe import *` → `u32(va)`, `f32(va)`, `cstr(va)`, `dump(va, n)` (annotates strings/floats).
  - `vt.py Class@scimitar ...` → lists every vftable (per base offset) and slot addresses.
  - Float constants (speeds, angles, distances) referenced by code as `dword_XXXX` / `flt_XXXX`
    can be read with `f32(va)`. Record them — they are recreation gold.

## What is already known (verified)
- Image base 0x400000, 32-bit MSVC C++, full RTTI (IDA named vftables `??_7Class@scimitar@@6B@`).
- Engine namespace `scimitar`. Characters are `Human` (vftable 0x16d9314). Human behaviour is
  split into state modules, each with a matching `*Data` class. CORRECTION: `*Data` classes appear to
  be each module's reflected RUNTIME STATE (DataContext<Human,HumanData,HumanXxxData>), not tuning —
  e.g. HumanGroundData stores the current sub-state enum. Decoded layouts: RE/data/data_classes.txt.
  Enum value names for everything: RE/data/enums_movement.txt. Modules:
  HumanGround(+Data), HumanLedge, HumanClimb(+HumanClimbData), HumanInAir, HumanLadder,
  HumanPole, HumanRope, HumanWalling, HumanNarrowObject(+Beam), HumanRiding, HumanKiosk,
  HumanHayStack, HumanDead, HumanLookAt, HumanUpperBody. Interfaces: IHuman*, e.g.
  IHumanGroundMovement, IHumanMovement, IHumanClimb, IHumanLedge, IHumanInAir.
- All state modules share a **16-slot base vftable at this+0** (slots 1,3,4,5,6 are shared
  stubs 0x116c050/0x116c080/0x116c090/0x116c070/0x116c0a0; slot 0 = scalar deleting dtor).
  Secondary vftables at this+0x14 (module-specific interface, e.g. IHumanClimb), this+0x18
  (27 slots, common listener interface), this+0x1c (2 slots).
- Approx. code ranges: Human 0xB0D000–0xB2F000; HumanGround 0xD7C000–0xDC3700;
  HumanUpperBody ~0xDC3C80–0xDC6C00; HumanLedge 0xDCB000–0xDE45A0; HumanClimb 0xDE4700–0xDFE000;
  HumanInAir 0xDFE000–0xE10700 (+ helpers to ~0xE1C000); HumanLadder 0xE1C000–0xE28500;
  HumanPole 0xE28600–0xE2DF00; HumanRope 0xE2E000–0xE33700; HumanWalling 0xE33700–0xE39D00;
  HumanNarrowObject 0xE4C000–0xE5A000; StPadControlled (player pad controller) 0xD684B0 + 0x746E90..;
  HumanDecision 0xF17000–0xF1D600; CharacterController ~0x578000; GuidanceSystem 0x669F00–0x66B100,
  GuidanceSystemComponent 0x544900–0x546600; MainNavigation 0xC5AC70 / 0x6FBC80.
- ActorStateID enum (reflection table at 0x190DFE8, 12-byte records {char* name; int value; u32 hash}):
  0 NONE, 1 Narrow, 2 Climb, 3 Fight, …, 65 Sprint, 69 Pilotis (values ascend in reverse order of the
  strings at 0x169F6C8..0x169FD90).
- Reflection: class descriptors hold name, hash, size and a list of 32-byte property descriptors;
  property **names are hashed** (not stored), but enum value names are stored as strings
  (e.g. HumanClimbData::EntryType = {Default, FromLedge, FromLedgeParallelJump, FromGround}).
  Where tuning values live (.rdata constants vs. .forge data objects vs. animation data) is an
  open question — record what you find.

## Deliverable rules
- Rename functions you understand (`Class__Method`), rename key locals/args, fix types where
  it clearly helps, add comments (function-level summary + key lines). Only rename functions
  inside your assigned area, plus clearly generic helpers you fully understand (math/vector);
  if a helper is already renamed by someone else, leave it.
- Never: patch bytes, undefine, delete/define functions, close or save the IDB.
- Never convert number bases by hand; use `int_convert` (or Python for floats from the exe).
- Cite addresses for every claim. Label guesses **(hypothesis)**.
- Write your report to the RE/ file you were assigned. Structure it for a re-implementer:
  1. Summary (what the subsystem does, in plain words)
  2. Class/object layout (field offsets you identified, with meaning)
  3. States / sub-states and transitions (table: from → to, condition, source address)
  4. Per-frame update logic (pseudo-code, engine-agnostic)
  5. Constants (address, value, meaning) and which values come from *Data (forge)
  6. Interfaces to other subsystems (who calls in / what it calls out)
  7. Open questions + suggested dynamic checks
  8. Renamed functions table (address → new name)
- Keep a short methodology section (steps you took).
- Finish with a concise summary message (<300 words) of the most important findings.
