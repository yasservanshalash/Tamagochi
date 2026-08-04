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
//! There is no walk animation in the package — every sprite is a seated or
//! standing pose — so crossing the screen is done as a series of hops: he keeps
//! whatever pose he is in and travels in an arc, which reads as deliberate
//! movement where sliding a static sprite would not.
//!
//! Playing the package's `jump` clip for that was tried and is wrong. Its
//! frames are the `img_y_big*` set, drawn at a visibly larger scale than the
//! seated idle — sitting shows him in a beanbag, so the person occupies much
//! less of the canvas — and he ballooned every time he moved. The arc alone
//! does the job and keeps him one size.
//!
//! [`Gait`] picks whichever the package actually has, so adding a `walk` clip
//! later switches him to walking and changes nothing else.

use std::time::Duration;

/// Where he is headed and how far along he is.
#[derive(Debug, Clone, PartialEq)]
pub enum Stroll {
    /// Sitting somewhere, with time to wait before wandering again.
    Resting { on: String, until_ms: i64 },
    /// Crossing to `target_x`, at `y`, on the ledge called `to`.
    ///
    /// `from_x` is where the current step began and `phase_ms` how far into it
    /// he is, so movement is interpolated every frame rather than teleporting
    /// once per step. That is what makes a hop an arc instead of a twitch.
    Walking { to: String, target_x: i32, y: i32, from_x: i32, phase_ms: i64 },
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

impl Gait {
    /// The looping clip to hold while moving, if the package has one.
    ///
    /// `None` means "keep the pose you are in" — correct for hopping, where
    /// every candidate clip is the wrong scale.
    pub fn emotion(self) -> Option<&'static str> {
        match self {
            Gait::Walk => Some("walk"),
            Gait::Hop => None,
        }
    }

