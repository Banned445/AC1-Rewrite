# AC1 movement port (Bevy)

A clean-room recreation of Assassin's Creed (2008) player movement, built from the specs in `../RE/`.
No game code or assets are included. Assets will be read at runtime from your own install,
iw4L-style. Personal and educational use only (see `../README.txt`).

Engine: **Bevy 0.19.1**, which renders through **wgpu**, so the wgpu dependency comes with Bevy.

## Run
```bash
cargo run
```
(The first build compiles Bevy and takes several minutes; after that it's quick.)

## Tests
```bash
cargo test
```
`src/sim_tests.rs` runs the real ground/air systems headlessly at 60 Hz with scripted input. The 9 tests
check:
- low profile tops out in the Walk band, high profile reaches Run, and sprint reaches Sprint;
- the speed parameter ramps at 1.0/s;
- a free-run jump over a 3.5 m gap lands on the target roof;
- chained jumps cross several roofs;
- an 8 m drop is fatal (respawn), and a 6 m drop is a safe roll;
- step-up over 0.3 m works, and walls block.

Debug capture (used for automated screenshots): set `AC_AUTOPILOT=1` (scripted sprint across the
roofs) and/or `AC_SCREENSHOT=out.png` with `AC_SCREENSHOT_AT=<seconds>`.

## Controls
| Input | Keyboard + mouse | Gamepad | Game meaning |
|---|---|---|---|
| Move | WASD (Left Alt = partial stick) | left stick | camera-relative stick, 0.35 dead-zone |
| High profile | hold right mouse | hold RT | run; required for jumps |
| Legs | Space | A | jump (0.3 s buffer); hold with high profile = sprint / free-run (auto-jump at edges) |
| Camera | mouse (left click to capture, Esc to release), wheel zoom | right stick | — |
| Debug | G = show guidance edges, F1 = hide HUD | | |

## Stage 1: what's in
- **Context framework** (`player/mod.rs`): one active locomotion context, numbered like `ActorContextID`.
  Switches are immediate, and the new context skips its first update (`AIActor::SwitchLocomotionContext`
  0x55F7E0).
- **Input** (`input.rs`): dead-zone/speed01, high profile, Legs jump buffer, turn attenuation
  (GoAssassinActionInterpreter 0xEE65A0).
- **Ground** (`player/ground.rs`): speed-parameter bands Walk/Jog/Run/Sprint, target = base + 0.25·stick,
  1.0/s ramp, 360°/s turning, ground-loss → fall (HumanGround, RE/02).
- **Jump targets** (`player/targets.rs`): 45° cone, dz ∈ (−3, +1.3] m, ≤ 7 m, "highest in front"
  (0xE96BF0 / 0xB1EC40).
- **In air** (`player/air.rs`): target-warped jump clips with linear correction, over-drop rule
  (> 5 m → free fall), g = 9.8, drift 4 m/s² ≤ 5 m/s, steering ≤ 15 m/s. Landing classes: heavy > 6.3 m,
  fatal > 7.0 m (respawn), roll if drop > 3 m (HumanInAir, RE/04).
- **Collision** (`collision.rs`): kinematic capsule proxy with sliding and step-up (like the game's
  CharacterController 0x57C7C0).
- **Greybox level** (`level.rs`): roof gaps of 2 / 3.5 / 5 / 6.5 m, a staircase of +1.2 m roofs, and
  3.5 / 6 / 8 m drops. LedgeGrab guidance edges are generated from roof tops.

## Stage 2: ledges and climbing (RE/03, RE/04)
- **Ledge context** (`player/ledge.rs`, ActorContextID 9):
  - **Hanging:** two hand contacts on guidance edges. Wall hang puts the feet 1.1 m below the hands and
    0.5 m out; free hang puts them 2.4 m below. The hang type is re-checked after every step.
  - **Shimmy:** alternating hands (`+0x85` flag), a free-space sphere sweep (r 0.15, 1.55 m, 0.75 out,
    0.2 up), and step = min(d − 0.4, 1.0 − spacing). It is blocked when d − 0.15 < 0.7 (obstacles and
    inner corners), the minimum step is 0.15 m, and chain ends are handled implicitly.
  - **Up/down:** switch to Climb when foot holds exist; else a hand-over-hand step (0.45–1.25 m); else
    a jump up (≤ 2 m, wall hang only); else "blocked up", which enables a **pull-up** when there's
    standing space on top.
  - **Exits:** let go (Legs), back eject (high profile + Legs + away), lost-ledge check.
- **Climb context** (`player/climb.rs`, ActorContextID 10):
  - **Hold grid:** 0.75 m × 0.6 m cells, with 7/8 columns and rows depending on the pose. Cells are foot
    cells, and hands hold two rows higher.
  - **Moves:** the 6 limb poses and the SHORT/LONG move tables exactly as dumped from the game, including
    redirects. The stick is quantised into 10 directions, and pushing past 0.5 tries the LONG table
    first (1.2 m hand-over-hand).
  - **Timing:** each move takes 0.5 s, which is the game's own default when a move has no animation.
  - **Exits:** no move up at a top edge switches to Ledge, which then pulls up. Legs releases.
- **Entries:**
  - **From the ground:** high profile + Legs into a wall starts a climb (hand holds 1.8–2.4 m up with
    foot holds 1.2 m below), grabs a ledge within 2.4 m, or jumps up to a ledge up to 3 m above.
  - **Jump targets:** these now include **ledge (hang) targets**, flag 0x40, with max up 3 m and up to
    8 m away.
  - **Mid-air catch:** falling with Legs held catches an edge in the 0.4 × 0.3 m hand box at +1.95 /
    +1.4 m reach, within 70° of facing.
- **Level additions:** a 9.6 m climb tower with hold bands every 0.6 m and a missing patch, a free-hang
  balcony, and a 2.6 m jump-up wall with a pillar that blocks shimmying.
- **Tests:** 6 new simulation tests (15 total) cover:
  - climbing the tower to the roof via the LONG table and a pull-up;
  - a climb blocked by missing holds;
  - jumping up to a ledge, the wall-hang offsets, and a pull-up;
  - a shimmy stopped by the pillar;
  - a free-hang shimmy that stops at the end of the ledge;
  - letting go into a fall (FallOrigin HangFree).
- **Debug autopilot:** `AC_AUTOPILOT=climb` and `AC_AUTOPILOT=ledge` scenarios.

Not yet ported: corner turns, side jumps between ledges, the ParallelJump/Rebound tables, PullDown
(ground → hang), climb back-eject direction details, the NarrowObject "stand on the ledge edge" step
after a pull-up (it goes straight to Ground), walls and beams.

## Stage 3: Altaïr's model (RE/09)
- **Loading:** at startup the port opens **your own install**'s `DataPC.forge` (default
  `C:\Users\benja\Desktop\Claude\Assassin's Creed`, override with `AC_GAME_DIR`), reads the "Rank 9" file
  and builds Altaïr from it. Nothing from the game is copied into this project. Without the install it
  falls back to the capsule and says why on the HUD. `AC_NO_MODEL=1` disables it.
