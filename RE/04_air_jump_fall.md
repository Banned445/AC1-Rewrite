# 04 — HumanInAir: jumps, free fall, landing, Leap of Faith, air catch

Source: AssassinsCreed_Dx9.exe (Steam 1.02), IDA session `ac1`. All addresses are VAs. Unverified items are marked **(hypothesis)**.

## 1. Summary

AC1 jumps are **not ballistic**. A jump is animation-driven and aimed at a target:

- Before the jump, the module that starts it (HumanGround free-run, Ledge, Climb and others) picks a **jump target** (a 96-byte JumpTarget struct holding a position, normal, type flags and an optional entity).
- `Human::SetupJumpToTarget` (0xB20200) picks the takeoff, air and landing animations. It writes them into **HumanInAirData**, the per-module runtime context (it is not a tuning table).
- While the jump plays, `HumanInAir__UpdateJumpMotion` (0xE0DEF0) moves the character by the animation's root motion. It adds a **linear correction** `(target − animEndPos) · (t / duration)`, so the animation ends exactly on the target. This is the famous "snap".
- When the animation finishes within **0.01 m** of the target, `CheckJumpTargetArrival` (0xE07D00) hands off to the context the target type names: Ledge, Beam, Pole, Ladder, Horse, HayStack, Ground and so on.

Real physics is used in only two cases:

1. **Overshoot / free-fall tail.** When the target is more than 5 m below the start (3 m for Leap of Faith), the jump is aimed at a point 5 m (or 3 m) down. Ballistic integration then takes over:
   - gravity is a hard-coded **9.8 m/s²**;
   - velocity is stored in metres per frame, with a fixed **dt = 1/30**;
   - each frame does `vz -= 0.326·dt`, which equals g·dt²;
   - horizontal velocity is steered so the character lands on the real target, capped at **15 m/s**.
2. **Drop from a hang or climb (sub-state 4) and plain falls.** These use the character controller's own gravity, which is not part of this module.

Landing is classified by **fall height**, measured from the apex. The apex is the position where vertical velocity first went below −0.001.

- **Landing type and damage** come from `ComputeLandingType` (0xE00FE0):
  - default thresholds: heavy above 6.3 m, fatal above 7.0 m (the small-damage threshold defaults to FLT_MAX, so it never triggers);
  - fixed table, used when flag `this+226 & 8` is set: 8 / 10 / 12 / 14 / 16 / 18 / 20 m steps.
- **Landing animation** is chosen from the total drop from the jump start:
  - 3 m or less: soft landing;
  - more than 3 m: roll, plus a camera shake.
- **Catch landing** (`CheckAirCatch`): a fall height of 9 m or more sends the character into the rag-fall state.

## 2. Object layout

### HumanInAir (size ≥ 0x2C0; constructor 0xE0FE80)

| off | meaning |
|---|---|
| +0 | vftable 0x17010B4 (16-slot base). Slot 11 = OnEnter 0xE09BA0, 12 = OnExit 0xE07120, 13 = Update 0xE0FDD0, 14 = InitSubStateTable 0xE019C0 |
| +8 | Human* |
| +0xC | Human transition/setup-data owner (`sub_B2FCxx(this+12)` returns per-context setup structs) |
| +0x10 | HumanInAirData* (the "Data" below) |
| +0x14 | IHumanInAir vftable 0x1701064 |
| +0x18 | IHumanDamage vftable 0x1700FF4 |
| +0x1C | IHumanTeleport vftable 0x1700FE8 |
| +0x20 | input/catch direction (vec4); set by IHumanInAir slot 8 `SetInputDirection` 0xE10460 |
| +0x30 | smoothed catch direction; lerp rate 7/s (0xE0BB70) |
| +0x40 | slope-contact start position (z at +0x48); used by GetSlopeSlideDrop 0xE03A20 |
| +0x50 | free-fall velocity, **metres per frame** (0xE0DEF0) |
| +0x88 | timer: reception sub-state end (0xE003F0) |
| +0xA0 | timer: drop-phase start (0xE064C0) |
| +0xC8, +0x68 | timers compared with the global clock `qword_1A1E7B0` |
| +0xE0 (224) | result flags. 1 Ledge, 2 Narrow, 4 Riding/Horse, 8 Pole, 0x10 Ladder, **0x20 grab requested** (slot 7 `SetGrabRequested` 0xE102D0), 0x40 Ground, 0x80 Ragdoll |
| +0xE1 (225) | flags. 1 RagdollGround, 2 → sub-state Reception, 4 → sub-state RagFall, 8 swing target, 0x10 air-assassination decided, 0x20 Ground (target type 0x8000), 0x40 HayStack, 0x80 external (slot 6) |
| +0xE2 (226) | flags. 1 anim-chain switch done, 2 human collision handled, 4 air-assassination kill pending, **8 use fixed fall-damage table** (slot 13), 0x10 steep-slope contact |
| +0xE4 / +0xE8 | swing-entry side / orientation class 0..3 (0xE07D00 case 0x400; cos 10° = 0.9848) |
| +0xF0 | copy of the jump target (96 bytes) |
| +0x150 | haystack entity handle |
| +0x154 | haystack side-entry flag (contact normal z ≤ 0.9) |
| +0x158 | int, set by slot 11 (0xDFEEB0); selects anim variant 32537012/3 |
| +0x15C / +0x160 | blend progress / blend duration (≤ 0.2 s) for the chained jump anim switch |
| +0x164 | ptr to saved blend weights |
| +0x16C (364) | **speed ratio** (input magnitude?) set by slot 12 (0xE102B0) |
| +0x170 / +0x174 | 0.3 / 0.3: catch-time thresholds (fallHeight > +0x174 enables catch) |
| +0x178 / +0x17C / +0x180 | small / heavy / fatal fall-damage heights (defaults FLT_MAX / 6.3 / 7.0); slots 14/15/16 |
| +0x184 | 0 = drop side (drop state) |
| +0x188 / +0x18C | list of already-pushed NPCs |
| +0x190 | NPC-push delay (drop state) |
| +0x194 | anti-stuck counter (byte) |
| +0x198 | anti-stuck timer |
| +0x261 / +0x262 | previous / current sub-state |
| +0x2AC.. | sub-state ID table: 684=1, 687=2, 690=3, 693=4, 701=5. The rag-fall sub-object pointer is at +0x2B8 (696), with flag 700 |

