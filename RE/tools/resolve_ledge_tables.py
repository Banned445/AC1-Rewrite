import pickle, struct, sys
acts, objs, anim, own = pickle.load(open(sys.argv[1], "rb"))
m = pickle.load(open(r"C:\Users\benja\Desktop\Claude\Game Reversal And Recreation\RE\data\ledge_init.pkl", "rb"))
def u32(x): return struct.unpack("<I", bytes(m.get(x + i, 0) for i in range(4)))[0]
def clips(aid):
    ac = acts[aid]; out = []
    for it in ac["items"]:
        if it and "ref" in it: it = objs.get(it["ref"], it)
        out.append("+".join(anim.get(x, "?") for x in it.get("animations", [])))
    return " | ".join(out)
for a in range(0x1A2C3F0, 0x1A2CB80, 4):
    v = u32(a)
    if v in acts:
        print(f"{a:#x}: {v:#x} [{own[v]}] {clips(v)}")
