# 06 — Guidance system: how the world tells Altaïr what is grabbable

Scope: `GuidanceSystem*`, `GuidanceZone*`, `GuidanceObject`, `GuidanceContactObject`/`GuidanceReport`,
`GuidanceChain`, the detectors, and how Human modules query them. IDA session `ac1`, exe v1.02.

## 1. Summary

- **Guidance data is baked offline and stored in the .forge level data. It is not derived from collision at runtime.**
  Each static collision object (`InertComponent`, a `PhysicComponent`) owns a `GuidanceSystem*` (property at
  InertComponent+0xB0, deserialiser `sub_651640` → class hash 0x55AF1C3E). A GuidanceSystem holds:
  - a vertex pool of quantised u16 xyz,
  - a list of **GuidanceObjects**, each one an *edge* (two vertex indices) plus the *two packed normals of the faces
    that meet at that edge* plus a 5-bit **subtype** (LedgeGrab/Beam/Ladder/Pole/Rope/Surface/Quadruped/Kiosk),
  - an `EdgeFilter` that decides which edge types count as valid,
  - a baked spatial `Partitioner` tree used to speed up queries.
- The only geometry generated at runtime belongs to two procedural subclasses, `GuidanceSystemCapsule` and
  `GuidanceSystemBarrel`. They rebuild the top-ridge edges of lying capsules and barrels/logs every query, because these
  rigid bodies roll.
- The classes `GuidanceSystemOptimizer(GroundWall)`, `GuidanceSystemOperations`, `GuidanceVerticesOperations` and
  `GuidanceObjectAdded/Modified/Removed` are the leftovers of the **authoring/build pipeline**. Their real algorithm is
  not in the game exe. Only the constructors, serializers and defaults survive (see §5). The optimizer is created only
  by an orphan function, `sub_547390`, which has no xrefs.
- **Runtime query model**:
  1. Build a `GuidanceZone` shape (Box / Sphere / Plane / Capsule / Mesh) in a local frame placed at the character.
  2. Call `GuidanceZone::Query`. It walks the world broadphase to find entities in the zone AABB. Each entity's guidance
     component tests its edges against the zone, filtered by a subtype bitmask. The edges can optionally be clipped to the zone.
  3. Each hit becomes a 96-byte **GuidanceContactObject** in world space (p0, p1, n0, n1, entity, source object, subtype),
     stored in a `GuidanceReport` array.
  4. Human code post-filters the report: by subtype, by edge slope (≤ 40° from horizontal), by facing direction, and by
     zone include/exclude cuts. It then links the contacts into **GuidanceChains** and asks a chain for a hand position
     (`sub_66E150`). Optional detectors are `GuidanceDepthDetector` (plane casts to measure ledge depth) and
     `GuidanceBeamDetectorAccurate` (finds beams).

## 2. Data layouts

### 2.1 GuidanceObject (12 bytes, reflection desc 0x18E09F0, hash 0x344FA659)
Accessors are in 0x5AB9E0–0x5ADA00. The binary reader is `sub_5ACC10`.

| bits / offset | meaning | source |
|---|---|---|
| dw0 bit 0 | enabled flag. Queries skip the object when it is clear (`sub_5452F0`). Barrel/Capsule toggle it. | 0x5452FE, 0x6695D0 |
| dw0 bits 1–5 | **GuidanceObjectSubType** (5 bits). Copied into the contact's 6-bit type and tested against the query mask `1<<type`. | 0x5AB1A7, 0x5AB0D0 |
| dw0 bits 6–18 | vertex index A (13 bits) (`sub_5ABA10`) | 0x5ABA10 |
| dw0 bits 19–31 | vertex index B (13 bits) (`sub_5ABA20`) | 0x5AC190 |
| +4 | normal n0, packed 10:10:10:2 (signed 10-bit /511, sign tables word_1690430/1690438), decode `sub_5ABC40` | 0x5AC9F0 |
| +8 | normal n1, same packing | 0x5ACA10 |

