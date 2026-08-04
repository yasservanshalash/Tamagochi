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
//! standing pose — so crossing the screen is done as a series of hops using the
//! existing `jump` clip, which reads as deliberate movement rather than the
//! sliding a static sprite would give. [`Gait`] picks whichever the package
//! actually has, so adding a `walk` clip later changes the animation and
//! nothing else.

use std::time::Duration;

/// Where he is headed and how far along he is.
#[derive(Debug, Clone, PartialEq)]
pub enum Stroll {
    /// Sitting somewhere, with time to wait before wandering again.
    Resting { on: String, until_ms: i64 },
    /// Crossing to `target_x`, at `y`, on the ledge called `to`.
    Walking { to: String, target_x: i32, y: i32, step_due_ms: i64 },
}

/// What the package can animate movement with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gait {
    /// A real walk cycle: he can move smoothly and continuously.
    Walk,
    /// No walk cycle, so he hops — a static sprite slid across the screen
    /// looks broken in a way a hop does not.
    Hop,
}

impl Gait {
    /// Which emotion to play for one unit of movement.
    pub fn emotion(self) -> &'static str {
        match self {
            Gait::Walk => "walk",
            Gait::Hop => "jump",
        }
    }

    /// How far he gets per step, and how long the step takes.
    ///
    /// A hop covers more ground per step because it happens less often; the
    /// two together come out at a similar crossing speed either way.
    pub fn step(self) -> (i32, i64) {
        match self {
            Gait::Walk => (14, 90),
            Gait::Hop => (46, 460),
        }
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
    /// Move to this position, playing a step of the gait.
    Move { x: i32, y: i32, animate: bool },
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

    /// Set off for a ledge.
    pub fn walk_to(&mut self, to: String, target_x: i32, y: i32) {
        *self = Stroll::Walking { to, target_x, y, step_due_ms: 0 };
    }

    /// Advance by `dt`. `x` is where he is now.
    ///
    /// `allowed` is false whenever he should not be wandering — asleep,
    /// talking, listening, being dragged. He stops where he is rather than
    /// resetting, so an interrupted walk resumes instead of starting over.
    pub fn tick(&mut self, dt: i64, x: i32, gait: Gait, allowed: bool) -> Step {
        match self {
            Stroll::Resting { until_ms, .. } => {
                *until_ms -= dt;
                Step::Stay
            }
            Stroll::Walking { to, target_x, y, step_due_ms } => {
                if !allowed {
                    return Step::Stay;
                }
                let (stride, per_step) = gait.step();
                let remaining = *target_x - x;
                if remaining.abs() <= ARRIVED_WITHIN {
                    let on = std::mem::take(to);
                    let arrived = Step::Arrived { on: on.clone() };
                    *self = Stroll::Resting { on, until_ms: 0 };
                    return arrived;
                }
                *step_due_ms -= dt;
                if *step_due_ms > 0 {
                    return Step::Stay;
                }
                *step_due_ms = per_step;
                // Never overshoot: a last stride longer than what is left
                // would put him past the target and walking back.
                let by = stride.min(remaining.abs()) * remaining.signum();
                Step::Move { x: x + by, y: *y, animate: true }
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
        assert_eq!(s.tick(60, 0, Gait::Hop, true), Step::Stay);
        assert!(!s.restless());
        s.tick(60, 0, Gait::Hop, true);
        assert!(s.restless(), "past its time: {s:?}");
    }

    #[test]
    fn walking_steps_toward_the_target_on_the_gait_s_beat() {
        let mut s = Stroll::Walking {
            to: "Editor".into(),
            target_x: 1000,
            y: 300,
            step_due_ms: 0,
        };
        let (stride, per_step) = Gait::Hop.step();
        assert_eq!(
            s.tick(16, 100, Gait::Hop, true),
            Step::Move { x: 100 + stride, y: 300, animate: true }
        );
        // Not again until the step is due.
        assert_eq!(s.tick(per_step / 2, 146, Gait::Hop, true), Step::Stay);
        assert!(matches!(s.tick(per_step, 146, Gait::Hop, true), Step::Move { .. }));
    }

    #[test]
    fn he_walks_left_as_readily_as_right() {
        let mut s = Stroll::Walking { to: "L".into(), target_x: 0, y: 10, step_due_ms: 0 };
        let Step::Move { x, .. } = s.tick(16, 500, Gait::Hop, true) else {
            panic!("should move");
        };
        assert!(x < 500, "moved toward the target, not away: {x}");
    }

    #[test]
    fn the_last_step_never_overshoots() {
        // Otherwise he lands past the target and walks back, forever.
        let mut s = Stroll::Walking { to: "E".into(), target_x: 130, y: 10, step_due_ms: 0 };
        let Step::Move { x, .. } = s.tick(16, 100, Gait::Hop, true) else {
            panic!("should move");
        };
        assert_eq!(x, 130, "stops exactly on it rather than sailing past");
    }

    #[test]
    fn arriving_settles_him_and_reports_where() {
        let mut s = Stroll::Walking { to: "Editor".into(), target_x: 100, y: 10, step_due_ms: 0 };
        assert_eq!(s.tick(16, 90, Gait::Hop, true), Step::Arrived { on: "Editor".into() });
        assert!(!s.is_walking());
        assert!(s.restless(), "ready to consider the next place");
    }

    #[test]
    fn an_interrupted_walk_holds_its_place_rather_than_restarting() {
        // Being spoken to mid-walk should not cost him the journey.
        let mut s = Stroll::Walking { to: "E".into(), target_x: 900, y: 10, step_due_ms: 0 };
        assert_eq!(s.tick(16, 100, Gait::Hop, false), Step::Stay);
        assert!(
            matches!(&s, Stroll::Walking { to, target_x, .. } if to == "E" && *target_x == 900),
            "still going there afterwards: {s:?}"
        );
        assert!(matches!(s.tick(16, 100, Gait::Hop, true), Step::Move { .. }));
    }

    #[test]
    fn a_walk_cycle_is_used_when_the_package_has_one() {
        assert_eq!(Gait::of(true), Gait::Walk);
        assert_eq!(Gait::of(false), Gait::Hop);
        assert_eq!(Gait::Walk.emotion(), "walk");
        assert_eq!(Gait::Hop.emotion(), "jump");
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
