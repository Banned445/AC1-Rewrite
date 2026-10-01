# 05 — Beams / narrow objects, wall-running ("walling"), poles, ladders, ropes

Status: walling and narrow-object/beam covered in depth; pole/ladder/rope lighter (see their sections).
All addresses are VA in `AssassinsCreed_Dx9.exe` (base 0x400000). Facts are verified from code unless marked **(hypothesis)**.

## 0. Common framework (applies to every module here)

- Each module is `HumanXxx : DataContext<Human,HumanData,HumanXxxData>, ActorContext, FSMStaticState(+4), IHumanXxx(+0x14), …` (RTTI hierarchy, dumped with a small RTTI walker).
- Common fields: `+0x08 Human*`, `+0x0C HumanData*` (its `+0x30` points to the HumanDataBundle), `+0x10 HumanXxxData*` (runtime context for this module; *not* tuning).
- Base vftable slots (seen identically in all modules): 0 dtor, 2 `ResetDataDefaults`, 7 `QueryInterface(id)` (16 = module interface, 22 = IHumanDamage, 23 = IHumanTeleport; `HumanWalling__QueryInterface` 0xE33760), 10 `Init(HumanData*)` (stores data pointer and writes the **ActorContextID**: 11 Walling `0xE33C60`, 12 NarrowObject `0xE4D0C0`), 11 `OnEnter`, 12 `OnExit`, 13 `Update(int phase)` (phase 0 = main tick), 14 `SetupFSMStates`.
- Each module owns an inner FSM: state ids stored as bytes at fixed offsets (3 bytes apart); "current" byte compared to them each tick. Transitions to other modules are posted with `sub_55F7E0(human, eventId, closure)` (event 8 = generic "leave context", 9 = "go to ledge" in walling, 4 = interrupt) **(event semantic = hypothesis)**.
- `sub_B1CBD0(human, ActorStateID, clear, 0)` sets the presentation ActorStateID (33 Walling, 40 Beam, 69 Pilotis, 25 JumpingOnPlace, 67 CameraReset …).
- Animation: `sub_7183F0(human->vfunc80())` returns the anim controller; on it `sub_501190(animRequestId,0,0,-1)` requests an animation node by numeric id (ids listed below; IDA sometimes shows them as `loc_xxxx+n`, they are plain integers), `sub_5017B0`/`sub_501760` = "current anim finished / at exit point", `sub_4FEBF0(1, w)` sets a blend parameter, `sub_502E30` sets a 4-way blend weight array, `sub_501C90`/`sub_502590` return anim duration. `sub_711130(pos, targetPos, fwd, right, targetDir, up, duration, …)` sets up a **motion-warp** (align root to a target over `duration`); `sub_7113F0(human, dt, 1)` advances it.
- Fixed timestep `dword_192DDA8 = 1/30 s`.
- `entity+0x7C` (`h` below) is a per-character float used as the reference length for all ledge-height bands **(hypothesis: character height, ≈1.8 m)**.
- Guidance queries: `sub_116E1A0(entity, pos, dir, halfAngle, boxMin, boxMax, …)` = find a guidance object (beam/edge) in an oriented box around `pos`; `sub_E1C820` / `sub_E1C8F0` = capsule/ray collision probes; `sub_B2D2A0` = free-space capsule test.

## 1. HumanWalling (wall-run, vertical and horizontal)

### 1.1 Summary
Altaïr runs up (Vertical) or along (Horizontal) a wall for one animation, then either grabs a ledge found by probes (→ HumanLedge), rebound-jumps off the wall (→ InAir), or drops back (→ Ground). Everything is anim-driven; the code only chooses animations, probes for ledges and sets up motion-warps.

### 1.2 Entry
- `0xB263B0` (Human-level, called from `0xE4D7F0` NarrowObject and from HumanDecision `0xF70E70`) and `0xDA2C30` (HumanGround) start walling: `WallingData.SubState = 1 (EntryA)`, `WallingType = 0 (Vertical)`, bool `+0x20 = arg`, request anim **68**, motion-warp to the wall contact (0.13 s in B263B0, anim length in DA2C30).
- `HumanWalling__ResetFlags 0xE35ED0` (OnEnter) stores `StartHeight = pos.z`, clears flags.
- `HumanWalling__OnEnter 0xE36950` → `ActorStateID 33 (Walling)`.

