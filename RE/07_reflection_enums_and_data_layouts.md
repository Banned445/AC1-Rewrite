# 07 — Reflection system, enums and module data layouts

Source: static analysis of `bin/AssassinsCreed_Dx9.exe` (v1.02 build 86610) with Python
readers (`RE/tools/pe.py`, `enums.py`, `vt.py`, `classes.py`) cross-checked in IDA.
Raw outputs: `RE/data/enums_all.json` (551 enums), `RE/data/enums_movement.txt`
(166 movement-related enums), `RE/data/data_classes.txt` (decoded field layouts).

## 1. Summary

The Scimitar engine keeps a runtime reflection database in `.data`:
- **Enums keep their value names** as strings. That gives exact names for every movement
  sub-state machine (ledge, climb, beam, walling, pole, ladder, ground, air…).
- **Class properties are hashed.** Each property's name is stored only as a 32-bit hash,
  but its **type, byte offset and (for enums) the enum type are stored in the clear**. So
  we know the exact memory/serialized layout of every reflected class, but not the field
  names. Names can be recovered later by hashing guessed names, once the hash function is
  identified (open question).

This matters for a recreation because the enum lists are effectively the designers' own
list of every movement state, and the layouts tell a `.forge` reader what to expect.

## 2. Binary formats (verified)

### 2.1 Enum records and descriptors
```c
struct EnumRecord {            // 12 bytes
    const char* name;          // e.g. "SubState_Pullup"
    int32_t     value;
    uint32_t    hash;          // hash of name
};
struct EnumDesc {              // 16 bytes
    EnumRecord* records;
    int32_t     count;
    uint32_t    hash;          // enum type hash; referenced by property descriptors
    const char* name;          // e.g. "HumanLedgeData::LedgeSubState"
};
```
Verified on `ActorStateID` (records @0x190DFE8, descriptor @0x190E340) and the three
`HumanClimbData` enums (descriptors @0x1997844, 0x1997854, 0x1997864).
The scanner `RE/tools/enums.py` found **551** descriptors; all validated (every record name is
a printable string).

### 2.2 Class descriptors
Observed layout (verified on HumanClimbData @0x19978D8, HumanGroundData @0x1996288,
HumanInAirData @0x1995920):
```c
struct ClassDesc {
    PropDesc*   props;        // +0x00 contiguous array of 32-byte property descriptors
    int32_t     numProps;     // +0x04
    EnumDesc*   enums;        // +0x08 enums declared in this class
    int32_t     numEnums;     // +0x0C
    uint32_t    unk10, unk14; // +0x10 (0 in all observed)
    const char**groups;       // +0x18 editor group labels ("IK values", "Jump distances", …)
    int32_t     numGroups;    // +0x1C
    const char* name;         // +0x20 "HumanClimbData"
    uint32_t    baseHash;     // +0x24 same value e00ae315 for all Human*Data module classes → base/parent id (hypothesis)
    uint32_t    classHash;    // +0x28 unique per class (hypothesis)
    uint32_t    unk2C;        // +0x2C size-like value (0x70 for HumanClimbData), exact meaning unverified
};
```
Immediately before each descriptor there is also an array of pointers to the same property
descriptors (for HumanClimbData @0x1997890..0x19978D0), most likely sorted for hash lookup
**(hypothesis)**.

### 2.3 Property descriptors (32 bytes)
```c
struct PropDesc {
    uint32_t flags;      // +0x00 0 or 0x02000001 (meaning unverified)
    uint32_t nameHash;   // +0x04 hash of the property name (name string NOT present)
    uint32_t typeHash;   // +0x08 for enums = EnumDesc.hash; for objects = class/type hash
    uint32_t typeCode;   // +0x0C type id in bits 16..31
    uint32_t packed;     // +0x10 byte offset in bits 18..31  (offset = packed >> 18)
    uint32_t pad[3];
};
```
The offset rule `packed >> 18` was checked against field spacing: consecutive floats come out 4
bytes apart, vector fields 16 bytes apart, bools 1 byte apart, and enums line up with their
enum type hash.