Serialized order (`sub_5ACC10`): `u8 enabled; u32 subtype; u32 vertA; u32 vertB; vec4 n0 (16 B, packed by sub_5ABF60); vec4 n1`.

The meaning of n0 and n1 follows from the validity test (§4.2). They are the normals of the two faces adjacent to the
edge. A typical ledge has one ~up face (top) and one ~horizontal face (wall).

### 2.2 Vertex decode
`pos_local = u16xyz * 0.005 − 163.84` (0x3BA3D70A = 0.005; offset xmmword_1A1FC60 initialised from flt 0x16B2040 = 163.84
at 0x1606140). This gives a 5 mm grid over ±163.84 m, local to the owning entity. World position = entity matrix ×
local (entity+0x10 matrix, or `sub_4EE1E0` if animated). Vertex index × 6 bytes into the u16 array.

### 2.3 GuidanceSystemComponent (desc 0x18D4960, size 0x34, parent Component) — vftable 0x168B074 (38 slots)
| off | field | notes |
|---|---|---|
| +0x08 | owner Entity* | transform source |
| +0x14 | u16* vertex coords, +0x18 count (low 14 bits = number of u16s) | serialized u32 count + raw u16 |
| +0x1C | GuidanceObject* array, +0x20 count (low 14 bits) | serialized u32 count + N×GuidanceObject |
| +0x24 | **EdgeFilter** {float cosAngle; float minCornerAngle; u8 flags} | `sub_6B08B0` reader, defaults `sub_6B0710` |
| +0x30 | bool "check edge validity in world orientation" | ctor sets 0 |

Reader: `GuidanceSystemComponent__Read` 0x545AF0. Clone: 0x545CA0. Ctor: 0x5464C0. The ctor ORs flags |= 0x12.

Subclasses (parent hash 0xEB482613):
- **GuidanceSystem** (desc 0x18FD088, size 0x38, vftable 0x1699E74): adds `Partitioner*` at +0x34. The Partitioner is a
  baked AABB tree (desc 0x18F16F8: u16 index list, AABB min/max, PartNode array), read by `sub_66A000` → `sub_931780`.
  Its queries use the tree (`sub_621530`) instead of brute force.
- **GuidanceSystemCapsule** (size 0x70): p0 +0x40, p1 +0x50, radius +0x60. `sub_668C10` (vslot 37):
  - If the capsule axis is vertical (within 0.001), the capsule is inactive.
  - Otherwise it writes one edge along the top of the capsule, offset by the radius along the "up ⟂ axis" direction.
    The normals are ±side and that up direction.
- **GuidanceSystemBarrel** (size 0x90): axis +0x60, profile array {float,float} at +0x88/+0x8C. `sub_6695D0`:
  - Places one top vertex per profile ring.
  - Writes two edges per segment (left/right side normals plus the up normal).
  - Disables all edges if the axis is vertical.
- InertComponent (static collision, desc 0x18F9FE0) embeds `GuidanceSystem*` at +0xB0. **This is where level guidance lives.**
- GuidanceSystemManager (desc 0x18C8FB8, MANAGER_ID 24) keeps an array of GuidanceSystem* at +8.
- GuidanceSystemGenerationType enum {0 Unknow, 1 None, 2 InertComponent, 3 RigidBody, 4 Behaviour} records which owner
  type a system was generated for. Its only code ref is the enum registration at 0x544A10.

### 2.4 EdgeFilter (desc 0x19037D0, 12 bytes)
Stream order is 7 bools (flags bits 0..6), then float, float, then float angle. The reader stores `[0] = cos(angle)`
(0x6B0A33). Defaults: `[0] = 0.707` (cos 45°), `[4] = 0.3927 rad (22.5°)`, flags 0 → component ctor sets 0x12.