### 1.3 Layout
HumanWalling (0xA0 bytes, ctor `0xE36980`):
| off | meaning |
|---|---|
| 0x20 | vec4 input/stick direction (world) — set via IHumanWalling slot1 `0xE36250` |
| 0x30 (48) | flag: ledge found → go to ledge (event 9) |
| 0x31 (49) | flag: rebound jump launched → leave (event 8) |
| 0x32 (50) | flag: walling finished → leave (event 8) |
| 0x34 (52) | flag: go to inner state 4 (post-run ledge grab) |
| 0x38 | int "action command" 0..4 (default 4) — IHumanWalling slot0 `0xE337D0`; 0 = keep going up / try ledge **(hypothesis: pad command)** |
| 0x3D/0x3E | next/current inner state; states at 0x88=1 (main), 0x8B=2, 0x8E=3, 0x91=4 |

HumanWallingData (0x24, runtime context): `+0x08 SubState` {0 None,1 WallingEntryA,2 WallingEntryB,3 WallingVertical,4 WallingHorizontal,5 ReboundTransition,6 WallingVerticalEnd,7 WallStep}, `+0x0C WallingType` {0 Vertical,1 Horizontal}, `+0x10 WallingSide` {0 Left,1 Right}, `+0x14 StartHeight`, `+0x18 ReboundVerticalDistance` (default **-3.5**), `+0x1C ReboundHorizontalDistance` (default **5.6**) (defaults written by `HumanWalling__ResetDataDefaults 0xE337A0`; names recovered by CRC32), `+0x20` bool (unnamed; selects alternate ledge-grab anim).

### 1.4 Sub-state machine (`HumanWalling__UpdateWallingSubState 0xE37590`, called each tick from `UpdateFSM 0xE39C20`)
| from | to | condition | anim |
|---|---|---|---|
| 1 EntryA | 2 EntryB | `HumanData+0x108` blend == 1.0 | Vertical: 0x1004122; Horizontal: 0x1004EAB (Left) / 0x1007671 (Right) |
| 2 EntryB (Horizontal) | 4 Horizontal | anim done; command default | 74/75 (side); commands 1–4 pick other anims |
| 2 EntryB (Vertical) | rebound | stick ≈ 0 (|v|<0.0005) → `ReboundJump` | |
| 2 EntryB (Vertical, cmd 0) | Ledge (pullup) | ledge probe A succeeds | 0x100759B / 0x109B6CF |
| 2 EntryB (Vertical, cmd 0) | Ledge (hang) | ledge probe B succeeds | 90487581 (4-way blend) |
| 2 EntryB (Vertical, cmd 0) | 3 Vertical | no ledge | 69 |
| 2 EntryB (cmd≠0) | leave | | 0xFF7CA1, flag50 |
| 3 Vertical | rebound | stick non-zero | |
| 3 Vertical (anim done, cmd 0) | Ledge | probes C/D (higher bands) | 90487582 / 0x109BB50 / 0x106CFC2 / 0x106CFC5 |
| 3 Vertical | 6 VerticalEnd | no ledge | 0x121847A |
| 6 VerticalEnd | rebound | stick ≠ 0 and cmd ≠ 4 | |
| 6 VerticalEnd | leave (Ground) | anim done | 0x1291762, flag50 |
| 4 Horizontal | (next step) | anim done → anim by cmd/side, lateral nudge ±0.02 (`0xE355C0`) | 76/77 etc |
| 5 ReboundTransition | rebound | anim done, stick ≠ 0 and angle(fwd,stick) ≥ 45° | else 0x1291761 + leave |

Inner FSM (`StateMain_CheckTransitions 0xE352D0`): SubState 6 → inner state 3 (wait flags 49/50 → event 8); flag52 → state 4 (`State4_Update 0xE39BB0` → `TryGrabLedgeAfterWallRun 0xE39550` → event 9); flag48 → event 9 (ledge); flags 49/50 → leave.

