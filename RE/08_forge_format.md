# 08 — `.forge` archive format (version 25) and the reader

Status: **container level verified end-to-end** on DataPC_Map_Menu.forge (all bytes of a 6.9 MB file
accounted for) and on DataPC_Acre.forge (869 files). Every resource class name resolves.
Reader: `RE/tools/forge.py` (pure Python, read-only, no third-party packages; LZO decoder in
`RE/tools/lzo1x.py`). Inventory of all archives: `RE/tools/forge_inventory.py` →
`RE/data/forge_inventory.json`.

## 1. Summary

A `.forge` is a flat archive of named **files**. Each file holds two LZO-compressed containers:
1. a **table of contents** listing resources as `{id, size}`;
2. the **resources** themselves, concatenated.

Each resource is one serialized engine object (World, Entity, Mesh, Animation, Skeleton, …). It has a
small header giving its class (CRC32 of the C++ class name), its name and its size.

```
forge
├─ header  "scimitar\0", version 25, → FileDataHeader
├─ FileDataHeader → linked list of IndexBlocks
│   └─ IndexBlock: index table (16 B/entry) + name table (188 B/entry)
└─ files (at entry.offset):  "FILEDATA" per-file header (0x1B8 bytes)
      ├─ compressed container #1 → TOC: u16 n, n × {u32 resourceId, u32 size}
      └─ compressed container #2 → resources back to back
            resource = {u32 classHash, u32 dataSize, u32 nameLen, char name[nameLen], u8 flag,
                        [sub-header if flag], payload[dataSize]}
```

## 2. Header and tables (verified on Map_Menu and Acre)

### 2.1 File header (offset 0)
| off | type | value / meaning |
|---|---|---|
| 0x00 | char[9] | `"scimitar\0"`. The exe writes the same in its header ctor 0x987330 |
| 0x09 | u32 | version = **25** (0x19; same ctor) |
| 0x0D | u64 | offset of FileDataHeader (0x416 in all files seen) |
| 0x15 | u32 | unknown (0x1000 Map_Menu, 0x10 Acre) |
| 0x19 | u32 | unknown (1) |

### 2.2 FileDataHeader (at header+0x0D)
`{i32 totalFiles; i32 ?(1); u64 ?(0); i64 ?(-1); i32 maxFiles; i32 ?(2); u64 firstIndexBlock}`

### 2.3 IndexBlock (linked list)
`{i32 count; i32 ?; u64 indexTable; u64 nextBlock (-1 = end); i32 firstIndex; i32 lastIndex;
u64 nameTable; u64 rawTable}`. Acre has one full block (869 files) and one empty reserve block.

### 2.4 Index entry (16 bytes)
`{u64 offset; u32 fileId; u32 size}`. `offset` points at the file's `FILEDATA` header, and `size` is the
payload size **after** that 0x1B8-byte header.

### 2.5 Name entry (188 = 0xBC bytes)
| off | meaning |
|---|---|
| 0x00 | u32 size (same as index) |
| 0x04 | u64 (hash/version, unknown) |
| 0x28 | u32 timestamp (e.g. 0x47C9797D = March 2008) |
| 0x2C | char name[128] |
| 0xAC | 16 bytes unknown (4, 4, …) |

### 2.6 Per-file header (0x1B8 bytes at entry.offset)
`"FILEDATA"`, then the file name (zero-padded), then a copy of the name-entry fields (fileId, size,
timestamp, …). The payload starts at `entry.offset + 0x1B8`.
Exception: the first file of every forge, **`GlobalMetaFile`** (170 bytes), is uncompressed metadata.

## 3. Compressed container (verified)

```
u64 magic   = 0x1004FA9957FBAA33
u16 version = 1
u8  codec        0 LZO1X_1, 1 LZO1X_999 (codec table in exe @0x192F46C: {compress, decompress,
                 decompress_safe, workmem, name}; 2 LZO2A, 3 LZX also listed). All files seen use 1.
u16 maxRawChunk     (0x8000)
u16 maxPackedChunk  (0x8000)
u16 chunkCount
chunkCount × {u16 rawSize; u16 packedSize}
chunkCount × {u32 checksum; u8 data[packedSize]}   // stored if packedSize == rawSize
```
- **Decompression:** both LZO1X codecs use the stock `lzo1x_decompress` (exe 0x9A0F40). The work-memory
  sizes in the table (64 KB and 448 KB) match stock LZO. `lzo1x.py` is a clean-room decoder from the
  public bitstream description, and decodes to exact sizes.
