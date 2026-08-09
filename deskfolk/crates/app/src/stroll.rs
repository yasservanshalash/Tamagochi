//! Going somewhere.
//!
//! He used to occupy one spot until dragged. This gives him somewhere to *be*:
//! he notices the windows actually open, picks one, crosses to it, and sits on
//! its top edge — so where he is says something about what you are doing.
//!
//! The whole decision is kept here as a pure state machine over plain numbers,
//! with the desktop reading and the window moving on either side of it. That is
//! deliberate: "does he set off at the right moment, stop in the right place,
//! and give up when the ledge disappears" is the part that is easy to get
//! subtly wrong and impossible to check by looking at him for a few minutes.
//!
//! ## On the walk cycle
//!
//! [`Gait`] takes whichever the package provides. With a `walk` clip he walks;
//! without one he hops, keeping whatever pose he is in and travelling in an
//! arc, because sliding a static sprite flat across the screen reads as a bug
//! where an arc reads as intent.
//!
//! Playing the package's `jump` clip for the hop was tried and is wrong. Its
//! frames are the `img_y_big*` set, drawn at a visibly larger scale than the
//! seated idle — sitting shows him in a beanbag, so the person occupies much
//! less of the canvas — and he ballooned every time he moved.
//!
//! Walking needs two clips, not one: the renderer draws a sprite as it is and
//! cannot mirror at draw time, so `walk_right` is a second, pre-flipped set of
//! frames. Facing therefore has to come out of the state machine rather than
//! being inferred by the caller, which is what [`Facing`] on [`Step::Move`] is
//! for.

use std::time::Duration;

/// Where he is headed and how far along he is.
#[derive(Debug, Clone, PartialEq)]
pub enum Stroll {
    /// Sitting somewhere, with time to wait before wandering again.
    Resting { on: String, until_ms: i64 },
    /// Crossing to `target_x` on the ledge called `to`, ending at `target_y`.
    ///
    /// Position is carried in thousandths of a pixel so that speed can vary
    /// continuously without the rounding turning it back into a stutter, and
    /// so the state stays comparable — no floats in an enum that derives
    /// `PartialEq`.
    Walking {
        to: String,
        target_x: i32,
        /// The height he is heading for — the target ledge's top.
        target_y: i32,
        /// The height he set off from. A change of level between the two is a
        /// hop *onto* the new surface at the end of the crossing rather than a
        /// teleport to it the moment he sets off.
        from_y: i32,
        /// Where the journey began, which is what the ease-in is measured from.
        from_x: i32,
        x_milli: i64,
        /// This journey's pace as a percentage of his normal walk.
        pace: i32,
        /// Until he next stops to look at something.
        next_pause_ms: i64,
        /// How much of the current stop is left.
        pause_left_ms: i64,
    },
}

/// What the package can animate movement with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gait {
    /// A real walk cycle: he can move smoothly and continuously.
    Walk,
    /// No walk cycle, so he hops, keeping whatever pose he is already in and
    /// travelling in an arc.
    ///
    /// The obvious alternative was to play the package's `jump` clip, and it
    /// was wrong: those frames are the `img_y_big*` set, drawn at a visibly
    /// larger scale than the seated idle — he is in a beanbag when sitting,
    /// so the person occupies far less of the canvas. Switching clips mid-move
    /// made him balloon. The arc alone reads as a hop and keeps him one size.
    Hop,
}

/// Which way he is travelling, and therefore which way he should face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Left,
    Right,
}