### 1.5 Ledge probes (per-frame logic, pseudo-code)
`FindLedgeCandidates(origin, width, height, depth, range, sortMode)` `0xE36BD0`: oriented box in front of the character (half-sizes = args), guidance candidates in a −75°..+135° fan, filtered to ±45° of facing, each validated by a ray (`sub_66E150`) and capsule (`sub_E1C820`), sorted (`0xE36A40`). `ClassifyLedgeCandidate 0xE341D0` puts a capsule 0.05 m out from the edge (rotated −45°) and returns 0 if blocked (<0.03 m), 2 if 0.03–0.4 m of room, 1 if ≥0.4 m (or material 8/9).
```
h = entity.height
EntryB vertical, command 0:
  A: FindLedge(pos, 0.3h, 1.05h+0.05, h, range 0.8)
     accept first candidate with class∉{0,2} and 0.25h ≤ ledgeHeight ≤ h
       blend = (max(d,0.3) - 0.3h)/(0.7h); LedgeData.SubState=4 (Pullup); flag48
  B: FindLedge(pos + 1.5*up - 0.6*fwd, 0.3h, h+0.05, 0.25h+0.1, range 1.2)
     first class≠0: heightBlend = min(dist-1.5,1); lateral = clamp(1-(lat-0.1)/0.7,0,1)
       4-way bilinear weights, warp target = ledge - (0,0,2.4); LedgeData.HangType=1,Reception=3 → hang
  else SubState=3, anim 69
Vertical (after run anim):
  C: FindLedge(pos - 0.6*fwd + up, 0.3h, 1.7h+0.1, 0.8h, 1.2): lateral < 0.25h+0.1 and 2h ≤ height < 2.7h+0.1 → hang (90487582)
  D: FindLedge(pos - 0.6*fwd, 0.8h, 2.7h+0.1, 1.5h, 1.2):
       0.5h ≤ height < h → pullup, blend (height-0.5h)/(0.5h)
       h ≤ height < 2.7h+0.1 → hang on wall with 4 hand/foot reports (`sub_E55CB0`), LedgeData.HangType=0
  else SubState=6 (VerticalEnd)
```
State 4 (`0xE39550`, after anim ids 56567712/43150439): FindLedge(pos, 0.3h, k·h+0.1, h, 1.2) with k = 2.8 (those anims) or 3.0; blend = (height − 2.3h)/(0.5h) or (height − 2.5h)/(0.5h); warp target = ledge − 1.1·up or − 2.4·up; LedgeHangType 0 or 2.

### 1.6 Rebound jump (`HumanWalling__ReboundJump 0xE365C0`)
Push-off direction = stick direction, clamped to within ±89° (1.5533 rad) of the wall normal (−forward). Query a landing point along it (Human+0xFC guidance interface, `sub_E96BF0`); if none, target = pos + 7·dir − 3·up. Launch jump `sub_B27180(target, type 2, …)`, set flag49, ActorState 67 (CameraReset).

### 1.7 Constants
1/30 dt; 0.0005 stick dead-zone; ±45° fan (0.785398); −75°/+135° search fan (−1.308997/2.356194); 0.42 query param (`flt_19BA818`); 0.27, −0.23 box offsets; 0.05/0.3/0.1/0.7/1.05/1.5/1.2/0.8/2.0/2.4/2.7 multipliers above; 7.0 / 3.0 rebound fallback; −3.5 / 5.6 rebound distances; 0.13 s warp.

## 2. HumanNarrowObject (+ HumanNarrowObjectBeam)

### 2.1 Summary
"Narrow object" = any narrow support you stay on with ground-like movement: narrow ledges walked along the wall ("Edge"), wall lean, **beams**, **pilotis** (wooden posts you hop between), crowd run, free-run on narrow geometry. Implements IHumanGroundMovement/IHumanMovement. Beams are delegated to a lazily created FSMClassState object `HumanNarrowObjectBeam` (0x8C0 bytes, ctor `0xE51FD0`, getter `HumanNarrowObject__GetBeamState 0xE523C0`) whose code lives at 0xF6E000–0xF81000.

