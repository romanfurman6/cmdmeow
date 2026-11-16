//! The cat's skeleton, played live: the moves of the bought Spriter pack
//! (`assets/cat/rig/rig.json`, its parts in `parts/`), posed at any moment,
//! blended into each other, and bent by hand (a head that looks, a paw that
//! reaches). Pure maths, no AppKit.
//!
//! Angles are in degrees and y points up, as in the pack.

use serde::Deserialize;
use std::collections::HashMap;

pub const D2R: f64 = std::f64::consts::PI / 180.0;

/// One picture of a part: its size and pivot in pack units, its PNG.
#[derive(Debug, Clone, Deserialize)]
pub struct File {
    pub img: String,
    pub name: String,
    pub w: f64,
    pub h: f64,
    pub px: f64,
    pub py: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct Key {
    t: f64,
    x: f64,
    y: f64,
    a: f64,
    sx: f64,
    sy: f64,
    al: f64,
    #[serde(default = "one")]
    spin: i8,
    #[serde(default)]
    f: Option<[u16; 2]>,
    #[serde(default)]
    px: Option<f64>,
    #[serde(default)]
    py: Option<f64>,
}

fn one() -> i8 {
    1
}

#[derive(Debug, Clone, Deserialize)]
struct Line {
    name: String,
    keys: Vec<Key>,
}

/// A mainline key: which bones (id, parent, timeline, key) and objects
/// (parent, timeline, key, z) make up the body from time `t`.
#[derive(Debug, Clone, Deserialize)]
struct Mark {
    t: f64,
    bones: Vec<[i32; 4]>,
    objs: Vec<[i32; 4]>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawAnim {
    len: f64,
    #[serde(rename = "loop")]
    looped: bool,
    main: Vec<Mark>,
    tl: Vec<Option<Line>>,
}

#[derive(Debug, Deserialize)]
struct Raw {
    files: HashMap<String, File>,
    anims: HashMap<String, RawAnim>,
    black: String,
}

/// A move of the pack, its timelines named by `Rig::names` ids.
#[derive(Debug, Clone)]
pub struct Anim {
    pub len: f64,
    pub looped: bool,
    main: Vec<Mark>,
    /// timeline → (name id, keys)
    tl: Vec<Option<(usize, Vec<Key>)>>,
    /// timeline → name id
    tl_names: Vec<Option<usize>>,
}

/// The pose of one bone or part: where, turned how far, scaled, how opaque,
/// and for a part its picture and pivot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct St {
    pub x: f64,
    pub y: f64,
    pub a: f64,
    pub sx: f64,
    pub sy: f64,
    pub al: f64,
    pub f: Option<[u16; 2]>,
    pub px: Option<f64>,
    pub py: Option<f64>,
}

/// A whole body's local states (each in its parent's space) at one moment.
#[derive(Debug, Clone)]
pub struct Local<'a> {
    st: Vec<Option<St>>,
    mark: &'a Mark,
    /// the timelines of the move `mark` belongs to → name ids
    names: &'a [Option<usize>],
}

/// A part placed in the world: its pose and its name id.
#[derive(Debug, Clone, Copy)]
pub struct Obj {
    pub st: St,
    pub name: usize,
}

/// The body in the world: every bone by name id, the parts in draw order.
#[derive(Debug, Clone)]
pub struct World {
    pub bones: Vec<Option<St>>,
    pub objs: Vec<Obj>,
}

impl World {
    pub fn bone(&self, id: usize) -> Option<St> {
        self.bones.get(id).copied().flatten()
    }
}

pub struct Rig {
    pub files: HashMap<[u16; 2], File>,
    pub anims: HashMap<String, Anim>,
    /// The picture the pupils show (large and black, as on the sheets).
    pub black: [u16; 2],
    pub names: Vec<String>,
}

fn key_of(s: &str) -> Option<[u16; 2]> {
    let (a, b) = s.split_once(',')?;
    Some([a.trim().parse().ok()?, b.trim().parse().ok()?])
}

impl Rig {
    pub fn parse(json: &str) -> Result<Rig, String> {
        let raw: Raw = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut names: Vec<String> = Vec::new();
        let mut anims = HashMap::new();
        for (n, a) in raw.anims {
            let tl: Vec<Option<(usize, Vec<Key>)>> = a
                .tl
                .into_iter()
                .map(|l| {
                    l.map(|l| {
                        let id = names.iter().position(|x| *x == l.name).unwrap_or_else(|| {
                            names.push(l.name.clone());
                            names.len() - 1
                        });
                        (id, l.keys)
                    })
                })
                .collect();
            let tl_names = tl.iter().map(|l| l.as_ref().map(|(id, _)| *id)).collect();
            // the pack marks its one-way moves (sit down, lie down, get up) as loops
            let looped = a.looped && !(n.contains("trans") || n.contains("tranis"));
            anims.insert(n, Anim { len: a.len, looped, main: a.main, tl, tl_names });
        }
        let files = raw.files.into_iter().filter_map(|(k, f)| Some((key_of(&k)?, f))).collect();
        let black = key_of(&raw.black).ok_or("no black pupils")?;
        Ok(Rig { files, anims, black, names })
    }

