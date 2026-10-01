# 13 — The animation graph (ActionKit / ActionBlock / Action) and the contact tracks

**Status:**
- The graph format is fully decoded: all 51 ActionBlocks in DataPC.forge "Game Fix" parse to the last byte, giving 4,454 actions and 5,143 items.
- The exe's hard-coded animation-state IDs resolve through it.
- The contact-track bit layout is decoded and checked on climb, hang, shimmy, catch and locomotion clips.
- Both are used by the port.

## 1. Method
1. **Found the graph.** Listed the animation-related resource classes in DataPC.forge. The `ActionBlock` resources are named after the movement contexts (`HumanGround`, `HumanInAir`, `HumanLedge`, `HumanClimb`, `HumanClimb_Jumps`, `HumanWalling`, `HumanNarrowObject`, `HumanLadder`, `HumanGround_TurnOnSpot`, …), and `Human_ActionKit` lists them.
2. **Linked it to the exe's IDs.** The block resource IDs sit next to the "animation IDs" seen in exe code:
   - `HumanClimb` is object 0x012DA3A7, and the climb pose-0 ID is 0x012DA39F;
   - `HumanLedge` is 0x0106C5EB, and the pull-up ID is 0x0106D2C5.

   So those IDs are object IDs of `Action`s inside the blocks, not Animation resource IDs. This explains the RE/03 note "anim IDs are not resource ids".
3. **Indexed every reflection descriptor** (`RE/tools/refl_index.py` → `RE/data/refl_classes.json`, 2,167 classes keyed by the class hash written in payloads, i.e. descriptor +0x28 = CRC32(name)). This gave the classes `ActionKit`, `ActionBlock`, `Action`, `ActionItem`, `ActionTransition`, `ActionBlend`, `AssociatedActionGroup` and their enum types.
4. **Read the format from the exe, not by guessing.** Serialization is per class (vftable slot 2 = `Serialize`), so each serializer was decompiled along with the stream helpers it calls (§2). Renamed in the IDB.
5. **Wrote and validated the decoder.** `RE/tools/ac_actions.py` must consume every byte of every block or it fails, and all 51 pass. The Rust port is `port/src/assets/ac_actions.rs`, and the install test checks the actions the exe uses.
6. **Resolved the exe's tables through the graph** (`RE/tools/resolve_climb_tables.py`, `resolve_ledge_tables.py`, using the emulated table memory `RE/data/climb_init.pkl`, `ledge_init.pkl`).
7. **Decoded the contact tracks** (fixed track 2) with `RE/tools/anim_contacts.py`, and named their bits from the `AcuatorsContactsTypes` enum.

## 2. Serialized format (verified)
Stream helpers (renamed in the IDB):

| Helper | Address | Bytes |
|---|---|---|
| `Stream__ReadObjectPtr` | 0x931780 | `u8 flag`: 0 = inline `{u32 id, u32 classHash, fields}`; 2 = `u32` reference id; 3 = null |
| `Stream__ReadHandleObject` | 0x9311A0 | same, and 1 also means reference |
| `Stream__ReadTypedReference` | 0x931410 | `u32 id` (inline data only if the property flag 0x4000 is set) |
| `Stream__ReadEmbeddedObject` | 0x438EF0 | `u32 id, u32 classHash, fields` (no flag byte) |
| `Stream__ReadHandle` | 0x433D80 | `u32 id` |
| `Stream__ReadBytes` | 0x930B10 | raw (used after a `u32` count for POD arrays) |

Classes, fields in stream order:
- **`ActionBlock::Serialize` 0x6E9BF0:** `u32 n`, n × handle-object `Action`; typed reference `BodyPartTemplate`; object `AssociatedActionGroup`.
- **`Action::Serialize` 0x507EE0:**
  1. `u32 actionId` (+8, equals the object ID);
  2. handle `BodyPartChannel`;
  3. objects `ActionTransition` ×2;
  4. 5 × `u8` bit flags (+48 bits 0–4);
  5. `u32` `ACTTorsoConstraintMode`;
  6. `u32` (+28), `u32` (+32);
  7. object `AssociatedActionGroup`;
  8. `u32 n`, n × object `ActionItem`.
- **`ActionItem::Serialize` 0x5BADF0:**
  1. `u32 n`, n × typed reference `Animation`;
  2. `u32 m`, m × object `ActionTransition`;
  3. embedded `ActionBlend`;
  4. `u32` `ACTDisplacementMode`;
  5. `u32` and `u32` (2-bit `ACTFeetPosition` ×2);
  6. 12 × `u8` bit flags;
  7. `f32` (+20), `u32` (+24), `u8` (+62);
  8. `u32 k`, k × `f32` **blend weights, one per Animation**.