Type codes are **verified**: the engine's own type-name table is at 0x192F198 (16-byte names), the
size table at 0x192F378 (u16) and the "is POD" table at 0x192F410. The type-to-name function is
`sub_98BDF0`, and the per-type copy dispatch is `sub_98CAA0`. The type id is the low 6 bits; higher
bits carry modifiers such as the element type of arrays (e.g. 0x051D = SmallArray of U16) and
bitfield packing (0xC0/0x60C0).

| id | name | size | id | name | size |
|---|---|---|---|---|---|
| 0x00 | ubiBool | 1 | 0x10 | ubiMatrix44 | 64 |
| 0x01 | ubiChar | 1 | 0x11 | ObjectID | 4 |
| 0x02 | ubiS8 | 1 | 0x12 | Handle | 4 |
| 0x03 | ubiU8 | 1 | 0x13 | Object (embedded) | var |
| 0x04 | ubiS16 | 2 | 0x14 | ObjectPtr | 4 |
| 0x05 | ubiU16 | 2 | 0x15 | BaseObjectPtr | 4 |
| 0x06 | ubiS32 | 4 | 0x16 | BaseObject | var |
| 0x07 | ubiU32 | 4 | 0x17 | StaticArray | var |
| 0x08 | ubiS64 | 8 | 0x18 | BigArray | var |
| 0x09 | ubiU64 | 8 | 0x19 | Enum | 4 |
| 0x0A | ubiFloat | 4 | 0x1A | String | 4 |
| 0x0B | ubiVector2 | 8 | 0x1B | LString | 4 |
| 0x0C | ubiVector3 | 12 | 0x1C | Reference | 4 |
| 0x0D | ubiVector4 | 16 | 0x1D | SmallArray | var |
| 0x0E | ubiQuaternion | 16 | 0x1E/0x1F | None | – |
| 0x0F | ubiMatrix33 | 36 | | | |

Corrections to earlier inferences: 0x06 is S32 and 0x07 is U32 (not the other way round); 0x12 is a
Handle (to another file/object, typeHash = class); 0x14 is an ObjectPtr; 0x16 is an embedded BaseObject
(the HumanData bone "handles" are BaseObjects); 0x051D is a SmallArray of U16.

## 3. Module data classes

Every Human state module has a reflected `HumanXxxData` object, bound through
`DataContext<Human, HumanData, HumanXxxData>` (RTTI @0x1700C84 for Climb). These objects
contain the module's **current sub-state enum** (e.g. HumanGroundData +0xB0 = HumanGroundSubState),
so they are at least partly **runtime state**. Group labels such as "rebound distances"
(Walling), "Jump distances" / "Beam Variables" (NarrowObject), "Grid settings" / "IK values"
(Climb) show they also carry **per-character parameters**, most likely loaded from the
character's data in `.forge` **(hypothesis — to confirm when the forge reader exists)**.

| Class | descriptor | unk2C (size?) | props | enums declared | groups |
|---|---|---|---|---|---|
| HumanData | 0x1954B80 | 0x370 | 49 | HumanDeathState, AnimSyncState, WeaponState | Bone Pointers, Carried objects |
| HumanGroundData | 0x1996288 | 0x300 | 38 | ObstacleLeanType, MvtDivisions, HumanGroundSubState | fight system |
| HumanInAirData | 0x1995920 | 0x400 | 32 | FallOrigin, JumpType | – |
| HumanLedgeData | 0x1994F10 | 0x150 | 21 | 10 enums (see §4) | GuidanceObject reports |
| HumanClimbData | 0x19978D8 | 0x70 | 17 | EntryPoseType, ClimbSubState, EntryType | IK values, Grid settings |
| HumanLadderData | 0x1995370 | 0x2D0 | 13 | MvtDivisions, EntryType, InclinationType, MvtAnimState | – |
| HumanPoleData | 0x1994210 | 0x200 | 12 | EntryType, MvtDivisions, MvtAnimState, InclinationType | – |
| HumanRopeData | 0x1993B70 | 0x40 | 6 | – | – |
| HumanWallingData | 0x19935D8 | 0x24 | 7 | WallingSide, WallingType, WallingSubState | rebound distances |
| HumanNarrowObjectData | 0x1994740 | 0x1C0 | 21 | PilotisEntryType, LeanState, FreeRunSubState, NarrowObjectSubState, NarrowEdgeState | Jump distances, Beam Variables |
| HumanLookAtData | 0x1994828 | 0x14 | 1 | – | – |
| HumanPushableContextData | 0x1993E48 | 0x30 | 3 | – | – |
| StPadControlledData | 0x1970ED8 | 0x40 | 1 | – | – |
| NavigationContextData | 0x1910938 | 0xC | 2 | NavStyleType, NavSpeed | – |
| MainNavigationData | 0x1956768 | 0x7C | 9 | – | – |
| AssassinAbilitySet | 0x19B55C8 | 0x18 | 38 | MaxSpeed | SuperSets |

