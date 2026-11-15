//! Which mood the cat is in and what it does next: plain rules over a
//! clock in seconds. No drawing, no sound, no system calls, so every rule
//! has a test.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::face::{self, Mood, Pose};
use super::{Kind, Prefs, Show};

/// What the Mac felt.
#[derive(Clone, Debug)]
pub enum Event {
    /// A hand lies on the trackpad. Sent a few times a second while it does.
    Hand,
    Slap,
    /// The battery's charge and whether the charger is in.
    Power { pct: u8, plugged: bool },
    /// The Mac woke from sleep.
    Wake,
    /// The lid was opened. On a charger or with a monitor the Mac stays
    /// awake under a closed lid, so this is not always a wake.
    LidOpen,
    /// The screen got locked or unlocked.
    Locked(bool),
    /// The pointer moved: how far the eyes turn to it and whether it is
    /// close enough to watch.
    Pointer { look: (f32, f32), near: bool },
    /// A link asked for a reaction.
    Sim(Kind),
    Prefs(Prefs),
}

/// What the cat does about it, besides changing its face.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    /// Play sound `n` (from 1) of the kind's sounds. The purr loops until `Hush`.
    Sound { kind: Kind, n: u8 },
    /// Let the purr fade out.
    Hush,
    /// The trackpad purrs under the hand, or stops.
    Buzz(bool),
    /// The big cat comes or goes.
    Big(bool),
    /// Remember the switches: the big cat showed a reaction for the first time.
    Keep(Prefs),
}