- **Decoder (`src/assets/`):**
  - forge reader with a clean-room LZO1X decoder and Adler-32 checks;
  - skinned Mesh decoder (s16/2048 positions, packed normals, s16/4096 UVs, 4 bone indices and weights,
    submeshes, bone palettes);
  - CPU BC1/BC3 decoder for TextureMaps (no GPU compression feature needed);
  - material resolution via the entity's override table → TextureSet → DiffuseMapSpec → TextureMap.
- **Assembly:** head, knives and sword sheath are mapped from their bone space into body space. The model
  is converted from Z-up to Y-up, faces −Z and stands with its feet at y = 0.
- **Shown:** 11 parts, 7,765 triangles, 6 textures, in the **bind pose**. Skinning and animation come
  with stage 4.
- **Tests:** CRC32/Adler-32 known values, the forge index read from your install (matches the Python
  reader), and Altaïr's assembly (vertex/triangle counts, feet at 0, height 1.75–2.0 m, head on the
  shoulders, decoded textures). The install tests skip if the game isn't found. `AC_AUTOPILOT=pose` /
  `back` give model screenshots.

## Stage 4: animation (RE/10)
- **Decoder (`src/assets/ac_anim.rs`):** a port of the verified spec. It reads the track table and every
  key compression (smallest-three quaternions at 16/24/32/48/64/96 bits, Vec3 at 32/48 bits in mm,
  Float8/16, byte tracks) with key times in 1/60 s ticks.
