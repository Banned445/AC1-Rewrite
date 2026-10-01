"""Parse AC1 `Skeleton` resource payloads (exploratory; see RE/09_mesh_skeleton_format.md).

Serialization rules observed (verified on UCMA_Altair):
  inline object    : u8 0, u32 objectId, u32 classHash, <fields>
  pointer to object: u8 2, u32 objectId          (ObjectPtr to an object serialized elsewhere)
  null pointer     : u8 3
  Bone fields      : u32 BoneID (name hash), ObjectPtr parent, BaseObject transform
                     {u8 0, u32 id, u32 class 0x6350E5A6, vec4 pos, quat rot, vec4 pos2, quat rot2},
                     SmallArray Modifiers (u32 count, inline objects), u8 Index?, u8 ?
"""
import struct
import sys

BONE = 0x95741049


class Reader:
    def __init__(self, d, pos=0):
        self.d, self.p = d, pos

    def u8(self):
        v = self.d[self.p]
        self.p += 1
        return v

    def u32(self):
        v = struct.unpack_from("<I", self.d, self.p)[0]
        self.p += 4
        return v

    def f32s(self, n):
        v = struct.unpack_from(f"<{n}f", self.d, self.p)
        self.p += 4 * n
        return v


def find_bones(d):
    """Return the offsets of every inline Bone object header (u8 0, id, class Bone)."""
    out = []
    for i in range(len(d) - 9):
        if d[i] == 0 and struct.unpack_from("<I", d, i + 5)[0] == BONE:
            out.append(i)
    return out


def parse_bone(d, off):
    r = Reader(d, off)
    assert r.u8() == 0
    obj_id = r.u32()
    assert r.u32() == BONE
    bone_id = r.u32()
    kind = r.u8()
    parent = r.u32() if kind == 2 else None
    # embedded transform object
    k = r.u8()
    tr_id = r.u32()
    tr_class = r.u32()
    a = r.f32s(8)
    b = r.f32s(8)
    return {"off": off, "id": obj_id, "bone_id": bone_id, "parent": parent, "tr_kind": k, "tr_class": tr_class,
            "a_pos": a[:4], "a_rot": a[4:], "b_pos": b[:4], "b_rot": b[4:], "end": r.p}


if __name__ == "__main__":
    d = open(sys.argv[1], "rb").read()
    bones = [parse_bone(d, o) for o in find_bones(d)]
    print(len(bones), "bones")
    for b in bones[:12]:
        print(f"{b['id']:08x} bid {b['bone_id']:08x} parent {b['parent'] and hex(b['parent'])} "
              f"A pos {tuple(round(x, 3) for x in b['a_pos'])} rot {tuple(round(x, 3) for x in b['a_rot'])} | "
              f"B pos {tuple(round(x, 3) for x in b['b_pos'])} rot {tuple(round(x, 3) for x in b['b_rot'])}")