### HumanInAirData (reflection descriptor 0x1995920, size 0x400; names from CRC32, see RE/data/data_classes_named.txt)

| off | type | name / meaning |
|---|---|---|
| 0x010 | mat4 | **JumpOrigin**: start transform (0xB20200, 0xE01C00) |
| 0x050 | mat4 | root displacement of the takeoff anim at t=1 |
| 0x090 | mat4 | root displacement of the alternate (chained) anim |
| 0x0D0 | vec4 | **JumpApexPosition**: where descent started; the fall-height reference |
| 0x0E0 / 0x0F0 | vec4 | positional correction `target − animEnd` (main / chained) |
| 0x100 | mat4 | **JumpDestOrientation** |
| 0x140 | mat4 | copy of the start matrix |
| 0x180 | vec4 | ReboundDirection |
| 0x190 | vec4 | last released hold position (no re-grab unless 0.5 m below) |
| 0x1A0 | int | fallback fall anim |
| 0x1A4 | int | JumpOrientationAction = takeoff anim id |
| 0x1A8 / 0x1AC | int | air anim ids (main / alternate) |
| 0x1B0 | int | JumpFallAction |
| 0x1B4 | int | (anim id) |
| 0x1B8 | enum | **JumpType** {0 Straight, 1 1m, 2 3m5m}; SetupJumpToTarget writes 2 |
| 0x1BC..0x1DC | float arrays | anim blend weights |
| 0x1E4..0x1F0 | float | anim segment durations (impulsion, flight, …) |
| 0x1F4 | int | 0/1 flag |
| 0x1F8 | bool | **JumpApexReached** |
| 0x1F9 | bool | target-driven jump active |
| 0x1FA | bool | jump ends in free fall (overshoot) |
| 0x1FB | bool | has JumpDestOrientation |
| 0x1FC | bool | target attached to a moving entity (types 0x100, 0x8000) |
| 0x1F4 (desc) | enum | FallOrigin {Ground, Climb, HangWall, HangFree} |

Runtime (non-reflected) fields:

| off | meaning |
|---|---|
| 0x200 | phase flag |
| 0x210 | timer |
| 0x230 | 96-byte **jump target**: pos +0, local pos +16, normal +48, type +80/+84, entity handle +92 (copy routine 0xB12540) |
| 0x290 | **target type flags** |
| 0x2A0 | second target |
| 0x300 | chained target type (0x80000000 = none) |
| 0x304 / 0x308 | jump params / foot |
| 0x310 | free-fall aim target |
| 0x370 | entry kind (0 Jump, 1 RagFall, 2 Drop, 3 Reception) |
| 0x3B0 | drop/release info (+0x40 type 0..8, +0x44 side) |
| 0x3F0 | catch-mode selector |

Target-type flags (Data+0x290), from the switch in 0xE07D00:

| flag | meaning |
|---|---|
| 1, 0x10000 | free step / narrow object / pilotis — roof-edge jumps land here (§4.1.7) |
| 2 | pass-over vault (flight `…_to_passover`, arrival → Ledge HandPassOver) — §4.1.13 |
| 0x40, 0x80, 4…0x20 | ledge / hang variants |
| 0x100 | step-on target (arrival SubState 8 → NarrowObject state 2, `StateCrowdRun_Update` 0xE4E610, back to InAir when the reception ends). **Not a beam** (RE/05 §2.8 correction) |
| 0x200 | horse |
| 0x400 | swing (225 \| 8) |
| 0x800 | **haystack (Leap of Faith)** |
| 0x1000 | ladder |
| 0x2000 | pole |
| 0x8000 | NPC: air assassination (flight `…_to_assassinate`), not plain ground — §4.1.2 |

LandingEvent (size 0x14): +8 LandingType {Safe, SmallDamage, HeavyDamage, Fatal}, +0xC FallHeight, +0x10 Damage.

## 3. Sub-states and transitions

Sub-states: 1 Jump/Fall (main), 2 short anim (returns to 1; **hypothesis**: rebound), 3 Reception, 4 Drop (from hang/climb), 5 RagFall object.

| from | to | condition | source |
|---|---|---|---|
| enter | 1/5/4/3 | Data+0x370 = 0/1/2/3 | 0xE01C00 |
| 1 | Ledge/Narrow/Pole/Ladder/Horse/HayStack/Ground | anim finished and within 0.01 m of target, dispatched on target type | 0xE07D00 |
| 1 | Ledge (hang) | grab held (224&0x20) and handhold pair found | 0xE0BB70 → 0xE00490 |
| 1 | Ladder / Narrow / Pole | catch found | 0xE0BB70 |
| 1 | 3 Reception | edge landing (edges within 0.15 m, fallHeight < 9) | 0xE0BB70 |
| 1 | 5 RagFall | edge landing with fallHeight ≥ 9, or steep-slope slide > 10 m | 0xE0BB70, 0xE05200 |
| 1 | HayStack | haystack hit while diving or by contact | 0xE05490 |
| 1 | Riding | 1.5–2.0 m above a horse, within 0.5 m of its back segment | 0xE03B40 |
| 1 | Ground | flat contact (slope ≤ 45°), landing not fatal | 0xE05200 → 0xE07390 (setup 0xE05940) |
| 1 | RagdollGround | fatal landing type, or character dead | 0xE05200 |
| 3 | 1 | reception timer expired | 0xE003F0 |
| 4 | Ladder/Ledge/Ground/Ragdoll/HayStack/Riding | same detectors (catch only when Data+0x3F0 == 2) | 0xE0DE20 / 0xE0DCD0 |
| any | death | below world death height | 0xE03FA0 |

ActorState events sent by this module: 24 Jumping and 25 JumpingOnPlace (ended on landing/exit), 41 LeapOfFaith, 47 LongFall (ended in Cleanup 0xE03DE0). Where FreeFalling (23), Landing (26) and Roll (39) are sent is not found here.

## 4. Per-frame logic (pseudo-code)

