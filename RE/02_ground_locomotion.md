# 02 — HumanGround: ground locomotion

Scope: `scimitar::HumanGround` (code ~0xD7C000–0xDC3700), its context object `HumanGroundData`,
speed modes, turning, start/stop, ground-loss → InAir, and hand-over points.
All addresses are VAs in `AssassinsCreed_Dx9.exe` (v1.02). Unverified readings are marked **(hypothesis)**.

## 1. Summary

* HumanGround is a **hierarchical state machine with 87 states** (IDs 1..87) embedded in the module
  object. Each state is a 3-byte record `{u8 id; u8 initialChild; u8 currentChild}`; some states are
  delegated to separate objects (fight/hurt/dodge/get-up/oriented-move state classes in 0xF2xxxx–0xF6xxxx).
  The tree is built in `HumanGround__InitStateTree` (0xD90D00).
* Locomotion is **animation-driven for translation, code-driven for rotation**:
  * Translation comes from the animation graph (root motion of blended walk/jog/run/sprint cycles,
    start/stop/turn clips). No velocity integration exists in the ground module. HumanGround only
    selects clips and sets **blend weights** from one scalar, the *speed parameter* (`HG+0x5E8`, 0..1).
  * Heading is rotated toward the desired heading by code at a clamped angular rate
    (`HumanGround__UpdateHeading` 0xD95290 → `RotateTowards` 0xD94F30), then written to the entity
    (sub_52BBB0), unless the current clip has the "anim drives rotation" flag (0x10).
* Speed modes = four equal bands of the speed parameter:
  `(0,0.25]` Walk, `(0.25,0.5]` Jog, `(0.5,0.75]` Run, `(0.75,1]` Sprint
  (`IHumanGround::GetSpeedBand` 0xD807B0 returns 0 none,1 Walk,2 Jog,3 Run,4 Sprint — the same order as
  `AssassinAbilitySet::MaxSpeed`).
  Target = `band_base + 0.25 * stick`, with band_base = 0 (low profile), 0.5 (high profile),
  0.75 (high profile + Sprint flag). The parameter moves toward the target at **1.0/s up, and
  down through a deceleration curve** (0xDA0810).
* Ground loss: a ground probe (Human+0xFC component) is polled every frame; when it reports a fall the module
  builds a `TransitionSetupDataToInAir` whose fall type depends on fall height (1 m / 2 m / 8 m) and
  horizontal speed (2.5 m/s) (0xD87720, 0xD8C380). A second "support" test (0xB23CB0) handles standing on
  steep/character surfaces with no floor within 0.8 m.

## 2. Object layouts

