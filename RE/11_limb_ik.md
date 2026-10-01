# 11 — Limb IK (hands and feet on holds)

Status (second pass in §5): the per-limb layer is mapped: weights, travel between holds, contact timing from animation tags, and the hand-off to the solver. The solver itself is third-party full-body middleware and was not reversed. The pelvis/lean post-adjustments are mapped at the constant level only.

## Method
1. Started from the facts already in RE/03:
   - Climb and Ledge store contacts in a component at Human+1328 (4 limbs × 144 bytes);
   - `HumanClimb` pass 1 calls `sub_E58EE0`.
2. Decompiled `sub_E58EE0` and its two callees, then the helpers they use (`sub_E560C0`, `sub_E56220`, `sub_E56450`, `sub_E55EF0`, `sub_E57400`).
3. Followed the effector call `sub_4FB5A0` → `sub_4FB330` and the solve call `sub_4FADA0` → `0x10Cxxxx`.
4. Searched the strings for effector names.
5. Renamed the functions in the IDB (listed below) and commented 0xE57570 and 0xE56FC0.

## 1. Component layout (Human+1328, `LimbIK`)

Four limbs: 0 = L hand, 1 = R hand, 2 = L foot, 3 = R foot (same numbering as RE/03 §4). Each limb is a 144-byte record starting at component+2.

| Off (in record) | Meaning | Source |
|---|---|---|
| +0 (byte) | limb active (has a contact) | 0xE57570 |
| +1 (byte) | contact pending (waiting for anim tag) | 0xE56FC0 |
| +2 (float) | IK **weight** 0..1 | 0xE57570 |
| +6 (byte) | contact fixed (no travel) | 0xE57570 |
| +10 / +14 (float) | travel window t0 / t1 (s, animation sequence time) | 0xE57570, 0xE56FC0 |
| +46, +94 | old / new contact (local position) | 0xE57570 |
| +94 / +110 / +126 / +130 | new contact pos / normal / guidance object / element type | `LimbIK__SetTarget` 0xE55EF0 (writes +96/+112/+128/+132 relative to the 144-byte base) |

Other fields:
- Per-limb effector handles sit at component+576 + 16·i.
- The animation contact-tag mask byte sits at component+584 + 16·i.
- Flags at +656; pelvis/lean state at +660/+664/+668/+672.

## 2. Per-frame update (`LimbIK__PostUpdate` 0xE58EE0)
It runs from the context's post pass, after the animation has been sampled.

**`LimbIK__UpdateContactsFromAnimTags` 0xE56FC0:**
- A limb attaches and detaches according to **animation contact tags**: the current clip's tag bits are ANDed with the limb's mask byte.
- When the tag drops, the limb is released.
- A grab is predicted when the next contact (`LimbIK__GetNextContactTime` 0xE56220) is less than **0.2 s** away.

**`LimbIK__SolveEffectors` 0xE57570:**

1. **Weight.**
   - Active: `w = min(1, w + 4·dt)`, a 0.25 s fade-in.
   - Released: `w = max(0, w − 5·dt)`, a 0.2 s fade-out.
   - A weight of 0 skips the limb.
2. **Goal.**
   - A fixed contact uses the contact position.
   - A moving limb uses
     `goal = lerp(old + animDelta(t0→t), new, s)`, with `s = (min(t, t1) − t0) / (t1 − t0)` (s = 1 if the window is ≤ 0.0005 s).
   - `animDelta` (`LimbIK__AccumAnimEffectorDelta` 0xE56450) is the limb end bone's own motion in the playing clip(s), rotated into world space. The hand therefore follows the clip's reach arc, and a linear correction pulls it onto the real hold. This is the same "clip + linear correction" pattern as the jump warp in RE/04.
   - If the contact's guidance object has gone, the limb is released (`LimbIK__ReleaseLimb` 0xE567E0).
3. **Hand-off.** `IKRig__SetEffectorGoal(effector, goal, weight, up = (0,0,1))` (0x4FB5A0).
4. **Post-adjustments** (flags at +656; constants verified, purpose is **(hypothesis)**):

   | Flag | Probe | Effect |
   |---|---|---|
   | bit 4 (wall hang) | sweep r 0.2, 0.225–0.75 m, from 0.075 m behind the root | pushes the hips off the wall by `max(0, 0.15 − d)`, rate-limited to 1 m/s; leans up to 0.218 rad / 0.349 × −40° |
   | bit 1 (hand height) | hand height difference | lowers the hips by `0.4 · (1 − |Δz|/0.6) · ((angle − 10°)/30°)` |
   | bit 2 (feet) | foot/knee wall probes at 0.3 m and 0.2 m | rate-limited to 1 m/s; knees rotated up to 45° |

5. **Solve.** `IKRig__Solve` (0x4FADA0) runs the solver at 0x10C68CC/0x10C9234/0x10CA068.

`LimbIK__Activate` (0xE57400, called on Climb Wait enter) releases all four limbs and clears the flags and the pelvis state.

