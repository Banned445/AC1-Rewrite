# 00 — Overview: how Assassin's Creed (2008) movement works, and how to rebuild it

Target: `AssassinsCreed_Dx9.exe` v1.02 (b86610), Scimitar engine, 32-bit MSVC with full RTTI.
Static analysis only, in IDA 9.3 plus Python readers over the exe; no runtime verification yet.
Every claim below is backed by an address in the subsystem report it links to.
**(hypothesis)** marks unverified interpretations.

---

## 1. The one-paragraph answer

Altaïr's movement is a **set of mutually exclusive "locomotion contexts"** (Ground, InAir, Ledge,
Climb, Walling, NarrowObject/Beam, Pole, Ladder, Rope, …). Exactly one is active at a time. Each
context is an **animation-driven state machine**: code picks animation clips, blend weights and
targets, and the **animation's root motion moves the character**. Code mostly turns the character,
snaps it onto targets and checks rules. The world tells the character where it can climb through
**hand-authored "guidance" edges baked into the level data** (tagged LedgeGrab, Beam, Pole, Ladder,
Rope, …). Contexts query those edges with small 3-D zones (boxes, spheres, capsules) around the body.
Jumps are **not physics**: the starting context picks a landing target first, and the jump animation
is warped so it ends exactly on it. Real gravity is only used for long drops.

## 2. Architecture

```
  Pad (16 buttons + 2 sticks)  ──►  GoAssassinActionInterpreter  (player "brain", 0xED4000–0xEF0000)
        DefaultBindings.map             • dead-zone 0.35, speed = (|stick|-0.35)/0.65
                                        • High profile = RB/right mouse, Legs = A (0.3 s jump buffer)
                                        • AssassinAbilitySet gates actions + MaxSpeed
                                        • picks jump targets (0xE96BF0) from guidance queries
                                              │ AIActor::GetInterface(id) → IHumanGround / IHumanInAir / …
                                              ▼
  Human : AIActor ── 5 extension slots ───────────────────────────────────────────────
     slot 1 LOCOMOTION (one active):  Ground(4) InAir(8) Ledge(9) Climb(10) Walling(11)
                                      NarrowObject(12) Ladder(5) Pole(6) Rope(7) HayStack(21) …
     slots 2–4 always-on layers:      Vocalization, LookAt, UpperBody
     HumanData+0x30 → HumanDataBundle: every context's runtime *Data block (reflected)
        │  switch = AIActor::SwitchLocomotionContext(id, cb) — immediate:
        │  old.Exit → cb applies TransitionSetupDataToX to dest *Data → new.Enter
        ▼
  Context Update(pass)  → inner FSM (byte state ids) → choose clips / blend / IK / warp targets
        │                                    ▲
        ▼                                    │ GuidanceZone::Query(shape, subtypeMask)
  Animation graph (root motion)        Guidance edges (baked in .forge, 12-byte packed edges)
        ▼
  CharacterController::Integrate (Havok-style kinematic proxy: velocity in, ≤10 sweep/slide iterations)
```
Details: [01](01_human_core_and_input.md) (framework, input, controller), [06](06_guidance_world_detection.md) (guidance).

## 3. Key facts per subsystem

### Framework and input — [01](01_human_core_and_input.md)
- Contexts are `ActorContext`s with a 16-slot interface: Bind, Enter, Exit, Update(pass 0/1),
  InitSubStates, QueryInterface, GetData.
- Context IDs come from the `ActorContextID` enum, and contexts are created lazily the first time
  they're used.
- **Transitions are immediate function calls with a setup object**; there's no request queue. A
  context that was just switched in skips its first update.
- **Input:**
  - Walk-to-run is analogue (stick magnitude above a 0.35 dead-zone).
  - High profile is a held modifier.
  - Sprint = high profile + Legs held.
  - Turning more than 45° away from facing slows the character.
