# 10 — `Animation` resource format (key compression, tracks, root motion)

Status: **decoded and verified.** All 12,340 `Animation` resources in DataPC.forge file "Game Fix" parse to the
exact last byte. All 4,136,993 decoded quaternion keys are unit length (|q| within 0.01 of 1). The footl/footr
half-cycles of walk, jog, run and sprint join with 0.00° and 0.000 m error.
Decoder: `RE/tools/ac_anim.py` (`decode`, `sample`, `root_motion`). Container rules: RE/08. Object
serialization rules: RE/09 §2.

## 1. Summary

An `Animation` (class hash 0x0FA3067F = CRC32("Animation")) contains:
- a duration;
- a few reflected **event tracks** (footstep/contact events, sound and FX), as ordinary serialized objects;
- an **AnimTrackData** object that lists each track's key: a **BoneID** (CRC32 of the bone name, as in the
  Skeleton) or a small **fixed track id** (0 DISPLACEMENT, 1 PIVOT, 2 ACUATORCONTACTS, 3 STEPPHASES,
  4 CENTEROFMASS);
- one **compressed track** per key. A track is a byte that selects one of 64 *descriptors*
  (`AnimTrackDescriptorTyped<KeyCount, Time, Value, Compression, Interpolator>`), then a key count, the key
  times (1/60 s ticks, 8 or 16 bit) and the packed values.

Rotations are stored as **"smallest three" quaternions** at 16, 24, 32, 48, 64 or 96 bits. Translations use
fixed-point millimetres (32- or 48-bit vectors). Bone tracks hold **local (parent-relative)** transforms in
the Skeleton's convention: metres, Z up, quaternions (x, y, z, w).

**Root motion:** fixed track 0 (DISPLACEMENT) is a Vec3 track plus a Quat track. It goes from the identity at
t=0 to the clip's total root displacement at t=duration. Forward is **+Y**.

## 2. Payload layout (Animation__Serialize 0x6DD8C0, verified)

All values are little-endian.

| off | type | field (runtime offset in the Animation object) |
|---|---|---|
| 0x00 | u32 | resource/object id |
| 0x04 | u32 | class hash 0x0FA3067F |
| 0x08 | f32 | **duration** in seconds (+0x0C). e.g. 0.3333 for one run step |
| 0x0C | u32 | hash (+0x10); always equals the first field of AnimTrackData. Meaning unknown (per-clip value) |
| 0x10 | u8 | flag → bit 0 of +0x20 (always 0 in Game Fix) |
| 0x11 | u8 | flag → bit 1 of +0x20 (always 0) |
| 0x12 | u32 | n = number of AnimTrack objects (+0x14 array; 0–4 seen) |
| … | n × object | each serialized with `sub_931780`: `u8 0, u32 objId, u32 classHash, fields…`. Seen: `AnimTrackEvent` (0x41682213) containing `ContactEventSeed` (0x993AB038), `ContactEvent` (0x97840CBF) and similar. **Not needed for skeletal playback.** The decoder skips to the AnimTrackData header by searching for its class hash; this succeeded on all 12,340 clips |
| … | object | AnimTrackData (+0x1C), serialized via reflection (`sub_9312F0(&0x181EFE8, …)`): `u8 0, u32 objId, u32 0x0181EFE8`, then:<br>`u32 hash` (= payload 0x0C)<br>`u32 m` (SmallArray count), then m × `{u32 objId(0), u32 0x653CAA76 (AnimTrackDataMapping), u32 trackKey}`<br>`u16` (0 in samples) |
| … | u32 | T = number of compressed tracks (Animation__ReadCompressedTracks 0x6DB850). T == m, and **track i belongs to mapping i** |
| … | T × track | see §3 |

The mappings are sorted: the fixed ids come first (0, 0, 2, …), then the BoneIDs in ascending order. A key
can appear twice: once for a **Vec3** (translation) track and once for a **Quat** (rotation) track. In the
samples this happens for DISPLACEMENT (0) and for `Reference` (0x2C52CBB0). Tell the two apart by the
track's value kind.