## 3. The solver
- The effector name table at 0x1A4B0E4 follows Autodesk HumanIK's naming and order: `HipsEffector`, `LeftAnkleEffector`, `RightAnkleEffector`, `LeftWristEffector`, …, `RightFootExtraFingerEffector`.
- Together with the out-of-line solve code at 0x10Cxxxx, this indicates a **full-body middleware solver (HumanIK-style)** **(hypothesis on the vendor)**.
- Effectors are **wrists and ankles** with pull weights. `HumanClimbData` has UseIK +0x3C and Left/RightToePull, Left/RightHandPull +0x40..+0x4C, all defaulting to 0.5 (RE/03 §2.2). A pulling effector moves the body towards it.

## 4. Port (`port/src/ik.rs`)
- **Same as the game:**
  - 4 limbs; weights rise at 4/s and fall at 5/s;
  - a limb moving to a new hold follows the game's rule `lerp(old + animDelta, new, s)`, where `animDelta` is the clip's own motion of that wrist/ankle since the move started;
  - a released limb fades from where it let go.
- **Contact fit:** stands in for the solver's pull. Before solving, the animated body is translated by the weighted mean of (goal − animated wrist/ankle), capped at 0.35 m. The clips were authored around the holds (§5), so this is a few centimetres in steady states.
- **Reach pull:** hands still beyond 97 % of arm length pull the body further. Several passes run, because pulling toward one hand can move the other shoulder away.
- **Solver:** analytic two-bone IK per limb, using Arm→ForeArm→Hand and UpLeg→Leg→Foot. It keeps the animation's bend plane, with elbows falling back to bend backwards and knees forwards.
- **End bones:** the hand/foot then gets its **animated world orientation** back, so grips and toes stay as authored instead of swinging with the forearm or shin.
- **Pinned limbs:**

  | Context | Pinned limbs |
  |---|---|
  | Climb | all four |
  | Ledge hang | the hands only; the feet follow the hang clip (braced against the wall, or dangling) |
  | Pull-up | none: the hands are released, standing in for the game's contact tags |

- **Verification:** `AC_IK_LOG=n` logs, every n frames after transform propagation, each weighted wrist/ankle error plus the fit size. Steady states show 0.000 m error with a fit of 0.5–9 cm. Unit tests cover two-bone closure, out-of-reach behaviour, the weight rates and the clip-arc travel. An install test checks the chains exist.
- **Not ported:**
  - animation contact tags;
  - the pelvis/lean post-adjustments of §2.4;
  - the solver's exact pull weighting.

## 5. Second pass: placement and transitions taken from the game's clips
The first port placed the body with guessed offsets and blended pose loops. That produced the problems reported in testing: the climber was inside the wall, the wall-hang legs were twisted, the pull-up clipped through the ledge, the fall looked frozen and the fingers looked wrong.

**Method:**
- `port/src/assets/probe.rs` (ignored tests, run with `--ignored --nocapture`) decodes clips from the user's install.
- It runs forward kinematics on Altaïr's skeleton and prints wrist, ankle, hip and finger positions in animation space (x right, y forward, z up, m), plus the DISPLACEMENT track (root motion).
- It also lists clip names (4,996 clips in DataPC.forge "Game Fix") and checks track coverage.

**Findings** (all verified from clip data):

### 5.1 Resting contacts (t = 0)

| Clip | Wrists | Ankles | Hips | Middle-finger joint vs wrist |
|---|---|---|---|---|
| `xx_h_hangwall_wait` | (±0.30, 0.45, 1.00) | (±0.08, 0.34, 0.05–0.15) | (0, 0.01, 0.12) | +0.07 fwd, +0.12 up |
| `xx_h_hangfree_wait` | (±0.30, −0.04, 2.30) | ≈(±0.1, 0, 0.26) | (0, 0.02, 1.13) | +0.06 fwd, +0.11 up |
| `xx_climb_wait_1m` | (±0.09, 0.44, 1.13) | (±0.04, 0.32, 0.03) | (0.03, 0.30, 0.89) | +0.07, +0.12 |
| `xx_climb_wait_2m` | (±0.36, 0.40, 1.12) | (±0.28, 0.35, 0.06) | (0.02, 0.28, 0.88) | +0.07, +0.12 |

What follows from these numbers:
- **Wall hang:** the reversed hang root (hands 1.1 m above, 0.5 m out; 0xDD6730) matches the clip with the wrist about 0.1 m below and 0.05 m out from the edge point.
- **Free hang:** the 2.4 m drop matches as well. The clip puts the root only 0.01 m out, not the old 0.3 m placeholder (`FREE_HANG_OUT` is now 0.01).
- **Climb:** the root is 0.49 m from the hold line (wrists 0.44 m forward + 0.05 m out); the old placeholder of 0.35 put the hips about 12 cm into the wall. The lower ankle sits 0.03 m above the root. Hands sit 1.10 m above the feet, against the grid's 1.2 m, and the contact fit absorbs the 0.1 m difference.
- The game builds the climb root at runtime from the hold frames (`HumanClimb__ComputeRootFromMove` 0xDEC6E0 → frame builder 0xB1CC20). There is no constant wall offset, so the clip-derived offset is the faithful choice.
- **Grip:** every hang and climb clip curls the middle finger the same way (+0.07 fwd, +0.12 up from the wrist).
  - **PORT:** the wrist goal is 0.08 m below and 0.04 m out, so the fingers end over the lip.
  - **Level change:** climb hold edges moved to the stone bands' outer top edge (8 cm out of the face), where a hand can actually grip.

