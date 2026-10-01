# 09 — Skeleton, Mesh, Material and TextureMap payload formats

Status: decoded on Altaïr's Rank 9 outfit (`DataPC.forge` → file 23 "Rank 9") and verified by internal
consistency checks (below). Readers: `RE/tools/ac_skeleton.py`, `RE/tools/ac_mesh.py` (exploration) and the
port's runtime loader `port/src/assets/` (Rust). Container format: RE/08.

## 1. Where Altaïr is
`DataPC.forge` files 22–36 are "AssassinsCreed" and "Rank 0".."Rank 9" (one per upgrade level). File 23
"Rank 9" contains:
- the Entity `UCMA_Altair_Rank_9`;
- Skeletons `UCMA_Altair` (90 bones), `UCMA_Altair_Head` (30), `UCMA_Altair_Skirt`;
- Meshes `UCMA_Altair_Body_C`, `_Head`, `_Boots_B`, `_Cloth_UP`, `_Flaps`, `_Shoulderpad`, `_BackSheath_B`,
  `_Sword_Sheath_D`, `_Knife_Foot/Shoulder/Belt`, `_Cloth` (soft body);
- weapons; Materials, TextureSets and TextureMaps.

## 2. Object serialization (verified on Skeleton)
- **Inline object:** `u8 0, u32 objectId, u32 classHash (CRC32 of class name), <fields in reflection order>`
- **ObjectPtr to an object serialized elsewhere:** `u8 2, u32 objectId`. **Null:** `u8 3`.
- **SmallArray:** `u32 count`, then the elements.
- The resource payload starts with `u32 resourceId, u32 classHash`.

## 3. Skeleton (class 0x24AECB7C) and Bone (0x95741049)
`u32 id, u32 class, u32 0, u32 boneCount (90)`, then bones as inline objects. Bone fields (reflection
desc 0x1908178):
- `u32 BoneID` (CRC32 of the bone name, e.g. 0x58988870 = "LeftFoot");
- `ObjectPtr parent`;
- an embedded transform object (class 0x6350E5A6) holding two `{vec4 position, quat rotation}` pairs:
  **pair A = global (model space), pair B = local (relative to parent)**;
- `SmallArray Modifiers` (HingeBoneModifier, CompressBoneModifier, RollBoneModifier, …);
- `u8 Index` and a second `u8` (= number of descendants for the root, hypothesis).

**Verified:** for all 89 child bones, `parent.A ∘ child.B == child.A` with error 0.0 (position and
rotation). Space is metres, Z-up, origin at the hips: the skeleton spans z −0.961 … +0.845, so Altaïr is
1.81 m tall.
Note: this stored global pose is rotated relative to the meshes' bind pose. Use the meshes' inverse bind
matrices for skinning.

### 3.1 Mesh (class 0x415D9568)
```
u32 id, u32 class, u32 1 (ResourceCategory?), u32 0
u32 nBones; nBones × { u32 objId, u32 class 0x9EF0E7A1, u32 BoneID, f32 m[16] }   // 76 B each
    m = inverse bind matrix: row-major, row-vector convention, translation in row 3, metres
u8 0, u32 objId, u32 class CompiledMesh (0xFC9E1595), u32 dataSize, Data:
    u32 vertexFormat (0x16), u32 stride, u32 vbBytes, u32 ibBytes, u32 0, u32 0, u32 nSub, u32 nSub, u32 ?
    vertices[vbBytes], u16 indices[ibBytes/2]   (triangle list, absolute indices)
nSub × { u32 4, u32 vStart, u32 vCount, u32 iStart, u32 triCount }   (written twice)
nSub × palette { u8 3, u8 1, u8 n, u16 ?, u16 vCount, u8 bone[n] }   (7-byte header)
… u32 nSub, nSub × u32 Material resource id   (one material per submesh, in order)
```
**Vertex format 0x16, stride 32 (skinned):**
| off | type | meaning |
|---|---|---|
| 0 | s16 × 4 | position; **metres = s16 / 2048**; w = 32767 |
| 8 | u8 × 4 | normal, `b/127.5 − 1` (unit length verified 0.98–1.01) |
| 12 | u8 × 4 | tangent |
| 16 | u8 × 4 | binormal |
| 20 | s16 × 2 | UV, `/4096` (range 0…0.996) |
| 24 | u8 × 4 | bone indices into the submesh palette |
| 28 | u8 × 4 | bone weights (sum 252–255, i.e. /255) |