| flag | accepted edge (n0, n1 with z = dot(n, up)) |
|---|---|
| 0x01 | wall/wall corner: both normals horizontal (\|z\| < cos) and the dihedral angle about the edge > `[4]` |
| 0x02 | floor + wall: one z > cos, the other \|z\| < cos (classic ledge) |
| 0x04 | ceiling + wall: one z < −cos, the other \|z\| < cos |
| 0x10 | floor + ceiling, i.e. a thin slab or beam: one z > cos, the other z < −cos |
| 0x08/0x20/0x40 | read but unused by `sub_6B0E10` |

Logic: `EdgeFilter__Accepts` 0x6B0E10. It is called only when component+0x30 is set (`sub_5452F0`), using the
world-rotated normals.

### 2.5 GuidanceContactObject (96 bytes, desc 0x18FDD78) = one entry of GuidanceReport (desc 0x18FD168: array)
| off | meaning |
|---|---|
| +0x00 | 16 bytes runtime scratch (zeroed by ctor 0x676250) |
| +0x10 | p0 world (vec4) |
| +0x20 | p1 world |
| +0x30 | n0 world |
| +0x40 | n1 world |
| +0x50 | Entity* owner |
| +0x54 | GuidanceObject* source |
| +0x58 | GuidanceContactObject* parent (set when a contact is split or clipped) |
| +0x5C | bits 0–5 subtype (GuidanceObjectSubType), bits 6–7 flags |

A contact is added only if `|p1−p0|` exceeds 0.0005 on some axis (0x5AB0D0, 0x5461E0). Report array header is
`{ptr; u32 count(14b) | cap(14b)<<14 | flags}`. Element stride 0x60.

### 2.6 GuidanceZone (desc 0x18E0720, 0x60) and shapes — vftable 0x16902A8 (14 slots)
- +0x10 right (X), +0x20 forward (Y), +0x30 up (Z), +0x40 position: a full 4×4 local frame.
- +0x50 Entity* (optional).
- Setters:
  - `sub_5AAD50` sets position.
  - `GuidanceZone__SetForward` 0x5AAB10 sets Y and rebuilds X/Z.
  - `GuidanceZone__SetUp` 0x5AAC10 sets Z and rebuilds X/Y.
- Ctor 0x5AAA60 sets identity.

| shape | size | params | ctor / dtor | component slot used |
|---|---|---|---|---|
| Box | 0x80 | min +0x60, max +0x70 (local; default ±1) | 0x661E50 / 0x661CC0 | vslot 32 (+0x80) |
| Plane | 0x70 | half-extents +0x60, +0x64 | 0x658780 / 0x6587B0 | vslot 33 |
| Sphere | 0x70 | radius +0x60 | 0x657A00 / 0x657A20 | vslot 34 |
| Capsule | 0x90 | p0 +0x60, p1 +0x70, radius +0x80 | 0x660E10 / 0x660F90 | vslot 35 |
| Mesh | 0x70 | MeshShape* +0x60 | — | vslot 36 |

Zone vslots:
- 9 = GetAABB.
- 10 = filter a source report into a destination.
- 11 = QueryComponent (box: 0x662050). It forwards to the component only if `component->flags & 0x4000000`.
- 12 = **Cut report in place** (box: 0x6621D0), mode {0 keep intersecting, 1 Include = clip to zone, 2 Exclude = remove
  inside part, may split the edge in two}. The modes match the enum GuidanceZoneCutType {None, Include, Exclude}.
- 13 = unknown.

`GameSetting` (desc 0x18D51C8, size 0x170) holds an inline **GuidanceZoneMesh at +0xD0, tagged "Main actor jump
detection shapes"**, a `GuidanceSystemOptimizer*` at +0x160, and many floats (names hashed; values in forge).

### 2.7 GuidanceChain (0x14) / GuidanceLink / GuidanceConnector
- Chain: {?, +4 bool startReversed, +8 bool endReversed, +0x0C GuidanceLink* (each link → GuidanceContactObject*),
  +0x10 count}. It is an ordered run of contacts that connect end to end.