The resource **sub-header** (RE/08 §4, flag = 1; 43–115 bytes) lists the class hashes used by the reflected
event objects (AnimTrackEvent, EventSeed 0x3AE012B2, ContactEventSeed, ContactEvent, 0x6CE04D52, …) with type
codes 0x0A1D and 0x0016 and counts. **(hypothesis: a type-dependency table for the object loader.)** It is
not needed to decode the tracks.

## 3. Compressed track (AnimTrackDescriptorTyped<…>::Read = vtable slot 21, e.g. 0x49FF10, 0x4A1940, 0x4A66A0)

```
u8   desc        index into the descriptor table 0x1A11E90 (filled by AnimTrackDescriptor__RegisterAll 0x4B45A0)
u32  allocSize   size of the runtime blob (ignore)
u32  n           key count
(n-1) × time     key times of keys 1..n-1 in 1/60 s ticks; u8, or u16 if Time16.
                 Key 0 is implicitly at time 0.
n × value        packed values, contiguous (no padding in the file; the runtime blob is realigned)
```
Time unit: f32 60.0 at 0x1912CA0, so `seconds = ticks / 60`. Sampling converts with
`frame = (i64)(t*60 + 0.5)` (0x4A5730). The last key time equals `duration*60` (run: 20 ticks = 0.3333 s).

### 3.1 Descriptor index → type (0x4B45A0, verified by the RTTI vftables)
`desc = 4*group + (KeyCount16 ? 2 : 0) + (Time16 ? 1 : 0)`. KeyCount8/16 only changes the runtime blob,
**not** the file layout. Time16 makes the time entries u16.

| group | desc | value | compression | bytes/key | interpolator |
|---|---|---|---|---|---|
| 0 | 0–3 | Quaternion | QuaternionNone | 16 | LinearSlerpTestForLerp |
| 1 | 4–7 | Quaternion | Quaternion16 | 2 | **LinearLerp** |
| 2 | 8–11 | Quaternion | Quaternion24 | 3 | LinearSlerpTestForLerp |
| 3 | 12–15 | Quaternion | Quaternion32 | 4 | LinearSlerpTestForLerp |
| 4 | 16–19 | Quaternion | Quaternion48 | 6 | LinearSlerpTestForLerp |
| 5 | 20–23 | Quaternion | Quaternion64 | 8 | LinearSlerpTestForLerp |
| 6 | 24–27 | Quaternion | Quaternion96 | 12 | LinearSlerpTestForLerp |
| 7 | 28–31 | Vector3 | Vector3None | 12 | Vector3Linear |
| 8 | 32–35 | Vector3 | Vector332 | 4 | Vector3Linear |
| 9 | 36–39 | Vector3 | Vector348 | 6 | Vector3Linear |
| 10 | 40–43 | Float | FloatNone | 4 | FloatLinear |
| 11 | 44–47 | Float | Float8 | 1 | FloatLinear |
| 12 | 48–51 | Float | Float16 | 2 | FloatLinear |
| 13 | 52–55 | Byte | ByteNone | 1 | ByteLinear |
| 14 | 56–59 | Byte | ByteNone | 1 | ByteLowest |
| 15 | 60–63 | Byte | ByteNone | 1 | ByteNearest |

Each singleton (e.g. 0x1A12B68) stores `{vftable, u32 valueSize(16/4/1), fnSample, fnLayout, u8 valueType,
u8 index}`. valueType: 0 quat, 2 vec3, 5 float, 4 byte.

Usage in Game Fix (tracks): Quat16 259k, Quat48 203k, Quat32 52k, Quat24 42k, Vec332 28k, Float8 25k,
Quat96 20k, Vec348 12k, ByteLowest 3.4k, ByteLinear 0.3k, Quat64 0.1k, Vec3None 4. QuatNone, Float16 and
FloatNone were not seen.

### 3.2 Quaternion "smallest three" decoders (exact; SSE code)

The three stored components are the quaternion without its largest-magnitude component. The decoder
rebuilds that component as `m = sqrt(1 − (c0²+c1²+c2²))` (rsqrt·x) and inserts it at index `idx`, in the
order **(x, y, z, w)**:
`idx 0 → (m,c0,c1,c2)`, `1 → (c0,m,c1,c2)`, `2 → (c0,c1,m,c2)`, `3 → (c0,c1,c2,m)`.
Unless noted, m is always positive. `OFF = 0.70710677` (0xBF3504F3 = −OFF).