### 2.1 HumanGround (size ≥ 0x7C4; ctor 0xDB27B0, dtor 0xDB38B0)
| Off | Type | Meaning | Source |
|---|---|---|---|
| +0x00 | vft | HumanGround (19 slots, 0x1700584) | 0xDB27B0 |
| +0x08 | Human* | owner | everywhere (`this+8`) |
| +0x0C | ptr | Human sub-object (B2FC50(owner)); +0x48/+0x4C misc state, +0x204..+0x210 float pairs used for footstep timers (0xDA0810 tail) | 0xD834C0 |
| +0x10 | HumanGroundData* | context/blackboard (see 2.2) | 0xD834C0 |
| +0x14 | vft | IHumanGround (401 slots, 0x16FFEFC) — event "CanX/DoX" pairs + accessors | ctor |
| +0x18 | vft | IHumanGround #2 (27 slots, damage/listener) | ctor |
| +0x1C | vft | IHumanGroundAccess (4) | ctor |
| +0x20 | vft | IHumanWeapon (18) | ctor |
| +0x24 | vft | IHumanTeleport (2) | ctor |
| +0x35 | u8 | root initial state id | 0xD90D00 |
| +0x36 | u8 | root current state id | 0xDAF160 |
| +0x38 | array | pending-event queue (8-byte entries, reserved 87) | 0xD90D00, 0xDB4F00 |
| +0x80..+0x1F5 | state records | 87 state records (see §3) + pointers to delegated state objects at +0x9C,+0x104,+0x10C,+0x118,+0x144,+0x14C,+0x160,+0x168,+0x170,+0x178,+0x180,+0x188,+0x190,+0x198,+0x1AC,+0x1D0,+0x1D8,+0x1E0,+0x1E8,+0x1F0 (each followed by an "is-object" byte) | 0xD90D00 |
| +0x290 | int | =1 enables idle turn-in-place (0xD84B80) | |
| +0x2B0 | float | lateral turn angle used by blend (|x|-π/2)/π/2 | 0xDA0810 |
| +0x440 | obj | crowd push helper (F1xxxx `HumanGroundPush`?) (hypothesis) | 0xDB27B0 |
| +0x528 | u8 | "stuck/abort" latch → posts deferred callback | 0xDAD1C0 |
| +0x529 | u8 | ObstacleCollision pending | 0xDAF2D0 |
| +0x56C | obj* | secondary FSM (weapon/sheath, states at +0xB4/+0xB7) | 0xDC7D80 |
| +0x580 | obj* | animation-graph param block; +0x44 = graph mode (1 transition, 2 locomotion) | 0xDA0810 |
| +0x5D4 | int | moving-foot / start phase (0 none, 1, 2) | 0xDA0810 |
| +0x5D8 | int | **desired move mode**: 0 = no input, 2 = move (1 = scripted walk) | 0xD7D4B0 |
| +0x5DC | int | in run-or-faster band (speed>0.25) | 0xDA0810 |
| +0x5E0 | int | **high profile** (copy of Data+0x1B1) | 0xD7D4B0 |
| +0x5E4 | int | reset on activate | 0xDAE6E0 |
| +0x5E8 | float | **speed parameter** 0..1 | 0xDA0810 |
| +0x5EC | float | forced speed parameter (when +0x6C8≠0) | 0xD7D4B0 |
| +0x5FC | float | signed angle CurHeading→DestHeading about +Z | 0xD97E30 |
| +0x600 | vec4 | CurHeading snapshot for this frame | 0xD97E30 |
| +0x63C | ResponseCurve (embedded, vft 0x168BE5C) | **deceleration curve** speed-param → rate/s; keys in §4.1 (earlier read as +0x608: wrong) | ctor 0xDB2D25, keys 0xDA7E62, use 0xDA175F |
| +0x6B4 | int | speed source: 0 = analog (slot 1), 1 = direct (slot 30) | 0xD7C930, 0xD83640 |
| +0x6C8 | int | speed forced by script (use +0x5EC) | 0xD7D4B0 |
| +0x6CC | float | sprint-band blend timer | 0xDA0810 |
| +0x6D0 | float | sprint "settle" weight (1→0 at 1/s once at full sprint) | 0xDA0810 |
| +0x6D4 | u8 | freeze locomotion blend | 0xDA0810 |
| +0x6D8 | float | turn rate min (rad/s) — IHumanGround slot 238 | 0xD95290 |
| +0x6DC | float | turn rate max (rad/s) — slot 239 | |
| +0x6E0 | float | turn angle low threshold (rad) — slot 240 | |
| +0x6E4 | float | turn angle high threshold (rad) — slot 241 | |
| +0x6E8 | float | 15.0 (ctor) **(hypothesis: max crowd-push/lean param)** | 0xDB27B0 |
| +0x6EC | float | 12.5 (ctor) | 0xDB27B0 |
| +0x6F0 | int | forced turn side (−1/0/+1) — slot 242 | 0xD94F30 |
| +0x6F7 | u8 | crouch flag (sets ActorStateID 29 Crouch) — base slot 17 | 0xD84120 |
| +0x720 | float | turn-in-place / U-turn threshold = π/2 (ctor) | 0xD84B10 |
| +0x724 | int | locomotion blend layout (0..7, which blend tree is active) | 0xDA0810 |
| +0x7C0 | int | last foot-contact state (footstep FX) | 0xDA0810 |

### 2.2 HumanGroundData (reflected; vft 0x16F2340; descriptor 0x1996288)
HumanGroundData is **not tuning**: it is the module's runtime context (written every frame by HumanGround).
Reflected fields (offset, type, name; names = CRC32 recoveries, see 07):