```
Update():                                   // 0xE0FDD0
  if pos.z < world.deathZ: kill
  switch substate:
    Jump:      UpdateJumpMotion(); applyFlagsAsTransitions()   // 0xE0FBE0
    Drop:      UpdateDropMotion(); transitions                  // 0xE0DE20
    Reception: align; if timer done -> Jump
    RagFall:   subobject.update()

UpdateJumpMotion():                         // 0xE0DEF0
  if !apexReached and ctrl.vel.z < -0.001: apex = pos; apexReached = true
  t = animTime + dt
  if targetDriven:
     root = sampleAnimRoot(t) * startMatrix
     root.pos += (t/(dur1+dur2)) * (targetPos - animEndPos)   // linear warp
     if hasDestOrientation: slerp the orientation toward JumpDestOrientation
     if type 0x8000 and timeLeft <= 0.333: decideAirAssassination()
     setRoot(root)
  if CheckJumpTargetArrival or CheckAirCatch or CheckHayStack or ... or CheckGroundLanding: return
  if overshoot (Data+0x1FA):
     if leapAnim (287367545/7) ended:
        t = TimeToReachHeight(pos.z, target.z, vz)
        play dive anim 598297013 with param t*0.6
     else:
        vz -= 0.326*dt                     // = 9.8*dt^2 per frame
        pos.z += vz
        t = TimeToReachHeight(pos.z, target.z, vz)
        dv = dt*2*(target_xy - pos_xy - v_xy*t)/t^2
        v_xy += dv; clamp |v_xy| <= 15*dt
        pos += v
  else if !targetDriven:
     horizontal drift decays at 4 m/s^2·dt, capped at 5 m/s     // 19BA434=4, 19BA438=5
  AntiStuckNudge()                          // 0xE0B1E0
```

**Target over-drop rule** (0xB1B8C0): `thr = −5` (haystack: −3; if the drop is deeper than −3, thr = −30, and jump kind 2 = long Leap of Faith). If `dz < thr`, the aim point becomes `(target.xy pulled back by (dist − max(dist−1.5, 0))·clamp((dz−thr)/−30, 0, 2), start.z + thr)` and free-fall mode is set.

**Air catch** (0xE0BB70):
- Detection shape: radius 1.2, angle −45°..135°, height 0.38, offset (0, 0.27, −0.23). A second radius of 0.35 is used for edges (**hypothesis** on parameter meaning).
- Ledge: hand box 0.4 × 0.3, edge within 70° (1.2217 rad), tolerance 0.1, collision filter 306. The reach point is the hand bone + 0.2 m, or pos + 1.4 m (ledge) / + 1.95 m (wall climb).
- Ladder: box 0.4 × 0.25 (or 0.5) × 0.3. The catch height must be between 0.45 and ladderHeight − 1.95.
- Variant choice: fallHeight < 3 uses short catch anims, ≥ 3 uses long catch anims. Ledge pair spacing ≥ 0.3 picks the wide variant.
- Catch anim blend speed: `(1 − min(speed/10, 1))·0.14 + 0.06` s.

**Drop state** (0xE064C0, 0xE04B60):
- for the first 0.3 s, rotate away from the wall at up to 3 rad/s;
- slide back toward the release point at ≤ 2 m/s;
- pushing NPCs: strength 1/2/3/4 by speed² ≤ 4 / 16 / 36 / higher (0xE06D70).

## 4.1 Jump animations, jump motion and landings (verified 2026-10-01)

### 4.1.1 Who calls the setup
`Human__SetupJumpToTarget` 0xB20200 takes `(target, chained target, jump kind a4, target type a5, chained type a6, foot a7, flag a8)`.

| Caller | Jump kind | Foot |
|---|---|---|
| `HumanGround__StartRunJumpToTarget` 0xD837A0 | **0** (running) | byte+60 bits 2–3 of the playing item, or 0 when not moving |
| `HumanGround__StartFreeStepJumpToTarget` 0xD858A0 | 1 (free step) | same |

- When the foot is 0, `Human__GetLeadingFoot` 0xB18850 compares two bone heights and returns 1 or 2.
- IHumanGround vt28 (`HumanGround__JumpToTargetOfType` 0xD832F0) resolves the target type (`sub_B0E4E0`) and calls vt24. **There is no jump without a target**, which corrects RE/01 §6.3.

### 4.1.2 Action choice (`Human__ComputeJumpAnimBlend` 0xB1EC40)
Ids were converted with `int_convert` and resolved in `RE/data/action_graph_movement.txt`. A foot of 1 selects the first id of each pair.

| Jump kind | Takeoff action | Clips |
|---|---|---|
| 0 run | `0x0A4C8C0E` / `0x0A4C8C0F` | 40 |
| 1 free step | `0x0112B589` / `0x0112B5AC` | 40 |
| 2 rebound | `0x01157C6B` / `0x01157C6C` | |
| 4 pass-over | `0x0A4C8B07` / `0x0A4C8B06` | 6 |
| other | `0x0292655B` swing | |
| haystack, drop ≥ 3 m | `0x23A949B1` / `0x23A949B7` | faith jump |

The 40-clip takeoff has five direction groups: 0–7 `run_*` (front), 8–15 left, 16–23 right, 24–31 back-left, 32–39 back-right. Each group holds front 050/300/550, down 050/300/550 and up 050/300 cm.

| Target type (HumanInAirData+0x290) | up / down / near / mid / far (m) | Flight action |
|---|---|---|
| 1, 0x10000 (free step / narrow), 0x100 beam, 0x200 horse, 0x400 swing | 1.3 / −3 / 2.5 / 5.0 / 7.0 | `0x010DDAFA` / `0x010DF0D8` `air_*_to_freestep` (16 clips) |
| 2 | same | `0x09A0A58D` / `0x09A0A58E` `…_to_passover` |
| 0x8000 | same | `0x21B4DC3D` / `0x21B4DC3E` `…_to_assassinate` |
| 0x40, 0x1000 ladder, 0x2000 pole, 0x4000 | 2.5 / −3 / 2.5 / 5.5 / 7.5 | `0x011E555B` / `0x011E555C` `…_to_surface` |
| any other | 3.0 / −3 / 2.5 / 6.0 / 8.0 | `0x0121A149` / `0x0121A151` `…_to_swing` |
| 0x800 haystack | 1.3 / −3 / 2.5 / 5.0 / 6.0; drop ≥ 3 m: −3 / −30, near 7.5 | freestep; `0x23A949B5` `faith_jump_fall` |

So 0x8000 is the **air-assassination** target, not plain ground, and type 2 is the pass-over vault. This corrects the flag table in §2. A roof jump lands on a **free-step target (type 1)**.

The 16-slot free-step flight is laid out as:

