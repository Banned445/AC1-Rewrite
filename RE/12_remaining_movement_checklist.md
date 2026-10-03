# 12 — Remaining movement work: checklist for a 1:1 recreation

**Goal:** copy Assassin's Creed (2008) movement exactly. Nothing invented. Every item here comes from the game: the exe's
code and enums (RE/01–07, RE/11) or its data (clips, guidance, collision in the `.forge` files).

**How to read this list:**
- Each item names its game source.
- `[~]` means partly ported or currently approximated.
- `[ ]` means not started.
- Section 1 lists the places where the port currently departs from the game. These must be removed or replaced.

---

## 0. Already done (for reference)
- Context framework: immediate switches, skip the first update, one data bundle.
- Input basics, the Ground speed bands, the jump-to-target warp, the free-fall tail and landing classes.
- Climb: grid plus the dumped SHORT/LONG tables, with the 54 move clips.
- Ledge: hang, shimmy (open/close), hand-over-hand steps, pull-up (the hangknee chain), catches.
- Altaïr's mesh, skeleton, textures, the animation decoder, limb IK (first version).

---

## 1. Places the port departs from the game (must be removed or replaced)
- [x] **Ground translation:** currently a speed per band. The game moves by **blended clip root motion** (walk/jog/run/sprint
      cycles weighted by the speed parameter, 0xDA0810). **→ done: the 17-weight blend of action 0x05923BDB (lean, bank, jog
      slowdown, sprint impulsion) drives the root motion and the animator (RE/02 §4.1, `player/move_blend.rs`). Clip time sync
      inside the item is a hypothesis (phase-synchronised); confirm it with a runtime trace (§10)**
- [x] **Deceleration:** currently a constant fall rate. The game uses a **deceleration curve** whose keys aren't decoded yet (0xDA0810).
      **→ done: ResponseCurve at HG+0x63C, keys (0,1)(0.333,1)(0.4,0.3)(0.666,0.2)(1,1) (RE/02 §4.1.1)**
- [x] **Jumps:** currently a placeholder arc formula. **→ done: running jumps (0xB1EC40 / 0xB20200, RE/04 §4.1) and jumps at a
      ledge (standing straight jump 0xB21DA0 with its height bands and receptions, running jumps onto ledges with the wall /
      swing receptions, RE/04 §4.1.11). No placeholder arc remains. Open: the straight-jump impulse 0x1099C96, min/max of the
      wall reception, the swing's SwingStrength** The game plays **takeoff / flight / landing clips** chosen by
      `Human::SetupJumpToTarget` 0xB20200 and `JumpType` (Straight / 1m / 3m5m), driven by their root motion plus the linear correction.
- [~] **Free jump (vt28):** the nominal distance is a placeholder. Decode the real clip and distance. **→ vt28 is not a free jump: it
      resolves a target type and calls vt24 (0xD832F0). The port's no-target jump (FREE_JUMP_DISTANCE) is PORT; find what the
      game does with no target in range (decision layer)**
- [ ] **Landing point:** currently `LAND_INSET` 0.45 m inside the roof. The game lands on the guidance contact.
- [x] **Landing recovery:** times are placeholders. Take them from the landing clips' lengths and exit flags. **→ done: the
      landing / reception action plays with its root motion until it ends (0xE05940 / 0xE07D00, RE/04 §4.1.7–8)**
- [x] **Landing momentum:** "soft landings and rolls keep speed" is a hypothesis. Verify it or replace it with the landing clips' root motion.
      **→ done: landings move by their clips; the exit clip (walk / jog / sprint impulsion / wait) is chosen by the speed bucket of
      HumanInAir+0x16C (its writer is not traced: hypothesis = ground speed ratio); HG+0x5E8 is not reset by OnEnterInit**
- [x] **Fall pose sway:** remove it. The game's fall loop `xx_h_jumpfalling01` is a static pose. **→ done: removed; the fall is the game's grasp blend (RE/13 §4.1)**
- [~] **Fall-entry clip choice:** currently chosen by my speed rule. **→ fall type 0–6 decoded (RE/04 §4.1.9), port uses the 2.5 m/s
      threshold; the type → entry action mapping (probe vt112) is still open.** The game chooses by fall type (fall height 1 / 2 / 8 m and **→ fall action from the graph; the entry clips are still chosen by name**
      horizontal speed 2.5 m/s, 0xD87720 / 0xD8C380).