| Off | Type | Name | Use in code |
|---|---|---|---|
| +0x10 | vec4 | CollideNormal | obstacle collision |
| +0x20 | vec4 | CurSight | zeroed on enter (0xDA7D20) |
| +0x30 | vec4 | DestSight | |
| +0x40 | vec4 | **CurHeading** | current body forward; rotated by 0xD95290 |
| +0x50 | vec4 | **DestHeading** | desired move direction, set by IHumanGround slot 0 (0xD834F0) |
| +0x60 | vec4 | (crowd-avoid vector) | −dir to nearby entity within 2 m (0xD9E5A0) |
| +0x70 | vec4 | CollidePosition | |
| +0xB0 | enum | SubState (Movement/Fight/FreeRun/OrientedMove/Hurt/ObstacleCollision) | 3 → OrientedMove (0xDAD1C0) |
| +0xB4 | float | CurSpeedRatio | |
| +0xB8 | float | **DestSpeedRatio** = stick magnitude 0..1 (slot 1, 0xD7C930) | 0xDA0810 |
| +0xC4/+0xC8 | int | ParamFlags / InternalFlags | |
| +0xCC | objref | entity being brushed past (crowd) | 0xD9E5A0 |
| +0xD4 | float | **CurrentBodyAngle** (lean toward avoid vector, ±π/2) | 0xDA0810 |
| +0xD8 | float | turn lean angle (±π/2, ±π/4 in "careful" mode) | 0xDA0810 |
| +0xDC | enum | ObstacleLeanType (Hands/Feet) | |
| +0xE0/+0xE4 | float/objref | CollideHeight / CollideEntity | |
| +0x11C | bool | Crouch (slots 16/17) | |
| +0x11F | bool | allow run-stop clip (slots 34/35) **(hypothesis)** | 0xD7EC90 |
| +0x121/+0x122 | bool | slots 273/274 | |
| +0x123 | bool | **Sprint** (slots 36/37) | 0xDA0810 |
Non-reflected runtime part (> 0x124): +0x184..+0x1B4 stick→mode threshold tables (`HumanGroundData__GetMoveModeFromStick` 0xC7F000:
count per profile at +0x185+p, thresholds at +0x188+20p+4i; reset on enter to {0, 0.9, 1.0} for both profiles (0xDA7D20));
+0x1B1 high-profile bool; +0x1DC current ActorState speed (0/3 walk/5 run/6 sprint) (0xD83C10);
+0x238/+0x23C turn-rate ramp k and k-rate (0xD95290); +0x244 transition tag; +0x248 module entry mode (0xDAE6E0).

## 3. State machine

Root children: **1** (+0x80, normal ground), **83** (+0x1D5, OrientedMove-like obj @+0x1D0, F26580/F257A0),
**84** (+0x1DD, obj F695E0), **85** (+0x1E5, F69B20 / entry mode 4), **86** (+0x1ED, F6DC80 / entry 2,3), **87** (+0x1F5, F6BD30).
Entry chosen from Data+0x248 at activation (0xDAE6E0): 0/5/6 → 1, 1 → 83, 2/3 → 86, 4 → 85.

State 1 children: **2** locomotion (+0x83), 56, 57 (GetUp obj F2C0B0), 58 (F514F0), 59 {60,61}, 62–69 (fight/block
state objects F55AF0..F67A60), 70 {71,72}, 73, 74 {75..80}, 81, 82.
State 2 children: **3** Movement (+0x86), 36, 37, 38 (weapon-related), 39, 40 (exit from OrientedMove, uses 0xD9DA20),
41, **42** OrientedMove (obj +0x104, F28570/F29900; entered when Data.SubState==3), 43 (obj F29DB0), 44, 45 (obj F2AF70),
46 {47..50}, 52 {53,54,55}.
State 3 (Movement) children — the locomotion core:

| ID | rec | Meaning | Enter / Update | Evidence |
|---|---|---|---|---|
| 4 | +0x89 | **Idle / standing** (children 5..10) | 0xDA6AB0 / 0xDA8B90 | transitions below |
| 11 | +0xA4 | **Moving** (children 12..17) | 0xDA4290 / 0xDA8C80 | calls MoveBlend 0xDA0810 |
| 18 | +0xB9 | **Run stop** (fast stop clip) | 0xD98E30 | from 11 via 0xD7EC90 |
| 19..23 | +0xBC..+0xC8 | short reaction/anim states (19 0xD85150, 20 plays clip table 0x1A2C0D4, 22/23 set ActorState 23/24) | 0xDA8890 | |
| 25 | +0xCE | **U-turn / pivot** (turn > π/2 with input) | 0xDA6150 | from 4 via 0xD84B10 |
| 26..35 | | misc (27 has children 28..; 30 has child 31) | 0xD7E7D0, 0xD88E60, 0xD859D0, 0xD85B30, 0xDA2E20 | |