- **Jump targets** come from guidance candidates, scored by a 45° cone, a minimum height
  (dz > −3 m) and preference rules ("highest in front", "nearest ledge", "beam beats ledge unless
  2.5 m higher").
- **Jump distance bands per target type** (`0xB1EC40`):

  | Target | Max up | Distance bands |
  |---|---|---|
  | Ground | 1.3 m | 2.5 / 5 / 7 m |
  | Ladder, pole | 2.5 m | 2.5 / 5.5 / 7.5 m |
  | Ledges | 3 m | 2.5 / 6 / 8 m |

### Ground locomotion — [02](02_ground_locomotion.md)
- An 87-state hierarchical state machine. Translation comes from root motion; **rotation is done in
  code** at up to 360°/s for the player (NPCs turn at 1.5–4 rad/s).
- **Speed:** one 0..1 speed parameter split into four bands: Walk ≤ 0.25 < Jog ≤ 0.5 < Run ≤ 0.75 < Sprint.
  - target = base + 0.25 × stick, where base = 0 (low profile), 0.5 (high) or 0.75 (sprint);
  - it rises at 1.0 per second and falls along a deceleration curve.
- **Starts, stops, U-turns (more than 90°) and turn-in-place** are separate clips. Transitions wait
  for each clip's exit flags.
- **Falling:** a ground-probe failure switches to InAir. The fall type is chosen by fall height
  (1, 2 or 8 m) and horizontal speed (2.5 m/s).

### Jumps, falls, landing — [04](04_air_jump_fall.md)
- The jump animation's root motion gets a linear correction `(target − animEnd) · t/dur`, so it lands
  exactly on target. Arrival within 0.01 m hands off to the context matching the target type.
- **Free-fall tail** (target more than 5 m below; 3 m for Leap of Faith): gravity is 9.8 m/s² at a
  fixed dt of 1/30, and horizontal steering is capped at 15 m/s.
- **Landing damage:** heavy above 6.3 m, fatal above 7.0 m (by default). Drops over 3 m play a roll
  and camera shake.
- **Catching ledges mid-air:** needs the grab input. The hand box is 0.4 × 0.3 m, with edges within
  70°; the reach point is +1.4 m for ledges, +1.95 m for walls.

### Climbing and ledges — [03](03_ledge_and_climb.md)
- **Climb is a hold grid:** 0.75 m columns × 0.6 m rows, probed against guidance edges.
  - Hands always sit 2 rows above their foot cell.
  - The stick is quantised into 10 directions (45° sectors).
  - **Static move tables** (dumped from the exe) map pose × direction → next pose, cell offsets and
    animation. A strong push uses the long table (1.2 m hand-over-hand moves).
  - The root is interpolated over the clip, with IK placing the 4 limbs on the holds.
  - There's no stamina.
- **Climb exits, in priority order:** ladder, ledge grab or reach, side jump, other surface, back eject.
- **Ledge:** 20 internal states mapped from `LedgeSubState` (entry, shimmy, corner turn, pull-up,
  pull-down, hand-pass-over, parallel jump, …). Per-state detail is being added to report 03.

### Beams, wall-runs, poles, ladders — [05](05_beams_walling_poles_ladders.md)
- **Walling (wall-run):** EntryA → EntryB → Vertical or Horizontal → End or Rebound.
  - Ledge probes in height bands scaled by character height (pull-up up to 1·h, hang up to about 3·h).
  - A rebound jump goes within ±89° of the wall normal; defaults are 3.5 m down / 5.6 m out.
- **Beams:** segments found by a guidance query (±2 m × ±0.5 m box, 60° cone).
  - Root motion is projected onto the beam line and pulled back toward it at 2·dt per frame.
  - Walking stops 0.3 m before the end, and stepping off needs the end within 0.16 m.
- **Poles:** guidance polylines, Light/Med/High inclination split at cos 20° and cos 60°, attached
  0.2 m off the pole.
- **Ladders and ropes:** state machine and data only (lightly analysed).

### World markup ("guidance") — [06](06_guidance_world_detection.md)
- Every static collision object carries a `GuidanceSystem` with:
  - a quantised vertex pool (`u16 · 0.005 − 163.84` m);
  - 12-byte edges: enabled bit, 5-bit subtype, two 13-bit vertex indices, and two packed normals
    (the faces meeting at the edge);
  - an edge filter (cos 45°, 22.5° corners);
  - a baked spatial tree.
- **The edge-extraction tool is not in the exe**, so it can't be copied from the game.
- Only lying capsules and barrels generate their edges at runtime.
- Queries return 96-byte contacts (p0, p1, n0, n1, entity, subtype). The post-filters reject edges
  sloped more than 40°, and contacts are linked into chains for hand placement.

### Data model — [07](07_reflection_enums_and_data_layouts.md)
- The engine's reflection tables survive in the exe:
  - **551 enums with value names**, including every movement sub-state list;
  - class layouts with typed fields.
- Property names are stored as **standard CRC-32**; **146 of 270** movement field names were
  recovered by dictionary search.
- All `Human*Data` objects are per-character **runtime state**, not tuning. Most tuning is hard-coded
  constants (listed in each report) or lives in animation data.

## 4. What this means for the recreation (Bevy plan)

| Need | Source | Status |
|---|---|---|
| State machines (contexts, sub-states, transitions) | Reports 01–05 + enums (07) | **Specified**. Mirror `ActorContextID` and the sub-state enums as Rust enums |
| Speeds, turn rates, thresholds, jump bands, fall damage | Hard-coded constants in reports | **Specified** (some need runtime confirmation) |
| Climb grid and move tables | 03 §4.3–4.4 (dumped tables) | **Specified**. Port the tables as data |
| Climbable/beam/pole/ladder edges | `.forge` GuidanceSystem blobs (06 §2) | **Format known**. Needs a `.forge` container reader; geometry fallback possible but loses subtypes |
| Root-motion animation clips (walk/run cycles, climb moves, jumps) | `.forge` animation data | **Not started**. Biggest remaining unknown (formats, `COMPRESSION_QUAT16/24`) |
| Collision geometry | `.forge` (Havok data) | Not started |
| Character controller | 01 §7 | Specified at algorithm level. Use Rapier/Avian's kinematic character controller |

**Recommended build order:**
1. **Greybox prototype in Bevy:** context framework (enum + immediate switch + per-context data),
   input mapping, Ground speed bands and turning, InAir jump-to-target warping, landing damage.
   Use authored test edges (hand-placed guidance edges in a test level) and placeholder clips with
   fake root motion.
2. **Climb grid and move tables** on test edges, then Ledge shimmy and pull-up, Walling and Beam.
3. **`.forge` reader:** container format, then GuidanceSystem blobs (format already decoded),
   then collision, then animations.
4. **Real clips:** swap placeholders for decoded root-motion clips. Compare against recorded traces
   from the real game (position, context and sub-state per frame) to tune the feel.

## 5. Biggest open questions

1. **Animation data.** Locomotion is root-motion driven, so the clips *are* the speeds. Next RE target:
   the `.forge` container and animation formats.
2. **Runtime confirmation.** All numbers are static reads. A small trace (Altaïr's position and current
   context ID at `Human`+slot1+0x48, via Cheat Engine/x64dbg on an offline save) would validate the
   speed bands, the 9.8 gravity and jump snapping. The game is single-player and has no anti-cheat.
3. **Ledge per-state logic** (shimmy speed, corner turns, pull-up checks): being completed in report 03.
4. **Guidance edge authoring:** if a geometry fallback is wanted, the edge-classification rules must be
   re-derived (filter thresholds are known; merging and occlusion pruning are not).
5. Ladder and rope per-frame logic, and the HumanGround minor states and event IDs.

## 6. State of the IDA database

- `bin/AssassinsCreed_Dx9.exe.i64`, with a pristine backup in `bin/backup/`.
- **About 370 functions renamed** (counted in the IDB) as `Class__Method` across AIActor, Human, the input interpreter,
  CharacterController, HumanGround, HumanInAir, HumanClimb, HumanLedge, HumanWalling,
  HumanNarrowObject(+Beam), HumanPole and Guidance*. Key functions are commented.
- Nothing was patched; only names, comments and types were changed.

## 7. Methodology (how this was produced)

1. **Triage:** checked the exe isn't packed (normal MSVC sections, entropy 5.6, 260 imports) and
   found it has full RTTI (about 3,400 class names).
2. **Setup:** ran IDA auto-analysis once in the GUI (about 2.5 h), then worked headless through the
   IDA MCP.
3. **Mapping:** listed every RTTI vftable for the Human* classes, which gave each context's code range
   and the shared 16-slot interface.
4. **Reflection:** recovered the enum, class and property tables, identified the name hash as CRC-32,
   and dictionary-attacked the field names.
5. **Parallel analysis:** six agents, each with its own subsystem and a shared brief
   (`_agent_brief.md`). They decompiled, renamed, commented and wrote reports 01–06. Two notes:
   - one move-table initialiser was emulated with a small x86 interpreter to dump its tables;
   - claims cite addresses, guesses are marked, and number-base conversions were done by tool.
6. **Synthesis:** cross-checked the reports against each other (e.g. the jump-target hand-off between
   01 and 04, and the `*Data` = runtime-state correction) and wrote this overview.