Full field-by-field layouts (offset, type code, name hash, enum type): `RE/data/data_classes.txt`.

Example (HumanClimbData, verified decoding):
```
+0x10 vec4
+0x20 enum ClimbSubState     +0x24 enum EntryType     +0x28 enum EntryPoseType
+0x2C .. +0x5C  13 floats (grid settings / IK values — names hashed)
```

## 4. Movement enums (value names verbatim from the exe)

### 4.1 Module ("context") IDs — `ActorContextID` @0x19640EC
0 NO_TRANSITION, 1 INITIALIZATION, 2 NOT_DEFINED, 3 Debug, **4 Ground, 5 Ladder, 6 Pole,
7 Rope, 8 InAir, 9 Ledge, 10 Climb, 11 Walling, 12 NarrowObject**, 13 Riding,
14 PushableContext, 15 RagdollGround, 16 Vocalization, 17 CustomAction, 18 LookAt, 19 Kiosk,
20 Dead, 21 HayStack, 22 UpperBody.
This is the list of Human state modules; each module class maps to one ID.

### 4.2 Ground
- `HumanGroundData::HumanGroundSubState`: 0 Movement, 1 Fight, 2 FreeRun, 3 OrientedMove, 4 Hurt, 5 ObstacleCollision
- `HumanGroundData::MvtDivisions`: 0 WaitLow, 1 WaitHigh, 2 WalkVerySlow, 3 Walk, 4 Jog, 5 Run, 6 Crouch, 7 MAX, 8 Invalid
- `HumanGroundData::ObstacleLeanType`: Hands, Feet
- `AssassinAbilitySet::MaxSpeed`: NoMovementAllowed, Walk, Jog, Run, Sprint
- `HumanSpeedCameraActivator::HumanSpeed`: WalkVerySlow, WalkSlow, Walk, Jog, Run, Sprint
- `NavigationContextData::NavSpeed`: NavigationDriven, Sneak, Walk, Jog, Run, Sprint
- `GoGamePlayBaseData::NavigationSpeeds`: WalkVerySlow, WalkSlow, Walk, WalkFast, Jog, Run, Sprint
- `StepPhase`: WALK_DOUBLE_LEFT_MOVE, WALK_SINGLE_LEFT_MOVE, WALK_DOUBLE_RIGHT_MOVE, WALK_SINGLE_RIGHT_MOVE, RUN_SINGLE_LEFT_MOVE, RUN_FLIGHT_LEFT_PASSING, RUN_SINGLE_RIGHT_MOVE, RUN_FLIGHT_RIGHT_PASSING, INVALID
- `GroundIKState` (foot IK): DELAYTOFADEIN, FADEIN, RUNNING, DELAYTOFADEOUT, FADEOUT, STOPPED
- `PushStrength`, `PushDistance`, `PushResponse` (crowd pushing) — see enums file.

### 4.3 In air
- `HumanInAirData::JumpType`: **0 Straight, 1 1m, 2 3m5m** (jumps are bucketed by distance)
- `HumanInAirData::FallOrigin`: Ground, Climb, HangWall, HangFree
- `LandingEvent::LandingType`: Safe, SmallDamage, HeavyDamage, Fatal
- `WorldArea::JumpLinkRange`: Normal, Extended