Transitions (all checked in Update; `anim gate` = current clip finished (0x501870) or the clip's exit flag for the
requested mode/profile is set (0xD80010: flag bit 0x40/0x80 idle low/high, 0x100/0x400 walk, 0x200/0x800 run; bit 0x20 = locked)):

| From → To | Condition | Addr |
|---|---|---|
| 4 Idle → 11 Move | desired mode ∈{1,2} and anim gate; on enter plays start clip (0xD98990) | 0xD84AC0 |
| 4 Idle → 25 Pivot | anim gate and |angle(CurHeading,DestHeading)| > HG+0x720 (π/2) and not blocked | 0xD84B10 |
| 4 Idle → turn in place | HG+0x290==1, no move input, |angle| ≥ 1° (0.017453292) | 0xD84B80 → 0xDA7610 |
| 4 Idle → (crouch-walk) | Data+0x11C && !HG+0x5E0… | 0xD84C10 → 0xDA7640 |
| 11 Move → 18 RunStop | in run band (+0x5DC), foot phase set, clip not locked, Data+0x11F, and (clip is locomotion node 93469659 or anim id 96/97 or clip time > 0.33) | 0xD7EC90 |
| 11 Move → 4 Idle | mode==0 and (walk band or clip ≠ locomotion node); plays stop (0xD8B220) | 0xD7ED30 |
| 11 → others | 0xD84F10 → 0xDA76C0; 0xD7ED80 → 0xDA76E0 | |
| 3 → 42 OrientedMove | Data.SubState == 3 | 0xDAD1C0 |
| 1 → InAir (deferred) | `Human__ShouldFallOffSupport` (0xB23CB0) true | 0xDB46B0 → 0xD8ADB0 |
| module → InAir | `HumanGround__CheckGroundLoss` (0xD87720) → 0xD8C380 | 0xDB4F00 |
| events | IHumanGround "DoX" events dispatched to current state; Movement event table 0xDB1470 (ids 1,2,7,8,11,13,38–43,45,49–52,54–58,60,63,65–72,85,86,96,100–107,124), each with a guard fn and an action | 0xDB1470 |

Start-move clip choice (0xD98990): foot in front from bone positions (0xB18850 returns 1/2);
walk start = anim 161516230 / 161522726, run start = 161524269 / 161524270; initial speed param 0.25 (walk) or 0.5 (run).

## 4. Per-frame logic (pseudo-code)

```
HumanGround.Update(dt):                                   // 0xDB4F00 (base slot 13)
  if CheckGroundLoss(): build TransitionToInAir(FallType()); return     // 0xD87720/0xD8C380
  dispatch current root state update (state1: 0xDB46B0)
State1.Update: if !inEvent && ShouldFallOffSupport(): defer TransitionToInAir(FallOrigin=Ground)
Movement(3).Update (0xDAD1C0):
  PreUpdate (0xD97E30):
     desiredMode = Data.stickModeTable(highProfile, Data.DestSpeedRatio) ? 2 : 0   // stick>0 → move
     highProfile = Data[0x1B1]
     broadcast ActorState Walk(55)/Run(56)/Sprint(65)                              // 0xD83C10
     cur = Data.CurHeading;  turnAngle = signedAngleZ(Data.DestHeading, cur)
     if footPhase: UpdateHeading()                                                 // 0xD95290
  if Data.SubState==3 → OrientedMove
  run child state (Idle / Move / Stop / Pivot …)
Move(11).Update → MoveBlend (0xDA0810):
  stick = Data.DestSpeedRatio
  target = stick*0.25 + (highProfile ? (Data.Sprint ? 0.75 : 0.5) : 0); clamp ≤1
  if speed < target: speed = min(target, speed + dt)                // 1.0 / s
  else: speed = max(target, speed - decelCurve(speed)*dt)           // curve at HG+0x63C (§4.1)
  band: ≤0.25 walk (w = speed*4), ≤0.5 jog, ≤0.75 run, else sprint; set +0x5DC/+0x5D4
  Data.CurrentBodyAngle = lean toward crowd-avoid vector, max π/2, slerp rate 5·dt  (0xD94C80)
  Data[0xD8]            = lean toward DestHeading, max π/2 (π/4 if careful), rate 5·dt
  write 17 blend weights: per band {straight, lean-left, lean-right}, cross-faded by band fraction
UpdateHeading (0xD95290):
  if clip flag 0x10 (anim owns rotation): skip
  k = min(1, Data[0x238] + Data[0x23C]*dt)
  CurHeading = RotateTowards(CurHeading, DestHeading, rateMin, rateMax, angLo*k, angHi*k, side)
  entity.SetForward(CurHeading, up=(0,0,1))
RotateTowards (0xD94F30): angle = unsigned angle (or forced side, 0..2π)
  rate = angle<angLo ? rateMin : angle≥angHi ? rateMax : lerp; step=rate*dt; snap if step≥angle
```
Translation = root motion of the blended clips (no velocity code in the module).

