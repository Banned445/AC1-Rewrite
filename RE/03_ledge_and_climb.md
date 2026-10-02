# 03 — HumanClimb (wall climbing on handholds) and HumanLedge (hanging on ledges)

Status: HumanClimb is mapped in depth (grid, move tables, decision order, state machine). HumanLedge (§7) is mapped per state: the shimmy algorithm, vertical steps and jumps, pull-up, pull-down, entries and exits. Corner internals and swing remain shallow.
Addresses are VAs in `AssassinsCreed_Dx9.exe` (image base 0x400000). Function names are the ones now in the IDB.

## 0. Methodology
1. Decoded the reflection descriptors for HumanClimbData/HumanLedgeData (python on the exe, `refl.py` in the scratchpad).
   Property names come from CRC32 recovery (coordinator's `RE/data/data_classes_named.txt`) plus my own brute force (`CurDirection`).
2. Followed the HumanClimb vftables (vt.py) to find ctor, Init, OnEnter, OnExit, Update and the per-state update functions.
3. `HumanClimb__StaticInitTables` (0xDE4F80, 0x2FDD bytes) is not an update function. It is a static initializer that fills the
   pose and move tables in .bss. I ran it through a small x86 interpreter of my own (`emu_init.py`, which handles only mov/xor/movq/movaps, and the whole
   function emulated cleanly) and dumped the resulting tables (`climbtab.py`). The tables are reproduced below.
4. Decompiled the decision chain (ChooseMove → Try* predicates) and the grid builder. Renamed the functions and added comments.

## 1. Summary (plain words)
- **HumanClimb (ContextID_Climb = 10)** is free climbing on a wall that has handholds. Every frame while idle it builds a **local
  occupancy grid of handholds** around the character. It does this by probing the world's *guidance objects* (climbable edges) at fixed cell centres. It then
  turns the stick direction into one of 10 directions and looks up a **table-driven move**. The lookup key is the current limb "pose"
  (6 poses) and the direction. Each move moves one side (one hand+foot pair) or both sides by whole cells. The root then follows an
  interpolated path whose duration comes from the move animation (0.5 s if no anim). Four IK targets (2 hands, 2 feet) are snapped to the
  probed hold positions and normals. **The grid is 0.75 m wide per column and 0.6 m tall per row. Hands sit 2 rows (1.2 m) above the matching foot cell.**
- If no grid move is possible, the climber checks the special exits in order. Ladder, grab a ledge (→ HumanLedge), side
  jump, reach another surface, back eject / wall jump, reach a ledge above, drop to a ledge below. If nothing is possible it plays a "blocked/reach" animation for that direction.
  Losing the hold (`sub_E55A10` false), being hit, or the release action sends the character to **InAir (context 8)** with FallOrigin_Climb.
- **HumanLedge (ContextID_Ledge = 9)** is hanging on a ledge (wall hang or free hang), shimmying, turning corners, pull-up, pull-down, hand-pass-over, parallel jump, swing reception, etc.
  It has 20 internal states. The entry state is chosen from `HumanLedgeData.SubState` (LedgeSubState enum), which the previous module writes.
- There is **no stamina or grip timer** in either module. Climbing has no time limit. The only "timeout" is a debug-ish timestamp at HumanClimb+0x1280 (see §4).

## 2. Layouts

### 2.1 Reflection property descriptor (32 bytes) — verified
`+0 flags | +4 CRC32(name) | +8 type hash (enum descriptor hash for enums) | +0xC type code in high 16 bits | +0x10 dword: byte offset = value >> 18`.
Type codes seen: 0x00 bool, 0x03 (1-byte? DestMvtType), 0x07 int32, 0x0A float, 0x0D vec4, 0x16 handle64 (GuidanceObject report), 0x19 enum.
The class descriptor for HumanClimbData is at 0x19978F8: {name, hash e00ae315, hash e96177a2, size 0x70}. Twenty bytes before it, at 0x19978D8, sits
{props*, nProps=17, enums*, nEnums=3, 0, 0, groups*, nGroups=2}. Offsets were verified against the ctor `sub_C843A0`.

### 2.2 HumanClimbData (size 0x70, ctor 0xC843A0; lives in HumanData at +3488; getter `sub_B2FCB0`) — runtime context, not tuning
| off | type | name | default | use |
|---|---|---|---|---|
| +0x04 | int | (context id) | set to 10 in HumanClimb__Init 0xDE4F50 | |
| +0x10 | vec4 | CurDirection | 0 | stick direction in world space. Read in ChooseMove 0xDFDE90 |
| +0x20 | enum | SubState (ClimbSubState{Wait}) | 0 | |
| +0x24 | enum | EntryType {Default, FromLedge, FromLedgeParallelJump, FromGround} | 0 | read in EnterCommon 0xDE97B0, cleared on Reset |
| +0x28 | enum | EntryPoseType {1M, 2M} | 0 | 2M → start in pose 3 (hands apart), else pose 0 |
| +0x2c | float | ? (stick magnitude, current) | 0 | copied from +0x30 every Wait frame (0xDFE510). `>0` means stick active. `>0.5` makes ChooseMove use the LONG move table |
| +0x30 | float | ? (stick magnitude, written by controller) | 0 | |
| +0x34 | float | ? | 0.75 | |
| +0x38 | float | ? | 0.6 | (equal to the cell sizes — **hypothesis**: unused copies) |
| +0x3c | bool | UseIK | 0 | |
| +0x40..+0x4c | float×4 | LeftToePull, RightToePull, LeftHandPull, RightHandPull | 0.5 | IK pull weights |
| +0x50..+0x5c | float×4 | ? (IK group, probably per-limb offsets/weights) | 0.1 | |
| +0x60 | int | (not reflected) | 4 | State3_Update checks `==0 or ==3` (0xDF5A70) |

### 2.3 HumanLedgeData (size 0x150, ctor 0xC7C110; getter `HumanData__GetLedgeData` 0xB2FCA0)
+0x10 vec4 CurDirection, +0x20 vec4 ?, +0x30 vec4 JumpDirection, +0x40 PullDownType, +0x44 PullDownSide, +0x48 GraspType,
+0x4c SubState (LedgeSubState), +0x50 HandPassOverSubState, +0x54 PullDownSubState, +0x58 int DirectionType, +0x5c/+0x60 floats (same two
floats as climb +0x2c/+0x30 — stick magnitude pair, **hypothesis**), +0x64 NextHandToMove, +0x68 LedgeHangType, +0x6c HangFreeReceptionType,
+0x70 float SwingStrength, +0x74/+0x7c GuidanceObject reports (handle64, group "GuidanceObject reports"), +0x84 DestMvtType, +0x85/+0x86 bools.
Not reflected: +0x85 is cleared on ledge enter (0xDE26D0 writes +133 = 0). +0x68 (=LedgeHangType) is read as `v12[26]`. +0x138 (v12[78]) is used on ParallelJump entry.

### 2.4 HumanClimb object (ctor 0xDF9E40; vftables 0x1700DFC / IHumanClimb 0x1700DC4 / IHumanDamage 0x1700D54 / IHumanTeleport 0x1700D48)
| off | meaning |
|---|---|
| +0x08 | owner entity (vcall +80 → anim/skeleton component) |
| +0x0C | Human* (slot10 Init arg). Human+80 is the root-motion interpolator, Human+264 float = move progress 0..1 |
| +0x10 | HumanClimbData* |
| +0x38 (56) | current **pose index** (0..5, 7). Selects the row of the pose and move tables |
| +0x3C..+0x53 (60..83) | current/candidate **move entry** (6 dwords: type, dxA, dyA, dxB, dyB, animId) |
| +0x54 (84) | "climb active / IK on" flag (cleared on exit) |
| +0x55 (85) | reach-ledge-above requested (set by TryReachLedgeAbove 0xDF1730) |
| +0x56 (86) | drop-to-ledge-below requested (TryDropToLedgeBelow 0xDF1D20) |
| +0xC4 (196) [64] | grid: hold present in cell (index = col + 8*row) |
| +0x104 (260) [64] | grid: hold is centred in cell (|dx|<=0.3, |dz|<=0.15) |
| +0x150 (336) [64]×16 | grid: hold world position |
| +0x550 (1360) [64]×16 | grid: hold orientation/normal frame |
| +0x950 (2384) [64] | grid: guidance object ptr |
| +0xA50 (2640) [64] | grid: edge/element index inside the guidance object |
| +0xB50/+0xB54 (2896/2900) | debug probe tolerances (default 0.375 / 0.3) |
| +0xB5C/+0xB5D | debug probe override flags (DebugTweakProbe 0xDE8F30, debug pad only) |
| +0xB5E (2910) | alternative (legacy) move-selection mode. Never set in the module. **Hypothesis**: dead/debug path (DF5EE0, DFD4A0, DEFC90) |
| +0xB5F (2911) | "a move was found this frame" (result of ChooseMove) |
| +0xB64 (2916) | reach-other-surface move pending (→ state 3) |
| +0xB65/+0xB66 (2917/2918) | back eject / release requested (TryBackEject 0xDF2F50) → state 6 |
| +0xB68 (2920) | target is a ledge → switch to Ledge context |
| +0xB69 (2921) | ledge grab found (TryLedgeGrab) → Ledge context |
| +0xB6A (2922) | ladder found (TryLadder) → Ladder context 5 |
| +0xB90.. (2960, 3040, 3120, 3200) | current limb contact positions (2 hands, 2 feet) |
| +0x1200..+0x1204 (4608..4612) | special-move flags: side jump near (4608) / far (4609), side ledge grab (4610/4611), corner/turn move (4612) |
| +0x1205 (4613) | transition-to-ledge-hang chosen |
| +0x1215/+0x1216 (4629/4630) | next / current internal state id |
| +0x1260..+0x1272 (4704..4722, stride 3) | state id table {1..7} (InitStateIds 0xDEB7F0) |
| +0x1280 (4736) | qword time limit. When `now >= it` the Update calls `sub_B1CA80(12)` |

## 3. HumanClimb states and transitions

### 3.1 Internal states (ids from 0xDEB7F0; names are mine)
| id | name | update fn | what it does |
|---|---|---|---|
| 1 | Wait | StateWait_Update 0xDFE760 | build grid, choose move, route to exits |
| 2 | Move | StateMove_Update 0xDF5990 | advance interpolation. When progress (Human+264) == 1.0 → Wait |
| 3 | ReachMove | State3_Update 0xDF5A70 | reach/transition anim. When the anim ends → StartMove-like (0xDEEE60) → Move |
| 4 | (unused) | — | |
| 5 | KnockedOff | StateKnockedOff_Update 0xDEE840 | entered from damage events. Plays anim 0x11FFC1A, at anim end → InAir |
| 6 | Release / BackEject | StateRelease_Update 0xDFA7E0 | anim 0x1E380CEB (506989803). At end → InAir (fill fn 0xDE8EF0) or 0xDF9E10 |
| 7 | Stunned? (**hypothesis**) | State7_Update 0xDF5AE0 | entered from events 4/5. 0xDE9250 → InAir (0xDF34B0) |

### 3.2 Entry (OnEnter 0xDEA410 → EnterCommon 0xDE97B0)
| HumanClimbData.EntryType | start state | notes |
|---|---|---|
| FromLedgeParallelJump (2) | Move | +2916 = 1 (reach move) |
| FromLedge (1), FromGround (3) | Move | pose = (EntryPoseType != 0) ? 3 : 0. EntryPoseType is then cleared |
| Default (0) | Wait | plays pose-0 anim (0x12DA39F), activates the limb-IK component (`sub_E57400(human+1328)`) |
On enter: blend time 0.2, collision/physics flags on the entity (+96 |= 0x10008000), hold-check component at human+1328.

### 3.3 Transitions
| from | to | condition | source |
|---|---|---|---|
| any | InAir (ctx 8) | `!sub_E55A10(human+1328)` (lost hold) | Update 0xDFE7F0. Fill fn 0xDF3300: anim state 33, blend 0.2, InAirData+400 = pos + (0,0,1.2), InAirData+500 = 1 (FallOrigin_Climb) |
| Wait | Move | move found and no special flag | CanStartGridMove 0xDE7FA0 → StartMove 0xDFA0C0 |
| Wait | ReachMove | move found and +2916 | 0xDE8020 → StartReachMove 0xDF6810 |
| Wait | Release | +2917 | StartRelease 0xDE96A0 |
| Wait | Ledge ctx 9 | found && (2920 or 2921 or 85) | WantsLedgeContext 0xDE8070 → 0xDF3DF0 (fill 0xDEEAC0) |
| Wait | Ladder ctx 5 | found && 2922 | WantsLadderContext 0xDE80F0 → 0xDE96E0 |
| Move | Wait | progress == 1.0, no pending flags | 0xDF5990 → 0xDE9B50 |
| Move | Ledge / InAir | flag 86 or 4612 pending at anim end | 0xDF5990 → 0xDF3E20 / 0xDEA4D0 / 0xDEB840 |
| KnockedOff | InAir | anim done (`sub_501760`) | 0xDEE840 |
| Release | InAir | anim done | 0xDFA7E0 |
| any (events) | KnockedOff / State7 | HandleEvent 0xDFA880 (IHumanDamage). Event 2 in Wait → KnockedOff, event 3 → 0xDF5BE0, 4/5 → State7, 6 → 0xDE9770 | |

## 4. Per-frame logic (pseudo-code)

```
Update(pass):                                   // 0xDFE7F0
  if pass==0:
     if now >= deadline: sub_B1CA80(12)
     if !limbIK.hasValidHold(): SwitchContext(InAir, FillInAirData_LostGrip); return
  dispatch on state (table above)

Wait:                                           // 0xDFE760
  data.stickMagPrev = data.stickMag             // +0x2c = +0x30
  BuildHoldGrid(); found = ChooseMove(); if !found: OnNoMoveFound()
  if found & plain: StartMove; state=Move
  elif reach: StartReachMove; state=ReachMove
  elif release: StartRelease
  elif ledge flags: SwitchContext(Ledge)
  elif ladder: SwitchContext(Ladder)
```

### 4.1 Stick → direction (QuantizeStickDirection 0xDEB8D0)
`a = signedAngle(characterFacing, stickDir)` about up (`sub_55E570`). Sectors are 45° wide (22.5° = 0.3927 rad):
| a | dir | meaning |
|---|---|---|
| (-22.5°, 0) | 0 | Up (slightly left) |
| [0, 22.5°) | 1 | Up (slightly right) |
| (-67.5, -22.5] | 6 | Up-Left |
| (-112.5, -67.5] | 4 | Left |
| (-157.5, -112.5] | 8 | Down-Left |
| [22.5, 67.5) | 7 | Up-Right |
| [67.5, 112.5) | 5 | Right |
| [112.5, 157.5) | 9 | Down-Right |
| else, a<0 | 2 | Down (left) |
| else, a>=0 | 3 | Down (right) |
Stick is "active" when HumanClimbData+0x2c > 0 and |CurDirection| > 0.001 on some axis.
Dir remap table at 0x1A2CB90 {dir, alt1, alt2}: 4→(6,8), 5→(7,9), 6→(4,0), 7→(5,1), 8→(4,2), 9→(5,3). 0..3 have no alternates (10 = none).

### 4.2 Hold grid (BuildHoldGrid 0xDF6A40) — the "Grid settings"
- Frame: origin = midpoint of (hands mid, feet mid). Z = lowest foot. Axes = character facing (flattened) / right / up.
- Columns are 0.75 m wide. Count = 8 when the pose has hands in different columns (pose flag at 0x1A2CD78), x ∈ [-3.0, 3.0]. Otherwise 7, x ∈ [-2.625, 2.625].
- Rows are 0.6 m tall, starting at z = -1.5. Count = 8 (z up to 3.3) when hands are at different heights (flag 0x1A2CD74), otherwise 7 (z up to 2.7).
- Cell centre = (x0 + 0.375 + 0.75·c, 0, -1.2 + 0.6·r) in that frame.
- Candidates are first gathered in a direction-dependent box (local min/max). Depth is from minLimbY-1.0 to maxLimbY+0.5.
  Up: x ±0.75, z -0.3..(4.2|4.8)-0.9. Down: z -2.1..2.4. Left: x from grid left to 0.75, z -1.2..2.4. Right: mirrored.
- Each cell is probed against the guidance edges with `sub_11713A0` (radius 0.375, 1.0, vertical tol 0.3, max angle 45° (0.785), up=(0,0,1),
  offset (0,-0.2,0), step 0.1, 50 iterations). A hit counts only if it lands in the same cell. The probe stores position, orientation frame, guidance
  object and element index, and the "centred" flag (|dx|<=0.3, |dz|<=0.15).

### 4.3 Poses (table 0x1A2CD70, stride 0x60: {animStateId, rowsFlag, colsFlag, heightType, footColL, footRowL, footColR, footRowR, 4×vec4 IK offsets(all 0)})
Cells are FOOT cells. The hand of that side is the cell 2 rows higher (verified: StartMove sets IK limb 0/1 at row+2, limbs 2/3 at row, 0xDFA0C0. ComputeRootFromMove 0xDEC6E0 uses +16-cell offsets).
| pose | anim id | L foot | R foot | description |
|---|---|---|---|---|
| 0 | 0x12DA39F | (3,2) | (3,2) | both sides on one column, level ("1M" entry) |
| 1 | 0x12DA3A0 | (3,3) | (3,2) | same column, left side one row higher |
| 2 | 0x12DA3A1 | (3,2) | (3,3) | same column, right side higher |
| 3 | 0x12DA3A2 | (3,2) | (4,2) | sides one column apart, level ("2M" entry) |
| 4 | 0x12DA3A3 | (3,3) | (4,2) | apart, left higher |
| 5 | 0x12DA3A4 | (3,2) | (4,3) | apart, right higher |
| 7 | -1 | (2,2) | (3,3) | special (no anim) |

### 4.4 Move tables (SHORT 0x1A2D070, LONG 0x1A2D7F0; 24-byte entries {nextPose, dxL, dyL, dxR, dyR, animId}, index pose*10+dir)
nextPose 9..14 = redirect (9→dir4, 10→dir5, 11→dir0, 12→dir1, 13→dir2, 14→dir3, then re-lookup). 15 = no move. Empty entries are all zero.
SHORT (always tried; LONG is tried first when stick magnitude > 0.5):
```
pose0: up0 →1 L(0,+1) | up1 →2 R(0,+1) | dn2 →2 L(0,-1) | dn3 →1 R(0,-1) | left →3 L(-1,0) | right →3 R(+1,0)
       UL →4 L(-1,+1) | UR →5 R(+1,+1) | DL →5 L(-1,-1) | DR →4 R(+1,-1)
pose1: up →0 R(0,+1) | dn →0 L(0,-1) | left →4 L(-1,0) | right →4 R(+1,0) | UL redirect→left | UR →3 R(+1,+1) | DL →3 L(-1,-1) | DR redirect→right
pose2: up →0 L(0,+1) | dn →0 R(0,-1) | left →5 L(-1,0) | right →5 R(+1,0) | UL →3 L(-1,+1) | UR redirect→right | DL redirect→left | DR →3 R(+1,-1)
pose3: up0 →4 L(0,+1) | up1 →5 R(0,+1) | dn2 →5 L(0,-1) | dn3 →4 R(0,-1) | left →0 R(-1,0) | right →0 L(+1,0) | diagonals redirect to up/down
pose4: up →3 R(0,+1) | dn →3 L(0,-1) | left →1 R(-1,0) | right →1 L(+1,0) | UL →0 R(-1,+1) | DR →0 L(+1,-1) | UR→up0, DL→down2
pose5: up →3 L(0,+1) | dn →3 R(0,-1) | left →2 R(-1,0) | right →2 L(+1,0) | UR →0 L(+1,+1) | DL →0 R(-1,-1) | UL→up0, DR→down2
```
LONG (strong push):
```
pose1: up →2 R(0,+2) | down →2 L(0,-2)        (hand-over-hand 1.2 m)
pose2: up →1 L(0,+2) | down →1 R(0,-2)
pose3: left →3 both(-1,0) | right →3 both(+1,0) (shuffle both sides)
pose4: up →5 R(0,+2) | down →5 L(0,-2) | left/right →4 both ±1
pose5: up →4 L(0,+2) | down →4 R(0,-2) | left/right →5 both ±1
```
The animIds for these entries are forge object ids (0x19A05F1.., 0x19A36CF.., 0x1A267EC.., 0x1A26815.., 0x1A26833..). The full dump is in the scratchpad (`climbtab.py`).

### 4.5 ChooseMove (0xDFDE90), in priority order
1. dir = Quantize(stick). Rebuild the grid for dir. dirM = remap[dir].
2. `TryLadder(dirM)` → 2922.
3. If stick magnitude > 0.5: take the LONG entry. Accept it if nextPose != 15 && `IsGridMoveValid(entry, checkBody=1)` && !`sub_DF47E0`.
4. Take the SHORT entry (with redirect). If it is valid (`IsGridMoveValid`): try `TryTransitionToLedgeHang(dirM)` (→4613+2920). Else accept, except for down/down-diag moves that fail `DF47E0`.
5. Special fall-backs: DF4670 / DF28A0 → corner move (4612). `TrySideJump` near (4608), far (4609) (lateral capsule check 1.8 m, `sub_B2A410`).
   `TrySideLedgeGrab` (4610/4611 + 2920). `TrySideReach`/`TryReachOtherSurface` (→2916). `TryLedgeGrab` (2921).
   `TryReachLedgeAbove` (85). `TryDropToLedgeBelow` (86). `TryBackEject` (2917/2918, up dirs only).
6. None found → OnNoMoveFound 0xDF4410 plays a "blocked" anim per direction: up 0x4A84D7B4, left 0x4759A110, right 0x4A84D7B6, down 0x4A84D7B5. With no stick it holds the pose anim (blend 0.5).

`IsGridMoveValid` (0xDECD70): for each moving side the destination foot cell AND the cell 2 rows up (hand) must contain a hold. Then the
target root is computed from the 4 contacts (ComputeRootFromMove 0xDEC6E0 → `sub_B1CC20`). `sub_B2DF90` checks body clearance with two box queries
(0.375 box at mid-height, offset 0.05·side, then a 0.15×0.15×0.4 box at 1.4×up).

### 4.6 StartMove (0xDFA0C0)
- Sets the anim graph state to `pose[nextPose].animStateId` (`sub_501190(id, 1, params)`). The duration is the move anim's length (`sub_507650`), or 0.5 s with a 0.5 blend if animId is 0/-1.
- For each moving limb it sets the IK target (`sub_E55EF0(limb, holdPos+offset, holdFrame, guidanceObj, elemIdx)`). Limb 0 = L hand, 1 = R hand, 2 = L foot, 3 = R foot.
- Computes the destination root (0xDEC6E0) and starts root interpolation `sub_711130(..., duration)`. `sub_7113F0(human+80, dt)` advances it every frame.
  Movement between holds is therefore root-interpolated over the anim duration and animation-driven visually.
- Special moves use direction-indexed anim tables: side jump near 0x1A2CD10[dir], far 0x1A2CD3C[dir] (dirs 4..9 only).
  Corner moves use anims 0x1EB8BDA6 / 0x1E383899 / 0x1E38389A / 0x1EB8BDA7, chosen by dot(facing, -normal) > 0.7071.

## 5. Constants
| addr / where | value | meaning |
|---|---|---|
| 0xDF6A40 | 0.75 | grid column width |
| 0xDF6A40 | 0.6 | grid row height. Row origin -1.5, row centre offset +0.3 |
| 0xDF6A40 | ±3.0 / ±2.625 | grid half-width (8 / 7 columns) |
| 0xDF6A40 | 4.8 / 4.2 | grid height (8 / 7 rows) |
| 0xDF6A40 | 0.375, 0.3, 45° | probe radius, vertical tol, max edge angle |
| 0xDF6A40 | 0.3 / 0.15 | "centred hold" tolerances |
| 0xDEB8D0 | 0.3927 / 1.1781 / 1.9635 / 2.7489 rad | 22.5/67.5/112.5/157.5° sector borders |
| 0xDFDE90 | 0.5 | stick magnitude for LONG moves |
| 0xDFDE90 | 0.001 | stick dead-zone per axis |
| 0xDFA0C0 | 0.5 s | default move duration |
| 0xDF29E0 | 1.8 | side-jump clearance distance |
| 0xDF3300 | +1.2 z, blend 0.2 | lost-grip InAir setup |
| 0xDE97B0 | 0.2, 0.5 | enter blend times |
Tuning values are almost all **hard-coded**. HumanClimbData is runtime state (entry type/pose, stick, IK weights).

## 6. Interfaces
- Context switch: `sub_55F7E0(ctxMgr, newContextId, delegate{fillFn, 0, this, sub_DE4D50})`. It calls old module slot 12 (OnExit), then the delegate (fills the next module's *Data), then new module slot 11 (OnEnter).
  Climb → InAir 8 (0xDF3300, 0xDEAE30, 0xDE8EF0, 0xDF34B0), → Ledge 9 (0xDEEAC0), → Ladder 5 (nullsub).
- Base vftable slots (same for all modules): 2 Reset, 7 QueryInterface(15→IHuman*,22→IHumanDamage,23→IHumanTeleport), 8/9 GetData, 10 Init, 11 OnEnter, 12 OnExit,
  13 Update(pass 0 = main, 1 = post (IK `sub_E58EE0`)), 14 InitStateIds.
- Limb IK / hold component at Human+1328 (code 0xE55xxx–0xE58xxx): 4 limbs × 144 bytes {pos@+96, normal@+112, guidanceObj@+128, elem@+132}.
- Anim: `sub_7183F0(animComp)` + `sub_501190(stateId, flag, params, -1)` sets the anim graph state. `sub_501760(0)` = current anim finished.
- Guidance: `sub_11713A0` (edge probe), `sub_1171050`/`sub_11718B0` (other queries).

## 7. HumanLedge (hanging on ledges) — full section

Method: the update dispatch 0xDE3E40 is followed into every per-state update. The decision function `HumanLedge__Movement_ChooseAction` 0xDE29E0
and every action it calls were decompiled. The static initializer `HumanLedge__StaticInitTables` 0xDCC2A0 (it contains `rep stosd`) was emulated
with the same interpreter (`emu_init.py`, output `ledge_init.pkl`) to recover the anim-id tables. Names marked (h) are hypotheses.

### 7.1 Overview — how ledge hanging works
- The character hangs by **two hand contacts** on guidance edges, plus two foot contacts when wall-hanging. Contacts are kept in the limb-IK
  component at Human+1328 (4 limbs × 144 B) and mirrored into HumanLedge by `SyncLimbContacts` 0xDD0390.
- **Hang types** (`HumanLedgeData.LedgeHangType`, +0x68):
  - Wall (0): feet are on the wall. The root sits **0.5 m out from the wall and 1.1 m below the hands**.
  - Free (1): feet dangle. The root sits **2.4 m below the hands** (vector (0,0,2.4) / (0,0.5,1.1); `ComputeRootFromHandTargets` 0xDD6730, `TrySwitchHangType` 0xDE1060).
  - WallFree (2) is treated as Free in every table lookup (`if (t==2) t=1`).
- **Movement is discrete and animation-driven.** Each action:
  1. picks new hand (and foot) targets from guidance probes;
  2. sets IK targets (`sub_E56D50` / `sub_E55CB0`);
  3. computes a target root transform;
  4. plays an anim id and interpolates the root (`sub_711130` + `sub_7113F0(human+80)`) over the **anim length** (`sub_501CB0`).
  While a step runs, `HumanLedgeData.SubState` = 3 (**HandPlacement**). When progress (Human+264) reaches 1.0, SubState goes back to 1 (**Movement**) (0xDE29E0 top branch).
  There is **no speed constant for shimmying**. Speed is set by the shimmy animations and the step length.
- **There is no grip/stamina timer.** HumanLedge+0x6D8 (1752) is a qword deadline. Reaching it only calls `sub_B1CA80(12)` (`PreUpdate` 0xDCD8A0), the same as in HumanClimb.

### 7.2 Layout additions (HumanLedge object, ctor 0xDE0330)
| off | meaning |
|---|---|
| +0x120 (288) [5]×80 | lateral probe results (rows z = -1.8, -1.2, -0.6, 0, +0.6 relative to lead hand) — `ProbeLateral` 0xDD9640 |
| +0x210 (528) | same-level lateral candidate (row 3): pos, normal, … ; +592 its guidance object, +596 edge idx |
| +0x2B0.. (688/752/768/832) | climb-hold candidates below (feet/hands) used by `TryTransitionToClimb` 0xDD46A0 |
| +0x350 (848)/+0x390 (912) | vertical (up/down) hand candidate and its object |
| +0x3A0..+0x4D0 (928,1008,1088,1168) | 4 limb targets for ledge-to-ledge jumps (80 B each) |
| +0x570/+0x580 (1392/1408) | current L/R hand contact world pos. +1456/+1472 hand normals. +1424/+1440 feet |
| +0x5F0/+0x600 (1520/1536) | L/R hand **targets**. +1584/+1600 their normals. +1552/+1568 foot targets. +1616/+1632 foot normals |
| +0x670/+0x680/+0x690 (1648/1664/1680) | target root position / forward / up |
| +0x6F0.. (1776..1780) | guidance object of each current contact. +1792.. contact type (2 = attached to a guidance edge) |
| +0x710.. (1808..1828) | guidance object / edge index of each target |
| +0x734 (1844) | hand moved by the current step (0 = L, 1 = R) |
| +0x738 (1848) | **pending second vertical step** (1/2). While non-zero, the next frame forces the same up/down direction (`ContinuePendingVerticalStep` 0xDCF3C0) |
| +0x73C (1852) | last step direction |
| +0x740 (1856) | preference order for side transitions into climb holds |
| +0x744/+0x745 (1860/1861) | request flags. 1860: 0x02 drop to InAir, 0x04 back to hang, 0x08 → Climb ctx, 0x10 → Ladder ctx, 0x20 free-hang pull-up variant, 0x40 → NarrowObject ctx, 0x80 → to-climb sequence. 1861: 0x01 start ledge jump, 0x02 blended jump, 0x04 jump in progress, 0x08 debug toggle |
| +0x746 (1862) | "blocked upward" — stick up and nothing else possible. This is the precondition for pull-up (`CanPullup` 0xDE2270) |
| +0x74C..+0x754 (1868..1876) | ledge-jump parameters: distance (≥0.5 = long), type (0 up, 1, 2, 3 …), side |
| +0xA91/+0xA92 (2705/2706) | next / current internal state. +2780 + 3k = state-id table (ids 1..20, `InitStateIds` 0xDD1420) |
| HumanLedgeData +0x85 (bool) | **shimmy hand alternation flag**. It is toggled on every lateral step (`StartHandStep` 0xDDE0C0). 0 = lead hand moves next, 1 = trailing hand catches up |

### 7.3 Internal states
| id | state-byte | name (mine) | update | enter / notes |
|---|---|---|---|---|
| 1 | 2780 | **Movement** (hang idle + shimmy, SubState 1/3) | `StateMovement_Update` 0xDE38F0 | from SubState Movement / TransitionInFromClimb / ParallelJump |
| 2 | 2783 | **LedgeJump** (ParallelJump / jump up or sideways between ledges) | `StateLedgeJump_Update` 0xDDFC30 | `StartLedgeJump` 0xDDCE40 |
| 3 | 2786 | **ToLadder** | 0xDD2430: interpolate, then Ladder ctx 5 (fill 0xDD1050) | 0xDD8800 |
| 4 | 2789 | **PullDown** (from ground to hang) | `StatePullDown_Update` 0xDDFCF0 | `PullDown_Enter` 0xDDE4D0 (SubState 11) |
| 5 | 2792 | **Entry / grab reception** | `StateEntry_Update` 0xDE3570 (0xDE1FE0), → Movement when SubState==1 | SubState 0 |
| 6 | 2795 | reception end (h) | 0xDCF830: anim done → Movement | from SwingReception |
| 7 | 2798 | swing settle (h) | 0xDE06E0 | from SwingReception (0xDD0140) |
| 8 | 2801 | **Pullup** (climb onto top) | `StatePullup_Update` 0xDE39D0 / `Pullup_Tick` 0xDE2EE0 | event 0 (`HandleEvent_Movement` 0xDE36D0 → `Pullup_Start` 0xDDBE80). Also Grasp with GraspType HangKnee/HangWaist |
| 9 | 2804 | **HandPassOver** (vault over the ledge) | `StateHandPassOver_Update` 0xDE0720. HandPassOverSubState==PassOver and anim done → Ground ctx 4 (fill 0xDCFA40) | SubState 12 |
| 10 | 2807 | event-2 reaction (knock-off, h) | 0xDD24A0: flag 1860&1 → InAir | event 2 |
| 11 | 2810 | **HangWallReception** | 0xDE07D0 | SubState 6 |
| 12 | 2813 | **HangFreeReception** | 0xDCF880 (0xDCEE00) | SubState 7 |
| 13 | 2816 | **SwingReception** | 0xDD24F0 → 6 / 7 / 0xDCF8E0 | SubState 8 |
| 14,15 | 2819/2822 | **to wall-climb** sequence | 0xDDFD70 → 0xDE0870. End anim 514991600 → Climb ctx 10 (fill 0xDDFBF0). Anim 1238269524 → back to hang | flag 1860&0x80 (set by `TryFreeHangDropToClimb`) |
| 16 | 2825 | **Grasp** | `StateGrasp_Update` 0xDDFDB0 | SubState 13 |
| 17 (18,19) | 2828 | **SecondHandGrab** (one-hand → two-hand, h) | 0xDDFEC0 (phases 18/19, `SecondHandGrab_Reach` 0xDD7310) | after corner anims / Grasp |
| 20 | 2837 | event 4/5 reaction (h) | 0xDDAFF0: → InAir (fill 0xDD7B80) | events (0xDDB200/0xDDB310) |

Entry routing from `HumanLedgeData.SubState` is listed in `EnterCommon` 0xDE26D0 (see the table below in 7.9).

### 7.4 Movement state — the decision order (`Movement_ChooseAction` 0xDE29E0, every frame while SubState==1)
```
if SubState==3 (step running): advance root interp; when progress==1 → SubState=1 (or corner follow-up)
dir = QuantizeStickDirection()        // 0xDD1920: -1 none; |a|<=45° → 0 (up/forward); a in (-135,-45) → 2 (left);
                                      //            a in (45,135) → 3 (right); else 1 (down/back)
                                      // a = angle between stick and character facing (DirectionType==1: camera-relative variant)
if dir == -1: PlayIdleOrBlocked(); return          // idle anim 0x106F2E8, blend 0.5
LedgeSearch(dir)                                    // 0xDDC620 (RE/06 §4.5 box volumes)
ContinuePendingVerticalStep(&dir)                   // second hand of a 2-step climb
if dir in {up, down}:                               // vertical
   gather vertical candidates (0xDD8F70)
   TryTransitionToClimb(dir)      → Climb ctx (flag 8), ClimbData.EntryType=FromLedge   [0xDD46A0]
   (wall-hang feet test 0xDD1DC0) → TrySwitchHangType(dir)                               [0xDE1060]
   TryFreeHangDropToClimb(dir)    (down, free hang only)                                 [0xDDF390]
   StartHandStep(dir)             hand-over-hand up/down to a ledge 0.6–1.2 m away       [0xDDE0C0]
   TryJumpUpOrTurnCorner(dir)     up → TryJumpUpToLedge (1.2 m hop)                      [0xDE05E0 / 0xDD5E10]
   TryWallJumpUp(dir)             wall hang, up: ledge ~1 m above → blended jump         [0xDD62A0]
   if dir==up and nothing: blockedUp(+1862)=1  → enables Pullup event
else (left/right):                                  // lateral
   ProbeLateral(dir)                                                                     [0xDD9640]
   transitions to climb holds at the side (3 height offsets, ordered by +1856)           [0xDD4CD0, 0xDD48B0]
   TrySwitchHangType / StartHandStep(dir) = **shimmy step**                              [0xDE1060, 0xDDE0C0]
   TrySideJumpToLedge(dir, near/far)                                                     [0xDD3BB0]
   corner turn: TryJumpUpOrTurnCorner(dir) → 0xDD55F0 / 0xDDD490                          [TurnCorner]
   otherwise: PlayIdleOrBlocked → "blocked" anims 1478711130..133 by dir
```
Before the decision runs, the Movement state checks:
- `HasLostLedge` 0xDD20D0. A sphere query of r = 0.25 (+ half the hand spacing) at the hands must still find each hand's edge, else → InAir.
- `WantsLetGo` 0xDCD4D0. Both hands on edges + the input action hash -119042087 → **let go** → InAir (`LetGoToInAir`, fill 0xDD08F0).
- Pending flags: 1860&0x80 → to-climb, &0x08 → Climb ctx, &0x10 → Ladder ctx.
- `CornerAnimFinished` 0xDCD570 → SecondHandGrab.

### 7.5 Shimmy (lateral)
1. **Probe** (`ProbeLateral` 0xDD9640):
   - The lead hand is L for left and R for right. The roles are swapped when the alternation flag (+0x85) is set.
   - A sphere sweep of r = 0.15 runs along the move direction, length 1.55 m, from `leadHand - 0.15·dir + 0.75·back + 0.2·up`. It gives the free distance d (`sub_B18C30`).
   - If the flag is clear: `step = min(d - 0.4, 1.0 - |handL-handR|)`. **If d - 0.15 < 0.7 the shimmy is blocked** (obstacle or inner corner), and all candidates are cleared.
   - Five guidance-edge probes (`sub_1170A70`, half-length = step/2 + 0.1, radius 0.5, vertical tolerance 0.3, max angle 45°) run at lead-hand height offsets −1.8, −1.2, −0.6, 0, +0.6.
     The 0 row is the shimmy target. The others are for transitions into climb holds and side jumps.
   - If the flag is set (closing step), the trailing hand re-targets the chain (`sub_B18600`, the closest grab point on the current edge) when it is more than 0.3 m away.
   - A target is used only if it is at least 0.15 m from the current lead-hand position + 0.1·dir. That is the **minimum step**.
   Chain ends are therefore handled implicitly. Near the end of an edge the probe finds no edge further on, or a step shorter than 0.15 m, so no step is taken.
   The character then either turns a corner, jumps sideways or plays the blocked anim.
2. **Root target** (0xDD6730):
   - Free hang: root.z = max(hand.z) − 2.4.
   - Wall hang: root.z = handZ − 1.1 (minimum for left/right, chosen by direction), and the root is moved 0.5 m off the wall along the average hand normal.
   - The root forward is the average of the negated hand normals (if the two normals are opposite, cos < −0.985, the second is used).
3. **Step** (`StartHandStep` 0xDDE0C0):
   - Sets all 4 IK targets.
   - Picks the moving hand and anim via `PickShimmyHandAndAnim` 0xDCB520, then toggles +0x85.
   - Stores +1844/+1848/+1852 and plays the anim. If the a4 argument is set, the anim id becomes `(anim != 0x1A279B9) - 1903967203`, an alternate anim pair. Every caller seen passes a4 = 0.
   - Starts the root interpolation over the anim length. SubState = 3.

Shimmy anim tables (index = hang type, Wall/Free):
| table | condition | Wall | Free |
|---|---|---|---|
| 0x1A2C490 | flag 0, dir left → move L | 0x1B70B35 | 0x1A2490A |
| 0x1A2C4A8 | flag 0, dir right → move R | 0x1B70B37 | 0x1A2490C |
| 0x1A2C49C | flag 1, dir left → move R | 0x1B70B36 | 0x1A2490B |
| 0x1A2C4B4 | flag 1, dir right → move L | 0x1B70B38 | 0x1A2490D |
So the pattern is: the lead hand reaches out, then the trailing hand closes in, and the two alternate.
**NextHandToMove** in HumanLedgeData (+0x64) matches the `a2[1]` result (2 or 3) of these pickers (h).

### 7.6 Vertical hand steps, jumps, corners (first pass — the jump and corner names here are corrected in §7.6b)
- **Vertical step** (`PickVerticalHandAndAnim` 0xDCB3E0) is a 2-step hand-over-hand. The first hand moves, and +1848 is set to 1/2 so the next frame forces the second hand.
  Tables: up 0x1A2C4C0/4CC/4D8/4E4, down 0x1A2C4F0/4FC/508/514 (Wall/Free columns: 0x1B70FAA/0x1A279B9 … 0x1B7093D/0x1C32FA4).
- **TryJumpUpToLedge** 0xDD5E10 (wall hang only):
  1. Probe for an edge 1.2 m above the hands (r 0.4, 0.5, 0.3, 45°), then a second edge 1.2 m below that point.
  2. Run a body clearance sweep (r 0.35) from (root −1.1 z, 0.65 back) along 1.8·up.
  3. Set flag 1861|1 with jump type 0 → `StartLedgeJump`.
- **TryWallJumpUp** 0xDD62A0 (wall hang, up): box guidance query 1 m above (two box variants). Sets 1861|3 (blended jump).
- **StartLedgeJump** 0xDDCE40:
  - Blended variant: anim 1289129807 with a 4-way bilinear blend. Vertical weight = clamp(targetZ − rootZ − 2.0, 0, 1). Horizontal weight = clamp(horizontal distance, 0, 1).
  - Table variant: index `((type*2 + (dist>=0.5))*4 + side)`, base 0x1A2C780 (Wall) or 0x1A2C980 (Free). Each entry is {category, animStart, animLoop?, animEnd}, queued as a sequence. The categories 0..3 are listed in `ledge_init.pkl`.
  Landing is resolved in `StateLedgeJump_Update` 0xDDFC30 → Movement (0xDD8550), ToLadder, etc. **This is the ParallelJump / ReboundTransition path** (LedgeSubState 14 ParallelJump enters with SubState := HandPlacement).
- **TrySideJumpToLedge** 0xDD3BB0 (near = +2144 slot / far = +2544 slot):
  - Anims 510126209..216: 4 for Free hang, 4 for Wall, by left/right and near/far.
  - Wall hang finds foot placements (`sub_B16130`). Clearance capsule 1.1 m (wall) or 2.4 m (free) (`sub_B2A410`). SubState = 3.
- **TurnCorner**: lateral `TryJumpUpOrTurnCorner` → `sub_DD55F0` (outer corner, h) or `sub_DDD490` (inner corner, h). The direction vector is ±character right.
  Corner anim ids live in the 0x491228xx family. Their end is detected by `CornerAnimFinished` 0xDCD570 (ids 0x49122886/8E/E2/EA, 0x4C460376/7E). That leads to the SecondHandGrab state (17), which re-grabs the second hand ±0.25 m sideways, 2.4 m above the root (r 0.25, 0.15, 0.15).
  **Inner-corner detection** = the shimmy sweep being blocked (d − 0.15 < 0.7).
- **TrySwitchHangType** 0xDE1060: probes for wall foot holds (two probes 0.2 apart via 0xDE0A60).
  - Free → Wall when feet are found: anims 29566306/7, clearance 1.1 m with 0.5 m back.
  - Wall → Free when the feet lose the wall: anims 29562045/6, 29565306/8, clearance 2.4 m.
  Variants a3 = 1/2 rotate the probe ±90° (corners).

### 7.6b Ledge moves: corners, side jumps, jump-ups (verified 2026-10-01, supersedes parts of §7.6)
**Corrected names.** The functions were renamed in the IDB:

| Address | Old name | Real function |
|---|---|---|
| 0xDD3BB0 | "TrySideJumpToLedge" | **`HumanLedge__TryTurnCorner`** |
| 0xDDD490 | (unnamed) | **`HumanLedge__TrySideJumpToLedge`** |
| 0xDD55F0 | "outer corner" | **`HumanLedge__TrySideJumpToLadder`** |
| 0xDD5E10 | "TryJumpUpToLedge" | **`HumanLedge__TryJumpUpToClimb`** |
| 0xDE05E0 | "TryJumpUpOrTurnCorner" | `HumanLedge__TryJumpUpOrSideJump` |

`HumanLedge__FindCornerEdges` is 0xDD0600.

**Lateral decision order** (`Movement_ChooseAction` 0xDE29E0, stick left/right):
1. `ProbeLateral`, side moves to climb holds, hang-type switch, `StartHandStep` (the shimmy).
2. `FindCornerEdges(dir, inner)`, then side moves to climb holds.
3. **`TryTurnCorner(dir, inner)`**.
4. `TrySwitchHangType`.
5. `TryJumpUpOrSideJump` (ladder at the side 0xDD55F0, else **side jump** 0xDDD490).
6. `FindCornerEdges(dir, outer)`, then side moves to climb holds again.
7. **`TryTurnCorner(dir, outer)`**.
8. `TrySwitchHangType`, then the blocked idle.

**Up:**
1. Climb, then the hand step.
2. `TryJumpUpOrSideJump(up)` = `TryJumpUpToClimb`.
3. **`TryWallJumpUp`** (the hop).
4. Otherwise blocked-up (enables the pull-up).

**Corner search (`FindCornerEdges` 0xDD0600).**
- Base = hands' midpoint at the lower hand's height.
- Five guidance probes (`sub_1170A70`: radius 0.25, vertical 0.4 / 0.3, ≤ 50°, filter `dword_19341B0`) at base heights −1.8 … +0.6 in 0.6 steps. Results go to +1904 + 80k (inner) and +2304 + 80k (outer). Row 3 (same height) = **+2144 / +2544**.
- Inner corner: probe at base + 0.6·move − 0.6·facing; the new facing is the move direction.
- Outer corner: probe at base + 0.3·facing + 0.2·move; the new facing is −move.

**Corner turn (`TryTurnCorner` 0xDD3BB0).**
- Both hand targets go to the candidate point.
- Wall hang needs foot holds on the new wall (`sub_B16130`).
- Clearance capsule: 2.4 m (free) or 1.1 m (wall), via `sub_B2A410`.
- Root target from the hands (and feet), interpolated over the action length (`sub_711130`, flag 0). SubState 3.
- Actions (510126209 + k):

  | | left in | left out | right in | right out |
  |---|---|---|---|---|
  | Free hang | `0x1E67E881` `hangfree_corner_left_090_in` | `0x1E67E882` | `0x1E67E883` | `0x1E67E884` |
  | Wall hang | `0x1E67E885` | `0x1E67E886` | `0x1E67E887` | `0x1E67E888` |

  The wall-hang actions are two-item `hangwall_strafe_{left,right}_050cm_open` + `_close`.
- The end of a corner clip leads to SecondHandGrab, ±0.25 m (§7.6, `CornerAnimFinished` 0xDCD570).

**Side jump (`TrySideJumpToLedge` 0xDDD490).**
- Search start = the hands' midpoint (higher hand) + 0.9·move.
- `sub_B153C0` searches 1.6 m along the move (hand radius 0.35, 0.3 / 0.3) for a hand pair. Each (hand offset, foot offset) pair is tried in turn: (0, −1.2), (+0.6, −0.6), (−0.6, −1.8).
- Result type: hands + wall feet → **type 1**, climb holds → **type 0**, hands only → **type 2**.
- Clearance capsule r 0.35 up 1.8 (type 0), 1.0 (type 1) or 2.0 (type 2) m.
- Sets dist = clamp(|target − start|/1.6) (≥ 0.5 = long), side = 2 or 3, flag 1861 | 1.

**`StartLedgeJump` 0xDDCE40, table variant.**
- Entry at base + 16·((type·2 + long)·4 + side), base = wall 0x1A2C780 or free 0x1A2C980.
- Entry layout: {landing category, start, loop, end}. The table is filled by `StaticInitTables` 0xDCC2A0 and was read from `RE/data/ledge_init.pkl`; the full table is in `port/src/player/ledge_moves.rs`.
- Plays start → loop → end (`HumanClimb_Jumps`, e.g. `climbing_hangwall_tr_hangwall_left_2_{a,b,c}`; "_2" short / "_3" long).
- Landing (`StateLedgeJump_Update` 0xDDFC30, when the action finishes):

  | Category | Result |
  |---|---|
  | 0 | Climb context (0xDD8CF0) |
  | 1 / 2 | Hang on the new ledge (0xDD8550 → Movement) |
  | 3 | Ladder (0xDD8800) |

- Up entries exist only for wall hang, type 0 (jump up to climb holds).
- Measured clips: a short wall→wall side jump is 0.20 s + 0.33 s (1.5 m sideways) + 0.60 s.

**Jump up to climb holds (`TryJumpUpToClimb` 0xDD5E10).**
- Wall hang only, no pending step.
- A hold row 1.2 m above the hands and another 1.2 m below that (r 0.4, 0.5 / 0.3, 45°, filter `dword_193419C`).
- Climb pose from `sub_B1CC20`; clearance r 0.35 from (root − 1.1 z, 0.65 back) up 1.8 m.
- Table jump type 0, side up, short.

**Hop up (`TryWallJumpUp` 0xDD62A0 → blended `StartLedgeJump`).**
- Wall hang, up. A box guidance query `sub_1171050` about 1 m out, two box variants. Its arguments are only partly recovered: **hypothesis**, the port searches 1.3–1.9 m above the hands within 0.6 m.
- Plays `0x4CD68F4F` `hangwall_to_swingback_up_{min,max}_{200,300}_a` with weights:

  | Clip | Weight |
  |---|---|
  | min_200 | (1−h)(1−v) |
  | min_300 | (1−h)v |
  | max_200 | h(1−v) |
  | max_300 | hv |

  v = clamp(targetZ − rootZ − 2.0) and h = clamp(horizontal hand distance).
- Then `HumanLedge__FinishBlendedJump` 0xDDB170 → `PlayHopUpSecondPart` 0xDDAB00 plays `0x4CD68F50` (`…_b`) with the same weights. The root is interpolated to the new hang over `_b` (`sub_711130` flag 0).
- Sets **LedgeHangType = Free**, SubState 0, +108 = 3 and +112 = w2 + w3: the hop ends swinging in a free hang. `_b`'s transition leads to the free-hang idle `0x012719F1`.

**Port (`port/src/player/ledge_moves.rs`).** The port has inner and outer corners, hang→hang side jumps (types 1/2) and the hop. Departures:
- **Ledge-jump root path:** the root follows the clips' displacement plus a linear correction (**hypothesis**; the interpolation for table jumps is not traced). Corners and the hop use the interpolator.
- **Second hand:** after a corner the hands are re-spread to the normal spacing (**hypothesis** for SecondHandGrab).
- **Not ported yet:** the ladder and climb-hold side jumps (types 0/3), `TryJumpUpToClimb`, and the free→wall switch after a hop.

### 7.6c Hang-type switching (`HumanLedge__TrySwitchHangType` 0xDE1060, verified 2026-10-01)
The hang type (HumanLedgeData +104: 0 wall, 1 free) **changes only through moves**. The switch is itself a step that carries the hands to the step's target.

**Decision layer** (0xDE29E0):
- For up/down with a vertical candidate (+848 / +912) and for left/right with the lateral candidate (+528 / +592), `HumanLedge__QuickWallTestForStep` 0xDD1DC0 runs first. It places the picked step clip's foot with a sphere query (r 0.25, 0.75, 0.15, ±0.15 m random jitter).
- `TrySwitchHangType(dir, 0)` is tried before the hand step when that test disagrees with the current type (free and wall found, or wall and none found).
- After the corner searches it is also tried with the inner (1) or outer (2) corner candidate. That frame is rotated ±90°.

**Preconditions:** no pending vertical step; lateral moves are refused when LedgeData +133 == 1; the target edges +1808 / +1812 exist.

**Feet (`HumanLedge__ProbeFootSupport` 0xDE0A60):** two collision rays (layer 43) of **1.2 m toward the wall**. They start at the target − 1.0 m up − 0.5·facing ∓ 0.1·side, with the second ray 0.2 m further to the side. A hit on a non-ladder entity is a foot support.

| Current | Rule | Actions | Root / checks |
|---|---|---|---|
| Free | **any** foot → wall | `0x01C32562` `hangfree_tr_hangwall_left` (left) / `0x01C32563` `…_right` (any other direction, including up/down) | Root with feet (`sub_B157B0`), check `sub_B2DC80`, clearance 1.1 m. +104 := 0, foot IK targets set |
| Wall | **unless both** feet → free | `0x01C314BD` `hangwall_tr_hangfree_left`, `0x01C314BE` `…_right`, `0x01C3217A` `…_up_left`, `0x01C3217C` `…_down_left` | Root from hands only (`sub_B15AD0`), check `sub_B2DD50`, clearance 2.4 m. +104 := 1 |

The root is interpolated over the action (`sub_711130`, flag 0). SubState 3.

**Port** (`ledge.rs` `feet_on_wall` / `needs_switch`, `ledge_moves::switch_move`):
- The hang type is state. It is evaluated on entry, then changed only by moves.
- The foot rays are ported. A shimmy or vertical step whose destination needs the other type becomes the switch move.
- The decision layer's quick test is replaced by the foot rays at the step's destination (**hypothesis**: same outcome on static geometry).
- The corner-candidate variants are not used.

### 7.7 Pull-up / climbing onto the top
- **Trigger**: event 0 in `HandleEvent_Movement` 0xDE36D0, sent from outside the module (decision layer). It is accepted when `CanPullup` 0xDE2270 returns 2:
  - **blockedUp** (+1862: the stick is up and no other vertical action exists);
  - hold valid;
  - SubState == Movement;
  - neither hand on a contact of type 2/3;
  - for free hang, special case `sub_DE0E30` (sets flag 0x20);
  - free standing space: `sub_B2E240` capsule test at the hands' midpoint, raised by (0, 0.5, 2.4) for free hang or (0, 0.5, 1.1) for wall hang;
  - a top-edge probe (r 0.25, 0.35, 0.25, 45°), with a standing test on the top edge.
  The hands' contacts are then snapped onto that edge.
- **Start** (`Pullup_Start` 0xDDBE80):
  - Wall hang: anim 0x106D2C5 (or 957955002 when `sub_DD1120` is true — a variant, h). It is blended by ledge slope: 2-way blend over ±30° (0.5236) up-slope / 45° (0.7854) down-slope.
    The root is interpolated to hands-mid − 0.5·forward.
  - Free hang: anim 0x12719F2 (+0x386E3B13 variant). The root goes to (hands-mid − 2.4 z) and then to hands-mid − 0.5·fwd − 1.0 z.
  - SubState = 4 (Pullup).
- **During** (`Pullup_Tick` 0xDE2EE0): the anim graph decides the outcome.
  - Ends with the stand-on-ledge anim → flag 0x40 → **NarrowObject context 12** (standing on the ledge edge). NarrowObjectData +16/+32 = edge points, +48/+52 = half-width.
  - Ends with a jump-off anim → flag 2 → **InAir** (fill nullsub, blend 0.2).
  - Ends with the idle anim 0x106F2E8 → flag 4 → back to hang.
  - The standing-on-ground outcome is reached via the NarrowObject/Ground module.
- **Grasp**: when the character grabs a ledge at **knee/waist height** (GraspType HangKnee=0 / HangWaist=2), the Grasp state goes straight to Pullup at progress 1 (0xDDFDB0). This is the "climb over a low ledge" behaviour.

### 7.8 PullDown (ground → hang) and HandPassOver
- PullDown is entered from Ground with LedgeSubState 11 (`PullDown_Enter` 0xDDE4D0).
  - Anim = table 0x1A2C3F0[PullDownSide + 8·PullDownType]. Ground_Soft_EdgeStop/Front = 0xB5EBD80B, Ground_Soft_Wait = 0x06E8BD80/0x082F8C53, Beam_Soft = 0x516D5CF9..0x516D5D05 (all sides).
  - Type 4 (HandPassOver) uses anim 161527367/161527369, blended by data+0x134.
  - Side Left/Right rotates the facing by ∓90°.
  - The root moves to the ledge point + 0.5·edge normal, offset by the wall-hang vector (0, 0.5, 1.1). For HandPassOver it is pulled back by 0.3·(1 − w).
  - PullDownSubState = Orientation. The update (`StatePullDown_Update` 0xDDFCF0) ends in Entry (state 5), or in **InAir when PullDownSubState == 4 ReleaseToInAir** (fill 0xDDA100).
- HandPassOver (state 9, SubState 12) is the vault over a ledge without hanging. HandPassOverSubState PassOver + anim done → **Ground context 4**.

### 7.8a Ledge stop and look-down (HumanGround, verified 2026-10-02)
**Edge report** (event payload, also used by pull-down): point +16, outward normal +32, drop +48, +52, flag +56, distance limit +64, wide flag +68.

**`HumanGround__ClassifyEdgeSide` 0xD9D7F0** returns 0 (no edge) when flag +56 is set, the drop is ≤ **1.3 m**, or the signed squared horizontal distance to the point (negative behind the edge) exceeds +64. Otherwise it classifies the angle between the facing and the flattened normal:
- **1 front:** |a| < 60°, or < 120° with the wide flag +68;
- **3:** −120° < a ≤ −60°; **4:** 60° ≤ a < 120°;
- **2 back:** otherwise.

**Ledge stop: Movement event 69** (`Movement_HandleEvent` 0xDB1470):
- **Side edges:** guard `Guard_LedgeStopSide` 0xDA5D90 (side 3 / 4) → 0xD8E5B0. That is another state, not traced.
- **Front edges:** guard `Guard_LedgeStopFront` 0xDA5DE0. It needs side 1, **drop > 2.0 m**, the body check `sub_B2E4F0` (0.5, 2.0), and `sub_C7F150` clear. It leads to `ToLedgeStop` 0xDA99F0, which aligns the facing to the normal when +68 is set.
- **Clips:** `HumanGround__LedgeStop_Enter` 0xD93C60 (sub-state 38) plays `0x06E8BD7F` `xx_h_ledge_stop_start_footl` (0.27 s, no root motion). When it is done, `LedgeStop_PlayEnd` 0xD7D9D0 plays `0x06E8BD7D` `xx_h_ledge_stop_end_footl` (0.67 s, root steps back 0.50 m). Its transition 0x06E8BD7E goes to wait. When the end action is done, `Locomotion_Update` 0xDAF2D0 returns to Movement.
- **Pull-down:** while the start action plays, `LedgeStop_HandleEvent` 0xDA4D90 accepts event 70, giving the EdgeStop pull-down (§7.8b).

**Look-down:** event **119** in another ground sub-state handler (0xDA7250; guard 0xD7E590 needs a mode value == 1, **hypothesis**: the LookDown ability) leads to `ToLookDown` 0xDA5260. It copies the report to +1920.., then `HumanGround__LookDown_Enter` 0xD9FC80 (sub-state 9):
- **Angle blend:** signed angle a between (point + normal − position) and the facing, clamped to ±90°. Weights are front = 1 − |a|/90°, and right (a ≥ 0) or left (a < 0) = |a|/90°.
- **Action:** `0x2669E0F7` (`xx_l_ledge_lookdown_{front,left,right}_footl`) or `0x2669E0F8` (`_footr`) by the leading foot.
- **Event:** posts event 62 with the edge.

**Port** (`ground.rs` `edge_report` / `ledge_stop`):
- Front ledge stop with the game's actions and timing.
- EdgeStop pull-down from it (Legs during the start action).
- **PORT:**
  - The trigger is walking (low profile) into a front edge within 0.45 m with more than 2 m of drop. The sender of event 69 is not traced.
  - The feet stay 0.2 m behind the edge.
  - After a stop, pushing on into the same edge holds the character there, until the stick is released or turned away.
- **Not ported:** side ledge stops and the look-down (event 119's sender and guard are unknown).
- `AC_AUTOPILOT=ledgestop`.

### 7.8b PullDown in full (verified 2026-10-01)
**Request:** event **70** from the decision layer (the input mapping is not traced), with an edge report. The report holds the point at +16, the outward normal at +32, the drop height at +48 and a flag at +56.

| Where | Guard | Fill | Type |
|---|---|---|---|
| Ground Movement (`Movement_HandleEvent` 0xDB1470) | `HumanGround__Guard_PullDownWait` 0xD9D6C0 | `FillPullDown_Wait` 0xD843E0 | **Wait (1)** |
| Ledge-stop sub-state handler 0xDA4D90 | `HumanGround__Guard_PullDownEdgeStop` 0xD9D580, which also needs the ledge-stop action `0x06E8BD7F` playing | `FillPullDown_EdgeStop` 0xD84360 | **EdgeStop (0)** |

Both guards require:
- the report flag (+56) clear (Wait) and the playing item not locked (+60 & 0x20);
- **facing along the outward normal** (forward · n > 0);
- **drop > 2.0 m** (+48);
- the body check `sub_B2E4F0`.

The fills copy the report to LedgeData +240 (point +256, normal +272), set the type (+64), side Front (+68) and LedgeSubState **11**.

**Stages** (`PullDown_Enter` 0xDDE4D0, `PullDown_Update` 0xDDE980, `StatePullDown_Update` 0xDDFCF0; PullDownSubState at LedgeData +84). Table `dword_1A2C3F0[side + 8·type]` = orientation, `[+4]` = descent. The ground types have front entries only; left/right rotate the facing ∓90°. Type 4 HandPassOver uses `0x09A0B647/49`, then `0x09A0B648/4A`, blended by +308.

| Stage | Action | Root target (interpolator over the action) | Other |
|---|---|---|---|
| 1 Orientation | `0xB5EBD80B` `ledge_stop_start_footl_pulldown_front_orientation` (EdgeStop) or `0x06E8BD80` `ledge_lookdown_front_pulldown_front_orientation` (Wait) | **p + 0.5·n**, facing −n | Limb IK on |
| 2 Descent | `0x082F8C53` `ledge_pulldown_soft_front` | **p + 1.0·n − 0.8 m** | First, two guidance probes (`sub_1170A00`, ±0.3 / 0.7) find the hands at p ∓ 0.25·side; **none → ReleaseToInAir** (stage 4 → InAir, fill 0xDDA100) |
| 3 Reception, foot support (`sub_B16130`) | `0x06E8BD81` (115916161) `pulldown_soft_to_hangwall_{straight,30_out,45_in}_a`, then `0x082F820C` `_b` + `_tr`. Weights by the signed wall angle: ≥ 0 → straight/30 out by angle/30°, < 0 → straight/45 in by −angle/45° | Wall-hang root (`sub_B157B0`) | Hang type 0 |
| 3 Reception, no support | `0x082F9F3B` (137338683) `pulldown_soft_front_to_hangfree_a`, then `0x082F9F3C` | Free root (`sub_B15AD0`) | Hang type 1 |

When the reception action is done (`PullDownReceptionDone` 0xDCDAA0), PullDownSubState → 0 and the state goes to Entry, i.e. hanging.

**Port** (`ledge_moves::pulldown`, three queued ledge moves; `ground.rs` `try_pulldown`):
- Type Wait, front side; reception weights straight.
- **PORT trigger:** Legs in low profile, facing an edge within 0.5 m with a drop over 2 m.
- Not ported: EdgeStop (no ledge-stop state yet), the side variants, beam pull-downs, HandPassOver, and the release into InAir when no hands are found (the port then simply doesn't pull down).
- `AC_AUTOPILOT=pulldown`.

### 7.9 Entry routing (`EnterCommon` 0xDE26D0, from HumanLedgeData.SubState)
| SubState in | state | extra |
|---|---|---|
| 0 Entry | 5 Entry | |
| 1 Movement | 1 | |
| 4 Pullup | 8 | |
| 6 HangWallReception | 11 | |
| 7 HangFreeReception | 12 | |
| 8 SwingReception | 13 | `+1840 = 0` |
| 9 TransitionInFromClimb | 1 | 0xDCDA10 sets contacts from climb |
| 11 PullDown | 4 | |
| 12 HandPassOver | 9 | |
| 13 Grasp | 16 | |
| 14 ParallelJump | 1 | LedgeHangType := (data+0x138 != 1), SubState := 3 HandPlacement |
Other entry work:
- Common setup: blend 0.2 (skipped for HandPassOver); data+0x85 cleared; +1844/+1848 = 0; +1852 = -1.
- If the current anim is not a known pull-down anim and SubState is not in {11, 4, 13, 0, 6, 8}, it calls `sub_DE1970(1)` (re-pose).

### 7.10 Swing / receptions
- SwingReception (13): Human+2704 is reset. `sub_DCFE90` → state 6, `sub_DCD300` → state 7 (`sub_DD0140`), `sub_DCD260` → 0xDCF8E0.
  **SwingStrength** (data +0x70) is written by the caller (InAir) and read by these helpers (h). Not decoded further.
- HangWallReception (11): interpolate until progress 1 → Movement when `sub_DCC190`. On anim event bit 4 → 0xDCF8B0.
- HangFreeReception (12): 0xDCEE00 drives it until SubState == 1.

### 7.11 Exits summary
| exit | to context | where |
|---|---|---|
| lost ledge / let go / pull-up jump-off / PullDown ReleaseToInAir / damage | InAir 8 | 0xDE3E40, 0xDD1550, 0xDCE780, 0xDDFCF0, 0xDDAFF0, 0xDD24A0 |
| down/side onto climb holds | Climb 10 (ClimbData.EntryType = FromLedge) | 0xDD46A0 → 0xDCE6B0; 0xDE0870 |
| ladder | Ladder 5 | 0xDD2430, 0xDCE6E0 |
| pull-up onto ledge | NarrowObject 12 | 0xDD8D50 |
| HandPassOver vault | Ground 4 | 0xDE0720 |

### 7.12 Ledge constants
| where | value | meaning |
|---|---|---|
| 0xDD6730, 0xDE1060 | 1.1 / 0.5 | wall-hang root offset below hands / out from wall |
| 0xDD6730, 0xDD7310 | 2.4 | free-hang root offset below hands |
| 0xDD9640 | 0.15 r, 1.55 len, 0.75 back, 0.2 up | shimmy free-space sweep |
| 0xDD9640 | 0.7 / 0.4 / 1.0 | min free distance / wall margin / max hand spacing |
| 0xDD9640 | −1.8 + 0.6k (5 rows), 0.5, 0.3, 45° | lateral probe rows and probe params |
| 0xDD9640 | 0.15 / 0.3 | minimum step / trailing-hand re-snap distance |
| 0xDD1920 | 45°, 135° | stick sectors |
| 0xDD20D0 | 0.25 + ½ hand spacing | lost-ledge check radius |
| 0xDD5E10 | 1.2, 0.35, 0.65, 1.1, 1.8 | jump-up probe height, body capsule r, offsets |
| 0xDDCE40 | 2.0 | jump blend height offset |
| 0xDE2270 | (0,0.5,2.4)/(0,0.5,1.1), 0.25/0.35/0.25 | pull-up standing test / top-edge probe |
| 0xDDBE80 | 0.5236 / 0.7854 | pull-up slope blend range |
| 0xDD3BB0, 0xDD48B0 | 1.1 / 2.4 / 1.8 | lateral clearance (wall / free / climb holds) |
| 0xDD7310 | 0.25, 0.15 | second-hand regrab offset / probe |
All animation ids are in the scratchpad dump (`ledge_init.pkl`, tables 0x1A2C3F0–0x1A2CB80).

### 7.13 Open questions (ledge)
- Decompile `sub_DD55F0` / `sub_DDD490` to confirm inner vs outer corners and their geometry.
- Who sends Ledge event 0 (pull-up) and with what input? Probably HumanDecision on the "up + action" input.
- Map the jump table categories (0x1A2C780) to LedgeSubState ParallelJump / ReboundTransition.
- Decode SwingStrength usage (0xDCFE90 / 0xDCD300 / 0xDCD260).

## 8. Open questions / dynamic checks
- Confirm the meaning of climb HumanClimbData +0x2c/+0x30 (stick magnitude?). Breakpoint at 0xDFE577.
- Confirm the left/right sign of direction sectors 4/5 (dir 4 moves the left side by -1 column, so it is "left" if the local x axis points right).
- Name states 3, 7 and the IHumanDamage event ids (2..6) in HandleEvent 0xDFA880.
- Is +2910 (legacy mode) ever set? Watch-write on HumanClimb+0xB5E.
- HumanLedge: per-state logic, shimmy speed, corner turning, pull-up conditions, ledge-detection probes.

## 9. Renamed functions
HumanClimb: 0xDF9E40 ctor, 0xDF5CC0 dtor, 0xDFA0A0 scalar_deleting_dtor, 0xDE4780 Reset, 0xDE4740 QueryInterface, 0xDE4B30/0xDE4B20 GetClimbData(2),
0xDE4F50 Init, 0xDF5EA0/0xDEA410 OnEnter, 0xDE97B0 EnterCommon, 0xDF5EB0 OnExit, 0xDE9A00 ExitCleanup, 0xDFE970/0xDFE7F0 Update, 0xDEB7F0 InitStateIds,
0xDFE760 StateWait_Update, 0xDF5990 StateMove_Update, 0xDF5A70 State3_Update, 0xDEE840 StateKnockedOff_Update, 0xDFA7E0 StateRelease_Update, 0xDF5AE0 State7_Update,
0xDFE510 WaitPreUpdate_BuildGridAndChoose, 0xDFDE90 ChooseMove, 0xDEB8D0 QuantizeStickDirection, 0xDF6A40 BuildHoldGrid, 0xDECD70 IsGridMoveValid,
0xDFA0C0 StartMove, 0xDEC6E0 ComputeRootFromMove, 0xDE4F80 StaticInitTables, 0xDE7FA0 CanStartGridMove, 0xDE8020 CanStartReachMove,
0xDE8070 WantsLedgeContext, 0xDE80F0 WantsLadderContext, 0xDF3DF0 SwitchToLedgeContext, 0xDE96E0 SwitchToLadderContext, 0xDE96A0 StartRelease,
0xDFA880 HandleEvent, 0xDE8F30 DebugTweakProbe, 0xDE8150 UpdateMoveInterp, 0xDF10C0 TryLadder, 0xDF4BA0 TryTransitionToLedgeHang, 0xDF29E0 TrySideJump,
0xDF22C0 TrySideLedgeGrab, 0xDF0980 TryLedgeGrab, 0xDF1730 TryReachLedgeAbove, 0xDF1D20 TryDropToLedgeBelow, 0xDF2F50 TryBackEject,
0xDF9B30 TryReachOtherSurface, 0xDF2EC0 TrySideReach, 0xDF4410 OnNoMoveFound, 0xDF6810 StartReachMove, 0xDEA500 EnterKnockedOff,
0xDF5B60/0xDF5BA0 EnterState7_A/B, 0xDE91B0 State7_Enter, 0xDF3300 FillInAirData_LostGrip (+thunk 0xDF3C10). All are prefixed `HumanClimb__`.
HumanLedge (all `HumanLedge__`): 0xDE0330 ctor, 0xDE3E40 Update (+thunk 0xDE45A0), 0xDE3460 OnEnter (+thunk 0xDE38E0), 0xDE26D0 EnterCommon, 0xDD1420 InitStateIds, 0xDCD8A0 PreUpdate, 0xDD20D0 HasLostLedge, 0xDE38F0 StateMovement_Update, 0xDE29E0 Movement_ChooseAction, 0xDD1920 QuantizeStickDirection, 0xDCF3C0 ContinuePendingVerticalStep, 0xDD9640 ProbeLateral, 0xDD6730 ComputeRootFromHandTargets, 0xDDE0C0 StartHandStep, 0xDCB520 PickShimmyHandAndAnim, 0xDCB3E0 PickVerticalHandAndAnim, 0xDE1060 TrySwitchHangType, 0xDD3BB0 TrySideJumpToLedge, 0xDD3A10 PlayIdleOrBlocked, 0xDD0390 SyncLimbContacts, 0xDD46A0 TryTransitionToClimb, 0xDE05E0 TryJumpUpOrTurnCorner, 0xDD5E10 TryJumpUpToLedge, 0xDD62A0 TryWallJumpUp, 0xDDF390 TryFreeHangDropToClimb, 0xDDCE40 StartLedgeJump, 0xDD48B0 TrySideMoveToClimbHolds, 0xDCD4D0 WantsLetGo, 0xDCD570 CornerAnimFinished, 0xDDFC30 StateLedgeJump_Update, 0xDD2430 StateToLadder_Update, 0xDDFCF0 StatePullDown_Update, 0xDDE4D0 PullDown_Enter, 0xDE3570 StateEntry_Update, 0xDCF830 State6_Update, 0xDE06E0 State7_Update, 0xDE39D0 StatePullup_Update, 0xDE2EE0 Pullup_Tick, 0xDCBF70 Pullup_EnterBlend, 0xDDBE80 Pullup_Start, 0xDE2270 CanPullup, 0xDE0720 StateHandPassOver_Update, 0xDD24A0 State10_Update, 0xDE07D0 StateHangWallReception_Update, 0xDCF880 StateHangFreeReception_Update, 0xDD24F0 StateSwingReception_Update, 0xDDFD70/0xDE0870 StateToClimbA/B_Update, 0xDDFDB0 StateGrasp_Update, 0xDDFEC0 StateSecondHandGrab_Update, 0xDDADE0 SecondHandGrab_Enter, 0xDD7310 SecondHandGrab_Reach, 0xDDAFF0 State20_Update, 0xDE36D0 HandleEvent_Movement, 0xDE3BE0 HandleEvent, 0xDCC2A0 StaticInitTables, 0xDD1550 LetGoToInAir, 0xDCE6B0 SwitchToClimbContext, 0xDCE6E0 SwitchToLadderContext, 0xDD8D50 SwitchToNarrowObjectContext, 0xDCE780 PullupToInAir.