- **`ActionTransition::Serialize` 0x564DB0:** twice {embedded `ActionBlend`, handle `Action`, `u32`}.
- **`ActionBlend::Serialize` 0x5BCDE0:**
  1. `u32` (3 bits `ACTBlendType`);
  2. 3 × `u8` bits;
  3. 3 × `u32` (2 bits each: `ACTBlendBPosMode`, `ACTBlendDispSrcMode`, `ACTBlendAcuatorMode`);
  4. `u8` bit;
  5. `f32` **blend time**, `f32`, `f32`;
  6. `u8`;
  7. object `ActionBlendFrankenstein` (0x6E8840). It is never present in the shipped data.
- **`AssociatedActionGroup::Serialize` 0x6D8D20:** `u32 n`, n × embedded `AssociatedAction` {handle `Action`, `f32`, `f32`} (0x6D8850); `f32`; 3 × `u8`.

Enums (reflection):

| Enum | Values |
|---|---|
| `ACTDisplacementMode` | FROMANIM / FROMPHYSICS / FROMAI |
| `ACTFeetPosition` | NOTSET / LEFTAHEAD / RIGHTAHEAD / PARALLEL |
| `ACTBlendType` | NONE / AROLLBROLL / ASTOPBROLL / AROLLBSTOP / ASTOPBSTOP / ASTOPBSTOPCUBICQUAT |
| `ACTBlendBPosMode` | FROMSTART / PROGRESSIVE / INVPROGRESSIVE / FROMTARGETTIME |
| `ACTBlendDispSrcMode` | BLENDAB / FROMAONLY / FROMBONLY |
| `ACTBlendAcuatorMode` | FROMA / FROMB |
| `ACTTorsoConstraintMode` | — |
| `ACTPriority` | — |

## 3. What an action is
- An **action** is a sequence of **items**, played in order. The pull-up, for example, is item a then item b.
- An **item** blends one or more clips with weights. Code overrides the weights at runtime: slope, angle, speed, reach direction.
- Each item has a blend time and a displacement mode, and its transitions name a (transition action, destination action) pair.
- The full movement dump is in `RE/data/action_graph_movement.txt` (generated by `ac_actions.py`).

**Displacement modes:**
- Climb moves, shimmies, vertical steps and the first pull-up item are `FROMAI`: code moves the root.
- The root interpolator `sub_711130` → setup 0x710C20 / update 0x7113F0 works like this:
  - progress `u += dt/duration`; rotation is slerped;
  - position is a 3-point spline (start, midpoint, end), unless the setup flag at +0xC8 is set;
  - with that flag, the position is *the playing animation's displacement over 0..u plus a linear correction to the target*.
  - The climb `StartMove` passes 0, so the climb root is the spline. The climb clips' own displacement is linear anyway (verified on `xx_l_climb_1lu_u_1ru`: +0.075 m per 1/15 s).