- **Clip loader (`src/assets/anims.rs`):** loads idle (low/high), walk, jog, run and sprint from your
  install (`DataPC.forge` → "Game Fix") and joins each gait's left- and right-foot halves into one loop.
- **Skinning:** Altaïr is now a skinned mesh on his 90-bone skeleton. Inverse bind poses come from the
  rest hierarchy, so the rest pose is exact.
- **Playback (`src/anim.rs`):**
  - the clip is chosen from the context and speed band;
  - it plays at actual speed ÷ clip root speed, so the feet don't slide;
  - foot phase is kept between gaits, and changes crossfade over 0.2 s;
  - interpolation is the game's own: nlerp above dot 0.98936, otherwise slerp.
  - While a clip plays, the rig root maps animation space (Z-up, facing +Y, feet at 0) to Bevy.
- **Real speeds:** the ground speed bands now use the measured root-motion speeds (walk 1.90, jog 3.54,
  run 5.12, sprint 6.28 m/s), replacing the placeholders.
- **Tests:** 20 pass. A new one decodes the clips from your install and checks those speeds and that
  every rotation key is a unit quaternion.

### Stage 4b: traversal animation
All clips come from your install (`Game Fix`), chosen by context:

| Context | Clips |
|---|---|
| Ground | idle (low/high), walk, jog, run, sprint |
| Landings | `xx_roll_hipm` (drop > 3 m), `xx_h_landing_damage_footl` (heavy), soft landing, each played once before locomotion resumes |
| In air | `xx_h_jumpstraight_clear_footl` stretched over the target-warped jump, then `xx_h_jumpfalling01` looping while falling |
| Ledge | `xx_h_hangwall_wait` / `xx_h_hangfree_wait`; shimmy uses the game's own alternating-hand pair `xx_h_hang{wall,free}_strafe_{left,right}_050cm_{open,close}` |
| Ledge vertical | hand-over-hand up/down `xx_h_hangwall_{u,d}_climb_1m`; pull-up uses the vault clip `xx_h_hangwall_tr_passover_handr` (stand-in) |
| Climb | the wait clip of the current limb pose (`xx_climb_wait_{1m,1lu,1ru,2m,2lu,2ru}` = poses 0–5), blended over each 0.5 s move |

Ledge move durations now equal the clip lengths: shimmy open 0.533 s / close 0.600 s, hand step up
0.600 s / down 0.533 s.

Limitations:
- No start/stop/turn clips yet.

### Stage 4c: limb IK and traversal fixes (RE/11 §4–§5)
`src/ik.rs` pins the wrists and ankles onto the holds and ledge edges chosen by the Ledge and Climb contexts.

**IK pipeline**
1. **Game rules:** 4 limbs. Weights rise at 4/s (0.25 s) and fall at 5/s (0.2 s). A limb moving to a new hold follows
   the clip's own reach and is corrected linearly onto the hold (`lerp(old + animDelta, new, s)`).
2. **Contact fit, then a two-bone solve per limb.** The fit moves the animated body so the clip's own hands and feet
   meet their goals, standing in for the game's full-body HumanIK-style solver. The two-bone solve keeps the clip's
   bend plane.
3. **Hand and foot orientation:** they keep their animated orientation, so the grip looks as it does in the game.

**Placement from the game's clips**
- **Climb:** the root sits 0.49 m off the holds (it was 0.35 m, which put him inside the wall).
- **Free hang:** the root sits 0.01 m out (it was a 0.3 m guess).
- **Grip:** the wrist sits 8 cm below the edge so the curled fingers hook over it.
- **Climb holds:** these are now the stone bands' outer top edges.

**The game's transition clips, now wired**

| Moment | Clips |
|---|---|
| Climb moves | the 54 `xx_l_climb_<from>_<dir>_<to>` clips, 0.533 s each, using their root motion |
| Pull-up | wall hang → kneel on the edge → stand (1.6 s); free hang → waist → kneel → stand (2.6 s). Hands are released during it |
| Catch from a fall | `xx_fall_tr_hang{wall,free}_straight_min` |
| Jump at a ledge | `xx_h_jumpstraight_footl_to_hang{wall,free}` plus its reception clip |
| Falling | entry transitions after a jump, walking or running off an edge, or letting go of a hang |