### 4.1 MoveBlend in full (`HumanGround__UpdateMoveBlend` 0xDA0810, verified)
MoveBlend has two paths. Which one runs depends on the action that is playing (0xDA08C0):
- If the action is **not** `0x05923BDB` (93469659), the start/transition layouts 1–7 (HG+0x724) are used. They are not covered here.
- Otherwise the **locomotion path** below runs. It also sets the graph mode `HG+0x580 → +0x44 = 2`.

#### 4.1.1 Speed parameter (0xDA1700–0xDA18A2)
| Case | Speed param HG+0x5E8 | Slowdown timer HG+0x6CC |
|---|---|---|
| \|target − s\| ≤ 0.0005 | unchanged | −4·dt (≥ 0) |
| s ≤ target | `min(target, s + dt)` | −8·dt (≥ 0) |
| s > target, not forced | `max(target, s − curve(s)·dt)` | +dt (≤ 1) |
| s > target, forced (HG+0x6C8) | `max(target, s − dt)` | unchanged |

**The deceleration curve** is a `scimitar::ResponseCurve` embedded at **HG+0x63C**.
- Constructed by `ResponseCurve__ctor` 0x5637C0 from the HumanGround ctor at 0xDB2D25.
- Its keys are added in `HumanGround__OnEnterInit` 0xDA7D20 at 0xDA7E62–0xDA7ED0 (`ResponseCurve__AddKey` 0x563140): **(0, 1) (0.333, 1) (0.4, 0.3) (0.666, 0.2) (1, 1)**.
- `ResponseCurve__Evaluate` 0x5631D0 is piecewise linear. Every segment that contains x is evaluated in key order and the last one wins. Outside the keys it returns 0.
- So a slow-down from sprint is fast at the top (rate 1 → 0.2), slow through the run band (≈ 0.2–0.3/s) and fast again below 0.333.
- Worked example: sprint (1.0) to run target 0.75 reaches 0.841 after 0.2 s. From 1.0 down to 0.5 takes about 1.39 s.

#### 4.1.2 Bands and the in-band fraction f (0xDA18C0)
| s | lower slot | upper slot | f | side effects |
|---|---|---|---|---|
| ≤ 0.25 | 0 slow walk | 3 walk | 4s | slowdown := 0, settle := 1, `sub_DC3FA0(1−f, f)` (not traced) |
| ≤ 0.5 | 3 walk | 6 jog | 4(s−0.25) | settle := 1 |
| ≤ 0.75 | 6 jog | 10 run | 4(s−0.5) | settle := 1 |
| > 0.75 | 10 run | 13 sprint | 4(s−0.75) | settle HG+0x6D0 −= dt when f > 0.99 or s > target |

f is clamped to ≤ 1. The slot pairs are packed in a 16-bit local, with the low byte the upper slot and the high byte the lower slot: 3, 0x0306, 0x060A, 0x0A0D.

#### 4.1.3 Lean and bank (0xDA1A3F–0xDA1C2A, `HumanGround__UpdateLeanAngle` 0xD94C80)
- `UpdateLeanAngle(angle, targetDir, max)` works in three steps:
  1. It rebuilds the filtered direction as the heading snapshot HG+0x600 rotated by −angle about Z.
  2. If the target is non-zero (|v| > 0.001), it moves towards it by the **fraction 5·dt** in angle space along the short way (`Math__LerpHeadingAngle` 0xD4C390). Otherwise it moves back to the heading by the **fraction 0.1 per frame**.
  3. It returns `SignedAngle(filtered, heading, Z)` (`Math__SignedAngleAroundAxis` 0x55E570, sign = `cross(a,b)·axis`, 0x55E2C0), clamped to ±max.