impl Gait {
    /// The looping clip to hold while moving, if the package has one.
    ///
    /// Two clips rather than one because the renderer draws a sprite as it is
    /// and cannot flip at draw time, so the mirrored cycle is a second set of
    /// frames. `None` means "keep the pose you are in" — correct for hopping,
    /// where every candidate clip is the wrong scale.
    pub fn emotion(self, facing: Facing) -> Option<&'static str> {
        match (self, facing) {
            (Gait::Walk, Facing::Left) => Some("walk"),
            (Gait::Walk, Facing::Right) => Some("walk_right"),
            (Gait::Hop, _) => None,
        }
    }

    /// His flat-out pace, in thousandths of a pixel per millisecond.
    ///
    /// Same derivation as [`Gait::step`], expressed continuously so that
    /// easing and a per-journey pace can scale it without quantising the
    /// result back into a stutter.
    pub fn speed_mpms(self, height_px: i32) -> i64 {
        let (px, ms) = self.step(height_px);
        (px as i64 * 1000 / ms.max(1)).max(1)
    }

    /// How far one movement quantum carries him, and how long it takes.
    ///
    /// Derived from how tall he is on screen rather than tuned in pixels. A
    /// fixed pixel speed is wrong in two directions at once: on a dense
    /// display he is drawn larger and the same pixels-per-second reads as a
    /// shuffle, and on a coarse one as a sprint. Scaling by his own height
    /// keeps the *apparent* pace constant, and it is the only version that
    /// also keeps his feet from sliding, since the animation is a fixed
    /// number of frames per stride.
    ///
    /// At his drawn height of 289px that comes to about 223 px/s, and a walk
    /// cycle of 7 frames at 155ms covers 242px — two steps of 121px, which is
    /// a 0.73m stride for a 1.75m person. The art and the movement agree.
    pub fn step(self, height_px: i32) -> (i32, i64) {
        let px_per_m = height_px.max(1) as f32 / HUMAN_HEIGHT_M;
        let per_step: i64 = match self {
            Gait::Walk => 100,
            Gait::Hop => 320,
        };
        let speed = match self {
            Gait::Walk => WALK_SPEED_MS,
            // A hop covers more ground per beat because it happens less often.
            Gait::Hop => WALK_SPEED_MS * 0.8,
        };
        let px = (speed * px_per_m * per_step as f32 / 1000.0).round().max(1.0);
        (px as i32, per_step)
    }

    /// How high the arc peaks mid-step. Walking does not leave the ground.
    pub fn lift(self) -> f32 {
        match self {
            Gait::Walk => 0.0,
            Gait::Hop => 26.0,
        }
    }

    /// Where he is within one step: `t` runs 0 to 1.
    ///
    /// A half sine, so he leaves and lands softly instead of moving in a
    /// triangle — the difference between a hop and a bounce.
    pub fn arc(self, t: f32) -> i32 {
        (self.lift() * (t.clamp(0.0, 1.0) * std::f32::consts::PI).sin()).round() as i32
    }

    /// Pick from what the package actually provides.
    pub fn of(has_walk_clip: bool) -> Gait {
        if has_walk_clip { Gait::Walk } else { Gait::Hop }
    }
}

/// How tall he would be as a person. Everything about how fast he moves is
/// derived from this and his height on screen, so it stays right whatever DPI
/// or scale he happens to be drawn at.
const HUMAN_HEIGHT_M: f32 = 1.75;
/// An unhurried walking pace, in metres per second.
const WALK_SPEED_MS: f32 = 1.35;

/// How long he settles somewhere before thinking about moving again. Kept on
/// the livelier side now that he roams — an outing is several hops, and the gap
/// between outings is what would otherwise read as him being parked.
pub const REST_MIN: Duration = Duration::from_secs(30);
pub const REST_MAX: Duration = Duration::from_secs(100);

/// Close enough to the target to stop rather than shuffle the last few pixels.
const ARRIVED_WITHIN: i32 = 24;

// --- what keeps it from looking like a machine ------------------------------
//
// A constant speed in a straight line from A to B is the thing that reads as
// robotic, and no amount of animation quality hides it: he starts at full
// pace, holds it exactly, and stops dead. Three cheap corrections between
// them cover most of the difference.

/// Slowest he moves while easing off, as a percentage of his pace. Not zero:
/// a true ease to nothing means an endless crawl over the last few pixels.
const EASE_FLOOR: i64 = 30;
/// Pace varies this much per journey, so no two crossings are the same speed.
const PACE_SPREAD: i32 = 22;
/// He does not bother stopping to look around on a short hop.
const DAWDLE_IF_FURTHER_THAN: i32 = 260;
/// Range for how long he walks before stopping to look at something.
const DAWDLE_EVERY_MS: (i64, i64) = (1_400, 4_800);
/// Range for how long that stop lasts.
const DAWDLE_FOR_MS: (i64, i64) = (700, 2_400);