### 2.2 Entry (writers of HumanNarrowObjectData before switching to context 12)
- Ground `0xD7DAB0`: SubState=7 Lean. Ground `0xD8A620`: SubState=5 Edge, EdgeState 3 (Left) or 2 (Right), CurrentEdgeDir (+0x80) = cross(edgeDir, up), +0x90 = −edgeDir; +0xE4/+0xE5 bytes copied from ground.
- Also Ground `0xD84A40`, `0xD9F4C0`; Ledge `0xDD2A50`, `0xDE2EE0`; Climb `0xDEEDD0`; InAir `0xE07D00`, `0xE0B890` (beam/pilotis landing: guidance box (−0.4,−0.35,…)–(0.4,0.35,…) around landing point +0.15 m), `0xE0BB70`.
- `SelectInitialState 0xE528C0` maps SubState → inner state:
| SubState | inner state | extra |
|---|---|---|
| 0 Movement | unchanged | |
| 1 Beam / 2 BeamEntry | Beam class-state | beam.mode=1 |
| 3 BeamReception (from air) | Beam | mode 7 |
| 4 PilotisReception | 3 Pilotis | PilotisEntryType=1 FromInAir |
| 5 Edge | 8 Edge | copies edge vectors |
| 6 FreeRun | 1 Movement | |
| 7 Lean | 12 Lean | copies lean vectors, CurrentLeanHeight/Width |
| 8 CrowdRun | 2 CrowdRun | |
| 9 ObstacleCollision | Beam | mode 6 |

### 2.3 Inner FSM (`SetupFSMStates 0xE4EC60`, `UpdateFSM 0xE53730`)
State bytes at +0x1C8.. : 1 Movement (`StateMovement_Update 0xE534B0`), 2 CrowdRun (`0xE4E610`, leaves when anim done), 3 Pilotis with sub 4/5/6 = PilotisEntryType FromFreeStep/FromInAir/FromJumpImpulsionStart (`0xE52170`, `StatePilotis_Enter 0xE4D8A0` sets ActorState 69), 7 Beam class-state (`HumanNarrowObjectBeam__UpdateFSM 0xF80AA0`), 8 Edge with sub 9/10/11 (`0xE52720`), 12 Lean with sub 13/14/15 (`0xE521B0`). `+0xD0` float ramps 0→1 at 2/s (4/s when flag bit7 of +0xB8 set) — blend-in weight. `+0xC4` = stick magnitude (≤0.1 = no input). `+0x30` = stick vector.

Movement-state tick (`0xE534B0`):
1. `CheckSupportAndFall 0xE51190`: ground-support query on Human+0xFC; if anim phase ≥0.5 and support flags say side 1/2 → set fall flags; if no support → choose fall/jump (`sub_C7F000`, `sub_B0FB70`, jump types 0/1/3/4/6, ±120° facing check 2.0944) or play recover anims 250441553/4 (blend (t−0.5h)/(0.5h)); a valid hay-stack under the character → `TransitionSetupDataToHumanHayStack`.
2. `TryPilotisFreeStep 0xE50190` → anim 957955007/8, warp, PilotisEntryType 0.
3. `TryMountBeam 0xE52AD0` → `GotoBeamState 0xE527F0` (ActorState 40 Beam). Beam entry mode chosen from beam axis vs facing vs stick: stick>0.25 and facing 80°–150° to beam and stick within 45° of beam → mode 4; facing 30°–100° and stick >135° → mode 5; else |dot(beam,facing)| ≥ 0.866 → mode 2 (straight), else 3 (side). Writes BeamEntryVector (+0x40), BeamEntryPoint (+0x50), beam start/end (+0x60/+0x70).