- **Root motion:** for these one-shots the visual root follows the clip's DISPLACEMENT track, and a linear correction
  lands it where the logic expects.
- **Ledge hang feet:** they follow the hang clip (braced against the wall, or dangling) and are no longer pinned.
- **Fall loop:** in the game data it is a single held pose. PORT: it sways slowly through the game's own jump-to-fall
  frames.

**Ground locomotion (RE/02 §4.1)**
- The ground context moves by the root motion of the game's locomotion action `0x05923BDB`. That is 17 clips per
  leading foot, with weights set exactly as `HumanGround__UpdateMoveBlend` 0xDA0810 sets them: band fraction, bank toward
  the wanted heading, the jog slowdown timer and the sprint impulsion weight.
- The speed parameter decelerates through the game's ResponseCurve (HG+0x63C).
- The sim owns the step cycle (`player/move_blend.rs`), and the animator shows it at the sim's phase. PORT
  (hypothesis): the clips inside the item are phase-synchronised.

**Ledge moves (RE/03 §7.6b)**
- At the end of a shimmy (or when blocked), the game's order applies: inner corner, side jump, outer corner. Up beyond a hand
  step: the hop.
- Corners play `hangfree_corner_*_090_{in,out}` (free hang) or the wall strafe pair (wall hang).
- Side jumps play the `StartLedgeJump` table's start / loop / end actions.
- The hop plays `swingback_up` `_a` + `_b` into a free hang.
- `AC_AUTOPILOT=ledgemoves` shows a side jump and an outer corner.
- Hang type (wall / free) changes only through moves. A step whose destination has (or lacks) foot support on the wall plays
  the game's switch (`hangfree_tr_hangwall_*` / `hangwall_tr_hangfree_*`, 0xDE1060). `AC_AUTOPILOT=hangswitch` shows both.
- **Pull-down** (RE/03 §7.8b): Legs in low profile at an edge with a drop over 2 m. It plays the game's orientation, descent and
  wall / free reception into a hang. `AC_AUTOPILOT=pulldown`.
- **Ledge stop** (RE/03 §7.8a): walking into an edge with a drop over 2 m stops at it (`ledge_stop_start`, then `ledge_stop_end` steps
  back). Legs during the stop pulls down (EdgeStop). `AC_AUTOPILOT=ledgestop`.
- **Leap of Faith** (RE/04 §4.1.12): run (high profile + Legs) off an edge toward a haystack 3–30 m below. It plays the game's faith
  takeoff and dive, then the haystack landing and wait (context HayStack, 21). The stick hops out. `AC_AUTOPILOT=faith`.
- **Wall run** (RE/05 §1.8): high profile, stick into a wall, press Legs within 1.5 m of it and keep holding Legs. He runs up
  it and catches or pulls up onto an edge in reach, or drops back if there is none. Pushing the stick away from the wall
  rebounds. `AC_AUTOPILOT=wallrun`.
- **Beams** (RE/05 §2.7): walk at a beam end to step onto it. He crouch-walks along it (crouch-jogs in high profile) on the
  game's root motion, stops short of an open end, turns around when you pull back, and steps off onto a floor at the far end.
  `AC_AUTOPILOT=beam`.
- **Beams and pilotis from the air, beam jumps** (RE/05 §2.8): running jumps land on beams (straight or side entries) and on
  wooden posts (pilotis); falling onto either is caught. On a beam or post, high profile + Legs with the stick jumps to the
  next target; without the stick he crouches (impulsion), and Legs again jumps on the spot, or up to a ledge above with the
  game's beam jump flights. `AC_AUTOPILOT=pilotis`, `AC_AUTOPILOT=beamjump`.

**Jumps and landings (RE/04 §4.1)**
- A running jump to a roof edge (free-step target, type 1) plays the game's takeoff item (`run_*_to_air`, 40 clips) then
  its flight item (`air_*_to_freestep`, 16 clips), weighted by height and distance class exactly as
  `Human__ComputeJumpAnimBlend` 0xB1EC40.
- Their Σw·T durations and blended root motion, plus the game's linear correction, carry Altaïr onto the target.
- Arrival plays the free-step reception. Ground contact plays the landing chosen by `SetupToGround_Landing` 0xE05940
  (soft/hard × walk/jog/sprint exit, or damage / damage-roll above 3 m).