// --- hopping onto a surface at a different height ---------------------------
//
// He crosses to a window at another height by walking flat and then hopping
// onto it at the end, rather than snapping to the new height the instant he
// sets off. The hop happens entirely within the last `HOP_SPAN` pixels.

/// The last stretch of a crossing over which a change of level happens. Kept
/// below `DAWDLE_IF_FURTHER_THAN` so he never stops to look around mid-hop.
const HOP_SPAN: i32 = 220;
/// Base height the hop clears the edge by, before the drop or climb is added.
const HOP_CLEAR: f32 = 20.0;
/// Extra clearance per pixel of height change, so a big step up arcs higher
/// than a small one.
const HOP_SCALE: f32 = 0.12;

/// A small deterministic spread from a counter, in `0..n`.
///
/// The counter is a frame index, so using it directly makes every derived
/// value a near-linear function of time — the bug that had every walk target
/// landing on the same side of the screen.
fn scatter(seed: u32, salt: u32, n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let h = seed.wrapping_add(salt).wrapping_mul(2_654_435_761) >> 8;
    (h as i64) % n
}

fn in_range(seed: u32, salt: u32, range: (i64, i64)) -> i64 {
    range.0 + scatter(seed, salt, range.1 - range.0 + 1)
}

/// Speed multiplier for where he is along the journey, as a percentage.
///
/// Ramps up out of a standing start and back down into the destination, over
/// whichever is shorter: about one stride, or a third of the trip. Measured
/// against both ends so a short walk still eases at both.
///
/// One stride and not more: over his full height the ramp stops being a shape
/// and becomes a tax, dragging a long crossing down to half pace throughout.
fn ease(travelled: i32, remaining: i32, height_px: i32, total: i32) -> i64 {
    let ramp = (height_px / 2).min((total / 3).max(1)).max(1) as i64;
    let up = (travelled.max(0) as i64 * 100 / ramp).min(100);
    let down = (remaining.max(0) as i64 * 100 / ramp).min(100);
    let t = up.min(down);
    // Smoothstep, so the change of pace itself is not a sharp corner.
    let smooth = t * t * (300 - 2 * t) / 10_000;
    EASE_FLOOR + (100 - EASE_FLOOR) * smooth / 100
}

/// The base height and the extra hop arc for the current point in a crossing,
/// given how far he still has to go horizontally.
///
/// Flat (returns the departure height, no arc) until `remaining` drops under
/// [`HOP_SPAN`]; then the base height eases from where he set off to the target
/// while a half-sine arc lifts him clear of the edge, both reaching the target
/// exactly as he arrives (at [`ARRIVED_WITHIN`]). A same-height crossing
/// (`dy == 0`) is flat the whole way, so ordinary walking is untouched.
fn surface_hop(from_y: i32, target_y: i32, remaining: i32) -> (i32, i32) {
    let dy = target_y - from_y;
    if dy == 0 || remaining > HOP_SPAN {
        return (from_y, 0);
    }
    // Runs 0 at the start of the hop stretch to 1 by the time he arrives.
    let denom = (HOP_SPAN - ARRIVED_WITHIN).max(1) as f32;
    let q = (1.0 - (remaining - ARRIVED_WITHIN).max(0) as f32 / denom).clamp(0.0, 1.0);
    let smooth = q * q * (3.0 - 2.0 * q); // smoothstep
    let base = (from_y as f32 + dy as f32 * smooth).round() as i32;
    let peak = HOP_CLEAR + dy.unsigned_abs() as f32 * HOP_SCALE;
    let arc = (peak * (q * std::f32::consts::PI).sin()).round() as i32;
    (base, arc)
}

/// What the caller should do this tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Nothing to do.
    Stay,
    /// Put him here. `y` already includes the arc, so the caller does not
    /// need to know anything about gaits.
    Move { x: i32, y: i32, facing: Facing },
    /// He has stopped part-way, looking at something. Still on his way, so
    /// the caller should hold him where he is and drop the walk cycle.
    Dawdle { y: i32, facing: Facing },
    /// He got where he was going.
    Arrived { on: String },
}