### 2.4 Beam class-state (HumanNarrowObjectBeam)
- `+0xB4` owner HumanNarrowObject, `+0xB8` NarrowObjectData, `+0x14` entry mode, `+0x260/+0x270` current beam segment endpoints, `+0x2B0/+0x2C0` next segment, `+0x120` look-ahead target (0.6 m ahead), `+0x2A8` on-beam flag, `+0x2F8` lost flag, `+0x6B8` forward input along beam.
- `UpdateFSM 0xF80AA0`: states at bytes +0x4C (Entry), +0x4F/+0x52/+0x55, +0x58 (Main), +0x91… Entry tick: `AlignToBeamEntry 0xF7EBA0` (frame from BeamEntryVector/Point) until `IsEntryReceptionDone 0xF76AD0` (ground found or >0.5 m from entry point), then `EnterBeamMain 0xF722C0` (ActorState 40).
- Main (`StateBeam_Update 0xF808D0`), each tick: `DetectBeamSegments 0xF753A0` (guidance query AABB ±7 m horizontal, −12..+2.5 m vertical; oriented box ±2 m along / ±0.5 m lateral / ±0.5 m vertical in a 60° cone, fallback rotated 90°, fallback ±1 m any direction; next-segment box 0.4–1 m ahead, 45° cone) then `ConstrainRootMotionToBeam 0xF7C3A0` (movement is **animation root motion** projected onto the beam line; stops 0.3 m before an end (<0.31 m triggers), lateral drift corrected toward the line at ≤ 2·dt m per frame; switches to the next segment when present).
- Stick classification `ClassifyStickDir 0xF76150` vs facing: |a|<75° → 0 forward, a>135° → 3 back (turn-around state), else 1/2 left/right.
- `CanStepOffBeamEnd 0xF77C00`: within 0.16 m of (or past) the segment end and free capsule (r 0.25, h 0.5, 0.4) at end + 0.5·dir, 1.2 m above feet → step-off state.
- Pilotis → beam `0xE53680` (mode 8). Jump-on-place anims 1366119147/1366118938 (`0xF717D0`, ActorState 25).

### 2.5 Data layout (HumanNarrowObjectData 0x1C0, runtime context)
`+0x10 CurrentBeamDir, +0x20 CurrentBeamCenter, +0x30 float, +0x40 BeamEntryVector, +0x50 BeamEntryPoint, +0x60/+0x70 beam start/end, +0x80 CurrentEdgeDir, +0x90/+0xA0/+0xB0 vec4, +0xC0 CurrentLeanHeight, +0xC4 CurrentLeanWidth, +0xC8 SubState, +0xCC PilotisEntryType, +0xD0 EdgeState {0 Invalid,1 Front,2 Right,3 Left}, +0xD4 JumpVerticalDistance, +0xD8 JumpHorizontalDistance, +0xDC LeanState {FaceLeft,FaceRight}, +0xE0 FreeRunSubState {EntryB,EntrySide}`; unreflected runtime: +0xE4/+0xE5 bytes, +0xF0/+0x100 support point/normal.

### 2.6 Beam constants (from scan of 0xF6E000–0xF81000)
0.16, 0.25, 0.3, 0.31, 0.4, 0.5, 0.6, 0.8, 1.2, 1.5, 2.5, 7, −7, −12, 15, angles 30°/45°/60°/75°/90°/120°/135°/58° (1.0123), cos 80° (0.17365).

## 3. HumanPole (lighter)

### 3.1 Summary
A "pole" is a guidance **polyline** (pole object at `PoleData+0x1F4`, its vfunc+40 returns the point array; `PoleData+0x50` = current segment index). The character is attached 0.2 m off the pole axis and climbs with root-motion animations chosen from the pole's inclination; horizontal/inclined poles are handled by the same module with different anim tables.

### 3.2 Entry
- From Ground: `0xD9C360` (`CanGrabPole`, hypothesis name): closest point on pole (`sub_C7AF50`) must be within 3.5 m vertically, within the reach distance (+0.75 m when the pole is >70° from vertical, |dot(axis,up)| < 0.342), and within 90° of facing; stores GrabPosition (+0x30) and segment index (+0x50).
- From InAir: `HumanInAir__CheckJumpTargetArrival 0xE07D00` sets `PoleData.EntryType = 1 (FromAirStraight)`, binds the pole (`sub_D8DF10`), sets segment index.
- EntryType {0 FromGround, 1 FromAirStraight, 2 FromAirInclined}.

### 3.3 Inner FSM (`SetupFSMStates 0xE29B10`, `UpdateFSM 0xE2D9A0`)
States 1..9 (bytes +0x13C..+0x154). 1 Entry (`StateEntry_Enter 0xE2AFB0`: anim chosen by InclinationType==2 / jump-vs-ground / byte +0x1F0, motion-warp to GrabPosition − 0.2·(dir to pole) over the anim length; `StateEntry_Update 0xE28F10` waits for blend == 1), 2 second entry phase (`0xE2C940`, `WantsLetGoDown 0xE29000`: DestSpeedRatio>0.25 and DestHeading.z<0 at anim exit point → drop), 3 Climb with sub-states 4..7 (`StateClimb_Update 0xE2D870`), 8 and 9 exit states (post event 4 / 8 when the exit anim ends).

