//! Analysis probes over the user's install (ignored tests; run with `cargo test probe_ -- --ignored --nocapture`).
//! They print data used to derive port constants from the game's own animations.

use std::collections::HashMap;

use bevy::prelude::*;

use super::ac_anim::{bone_tracks, decode, TRACK_DISPLACEMENT};
use super::altair::load_altair;
use super::forge::Forge;
use super::game_dir;
use crate::anim::{sample_rot, sample_vec};

const CLASS_ANIMATION: u32 = 0x0FA3_067F;

fn game_fix() -> Vec<super::forge::Resource> {
    let mut f = Forge::open(&game_dir().join("DataPC.forge")).unwrap();
    let e = f.find("Game Fix").cloned().unwrap();
    f.resources(&e).unwrap()
}

#[test]
#[ignore]
fn probe_list_clips() {
    let pat: Vec<String> = std::env::var("PROBE_PAT").unwrap_or("hang|climb|fall|grab|catch|pull|ledge".into()).split('|').map(String::from).collect();
    let mut names: Vec<(String, f32)> = game_fix()
        .iter()
        .filter(|r| r.class_hash == CLASS_ANIMATION && pat.iter().any(|p| r.name.contains(p.as_str())))
        .map(|r| (r.name.clone(), decode(&r.payload).map(|a| a.duration).unwrap_or(-1.0)))
        .collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    for (n, d) in &names {
        println!("{n} {d:.3}");
    }
    println!("{} clips", names.len());
}

/// Limb positions in animation space (x right, y forward, z up; metres) at several times.
#[test]
#[ignore]
fn probe_clip_contacts() {
    let model = load_altair(&game_dir()).unwrap();
    let res = game_fix();
    let by_name: HashMap<&str, &[u8]> = res.iter().map(|r| (r.name.as_str(), r.payload.as_slice())).collect();
    let want: Vec<String> = std::env::var("PROBE_CLIPS")
        .unwrap_or("xx_h_hangwall_wait,xx_h_hangfree_wait,xx_climb_wait_1m,xx_climb_wait_2m".into())
        .split(',')
        .map(String::from)
        .collect();
    let named = [
        (0xb675_f36c_u32, "LHand"),
        (0x75f9_4d30, "RHand"),
        (0x5898_8870, "LFoot"),
        (0x9b14_362c, "RFoot"),
        (0xded1_0611, "Hips"),
        (0x07c1_59a2, "Head"),
        (0x2c52_cbb0, "Ref"),
        (0x7f11_dbe3, "LMid3"),
        (0xf069_fffc, "LIdx3"),
        (0xab4a_365e, "RMid3"),
    ];
    for name in &want {
        let Some(p) = by_name.get(name.as_str()) else {
            println!("{name}: not found");
            continue;
        };
        let a = decode(p).unwrap();
        let (rot, pos) = bone_tracks(&a);
        let rot: HashMap<u32, Vec<(f32, Quat)>> =
            rot.into_iter().map(|(k, v)| (k, v.into_iter().map(|(t, q)| (t, Quat::from_array(q).normalize())).collect())).collect();
        let pos: HashMap<u32, Vec<(f32, Vec3)>> = pos.into_iter().map(|(k, v)| (k, v.into_iter().map(|(t, p)| (t, Vec3::from_array(p))).collect())).collect();
        println!("== {name}  duration {:.3}", a.duration);
        let steps = std::env::var("PROBE_STEPS").ok().and_then(|s| s.parse().ok()).unwrap_or(4);
        for s in 0..=steps {
            let t = a.duration * s as f32 / steps as f32;
            let mut g: Vec<(Quat, Vec3)> = Vec::new();
            for b in &model.skeleton {
                let r = rot.get(&b.bone_id).map(|k| sample_rot(k, t)).unwrap_or(Quat::from_array(b.local_rot).normalize());
                let tr = pos.get(&b.bone_id).map(|k| sample_vec(k, t)).unwrap_or(Vec3::from_array(b.local_pos));
                let (pr, pp) = b.parent.map(|i| g[i]).unwrap_or((Quat::IDENTITY, Vec3::ZERO));
                g.push((pr * r, pp + pr * tr));
            }
            let disp = pos.get(&TRACK_DISPLACEMENT).map(|k| sample_vec(k, t)).unwrap_or(Vec3::ZERO);
            let mut line = format!("t={t:5.2} disp=({:5.2},{:5.2},{:5.2})", disp.x, disp.y, disp.z);
            for (id, label) in named {
                if let Some(i) = model.skeleton.iter().position(|b| b.bone_id == id) {
                    let p = g[i].1;
                    line += &format!(" {label}=({:5.2},{:5.2},{:5.2})", p.x, p.y, p.z);
                }
            }
            println!("{line}");
        }
    }
}

/// Which skeleton bones each clip animates (rotation tracks), to check finger coverage.
#[test]
#[ignore]
fn probe_track_coverage() {
    let model = load_altair(&game_dir()).unwrap();
    let names: HashMap<u32, String> = serde_free_names();
    let res = game_fix();
    let clips: Vec<String> = std::env::var("PROBE_CLIPS").unwrap_or("xx_h_hangwall_wait".into()).split(',').map(String::from).collect();
    for c in clips {
        let Some(r) = res.iter().find(|r| r.name == c) else { continue };
        let a = decode(&r.payload).unwrap();
        let (rot, pos) = bone_tracks(&a);
        let missing: Vec<String> = model
            .skeleton
            .iter()
            .filter(|b| !rot.contains_key(&b.bone_id))
            .map(|b| names.get(&b.bone_id).cloned().unwrap_or(format!("{:08x}", b.bone_id)))
            .collect();
        println!("{c}: {} rot tracks, {} pos tracks; not animated: {}", rot.len(), pos.len(), missing.join(" "));
    }
}

fn serde_free_names() -> HashMap<u32, String> {
    let txt = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../RE/data/altair_bone_names.json")).unwrap();
    txt.lines()
        .filter_map(|l| {
            let l = l.trim().trim_end_matches(',');
            let (k, v) = l.split_once(':')?;
            let k = u32::from_str_radix(k.trim().trim_matches('"'), 16).ok()?;
            Some((k, v.trim().trim_matches('"').to_string()))
        })
        .collect()
}