- A **positive angle means the filtered direction is right of the body**, and it selects the right clips.
- **Lean** Data+0xD4: target is the crowd-avoid vector Data+0x60, max π/2. The vector is zeroed after use.
- **Bank** Data+0xD8: target is DestHeading Data+0x50, max **π/2**, or **π/4** (0x3F490FDB) when IHumanGroundAccess vt1120 returns true. When HG+0x6F0 = ±1 (forced turn side), the target is first limited to 3π/4 on that side.
- Normalised: `L = |lean|/(π/2)` and `B = |bank|/(π/2)`, each ≤ 1. The side clip is slot+1 for angle < 0 (left) and slot+2 for angle ≥ 0 (right).

#### 4.1.4 The 17 weights (0xDA1C8C–0xDA1FCB → `AnimGraph__SetItemBlendWeights` 0x503040)
Slot order of both items of `0x05923BDB` (`RE/data/action_graph_movement.txt`):

| Slots | Clips |
|---|---|
| 0–2 | `walk_slow_hip{m,l,r}` |
| 3–5 | `walk_hip{m,l,r}` |
| 6–9 | `jog_hipm`, `jog_bank_left`, `jog_bank_right`, `jog_slowdown` |
| 10–12 | `run_hipm`, `run_bank_{left,right}` |
| 13 | `sprint_hipm` |
| 14–15 | `run_bank_{left,right}` again (the sprint band's sides) |
| 16 | `sprint_impultion` |

S is the total side weight: L for s ≤ 0.25; B for s > 0.5; `(1−f)·L + f·B` for 0.25 < s ≤ 0.5. In the formulas below, "side" stands for L or B, whichever that band uses.

| Band | lower mid | upper mid | extra | lower side | upper side |
|---|---|---|---|---|---|
| ≤ 0.5 | (1−f)(1−S) | (1−slowdown)·f(1−S) | slot 9 = slowdown·f(1−S) | side·(1−f) | side·f |
| ≤ 0.75 | (1−slowdown)(1−f)(1−S) | f(1−S) | slot 9 = slowdown·(1−f)(1−S) | B(1−f) | B·f |
| > 0.75 | (1−f)(1−S) | (1−settle)·f(1−S) | slot 16 = settle·f(1−S) | B(1−f) | B·f |

In the walk→jog band (0.25 < s ≤ 0.5), the exe uses `(1−f)·L` and `f·B` as the side factors and then multiplies them by (1−f) and f again. So that band's weights sum to `1 − S + (1−f)²L + f²B` instead of 1. This is verified by the decompile and is not a transcription slip.

`SetItemBlendWeights` clamps each weight to [0, 1] and writes them to the playing item instances (0x726280 / 0x726200).

Consequences:
- **Jog slowdown:** while decelerating, the slowdown timer blends `xx_h_jog_slowdown` in place of `jog_hipm`.
- **Sprint impulsion:** on reaching the sprint band, `sprint_impultion` plays first (settle = 1). It hands over to `sprint_hipm` at 1/s once the band is full.
- **Walk band:** only crowd avoidance leans the hips. Turning shows from jog upwards (bank).

#### 4.1.5 Root motion
All 34 clips have 2-key DISPLACEMENT tracks, so each one has a constant speed (RE/10 §6, `RE/data/anim_root_motion_gamefix.txt`).

| Slot | footl d (m) / T (s) | footr d (m) / T (s) |
|---|---|---|
| 0 slow walk m | 0.170 / 1.6667 | 0.150 / 1.4667 |
| 1–2 slow walk l/r | 0.269 / 2.0 | 0.269 / 2.0 |
| 3 walk m | 1.012 / 0.5333 | 0.885 / 0.4667 |
| 4–5 walk l/r | 0.992 / 0.6 | 0.992 / 0.6 |
| 6, 9 jog / slowdown | 1.651 / 0.4667 | same |
| 7–8 jog bank | 1.707 / 0.4667 | same |
| 10–12, 14–15 run (+bank) | 1.707 / 0.3333 | same |
| 13, 16 sprint / impulsion | 1.674 / 0.2667 | same |

**Verified (RE/04 §4.1.6):** the clips of an item share one normalised clock. The item lasts **Σwᵢ·Tᵢ**, not normalised (`AnimItem__GetBlendedDuration` 0x507650 → 0x5B9480), and its root covers the blended displacement Σwᵢ·dᵢ (`AnimItem__GetBlendedDisplacement` 0x5084F0).
- The only weight check rejects an all-zero set (0x507880).
- So the band tops are exact: walk 1.898, jog 3.538, run 5.121, sprint 6.277 m/s.
- Between bands the speed is below a linear mix: 4.20 m/s at s = 0.625 instead of 4.33.
- In the walk→jog band (weights not summing to 1) the step lasts Σwᵢ·Tᵢ as is.
- A runtime trace is still worth doing to confirm the speeds.

**Port:** `port/src/player/move_blend.rs` implements all of the above. The ground context moves by `MoveBlend::advance`, and the animator plays action `0x05923BDB` (item of the leading foot) with these weights at the sim's step phase.

## 5. Constants

| Addr / where | Value | Meaning |
|---|---|---|
| 0x192DDA8 (global) | 1/30 default | frame dt (followed by 30.0 = fps) |
| 0xDA0810 | 0.25 / 0.5 / 0.75 | band boundaries of the speed parameter |
| 0xDA0810 | 1.0·dt | speed-param acceleration |
| 0xDA0810 | 5·dt, π/2, π/4 (0x3F490FDB) | lean slerp rate and limits |
| 0xDA0810 | 2.3561945 (3π/4) | max lean deviation when HG+0x6F0 forced |
| 0xDA0810 | 0.99 | sprint "settled" threshold |
| 0xDB27B0 | π/2 (0x3FC90FDB) @HG+0x720 | U-turn threshold |
| 0xD84B80 | 0.017453292 (1°) | idle turn-in-place threshold |
| 0xDA7D20 | {0, 0.9, 1.0} | stick thresholds per profile |
| 0xDA7D20 | curve (0,1)(0.333,1)(0.4,0.3)(0.666,0.2)(1,1) | Data curve built on enter (hypothesis: decel/step-phase curve) |
| 0xDA7D20 | capsule 1.8 / 0.4 (sub_52ED20/52ED40); offsets r−0.4, r−0.4, r−0.15 | ground collision capsule |
| 0xDAE6E0 | controller +0x60=0.58, +0x64=0.37, flags +0x7F,+0x80=1 | character-controller params on ground entry (hypothesis: step height 0.58, ground snap 0.37) |
| 0xD671C0 (player) | turn rate min 4.712389 (270°/s), max 6.2831855 (360°/s); thresholds 0 → always max | player heading turn rate |
| 0xED72E0 (NPC goal) | 1.5 / 4.0 rad/s, 10° / 60° | NPC turn rates |
| 0xD80280 | 0.375 / 1.0 | turn rates in "careful" mode (slot 280) |
| 0xD9E5A0 | 2.0 m, local y∈(−0.5,1.5), |x|<1 | crowd-brush detection box |
| 0xB23CB0 | normal.z 0.7071 / 0.5, probe 0.8 m | fall-off-support test |
| 0xD87720 | probe 0.5; time-to-ground < 0.01 | ground loss |
| 0xD8C380 | 1.0, 2.0, 8.0 m; 2.5 m/s | fall-type selection (0/1 small, 2/3 medium, 4/6 high; odd=fast) |
| 0xD9F4C0 | 0.53 height unit, 0.25 sweep radius, +0.6+0.5 height | event-72 obstacle/edge guard (hypothesis: vault/climb-up check) |

Tuning values are hard-coded or set by controllers; per-clip behaviour (exit windows, rotation ownership) is in
**animation data flags** (`sub_5021F0(0)+0x3C` bits), not in HumanGroundData.

## 6. Interfaces
* In: IHumanGround slots — 0 SetDestHeading (0xD834F0), 1 SetDestSpeedRatio (0xD7C930), 30 SetSpeedDirect (0xD83640),
  33 GetSpeedBand (0xD807B0), 16/17 Crouch, 34/35 Data+0x11F, 36/37 Sprint, 238–242 turn params; odd/even pairs
  `Can(ev)`=0xDB48B0 (returns 2 if accepted) / `Do(ev)`=0xDB4A40. Player controller StPadControlled (0xD671C0, 0xD66F50, 0xD63940).
* Out: animation graph (sub_501190 play clip, sub_502E30 set blend weights, sub_5021F0 clip flags), Human actor-state
  broadcast (sub_B1CBD0 Walk55/Run56/Sprint65/Crouch29), TransitionSetupDataToInAir (0x16FF678), ToDead, ToRagdollGround,
  ToHumanHayStack; ledge/climb/beam hand-over is **not** issued by HumanGround — it happens through Human-level
  decision/guidance (only the event-72 guard 0xD9F4C0 queries guidance edges from here).

## 7. Open questions / dynamic checks
* ~~Confirm decel curve object at HG+0x608 and its keys~~ → done statically: HG+0x63C, §4.1.
* Clip time sync inside a blend item (phase-synchronised? normalised by total weight?) — runtime check, §4.1.5.
* Identify state names 5–10, 12–17, 26–35 (log state ids at 0xDAD1C0 while walking/stopping/turning).
* Verify meaning of controller +0x60/+0x64 (0.58/0.37) in report 01.
* Map IHumanGround event ids (0x01..0x88) to actions (push, crouch, free-run…).

## 8. Renamed functions
| Addr | Name |
|---|---|
| 0xDB27B0 | HumanGround__ctor |
| 0xD90D00 | HumanGround__InitStateTree |
| 0xDA7D20 | HumanGround__OnEnterInit |
| 0xD7C8A0 | HumanGround__QueryInterface |
| 0xD834C0 | HumanGround__Init |
| 0xD84120 | HumanGround__SetCrouch |
| 0xDAF160 / 0xDAF200 | HumanGround__Activate / __Deactivate |
| 0xDB4F00 | HumanGround__Update |
| 0xDAE6E0 | HumanGround__OnActivateSetup |
| 0xDB46B0 / 0xDACE80 | HumanGround__State1_Update / _Enter |
| 0xDAF2D0 / 0xDAAA60 | HumanGround__Locomotion_Update / _Enter |
| 0xDAD1C0 / 0xDA8890 | HumanGround__Movement_Update / _Enter |
| 0xDB1470 | HumanGround__Movement_HandleEvent |
| 0xDA8B90 | HumanGround__Idle_Update |
| 0xDA8C80 / 0xDA4290 | HumanGround__Move_Update / _Enter |
| 0xDA0810 | HumanGround__UpdateMoveBlend |
| 0xD97E30 | HumanGround__Movement_PreUpdate |
| 0xD7D4B0 | HumanGround__UpdateDesiredMoveMode |
| 0xD83C10 | HumanGround__UpdateSpeedActorState |
| 0xD95290 | HumanGround__UpdateHeading |
| 0xD94F30 | HumanGround__RotateTowards |
| 0xD94C80 | HumanGround__UpdateLeanAngle |
| 0xD9E5A0 | HumanGround__UpdateCrowdAvoidVector |
| 0xD80010 | HumanGround__AnimAllowsModeExit |
| 0xD84AC0 / 0xD84B10 / 0xD84B80 | HumanGround__Idle_CanStartMove / _CanUTurn / _CanTurnInPlace |
| 0xD7EC90 / 0xD7ED30 | HumanGround__Move_CanRunStop / _CanStop |
| 0xD98990 | HumanGround__PlayStartMove |
| 0xD87720 | HumanGround__CheckGroundLoss |
| 0xD8C380 | HumanGround__TransitionToInAirFall |
| 0xD8ADB0 | HumanGround__TransitionToInAirOffSupport |
| 0xD807B0 | HumanGround__GetSpeedBand |
| 0xD834F0 / 0xD7C930 | HumanGround__SetDestHeading / __SetDestSpeedRatio |
| 0xD80280 | HumanGround__ApplyCarefulTurnRates |
| 0xD9F4C0 | HumanGround__Guard_Event72_EdgeCheck |
| 0xDB48B0 / 0xDB4A40 | HumanGround__CanHandleEvent / __PostEvent |
| 0xC7F000 | HumanGroundData__GetMoveModeFromStick |
| 0xB23CB0 | Human__ShouldFallOffSupport |
| 0x192DDA8 (data) | g_FrameDt |

## Methodology
RTTI vftables via vt.py; decompiled ctor/Enter/Exit/Update slots, followed the nested dispatchers, disassembled all
401 IHumanGround slots with capstone, cross-checked HumanGroundData fields against the reflection layout (07) and
CRC32 name recovery (new: CurSight, CurHeading, CurSpeedRatio, DestSpeedRatio).