### 3.4 Per-frame climb (`ClimbMovement 0xE2C610`)
```
AttachToPole()                         // 0xE2C580: closest point on segment, 0.2 m offset, face pole
speed = PoleData.DestSpeedRatio (+0x54)  // stick magnitude → anim playback (sub_E28CB0)
if speed > 0 and DestHeading.z > 0 and !ReachedTop:  MvtAnimState = high grip ? ClimbHigh_Up(4) : ClimbLow_Up(2)
elif speed > 0 and DestHeading.z < 0:                 MvtAnimState = high ? ClimbHigh_Down(5) : ClimbLow_Down(3)
else: wait (MvtAnimState 0/1)
anim = table[MvtAnimState][InclinationType]   // PoleData+304 / +328 / +352 / +376
ReachedBottom(+0x5D) = (segment 0 and dist to pole start ≤ 1.0 and moving down)
```
`ComputeAttachFrame 0xE2BD80`: InclinationType = Light if dot(axis,up) > cos 20° (0.9397), Med if > 0.5, else High; when the pole is horizontal (|axis.z| == 1 test fails) the "forward" is built from cross products.
Constants (scan 0xE28600–0xE2DF00): 0.2 attach offset, 0.25 input threshold, 0.5, 0.65, 0.75, 1.25, 1.5, 0.4, 30°/60°, 2π, 10000 (sentinel distance).

PoleData layout: +0x10 DestHeading (input), +0x20 PoleJumpDirection, +0x30 GrabPosition, +0x40 EntryType, +0x44 InclinationType, +0x48 MvtDivision, +0x4C MvtAnimState, +0x50 segment index (u32), +0x54 DestSpeedRatio, +0x58 Pole (objref), +0x5C ReachedTop, +0x5D ReachedBottom; unreflected +0x130..+0x188 anim-id tables, +0x1F0 byte "high grip/swing" **(hypothesis)**, +0x1F4 pole object.
Swinging (ActorState 44, SwingEventMonitor) is handled by HumanLedge (`LedgeSubState 8 SwingReception`), not by HumanPole **(hypothesis; not traced)**.

## 4. HumanLadder (light)
- Data: +0x20 DestHeading, +0x30 EntryType {FromGround, FromAirStraight, FromAirInclined, FromWalling, FromClimb}, +0x34 InclinationType {Vertical, Horizontal}, +0x38 MvtDivision, +0x3C MvtAnimState (20 values: Wait/Climb Low/High Up/Down, Revolve, Enter/Exit Ground/Top Low/High, Release, Jump), +0x48 Ladder (objref), +0x4C LadderHeight, +0x50 HeightInLadder, +0x54 ReachedTop, +0x55 ReachedBottom.
- FSM: 12 states (`SetupFSMStates 0xE1EDE0`), `UpdateFSM 0xE27D30`: if the ladder object reference becomes invalid → leave (event 8). State 1 = entry (`sub_E25240`), then state 5 = main (`StateMain_Update 0xE278E0`, sub-states 6/7/8); states 9..12 = exits (`0xE1F6C0`, `0xE21D60`, `0xE1F710`, `0xE21E20`). Big helpers `sub_E228E0`, `sub_E266D0` not analysed.
- Same structure as pole: root-motion climb anims selected by MvtAnimState; enter-from-top / exit-to-top use TopOfLadderEventMonitor (ActorState 60 LadderTop). Entry from Walling (EntryType 3) exists.
- Constants in 0xE1C000–0xE28500 include 0.35, 0.45, 0.55, 0.65, 0.85, 0.9, 0.95, 1.05, 1.55, 1.6, 1.8, 2.35, 2.85, 3.5, −3.5, 5 — likely rung spacing / top-exit heights **(hypothesis, not mapped to code)**.

## 5. HumanRope (light)
- Data: +0x10 DestHeading, +0x20 vec4, +0x30 Rope (objref), +0x34 DestSpeedRatio, +0x38 ReachedTop, +0x39 ReachedBottom.
- `UpdateFSM 0xE33110`: state +0x1E8 with sub-states (+0x1EB/+0x1EE/+0x1F1/+0x1F4) and state +0x1F7. Per-frame `UpdateLimbGripsFromAnimTags 0xE32780`: animation tag bits 0x10/0x20/0x01/0x02 attach/detach the four limbs to the rope (IK slots 0–3 vs 4–7); accumulates a timer at +0x198 while hanging. Big helper `sub_E31490` not analysed. Constants: 0.55, 0.7, 1.2, 2.4, 10, cos 110° (−0.342).

