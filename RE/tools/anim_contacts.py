import os, sys
sys.path.insert(0, r"C:\Users\benja\Desktop\Claude\Game Reversal And Recreation\RE\tools")
from forge import Forge
import ac_anim
GAME = r"C:\Users\benja\Desktop\Claude\Assassin's Creed"
fg = Forge(os.path.join(GAME, "DataPC.forge"))
e = [x for x in fg.entries if x.name == "Game Fix"][0]
want = sys.argv[1].split(",")
BITS = ["Lheel", "Rheel", "Ltoe", "Rtoe", "Lhand", "Rhand", "noLook"]
for r in fg.resources(e):
    if r["name"] not in want:
        continue
    a = ac_anim.decode(r["payload"])
    print(f"== {r['name']} dur={a['duration']:.3f}")
    for t in a["tracks"]:
        if t["key"] in (2, 3):
            kind = "CONTACTS" if t["key"] == 2 else "STEPPHASE"
            seq = []
            for tm, v in zip(t["times"], t["values"]):
                v = v[0]
                lab = "+".join(b for i, b in enumerate(BITS) if v >> i & 1) if t["key"] == 2 else str(v)
                seq.append(f"{tm:.3f}:{lab or '-'}")
            print(f"  {kind} ({t['comp']}): " + "  ".join(seq))