Stride 24 is the unskinned format (weapons). Its position scale is different and not decoded yet.

**Verification (Body_C: 3,128 vertices, 3,887 triangles, 77 bones, 5 submeshes):**
- **Topology:** submesh vertex ranges tile all vertices, each index start equals 3 × the preceding
  triangle count, the max index is 3127, and every palette's vCount matches its submesh.
- **Inverse binds:** transforming a bone's own position by its matrix gives about 0 (metres, skeleton
  space).
- **Scale fit:** fitting vertex-cluster centroids (vertices weighted ≥ 250 to one bone) against the bind
  positions picks a uniform 1/2048. X/Y residuals are ±3 mm; Z is about −3 cm, because joints sit at the
  tops of limb segments. 1/1024 and 1/4096 are 0.2–0.46 m off.
- **Assembly:** the boots end at −0.93 m against the skeleton's toe at −0.96 m. The assembled model is
  1.8–1.9 m tall (asserted in the Rust tests).

### 3.2 Parts in bone space
A part whose shared bones have the **same** inverse bind as the body is already in body space (Boots,
Cloth_UP, Flaps, Shoulderpad). A part whose shared bone has a **different** bind (Head, Knife_*,
Sword_Sheath) is modelled in that bone's space. It is mapped with
`v_body = v_part · invBind_part(b) · invBind_body(b)⁻¹` for the shared bone b. That places the head on the
shoulders and the sheath on the hip (checked visually).

## 4. Materials and textures
### 4.1 TextureMap
| off | meaning |
|---|---|
| +0x08 | width |
| +0x0C | height |
| +0x14 | format code (2 for these BC1 textures; full meaning not mapped) |
| +0x20 | mip count (11 for 1024²) |

A `u32 dataSize` (found at +0x4F on these) is followed by the mip chain, largest first, as **BC1 (DXT1)**
8-byte blocks, or BC3 16-byte blocks for alpha maps. The format is identified from the data size.
**Verified:** 699,064 bytes is exactly a 1024², 11-mip BC1 chain; the 512² head texture's 174,776 bytes is
exactly a 10-mip chain.

### 4.2 Material → texture chain
`Material` → `TextureSet` (refs) → `TextureMapSpec` (`…DiffuseMapSpec`, `…NormalMapSpec`,
`…SpecularMapSpec`) → `TextureMap`. These are found by scanning a payload for u32 ids of resources in the
same file. The exact field layout of each step is not decoded; reference scanning is unambiguous here.

### 4.3 Entity material overrides
Meshes reference placeholder materials (`ARCM_Altair_Cloth_Empty`, `_Leather_Empty`, `_Hands_Empty`). The
character Entity (`UCMA_Altair_Rank_9`) holds adjacent (placeholder, real) Material id pairs:
Leather_Empty→Leather, Cloth_Empty→Cloth, **Hands_Empty→Leather** (gloves at Rank 9),
Cloth_Alpha_Empty→Cloth_Alpha, Eye→Eye_Altair. Lower ranks reference `ARCM_Altair_Hands_Rank0` instead.

## 5. Not decoded yet
- The unskinned stride-24 vertex format (weapons).
- The soft-body `UCMA_Altair_Cloth` mesh (different trailer) and `DynamicMesh`.
- `Universal_Head_Obj_Clean` eyes and mouth (LODSelector), normal/specular use, and the alpha cloth.
- The Bone second u8, the transform class name, CompiledMesh header fields `?`, and the Mesh u16/bool fields.
- **Animation** (stage 4): track descriptors `AnimTrackDescriptorTyped<KeyCount8|16, Time8|16,
  Value…, CompressionFloat8|16|None, Interpolator…>` and the quaternion compression enum (RE/08 §5).