- Connector (0x60): two vec4 + 8 floats + u32 (layout from desc 0x18FDF98; usage not traced).

## 3. Enums (verified from reflection, see RE/data/enums_movement.txt)
- GuidanceObjectSubType (0x18E087C): 0 None, 1 LedgeGrab, 2 Beam, 3 Ladder, 4 Pole, 5 Rope, 6 Surface, 7 Quadruped, 8 Kiosk.
- GuidanceZoneCutType: 0 None, 1 Include, 2 Exclude.
- GuidanceSystemOptimizer::CollisionType: 0 Dynamic, 1 Static.
- GuidanceSystemGenerationType: 0 Unknow, 1 None, 2 InertComponent, 3 RigidBody, 4 Behaviour.
- WorldArea::JumpLinkRange: 0 Normal, 1 Extended. This and MetaLinkTypeID JumpLink* belong to AI navmesh jump links,
  not to player guidance.
- There is **no GuidanceObjectType enum**. "Type" is the subtype field.

## 4. Per-query logic (engine-agnostic pseudo-code)

### 4.1 GuidanceZone::Query — `GuidanceZone__Query` 0x5AB580
```
Query(zone, report, cacheEntity, clip, typeMask, world=default, clearFirst):
  if clearFirst: report.clear()
  aabb = zone.GetAABB()
  if cacheEntity and cacheEntity.world == world:          # fast path through a cached tree (sub_61F150)
      comps = tree_query(aabb); return zone.QueryComponents(comps)   # 0x5AAFF0
  for entity in world.broadphase.query(aabb):
      for comp in entity.components (+ sub-entities of group entities):
          if comp.isGuidance (flags & 0x4000000):
              hit |= zone.QueryComponent(comp, {report, typeMask}, clip)
```

### 4.2 Component test (per shape; box shown — 0x545460 brute force, 0x66A4E0 tree version)
```
if !comp.IsActive(): return            # vslot 37; Capsule/Barrel regenerate edges here
M = inverse(entity world matrix) * zone frame
for obj in candidates (tree query or all):
   if comp.checkValidity and !(obj.enabled and EdgeFilter.Accepts(R*n0, R*n1)): continue
   a,b = decode(obj.vA), decode(obj.vB)          # local
   if clip: if segment_clip_OBB(a,b,box) -> (a',b'): add contact(a',b')   # sub_97A730
   else:    if segment_hits_OBB(a,b,box):          add contact(a,b)       # sub_979C00
add contact = to world, copy normals (rotated), type=obj.subtype; keep if (1<<type)&typeMask and length>0.0005
```
Other shapes: plane `sub_979A50`, sphere `sub_5AC0A0/5AC460`, capsule `sub_5AC370/5AC640`, mesh `sub_5ACEF0/5AD580`.

### 4.3 Post-filters used by Human code
| function | effect | constant |
|---|---|---|
| `GuidanceReport__ExtractByTypeMask` 0x66C220 | move contacts whose `1<<type & mask` into another report (e.g. mask 8 = Ladder in HumanLedge 0xDDCA1C) | — |
| `GuidanceReport__RemoveSteepEdges` 0x66C350 / test 0x66B6D0 | keep an edge only if \|dir.z\| ≤ 0.64278764 (= cos 50°), i.e. **slope ≤ 40° from horizontal** | 0x66B6D0 |
| `GuidanceReport__RemoveNotFacing` 0x66C130 | keep if dot(dir, horizontal wall normal) ≥ minDot | arg |
| `GuidanceContact__GetWallNormal` 0x66BB40 / 0x66B780 | take the normal with the lower z, flatten it to horizontal and normalise. Returns 0 if both normals are horizontal. | 0.0005 |
| zone vslot 12 | include/exclude cut | — |

### 4.4 Chain grab point — `GuidanceChain__FindGrabPoint` 0x66E150
- Inputs: chain, target point, preferred direction, array of hand offsets (e.g. {0.5, 0.1} in HumanInAir `sub_E07820`),
  and a margin.