    /// The id of a bone or part name (None: the pack has none by that name).
    pub fn id(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// The pose of move `name` at `time` ms (a move that does not loop
    /// holds its last moment).
    pub fn local(&self, name: &str, time: f64) -> Option<Local<'_>> {
        let anim = self.anims.get(name)?;
        let time = if anim.looped { time.rem_euclid(anim.len) } else { time.clamp(0.0, anim.len - 0.01) };
        let mut mark = anim.main.first()?;
        for m in &anim.main {
            if m.t <= time {
                mark = m;
            }
        }
        let mut st = vec![None; self.names.len()];
        let refs = mark.bones.iter().map(|b| (b[2], b[3])).chain(mark.objs.iter().map(|o| (o[1], o[2])));
        for (tl, key) in refs {
            let Some(Some((id, keys))) = anim.tl.get(tl as usize) else { continue };
            if let Some(s) = state(anim, keys, key as usize, time) {
                st[*id] = Some(s);
            }
        }
        Some(Local { st, mark, names: &anim.tl_names })
    }
}

/// One timeline at `time`, from key `kid` towards the next (or the first
/// again, for a loop).
fn state(anim: &Anim, keys: &[Key], kid: usize, time: f64) -> Option<St> {
    let k = keys.get(kid)?;
    let (kb, tb) = if kid + 1 < keys.len() {
        (&keys[kid + 1], keys[kid + 1].t)
    } else if anim.looped && keys.len() > 1 {
        (&keys[0], anim.len + keys[0].t)
    } else {
        return Some(St { x: k.x, y: k.y, a: k.a, sx: k.sx, sy: k.sy, al: k.al, f: k.f, px: k.px, py: k.py });
    };
    let t = if tb == k.t { 0.0 } else { ((time - k.t) / (tb - k.t)).clamp(0.0, 1.0) };
    let (a, mut b) = (k.a, kb.a);
    match k.spin {
        1 if b < a => b += 360.0,
        -1 if b > a => b -= 360.0,
        0 => b = a,
        _ => {}
    }
    let mix = |p: f64, q: f64| p + (q - p) * t;
    Some(St { x: mix(k.x, kb.x), y: mix(k.y, kb.y), a: mix(a, b), sx: mix(k.sx, kb.sx), sy: mix(k.sy, kb.sy), al: mix(k.al, kb.al), f: k.f, px: k.px, py: k.py })
}

fn mix_angle(a: f64, b: f64, w: f64) -> f64 {
    let d = (b - a + 540.0).rem_euclid(360.0) - 180.0;
    a + d * w
}

/// `b` weighed by `w` over `a`.
pub fn blend<'a>(a: &Local<'a>, b: &Local<'a>, w: f64) -> Local<'a> {
    if w >= 1.0 {
        return b.clone();
    }
    if w <= 0.0 {
        return a.clone();
    }
    let st = b
        .st
        .iter()
        .zip(&a.st)
        .map(|(q, p)| match (p, q) {
            (None, None) => None,
            (Some(p), None) => Some(*p),
            (None, Some(q)) => Some(*q),
            (Some(p), Some(q)) => {
                let base = if w < 0.5 { p } else { q };
                let mix = |p: f64, q: f64| p + (q - p) * w;
                Some(St { x: mix(p.x, q.x), y: mix(p.y, q.y), a: mix_angle(p.a, q.a, w), sx: mix(p.sx, q.sx), sy: mix(p.sy, q.sy), al: mix(p.al, q.al), ..*base })
            }
        })
        .collect();
    let (mark, names) = if w < 0.5 { (a.mark, a.names) } else { (b.mark, b.names) };
    Local { st, mark, names }
}

fn parent(c: St, p: &St) -> St {
    let (px, py) = (c.x * p.sx, c.y * p.sy);
    let (sn, cs) = (p.a * D2R).sin_cos();
    let a = if p.sx * p.sy < 0.0 { 360.0 - c.a } else { c.a };
    St { x: px * cs - py * sn + p.x, y: px * sn + py * cs + p.y, a: a + p.a, sx: c.sx * p.sx, sy: c.sy * p.sy, al: c.al * p.al, ..c }
}