- `FROMANIM` items (e.g. the pull-up's second item and the stand-up) let the clip's root motion move the root.

## 4. Exe IDs resolved (verified)
- **Climb poses (pose table 0x1A2CD70):** 0x012DA39F…0x012DA3A4 → `xx_climb_wait_{1m,1lu,1ru,2m,2lu,2ru}`.
- **Climb moves:** every SHORT/LONG table entry resolves to an action (`RE/data/climb_move_actions.txt`). Corrections to the name-based port:
  - sideways moves are **2-clip blends**: `xx_l_climb_1m_l_2m` + `xx_l_lean_1m_l_2m`;
  - some LONG moves use clips whose names don't follow the pose pattern: 2ru → 2lu plays `xx_l_climb_2ru_u_2ru`.
- **Ledge tables (0x1A2C3F0–0x1A2CB80): 154 entries** (`RE/data/ledge_table_actions.txt`):
  - **pull-downs:** ground and beam/pilotis, all sides;
  - **shimmy:** wall `0x01B70B35..38` (left open/close, right open/close); free `0x01A2490A..0D`, each a **2-clip blend** `…_000cm` + `…_050cm`;
  - **vertical hand steps:** wall `0x01B70FAA..B1`, free `0x01A279B9..C0`, as `xx_h_hang{wall,free}_climb_1m_u_1lu` (first hand), then `…_1lu_u_1m` (second hand), and the down equivalents.

    The clip the earlier port used for this step (`hangwall_u_climb_1m`) is actually a hang-to-climb transition (0x01C32292…);
  - **ledge-jump sequences:** `HumanClimb_Jumps` `xx_h_climbing_hangwall_tr_climb1m_*_{a,b,c}` (start / loop / end).
- **Hang idles:**
  - wall: `0x0106F2E8` (`xx_h_hangwall_wait`);
  - free: `0x012719F1` (blend of `hangfree_wait` and `hangwallfree_wait`).
- **Pull-up:**
  - wall `0x0106D2C5`: two items, each a **3-way slope blend** straight / `45_in` / `30_out` (this is the ±30°/45° blend from RE/03 §7.7);
  - free `0x012719F2` (hangfree → hangwaist).
  - The last item's transitions are the outcomes: `0x0106C58B` hangknee → stand (then Ground `0x00D8258E` `h_wait`), and `0x082FB340` hangknee → walk/jog (then Ground `0x05923BDB`).
- **Ground locomotion `0x05923BDB` — the real blend tree:**
  - per foot half, 17 clips: slow walk and walk (hip mid/left/right); jog with bank left/right and slowdown; run with bank left/right; sprint; sprint impulsion;
  - the weights come from speed and turning in code (not traced yet).

### 4.1 Air catches and the fall (`HumanInAir__CheckAirCatch` 0xE0BB70, verified)
- **Falling (`0x1F0C22C2` / `0xCD6F5E10`)** is a 6-way blend: `xx_h_jumpfalling01` plus `xx_fall_grasp_{front,left,right,backleft,backright}`.
  - It only happens while grab (Legs) is held. The reach direction is the stick direction, or the facing direction without stick, smoothed at **7/s** (+48).
  - `w = |reach| ≤ 1`. `falling01` gets `1 − w`; the rest goes to the two grasp clips around the signed angle, in 90° sectors (0xE0BF2B–0xE0C2E9).
  - Without grab, `falling01` (a held pose) plays alone.
- **Catch actions:**

  | Catch | Fall < 3 m | Fall ≥ 3 m |
  |---|---|---|
  | Climb wall (hands + feet) | `0x1F0C1C57` | `0x1F0C1C58` |
  | Wall-hang ledge | `0x1F0C0C23` | `0x1F0C0C2D` |
  | Free hang | `0x1F0C2EB8` | `0x1F0C2EB9` straight, `0x201372F8` / `0x201372F9` left / right |

  For the wall-hang ledge catch, the weights straight / 30° out / 45° in come from the wall angle (÷30° and ÷45°) and are snapped to the class above 0.5.
- **Edge landings:** 538143983–986 (`xx_fall_step_off_{front,back,left,right}_max` + `…_tr_fall_max`).

## 5. Contact tracks (fixed track 2, ACUATORCONTACTS)
- **Format:** a byte per key, held until the next key (ByteLowest).
- **Bits:** `AcuatorsContactsTypes` — **0 L heel, 1 R heel, 2 L toes, 3 R toes, 4 L hand, 5 R hand, 6 no look-at**.
- Samples are in `RE/data/contact_tracks_sample.txt`:

| Clip | Contacts |
|---|---|
| `xx_climb_wait_1m` | toes and hands, constant |
| `xx_l_climb_1m_u_1lu` | L hand + L toe off at 0.067 s; L hand on at 0.333; L toe on at 0.533 |
| `xx_h_hangwall_wait` | **hands + toes** (the feet are planted on the wall) |
| `xx_h_hangfree_wait` | hands only |
| shimmy open (right) | R hand off 0.067–0.267, then R toe off 0.267–0.533 |
| pull-up (hangwall → hangknee) | none at all (hands released) |
| catch b | hands from 0; R toe at 0.4, L toe at 0.6 |
| walk / run | heel and toe contacts per foot |

This is the input to `LimbIK__UpdateContactsFromAnimTags` 0xE56FC0 (RE/11 §2):
- **Tag on:** the limb is in contact.
- **Tag off, re-contact later in the clip:** the limb travels over [off, on], following `lerp(old + animDelta, new, s)`.
- **Tag off, no re-contact:** the limb is released.

## 6. Port
- **Graph loading:** `assets/ac_actions.rs` decodes all ActionBlocks. `assets/anims.rs` loads every clip the movement blocks reference (1,238 clips), plus their contact tracks.
- **Action playback (`anim.rs`):** items play in sequence, clips are blended by weight, and the blend times come from the data. The visual root follows clip displacement for FROMANIM items.
- **IDs:** the contexts select actions by the exe's IDs. Climb poses and moves come from the SHORT/LONG tables (`player/climb.rs` `SHORT_ACTIONS` / `LONG_ACTIONS`, generated from `climb_init.pkl`), plus the hang idles, shimmies, vertical steps (first/second hand), pull-up chain, catches and fall.
- **Fall:** the grasp blend is ported exactly (`fall_grasp_weights`, tested). The earlier port's invented sway is removed.
- **Limb IK:** driven by contact tags (`ik.rs` `limb_tag` / `LimbState::update_tagged`). The context's timing is kept only for clips without tags.
- **Still name-based** (exe ID not traced yet): ground locomotion and landings, jump clips, fall entries, the jump-into-hang flight, its reception, and the ledge jump-up.

## 7. Open questions
- Who sets the item weights in the other blend trees (the ground locomotion bank/speed tree, the free-hang shimmy 000/050 cm blend, the lateral climb lean blend)? Right now the authored defaults are used.
- The meaning of the Action flags, item flags, `f20`, `b62` and `u28`/`u32`.
- How transitions are selected at runtime: the transition objects are decoded, but the code that picks one isn't traced.
- The STEPPHASES track (fixed track 3) values.