- If the sum of the offsets plus 0.0005 is at least the chain length, the chain is rejected (it cannot fit the hands).
- Otherwise it clamps the chain ends by the hand offsets and projects the target onto each link's segment
  (`sub_9765E0`, `sub_9470B0`). It returns the closest contact and the grab point.
- `sub_E07820` evaluates every chain in an array of 0x14-byte chains and keeps the one nearest to the target.

### 4.5 Example — HumanLedge `sub_DDC620` (search for ledge to grab)
1. Zone = Box. Position = midpoint of the two hand positions (Human+0x570 / +0x580), with z = max of the two.
   Forward = character forward with z zeroed.
2. Local extents (x = right, y = forward, z = up) per mode a2:

| mode | min | max |
|---|---|---|
| 0 | (−0.75, −0.8, −1.0) | (0.75, 0.8, 2.0) |
| 1 | (−0.75, −0.8, −2.0) | (0.75, 0.8, 0.0) |
| 2 | (−3.0, −0.5, −2.5) | (0.5, 1.0, 1.0) |
| 3 | (−0.5, −0.5, −2.5) | (3.0, 1.0, 1.0) |
| default | (−1, −0.5, −0.5) | (1, 1, 0.5) (centred on a bone position) |

3. `Query(report, cacheEntity = Human's world area, clip = 1, mask = −1, world default, clear)`.
4. `sub_1170880`, then extract Ladders (mask 8) into a second report, then RemoveSteepEdges.
5. Depth detection via `sub_B27E80` with settings {vec(0, 0.02, 0.02, 0), −10° (−0.17453292 rad), 1.0}. It also sets
   Human+0x470 → +0xD4 = 0.1 and +0xD8 = 0.5. Then `sub_C7EC60` (GuidanceWorkspace + DepthDetector), then `sub_B28180`.

`GuidanceZone::Query` has 36 call sites, all in Human modules (Human 0xB1xxxx–0xB2xxxx, Ground 0xD9E750, Ledge 0xDCD4xx,
0xDD6Fxx, 0xDDC9xx, Climb 0xDE1AAF, 0xDEBD42, 0xDEFE2A, InAir 0xDF41xx–0xE0BD6E, Ladder 0xE16673, 0xE19050, Pole 0xE22C77,
Walling 0xE36F4A, Narrow 0xE50604, 0xE59BE0, Decision 0xF7DCC2, helpers 0x1170C16, 0x11711EF). Box zones dominate
(`sub_661E50`: ~40 sites). Spheres, planes and capsules appear in Ledge, Climb, InAir and Walling.

## 5. Constants
| addr / where | value | meaning |
|---|---|---|
| 0x3BA3D70A imm (0x5AC1E4 …) | 0.005 | vertex quantum (m) |
| 0x16B2040 → xmm 0x1A1FC60 | 163.84 | vertex offset (m) |
| 0x3A03126F imm | 0.0005 | min edge length per axis / epsilon |
| 0x66B6D0 | 0.64278764 | cos 50° → max ledge slope 40° |
| `sub_6B0710` | 0.707, 0.3927 rad | EdgeFilter default cos(45°), corner angle 22.5° |
| 0x5464C0 | flags 0x12 | default edge classes: floor+wall, floor+ceiling |
| 0x3A83126F (0x668C10, 0x6695D0) | 0.001 | "axis is vertical" tolerance for Capsule/Barrel |
| `sub_6678D0` OptimizerGroundWall defaults | 0.2, 0.05, 0.1, 0.5, 0.1, 0.9; CollisionType = Static | box query of half-extents (0.5, 0.05, 0.45) (build-time free-space check; **hypothesis**) |
| 0xDDC620 | box tables above | HumanLedge search volumes |
| 0xB27E80 | 0.02, −10°, 1.0, 0.1, 0.5 | depth-detector settings |
| 0xE07820 | {0.5, 0.1} | hand offsets for chain grab |