- [~] **Ground jump up to a ledge:** currently uses jump-to-hang clips. The game goes through **Walling** (see §3.1). **→ the standing
      straight jump at a hand target (0xD85550 → 0xB21DA0) is ported with the game's bands; the running wall run-up is Walling
      (still open), and the port's trigger (high profile + Legs into a wall) is a stand-in**
- [ ] **Pull-up trigger:** currently "hold up while blocked" (hypothesis). The game sends ledge event 0 from the decision layer.
      Find the sender and its input.
- [ ] **Back eject from ledge and climb:** "high profile + Legs + stick away" is a hypothesis. Decode `TryBackEject` 0xDF2F50 and the ledge equivalent.
- [ ] **Hand spacing after a shimmy close:** currently a fixed 0.4 m. The game re-snaps the trailing hand to the guidance chain
      (0.3 m, 0xDD9640).
- [ ] **Climb root:** derived from the clips. Decode `ComputeRootFromMove` 0xDEC6E0 → 0xB1CC20 and use the exact frame.
- [~] **IK body:** **→ IK attach/travel/release now follow the contact tags; the body pull is still a stand-in**
  - The contact fit and reach pull are stand-ins. Reproduce the HumanIK effector pull with `HumanClimbData` pull weights
    (Left/RightToePull, Left/RightHandPull = 0.5, `UseIK`) and the hips effector.
  - Then return the grip offset to the game value (wrist 0.10 m below, not 0.08).
- [ ] **Ground-to-wall grab order:** currently climb, then ledge, then jump-up (hypothesis). Use the interpreter's real request order (RE/01 §6.3).
- [x] **Capsule size:** **→ 0.4 × 1.8 m from 0xDA7D20 / 0x52E9C0, step offset 0.37, snap 0.58 (RE/01 §7.1)**
- [ ] **Camera:** currently a stand-in orbit camera. Reverse the game's NavigationCamera (§7).
- [~] **Clip selection:** currently by clip **name**. The game selects through **animation-state IDs in the animation graph**, **→ graph decoded (RE/13); climb, ledge, catches and fall use the exe's action ids; ground, jumps and landings are still by name**
      which are not resource IDs (RE/03 IDs). Decode the graph's state → clip mapping and blend trees (§8).

## 2. Ground (HumanGround, context 4) — RE/02
- [ ] Full 87-state tree (`HumanGround__InitStateTree` 0xD90D00), including the event table 0xDB1470 (guards and actions).
- [ ] **Start-move clips:** chosen by leading foot from bone positions (0xD98990); walk start 161516230/161522726, run start 161524269/70.
- [x] **Stops:** walk stop (11 → 4, 0xD8B220) and **run stop** (state 18, 0xD7EC90 conditions). **→ RE/02 §4.0, ported**
- [x] **U-turn / pivot** above 90° (state 25, 0xD84B10). **→ done (RE/02 §4.3): pivot table 0x1A2C120, root yaw of FROMANIM clips; a run has no pivot**
- [x] **Start from standing** (0xD98990). **→ done (RE/02 §4.4): start items by profile and foot, speed parameter set to 0.25 / 0.5**
- [x] **Turn in place** at 1° or more (0xD84B80). **→ never enabled in v1.02 (2026-10-03): its guard needs HG+0x290 == 1,
      and the only writer is `HumanGround__ctor` (0 at 0xDB2A96); HumanGround is not reflected, so data cannot set it
      either. Nothing to port.**