## 7. Open questions / dynamic checks
- Meaning of walling command `+0x38` values 0–4 (set by the pad controller via IHumanWalling slot 0): breakpoint `0xE337D0`.
- Confirm `entity+0x7C` = character height (read it in-game; expect ≈1.8).
- Unbalanced (ActorState 32) is never set inside the narrow/beam code — find who sets it (bump while on beam?).
- Event ids 4/8/9 posted with `sub_55F7E0` — map to target contexts.

## 6. Interfaces to other subsystems
- In: Ground (`0xDA2C30` walling, `0xD7DAB0`/`0xD8A620` narrow, `0xD9C360` pole), InAir (`0xE07D00`, `0xE0B890`, `0xE0BB70`), Ledge (`0xDD2A50`, `0xDE2EE0`), Climb (`0xDEEDD0`), HumanDecision (`0xF70E70` → `0xB263B0` walling). Pad controller writes IHumanWalling slot0/1 (command + direction) and module `+0x30` input vectors.
- Out: HumanLedge (writes `HumanLedgeData` via `HumanData__GetLedgeData 0xB2FCA0`: +0x4C SubState, +0x68 LedgeHangType, +0x6C HangFreeReceptionType, +0x70 blend), InAir jump (`sub_B27180`), HayStack (`TransitionSetupDataToHumanHayStack`), fall setup (`sub_B0FB70`), ActorStateIDs via `sub_B1CBD0`.
- Data accessors (bundle offsets): Walling +0xE10 (`0xB2FCC0`), Ledge +0xC50 (`0xB2FCA0`), Pole +0xA10 (`0xB2FC80`), NarrowObject +0xE40 (`0xB2FCD0`).