/// The body in the world; `fix(name id, state)` bends a local state first.
pub fn world(l: &Local, fix: &dyn Fn(usize, St) -> St) -> World {
    let mut by_id: HashMap<i32, St> = HashMap::new();
    let mut bones = vec![None; l.st.len()];
    for b in &l.mark.bones {
        let Some(name) = slot_of(l, b[2]) else { continue };
        let Some(s) = l.st[name] else { continue };
        let s = fix(name, s);
        let s = if b[1] >= 0 {
            match by_id.get(&b[1]) {
                Some(p) => parent(s, p),
                None => continue,
            }
        } else {
            s
        };
        by_id.insert(b[0], s);
        bones[name] = Some(s);
    }
    let mut refs: Vec<&[i32; 4]> = l.mark.objs.iter().collect();
    refs.sort_by_key(|o| o[3]);
    let mut objs = Vec::with_capacity(refs.len());
    for o in refs {
        let Some(name) = slot_of(l, o[1]) else { continue };
        let Some(s) = l.st[name] else { continue };
        let s = fix(name, s);
        let s = if o[0] >= 0 {
            match by_id.get(&o[0]) {
                Some(p) => parent(s, p),
                None => continue,
            }
        } else {
            s
        };
        objs.push(Obj { st: s, name });
    }
    World { bones, objs }
}

/// The name id of timeline `tl` of the move the mainline key came from.
fn slot_of(l: &Local, tl: i32) -> Option<usize> {
    l.names.get(usize::try_from(tl).ok()?).copied().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> Rig {
        let json = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/cat/rig/rig.json")).expect("rig.json");
        Rig::parse(&json).expect("parses")
    }

    // the shipped numbers are rounded to 3 places: half a unit is 0.05 pt on screen
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.5
    }

    // poses taken from the browser prototype the skeleton was first played in
    #[test]
    fn poses_match_the_prototype() {
        let r = rig();
        let id = |n: &str| r.id(n).expect(n);
        let check = |anim: &str, t: f64, bone: &str, x: f64, y: f64, a: f64| {
            let w = world(&r.local(anim, t).expect(anim), &|_, s| s);
            let b = w.bone(id(bone)).expect(bone);
            assert!(near(b.x, x) && near(b.y, y) && near(b.a, a), "{anim} {t} {bone}: {b:?}");
        };
        check("idle", 0.0, "bone_011", -299.46, 785.6, 811.58);
        check("idle", 733.0, "bone_022", -97.63, 128.89, 959.36);
        check("walk", 333.0, "bone_022", 11.23, 118.73, 1007.45);
        check("sitting", 1500.0, "bone_007", 152.04, 337.48, 134.71);
        check("run", 100.0, "bone_022", -290.1, 251.27, 648.01);
        let w = world(&r.local("idle", 0.0).unwrap(), &|_, s| s);
        assert_eq!(w.objs.len(), 33);
        assert_eq!(r.names[w.objs[0].name], "back_leg_top_mid");
        assert!(near(w.objs[0].st.x, -150.23));
    }

    #[test]
    fn a_blend_lies_between_two_moves() {
        let r = rig();
        let (a, b) = (r.local("idle", 200.0).unwrap(), r.local("walk", 100.0).unwrap());
        let w = world(&blend(&a, &b, 0.4), &|_, s| s);
        let h = w.bone(r.id("bone_011").unwrap()).unwrap();
        assert!(near(h.x, -297.71) && near(h.y, 788.0) && near(h.a, 811.56), "{h:?}");
    }

    #[test]
    fn a_move_that_does_not_loop_holds_its_end() {
        let r = rig();
        let len = r.anims["sit_tranistion_to"].len;
        let end = |t| world(&r.local("sit_tranistion_to", t).unwrap(), &|_, s| s).bone(r.id("bone_007").unwrap()).unwrap();
        assert!(!r.anims["sit_tranistion_to"].looped);
        assert_eq!(end(len * 3.0), end(len + 10.0));
    }

    #[test]
    fn every_part_has_its_picture() {
        let r = rig();
        for a in r.anims.values() {
            for l in a.tl.iter().flatten() {
                for k in &l.1 {
                    if let Some(f) = k.f {
                        // the pack hides a few parts the cat does not wear (collars, spots)
                        let _ = r.files.get(&f);
                    }
                }
            }
        }
        assert!(r.files.contains_key(&r.black));
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/cat/rig/parts/");
        for f in r.files.values() {
            assert!(std::path::Path::new(&format!("{dir}{}", f.img)).exists(), "{}", f.img);
        }
    }
}