### 5.2 Root motion
The DISPLACEMENT track moves the animation origin. Bone positions are relative to that moving origin.
- **Pull-up from a wall hang** (`xx_h_hangwall_tr_hangknee_footl_a`/`_b`): the root rises 1.0 m, then moves 0.6 m forward, ending kneeling on the edge with one leg still over it. `xx_h_hangknee_footl_tr_h_wait_footr_a`/`_b` then stand up (+0.26 m forward). Total 1.6 s.
- **Pull-up from a free hang:** `xx_h_hangfree_tr_hangwaist_a`/`_b` (−0.5 m back, +1.4 m up) then `xx_h_hangwaist_tr_hangknee_footl` (+0.6 m forward, +1.0 m up). Total 2.6 s with the stand-up.
- **Climb moves:** `xx_l_climb_<from>_<dir>_<to>`, 54 clips of 0.533 s.
  - Pose names follow the pose table order: 1m, 1lu, 1ru, 2m, 2lu, 2ru. Directions are u/d/l/r/ul/ur/dl/dr.
  - A move that raises the lower foot carries +0.6 m root motion (one row). One that only lifts the higher side carries none.
  - So the climb root follows the lower foot.
- **Port warp:** the logical root moves linearly over the clip's length. The visual root adds `disp(t) − disp_end·phase`, so the body follows the clip's path and lands where the logic expects. This is the same "clip + linear correction" scheme as the game's jump warp (RE/04).

### 5.3 Fall
- `xx_h_jumpfalling01` (0.667 s) and `xx_h_jump_falling01.man` are a **single held pose**: every sample is identical.
- Motion comes from the entry transitions, which raise the arms into that pose:
  - `xx_h_jumpstraight_clear_footall_tr_fall` (after a jump);
  - `xx_l_walklowfall_footl_tr_fall` / `xx_h_runlowfall_footl_tr_fall` (walking or running off an edge);
  - `xx_h_hang{wall,free}_tr_fall_a`/`_b` (letting go).
- **PORT deviation:** while held, the pose sways slowly (1.1 Hz) through the last third of the jump-to-fall clip, using only the game's frames.

### 5.4 Catching and jumping to ledges
- **Catching from a fall:** `xx_fall_tr_hangwall_straight_min_a`/`_b` (0.667 s) and `xx_fall_tr_hangfree_min_a`/`_b` (0.867 s). `GRAB_TIME` is now 0.667.
- **Jumping at a ledge:** `xx_h_jumpstraight_footl_to_hang{wall_250cm,free_300cm}` (flight, ending in the hang pose), then `…_tr_hang{wall,free}_a`/`_b` (reception).
- Running up a wall uses the Walling context (`xx_h_wallingfront_*`, `impultionstraight_*_to_wallingfront_*`). That is not ported, so the port's ground jump-up uses the jump-to-hang clips.

### 5.5 Track coverage
All named finger bones are animated in these clips. The 32 unnamed bones are never animated (twist/helper bones, rest pose).

### 5.6 Visual verification
- **Capture mode:** `AC_SHOTS=t1,t2,…` with `AC_VIEWS=back,left,right,high,front,hands,fingers,fingers_side` freezes the simulation at each time and photographs it from views relative to the character's facing.
  - Time steps are a fixed 1/60 s, so runs are reproducible.
  - A shadowless fill light lights walls facing away from the sun.
  - `AC_STICK_OFF=a-b` releases the stick in a time window.
- **Scenarios:** `AC_AUTOPILOT=wallhang` (jump up, hang, pull up) and `drop` (walk off 6 m) were added.
- **Checked:**
  - climb: hold, mid-move, and the transfer to the ledge and pull-up at the top;
  - wall hang: catch, hang and pull-up;
  - free hang: jump, frame-by-frame catch and shimmy;
  - walk-off fall;
  - finger close-ups;
  - for each, with IK on and off (`AC_NO_IK=1`).

## Renamed in the IDB
| Address | Name |
|---|---|
| 0xE58EE0 | LimbIK__PostUpdate |
| 0xE56FC0 | LimbIK__UpdateContactsFromAnimTags |
| 0xE57570 | LimbIK__SolveEffectors |
| 0xE56450 | LimbIK__AccumAnimEffectorDelta |
| 0xE560C0 | LimbIK__GetSequenceTime |
| 0xE56220 | LimbIK__GetNextContactTime |
| 0xE55EF0 | LimbIK__SetTarget |
| 0xE57400 | LimbIK__Activate |
| 0xE567E0 | LimbIK__ReleaseLimb |
| 0x4FB5A0 | IKRig__SetEffectorGoal |
| 0x4FADA0 | IKRig__Solve |
