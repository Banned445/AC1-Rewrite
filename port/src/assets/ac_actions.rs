//! Animation graph decoder: `ActionBlock` resources (DataPC.forge "Game Fix"), RE/13.
//!
//! Port of RE/tools/ac_actions.py. Every read mirrors a serializer in the exe:
//! ActionBlock 0x6E9BF0, Action 0x507EE0, ActionItem 0x5BADF0, ActionTransition 0x564DB0,
//! ActionBlend 0x5BCDE0, AssociatedActionGroup 0x6D8D20. Stream helpers: object field 0x931780
//! (u8 flag: 0 inline {id, class, fields} / 2 ref id / 3 null), handle field 0x9311A0 (also 1 = ref),
//! typed reference 0x931410 (u32 id), embedded object 0x438EF0 ({id, class, fields}), handle 0x433D80
//! (u32 id), pod array 0x930B10 (u32 count + elements).

use std::collections::HashMap;

pub const CLASS_ACTION_BLOCK: u32 = 0xEF82_FCE4;
const CLASS_ACTION: u32 = 0x4060_89A4;
const CLASS_ACTION_ITEM: u32 = 0x80E5_0E4A;
const CLASS_TRANSITION: u32 = 0x46ED_6DF7;
const CLASS_BLEND: u32 = 0xC704_1AEE;
const CLASS_ASSOC_GROUP: u32 = 0x1E6D_DBE2;

/// `ACTDisplacementMode`: who moves the root while the item plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DisplacementMode {
    #[default]
    FromAnim,
    FromPhysics,
    FromAi,
}

#[allow(dead_code)] // decoded for the remaining graph work (RE/13)
#[derive(Clone, Debug, Default)]
pub struct ActBlend {
    /// `ACTBlendType` (0 NONE, 1 AROLLBROLL, …).
    pub kind: u8,
    /// Blend duration (s).
    pub time: f32,
}

#[allow(dead_code)] // decoded for the remaining graph work (RE/13)
#[derive(Clone, Debug, Default)]
pub struct ActItem {
    pub id: u32,
    /// Animation resource ids, blended with `weights` (the authored default weights).
    pub animations: Vec<u32>,
    pub weights: Vec<f32>,
    pub displacement: DisplacementMode,
    pub blend: ActBlend,
    /// Outgoing transitions: (transition action, destination action).
    pub transitions: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Default)]
pub struct Action {
    pub id: u32,
    pub block: String,
    pub items: Vec<ActItem>,
}

/// All decoded actions, by action id (the ids the exe passes to the animation graph).
#[derive(Default, Debug)]
pub struct ActionGraph {
    pub actions: HashMap<u32, Action>,
}

struct R<'a> {
    b: &'a [u8],
    o: usize,
}

impl R<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.o).ok_or("overrun")?;
        self.o += 1;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.b.get(self.o..self.o + 4).ok_or("overrun")?;
        self.o += 4;
        Ok(u32::from_le_bytes(s.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }
}

#[allow(dead_code)]
enum Obj {
    None,
    Ref(u32),
    Action(Action),
    Item(ActItem),
    Transition((u32, u32)),
    Other,
}

struct Ctx {
    block: String,
    items: HashMap<u32, ActItem>,
    actions: Vec<Action>,
}

fn read_obj(r: &mut R, c: &mut Ctx, handle: bool) -> Result<Obj, String> {
    match r.u8()? {
        3 => Ok(Obj::None),
        2 => Ok(Obj::Ref(r.u32()?)),
        1 if handle => Ok(Obj::Ref(r.u32()?)),
        0 => {
            let id = r.u32()?;
            let cls = r.u32()?;
            read_body(r, c, id, cls)
        }
        f => Err(format!("bad object flag {f} at {:#x}", r.o - 1)),
    }
}

fn read_blend(r: &mut R) -> Result<ActBlend, String> {
    let kind = (r.u32()? & 7) as u8;
    for _ in 0..3 {
        r.u8()?;
    }
    for _ in 0..3 {
        r.u32()?;
    }
    r.u8()?;
    let time = r.f32()?;
    r.f32()?;
    r.f32()?;
    r.u8()?;
    // optional ActionBlendFrankenstein: never present in the shipped data (RE/13)
    match r.u8()? {
        3 => {}
        2 => {
            r.u32()?;
        }
        f => return Err(format!("unsupported ActionBlendFrankenstein (flag {f})")),
    }
    Ok(ActBlend { kind, time })
}