EdgeFilter values, the GameSetting floats, the jump-detection mesh and all edge data come from **forge**.

## 6. Interfaces
- **In**:
  - Human modules call `GuidanceZone__Query`, the zone setters, report helpers (0x66C4A0 ctor, 0x66BD70 dtor,
    0x66C220, 0x66C350, 0x66C130, 0x66C760 copy) and `GuidanceChain__FindGrabPoint`.
  - Helper layer 0x116C000–0x1175000 (`sub_1170880`, `sub_116CD50` builds connectors, `sub_116E1A0`, …).
  - Depth/workspace: `sub_C7E300`–`sub_C7EC60`.
- **Out**: entity transforms (`sub_4EE1E0`), world broadphase (world+0x18 → +0x54 vslot 13), the Partitioner tree
  (`sub_621530`) and generic geometry helpers in 0x9765E0–0x97A730.
- `GuidanceZoneComponent` (desc 0x18FBEB8, size 0x3D0) is a debug/visualisation component. It holds zones, a
  workspace, tasks, depth/beam detectors, `GuidanceTestRayCast/LinearCast`, and many IColor fields. `HumanGuidance`
  (desc 0x1995BA8, 0x180) holds 8 vec4s, a Human* and two GuidanceReports. It is the per-Human cache of guidance results.

## 7. Open questions / dynamic checks
- Dump real GuidanceSystem blobs from a .forge to confirm the subtype distribution and the EdgeFilter flags per
  building. Expectation: most edges are LedgeGrab; beams are tagged Beam.
- Settle the n0/n1 order convention (top vs wall). Code treats it symmetrically, using "lower z = wall".
- Zone vslot 13 and the GuidanceConnector meaning.
- Details of the DepthDetector / Workspace CastPlanes / BeamDetectorAccurate algorithms (0x672600–0x67A000, 0x67A000–0x681000).
- What fills component+0x30 (validity check) in shipping data — probably only rigid bodies.
- Breakpoint suggestion: on `GuidanceZone__Query` (0x5AB580), log the zone frame/extents and the report count per Human state.

## 8. Renamed functions
| addr | name |
|---|---|
| 0x545AF0 | GuidanceSystemComponent__Read |
| 0x545CA0 | GuidanceSystemComponent__CloneInto |
| 0x5464C0 | GuidanceSystemComponent__ctor |
| 0x545460 / 0x545E50 / 0x5455C0 / 0x545750 / 0x545F70 | GuidanceSystemComponent__QueryBox / QueryPlane / QuerySphere / QueryCapsule / QueryMesh (vslots 32–36) |
| 0x5461E0 | GuidanceSystemComponent__GetAllContacts |
| 0x5452F0 | GuidanceSystemComponent__IsObjectValid |
| 0x66A000 / 0x66A140 | GuidanceSystem__Read / __Clone |
| 0x66A4E0 / 0x66A740 | GuidanceSystem__QueryBox / __QueryPlane (tree accelerated) |
| 0x668C10 | GuidanceSystemCapsule__RegenerateEdges |
| 0x6695D0 | GuidanceSystemBarrel__RegenerateEdges |
| 0x6678D0 / 0x667750 | GuidanceSystemOptimizerGroundWall__ctor / __Read |
| 0x6B08B0 / 0x6B0710 / 0x6B0E10 | EdgeFilter__Read / __SetDefaults / __Accepts |
| 0x5ACC10 | GuidanceObject__Read |
| 0x5ABA10 / 0x5ABA20 | GuidanceObject__GetVertexA / B |
| 0x5AC9F0 / 0x5ACA10 | GuidanceObject__GetNormal0 / 1 |
| 0x5ABC40 | UnpackNormal101010_2 |
| 0x5AB180 / 0x5AB340 | GuidanceObject__AddContact / AddClippedContact |
| 0x5AB0D0 | GuidanceQuery__AddContactIfMatches |
| 0x5AC190 / 0x5AC550 / 0x5AC280 | GuidanceObject__IntersectsBox / ClipToBox / IntersectsPlane |
| 0x5AB580 | GuidanceZone__Query |
| 0x5AAFF0 | GuidanceZone__QueryComponentList |
| 0x5AAF00 | GuidanceZone__FilterReport |
| 0x5AAA60 | GuidanceZone__ctor |
| 0x5AAB10 / 0x5AAC10 | GuidanceZone__SetForward / SetUp |
| 0x661E50 | GuidanceZoneBox__ctor |
| 0x662050 / 0x6621D0 / 0x6625E0 | GuidanceZoneBox__QueryComponent / CutReport / FilterReportInto |
| 0x676250 / 0x676DB0 | GuidanceContactObject__ctor / TransformToWorld |
| 0x66C4A0 | GuidanceReport__ctor |
| 0x66C220 / 0x66C350 / 0x66C130 | GuidanceReport__ExtractByTypeMask / RemoveSteepEdges / RemoveNotFacing |
| 0x66B6D0 / 0x66BB40 / 0x66B780 | GuidanceContact__IsSlopeOk / GetWallNormal / FlattenNormal |
| 0x66E150 | GuidanceChain__FindGrabPoint |