- [ ] Clip **exit-flag gating** (0xD80010 bits 0x40/0x80/0x100/0x200/0x400/0x800, 0x20 = locked).
- [ ] "Anim drives rotation" flag (0x10) overriding code heading.
- [ ] Turn attenuation above 45° (verify against the interpreter, 0xEE65A0).
- [ ] **Crowd-avoid vector** (Data+0x60, `UpdateCrowdAvoidVector` 0xD9E5A0): drives the walk-band hip lean (RE/02 §4.1.3).
- [ ] **Start/transition blend layouts 1–7** (HG+0x724): MoveBlend's other path while a start or transition action plays (0xDA08C0).
- [ ] **Crouch / crouch-walk** (`MvtDivision_Crouch`, 0xD84C10) and **WalkVerySlow**. **→ sender found (2026-10-03):
      not a button. `ProcessGroundMovement` 0xEE7048–0xEE7099 reads the player's `SocialStealthHelper` status (via the
      manager `dword_1A28290`, set at +0x28: `sub_B7D7C0(5)` = bit 5). With bit 5 set and the weapon interface's vt16
      false it calls IHumanGround vt64(1) (`sub_DB31D0`, GroundData+0x11C = crouch) and vt72(1); otherwise vt64(0).
      The helper's bits are set through its vtable slot 2 (`sub_B01730` → `sub_B7D820`); who sets bit 5 is not traced.
      Crouch broadcasts ActorStateID 29 (Blending is 52, a different state).**
- [ ] **Sub-states:**
  - FreeRun (2);
  - OrientedMove (3, state 42);
  - Hurt (4);
  - ObstacleCollision (5) with **ObstacleLeanType** Hands/Feet.
- [~] **Free-run steps:** `xx_h_freestep_*` (up / down / front / left / right at 50 / 300 / 550 cm) and
      `freerunfront_entry_*` low-fall entries. **→ (2026-10-03) the 50 / 300 / 550 cm clips are the free-step flight's own
      blend slots (`air_*_to_freestep`, ported with the jumps). Newly ported: the straight jump's < 0.7 m band
      (0xB21FCD: `collide_full_*_to_freestep_{050,070}cm` 0x012B291B, b = (dz − 0.5)/0.2, root +0.5 n, flags 1) and its
      reception 0x012B33F1 (0xE09752), a step-up onto knee-high obstacles (`AC_AUTOPILOT=stepup`). The free-step
      entry exits (0x0D9971F9…FC, 0x69C24BB2/3) and `freerunfront_entry_030cm` 0x0EED6F55 have no code references:
      they are reached only through action transitions in the graph (§8 item exits).**
- [x] **Vault / pass-over** (RE/04 §4.1.13): type 2 jumps, reception, HandPassOver vault, Ground / InAir. Open: pass-over pull-down, kind 4 chained jumps, roll ending, vt1592/1596.
- [ ] **High obstacle** request (vt744/748, minimum height 5.0).
- [ ] **Static jump on place** (vt36, ActorState 25 JumpingOnPlace).
- [~] **Ground loss → InAir:** **→ Movement falls only by `Human__ShouldFallOffSupport` 0xB23CB0 (ported); the
      edge-line ground loss and its fall-type drop are fight-only (grabbed, RE/01 §7.1). Open: InAir sub-state 3
      keeping the ground clip (0xE00EF0) instead of the named `*_to_fall` clips. Its two animation calls are not
      decoded yet: `sub_5045F0(1, 0, 0)` acts on the actions of animation slot 1 (calls `sub_726F40(0, 0)` on each),
      and the timer is `sub_724D50(sub_5014C0(0))`. Also open: the fight grab itself**
- [ ] **Step off edges:** `xx_fall_step_off_{front,back,left,right}_max`, and walk/run low-fall clips.
- [~] **Ledge stop / look-down:** **→ decoded (RE/03 §7.8a); front ledge stop + EdgeStop pull-down ported. The side ledge
      stop (0xD8E5B0) is a one-frame NarrowObject Edge stay that returns to Ground (RE/05 §2.9), the port's behaviour.
      Event 69's sender is the interpreter (RE/02 §4.5, ported). Open: event 119's (look-down) sender / guard**
- [~] **Leap of Faith** from a look-down edge. **→ faith jump + HayStack context ported (RE/04 §4.1.12); open: the ability /
      look-down trigger path (vt1540/1544), fall entry 0xE05490**
- [~] **PullDown, ground → hang** (§4): **→ type Wait / front ported (RE/03 §7.8b)**
  - from a stop at the edge, from wait, hard, from a beam, and hand-pass-over;
  - all four sides.
- [ ] **Crowd:** gentle push and shove (vt848–860, PushStrength), **CrowdRun**, blending with monks
      (BlendingSourceType), stumble / unbalance (ActorState 32) and get-up.
