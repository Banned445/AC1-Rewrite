# AC1 movement — reverse engineering reports

Target: `bin/AssassinsCreed_Dx9.exe` (Assassin's Creed, Steam v1.02 build 86610, 32-bit MSVC,
not packed, full RTTI). IDA 9.3 database: `bin/AssassinsCreed_Dx9.exe.i64`
(pristine pre-annotation backup in `bin/backup/`).

Start with **00_overview.md** (architecture and recreation plan), then read the subsystem reports.

| # | Report | Scope |
|---|---|---|
| 00 | [00_overview.md](00_overview.md) | Architecture, how the pieces fit, recreation roadmap |
| 01 | [01_human_core_and_input.md](01_human_core_and_input.md) | Human object, module/state framework, pad input → movement, character controller |
| 02 | [02_ground_locomotion.md](02_ground_locomotion.md) | Walk/jog/run/sprint, turning, stopping, obstacles, crowd push; §4.1 MoveBlend: 17-clip locomotion blend weights, deceleration curve, root motion |
| 03 | [03_ledge_and_climb.md](03_ledge_and_climb.md) | Ledge hanging/shimmy/pull-up, wall climbing; §7.6b corners, side jumps, ledge-jump table, hop up |
| 04 | [04_air_jump_fall.md](04_air_jump_fall.md) | Jumps, falls, landing, Leap of Faith; §4.1 jump action choice (takeoff/flight/reception), height/distance blends, Σw·T item timing, ground landing actions |
| 05 | [05_beams_walling_poles_ladders.md](05_beams_walling_poles_ladders.md) | Beams, wall-run, poles, ladders, ropes |
| 06 | [06_guidance_world_detection.md](06_guidance_world_detection.md) | How the world marks climbable/grabbable geometry |
| 07 | [07_reflection_enums_and_data_layouts.md](07_reflection_enums_and_data_layouts.md) | Reflection formats, all movement enums, data layouts, CRC32 name hashes, engine type table |
| 08 | [08_forge_format.md](08_forge_format.md) | `.forge` archive format (header, index, LZO containers, resources) and the reader |
| 09 | [09_mesh_skeleton_texture_format.md](09_mesh_skeleton_texture_format.md) | Skeleton, Mesh (skinned vertices, inverse binds, submeshes), Material chain, TextureMap (BC1/BC3) |
| 10 | [10_animation_format.md](10_animation_format.md) | Animation payloads: track table, all key compressions (Quat16..96, Vec3 32/48, Float8/16), interpolation, root DISPLACEMENT and measured locomotion speeds; decoder `tools/ac_anim.py` |
| 11 | [11_limb_ik.md](11_limb_ik.md) | Limb IK component (Human+1328): weight fade 4/s in, 5/s out, hold-to-hold travel, HumanIK-style solver hand-off. §5: hang/climb placement, grip, root motion, pull-up/catch/fall/climb-move clips measured from the game clips; multi-angle capture tooling |
| 12 | [12_remaining_movement_checklist.md](12_remaining_movement_checklist.md) | Checklist of every remaining movement feature for a 1:1 copy, including the port departures that must be removed |
| 13 | [13_animation_graph.md](13_animation_graph.md) | Animation graph: ActionKit/ActionBlock/Action/ActionItem format (all 51 blocks, 4,454 actions), exe action ids resolved (climb, ledge, pull-up, catches, fall-grasp blend), contact-track bits; decoder tools/ac_actions.py |

Supporting material:
- `data/enums_movement.txt`, `data/enums_all.json` — every reflected enum with value names
- `data/data_classes.txt` — decoded field layouts of the module data classes
- `data/recovered_property_names.json` — property names recovered from their CRC32 hashes
- `tools/` — Python helpers: exe readers (`pe.py`, `vt.py`, `enums.py`, `classes.py`, `refl.py`, `calls.py`,
  `crack.py`) and the archive reader (`forge.py`, `lzo1x.py`, `forge_inventory.py`)
- `data/forge_inventory.json` — every archive's files and resource-class histogram
- `_agent_brief.md` — shared conventions used by the analysis agents