| Slots | Clips |
|---|---|
| 0–2 | front 050/300/550 |
| 3–6 | down 050/300/550/800 |
| 7–8 | up 050/300 |
| 9–11 | front …_down |
| 12–15 | down …_deep 050/300/550/800 |

Nine-slot flights use slots 0–8 only.

### 4.1.3 Height blend and distance class (0xB1EC40)
Inputs:
- `o` = 0 for jump kinds 0, 1, 2 and 4, and −0.7 otherwise;
- `v56 = −0.5 − o`;
- `dz` = target − start height, clamped to ≥ −3 (raw dz is kept for the > 3 m rule below);
- `k` = entity+0x7C (**hypothesis**: character scale; the port uses 1).

```
down = type != 2 && target.z < start.z + v56
h    = clamp((dz − v56) / ((dz ≥ v56 ? up−o : down−o) − v56) / k, 0, 1)
d < near           → class 0, f = d/near
dz ≥ v56 (not down) → class 1, f = (d − near) / ((mid − near)(1 − h))
d ≥ mid            → class 3, f = (d − mid) / ((far − mid)·h)
else               → class 2, f = (d − near) / (mid − near)
f clamped to [0, 1]
```

The target point used for d is first moved 0.5·k down along an axis vector (`sub_480D20`; **hypothesis**: up).

### 4.1.4 Flight weights (`Human__ComputeJumpFlightWeights` 0xB140B0)
Let (s0, s1, s2) be (9, 10, 11) when the type has deep variants (1, 0x10000, 0x100, 0x200, 0x400) and the jump is going down; otherwise (0, 1, 2).

| Class | Weights |
|---|---|
| 0 | w[s0] = (1−h)(1−f), w[s1] = f(1−h); going down: w3 = h(1−f), w4 = fh; otherwise w7 = h(1−f), w8 = fh |
| 1 | w1 = (1−f)(1−h), w2 = f(1−h), w8 = h |
| 2 | w[s1] = (1−h)(1−f), w[s2] = f(1−h), w4 = h(1−f), w5 = fh |
| 3 | w5 = (1−f)h, w6 = fh, w[s2] = 1−h |

For deep types with raw dz < −3, after the takeoff weights are computed: `k = clamp((−dz − 3)/5)`. Then w12..15 = k·(w9+w7+w3, w10+w8+w4, w11+w5, w6), and those sources are scaled by 1−k.

### 4.1.5 Takeoff weights (`Human__ComputeJumpTakeoffWeights` 0xB13A60)
- The takeoff mirrors the flight blend on its own 8-clip family.
- It is split between group A and group B by `t = (|angle| mod π/2)/(π/2)`. The angle is the side angle to the target, forced to **0 for jump kinds 0 and 4**, so a running jump plays only the `run_*` group.
- **Class 0:** A.front050 = F[s0], A.front300 = F[s1]; then A.down050/300 = F3/F4, or A.up050/300 = F7/F8.
- **Other classes:** A.front300 = F[s1], A.front550 = F[s2] + F6; then A.up300 = F8 (class 1), or A.down300/550 = F4/F5.
- Quadrants:

  | Angle | Group A | Group B |
  |---|---|---|
  | [0, 90°) | front | right |
  | ≥ 90° | right | back-right |
  | (−90°, 0) | front | left |
  | ≤ −90° | left | back-left |

  Kind 2 shifts the slots by +8; kind 3 uses the up variants; kind 4 blends by `HumanLedgeData+0x134`.

### 4.1.6 Motion (0xB20200, 0xE0DEF0)
The engine computes item times and displacement as follows:

| Function | Computes |
|---|---|
| `AnimItem__GetBlendedDuration` 0x507650 → 0x5B9480 | item duration **Σ wᵢ·Tᵢ** (clip duration at +12), not normalised |
| `AnimItem__HasNonZeroWeights` 0x507880 | only rejects \|Σw\| ≤ 0.0005 |
| `AnimItem__GetBlendedDisplacement` 0x5084F0 → 0x508260 | the blended displacement over a normalised span of that duration |

The setup then:
- stores T₁ = takeoff duration (+0x1E4) and T₂ = flight duration (+0x1E8);
- stores the takeoff and flight displacement matrices (+0x50 / +0x90);
- sets the correction `target − (start + takeoffEnd + flightEnd)` (+0xE0).

The root at time t is `start · disp(takeoff ⊕ flight, t) + (t/(T₁+T₂))·correction`. The flight item continues from the takeoff's end. JumpType := 2, target-driven := 1.

### 4.1.7 Arrival on a free-step target (0xE07D00, types 1 / 0x10000)
- Plays the reception `0x010DE1FE` / `0x010DF150` (`air_*_to_freestep_tr_freestep_entry`, 24 clips, FROMANIM). Slots 0–11 get the flight's weights ×(1 − r) and slots 12–23 (`_fast`) get ×r, where r = HumanInAir+0x16C.
- Aligns over 0.067 s and sets result flag 2, which leads to **NarrowObject** (NarrowObjectData+200 = 6).
- The reception's transitions lead to wait `0xD82508` or locomotion `0x05923BDB`.

### 4.1.8 Ground landing (`HumanInAir__SetupToGround_Landing` 0xE05940)
Inputs:
- `drop` = start.z − z;
- `horiz` = horizontal distance from the start (JumpOrigin translation, Data+0x40);
- `b = min(drop/2.5, 1)`, raised to `min((horiz − 5)/7.5, 1)` when horiz > 5;
- **speed bucket** from r = HumanInAir+0x16C: < 0.2 → 0, < 0.5 → 1, < 0.9 → 2, else 3.

Choice:
- **drop > 3:** `0x010DD707` `landing_damage_footl` (r ≤ 0.2) or `0x010DD70B` `landing_damage_footl_roll`. TransitionSetupToMovement type 10, camera shake `min((drop−3)/7, 1)`.
- **drop ≤ 3, wanted move ~0 or more than 75° (1.309 rad) from the motion:** `0x6E9C702A` straight → walk/jog/sprint (bucket ≥ 1), or `0x6E9C7557` straight → wait. ActorState 25/24 ended.
- **drop ≤ 3 otherwise:** by foot, `0x6D200160` / `0x6D200161` forward (bucket ≥ 1: 6 clips {soft, hard} × {walk, jog, sprint_impultion}; w[bucket−1] = 1−b, w[bucket+2] = b), or `0x6E9C754A` / `0x6E9C754C` forward → wait (bucket 0: [1−b, b]).
- Transition type 9, +0x1C = 0.5. Every landing clip is FROMANIM, and its transitions return to `0x05923BDB`.
- Landing with type 0x8000 (225 & 0x20) skips all this (GroundData+584 = 2).