- [ ] **Steep-slope slide;** a slide over 10 m becomes RagFall (0xE05200).
- [ ] **AssassinAbilitySet gating:** ability bits for Jump, Crouch, PassOver, Walling, Climb, Ladder, Grasp, LeapOfFaith,
      LookDown, and MaxSpeed. The ability stack is at 0xEF0320.

## 3. Missing locomotion contexts
### 3.1 Walling / wall-run (context 11) — RE/05 §1
**→ ported 2026-10-02 (RE/05 §1.8): entry test, sub-states, probes A–D, rebound (PORT trigger). Open: pass-over exits,
post-run state 4, the command source, the rebound landing query.**
- [~] **Entries:**
  - from Ground (0xDA2C30) and from NarrowObject / the decision layer (0xB263B0);
  - takeoff `impultionstraight_*_to_wallingfront_*`.
- [ ] **Sub-states:** EntryA → EntryB → Vertical / Horizontal, ReboundTransition, VerticalEnd, WallStep. Both sides (Left/Right).
- [ ] **Ledge probes A–D** (height bands scaled by character height): pull-up, hang and wall hang with 4 hand/foot reports.
- [ ] **Post-run ledge grab** (state 4, `TryGrabLedgeAfterWallRun` 0xE39550).
- [ ] **Rebound jump** (0xE365C0): within ±89° of the wall normal; fallback 7 m out, 3 m down.
- [ ] **Walling commands 0–4** from the pad controller (+0x38; meaning still open).
### 3.2 NarrowObject (context 12) — RE/05 §2
**→ beams ported 2026-10-02 (RE/05 §2.7): straight mount from Ground, walk / jog / stop / turn on the beam line, step-off. Free-step
arrival decoded as a transient (back to Ground on wide support).**
**→ 2026-10-02 (RE/05 §2.8): free-step arrivals on beams (modes 2–5) and pilotis, the air catch of both (BeamReception / PilotisReception),
the pilotis wait and lean, the beam impulsion, the jump on the spot (clear or to a hand target with the `beam_jumpstraight_*` flights),
free-step jumps (kind 1) from beams and pilotis.**
- [ ] **Free-step arrival:** roof-edge jumps (type 1) end in NarrowObject after the free-step reception (0xE07D00, RE/04 §4.1.7).
      The port stays in Ground.
- [ ] **Standing on the ledge edge after a pull-up.** Currently the pull-up goes straight to Ground.
- [x] **Edge:** decoded as a one-frame redirect back to Ground in this build (RE/05 §2.9); the side look-down is ported (RE/02 §4.2).
- [x] **Lean:** NarrowObject's Lean is unreachable (event 71 rejected, RE/05 §2.9). The game's lean is Ground ObstacleCollision (event 42, RE/02 §4.2): the state is ported (hand lean 70/150 cm, foot bump 50/70 cm, exits), but event 42 is unreachable from player input (RE/02 §4.5), so the port no longer enters it when running into a wall (2026-10-03).
- [ ] **Beam:**
  - ~~entry modes and BeamReception from the air~~ (done, §2.8); 90° waits / turns, corner hops / walks, edge stop, crouch attacks;
  - segment detection (±2 × ±0.5 m box, 60° cone);
  - projection onto the beam line;
  - turn-around;
  - stop 0.3 m before the end;
  - step off within 0.16 m;
  - Unbalanced.
- [x] **Pilotis** (wooden posts): entries FromFreeStep / FromInAir, wait + lean, impulsion and jump-on-place, jumps to targets (§2.8).
      Open: FromJumpImpulsionStart, event 9, pull-down from a pilotis (`beam_pilotis_to_pulldown_*`).