## 8. Renamed functions
| addr | name |
|---|---|
| 0xE36980 | HumanWalling__ctor |
| 0xE33C60 | HumanWalling__Init |
| 0xE337A0 | HumanWalling__ResetDataDefaults |
| 0xE33760 | HumanWalling__QueryInterface |
| 0xE36950 | HumanWalling__OnEnter |
| 0xE39CC0 / 0xE39C20 | HumanWalling__Update / __UpdateFSM |
| 0xE37590 | HumanWalling__UpdateWallingSubState |
| 0xE36BD0 | HumanWalling__FindLedgeCandidates |
| 0xE341D0 | HumanWalling__ClassifyLedgeCandidate |
| 0xE36A40 | HumanWalling__SortCandidates |
| 0xE365C0 | HumanWalling__ReboundJump |
| 0xE35290 | HumanWalling__SetupFSMStates |
| 0xE35ED0 | HumanWalling__ResetFlags |
| 0xE352D0 / 0xE35330 / 0xE35370 | HumanWalling__StateMain/State2/State3_CheckTransitions |
| 0xE39BB0 / 0xE39550 | HumanWalling__State4_Update / __TryGrabLedgeAfterWallRun |
| 0xE33F50 | HumanWalling__GotoState4 |
| 0xE33800 | HumanWalling__WantsExit |
| 0xE36350 | HumanWalling__IsInMainState |
| 0xE36250 / 0xE360F0 / 0xE337D0 | HumanWalling__SetInputDirection / __SetDataFlag20 / __SetActionCommand |
| 0xE4D0C0 | HumanNarrowObject__Init |
| 0xE4C0D0 | HumanNarrowObject__ResetDataDefaults |
| 0xE4EC60 | HumanNarrowObject__SetupFSMStates |
| 0xE55270 / 0xE53730 | HumanNarrowObject__Update / __UpdateFSM |
| 0xE53DE0 / 0xE53330 | HumanNarrowObject__OnEnter / __OnEnterImpl |
| 0xE533D0 | HumanNarrowObject__OnExit |
| 0xE528C0 | HumanNarrowObject__SelectInitialState |
| 0xE534B0 / 0xE4FE20 | HumanNarrowObject__StateMovement_Update / _Enter |
| 0xE51190 | HumanNarrowObject__CheckSupportAndFall |
| 0xE52AD0 / 0xE527F0 | HumanNarrowObject__TryMountBeam / __GotoBeamState |
| 0xE50190 | HumanNarrowObject__TryPilotisFreeStep |
| 0xE4D8A0 / 0xE52170 | HumanNarrowObject__StatePilotis_Enter / _Update |
| 0xE53680 | HumanNarrowObject__PilotisToBeam |
| 0xE52720 / 0xE521B0 / 0xE4E610 | HumanNarrowObject__StateEdge/StateLean/StateCrowdRun_Update |
| 0xE523C0 | HumanNarrowObject__GetBeamState |
| 0xE51FD0 / 0xE52130 | HumanNarrowObjectBeam__ctor / __SetOwner |
| 0xF80AA0 / 0xF808D0 | HumanNarrowObjectBeam__UpdateFSM / __StateBeam_Update |
| 0xF7EBA0 / 0xF76AD0 | HumanNarrowObjectBeam__AlignToBeamEntry / __IsEntryReceptionDone |
| 0xF722C0 / 0xF717D0 | HumanNarrowObjectBeam__EnterBeamMain / __PlayJumpOnPlace |
| 0xF753A0 / 0xF7C3A0 | HumanNarrowObjectBeam__DetectBeamSegments / __ConstrainRootMotionToBeam |
| 0xF76150 / 0xF77C00 / 0xF7D650 | HumanNarrowObjectBeam__ClassifyStickDir / __CanStepOffBeamEnd / __StateIdle_PreUpdate |
| 0xE2DBA0 / 0xE2D9A0 | HumanPole__Update / __UpdateFSM |
| 0xE2CC90 / 0xE2C9B0 | HumanPole__OnEnter / __OnEnterImpl |
| 0xE29B10 | HumanPole__SetupFSMStates |
| 0xE2AFB0 / 0xE28F10 / 0xE2C940 | HumanPole__StateEntry_Enter / _Update / __StateEntry2_Update |
| 0xE29000 | HumanPole__WantsLetGoDown |
| 0xE2C580 / 0xE2BD80 | HumanPole__AttachToPole / __ComputeAttachFrame |
| 0xE2D870 / 0xE2C610 / 0xE2A9F0 | HumanPole__StateClimb_Update / __ClimbMovement / __CommonTick |
| 0xE29B80 / 0xE29BD0 | HumanPole__State8_Update / __State9_Update |
| 0xE27FA0 / 0xE27D30 / 0xE1EDE0 / 0xE278E0 | HumanLadder__Update / __UpdateFSM / __SetupFSMStates / __StateMain_Update |
| 0xE333F0 / 0xE33110 / 0xE32780 | HumanRope__Update / __UpdateFSM / __UpdateLimbGripsFromAnimTags |
| 0xB2FCC0 / 0xB2FCA0 / 0xB2FC80 / 0xB2FCD0 | HumanData__GetWallingData / GetLedgeData / GetPoleData / GetNarrowObjectData |
| 0xDE4EB0 | Vec_ProjectPointOnLine |

## 9. Summary
Walling is an anim-driven 7-sub-state machine whose only gameplay logic is ledge probing in height bands proportional to `entity+0x7C` (pull-up 0.25h–h, hang up to 2.7h, post-run up to 3h) and a rebound jump off the wall when the stick is pushed. Beams live inside HumanNarrowObject (which also does edges, lean, pilotis, crowd-run): beam mounting picks one of 8 entry modes, then a separate FSMClassState walks the beam by projecting animation root motion onto guidance beam segments (stop 0.3 m before the end, lateral pull 2 m/s, step off when the end is within 0.16 m and free). Poles are guidance polylines climbed with root-motion anims selected by inclination (cos 20°/60° thresholds) and stick up/down; ladders and ropes follow the same pattern (light coverage). All *Data classes are runtime context, not tuning; tuning is in animations/forge.

## Methodology
RTTI hierarchy + vftable dump (vt.py), reflection descriptors (refl.py; names recovered by CRC32), decompiled update/enter functions, float-constant scan of each code range, xrefs to the `HumanData__Get*Data` accessors to find entry points from other modules.