**r = HumanInAir+0x16C:** the ctor sets 0 (0xE100D5 → 0xE10124). Its only other writer is IHumanInAir slot 16 (0xE10350, vft 0x17010A4; the old name `SetFatalFallHeight` is likely wrong). Its caller is not traced. **(hypothesis)** It is the ground speed ratio, because the buckets pick walk, jog or sprint exits. The port passes the speed parameter at takeoff.

### 4.1.9 Ground loss → fall type (`HumanGround__TransitionToInAirFall` 0xD8C380)
- h = the ground probe's fall height (probe component Human+0xFC, vt112 → +48); v = horizontal speed (vector at +1264).
- Fall type:

  | Type | Condition |
  |---|---|
  | 0 / 1 | h < 1, or h < 2 when the probe type (vt328) is 1 |
  | 2 / 3 | 2 ≤ h < 8 with probe type 1 |
  | 4 / 6 | anything else |

  The second value of each pair is used when v ≥ 2.5 m/s.
- The type goes back through probe vt112 to pick the entry, and the result feeds `sub_D88AA0(setup, 2, …, 3)`. That mapping is **not traced**.
- The port keeps name-based entry clips, but switches on the game's 2.5 m/s.

### 4.1.11 Jumps at a ledge (verified 2026-10-01)
**Standing straight jump at a hand target** (`HumanGround__StartStraightJump` 0xD85550).
- With no target (+880 == 0x80000000), it plays the in-place jump (action 30 / `0x0109B1AC` by foot, InAir +416 = 31).
- With a target, it calls **`Human__SetupJumpToHandTarget` 0xB21DA0** on the ground's JumpTarget (+784). That function is also called from 0xD8A9C0, 0xDE8EF0, 0xE3BF90 and 0xF73B80.

The band is chosen by dz = hand height above the feet. "Subtype 8" is JumpTarget+84; **hypothesis**: it means a wall below the edge.

| dz (m) | Flight (2 clips, weights [1−b, b]) | b | Root = hand + out·n − down | Flags | Arrival (0xE07D00) |
|---|---|---|---|---|---|
| < 0.7 | `0x012B291B` `collide_full_*_to_freestep_{050,070}cm` | (dz−0.5)/0.2 | +0.5 n | 1 | `0x012B33F1` → NarrowObject |
| 0.7–1.5 | `0x01290ECF` `lean_wait…_to_hangknee_{070,150}cm` | (dz−0.7)/0.8 | +0.5 n | 4 | `0x01290ED0`, Ledge SubState 4 (pull-up), root → edge − 0.1 n |
| 1.5–2.0 | `0x01272A69` `jumpstraight_to_hangknee_{150,200}cm` | (dz−1.5)·2 | +0.5 n | 4 | `0x01272A6A` (a, b) → `0x0106C58B` hangknee→wait |
| 2.0–2.5, subtype 8 | `0x01271631` `jumpstraight_to_hangwall_{200,250}cm` | (dz−2)·2 | +0.5 n − 1.1 | 0x40 | `0x01271632` (a, b) → wall idle `0x0106F2E8`; needs foot holds; hang type 0 |
| 2.0–2.5, else | `0x01271639` `jumpstraight_to_hangwaist_{250,200}cm` | (dz−2)·2 | +0.5 n − 1.0 | 8 | `0x0127163A`, SubState 4 (pull-up) |
| ≥ 2.5, subtype 8 | `0x0121A8B1` `jumpstraight_to_hangwallfree_{250,300}cm` | (dz−2.5)·2 | +0.5 n − 2.4 | 0x80 | `0x0121B072` (a, b) → free idle; **hang type 1** |
| ≥ 2.5, else | `0x0121A598` `jumpstraight_to_hangfree_{250,300}cm` | (dz−2.5)·2 | −2.4 | 0x80 | `0x012723A5` (a, b) → free idle `0x012719F1` |

- These ids apply while the playing action is 89 or `0x01099C96` (`impultionstraight_footr_to_jumpstraight_clear`, the straight-jump impulse). Otherwise the `beam_jumpstraight_*` variants `0x516D52DB…DF` are used.
- `a4` = beam auto-climb `0x6DA7646E` (+ `0x6EFC142E` for the other foot), with b = (clamp(dz, 0.2, 1.3) − 0.2)/1.1.
- InAir data: JumpType 0, **no takeoff** (+0x1A4 = −1, +0x1E4 = 0), flight +0x1A8 with Σw·T at +0x1E8. The correction is target root − anim end, as in §4.1.6.

**Running jump onto a ledge** (`SetupJumpToTarget` §4.1.2; type 0x40 flight `…_to_surface`, 0x80 `…_to_swing`). The arrival takes the generic branch of 0xE07D00.

| Case | Reception | Then |
|---|---|---|
| Foot holds found | `HumanInAir__PlayLedgeReception` 0xE02BA0 plays `0x011FF16A` (type 64: 3 items × 6 clips `air_surface_tr_hangwall_reception_{straight,30_out,45_in}_{min,max}`, then `hangwall_reception_*_{a,b}`) or `0x8E5444EE` (other types, timed 0.1666 s) | Weights from `sub_E02790`; Ledge SubState 6 |
| No feet, type 0x80 | `0x023E0C60` swing cycle (front/back up/down) | SubState 8 (SwingReception) |
| No feet, other types | No arrival | The air catch takes over |

**Port.**
- The ground's grab into a wall uses the straight jump with these bands (hands 0.7–3.0 m up). Knee and waist heights end standing on top; higher ones end hanging.
- Running jumps onto ledges use `SetupJumpToTarget` with the `…_to_surface` flight (wall hang, type 0x40) or the `…_to_swing` flight (free hang, type 0x80), then the wall reception or one swing.
- The placeholder jump arc is gone.
- Departures:
  - the straight-jump impulse `0x01099C96` the ground plays first is not played;
  - the ≤ 0.7 m step-up and the beam variants are not used;
  - the wall reception uses the "straight, min" clip (**hypothesis**: min/max selection not traced);
  - the swing plays once, with no SwingStrength;
  - knee / waist pull-ups end in Ground instead of NarrowObject;
  - the trigger (high profile + Legs into a wall) is the port's: the game's straight jump comes from the static-jump path, and running into a wall is Walling (RE/12 §3.1).