- Both move Altaïr by their root motion until they end.
- Clip durations and root curves are in `src/player/jump_clips.rs`. It is generated from your install by
  `cargo test probe_dump_jump_clips -- --ignored` and holds derived numbers only.
- Jumps at a ledge use the game's straight jump (0xB21DA0). Its bands by hand height are knee, waist / wall hang, and free
  hang. Each band has its own reception. Running jumps onto ledges use the wall reception or the swing (RE/04 §4.1.11).

**Debugging and verification**
- `AC_SHOTS=t1,t2,…` freezes the simulation at each time and saves screenshots from every view in `AC_VIEWS` into
  `AC_SHOT_DIR`:
  - the views are `back,left,right,high,front,hands,fingers,fingers_side`;
  - time steps are a fixed 1/60 s, so runs are reproducible;
  - a fill light is added for these runs.
- New scenarios: `AC_AUTOPILOT=wallhang` (jump up, hang, pull up) and `AC_AUTOPILOT=drop` (walk off 6 m).
- `AC_STICK_OFF=a-b` releases the stick in a time window.
- `AC_NO_IK=1` turns the IK off for comparison.
- `AC_IK_LOG=n` logs the error and the fit every n frames. The wrist/ankle error is 0.000 m; the fit is 0.5–9 cm in steady states.
- **Analysis probes:** run `cargo test probe_ -- --ignored --nocapture` with `PROBE_CLIPS` / `PROBE_PAT`. They list
  clips, dump limb and finger positions and root motion, and check track coverage.

**Not yet:** animation contact tags, the game's hip push-off and lean adjustments, wall-run (Walling) for running up
walls.

**Tests:** 23 pass.

### Stage 4d: the game's animation graph and contact tags (RE/13)
- **Graph:** `src/assets/ac_actions.rs` decodes every `ActionBlock` from your install (4,454 actions). The loader brings in
  every clip the movement blocks use (1,238 clips) with their contact tracks.
- **Action playback (`src/anim.rs`):** an action is a sequence of items. Each item blends clips by weight, and the blend
  times come from the data.
- **Actions the exe asks for:** these contexts now request actions by the exe's own IDs, not by clip names:

  | Context | Actions |
  |---|---|
  | Climb | pose waits and moves (SHORT/LONG table actions, including the lateral climb+lean blends) |
  | Ledge | hang idles; shimmies (the free-hang 000/050 cm blend); first/second-hand vertical steps; the pull-up chain with its slope-blend items and the stand-up outcome |
  | Air catches | by type and fall height (≥ 3 m) |
  | Fall | the falling blend |

- **Fall:** as in `HumanInAir::CheckAirCatch`, holding grab while falling blends in the grasp clips toward the stick
  direction (smoothed at 7/s). Without grab the fall pose is still. The invented sway is gone.
- **Limb IK:** follows the clips' **contact tags** (hands, toes):
  - in contact = pinned;
  - released with a re-contact later in the clip = travel over exactly that window;
  - released with none = let go.
- **Debugging:** `AC_ANIM_LOG=1` logs every action change with its clips and weights. `AC_AUTOPILOT=dropgrab` falls with
  grab held.
- **Tests:** 25 pass, including graph decoding from the install, the fall-grasp weights and the contact windows.
- **Still by clip name:** ground locomotion (the game's 17-clip blend tree is decoded but its weights aren't traced),
  landings, jumps, fall entries, the ledge jump-up.

## Placeholders (to be replaced by decoded game data)
All marked `PLACEHOLDER` in `src/tuning.rs`:
- **Root-motion speeds per band.** The game moves the character by animation; those clips aren't
  decoded yet.
- **Jump clip timing and arc.** In the game these come from takeoff/flight clips.
- **Speed deceleration curve.** Its keys aren't decoded.
- **Capsule size.** It will come from Altaïr's Skeleton.
- **Player model.** A capsule for now; Altaïr's Mesh/Skeleton come in stage 3.

## Next stages
2. Ledge (hang/shimmy/pull-up) and Climb (hold grid + dumped move tables), then Walling and Beams.
3. Altaïr's model: decode `Mesh`/`Skeleton` from `.forge` and load them at runtime from the install.
4. Animations: decode `Animation` and drive movement by real root motion.