fn read_body(r: &mut R, c: &mut Ctx, id: u32, cls: u32) -> Result<Obj, String> {
    match cls {
        CLASS_ACTION => {
            let _action_id = r.u32()?;
            read_obj(r, c, true)?; // BodyPartChannel
            read_obj(r, c, false)?; // transition A
            read_obj(r, c, false)?; // transition B
            for _ in 0..5 {
                r.u8()?;
            }
            for _ in 0..3 {
                r.u32()?;
            }
            read_obj(r, c, false)?; // AssociatedActionGroup
            let n = r.u32()?;
            let mut items = Vec::new();
            for _ in 0..n {
                match read_obj(r, c, false)? {
                    Obj::Item(it) => items.push(it),
                    Obj::Ref(rid) => {
                        if let Some(it) = c.items.get(&rid) {
                            items.push(it.clone());
                        }
                    }
                    _ => {}
                }
            }
            let a = Action { id, block: c.block.clone(), items };
            c.actions.push(a.clone());
            Ok(Obj::Action(a))
        }
        CLASS_ACTION_ITEM => {
            let n = r.u32()?;
            let animations = (0..n).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
            let n = r.u32()?;
            let mut transitions = Vec::new();
            for _ in 0..n {
                if let Obj::Transition(t) = read_obj(r, c, false)? {
                    transitions.push(t);
                }
            }
            let (bid, bcls) = (r.u32()?, r.u32()?);
            let _ = (bid, bcls);
            let blend = read_blend(r)?;
            let disp = r.u32()?;
            r.u32()?;
            r.u32()?;
            for _ in 0..12 {
                r.u8()?;
            }
            r.f32()?;
            r.u32()?;
            r.u8()?;
            let n = r.u32()?;
            let weights = (0..n).map(|_| r.f32()).collect::<Result<Vec<_>, _>>()?;
            let it = ActItem {
                id,
                animations,
                weights,
                displacement: match disp {
                    1 => DisplacementMode::FromPhysics,
                    2 => DisplacementMode::FromAi,
                    _ => DisplacementMode::FromAnim,
                },
                blend,
                transitions,
            };
            c.items.insert(id, it.clone());
            Ok(Obj::Item(it))
        }
        CLASS_TRANSITION => {
            let (_, _) = (r.u32()?, r.u32()?);
            read_blend(r)?;
            let a = r.u32()?;
            r.u32()?;
            let (_, _) = (r.u32()?, r.u32()?);
            read_blend(r)?;
            let b = r.u32()?;
            r.u32()?;
            Ok(Obj::Transition((a, b)))
        }
        CLASS_BLEND => {
            read_blend(r)?;
            Ok(Obj::Other)
        }
        CLASS_ASSOC_GROUP => {
            let n = r.u32()?;
            for _ in 0..n {
                for _ in 0..5 {
                    r.u32()?;
                }
            }
            r.f32()?;
            for _ in 0..3 {
                r.u8()?;
            }
            Ok(Obj::Other)
        }
        CLASS_ACTION_BLOCK => {
            let n = r.u32()?;
            for _ in 0..n {
                read_obj(r, c, true)?;
            }
            r.u32()?; // BodyPartTemplate reference
            read_obj(r, c, false)?;
            Ok(Obj::Other)
        }
        _ => Err(format!("no reader for class {cls:08x} at {:#x}", r.o)),
    }
}

/// Decode one ActionBlock payload; returns its actions. Errors if any byte is left over.
pub fn parse_block(name: &str, payload: &[u8]) -> Result<Vec<Action>, String> {
    let mut r = R { b: payload, o: 0 };
    let id = r.u32()?;
    let cls = r.u32()?;
    let mut c = Ctx { block: name.to_string(), items: HashMap::new(), actions: Vec::new() };
    read_body(&mut r, &mut c, id, cls)?;
    if r.o != payload.len() {
        return Err(format!("{name}: parsed {} of {} bytes", r.o, payload.len()));
    }
    Ok(c.actions)
}

impl ActionGraph {
    pub fn add_block(&mut self, name: &str, payload: &[u8]) -> Result<usize, String> {
        let acts = parse_block(name, payload)?;
        let n = acts.len();
        for a in acts {
            self.actions.insert(a.id, a);
        }
        Ok(n)
    }
}