### 4.1.13 Pass-over vault (verified 2026-10-02)
A running jump at a **type 2** target vaults a thin wall with one hand.

**Jump** (`Human__ComputeJumpAnimBlend` 0xB1EC40):
- The bands are the free-step ones: up 1.3, down −3, near / mid / far 2.5 / 5.0 / 7.0 m. "Going down" is never set for type 2.
- **Flight** `0x09A0A58D` / `0x09A0A58E` (`xx_h_air_{front_050,300,550, up_050,300}cm_foot{l,r}_to_passover`). Weights from `Human__ComputeJumpFlightWeightsPassOver` 0xB142A0:
  - near class: [(1−d)(1−h), d(1−h), 0, (1−d)h, dh];
  - other classes: [0, (1−d)(1−h), d(1−h), 0, h].
- **Takeoff** (`Human__ComputeJumpTakeoffWeightsPassOver` 0xB13E20; side angle 0 for kind 0). Weights go into the front group:
  - near: slots 0, 1, 6, 7 = f0, f1, f3, f4;
  - class 1: slots 1, 2, 7 = f1, f2, f4;
  - classes 2 / 3: slots 1, 2, 4, 5 = f1, f2, f4, f4.

**Arrival** (`CheckJumpTargetArrival` 0xE07D00 case 2):
- **Reception:** `0x0A4C9776` after the footl flight (`…_to_passover_tr_passover_handr`), else `0x0B5640DB` (`…_handl`).
- **Root:** interpolated to target + 0.5·forward over 0.067 s. The reception clips also move 0.5 m forward: the target sits 0.5 m before the edge.
- **Ledge data:** +304 = 1 − (w_front050 + w_up050); the target is copied to LedgeData.
- **Hand-off:** Ledge SubState **12** HandPassOver.

**HandPassOver** (`HumanLedge__HandPassOver_Update` 0xDDB800, `HumanLedge__StateHandPassOver_Update` 0xDE0720):
- **SubState 0:** the reception's interpolation runs. When it completes, the vault starts.
- **The vault:**
  - Action `0x09A0A7E1` (`xx_h_passover_handl_{030,100}cm`) when the reception's item ends left foot ahead (flags & 0xC == 4), else `0x09A0A7E2` (hand r).
  - Weights [1 − w, w], w = min(thickness, 1). `Human__ProbeWallTopThickness` 0xB18FB0 measures the thickness: guidance edges crossed by the forward line from the edge − 0.3·fwd − 0.25 up; near and far edge; midpoint, direction and distance.
  - Clip root motion: 030cm moves 0.3 m flat in 0.067 s; 100cm moves 1.0 m in 0.2 s.
  - The root is interpolated over the vault to the probe's point + 0.3·(1 − w)·fwd. With no wall top found: edge + 0.3·fwd, w = 0.
  - Then SubState 1, and +308 = w.
- **At the vault's release point (sub_5017B0):**
  - +328 == 0 → **Ground** (`SetupPassOverToGround` 0xDCFA40: move mode from the stick, entry 12);
  - otherwise, with a drop ≥ 3 m beyond (`PassOverWantsPullDown` 0xDD2BD0 → `PassOverToPullDown` 0xDE0220) → pull-down type 4. Clips `0x09A0B647` / `49` orientation, then `0x09A0B648` / `4A`, blended by +308.
  - otherwise (`PassOverWantsInAir` 0xDCC0E0 → `PassOverToInAir` 0xDE0280) → **InAir**: `SetupPassOverToInAir` 0xDDBC60 plays `0x0109B7C8` / `0x0109BB53` `passover_hand{l,r}_{030,100}cm_tr_fall` [1 − w, w].
  - The meaning of +328 is unknown (hypothesis: a chained-jump request; jump kind 4 takes off from the pass-over with `0x0A4C8B07` / `06`, its weights from +308).

**Port** (`player/passover.rs`):
- The flight and takeoff weights.
- The reception and its interpolation.
- The thickness probe (the far LedgeGrab edge along the facing) and the vault blend and root.
- Then Ground when there is support under the root, else InAir with the `tr_fall` action.

**PORT:**
- **Targets:** type 2 targets are wall tops ≤ 1 m thick within the free-step up band. The target is 0.5 m before the edge at the top's height. The game's type 2 guidance candidates are not traced.
- **Probe point:** the far edge is taken as the probe's point (hypothesis for sub_B0FC50).
- **Release:** the release point is the vault's end.
- **Not ported:** the pass-over pull-down (≥ 3 m drop), chained jumps from the pass-over (kind 4), and `passover_*_to_roll_ending`.

**Test geometry:** a 1 m railing, 0.3 m thick, at (40, 4). `AC_AUTOPILOT=passover`.

### 4.1.10 Port (`port/src/player/jump_blend.rs`, `air.rs`, `ground.rs`)
- §4.1.2–4.1.8 are ported exactly for running jumps to roof edges (type 1) and for ground landings.
- The clips' durations and displacement (9 samples) come from `player/jump_clips.rs`. It is generated from the install by `cargo test probe_dump_jump_clips -- --ignored` and holds derived numbers only.
- Departures:
  - **jumps at a ledge:** now ported too (§4.1.11), so no placeholder arc remains;
  - **free-step arrival:** it continues in Ground with the reception playing, since NarrowObject is not ported;
  - **no target in range:** the port still jumps `FREE_JUMP_DISTANCE` ahead (PORT);
  - **leading foot:** taken from the playing locomotion item (**hypothesis** on the bit meaning).

### 4.1.12 Leap of Faith and the haystack (verified 2026-10-02)
**Jump** (`Human__ComputeJumpAnimBlend` 0xB1EC40, target type **0x800**):
- **Faith mode** when the jump kind is 0 or 1 and target z − start z ≤ **−3 m** (internal mode v26 = 2).
- **Actions:**
  - takeoff `0x23A949B1` (foot 1) / `0x23A949B7`, `xx_h_freestep_footr_to_faith_jump_{100,800}cm_long_{300,3000}cm_down`;
  - flight `0x23A949B2`, `xx_h_faith_jump_*` with the same four clips;
  - third action `0x23A949B5`, `xx_h_faith_jump_fall` (FROMPHYSICS), used for the free-fall tail.