### 4.4 Ledge / climb
- `HumanLedgeData::LedgeSubState`: 0 Entry, 1 Movement, 2 TurnCorner, 3 HandPlacement, 4 Pullup, 5 ReboundTransition, 6 HangWallReception, 7 HangFreeReception, 8 SwingReception, 9 TransitionInFromClimb, 10 TransitionInFromLadder, 11 PullDown, 12 HandPassOver, 13 Grasp, 14 ParallelJump
- `HumanLedgeData::GraspType`: HangKnee, Climb, HangWaist, HangWall, HangFree_1Hand, HangFree_2Hands
- `HumanLedgeData::LedgeHangType`: Wall, Free, WallFree
- `HumanLedgeData::{NextHandToMove, PullDownPart, PullDownSide, PullDownSubState, PullDownType, HandPassOverSubState, HangFreeReceptionType}` — see enums file
- `HumanClimbData::EntryType`: Default, FromLedge, FromLedgeParallelJump, FromGround
- `HumanClimbData::EntryPoseType`: 1M, 2M · `ClimbSubState`: Wait

### 4.5 Beams, walling, poles, ladders
- `HumanNarrowObjectData::NarrowObjectSubState`: 0 Movement, 1 Beam, 2 BeamEntry, 3 BeamReception, 4 PilotisReception, 5 Edge, 6 FreeRun, 7 Lean, 8 CrowdRun, 9 ObstacleCollision
- `HumanWallingData::WallingSubState`: 0 None, 1 WallingEntryA, 2 WallingEntryB, 3 WallingVertical, 4 WallingHorizontal, 5 ReboundTransition, 6 WallingVerticalEnd, 7 WallStep
- `HumanPoleData::MvtAnimState` (11 values), `HumanLadderData::MvtAnimState` (20 values), plus EntryType / InclinationType / MvtDivisions for both — see enums file.

### 4.6 World markup
- `GuidanceObjectSubType`: 0 None, **1 LedgeGrab, 2 Beam, 3 Ladder, 4 Pole, 5 Rope, 6 Surface**, 7 Quadruped, 8 Kiosk
- `GuidanceSystemGenerationType`: Unknow, None, InertComponent, RigidBody, Behaviour
- `WalkabilityTypeID`: Impossible, Difficult, Full_NoSpawn, Full, IgnoredByNavMesh