- [ ] **FreeRun** on narrow geometry (EntryB / EntrySide).
- [ ] **CrowdRun** and **ObstacleCollision**.
- [ ] **Support check and fall** (`CheckSupportAndFall` 0xE51190, jump types 0/1/3/4/6).
### 3.3 Pole (context 6) — RE/05 §3
- [x] **Poles are cut content** in v1.02: no HumanPole animations ship (RE/05 §3.5). The playable "poles" are swing bars: ported (RE/03 §7.10a).
- [ ] ~~**Entries:** from Ground (`CanGrabPole` 0xD9C360), FromAirStraight, FromAirInclined.~~
- [ ] **Inclination:** Light / Med / High (cos 20°, cos 60°).
- [ ] **Climbing:** attach 0.2 m off the pole; climb up/down at low/high grip; turn left/right; reached top/bottom.
- [ ] **Jumps:** jump off (PoleJumpDirection); horizontal poles and **swing** (ActorState 44, SwingReception in Ledge).
### 3.4 Ladder (context 5) — RE/05 §4
- [ ] **All 20 MvtAnimStates:**
  - wait and climb at low/high grip, up/down;
  - revolve;
  - enter from ground or top; exit to ground or top;
  - release;
  - jump.
- [x] **Ladder** (RE/05 §4.1): the animation table, ground / top entries, climb up / down by profile, exits to the top / ground, release, jump.
- [ ] **Entries:** FromAirStraight / FromAirInclined / FromWalling / FromClimb, and Ledge → Ladder (ToLadder 0xDD2430); the turn and revolve.
- [ ] **TopOfLadder monitor** (ActorState 60).
- [ ] Rung and exit constants (0xE1C000–0xE28500, not yet mapped).
### 3.5 Rope (context 7) — RE/05 §5
- [ ] **Rope climbing:** horizontal and vertical ropes; limb grips from animation tags (`UpdateLimbGripsFromAnimTags` 0xE32780).
### 3.6 HayStack (context 21)
- [ ] **Entries:** Top / FreeStep / Ground / SideJump; diving in from the air (0xE05490); exit.
### 3.7 Kiosk (19), benches and hiding spots
- [ ] **Entries:** FreeStep / HangKnee (HumanKioskEntryType), and the entry sides.
### 3.8 RagdollGround (15) and Dead (20)
- [ ] **Fatal landings:** fatal landing type, rag-fall at 9 m or more of fall height, death below the world death height (0xE03FA0).
- [ ] **Drowning:** `xx_fall_tr_drowning*`.
### 3.9 Riding (13), horse
- [ ] **Horse:** mount/dismount from the air (1.5–2.0 m above, 0xE03B40), ClimbUp / Riding / ClimbDown. This is a separate horse system.
### 3.10 Always-on layers
- [ ] **LookAt (18):** head tracking.
- [ ] **UpperBody (22):** upper-body overlay.
- [ ] **Vocalization (16):** fall and climb vocals (VocalFamily).

## 4. Ledge (context 9): the missing parts of RE/03 §7
- [x] **Corners:** inner and outer (`sub_DDD490` / `sub_DD55F0`, anim family 0x491228xx), then **SecondHandGrab** (state 17, ±0.25 m).
      **→ done: the corner turn is 0xDD3BB0 with candidates from 0xDD0600 (RE/03 §7.6b); the port turns inner and outer corners
      with the game's actions. SecondHandGrab's exact re-grab is still a hypothesis.**
- [~] **Side jumps between ledges** (`TrySideJumpToLedge` 0xDD3BB0): near/far, wall/free, anims 510126209–216, clearance checks.
      **→ the real side jump is 0xDDD490 → the StartLedgeJump table (RE/03 §7.6b); hang → hang (types 1/2) ported. To climb holds
      (type 0) and to a ladder (0xDD55F0, type 3) are still open.**
- [ ] **Jumps up and sideways:**
  - **ParallelJump / LedgeJump** (`StartLedgeJump` 0xDDCE40): the blended variant (anim 1289129807, 4-way) and the table variant (0x1A2C780 / 0x1A2C980).
    **→ done: table decoded and ported; the blended variant is the hop up (→ free hang, `_a` then `_b`).**
  - [x] **TryWallJumpUp** 0xDD62A0. **→ ported as the hop (its box query geometry is a hypothesis).**
- [ ] **Exact jump-up rules** (`TryJumpUpToLedge` 0xDD5E10): edge probe and body sweep. **→ it is TryJumpUpToClimb (to climb holds,
      RE/03 §7.6b): decoded, not ported (needs a climbable wall above a ledge in the greybox).**
