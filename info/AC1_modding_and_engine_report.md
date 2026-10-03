# Assassin's Creed (2008) — modding scene & engine knowledge

Research date: 2026-10-02. Sources: Nexus Mods (all 123 AC1 mods, browsed by endorsements), ModDB
(9 mods + downloads), GitHub, the Ubisoft REAC 2025 Anvil talk, and community tool docs. Third-party
claims are marked **(community claim)** until we verify them in our IDB.

---

## 1. TL;DR — what matters for this project

1. **EaglePatch's C++ source is public and targets our exact exe.** Its README lists
   `AssassinsCreed_Dx9.exe` MD5 `8E72C3333743780E43BC2C34BBF625F9`, which is the same as both
   `bin/AssassinsCreed_Dx9.exe` and the installed copy (checked with `md5sum`). Its source
   gives class names, struct layouts and addresses for the **input layer** (`scimitar::Pad`,
   `PadXenon`, `PadProxyPC`). These can go straight into the IDB (§4).
2. **The Definitive AC1 Parkour Mod** (HenryPDT, Nexus #62) is a Cheat Engine table that
   rewrites the parkour targeting at runtime. Its feature list is a map of the movement
   system's internal concepts, in the modder's terms: prescriptive vs freeform airtime,
   NSO snap, ejects, hang-to-hang, staggered hangs, perfect grabs and stumble landings (§5).
   The `.CT` file probably holds addresses and offsets for those systems.
3. **"Catch Ledge" (grabbing a ledge while falling) is gated by rank, and the gate is in data.**
   "Catch Ledge For All Ranks" (#116) and "Desynced" (#177) turn it on for every rank by editing
   `.data` files inside `DataPC.forge` with AnvilToolkit. No exe patch is involved. Our RE docs
   don't cover this rank gate yet; the recreation needs it.
4. **Level geometry and climbable markup can be modded.** "Jerusalem Reworked (With Tree
   Parkour)" (#156) moves world pieces and makes palm trees climbable, which shows that the
   guidance/climbable data (RE/06) lives in editable world data.
5. **Engine:** the engine is called **Scimitar**. Ubisoft later renamed it **Anvil**, and Scimitar
   is still the internal name of the AC branch of Anvil (Blacksmith = Rainbow Six/For Honor,
   Silex = Ghost Recon). The forge magic `"scimitar\0"` and the C++ namespace `scimitar::` in
   EaglePatch both match this.
6. **No anti-cheat and nothing online today.** The only network code is telemetry to
   `gconnect.ubi.com`. That server is gone, and the connection attempts cause stutters, so
   most fix mods remove them. Modding is safe.

---

## 2. How people mod AC1 — the routes

| Route | Tools | What it can change | Examples |
|---|---|---|---|
| **Forge/data repack** | **AnvilToolkit** (kamzik123, Nexus #30): unpacks/repacks `.forge` → `.data` → resources, converts many resource types to XML and back. Older: Turfster's `.forge` extractor/replacer (ModDB), QuickBMS `scimitar.bms`, Delutto's 2019 tools (replaced by ATK) | Textures, meshes, outfits, entity builders, AI spawns, world layout, gameplay data flags | Hi-Res Texture Mod, outfit mods, Hardcore AC1 (swaps soldier types in `DataPC_Common.forge`), Catch Ledge For All Ranks, Jerusalem/Acre/Damascus Reworked |
| **ASI plugin (native hooks)** | **Ultimate ASI Loader** (`dinput8.dll` proxy) + `scripts/*.asi`; EaglePatch uses withmorten's small win32 `patcher` (InjectHook/PatchByte/Nop) | Anything in the exe: input, rendering flags, combat behaviour | EaglePatchAC1, No Sword Swap (`CombatPatch.asi`), SubtitleSynchAC1, SpitePatch (now pattern-scans), Modern Fixes |
| **Proxy DLL + ini** | `.dll` + `.ini` dropped in the root | Camera and controller fixes | Camera and Controls Fix (Khemitude, originally from FilePlanet) |
| **Cheat Engine tables** | `.CT` with Lua, applied to the running game | Runtime patches to movement/AI | **Definitive AC1 Parkour Mod**, trainers |
| **Patched exes** | Hex edits | Telemetry removal (zero out the `gconnect.ubi.com` string) | Lag Fix (#81) |
| **Post-process injection** | ReShade, ENBSeries (old) | Colour, AO, DOF, "RTGI" | About 30 of the 123 Nexus mods |
| **Video swaps** | Replace the Bink files in `Videos/` | Intros | 4K intro mods |
| **Save files** | Replace saves | Unlocks | "All Flags and Templars Unlocked" |

The mod list leans heavily toward texture, outfit and ReShade mods. Only a few touch gameplay or
the exe, and those few are the ones useful to us.

---

## 3. Engine facts ("how the game is built")

### 3.1 Lineage
- **Scimitar**: Ubisoft Montréal's engine, built for Assassin's Creed (2007). At a REAC 2025 talk
  (Lopez & Bouchard, *Anvil rendering architecture*), Ubisoft said Anvil has three forks:
  **Scimitar** ("the Assassin's Creed engine", the main line), **Blacksmith** and **Silex**. The
  modern monorepo still builds solutions named `scimitar.engine.*.sln` / `scimitar.tool.*.sln`
  and keeps a separate package group for Assassin's Creed, because the engine was originally
  built for AC.
- The talk describes modern Anvil, not the 2007 code, but some of it plausibly goes back to AC1
  **(hypothesis, matches what we found in the exe)**:
  - Data/code bridging comes from generated reflection ("Mold" files that generate C++/C#
    property grids, **serialization** and data deprecation). AC1 has full RTTI and a reflection
    system with CRC32 name hashes for classes and properties. We decoded it in RE/07.
  - A C# editor talks to the C++ engine (built in "ToolMode") over TCP/IP. The shipped exe is
    "EngineMode" and reads cooked data. Any debug and editor strings left in the exe would come
    from ToolMode.
  - The engine is "massively jobified" today, especially culling and animation. AC1 already ran
    on multicore Xbox 360/PS3 hardware.
- ModDB lists the engine as "Anvil". Wikipedia and the community use "Scimitar" for AC1.

### 3.2 Binary and install (our install, verified)
- `AssassinsCreed_Dx9.exe` / `AssassinsCreed_Dx10.exe`: 32-bit x86, MSVC, not packed. EaglePatch
  calls this build the "Director's Cut Edition" (GOG/Steam). `AssassinsCreed_Game.exe` and
  `AssassinsCreed_Launcher.exe` are launcher stubs.
- Data: `DataPC.forge` (core and Altaïr: files 22–36 are "AssassinsCreed" and "Rank 0..9";
  see RE/09), `DataPC_Common.forge` (shared NPCs and soldiers; Hardcore AC1 edits it), one forge
  per city or memory (`Acre`, `Damascus`, `Jerusalem`, `Kingdom`, `Masyaf`, `Arsuf`,
  `SolomonTemple`, …), `DataPC_StreamedSounds*.forge` (audio), and `DefaultBindings.map` (key
  bindings).
- Middleware: Bink video (`binkw32.dll`), EAX audio (`eax.dll`), D3DX9/10.
- `um scan`: no anti-cheat, no loader installed, engine signature not recognised (expected for a
  native proprietary engine).

### 3.3 `.forge` / `.data`
- Magic `"scimitar"`, then a two-level structure: a **forge** holds named **files** (which the
  community calls `.data` files), and each file is a pair of LZO containers with a TOC and the
  resources. Our RE/08 documents version 25 in full, and that matches the community tools.
- AnvilToolkit exports AC1-era resources as "Standard XML" (tag-based). The "Schema-based XML"
  that mirrors the engine schema only applies to Origins and later. Its wiki lessons cover UI,
  text and fonts, textures, meshes, BuildTables and materials, and animations, mostly with AC2
  files as the examples.
- Other references: Mischa-Alff/broadside wiki (AnvilNext `.forge` format, later games),
  gentlegiantJGC/ACExplorer (Python forge explorer, written for Unity-era games), XeNTaX/ZenHAX
  threads (QuickBMS scripts for AC1 sound forges).

### 3.4 Telemetry
- When the player kills, collects or completes something, the exe tries to reach
  `gconnect.ubi.com`. The server is down, and the attempts cause hitches. EaglePatch patches
  one byte at `0x017382D8` (DX9) to turn it off (§4). The Lag Fix zeroes the hostname string
  instead.

---

## 4. EaglePatch — symbols for our IDB (DX9, same MD5 as ours)

Source: `github.com/sergeanur/eaglepatch`, `EaglePatch/src/ac1.cpp` (C++, last push 2025-12-02,
no licence file, so treat it as reference only and don't copy the code into the port).
The exe is detected by `MEMCMP32(0x00401375+1, 0x42d6)`. **(community claim; not yet checked
in IDA)**

### 4.1 Functions and globals (DX9)
| Address | EaglePatch name | Notes |
|---|---|---|
| 0x93F990 | `Pad::UpdateTimeStamps` | `__thiscall(Pad*)` |
| 0x93FC80 | `Pad::ScaleStickValues` | stick deadzone/scale (useful for the port's input model) |
| 0x9161A0 | `PadXenon::ctor(padId)` | XInput pad |
| 0x90B2F0 | `PadProxyPC::AddPad(pad, type, name, u16, u16)` | |
| 0x909C90 | `PadProxyPC::Update` (patched entry) | picks the active pad and copies its state into the proxy each frame |
| 0x916979 / 0x916990 | XInput-add hook site / jump-out | |
| 0x924070 | `getNewDescriptor(size, align, var)` | engine allocator descriptor |
| 0x7A4510 | `allocate(...)` | engine allocator (file/line args) |
| 0x916440 | `delete(ptr, ?, ?)` | |
| 0x1A1E680 | allocator descriptor global | |
| 0x98CF98..0x98D092 | PS3-control byte/dword patch sites | Button-to-action remap table code. Shows where pad buttons are bound to game actions |
| 0x405495 | skip-intro-videos branch (`0xEB`) | |
| 0xE91422 / 0xE9116D / 0xE91178 | multisampling clamp | |
| 0x017382D8 | telemetry enable byte | |

### 4.2 Struct layouts (from `static_assert`s in the source)
```
scimitar::Object        { void** vtable; }
scimitar::ManagedObject : Object { int m_Flags; }
scimitar::Pad : ManagedObject                          // sizeof 0x500
  ButtonStates m_LastFrame, m_ThisFrame;              // bool[16]
  u64 m_LastFrameTimeStamp, m_ThisFrameTimeStamp;
  u64 m_ButtonPressTimeStamp[16];
  float m_ButtonValues[16];                           // analog per button
  align16 {float x,y} LeftStick, RightStick;
  int field_1B0[17]; float* vibrationData; char pad[0x390];
  vtable[10] = UpdatePad(InputBindings*)
enum PadType   { MouseKeyboardPad, PCPad, XenonPad, PS2Pad }
enum PadButton { Button1..4, PadDown, PadLeft, PadUp, PadRight, Select, Start,
                 ShoulderLeft1, ShoulderLeft2, ShoulderRight1, ShoulderRight2,
                 StickLeft, StickRight }               // 16
PadXenon : Pad { u32 m_PadIndex; {XINPUT_CAPABILITIES; bool Connected, Inserted, Removed} } // 0x520
PadData   { Pad* pad; char[528]; InputBindings* }     // 536 bytes
enum PadSets { Keyboard1..4, Joy1..4 }
PadProxyPC : Pad { int; u32 selectedPad; int; PadData pads[8]; }   // 0x15D0
```
Link to RE/01: `Pad__IsPressed` 0x93F170, `JustPressed` 0x93F190, `PressedWithin` 0x93F370,
`GetStick` 0x93F570 sit right next to EaglePatch's `UpdateTimeStamps` 0x93F990 and
`ScaleStickValues` 0x93FC80. `PressedWithin` reads `m_ButtonPressTimeStamp`.

**Port note:** in the original code, keyboard buttons give analog values of exactly 0 or 1.
Only gamepads produce true analog values, and only once EaglePatch is installed. The vanilla
`PadProxyPC::Update` also turned gamepad values into 0/1 (per the EaglePatch comment).

---

## 5. Definitive AC1 Parkour Mod — what its features say about the movement code

Cheat Engine `.CT` with Lua, applied at runtime, needs EaglePatch. It works on both Altaïr and
Ezio, which suggests AC2 kept the same parkour core. Its terms, mapped to ours as **hypotheses**
to check in IDA:

| Mod term | What the mod says it does | Probably maps to |
|---|---|---|
| **Prescriptive** vs **freeform** airtime | Normal jumps aim at a chosen landing target. "Force Freeform" turns the targeting off and jumps on a plain trajectory | RE/04 §4.1 jump target choice (takeoff/flight/reception), versus a free fall with no `CheckJumpTargetArrival` |
| Autograb / wall rebound in freeform | Disabled in freeform | Air-catch detectors (`CheckAirCatch` 0xE0BB70) |
| **NSO** ("NSO snap", "NSO state") | Snap to nearby objects during airtime. "Beam drop from any NSO state" | Probably "Narrow Surface Object" (beams, poles, perches). Compare RE/05 and the Narrow catch types in RE/04 §catch table |
| Parkour Down / Far / Down Low | Bias the target search lower, closer or further | Weights in the jump-target scoring |
| Force Hang / foot landing | Make the target a hang instead of a foot landing | Reception type choice |
| **Eject** / back eject, 90° angle snap, Fast Eject, perfect grabs | Ejects (jumping off a wall) snap to 90° directions. Instant eject only after a "perfect grab" | `TryBackEject` 0xDF2F50, RE/03 state 6 |
| **Staggered hang** / one-hand grab, recovery time | One-handed catch with a recovery delay | Catch variants (short/long, `fallHeight` 3 m), RE/04 |
| **Stumble** landing | Plays when landing on an edge at certain angles | Ground landing actions, RE/04 |
| Swing / freehang (feet not on geometry) | Swing only on certain object types | Pole/bar/freehang in RE/05 |
| H2H (hang-to-hang) | Automatic jumps between hangs | RE/03 ledge-jump table, side jumps |
| Kiosk dive, water dive, ledge "safety hang" | Dive into a kiosk, dive into water, auto-stop at a ledge (EdgeStop) | HayStack/hide contexts; EdgeStop, RE/02 |
| Eject anim mirroring snap | A bug in the vanilla left-facing eject animation | Animation mirror flag |
| Walk speeds | 3 manual walk speeds | Locomotion speed classes, RE/02 |

**Next step:** if the user downloads the `.CT` (Nexus needs a login, so they have to do it),
its addresses and AOB patterns would lead straight to the targeting and scoring functions.

---

## 6. Other gameplay/data findings

- **Rank gating:** catch ledge, and in "Permanent Rank 9", weapons and abilities, depend on the
  player's rank (0–9). The game stores ranks as separate "Rank N" files in `DataPC.forge`
  (RE/09). The Catch Ledge mod changes `.data` files only, so the ability flags live in data,
  probably a per-rank Entity/descriptor. The port should read them rather than hard-code
  "always catch". **(Open RE item)**
- **Combat:** entering combat forces the sword. This lives in the exe, and the No Sword Swap ASI
  patches it.
- **Max sync and health regen:** these are data values. Desynced sets max sync to 3 and turns
  off regen during combat.
- **AI/spawns:** soldier archetypes are entities in `DataPC_Common.forge` and can be swapped
  directly (Hardcore AC1). Swapping them can crash fast travel.
- **World layout and parkour markup:** these can be edited per city (Reworked mods, tree
  parkour, "City Horse Riding" / horses inside cities).
- **Blend/prayer animations:** beta prayer-blend animations are still in the data and can be
  re-enabled ("Beta Prayer Blending Animation").
- **Renderer:** AA is turned off above a resolution cap, and shadows have a resolution cap. The
  game runs badly with exclusive fullscreen and with 32+ CPU threads. Many fix mods use
  pattern scanning on DX9 and DX10.

---

## 7. Notable mods (Nexus, by endorsements)

| # | Mod | Author | Type | Why it matters to us |
|---|---|---|---|---|
| 1 | Hi-Res Texture Mod | A5phyxiati0n | forge textures | texture replacement pipeline |
| 32 | Animus Reforged | shazzaam | pack | curated "remaster" |
| 29 | **EaglePatchAC1** | Sergeanur | ASI, **open source** | input layer symbols (§4) |
| 81 | Lag Fix | J3r0m3 | patched exe | telemetry |
| 30 | **AnvilToolkit** | kamzik123 | tool | forge/data editing, XML conversion |
| 62 | **Definitive AC1 Parkour Mod** | HenryPDT | CE table | movement internals (§5) |
| 153 | SubtitleSynchAC1 | bloxtbc | ASI | dialogue/cutscene timing hooks |
| 55 | Camera and Controls Fix | Khemitude | DLL+ini | camera behaviour |
| 110 | Trainer | hex | trainer | player struct offsets |
| 175 | SpitePatch AC1 | NoisySC | ASI, pattern scan | windowing |
| 212 | Modern Fixes | nietzchivelli | ASI | menu/HUD/loader hooks, "keep weapon" |
| 126 | No Sword Swap | bloxtbc | ASI | combat weapon auto-equip |
| 116 | **Catch Ledge For All Ranks** | wajajan697 | forge data | rank-gated movement ability |
| 156 | **Jerusalem Reworked (Tree Parkour)** | wajajan697 | forge world | climbable markup is in data |
| 36 | Hardcore AC1 | LeafyyIsHere | forge data | NPC archetypes in Common forge |
| 177 | Desynced | TheButrAnvil | forge data | sync/health/markers/catch ledge |
| 107 | AC1's True Colors | Ev3rgr33n | forge (5.6 GB) | per-city colour grading in data |
| 119 | High Resolution Shadows | Ev3rgr33n | — | shadow cap |

The rest are mostly ReShade presets, outfits and texture swaps (many say "files for
AnvilToolkit"), translations built on SubtitleSynch, and intro video replacements.

**ModDB** has 9 older mods (texture packs, ENB colour correction, "Assassin's Breed", Complete
Remaster Project) and Turfster's `.forge` extractor/replacer in its downloads. It has nothing
technical that Nexus doesn't.

---

## 8. Modding plan (game-recon summary)

- **Install:** `C:\Users\benja\Desktop\Claude\Assassin's Creed\` (Steam, Director's Cut,
  v1.02 build 86610). The DX9 exe MD5 matches EaglePatch's supported build.
- **Engine:** Scimitar (Anvil, AC branch), native C++ x86, MSVC, full RTTI, reflection with
  CRC32 hashes.
- **Anti-cheat/online:** none. Only telemetry to a dead server. Safe.
- **Community route:** Ultimate ASI Loader + ASI plugins for code, AnvilToolkit for data.
- **Our route (unchanged):** a clean reimplementation in Rust/Bevy that loads the user's own
  forges (our own reader, RE/08). The community tools only serve as **reference and test
  rigs**:
  - read EaglePatch's source for input symbols;
  - optionally run EaglePatch with `AllocConsole`-style debugging, or write our own ASI to log
    movement state from the live game (ground truth for the port);
  - use AnvilToolkit to cross-check our forge/resource decoding.
- **Rules:** the working-copy exe in `bin/` stays unpatched. Any live testing with ASI plugins
  happens on a separate copy of the install, never the read-only original.

## 9. Open questions / follow-ups
1. Import the §4 names into the IDB (`PadProxyPC__Update` 0x909C90, etc.) after checking each
   one in IDA.
2. Find the per-rank ability flags (catch ledge) in the `DataPC.forge` "Rank N" files.
3. Get the Parkour Mod `.CT` (user download) and map its addresses to our function names.
4. Confirm what "NSO" stands for in the exe's strings and RTTI.
5. Check whether SubtitleSynchAC1 or Modern Fixes publish source (they hook dialogue and HUD).

## Sources
- Nexus Mods AC1: https://www.nexusmods.com/games/assassinscreed/mods (and mod pages #29, #30,
  #36, #55, #62, #81, #116, #126, #156, #175, #177, #212)
- ModDB: https://www.moddb.com/games/assassins-creed/mods ,
  https://www.moddb.com/groups/assassins-creed-fans/downloads/forge-extractorreplacer-by-turfster
- EaglePatch source: https://github.com/sergeanur/eaglepatch
- Ultimate ASI Loader: https://github.com/ThirteenAG/Ultimate-ASI-Loader
- AnvilToolkit wiki: https://github-wiki-see.page/m/Kamzik123/AnvilToolkit-Resources/wiki/Lesson-0
- REAC 2025 Anvil talk: https://enginearchitecture.org/downloads/REAC_2025_Anvil.pdf
- Ubisoft Anvil (Wikipedia): https://en.wikipedia.org/wiki/Ubisoft_Anvil
- AnvilNext forge format: https://github.com/Mischa-Alff/broadside/wiki/AnvilNext-%60.forge%60-file-format
- ACExplorer: https://github.com/gentlegiantJGC/ACExplorer
- XeNTaX AC PC forge thread: https://forum.xentax.com/viewtopic.php?f=10&t=3005
- Steam guide (deprecated): https://steamcommunity.com/sharedfiles/filedetails/?id=2288998586