- **Bands:** max down −30 m, near 7.5 m. The over-drop threshold (0xB1B8C0) is therefore −30 m instead of −3 m.
- **Flight weights** (the end of 0xB1EC40): l = clamp(dist / 7.5), d = clamp(−dz / 27). Weights are [(1−l)(1−d), l(1−d), d(1−l), d·l] for 100/300, 800/300, 100/3000 and 800/3000.
- **Takeoff weights:** the function returns before writing them. The port uses the same weights (**hypothesis**; the item's default is [0, 0, 1, 0]).
- **Shallower haystacks** (dz > −3) use the free-step flight with bands 1.3 / −3 / 2.5 / 5.0 / 6.0.
- **Clip lengths:** the takeoffs run 0.87–1.67 s and move 0.9–4.2 m forward. The flights run 0.47 s (1.3 m down) to 1.07 s (10.4 m down).

**HayStack context (21)** (`HumanHayStack__Enter` 0xE43140):
- **Entry:**
  - From a faith jump, `EnterTop_FaithLanding` 0xE41D50 plays `0x23A9666C` `xx_h_faith_jump_landing` (blend 0.3 s).
  - Other air entries go through `EnterFromAir` 0xE42700, which plays `0x7750D212` `xx_h_air_to_haystack` (blend 0.1 s).
  - Both interpolate the root (`sub_711130`) to the haystack entity's position over clamp(distance / speed, 0.1, 0.4) s.
- **Wait:** `ChooseWait` 0xE416B0, then `PlayWaitHigh` 0xE408C0, plays `0x23A9666D` `xx_h_haystack_wait` (5 s loop). The low wait is `0x23A96674`.
- **Hop out:**
  - Trigger: event **3** in `Wait_HandleEvent` 0xE43BD0.
  - Guard `Guard_HopOut` 0xE434E0: a ray along the wanted direction must cross the haystack's footprint edge. The exit point is that edge + 0.5·dir, 1.25 m up, and must have room for the body (`sub_B2D2A0`, 0.35 / 0.5 / 0.75).
  - Then `ToHopOut` 0xE41D00 → `PlayHopOut` 0xE41890 faces that direction and plays `0x2C4C2431` `xx_l_haystack_hop_out` (0.53 s, 0.8 m forward). Its transitions go to the low or high wait.
- Events 2, 4 and 5 in the wait handler lead to other actions, not traced.

**Port** (`jump_blend::faith`, `targets.rs` haystacks, `hay.rs`):
- Running (high profile + Legs) off an edge toward a haystack 3–30 m below and ≤ 7.5 m away plays the faith takeoff and dive.
- Arrival enters the HayStack context: the landing action and root interpolation, then the wait.
- Pushing the stick hops out into Ground, with the hop-out action's root motion.
- No landing damage.
- **PORT:**
  - A haystack in the jump cone wins over roof targets. The game's LeapOfFaith ability path (IHuman vt1540/1544) is not traced.
  - Hop out is triggered by the stick (event 3's sender is not traced).
  - Haystacks are not solid.
  - Entry by a ballistic fall (`CheckHayStackEntry` 0xE05490) is not ported.
- `AC_AUTOPILOT=faith`.

## 5. Constants

| addr / site | value | meaning |
|---|---|---|
| dword_192DDA8 | 1/30 | fixed simulation dt |
| 0xDFFC90 | 9.8, 19.6 | gravity |
| 0xE0DEF0 | 0.326 | g·dt (per-frame gravity on m/frame velocity) |
| 0xE0DEF0 | 15.0 | max free-fall horizontal speed (m/s) |
| 0xE0DEF0 | 0.333 | air-assassination decision window (s) |
| 0xE0DEF0 | 0.2 | max chained-anim blend (s) |
| 0xE0DEF0 | 0.4 | anim-time normaliser cap |
| 0xE0DEF0 | 0.75 | chain-switch anim fraction |
| 0xE0DEF0 | 0.8 | anim speed factor (anim 43148635) |
| 0x19BA434 / 0x19BA438 | 4.0 / 5.0 | non-target drift decel / max speed |
| 0x19BA428 / 42C / 430 | 1.2 / 0.35 / 0.38 | catch shape radii / height |
| 0xE07D00 | 0.01 | arrival tolerance (m) |
| 0xE07D00 | 0.067, 0.2 | align durations (s) |
| 0xE07D00 | 0.9848 | cos 10° |
| 0xB1B8C0 | −5, −3, −30, 1.5, 2.0 | over-drop rule |
| 0xE0BB70 | 3.0, 9.0 | catch variant / fatal threshold |
| 0xE0BB70 | 7.0 | direction smoothing rate |
| 0xE0BB70 | 0.15 | edge-landing tolerance |
| 0xE0BB70 | 0.14, 0.06, 10 | catch blend formula |
| 0xE0BB70 | π/6, π/4 | catch angles |
| 0xE0BB70 | 0.3 | ledge pair spacing threshold |
| 0xE0BB70 | 0.7071 | catch angle |
| 0xE0A990 / 0xE0AC70 | 1.4 / 1.95 / 0.2 | reach heights |
| 0xE0A990 / 0xE0AC70 | 0.4, 0.3, 70°, 0.1, 0.5 | hand box and tolerances |
| 0xE04100 | 0.25 / 0.5, 0.4, 0.3, 0.45, 1.95, 0.6 | ladder catch |
| 0xE038A0 | 45° / 70° / 135°, 0.3 | ground contact classes, skip zone below start |
| 0xE00FE0 | 8–20 m table; damage 10/20/40/80/120/160/200 | fall damage |
| ctor 0xE0FE80 | 0.3, 0.3, FLT_MAX, 6.3, 7.0 | per-instance thresholds |
| 0xE05940 | 2.5, 5.0, 7.5, 3.0, 7.0, 75°, 0.2 / 0.5 / 0.9 | landing anim selection, shake |
| 0xE05200 | 10.0 | max slope slide before rag fall |
| 0xDFF1D0 | 5.25 | (unused path) |
| 0xE03B40 | 1.5–2.0, 1.2 / −1.1, 0.5 | horse landing |
| 0xE03140 | 2.0 m/s, 60°, cos 80° | mid-air collision with a human |
| 0xE0B1E0 | 0.25, 0.5, 3.0, 0.15 | anti-stuck |
| 0xE064C0 | 0.3 / 0.6 / 1.0 / 0.1 (phase), 0.2 / 0.5 / 0.6 / 0.1 (push delay) | drop state |
| 0xDFF680 (static init) | anim id tables 0x1A2EAE0..; floats 2.5, 2.5, 5.5, 5.5, 1.0 at 0x1A2EA50 | **(hypothesis)** jump ranges |

The jump-candidate scoring, maximum jump distance and JumpLinkRange Normal/Extended live in the module that chooses the target (HumanGround, B1EC40 and others), not in InAir.

## 6. Interfaces

**Calls in:**
- Context transition into InAir (context 8) through `sub_55F7E0(8, setupCb)` from Ground (D80810, D7D570, D85550, D858A0, D8ADB0), Ledge (DCB110, DCB280, …), Climb (DF3C10, DF34B0), Ladder, Pole, Rope, Walling, Narrow and HumanDecision (F216F0, F232E0).
- Ground's jump goes through `Human::SetupJumpToTarget` 0xB20200.
- IHumanInAir setters, called by the controller/decision code: grab requested, input direction, speed ratio, damage thresholds.

**Calls out:**
- `sub_55F7E0(ctx, setup)` with ctx 4 Ground, 5 Ladder, 6 Pole, 9 Ledge, 12 Narrow, 13 Riding, 15 RagdollGround, 19 Kiosk, 21 HayStack.
- Animation API `sub_501190(animId, …)` (play) and the 0x50xxxx family.
- Alignment `sub_711130` / `sub_7113F0`.
- Character controller (`sub_718440`): velocity at +0x4F0, contacts.
- LandingEvent, air-assassination code `sub_AF2080`, push-NPC event `sub_B6B230`.

## 7. Open questions / dynamic checks

- Confirm the velocity units at this+0x50: watch it in free fall and expect a −0.0109 per-frame delta.
- Who sets `226&8`? The player is expected to use the fixed 8–20 m table and NPCs the 6.3/7.0 thresholds; verify at runtime.
- Decode the target-selection code (B1EC40, HumanGround jump detection, "Main actor jump detection shapes").
- Sub-state 2 and `E04860`; `sub_E01B70` (B23CB0 condition); E0B890 / E04630 / E044D0 / DFFE30 catch variants are not fully decoded.
- Where FreeFalling, Landing and Roll ActorStates are raised.

## 8. Renamed functions

The renames covering 0xDFE000–0xE10700 are:

- **Lifecycle:** E0FE80 HumanInAir__ctor, E019C0 InitSubStateTable, E09BA0 OnEnter, E07120 OnExit, E0FDD0 Update, E01C00 ResetAndPickEntrySubState, E03DE0 Cleanup.
- **Jump and fall motion:** E0FBE0 UpdateJumpState, E0DEF0 UpdateJumpMotion, E0DCD0 UpdateDropMotion, E0DE20 UpdateDropState, E064C0 EnterDropState, E04B60 DropSteerAwayFromWall, E003F0 UpdateReceptionState, E00EF0 EnterReceptionState, E07180 UpdateSubState2.
- **Detectors:** E07D00 CheckJumpTargetArrival, E0BB70 CheckAirCatch, E05490 CheckHayStackEntry, E03B40 CheckHorseLanding, E05200 CheckGroundLanding, E06D70 PushHumansWhileFalling, E0B1E0 AntiStuckNudge.
- **Landing and fall height:** E038A0 GetGroundContactType, E01FA0 AverageNearbyEdges, E047F0 GetFallHeight, DFEFC0 IsLeapOfFaithDiveAnim, E00FE0 ComputeLandingType, E04E40 SendLandingEvent, E03010 AllocLandingEvent, E03A20 GetSlopeSlideDrop.
- **Transition setups:** E05940 SetupToGround_Landing, E06890 SetupToGround_FromDrop, E06C40 SetupToRagdollGround_FromDrop.
- **Transition requests:** E07390 RequestGround, E073C0 RequestRagdollGround_FromDrop, E073F0 RequestGround_FromDrop, E00460 RequestRagdollGround, E00490 RequestLedge, E004C0 RequestNarrowObject, E004F0 RequestRiding, E00520 RequestLadder, E00550 RequestPole, E00580 RequestLadder_FromDrop, E005B0 RequestLedge_FromDrop, E005E0 RequestRiding_FromDrop, E015A0 RequestKiosk, E015F0 RequestHayStack_FromDrop, E01AF0 RequestHayStack.
- **Transition flag checks:** DFEEC0 WantsGroundTransition, DFF180 WantsRagdollTransition.
- **Generic math helpers:** DFFC90 Ballistic__TimeToReachHeight, E00730 Ballistic__SolveCorrectionAccel.
- **Catch searches:** E04100 FindLadderCatch, E0A990 FindLedgeCatch, E0AC70 FindClimbCatch, E0A150 FindHandholdPair.
- **Misc:** E03140 CheckAirCollisionWithHuman, E04030 FinishAirAssassination, E00130 ClearJumpTarget, E03FA0 CheckWorldDeathHeight.
- **IHumanInAir setters and slots:** E102D0 SetGrabRequested, E10460 SetInputDirection, E102B0 SetSpeedRatio, E10310 SetSmallDamageFallHeight, E10330 SetHeavyDamageFallHeight, E10350 SetFatalFallHeight, E102F0 SetUseFixedFallDamageTable, E09E10 / E0A600 IHumanInAir_slot0/1.

Comments were also added at B20200 (suggested name Human__SetupJumpToTarget) and B1B8C0 (over-drop rule).

Not part of InAir: E106E0 is a static initialiser for the AAE_* achievement enum; DFF680 is the static initialiser for the InAir anim tables. E13370, E15AA0, E165D0 and E18970 have no vftable references and are called from Human/Ground (B13260, D72F60, D73D30, DF9360, E6B200), so they belong to shared Human helpers (**hypothesis**) and were not analysed.

## Methodology

1. Listed vftables with vt.py and scanned every vftable slot that points into the range.
2. Found context-transition calls by scanning for the `push ctx; call 55F7E0` byte pattern.
3. Decompiled the update chain from the Update slot.
4. Decoded HumanInAirData reflection (offset = dword >> 18) and recovered names by CRC32 brute force: JumpApexPosition, JumpApexReached, JumpDestOrientation, LandingEvent fields.
5. Read floats with pe.py and int_convert.