### 4.7 Input and high-level
- `ControlType` (50 context actions): e.g. 10 FreeRun, 16 Grasp, 21 Jump, 29 QuickDrop, 35 Step, 37 Swing, 44 Walk
- `AssassinButtons`: ChangeProfile, Head, ArmedHand, UnarmedHand, Legs, … (AC1's "puppeteer" button mapping)
- `Pad::PadButton` (16 buttons), `Pad::PadStick` (Left, Right)
- `PresentationEvent::ActorStateID` (70 values; used for HUD/presentation events, not the movement FSM itself (hypothesis))
- `StateID` (AI/controller brain states): 0 PadControlled, 1 Wander, … 16 GamePlay, 20 Tutorial

## 4b. Property-name hash = standard CRC32 (verified)

The hashes in `EnumRecord.hash` and `PropDesc.nameHash` are **standard CRC-32 (zlib/IEEE,
reflected, init and xorout 0xFFFFFFFF) of the plain name**. Verified on `"Enter"` → 0x78B1EF6A,
`"Exit"` → 0x343B2B30 and `"ActorStateID_NONE"` → 0x1660C3C8, and on many property names below.

Recovery (`tools/crack.py`): hashed every identifier in the exe, then 1-, 2- and 3-word CamelCase
combinations of a 2,500-word vocabulary built from the exe's own identifiers.
**146 of 270** movement property names recovered. About 50M guesses against 270 targets should
produce roughly 3 chance collisions, so implausible names are marked `(?)` in
`data/data_classes_named.txt`.

Highlights (offsets inside each data object):
- **HumanDataBundle** (one per Human, holds every module's data): DebugData +0x10, GroundData +0x40,
  InAirData +0x340, LadderData +0x740, PoleData +0xA10, RopeData +0xC10, LedgeData +0xC50,
  ClimbData +0xDA0, WallingData +0xE10, NarrowObjectData +0xE40, RidingData +0x1000,
  PushableContextData +0x1040, CustomActionData +0x1070, HeadOrientationData +0x1080,
  VocalizationData +0x10B0, LookAtData +0x10CC, KioskData +0x10E0, DeadData +0x1110,
  HayStackData +0x1120, UpperBodyData +0x1150.
- **HumanData**: LeftHandObject/RightHandObject, CurrentWeapon(+State), FootMeshOffset +0x168,
  ToeMeshOffset +0x16C, Tolerance +0x170, bone handles Head/Pelvis/LeftHand/RightHand/LeftFoot/RightFoot
  (+0x180..+0x1A8), GodMode +0x218.
- **HumanGroundData**: CollideNormal +0x10, DestSight +0x30, DestHeading +0x50, CollidePosition +0x70,
  SubState +0xB0, ParamFlags +0xC4, InternalFlags +0xC8, CurrentBodyAngle +0xD4, ObstacleLeanType +0xDC,
  CollideHeight +0xE0, CollideEntity +0xE4, PinDownData +0x100, Crouch +0x11C, Sprint +0x123.
- **HumanInAirData**: JumpOrigin +0x10 (64-byte transform), ReboundDirection +0x180,
  JumpOrientationAction +0x1A4, JumpFallAction +0x1B0, JumpType +0x1B8, FallOrigin +0x1F4.
- **HumanLedgeData**: JumpDirection +0x30, PullDownType/Side, GraspType +0x48, DirectionType +0x58,
  NextHandToMove +0x64, LedgeHangType +0x68, SwingStrength +0x70.
- **HumanClimbData**: UseIK +0x3C, LeftToePull/RightToePull/LeftHandPull/RightHandPull +0x40..+0x4C (IK pull weights).
- **HumanNarrowObjectData**: CurrentBeamDir +0x10, CurrentBeamCenter +0x20, BeamEntryVector +0x40,
  BeamEntryPoint +0x50, CurrentEdgeDir +0x80, CurrentLeanHeight/Width +0xC0/+0xC4, EdgeState +0xD0.
- **HumanWallingData**: WallingType +0xC, WallingSide +0x10, StartHeight +0x14.
- **HumanPoleData**: PoleJumpDirection +0x20, GrabPosition +0x30, Pole +0x58.
- **HumanLadderData**: Ladder +0x48, LadderHeight +0x4C, HeightInLadder +0x50, ReachedTop/ReachedBottom +0x54/+0x55.
- **AssassinAbilitySet** (progression-gated abilities, bit flags): Jump, Climb, Grasp, Walling, LeapOfFaith,
  PassOver, BalanceAlways, AirAssassination, LookDown, StealthAssassination, CounterGrab, …; MaxSpeed; HighProfile.

This confirms the data objects are mostly **runtime state** (current beam direction, height in ladder,
collision normal, current sub-state), not static tuning.

## 5. Implications for a recreation
1. **Use these enums as the state machine spec.** They are the original design's state lists;
   a Bevy recreation can mirror them as Rust enums one-to-one.
2. **A `.forge` reader can use the layouts to parse module data objects** without knowing
   field names, and decode enum fields to readable names.
3. **Property names are hashed.** Recovering them means identifying the hash function (likely
   a CRC32 variant; enum records also carry hashes of their known names, which are ideal test
   vectors) and hashing candidate names. Worth doing once `.forge` parsing starts.

## 6. Open questions
- Identify the name-hash function using EnumRecord (name, hash) pairs as test vectors.
- Confirm type-code meanings from the serializer code (find the switch over type codes).
- Confirm what `unk2C` is (size vs. alignment/version).

## 7. Methodology
1. Found `ActorStateID_*` strings in IDA, found no code xrefs, then searched for their pointer
   bytes (`find_bytes`), which located the 12-byte record table in `.data`.
2. Found the enum descriptor format next to the `HumanClimbData` strings, then generalised it into
   a whole-image scanner (`tools/enums.py`), validating every record.
3. Followed the class name pointer to the class descriptor, decoded the property descriptors, and
   checked the `>> 18` offset rule against float/vector/bool strides.