- [x] **Vertical hand-step tables:** up 0x1A2C4C0…, down 0x1A2C4F0…, with the Wall/Free anim columns. Currently one clip each way. **→ done: the game's first/second-hand step actions (RE/13 §4)**
- [x] **Hang-type switching:** **→ done (RE/03 §7.6c): foot rays, free ↔ wall switch moves with the game's actions; the corner-candidate
  variants and WallFree as a separate type are not ported**
  - Free → Wall (anims 29566306/7) and Wall → Free (29562045/6, 29565306/8) (`TrySwitchHangType` 0xDE1060);
  - **WallFree** hang type (2).
- [ ] **Pull-up:**
  - slope blend (±30° / 45°);
  - variants (957955002, 0x386E3B13);
  - free-hang special case (`sub_DE0E30`);
  - the three outcomes: stand on the edge → NarrowObject, jump off → InAir, back to hang.
- [~] **Grasp** (low ledges at knee/waist height → pull-up straight away) **→ via the straight jump's knee / waist bands (RE/04
      §4.1.11); the GraspType states themselves are not traced** and **GraspType** (HangKnee, Climb, HangWaist, HangWall,
      HangFree 1 hand, HangFree 2 hands).
- [~] **Receptions:** Wall / Free / **Swing** reception (SwingStrength from InAir); **HangFreeReceptionType** Front/Straight/Back; **→ catch actions by type and ≥3 m (CheckAirCatch); swing/one-hand/angle classes still open**
      **min/max** catch variants (long catch at a fall of 3 m or more, 0xE0BB70); angle variants **30_out / 45_in / straight**;
      `air_surface_tr_hangwall_reception_*`.
- [ ] **One-hand catch** (`hangfree_onehand`, `hangwall_onehand`) → SecondHandGrab.
- [ ] **Impacts:** `hangfree_impact_*` (catching at elbow/shoulder height, 50 cm, 150 cm).
- [x] **HandPassOver:** vault over the ledge without hanging (state 9 → Ground), RE/04 §4.1.13.
- [~] **PullDown, ground → hang:** all PullDownTypes × PullDownSides, Orientation → Descent → Reception → ReleaseToInAir.
      **→ decoded (RE/03 §7.8b); type Wait / front ported with the game's actions and roots. Open: EdgeStop (needs the ledge stop),
      side and beam variants, HandPassOver, ReleaseToInAir, the decision layer's event 70 input**
- [ ] **Ledge → Climb:** down/side onto climb holds; the free-hang drop-to-climb sequence (states 14/15, `TryFreeHangDropToClimb`).
- [ ] **Ledge → Ladder** (ToLadder).
- [ ] **Exits:** knock-off / damage reactions (events 2/4/5) → InAir.
- [ ] **Idle variants:** look-around idles (`hangwall_wait_lookaround_*`); `waitopen` / `waitclose`.

## 5. Climb (context 10): the missing parts of RE/03 §3–4
- [ ] **Exits in priority order:** ladder, ledge grab / reach (`TryReachLedgeAbove` 0xDF1730), **side jump**, other surface, back eject.
- [ ] **Entry clips:**
  - from ground (`climb_1m_to_groundentry_*`);
  - from ledge (`hangwall_climb_*`, `hangfree_climb_*`);
  - climb ↔ hang transitions (`xx_l_climb_1m_{u,d,l,r,…}_hang{wall,free}`).
- [ ] **Climb → standing** (`climb_*_tr_h_wait_*`, `tr_hangknee_*`).
- [x] **Long hand-over-hand clips** (`xx_h_climb_*_{l,r}hand_2/3`, `xx_h_climbing_climb1m_tr_climb1m_*`). Check which the LONG table uses. **→ done: SHORT/LONG table actions resolved (RE/13 §4)**
- [ ] **Overhang moves** (`climb_1m_u_overhangfree_*`).
- [ ] **Blocked-move reaction clips** (`OnNoMoveFound` 0xDF4410).
- [ ] **Look-around idles** (`climb_1m_lookaround_*`).
- [ ] **Lost hold** → InAir with FallOrigin_Climb (`sub_E55A10`); release clip.
- [ ] **Damage / teleport interfaces** (IHumanDamage, IHumanTeleport).
- [ ] **Climb data fields:** the IK group (+0x50..0x5C, purpose unknown), and the debug timestamp +0x1280.

