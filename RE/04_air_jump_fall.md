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
| 1, 0x10000 | narrow object / pilotis |
| 2 | ledge |
| 0x40, 0x80, 4…0x20 | ledge / hang variants |
| 0x100 | beam |
| 0x200 | horse |
| 0x400 | swing (225 \| 8) |
| 0x800 | **haystack (Leap of Faith)** |
| 0x1000 | ladder |
| 0x2000 | pole |
| 0x8000 | ground or NPC (air assassination) |

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