## Implications for a recreation
1. **Extract guidance from .forge.** For each InertComponent, parse the embedded GuidanceSystem:
   - u32 nObj, then per object `u8 enabled, u32 subtype, u32 vA, u32 vB, 16B n0, 16B n1`;
   - u32 nU16 + u16 coords;
   - EdgeFilter (7 bools, 3 floats);
   - u8 flag;
   - Partitioner.
   Decode vertices as `u16*0.005−163.84` in entity space. **This gives the designers' exact climb edges and their
   subtypes (Beam/Ladder/Pole/Rope/Kiosk/Surface).** The subtypes are authored semantics that cannot be reliably
   recovered from meshes.
2. **If forge extraction is postponed, edges can be approximated from collision meshes.** Take each mesh edge shared by
   two faces with normals n0, n1 and keep it under the EdgeFilter rules:
   - ledge: one face z > 0.707 and the other |z| < 0.707;
   - optional ceiling and beam slab classes;
   - corner: both normals horizontal with dihedral > 22.5°;
   - keep runtime slope ≤ 40°.

   Also merge colinear edges and drop interior or occluded ones (that is the missing Optimizer step: the GroundWall box
   ~1.0 × 0.1 × 0.9 m suggests a free-space check behind and above the edge — hypothesis). Subtypes other than LedgeGrab
   need level annotation or heuristics, e.g. thin long slabs → Beam.
3. **Runtime side is easy to reimplement.** Store edges with two normals and a subtype in a BVH. Query with an oriented
   box, sphere or capsule, plus optional clip and a type bitmask. Post-filter by slope, facing (horizontal wall normal)
   and include/exclude volumes. Chain the contacts and pick grab points with hand-offset margins.
4. **Dynamic objects.** Procedural edges are needed for lying capsules and barrels (top ridge, disabled when the axis is
   vertical). Edge validity is re-checked with EdgeFilter in world orientation when the component flag is set.
5. **Tuning values that live in forge** must also be extracted: GameSetting, the jump-detection GuidanceZoneMesh,
   and the per-module *Data.

## Methodology
- Enumerated RTTI vftables (vt.py) and reflection descriptors. A new helper `refl.py` in the scratchpad dumps
  descriptors and properties. The property format is {flags, nameHash, typeHash, (type<<16 | elem<<24), offset<<18, get, set}.
- Followed the binary readers (0x545AF0, 0x5ACC10, 0x6B08B0) to get the on-disk layouts.
- Decompiled the query path (0x5AB580 → zone vslot 11 → component vslots 32–37 → per-object tests → contact creation).
- Scanned E8 call sites (calls.py) to find the Human callers and the report helpers.