    /// How far one step carries him, and how long it takes.
    ///
    /// A hop covers more ground because it happens less often; both come out
    /// at a similar crossing speed.
    pub fn step(self) -> (i32, i64) {
        match self {
            Gait::Walk => (14, 80),
            Gait::Hop => (46, 320),
        }
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

/// How long he settles somewhere before thinking about moving again.
pub const REST_MIN: Duration = Duration::from_secs(45);
pub const REST_MAX: Duration = Duration::from_secs(150);

/// Close enough to the target to stop rather than shuffle the last few pixels.
const ARRIVED_WITHIN: i32 = 24;

/// What the caller should do this tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Nothing to do.
    Stay,
    /// Put him here. `y` already includes the arc, so the caller does not
    /// need to know anything about gaits.
    Move { x: i32, y: i32 },
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
    pub fn walk_to(&mut self, to: String, from_x: i32, target_x: i32, y: i32) {
        *self = Stroll::Walking { to, target_x, y, from_x, phase_ms: 0 };
    }

    /// Advance by `dt`.
    ///
    /// `allowed` is false whenever he should not be wandering — asleep,
    /// talking, listening, being dragged. He stops where he is rather than
    /// resetting, so an interrupted walk resumes instead of starting over.
    pub fn tick(&mut self, dt: i64, gait: Gait, allowed: bool) -> Step {
        match self {
            Stroll::Resting { until_ms, .. } => {
                *until_ms -= dt;
                Step::Stay
            }
            Stroll::Walking { to, target_x, y, from_x, phase_ms } => {
                if !allowed {
                    return Step::Stay;
                }
                let (stride, per_step) = gait.step();
                if (*target_x - *from_x).abs() <= ARRIVED_WITHIN {
                    let on = std::mem::take(to);
                    let arrived = Step::Arrived { on: on.clone() };
                    *self = Stroll::Resting { on, until_ms: 0 };
                    return arrived;
                }
                *phase_ms += dt;
                let remaining = *target_x - *from_x;
                // Never overshoot: a final stride longer than what is left
                // would land him past the target and walking back.
                let leg = stride.min(remaining.abs()) * remaining.signum();
                let t = (*phase_ms as f32 / per_step as f32).clamp(0.0, 1.0);

                if *phase_ms >= per_step {
                    // Step done: he is on the ground at its far end.
                    *from_x += leg;
                    *phase_ms -= per_step;
                    return Step::Move { x: *from_x, y: *y };
                }
                Step::Move {
                    x: *from_x + (leg as f32 * t).round() as i32,
                    y: *y - gait.arc(t),
                }
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

/// Choose somewhere to go, given where he is and what is on screen.
///
/// Returns `None` when he is already somewhere sensible — moving for the sake
/// of it is what makes a desk companion tiring rather than alive.
pub fn pick<'a>(
    ledges: &'a [deskfolk_render_win::ledges::Ledge],
    x: i32,
    resting_on: &str,
    roll: u32,
) -> Option<&'a deskfolk_render_win::ledges::Ledge> {
    if ledges.is_empty() {
        return None;
    }
    // Prefer somewhere he is not already, so he actually goes somewhere; but
    // a single window on screen should not make him pace between it and the
    // desktop forever.
    let elsewhere: Vec<_> = ledges.iter().filter(|l| l.title != resting_on).collect();
    let choices = if elsewhere.is_empty() { ledges.iter().collect() } else { elsewhere };

    // Weight the front-most windows: what you are working in is the
    // interesting place to be, and `scan` already returns front to back.
    let near_front = choices.len().min(3);
    let idx = (roll as usize) % near_front.max(1);
    let chosen = choices[idx];
    // Not worth crossing the screen for a few pixels.
    if (chosen.left..=chosen.right).contains(&x) && chosen.title == resting_on {
        return None;
    }
    Some(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskfolk_render_win::ledges::Ledge;

    fn ledge(title: &str, left: i32, right: i32, top: i32) -> Ledge {
        Ledge { left, right, top, title: title.into() }
    }

    #[test]
    fn resting_counts_down_and_then_gets_restless() {
        let mut s = Stroll::new(100);
        assert!(!s.restless());
        assert_eq!(s.tick(60, Gait::Hop, true), Step::Stay);
        assert!(!s.restless());
        s.tick(60, Gait::Hop, true);
        assert!(s.restless(), "past its time: {s:?}");
    }

    fn walking(target_x: i32, y: i32, from_x: i32) -> Stroll {
        Stroll::Walking { to: "Editor".into(), target_x, y, from_x, phase_ms: 0 }
    }

    #[test]
    fn he_moves_every_frame_not_once_per_step() {
        // Teleporting a stride at a time is a twitch; this is what makes the
        // hop an arc.
        let mut s = walking(1000, 300, 100);
        let mut seen = Vec::new();
        for _ in 0..6 {
            if let Step::Move { x, .. } = s.tick(16, Gait::Hop, true) {
                seen.push(x);
            }
        }
        assert_eq!(seen.len(), 6, "moved on every frame: {seen:?}");
        assert!(seen.windows(2).all(|w| w[1] >= w[0]), "monotonic: {seen:?}");
        assert!(seen.last().unwrap() > &100, "made progress: {seen:?}");
    }

    #[test]
    fn a_hop_leaves_the_ground_and_lands_again() {
        let mut s = walking(1000, 300, 100);
        let (_, per_step) = Gait::Hop.step();
        let mut heights = Vec::new();
        for _ in 0..(per_step / 16) {
            if let Step::Move { y, .. } = s.tick(16, Gait::Hop, true) {
                heights.push(y);
            }
        }
        let peak = *heights.iter().min().expect("moved");
        assert!(peak < 300, "left the ground: {peak}");
        assert!(300 - peak <= Gait::Hop.lift() as i32, "no higher than the arc");
        // And comes back down by the end of the step.
        let landed = s.tick(per_step, Gait::Hop, true);
        assert!(matches!(landed, Step::Move { y, .. } if y == 300), "{landed:?}");
    }

    #[test]
    fn walking_keeps_his_feet_on_the_ground() {
        assert_eq!(Gait::Walk.arc(0.5), 0, "a walk cycle does not hop");
        let mut s = walking(1000, 300, 100);
        for _ in 0..8 {
            if let Step::Move { y, .. } = s.tick(16, Gait::Walk, true) {
                assert_eq!(y, 300);
            }
        }
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
        let mut s = Stroll::Walking {
            to: "L".into(), target_x: 0, y: 10, from_x: 500, phase_ms: 0,
        };
        let Step::Move { x, .. } = s.tick(16, Gait::Hop, true) else {
            panic!("should move");
        };
        assert!(x < 500, "moved toward the target, not away: {x}");
    }

    #[test]
    fn the_last_step_never_overshoots() {
        // Otherwise he lands past the target and walks back, forever.
        let mut s = walking(130, 10, 100);
        let (_, per_step) = Gait::Hop.step();
        let Step::Move { x, .. } = s.tick(per_step, Gait::Hop, true) else {
            panic!("should move");
        };
        assert_eq!(x, 130, "stops exactly on it rather than sailing past");
    }

    #[test]
    fn arriving_settles_him_and_reports_where() {
        let mut s = walking(100, 10, 90);
        assert_eq!(s.tick(16, Gait::Hop, true), Step::Arrived { on: "Editor".into() });
        assert!(!s.is_walking());
        assert!(s.restless(), "ready to consider the next place");
    }

    #[test]
    fn an_interrupted_walk_holds_its_place_rather_than_restarting() {
        // Being spoken to mid-walk should not cost him the journey.
        let mut s = Stroll::Walking {
            to: "E".into(), target_x: 900, y: 10, from_x: 100, phase_ms: 0,
        };
        assert_eq!(s.tick(16, Gait::Hop, false), Step::Stay);
        assert!(
            matches!(&s, Stroll::Walking { to, target_x, .. } if to == "E" && *target_x == 900),
            "still going there afterwards: {s:?}"
        );
        assert!(matches!(s.tick(16, Gait::Hop, true), Step::Move { .. }));
    }

    #[test]
    fn hopping_never_swaps_to_a_differently_scaled_clip() {
        // The regression: hopping played the package's `jump` clip, whose
        // frames are the `img_y_big*` set — drawn much larger than the seated
        // idle, so he ballooned every time he moved.
        assert_eq!(Gait::Hop.emotion(), None, "keeps whatever pose he is in");
        assert_eq!(Gait::Walk.emotion(), Some("walk"));
        assert_eq!(Gait::of(true), Gait::Walk);
        assert_eq!(Gait::of(false), Gait::Hop);
        // Hops are rarer and longer, so both gaits cross at a similar pace.
        let (walk_d, walk_t) = Gait::Walk.step();
        let (hop_d, hop_t) = Gait::Hop.step();
        assert!(hop_d > walk_d && hop_t > walk_t);
        let walk_speed = walk_d as f64 / walk_t as f64;
        let hop_speed = hop_d as f64 / hop_t as f64;
        assert!(
            (walk_speed - hop_speed).abs() < walk_speed * 0.5,
            "walk {walk_speed:.3} vs hop {hop_speed:.3} px/ms"
        );
    }

    #[test]
    fn he_prefers_somewhere_he_is_not_already() {
        let ls = vec![ledge("Editor", 0, 800, 200), ledge("the desktop", 0, 1920, 1040)];
        let picked = pick(&ls, 100, "the desktop", 0).expect("somewhere");
        assert_eq!(picked.title, "Editor");
    }

    #[test]
    fn with_nowhere_else_he_stays_put_rather_than_pacing() {
        let ls = vec![ledge("the desktop", 0, 1920, 1040)];
        assert!(pick(&ls, 500, "the desktop", 0).is_none(), "no pointless pacing");
    }

    #[test]
    fn an_empty_desktop_offers_nowhere_to_go() {
        assert!(pick(&[], 0, "anywhere", 0).is_none());
    }

    #[test]
    fn the_roll_only_ever_picks_a_real_ledge() {
        // A modulo over the wrong length is the classic way this panics.
        let ls = vec![ledge("A", 0, 400, 100), ledge("the desktop", 0, 1920, 1040)];
        for roll in 0..50 {
            let p = pick(&ls, 0, "nowhere", roll).expect("something");
            assert!(ls.iter().any(|l| l.title == p.title));
        }
    }
}