- **Checksum = Adler-32 of the packed bytes with initial value 0** (`zlib.adler32(blob, 0)`). Verified:
  stock Adler-32 differed by exactly 1 in the low half and by `len` in the high half.

## 4. Resources (verified)
TOC: `u16 count; count × {u32 resourceId; u32 size}`, and the sizes sum exactly to the data length.

Resource:
| field | meaning |
|---|---|
| u32 classHash | **CRC32 of the C++ class name** (e.g. 0xFBB63E47 = "World", 0x0984415E = "Entity"). Same hash family as reflection property names (07 §4b) |
| u32 dataSize | payload size |
| u32 nameLen, char[nameLen] | resource name (no terminator) |
| u8 flag | 0 = no sub-header; 1 = sub-header follows (seen on Animation, NavMeshManager) |
| sub-header | present when flag != 0. Length = `tocSize − 12 − nameLen − 1 − dataSize`. Looks like a data-layout description (records `{u32 typeHash, u16 typeCode e.g. 0x0A1D = SmallArray<float>, u32 count}`) **(hypothesis)** |
| payload[dataSize] | serialized object. Starts with `u32 resourceId, u32 classHash` (repeated), then fields |

Payload fields follow the reflection descriptors (07); type ids are the engine's `ubi*` types
(type table at 0x192F198). The per-type (de)serializer switches are in exe `0x99BA20–0x99C590`
(13 functions, "switch 26 cases"). Decoding object payloads is the next step.

## 5. What is in the archives

**Full inventory (verified):** all 18 archives read with **0 errors**, giving 532,630 resources in about
12 minutes of pure-Python decompression. Totals include 75,472 Entity, 43,984 Mesh, 22,228 Animation,
14,889 MeshShape, 3,342 Skeleton and 1,432 NavMeshManager. `DataPC_StreamedSounds*.forge` (6,533 files)
hold raw streamed audio without the compressed-container wrapper, so the reader returns no resources
for them. No codec other than 1 (LZO1X_999) was encountered.
Per-archive details: `data/forge_inventory.json`. Earlier sample: the first 12 files of
DataPC_Acre.forge contain 10,240 SoundBao, 366 TextureMap, 356 Entity, 272 Mesh, 167 Material,
133 TextureSet, 98 MeshShape, 88 LODSelector, 24 CollisionMaterial, **22 Skeleton**, **8 Animation**,
3 NavMeshManager, 5 BoxShape, World/WorldArea/GridCellDataBlock and more.

Movement-relevant classes to decode next:
- **Entity** (and its components: InertComponent → `GuidanceSystem` blob, see 06 §2) gives the
  climbable edges.
- **Animation** / **Skeleton** give the root-motion clips. Track templates seen in RTTI:
  `AnimTrackDescriptorTyped<KeyCount8|16, Time8|16, Value…, CompressionFloat8|16|None, Interpolator…>`;
  rotation compression enum `CompressionTypes` {NONE, QUAT64, QUAT48, QUAT32, QUAT24, QUAT16}.
- **MeshShape / BoxShape / ListShape / CollisionMaterial** give collision.
- **NavMeshManager** gives navigation (NPCs only).

## 6. Methodology
1. Hex-dumped the smallest archive (Map_Menu, 1.6 MB) and identified the magic and the offset chain.
2. Found the header writer in the exe from the `"scimitar"` string reference (0x987330: magic,
   version 0x19). Found the codec table from the `LZO1X_*` strings (20-byte records with compress and
   decompress function pointers and LZO work-memory sizes).
3. Parsed the index/name tables and checked them against the file sizes; found the
   `0x1004FA9957FBAA33` container magic after the 0x1B8 per-file header.
4. Wrote a clean-room LZO1X decoder. The first chunk decoded to exactly the declared 1,474 bytes and
   parsed as a TOC whose sizes sum to the second container's length.
5. Identified the checksum (Adler-32, init 0) by comparing candidates.
6. Matched resource class hashes against CRC32 of all RTTI class names, with 100% resolution on both
   test archives. Generalised the header rule (`flag` + sub-header) when an Animation resource broke
   the simple rule.

## 7. Open questions
- Meaning of the unknown header and name-entry fields, and of the sub-header records.
- Object payload serialization: decode with the reflection descriptors plus the exe's type
  switch (0x99BA20…). Validate by round-tripping a resource.
- Codec 2/3 (LZO2A/LZX) never observed. Confirm with the full inventory.