/// A hand has to lie this long before the purr starts (a four-finger
/// swipe is over sooner).
const HAND_HOLD: f64 = 0.6;
/// Two signs of a hand further apart than this are two hands.
const HAND_GAP: f64 = 0.6;
/// The purr goes on this long after the hand is gone.
const PURR_TAIL: f64 = 0.9;
/// A purr lasts at least one rub of the head.
const PURR_MIN: f64 = 1.6;
/// The cat is hungry at this charge.
const LOW: u8 = 10;
/// It can get hungry again once the charge was back up here.
const FED: u8 = 15;
/// After a wake, what happened to the charger during sleep is no news.
const WAKE_QUIET: f64 = 5.0;
/// The hello comes this long after the wake, or after the unlock.
const HELLO_AFTER_WAKE: f64 = 1.0;
const HELLO_AFTER_UNLOCK: f64 = 0.8;
/// One opening of the lid can be told twice (the wake, then the lid): no
/// second hello this soon after one.
const HELLO_GAP: f64 = 10.0;
/// The eyes close most of the way to the pointer in this time.
const LOOK_EASE: f64 = 0.07;
/// The eyes move in steps of this much of the 1024 box (a seventh of a
/// pixel in the menu bar: smaller moves can't be seen).
const LOOK_STEP: f32 = 4.0;
/// The resting clock starts over after this long, at a still moment (the
/// face takes its time as `f32`).
const REST_WRAP: f64 = 600.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Act {
    kind: Kind,
    since: f64,
    until: f64,
    /// A purr under a hand: it lasts while the hand stays.
    held: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hello {
    No,
    AfterUnlock,
    At(f64),
}

#[derive(Clone, Copy, PartialEq)]
enum Why {
    /// The Mac felt it.
    Felt,
    /// A hand on the trackpad.
    Hand,
    /// A link asked.
    Asked,
}

pub struct Engine {
    prefs: Prefs,
    act: Option<Act>,
    /// When the cat last came to rest: the clock its resting face runs on.
    rest: f64,
    /// The hand on the trackpad: since when, and its last sign.
    hand: Option<(f64, f64)>,
    power: Option<(u8, bool)>,
    /// It was fed since it last said it was hungry.
    fed: bool,
    quiet_until: f64,
    locked: bool,
    hello: Hello,
    /// When it last said hello.
    greeted: f64,
    look: (f32, f32),
    target: (f32, f32),
    near: bool,
    looked: f64,
    /// The sound each reaction played last.
    last: [u8; 5],
    rng: u32,
    big: bool,
}

impl Engine {
    pub fn new(prefs: Prefs, seed: u32) -> Engine {
        Engine {
            prefs,
            act: None,
            rest: 0.0,
            hand: None,
            power: None,
            fed: true,
            quiet_until: 0.0,
            locked: false,
            hello: Hello::No,
            greeted: f64::NEG_INFINITY,
            look: (0.0, 0.0),
            target: (0.0, 0.0),
            near: false,
            looked: 0.0,
            last: [0; 5],
            rng: seed | 1,
            big: false,
        }
    }

    /// Something happened at `now`.
    pub fn handle(&mut self, now: f64, ev: Event) -> Vec<Cmd> {
        let mut out = Vec::new();
        match ev {
            Event::Hand => {
                if !self.prefs.purr {
                    return out;
                }
                let since = match self.hand {
                    Some((since, last)) if now - last <= HAND_GAP => since,
                    _ => now,
                };
                self.hand = Some((since, now));
                match &mut self.act {
                    Some(a) if a.kind == Kind::Purr => {
                        if !a.held {
                            a.held = true;
                            out.push(Cmd::Buzz(true));
                        }
                        a.until = (now + PURR_TAIL).max(a.since + PURR_MIN);
                    }
                    // other reactions finish first
                    Some(_) => {}
                    None => {
                        if now - since >= HAND_HOLD {
                            self.begin(now, Kind::Purr, Why::Hand, &mut out);
                        }
                    }
                }
            }
            Event::Slap => {
                if self.prefs.hiss {
                    self.begin(now, Kind::Hiss, Why::Felt, &mut out);
                }
            }
            Event::Power { pct, plugged } => {
                let was = self.power.replace((pct, plugged));
                let quiet = now < self.quiet_until;
                match was {
                    // the first reading is no news either
                    None => self.fed = plugged || pct > LOW,
                    Some((_, was_plugged)) => {
                        if plugged || pct >= FED {
                            self.fed = true;
                        }
                        if quiet {
                            // it says so at the next reading after the quiet
                        } else if plugged && !was_plugged {
                            if self.prefs.eat {
                                self.begin(now, Kind::Eat, Why::Felt, &mut out);
                            }
                        } else if !plugged && pct <= LOW && self.fed {
                            self.fed = false;
                            if self.prefs.meow {
                                self.begin(now, Kind::Hungry, Why::Felt, &mut out);
                            }
                        }
                    }
                }
            }
            Event::Wake => {
                self.quiet_until = now + WAKE_QUIET;
                self.greet(now);
            }
            Event::LidOpen => self.greet(now),
            Event::Locked(locked) => {
                self.locked = locked;
                self.hello = match (self.hello, locked) {
                    (Hello::AfterUnlock, false) => Hello::At(now + HELLO_AFTER_UNLOCK),
                    // the lock came right after the wake
                    (Hello::At(_), true) => Hello::AfterUnlock,
                    (h, _) => h,
                };
            }
            Event::Pointer { look, near } => {
                if !self.watching() || !near {
                    // nothing to turn from: the eyes open where the pointer is
                    self.look = look;
                }
                self.target = look;
                self.near = near;
            }
            Event::Sim(kind) => {
                if self.prefs.awake() {
                    self.begin(now, kind, Why::Asked, &mut out);
                }
            }
            Event::Prefs(p) => {
                self.prefs = p;
                if !self.prefs.purr {
                    self.hand = None;
                }
                if !self.prefs.greet {
                    self.hello = Hello::No;
                }
                if self.prefs.show == Show::Off && self.big {
                    self.big = false;
                    out.push(Cmd::Big(false));
                }
            }
        }
        out
    }

    /// A hello is due: after the unlock if the screen is locked.
    fn greet(&mut self, now: f64) {
        if !self.prefs.greet || self.hello != Hello::No || now - self.greeted < HELLO_GAP {
            return;
        }
        self.hello = if self.locked { Hello::AfterUnlock } else { Hello::At(now + HELLO_AFTER_WAKE) };
    }

    /// What is due at `now`: a reaction ends, the hello comes, the eyes turn.
    pub fn tick(&mut self, now: f64) -> Vec<Cmd> {
        let mut out = Vec::new();
        if self.act.is_some_and(|a| now >= a.until) {
            self.end(now, &mut out);
        }
        if let Hello::At(t) = self.hello {
            if now >= t {
                self.hello = Hello::No;
                self.greeted = now;
                self.begin(now, Kind::Hello, Why::Felt, &mut out);
            }
        }
        let dt = (now - self.looked).clamp(0.0, 0.1);
        self.looked = now;
        if self.watching() {
            let k = (1.0 - (-dt / LOOK_EASE).exp()) as f32;
            self.look.0 += (self.target.0 - self.look.0) * k;
            self.look.1 += (self.target.1 - self.look.1) * k;
            if (self.target.0 - self.look.0).abs() < LOOK_STEP / 2.0 && (self.target.1 - self.look.1).abs() < LOOK_STEP / 2.0 {
                self.look = self.target;
            }
        } else {
            self.look = self.target;
        }
        // keep the resting clock small (see REST_WRAP)
        if self.act.is_none() && now - self.rest > REST_WRAP {
            let (mood, t) = self.mood(now);
            if face::still_for(mood, t).is_none_or(|s| s > 0.5) {
                self.rest = now;
            }
        }
        out
    }

    fn begin(&mut self, now: f64, kind: Kind, why: Why, out: &mut Vec<Cmd>) {
        if let Some(a) = self.act {
            if a.kind == Kind::Purr {
                out.push(Cmd::Hush);
                if a.held {
                    out.push(Cmd::Buzz(false));
                }
            }
        }
        let held = why == Why::Hand;
        let until = if held { now + PURR_TAIL.max(PURR_MIN) } else { now + kind.secs() };
        self.act = Some(Act { kind, since: now, until, held });
        out.push(Cmd::Sound { kind, n: self.pick(kind) });
        if held {
            out.push(Cmd::Buzz(true));
        }
        let slug = kind.slug();
        let big = match self.prefs.show {
            Show::Off => false,
            Show::Always => true,
            // asked for, it always shows: that is what the link is for
            Show::FirstTime => why == Why::Asked || !self.prefs.seen.iter().any(|s| s == slug),
        };
        if big && self.prefs.show == Show::FirstTime && why != Why::Asked {
            self.prefs.seen.push(slug.to_string());
            out.push(Cmd::Keep(self.prefs.clone()));
        }
        if big != self.big {
            self.big = big;
            out.push(Cmd::Big(big));
        }
    }

    fn end(&mut self, now: f64, out: &mut Vec<Cmd>) {
        if let Some(a) = self.act.take() {
            if a.kind == Kind::Purr {
                out.push(Cmd::Hush);
                if a.held {
                    out.push(Cmd::Buzz(false));
                }
            }
        }
        if self.big {
            self.big = false;
            out.push(Cmd::Big(false));
        }
        self.rest = now;
    }

    /// One of the kind's sounds, never the one it played last.
    fn pick(&mut self, kind: Kind) -> u8 {
        let (_, count) = kind.sounds();
        let slot = &mut self.last[Kind::ALL.iter().position(|k| *k == kind).unwrap_or(0)];
        if count <= 1 {
            *slot = 1;
            return 1;
        }
        // xorshift
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        let mut n = (self.rng % count as u32) as u8 + 1;
        if n == *slot {
            n = n % count + 1;
        }
        *slot = n;
        n
    }

    fn watching(&self) -> bool {
        self.act.is_none() && self.prefs.watch && self.near
    }

    /// The mood at `now` and how long it has lasted.
    fn mood(&self, now: f64) -> (Mood, f32) {
        match self.act {
            Some(a) => (a.kind.mood(), (now - a.since) as f32),
            None if self.watching() => (Mood::Watch, (now - self.rest) as f32),
            // switched off: the plain mark, no ear moves
            None if !self.prefs.awake() => (Mood::Idle, 0.0),
            None => (Mood::Idle, (now - self.rest) as f32),
        }
    }

    /// The cat's face at `now`.
    pub fn pose(&self, now: f64) -> Pose {
        let (mood, t) = self.mood(now);
        face::pose(mood, t, step(self.look))
    }

    /// Whether the big cat is on screen.
    pub fn big(&self) -> bool {
        self.big
    }

    /// Seconds from `now` until the cat next has something to do: 0 while
    /// it moves, None when only news can move it.
    pub fn sleep(&self, now: f64) -> Option<f64> {
        if self.act.is_none() && !self.prefs.awake() {
            return None;
        }
        let (mood, t) = self.mood(now);
        let mut s = face::still_for(mood, t).map(f64::from);
        let mut by = |d: f64| s = Some(s.map_or(d, |x| x.min(d)).max(0.0));
        if let Some(a) = self.act {
            by(a.until - now);
        }
        if let Hello::At(t) = self.hello {
            by(t - now);
        }
        if self.watching() && self.look != self.target {
            by(0.0);
        }
        if self.act.is_none() && now - self.rest > REST_WRAP {
            by(0.5);
        }
        s
    }
}

fn step(look: (f32, f32)) -> (f32, f32) {
    ((look.0 / LOOK_STEP).round() * LOOK_STEP, (look.1 / LOOK_STEP).round() * LOOK_STEP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Engine {
        Engine::new(Prefs::all_on(), 7)
    }

    /// A hand lying on the trackpad from `from` to `to`, a sign every 0.2 s.
    fn pet(e: &mut Engine, from: f64, to: f64) -> Vec<Cmd> {
        let mut out = Vec::new();
        for i in 0..=((to - from) / 0.2).round() as usize {
            let t = from + i as f64 * 0.2;
            out.extend(e.handle(t, Event::Hand));
            out.extend(e.tick(t));
        }
        out
    }

    fn sounds(cmds: &[Cmd]) -> Vec<Kind> {
        cmds.iter().filter_map(|c| if let Cmd::Sound { kind, .. } = c { Some(*kind) } else { None }).collect()
    }

    #[test]
    fn a_hand_that_stays_makes_it_purr() {
        let mut e = cat();
        let cmds = pet(&mut e, 1.0, 3.0);
        assert_eq!(sounds(&cmds), [Kind::Purr], "one purr, not one per sign");
        assert!(cmds.contains(&Cmd::Buzz(true)) && cmds.contains(&Cmd::Big(true)));
        assert_eq!(e.pose(3.0).mood, Mood::Purr);
        // the purr started once the hand had stayed long enough
        let since = e.act.unwrap().since;
        assert!((1.59..1.81).contains(&since), "{since}");
        // the hand leaves: it purrs a little longer, then stops
        assert!(e.tick(3.5).is_empty());
        let end = e.tick(3.95);
        assert_eq!(end, [Cmd::Hush, Cmd::Buzz(false), Cmd::Big(false)]);
        assert_eq!(e.pose(4.0).mood, Mood::Idle);
    }

    #[test]
    fn a_short_touch_is_not_petting() {
        let mut e = cat();
        assert!(pet(&mut e, 1.0, 1.4).is_empty());
        // a second short touch later does not add up with the first
        assert!(pet(&mut e, 3.0, 3.4).is_empty());
        assert_eq!(e.pose(3.5).mood, Mood::Idle);
    }

    #[test]
    fn the_big_cat_shows_each_reaction_once() {
        let mut e = cat();
        let first = e.handle(1.0, Event::Slap);
        assert!(first.contains(&Cmd::Big(true)));
        let kept = first.iter().find_map(|c| if let Cmd::Keep(p) = c { Some(p.clone()) } else { None }).unwrap();
        assert_eq!(kept.seen, ["hiss"]);
        assert_eq!(e.tick(2.6), [Cmd::Big(false)]);
        let again = e.handle(5.0, Event::Slap);
        assert_eq!(sounds(&again), [Kind::Hiss]);
        assert!(!again.contains(&Cmd::Big(true)), "seen already: only the menu bar reacts");
        // another reaction is still new
        e.tick(7.0);
        assert!(e.handle(8.0, Event::Sim(Kind::Eat)).contains(&Cmd::Big(true)));
    }

    #[test]
    fn show_always_and_off() {
        let mut e = Engine::new(Prefs { show: Show::Always, ..Prefs::all_on() }, 7);
        for t in [1.0, 5.0] {
            let c = e.handle(t, Event::Slap);
            assert!(c.contains(&Cmd::Big(true)) && !c.iter().any(|c| matches!(c, Cmd::Keep(_))));
            e.tick(t + 2.0);
        }
        let mut e = Engine::new(Prefs { show: Show::Off, ..Prefs::all_on() }, 7);
        assert_eq!(sounds(&e.handle(1.0, Event::Slap)), [Kind::Hiss]);
        assert!(!e.big());
        assert!(!e.handle(4.0, Event::Sim(Kind::Hello)).contains(&Cmd::Big(true)));
    }

    #[test]
    fn a_link_shows_the_big_cat_every_time() {
        let mut e = cat();
        for t in [1.0, 6.0, 12.0] {
            let c = e.handle(t, Event::Sim(Kind::Hello));
            assert!(c.contains(&Cmd::Big(true)) && !c.iter().any(|c| matches!(c, Cmd::Keep(_))));
            e.tick(t + 4.0);
        }
    }

    #[test]
    fn hungry_once_at_ten_percent() {
        let mut e = cat();
        let p = |pct, plugged| Event::Power { pct, plugged };
        assert!(e.handle(0.0, p(12, false)).is_empty());
        assert!(e.handle(60.0, p(11, false)).is_empty());
        assert_eq!(sounds(&e.handle(120.0, p(10, false))), [Kind::Hungry]);
        e.tick(125.0);
        assert!(e.handle(180.0, p(9, false)).is_empty(), "it said so already");
        // on the charger it eats, and can get hungry again
        assert_eq!(sounds(&e.handle(240.0, p(9, true))), [Kind::Eat]);
        e.tick(245.0);
        assert_eq!(sounds(&e.handle(300.0, p(9, false))), [Kind::Hungry]);
    }

    #[test]
    fn a_low_battery_at_launch_is_no_news() {
        let mut e = cat();
        assert!(e.handle(0.0, Event::Power { pct: 7, plugged: false }).is_empty());
        assert!(e.handle(60.0, Event::Power { pct: 6, plugged: false }).is_empty());
        // plugged in at launch: nothing to eat either
        let mut e = cat();
        assert!(e.handle(0.0, Event::Power { pct: 50, plugged: true }).is_empty());
    }

    #[test]
    fn hello_after_the_wake_or_the_unlock() {
        let mut e = cat();
        assert!(e.handle(12.5, Event::Wake).is_empty());
        assert_eq!(e.sleep(12.5), Some(1.0));
        assert!(e.tick(13.0).is_empty());
        assert_eq!(sounds(&e.tick(13.5)), [Kind::Hello]);
        e.tick(18.0);
        assert!(e.tick(25.0).is_empty(), "once per wake");

        // locked: it waits for the unlock
        let mut e = cat();
        e.handle(1.0, Event::Locked(true));
        e.handle(10.0, Event::Wake);
        assert!(e.tick(30.0).is_empty());
        e.handle(31.0, Event::Locked(false));
        assert!(e.tick(31.5).is_empty());
        assert_eq!(sounds(&e.tick(31.8)), [Kind::Hello]);

        // the lock comes just after the wake
        let mut e = cat();
        e.handle(10.0, Event::Wake);
        e.handle(10.2, Event::Locked(true));
        assert!(e.tick(12.0).is_empty());
        e.handle(20.0, Event::Locked(false));
        assert_eq!(sounds(&e.tick(20.8)), [Kind::Hello]);
    }

    #[test]
    fn hello_when_the_lid_opens_on_a_mac_that_stayed_awake() {
        let mut e = cat();
        assert!(e.handle(20.0, Event::LidOpen).is_empty());
        assert_eq!(sounds(&e.tick(21.0)), [Kind::Hello]);

        // the wake and the lid tell of the same opening: one hello
        let mut e = cat();
        e.handle(20.0, Event::Wake);
        assert_eq!(sounds(&e.tick(21.0)), [Kind::Hello]);
        e.handle(22.5, Event::LidOpen);
        e.tick(26.0);
        assert!(e.tick(40.0).is_empty());
        let mut e = cat();
        e.handle(20.0, Event::LidOpen);
        e.handle(20.4, Event::Wake);
        assert_eq!(sounds(&e.tick(21.0)), [Kind::Hello]);
        e.tick(26.0);
        assert!(e.tick(40.0).is_empty());

        // opened again later: hello again
        e.handle(60.0, Event::LidOpen);
        assert_eq!(sounds(&e.tick(61.0)), [Kind::Hello]);

        // switched off
        let mut e = Engine::new(Prefs { greet: false, ..Prefs::all_on() }, 7);
        e.handle(20.0, Event::LidOpen);
        assert!(e.tick(30.0).is_empty());
    }

    #[test]
    fn switched_off_the_cat_is_not_there() {
        let mut e = Engine::new(Prefs::default(), 7);
        assert_eq!(e.sleep(0.0), None);
        assert_eq!(e.sleep(500.0), None);
        // the plain mark at any time: no ear flicks
        assert_eq!(e.pose(40.0), e.pose(0.0));
        assert!(e.handle(1.0, Event::Sim(Kind::Hiss)).is_empty());
        assert!(e.handle(2.0, Event::Slap).is_empty());
        e.handle(3.0, Event::LidOpen);
        assert!(e.tick(10.0).is_empty());
        assert_eq!(e.sleep(10.0), None);
        // one switch on: it is there again
        e.handle(11.0, Event::Prefs(Prefs { hiss: true, ..Prefs::default() }));
        assert!(e.sleep(11.0).is_some());
        assert_eq!(sounds(&e.handle(12.0, Event::Sim(Kind::Purr))), [Kind::Purr]);
    }

    #[test]
    fn a_charger_put_in_during_sleep_is_no_news() {
        let mut e = cat();
        e.handle(0.0, Event::Power { pct: 40, plugged: false });
        e.handle(100.0, Event::Wake);
        assert!(e.handle(100.5, Event::Power { pct: 40, plugged: true }).is_empty());
        // hungry during the quiet: it says so at the next reading
        let mut e = cat();
        e.handle(0.0, Event::Power { pct: 30, plugged: false });
        e.handle(100.0, Event::Wake);
        assert!(e.handle(101.0, Event::Power { pct: 9, plugged: false }).is_empty());
        assert_eq!(sounds(&e.handle(200.0, Event::Power { pct: 8, plugged: false })), [Kind::Hungry]);
    }

    #[test]
    fn switched_off_reactions_stay_quiet_but_links_work() {
        // only the pointer is watched: every reaction is off
        let off = Prefs { watch: true, ..Prefs::default() };
        let mut e = Engine::new(off, 7);
        assert!(pet(&mut e, 1.0, 3.0).is_empty());
        assert!(e.handle(4.0, Event::Slap).is_empty());
        e.handle(5.0, Event::Power { pct: 50, plugged: false });
        assert!(e.handle(6.0, Event::Power { pct: 50, plugged: true }).is_empty());
        assert!(e.handle(7.0, Event::Power { pct: 5, plugged: false }).is_empty());
        e.handle(8.0, Event::Wake);
        assert!(e.tick(20.0).is_empty());
        assert_eq!(sounds(&e.handle(21.0, Event::Sim(Kind::Hiss))), [Kind::Hiss]);
    }

    #[test]
    fn a_new_reaction_takes_over() {
        let mut e = cat();
        pet(&mut e, 1.0, 2.0);
        assert_eq!(e.pose(2.0).mood, Mood::Purr);
        let c = e.handle(2.1, Event::Slap);
        assert_eq!(c[..2], [Cmd::Hush, Cmd::Buzz(false)], "the purr stops for the hiss");
        assert_eq!(e.pose(2.2).mood, Mood::Hiss);
        // the hand is still there: the purr comes back after the hiss
        let after = pet(&mut e, 2.2, 4.6);
        assert_eq!(sounds(&after), [Kind::Purr]);
    }

    #[test]
    fn the_same_sound_never_plays_twice_in_a_row() {
        let mut e = Engine::new(Prefs { show: Show::Off, ..Prefs::all_on() }, 12345);
        let mut last = 0;
        let mut heard = [false; 4];
        for i in 0..60 {
            let c = e.handle(i as f64 * 2.0, Event::Slap);
            let Cmd::Sound { n, .. } = c[0] else { panic!("no sound") };
            assert!((1..=3).contains(&n));
            assert_ne!(n, last);
            last = n;
            heard[n as usize] = true;
        }
        assert_eq!(heard, [false, true, true, true], "all three get their turn");
        // one sound only: that one
        let c = e.handle(500.0, Event::Sim(Kind::Hungry));
        assert_eq!(c[0], Cmd::Sound { kind: Kind::Hungry, n: 1 });
    }

    #[test]
    fn the_resting_cat_sleeps_until_its_ear_flicks() {
        let mut e = cat();
        e.tick(0.0);
        let s = e.sleep(0.0).unwrap();
        assert!((s - (3.0 + 0.96 * 36.0)).abs() < 1e-3, "{s}");
        assert_eq!(e.sleep(38.0), Some(0.0), "mid flick");
        // a reaction moves all the time, and ends on time
        e.handle(50.0, Event::Slap);
        assert_eq!(e.sleep(50.5), Some(0.0));
        assert_eq!(e.tick(51.5), [Cmd::Big(false)]);
        assert!(e.sleep(51.5).unwrap() > 1.0);
    }

    #[test]
    fn the_eyes_turn_to_the_pointer_only_when_asked_to() {
        let mut e = cat();
        e.handle(1.0, Event::Pointer { look: (120.0, 30.0), near: true });
        assert_eq!(e.pose(1.0).mood, Mood::Idle, "watching is off until switched on");
        e.handle(1.0, Event::Prefs(Prefs { watch: true, ..Prefs::all_on() }));
        e.tick(1.0);
        let p = e.pose(1.0);
        assert_eq!((p.mood, p.look), (Mood::Watch, (120.0, 32.0)), "the eyes open where the pointer is");
        // the pointer jumps: the eyes follow over a few frames, then rest
        e.handle(1.1, Event::Pointer { look: (-150.0, 0.0), near: true });
        assert_eq!(e.sleep(1.1), Some(0.0));
        let mut t = 1.1;
        let mut frames = 0;
        while e.sleep(t) == Some(0.0) {
            t += 1.0 / 30.0;
            e.tick(t);
            frames += 1;
            assert!(frames < 40, "the eyes never settle");
        }
        assert!(frames >= 3, "the eyes jumped");
        assert_eq!(e.pose(t).look, (-152.0, 0.0));
        // far from the menu bar it is the plain mark again
        e.handle(t, Event::Pointer { look: (0.0, 70.0), near: false });
        assert_eq!(e.pose(t).mood, Mood::Idle);
    }

    #[test]
    fn the_resting_clock_starts_over_at_a_still_moment() {
        let mut e = cat();
        let mut t = 0.0;
        while t < 700.0 {
            e.tick(t);
            t += 0.25;
        }
        assert!(t - e.rest < 620.0, "rest clock at {}", t - e.rest);
        assert!(e.sleep(t).is_some());
    }
}
