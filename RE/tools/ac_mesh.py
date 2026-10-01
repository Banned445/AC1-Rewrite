"""Parse AC1 `Mesh` resource payloads (exploratory; see RE/09_mesh_skeleton_format.md).

Layout observed on UCMA_Altair_Body_C (all verified by internal consistency):
  u32 resourceId, u32 classHash(Mesh)
  u32 ?(1), u32 ?(0)
  u32 nBones, nBones x {u32 objId, u32 class 0x9EF0E7A1, u32 boneId, f32 mat44 (row-major, row-vector,
                        translation in row 3) = inverse bind matrix, metres, skeleton space}
  u8 ?, u32 objId, u32 class CompiledMesh, u32 dataSize, Data[dataSize]:
      u32 vertexFormat(0x16), u32 stride, u32 vbBytes, u32 ibBytes, u32 0, u32 0, u32 nSub, u32 nSub, u32 ?,
      vertices[vbBytes] (stride 32: s16x4 pos | u8x4 normal | u8x4 tangent | u8x4 binormal | s16x2 uv |
                         u8x4 boneIdx (into the submesh palette) | u8x4 weights (sum ~255)),
      u16 indices[ibBytes/2] (triangle list)
  then submesh tables: nSub x {u32 4, u32 vStart, u32 vCount, u32 iStart, u32 triCount} (twice)
  then per-submesh bone palettes: {u8 3, u8 1, u8 n, u16 ?, u16 vCount, u8 palette[n]} (7-byte header) …
"""
import struct
import sys

MESH = 0x415D9568
COMPILED_MESH = 0xFC9E1595


def parse(d):
    p = 8
    _a, _b, nb = struct.unpack_from("<3I", d, p)
    p += 12
    bones = []
    for i in range(nb):
        _oid, _cls, bid = struct.unpack_from("<3I", d, p)
        m = struct.unpack_from("<16f", d, p + 12)
        bones.append((bid, m))
        p += 76
    # CompiledMesh object header
    cm = d.find(struct.pack("<I", COMPILED_MESH), p)
    size = struct.unpack_from("<I", d, cm + 4)[0]
    data_off = cm + 8
    fmt, stride, vb, ib, _z0, _z1, nsub, _nsub2, _q = struct.unpack_from("<9I", d, data_off)
    v0 = data_off + 36
    nv = vb // stride
    verts = []
    for i in range(nv):
        o = v0 + i * stride
        pos = struct.unpack_from("<4h", d, o)
        nrm = d[o + 8:o + 12]
        uv = struct.unpack_from("<2h", d, o + 20)
        bi = tuple(d[o + 24:o + 28])
        bw = tuple(d[o + 28:o + 32])
        verts.append({"q": pos, "n": nrm, "uv": uv, "bi": bi, "bw": bw})
    idx = struct.unpack_from(f"<{ib // 2}H", d, v0 + vb)
    t = v0 + vb + ib
    subs = []
    count = struct.unpack_from("<I", d, t)[0]  # first entry tag (4)
    for k in range(nsub):
        tag, vs, vc, ist, tc = struct.unpack_from("<5I", d, t + 20 * k)
        subs.append({"vstart": vs, "vcount": vc, "istart": ist, "tris": tc})
    t += 40 * nsub
    for k in range(nsub):
        assert d[t] == 3 and d[t + 1] == 1, f"palette marker at {t:#x}"
        n = d[t + 2]
        vcount = struct.unpack_from("<H", d, t + 5)[0]
        pal = list(d[t + 7:t + 7 + n])
        subs[k]["palette"] = pal
        subs[k]["pal_vcount"] = vcount
        t += 7 + n
    return {"bones": bones, "fmt": fmt, "stride": stride, "verts": verts, "indices": idx, "subs": subs,
            "data_size": size, "tail": t}


if __name__ == "__main__":
    m = parse(open(sys.argv[1], "rb").read())
    print(len(m["bones"]), "bones;", len(m["verts"]), "verts;", len(m["indices"]) // 3, "tris;", "fmt", hex(m["fmt"]))
    for s in m["subs"]:
        print(s)
