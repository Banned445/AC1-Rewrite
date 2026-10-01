import pickle, struct, sys
sys.path.insert(0, r"C:\Users\benja\Desktop\Claude\Game Reversal And Recreation\RE\tools")
import ac_actions as a
acts, objs, anim, own = pickle.load(open(sys.argv[1], "rb"))
m = pickle.load(open(r"C:\Users\benja\Desktop\Claude\Game Reversal And Recreation\RE\data\climb_init.pkl", "rb"))
def u32(x): return struct.unpack("<I", bytes(m.get(x + i, 0) for i in range(4)))[0]
def i32(x): return struct.unpack("<i", bytes(m.get(x + i, 0) for i in range(4)))[0]
POSE = ["1m", "1lu", "1ru", "2m", "2lu", "2ru"]
def clips(aid):
    ac = acts.get(aid)
    if not ac: return f"{aid:#x} NOT IN GRAPH"
    out = []
    for it in ac["items"]:
        if it and "ref" in it: it = objs.get(it["ref"], it)
        out.append("+".join(anim.get(x, "?") for x in it.get("animations", [])))
    return f"{aid:#x} -> " + " | ".join(out)
for base, nm in [(0x1A2D070, "SHORT"), (0x1A2D7F0, "LONG")]:
    print(nm)
    for p in range(6):
        for d in range(10):
            e = base + 24 * (p * 10 + d)
            nxt = i32(e); aid = u32(e + 20)
            if nxt in (0, 15) and aid == 0 or nxt >= 9: continue
            print(f"  {POSE[p]} dir{d} -> {POSE[nxt] if nxt < 6 else nxt}: {clips(aid)}")