impl Stroll {
    /// Start out sitting still.
    pub fn new(rest_ms: i64) -> Stroll {
        Stroll::Resting { on: "where he started".into(), until_ms: rest_ms }
    }

    pub fn is_walking(&self) -> bool {
        matches!(self, Stroll::Walking { .. })
    }

    /// Set off for a ledge, from wherever he is now.
    ///
    /// `seed` varies this journey from the last one — its pace, and when he
    /// first stops to look at something.
    pub fn walk_to(&mut self, to: String, from_x: i32, target_x: i32,
                   from_y: i32, target_y: i32, seed: u32) {
        *self = Stroll::Walking {
            to,
            target_x,
            target_y,
            from_y,
            from_x,
            x_milli: from_x as i64 * 1000,
            pace: 100 - PACE_SPREAD + scatter(seed, 11, PACE_SPREAD as i64 * 2 + 1) as i32,
            next_pause_ms: in_range(seed, 29, DAWDLE_EVERY_MS),
            pause_left_ms: 0,
        };
    }

    /// Advance by `dt`.
    ///
    /// `allowed` is false whenever he should not be wandering — asleep,
    /// talking, listening, being dragged. He stops where he is rather than
    /// resetting, so an interrupted walk resumes instead of starting over.
    pub fn tick(&mut self, dt: i64, gait: Gait, height_px: i32, allowed: bool) -> Step {
        match self {
            Stroll::Resting { until_ms, .. } => {
                *until_ms -= dt;
                Step::Stay
            }
            Stroll::Walking {
                to, target_x, target_y, from_y, from_x, x_milli, pace,
                next_pause_ms, pause_left_ms,
            } => {
                if !allowed {
                    return Step::Stay;
                }
                let x = (*x_milli / 1000) as i32;
                let remaining = *target_x - x;
                if remaining.abs() <= ARRIVED_WITHIN {
                    let on = std::mem::take(to);
                    let arrived = Step::Arrived { on: on.clone() };
                    *self = Stroll::Resting { on, until_ms: 0 };
                    return arrived;
                }
                let facing = if remaining < 0 { Facing::Left } else { Facing::Right };

                // Stopped to look at something. He is still on his way, and
                // still on the height he set off at — a dawdle only ever fires
                // far from the target, before the hop onto the new surface.
                if *pause_left_ms > 0 {
                    *pause_left_ms -= dt;
                    return Step::Dawdle { y: *from_y, facing };
                }
                *next_pause_ms -= dt;
                if *next_pause_ms <= 0 {
                    if remaining.abs() > DAWDLE_IF_FURTHER_THAN {
                        let seed = (*x_milli as u32) ^ (*target_x as u32);
                        *pause_left_ms = in_range(seed, 71, DAWDLE_FOR_MS);
                        *next_pause_ms = in_range(seed, 97, DAWDLE_EVERY_MS);
                        return Step::Dawdle { y: *from_y, facing };
                    }
                    // Too near the end to be worth stopping; do not ask again.
                    *next_pause_ms = i64::MAX / 2;
                }

                let total = (*target_x - *from_x).abs();
                let factor = ease((x - *from_x).abs(), remaining.abs(), height_px, total);
                let speed = gait.speed_mpms(height_px) * *pace as i64 / 100 * factor / 100;
                let travel = (speed * dt).max(1);
                let before = *x_milli;
                *x_milli += travel * remaining.signum() as i64;
                // Never sail past the target and walk back to it.
                if (*target_x as i64 * 1000 - *x_milli).signum()
                    != (*target_x as i64 * 1000 - before).signum()
                {
                    *x_milli = *target_x as i64 * 1000;
                }

                let now = (*x_milli / 1000) as i32;
                // Hop-gait characters leave the ground every stride; that arc
                // comes from how far through the current stride he is, so it
                // survives a varying speed instead of assuming a fixed beat.
                let stride_lift = if gait.lift() > 0.0 {
                    let stride = gait.step(height_px).0.max(1);
                    let phase = (now - *from_x).abs() % stride;
                    gait.arc(phase as f32 / stride as f32)
                } else {
                    0
                };
                // A change of level is a hop onto the new surface, not a
                // teleport to it: flat until the last stretch, then an arc up
                // (or down) that lands on the target ledge exactly on arrival.
                let (base_y, surface) = surface_hop(*from_y, *target_y, remaining.abs());
                Step::Move { x: now, y: base_y - stride_lift - surface, facing }
            }
        }
    }