## 6. InAir (context 8): the missing parts of RE/04
- [ ] **Full jump-target selection** (0xE96BF0 / 0xB1EC40):
  - every target type: ground, ledge, beam, pole, ladder, horse, haystack;
  - behind-the-character rules;
  - "beam beats ledge unless 2.5 m higher";
  - distance bands per type.
- [ ] **Sub-states:**
  - 2: short anim / rebound;
  - 3: Reception, an edge landing with edges within 0.15 m;
  - 4: Drop from hang or climb, with catching when `Data+0x3F0 == 2`;
  - 5: RagFall.
- [ ] **Air catch of ladders, beams/narrow objects and poles**, not only ledges.
- [x] **Leap of Faith:** 3 m over-drop, `faith_jump_*` clips, haystack landing, ActorState 41. **→ RE/04 §4.1.12 (ActorState 41 not sent by the port)**
- [ ] **Small-damage landing** and the fixed height table (flag `this+226 & 8`: 8–20 m); camera shake on rolls.
- [ ] **ActorState events:** Jumping, JumpingOnPlace, LongFall, FreeFalling, Landing, Roll.

## 7. World data, collision and camera
- [ ] **Real guidance from `.forge`:**
  - GuidanceSystem blobs: quantised vertices, 12-byte edges, subtypes LedgeGrab / Beam / Ladder / Pole / Rope / Surface / Kiosk;
  - the spatial tree, query shapes and the 40° post-filter;
  - **GuidanceChain** and `FindGrabPoint` 0x66E150;
  - edges generated at runtime for capsules and barrels.
- [ ] **Real collision geometry** from `.forge` (Havok), so a real city map can be loaded and walked.
- [~] **CharacterController** **→ shape and ground behaviour decoded (RE/01 §7.1) and ported:**
      - capsule r 0.4 (0.35 + keep 0.05), h 1.8;
      - lifted 0.37 m on the ground (step offset);
      - stick-to-ground 0.58 m;
      - 45° max slope;
      - the fall-off-support rule;
      - the ground loss at the edge line (0xD87720 / 0xB248B0).

      Still open:
      - the Havok cast + simplex solver itself (the port depenetrates boxes);
      - the 1.0 m capsule of Movement sub-states 230/233;
      - the unknown controller fields +80 / +88 / +92 / +84.

      The port integrates the controller's velocity on a context's skipped first frame (`player::coast`), as the
      game's proxy keeps running while the context skips. Before that the body froze for one frame at every takeoff
      and landing.
- [ ] **NavigationCamera / free-roaming camera:** follow, orbit and collision (FreeRoamingCameraSettings); the activators
      (speed, high profile, buttons); assassin-fall camera shake.
- [ ] **Input:** the real `DefaultBindings.map` keyboard and gamepad mapping.

## 8. Animation system (needed to select and blend clips exactly as the game does)
- [x] **Animation graph:** decode the state-ID → clip / blend-tree mapping (state IDs from RE/03 tables, the Ground clip **→ done: format decoded, all 51 blocks, exe ids resolved (RE/13)**
      tables, the ledge tables 0x1A2C3F0–0x1A2CB80, the climb pose table 0x1A2CD70).
- [ ] **Blend and transition rules:** ACTBlendType (roll/stop curves), ACTEndEvent, ACTTargetFlag (auto transit-out etc.),
      blend times per transition.
      **Port (2026-10-03):**
      - crossfades start from the pose on screen, frozen, so a transition that starts mid-fade no longer snaps;
      - the root-motion offset is faded too;
      - the game keeps the outgoing item playing (not traced). This is the largest remaining source of differences
        when actions switch quickly.
- [ ] **Item exits** (`ACTTargetFlag` / the end-of-item transitions): which item follows a landing, stop or lean is
      chosen by the graph in the game (e.g. a landing with the stick released goes to the wait). The port chooses in
      code, so a stale choice can play an unfitting clip.
- [~] **Fixed tracks:** **→ ACUATORCONTACTS decoded and driving the IK (RE/13 §5); STEPPHASES/PIVOT/CENTEROFMASS still open**
  - ACUATORCONTACTS (id 2): limb contact tags. These drive IK attach/detach and the hold-to-hold travel windows (RE/11 §2).
  - STEPPHASES (id 3): foot phase sync between gaits and stops.
  - PIVOT (id 1).
  - CENTEROFMASS (id 4).