| scheme | decoder | bit layout | component formula | idx |
|---|---|---|---|---|
| Quat16 | AnimQuat16__Decode 0x48CA30 | u16 v | c0=(v>>8)&15, c1=(v>>4)&15, c2=v&15; `c·0.094280906 − OFF` (0x3DC11659 = √2/15) | `v>>14`; **m negated if v & 0x2000**; bit 12 unused |
| Quat24 | AnimQuat24__Decode 0x48CE10 | 3 × u8 b | `(b&0x7F)·0.01113554 − OFF` (0x3C3671D7 = √2/127) | `(b0>>7) \| (b1>>7)<<1`; b2 bit 7 unused |
| Quat32 | AnimQuat32__Decode 0x48D120 | u32 v | c0=(v>>20)&0x3FF, c1=(v>>10)&0x3FF, c2=v&0x3FF; `c·0.0013810679 − OFF` (0x3AB504F3 = √2/**1024**) | `v>>30` |
| Quat48 | AnimQuat48__Decode 0x48D4C0 | 3 × u16 s | `(s&0x7FFF)·4.3159689e-5 − OFF` (0x3835065D = √2/32767) | `(s0>>15) \| (s1>>15)<<1` |
| Quat64 | AnimQuat64__Decode 0x48D7D0 | u64 q | c0=q[8..27], c1=q[52..63] \| (q[0..7]<<12), c2=q[32..51] (20 bits each); `c·1.3487005e-6 − OFF` (0x35B504FF = √2/1048575) | `q[30..31]`; bits 28–29 unused |
| Quat96 | AnimQuat96__Decode 0x48DBE0 | 3 × f32 | the raw floats (their LSBs carry the index) | `(bits(f0)&1) \| (bits(f1)&1)<<1` |
| QuatNone | 0x49EA20 | 4 × f32 | (x,y,z,w) directly | — |

### 3.3 Vector3 and Float decoders
| scheme | function | formula |
|---|---|---|
| Vec348 | 0x4AE390 | 3 × s16, `·0.001` m (range ±32.767 m) |
| Vec332 | 0x4AC6C0 | u32 v: `x = sext11(v>>21)`, `y = sext11((v>>10)&0x7FF)`, `z = sext10(v&0x3FF)`, each `·0.001` m (x,y ±1.024 m; z ±0.512 m) |
| Vec3None | 0x4AAB30 | 3 × f32 |
| Float8 | 0x4935F0 | s8 `·0.0080000004` |
| Float16 | 0x494BA0 | s16 `·0.0080000004` |
| FloatNone | — | f32 |
| Byte* | — | u8 |

### 3.4 Sampling / interpolation (AnimTrackDescQuat48K8T8__Sample 0x49ADA0)
1. `frame = (int)(t·60 + 0.5)`. Find key k with `time[k] ≤ frame < time[k+1]` (AnimTrack__FindKeyTime8
   0x48F610, cached key index). Before key 0 → key 0; after the last key → the last key; n = 1 → constant.
2. `u = (clamp(t, t_k, t_k+1) − t_k) / (t_k+1 − t_k)`, using the float time (times × 1/60, table 0x1A13490).
3. Quaternions (Quat__InterpSlerpTestForLerp 0x4916F0): `d = dot(a,b)`. If `d ≥ 0.98935574` (0x3F7D466B),
   use **nlerp**: Quat__NLerpShortest 0x48F310 flips b when d < 0, lerps, then normalises. Otherwise use
   **slerp** (Quat__SlerpShortest 0x48F120, shortest arc; returns b if |θ| ≤ 0.0005). The Quat16 group uses
   "LinearLerp" (nlerp, **hypothesis**).
4. Vec3 and Float: linear. Byte: Linear / Lowest / Nearest. Lowest and Nearest are step functions
   **(hypothesis; not traced)**.

## 4. Track semantics

| key | tracks seen | meaning |
|---|---|---|
| 0 DISPLACEMENT | Vec332/Vec348/Vec3None (translation) + Quat96 (rotation) | root motion of the clip. Identity at t=0, total motion at t=duration. Frame: **+Y forward, +X right (hypothesis), Z up**. Locomotion clips have 2 keys, so the speed inside the clip is constant |
| 1 PIVOT | Quat48 / Vec3 | rare (71 clips) |
| 2 ACUATORCONTACTS | ByteLowest | foot-contact bitmask per key. Run: 4, 0, 10, 8; idle: 15 = all four set **(hypothesis: L/R heel/toe bits)** |
| 3 STEPPHASES | ByteLowest | 183 clips |
| 4 CENTEROFMASS | Vec332 | 65 clips |
| BoneID | Quat* (rotation), optional Vec3 (translation) | **local** bone transform, relative to the parent bone |
| other hashes | Float8 | facial / blend-shape channels (cinematic `ai_cm1_*` clips) |

**Verified: bone rotations are local.** For the idle `xx_h_wait_hipm_footl`, the first key is within 0.4° of the
Skeleton's local rest rotation (RE/09 pair B) for Hips and Spine, and within ~3° for the hands. Other bones
differ by 2–45°, which is real pose difference. Against the *global* rest the differences are 70–170°. Vec3
bone tracks also match the local rest positions (e.g. helper bone 0xA9611103: anim (0.066,0,0), rest local
(0.073,…)).

**`Reference` (root bone 0x2C52CBB0)** is identity at the origin in the Skeleton's rest pose. In animations it
carries a full transform (pelvis position about 0.86–0.96 m above the displacement origin, plus a yaw of about
70–100° about Z). So the animation space origin is at the feet / ground contact. The Skeleton's rest
origin is at the hips. Pose a character as:

```
world(bone) = EntityTransform · Displacement(t) · Reference(t) · … · local(bone, t)
```
**(hypothesis for the exact composition of DISPLACEMENT with the entity; the engine-side root-motion
consumer is not traced.)** A bone without a track keeps its rest local transform. A bone with only a
rotation track keeps its rest local translation **(hypothesis, consistent with data)**.

## 5. Validation results
- **Parse:** 12,340 / 12,340 Animation resources in "Game Fix" consumed to exactly `len(payload)`, with no
  exceptions. This exercises K16, T16, all quaternion schemes except None, Vec3None/32/48, Float8 and Byte.
- **Unit quaternions:** 4,136,993 / 4,136,993 keys have | |q| − 1 | < 0.01.
- **Loop continuity:** the last key of every bone in `*_footl` equals the first key of `*_footr`, and the
  reverse: max 0.00° / 0.000 m for walk, jog, run and sprint.
- **Smoothness:** the largest change between adjacent keys in run is 57° on LeftLeg (knee) over 4 ticks
  (67 ms). In walk it is 29°.
- **Rest-pose match:** see §4.

## 6. Measured root-motion speeds (Altaïr, `xx_*_hipm_foot{l,r}`, DISPLACEMENT end value / duration)

| gait | clip | duration | displacement (one step) | **speed** | cadence |
|---|---|---|---|---|---|
| walk | xx_l_walk_hipm_footl / footr | 0.5333 / 0.4667 s | 1.012 / 0.885 m | **1.90 m/s** | 1.0 s per stride |
| walk (front, slow) | xx_l_walkfront_hipm_footl / footr | 0.2667 / 0.3333 s | 0.438 / 0.548 m | **1.64 m/s** | |
| jog | xx_h_jog_hipm_footl / footr | 0.4667 s | 1.651 m | **3.54 m/s** | 0.933 s/stride |
| run | xx_h_run_hipm_footl / footr | 0.3333 s | 1.707 m | **5.12 m/s** | 0.667 s/stride |
| sprint | xx_h_sprint_hipm_footl / footr | 0.2667 s | 1.674 m | **6.28 m/s** | 0.533 s/stride |
| idle | xx_h_wait_hipm_footl | 5.0 s | 0 | 0 | |
| strafe (combat) | xx_h_defence_strafe_* | 1.5333 s | ~1.3 m | 0.79–0.91 m/s | |

(Displacement is pure +Y with zero yaw. The full list of 1,789 walk/run/sprint/jog/strafe clips is in
`RE/data/anim_root_motion_gamefix.txt`, regenerated with `ac_anim.root_motion`.) Whether the game's
locomotion code then scales playback to match a separately tuned speed is covered in RE/02 (open).

## 7. Open questions
- Exact bit meaning of ACUATORCONTACTS / STEPPHASES bytes; ByteLowest/Nearest semantics.
- AnimTrackEvent / ContactEvent field layout (reflected; decode with RE/07 descriptors if events are needed for
  footstep sounds).
- The u32 hash at payload 0x0C / AnimTrackData field 0x567530C5, the trailing u16 0x66A6DC91, and the two
  Animation flag bits.
- The sub-header records.
- Quat16 "LinearLerp" interpolator code not read (assumed nlerp); the sign bit 0x2000 only exists in Quat16.
- How the engine composes DISPLACEMENT with the character's transform, and Reference's role (check in the
  root-motion consumer; dynamic check: log Human position per frame while running).
- Mirroring: footl/footr have identical displacement. Whether right-foot clips are stored separately or mirrored
  at runtime: both are stored (both exist as resources).

## 8. Renamed functions
| address | name |
|---|---|
| 0x6DD8C0 | Animation__Serialize |
| 0x6DC620 | Animation__Clone |
| 0x6DB850 | Animation__ReadCompressedTracks |
| 0x4B45A0 | AnimTrackDescriptor__RegisterAll |
| 0x48CA30 / 0x48CE10 / 0x48D120 / 0x48D4C0 / 0x48D7D0 / 0x48DBE0 | AnimQuat16/24/32/48/64/96__Decode |
| 0x49FF10 / 0x4A1940 / 0x4A66A0 | AnimTrackDescQuat16K8T8 / Quat24K8T8 / Quat48K16T16 __Read |
| 0x49FE80, 0x4A18B0, 0x4A3370, 0x4A4FA0, 0x4A6C50, 0x4A8A40, 0x49EA20, 0x4AAB30, 0x4AC6C0, 0x4AE390, 0x4935F0, 0x494BA0, 0x4A65E0 | AnimTrackDesc<scheme>__GetKey (vtable slot 19) |
| 0x48F610 | AnimTrack__FindKeyTime8 |
| 0x4A5730 | AnimTrackDescQuat48K8T8__FindKeyAtTime |
| 0x49ADA0 | AnimTrackDescQuat48K8T8__Sample |
| 0x4916F0 / 0x48F310 / 0x48F120 | Quat__InterpSlerpTestForLerp / Quat__NLerpShortest / Quat__SlerpShortest |
| 0x4A0480 / 0x4A03C0 | AnimTrackDescQuat16K8T8__CompressFrames / __CompressTimes (encoder side, names tentative) |

Descriptor vftable slot map (all 64 share it): 19 = GetKey(blob, keyIndex, out), 21 = Read(stream), 13/14 =
FindKey(frame / time), 10/11 = build from source keys (frames / float times × 60), 20 = swap keys, 22 = allocate.

## 9. Methodology
1. The Animation vftable 0x169D954 (RTTI) has slot 2 = 0x6DD8C0, the serializer. It reads two scalars and
   two bits, an AnimTrack* array, the reflected AnimTrackData, and then a custom loop (0x6DB850) that indexes
   a global descriptor table with a leading byte.
2. The table 0x1A11E90 is filled at start-up by 0x4B45A0. A Python scan of its `mov [abs], imm` stores mapped
   all 64 indices to their RTTI vftable names (scratch `desc_table.txt`).
3. Decompiled the vtable slot-21 readers (file layout) and slot-19 key getters. These lead to the SSE
   decoders. Read the masks (0x1683350 etc.) and float constants from the exe.
4. Wrote `ac_anim.py` and checked it on "Game Fix": exact byte consumption, unit norms, cycle continuity,
   and the rest-pose comparison with Skeleton UCMA_Altair (Rank 9).