    /// Is he done resting and ready to consider going somewhere?
    pub fn restless(&self) -> bool {
        matches!(self, Stroll::Resting { until_ms, .. } if *until_ms <= 0)
    }

    /// Settle for a while without going anywhere.
    pub fn rest(&mut self, on: String, for_ms: i64) {
        *self = Stroll::Resting { on, until_ms: for_ms };
    }
}

/// Somewhere on `ledge` worth walking to, given he is at `from`.
///
/// Clamping his current position into the ledge — the obvious thing, and what
/// this replaced — lands him exactly where he already stands whenever the
/// ledge is wide enough to contain him. The desktop floor always is. He set
/// off and arrived in the same frame, so he almost never walked, and when he
/// did the direction was whatever the one reachable ledge happened to be.
///
/// So: vary the spot along the ledge, and if the roll lands near where he is,
/// head for the far end instead. A journey shorter than his own width is not
/// a journey.
pub fn spot_on(ledge: &deskfolk_render_win::ledges::Ledge, from: i32, half_w: i32,
               roll: u32) -> i32 {
    let (lo, hi) = (ledge.left + half_w, ledge.right - half_w);
    if lo >= hi {
        // Narrower than he is: centre him and let him overhang evenly.
        return (ledge.left + ledge.right) / 2;
    }
    let span = hi - lo;
    // Hashed, not used directly: `roll` is a frame counter, so `roll % span`
    // makes the target a near-linear function of it — a small counter lands
    // near the left end every time, and he only ever walked one way.
    let scattered = roll.wrapping_mul(2_654_435_761) >> 8;
    let mut target = lo + (scattered % span as u32) as i32;
    let min_trip = (half_w * 2).max(120);
    if (target - from).abs() < min_trip {
        target = if from - lo > hi - from { lo } else { hi };
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskfolk_render_win::ledges::Ledge;

    /// His drawn height, as the walk sheet provides it.
    const TEST_H: i32 = 289;

    fn ledge(title: &str, left: i32, right: i32, top: i32) -> Ledge {
        Ledge { left, right, top, title: title.into() }
    }

    #[test]
    fn resting_counts_down_and_then_gets_restless() {
        let mut s = Stroll::new(100);
        assert!(!s.restless());
        assert_eq!(s.tick(60, Gait::Hop, TEST_H, true), Step::Stay);
        assert!(!s.restless());
        s.tick(60, Gait::Hop, TEST_H, true);
        assert!(s.restless(), "past its time: {s:?}");
    }

    fn walking(target_x: i32, y: i32, from_x: i32) -> Stroll {
        // A flat crossing: sets off and arrives at the same height.
        let mut s = Stroll::new(0);
        s.walk_to("Editor".into(), from_x, target_x, y, y, 3);
        s
    }

    /// Run a journey to completion, returning every position he passed
    /// through and how many frames he spent standing still on the way.
    fn journey(s: &mut Stroll, gait: Gait) -> (Vec<i32>, usize) {
        let (mut xs, mut paused) = (Vec::new(), 0);
        for _ in 0..4000 {
            match s.tick(16, gait, TEST_H, true) {
                Step::Move { x, .. } => xs.push(x),
                Step::Dawdle { .. } => paused += 1,
                Step::Arrived { .. } => break,
                Step::Stay => {}
            }
        }
        (xs, paused)
    }

    #[test]
    fn he_moves_every_frame_not_once_per_step() {
        // Teleporting a stride at a time is a twitch; continuous motion is
        // also what makes the easing visible at all.
        let mut s = walking(1000, 300, 100);
        let (xs, _) = journey(&mut s, Gait::Walk);
        assert!(xs.len() > 50, "too few frames to be smooth: {}", xs.len());
        assert!(xs.windows(2).all(|w| w[1] >= w[0]), "never goes backwards");
        assert!(*xs.last().unwrap() > 950, "got there: {:?}", xs.last());
    }

    #[test]
    fn he_eases_out_of_a_standing_start_and_into_the_target() {
        // A constant speed from A to B is the thing that reads as robotic.
        let mut s = walking(1600, 300, 100);
        let (xs, _) = journey(&mut s, Gait::Walk);
        let gap = |i: usize| (xs[i + 1] - xs[i]).abs();
        let early: i32 = (0..6).map(gap).sum();
        let middle: i32 = (xs.len() / 2..xs.len() / 2 + 6).map(gap).sum();
        let late: i32 = (xs.len() - 8..xs.len() - 2).map(gap).sum();
        assert!(early < middle, "no ease-in: {early} then {middle}");
        assert!(late < middle, "no ease-out: {middle} then {late}");
    }

    #[test]
    fn the_ease_never_stalls_him_completely() {
        // Easing all the way to zero means an endless crawl over the last few
        // pixels, which looks worse than starting abruptly.
        assert!(ease(0, 9999, 289, 9999) >= EASE_FLOOR);
        assert!(ease(9999, 0, 289, 9999) >= EASE_FLOOR);
        assert_eq!(ease(9999, 9999, 289, 9999), 100, "full pace in the middle");
    }

    #[test]
    fn he_stops_to_look_at_things_on_a_long_walk() {
        let mut s = walking(2000, 300, 0);
        let (_, paused) = journey(&mut s, Gait::Walk);
        assert!(paused > 0, "walked the whole way without once pausing");
    }

    #[test]
    fn he_does_not_dawdle_on_a_short_hop() {
        // Stopping to look around while crossing 200px reads as a stall, not
        // as character.
        let mut s = walking(200, 300, 0);
        let (_, paused) = journey(&mut s, Gait::Walk);
        assert_eq!(paused, 0, "paused on a walk not worth pausing in");
    }

    #[test]
    fn no_two_journeys_are_paced_the_same() {
        // Identical timing every trip is half of what reads as mechanical.
        let mut lens = std::collections::BTreeSet::new();
        for seed in 0..12u32 {
            let mut s = Stroll::new(0);
            s.walk_to("E".into(), 0, 1200, 10, 10, seed);
            lens.insert(journey(&mut s, Gait::Walk).0.len());
        }
        assert!(lens.len() > 4, "only {} distinct durations", lens.len());
    }

    #[test]
    fn a_pace_is_a_variation_not_a_lurch() {
        // Wide enough to notice, narrow enough that he never sprints.
        for seed in 0..200u32 {
            let mut s = Stroll::new(0);
            s.walk_to("E".into(), 0, 1200, 10, 10, seed);
            let Stroll::Walking { pace, .. } = &s else { panic!("not walking") };
            assert!((100 - PACE_SPREAD..=100 + PACE_SPREAD).contains(pace), "{pace}");
        }
    }

    #[test]
    fn a_hop_still_leaves_the_ground() {
        let mut s = walking(1200, 300, 0);
        let mut heights = Vec::new();
        for _ in 0..200 {
            if let Step::Move { y, .. } = s.tick(16, Gait::Hop, TEST_H, true) {
                heights.push(y);
            }
        }
        let peak = *heights.iter().min().expect("moved");
        assert!(peak < 300, "never left the ground: {peak}");
        assert!(300 - peak <= Gait::Hop.lift() as i32 + 1, "higher than the arc");
    }

    #[test]
    fn walking_keeps_his_feet_on_the_ground() {
        assert_eq!(Gait::Walk.arc(0.5), 0, "a walk cycle does not hop");
        let mut s = walking(1000, 300, 100);
        for _ in 0..40 {
            if let Step::Move { y, .. } = s.tick(16, Gait::Walk, TEST_H, true) {
                assert_eq!(y, 300);
            }
        }
    }

    #[test]
    fn a_change_of_level_lands_on_the_new_ledge_with_an_arc() {
        // Sets off low (y=300) and crosses to a ledge higher on screen (y=100).
        // He should stay flat for most of it, then hop up and land on the ledge
        // — not teleport his height the instant he sets off.
        let mut s = Stroll::new(0);
        s.walk_to("Up".into(), 0, 1200, 300, 100, 3);
        let mut ys = Vec::new();
        for _ in 0..5000 {
            match s.tick(16, Gait::Walk, TEST_H, true) {
                Step::Move { y, .. } => ys.push(y),
                Step::Arrived { .. } => break,
                _ => {}
            }
        }
        assert!(ys.len() > 20, "too short to judge: {}", ys.len());
        // Sets off at the departure height, not the target one.
        assert_eq!(ys[0], 300, "teleported his height at the start: {}", ys[0]);
        // Still flat well into the crossing (before the last HOP_SPAN pixels).
        assert_eq!(ys[ys.len() / 3], 300, "changed height too early");
        // Arcs above the target ledge mid-hop, then settles onto it.
        let peak = *ys.iter().min().unwrap();
        assert!(peak < 100, "no arc up over the edge: peak {peak}");
        let last = *ys.last().unwrap();
        assert!((last - 100).abs() <= ARRIVED_WITHIN, "did not land: {last}");
    }

    #[test]
    fn a_same_height_crossing_never_leaves_the_ground() {
        // The hop machinery must be invisible on a flat walk.
        let mut s = Stroll::new(0);
        s.walk_to("Flat".into(), 0, 1200, 200, 200, 3);
        for _ in 0..500 {
            match s.tick(16, Gait::Walk, TEST_H, true) {
                Step::Move { y, .. } => assert_eq!(y, 200, "a flat walk hopped"),
                Step::Arrived { .. } => break,
                _ => {}
            }
        }
    }

    #[test]
    fn dropping_to_a_lower_ledge_also_arcs_and_lands() {
        // The mirror case: set off high (y=100), cross to a lower ledge (y=340).
        let mut s = Stroll::new(0);
        s.walk_to("Down".into(), 0, 1200, 100, 340, 3);
        let mut ys = Vec::new();
        for _ in 0..5000 {
            match s.tick(16, Gait::Walk, TEST_H, true) {
                Step::Move { y, .. } => ys.push(y),
                Step::Arrived { .. } => break,
                _ => {}
            }
        }
        assert_eq!(ys[0], 100, "did not start at the departure height");
        let last = *ys.last().unwrap();
        assert!((last - 340).abs() <= ARRIVED_WITHIN, "did not land low: {last}");
        // A little lift off the top edge before the drop reads as a hop, not a
        // slide off the end.
        assert!(ys.iter().any(|&y| y < 100), "stepped off without a hop");
    }

    #[test]
    fn the_arc_peaks_in_the_middle_of_the_step() {
        let g = Gait::Hop;
        assert_eq!(g.arc(0.0), 0, "starts on the ground");
        assert_eq!(g.arc(1.0), 0, "lands on the ground");
        assert!(g.arc(0.5) > g.arc(0.2), "rising into the middle");
        assert!(g.arc(0.5) > g.arc(0.8), "falling out of it");
    }

    #[test]
    fn he_walks_left_as_readily_as_right() {
        let mut s = Stroll::new(0);
        s.walk_to("L".into(), 500, 0, 10, 10, 5);
        let Step::Move { x, .. } = s.tick(16, Gait::Walk, TEST_H, true) else {
            panic!("should move");
        };
        assert!(x < 500, "moved toward the target, not away: {x}");
    }

    #[test]
    fn he_never_overshoots_and_walks_back() {
        // With a varying speed the final frame can land past the target, and
        // then he turns round for it — a visible twitch at the end of a walk.
        for seed in 0..40u32 {
            let mut s = Stroll::new(0);
            s.walk_to("E".into(), 0, 700, 10, 10, seed);
            let (xs, _) = journey(&mut s, Gait::Walk);
            assert!(xs.iter().all(|&x| x <= 700), "overshot: {:?}", xs.iter().max());
        }
    }

    #[test]
    fn arriving_settles_him_and_reports_where() {
        let mut s = walking(100, 10, 90);
        assert_eq!(
            s.tick(16, Gait::Walk, TEST_H, true),
            Step::Arrived { on: "Editor".into() }
        );
        assert!(!s.is_walking());
        assert!(s.restless(), "ready to consider the next place");
    }

    #[test]
    fn an_interrupted_walk_holds_its_place_rather_than_restarting() {
        // Being spoken to mid-walk should not cost him the journey.
        let mut s = walking(900, 10, 100);
        for _ in 0..10 {
            s.tick(16, Gait::Walk, TEST_H, true);
        }
        assert_eq!(s.tick(16, Gait::Walk, TEST_H, false), Step::Stay);
        assert!(
            matches!(&s, Stroll::Walking { target_x, .. } if *target_x == 900),
            "still going there afterwards: {s:?}"
        );
        assert!(matches!(s.tick(16, Gait::Walk, TEST_H, true), Step::Move { .. }));
    }

    #[test]
    fn he_faces_the_way_he_is_going() {
        // The cycle as drawn faces one way, so the other needs the mirrored
        // clip. Getting this backwards makes him moonwalk everywhere.
        let mut right = walking(900, 10, 100);
        assert!(matches!(
            right.tick(16, Gait::Walk, TEST_H, true),
            Step::Move { facing: Facing::Right, .. }
        ));
        let mut left = Stroll::new(0);
        left.walk_to("L".into(), 900, 0, 10, 10, 2);
        assert!(matches!(
            left.tick(16, Gait::Walk, TEST_H, true),
            Step::Move { facing: Facing::Left, .. }
        ));
    }

    #[test]
    fn facing_picks_the_mirrored_clip_not_the_same_one() {
        // Both directions resolving to one clip is the failure that looks like
        // he is sliding backwards half the time.
        assert_ne!(Gait::Walk.emotion(Facing::Left), Gait::Walk.emotion(Facing::Right));
    }

    #[test]
    fn hopping_never_swaps_to_a_differently_scaled_clip() {
        // The regression: hopping played the package's `jump` clip, whose
        // frames are the `img_y_big*` set — drawn much larger than the seated
        // idle, so he ballooned every time he moved.
        assert_eq!(Gait::Hop.emotion(Facing::Left), None, "keeps the pose he is in");
        assert_eq!(Gait::Hop.emotion(Facing::Right), None);
        assert_eq!(Gait::Walk.emotion(Facing::Left), Some("walk"));
        assert_eq!(Gait::Walk.emotion(Facing::Right), Some("walk_right"));
        assert_eq!(Gait::of(true), Gait::Walk);
        assert_eq!(Gait::of(false), Gait::Hop);
    }

    #[test]
    fn a_target_is_never_where_he_already_is() {
        // The regression: clamping his position into a ledge that contains him
        // returns that same position, so he arrived on the frame he set off.
        let l = ledge("the desktop", 0, 1920, 1040);
        for roll in 0..200u32 {
            for from in [0, 400, 960, 1500, 1919] {
                let t = spot_on(&l, from, 73, roll);
                assert!((t - from).abs() >= 120, "{t} is no walk from {from}");
                assert!(t >= l.left && t <= l.right, "{t} left the ledge");
            }
        }
    }

    #[test]
    fn he_is_sent_both_ways_over_time() {
        // Direction follows the target, so a target generator that always
        // lands on one side means a companion that only ever walks one way.
        let l = ledge("the desktop", 0, 1920, 1040);
        let (mut left, mut right) = (0, 0);
        for roll in 0..200u32 {
            match spot_on(&l, 960, 73, roll) {
                t if t < 960 => left += 1,
                t if t > 960 => right += 1,
                _ => {}
            }
        }
        assert!(left > 20 && right > 20, "lopsided: {left} left, {right} right");
    }

    #[test]
    fn a_ledge_narrower_than_he_is_still_gets_a_target() {
        let l = ledge("tiny", 300, 400, 200);
        let t = spot_on(&l, 0, 200, 7);
        assert_eq!(t, 350, "centred, overhanging evenly");
    }
}