- [ ] **Event tracks:** sound, FX and AI events (footsteps, ContactEventTypeHuman: jump start, land light/medium/heavy,
      roll, slide …).
- [ ] **Ground foot IK** (GroundIKState: delay, fade in, running, fade out) and the IK pelvis/lean post-adjustments (RE/11 §2.4).
      **Stand-in (2026-10-03):** a foot the clip puts below the surface under it is lifted onto it (`ik::ground_foot_target`).
      This fixed landings that put a leg through the roof. Without the real system, feet still don't adapt to slopes or steps.
- [ ] **Roll bones** (RollBoneModifier: upper arm / forearm twist) and HumanSkeletonStyler hips.

## 9. Character presentation that moves with the body
- [ ] **Cloth:** the robe skirt cloth simulation (ClothCollisionMode).
- [ ] **Weapons and sheath** (stride-24 meshes); WeaponState sheathed/unsheathed.
- [ ] **Face:** eyes and mouth meshes.

## 10. Verification against the real game
- [ ] **Runtime traces** from the real game (offline save, debugger or Cheat Engine): position, context ID (Human slot 1 +0x48) and
      sub-state per frame. Compare frame by frame with the port: speeds, the 9.8 m/s² gravity, jump snapping, ledge/climb timings.
- [ ] **Live memory checks** (tagged `LIVE:` in the port). Each needs a value or a trace read from the running game
      (offline save, Cheat Engine / debugger), because static reading did not settle it:
  - **Jump-link reach:** `WorldArea::JumpLinkRange` Normal / Extended (strings at 0x168B2E8 / 0x168B300, no code
    xrefs in the unanalysed IDB). Jumps only follow the world's precomputed jump links (`MetaLinkTypeID_JumpLink`),
    so the reach is level data. The port caps ledge jumps at 5.5 m, or 2.5 m when the root rises more than 1 m
    (`tuning::LEDGE_JUMP_FAR*`, hypothesis from the floats at 0x1A2EA50). Read both ranges from a loaded WorldArea.
  - **Speed parameter after a pull-up:** HG+0x5E8 right after a pull-up / beam / ladder exit into Ground. The port
    zeroes it (`HumanGroundData::enter`), because the stale value slid the character on in a run stop.
  - **Run stop at an edge:** does the run stop's root motion carry the character off a roof, or does a guard stop it?
    The port stops the slide at the last supported point.
  - **Target jump over a lip:** the root path when a target jump's capsule catches the lip of a higher roof (the
    port lifts it up to 0.5 m; 0xE0B1E0 is only the stuck-in-air nudge).
  - **Sprint off an edge:** horizontal speed through a fall entered above the 5 m/s drift cap (0x19BA438). The port
    decays it to the cap at 4 m/s² instead of clamping.
- [ ] **Static RE still open from the 2026-10-03 bug round:**
  - `RotateTowards` 0xD94F30: its rule for a ±180° turn (the port keeps the side a turn started on);
  - the ground foot IK (`GroundIKState`, RE/11 §2.4). The port only lifts feet that would go below the surface.
- [ ] **Reference captures** from the real game at the same moments and camera angles as the port's `AC_SHOTS` views.
- [ ] **Field names:** recover the remaining reflected field names (124 of 270 still hashed).
- [ ] **Open questions:**
  - who sends ledge event 0;
  - the walling commands;
  - `entity+0x7C` = height;
  - who sets Unbalanced;
  - event IDs 4/8/9.

---

### Suggested order
1. Animation graph and contact tracks (§8). Most of the departures in §1 disappear once clips are selected and timed the game's way.
2. Real jump clips and ground root motion (§1).
3. Ledge completion (§4).
4. Climb completion (§5).
5. Walling (§3.1).
6. NarrowObject / Beam / Edge (§3.2).
7. Real guidance and collision from `.forge` (§7), so all of this can be tested on a real map.
8. Pole, ladder and rope (§3.3–3.5).
9. HayStack, Leap of Faith, receptions, ragfall (§3.6, §3.8, §6).
10. Camera (§7) and presentation (§9).
11. Runtime traces (§10) to confirm each item before it's ticked off.
