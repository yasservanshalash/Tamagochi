//! Turning the mind's chosen `action` into what his body does.
//!
//! A reply carries one `action` — his bodily verb, the counterpart to `music`,
//! which acts on whatever is playing *alongside* it. Two of those verbs the
//! engine already owns: `listen` reopens the mic, `sleep` lies him down, and it
//! performs both from inside `apply_reply`. The movement verbs are the *app's*
//! to grant, because walking moves the window across the screen and the engine
//! has no idea where he is on it.
//!
//! So this is the one seam between "the mind decided to move" and the wander
//! loop that actually moves him. It routes a walk onto the same `nudge` the
//! menu's Walk uses — one mechanism, whether the order came from a right-click
//! or from you saying "take a walk" out loud. Everything it does not recognise
//! it leaves alone for the engine to interpret, so an emotion the model invents
//! or a verb from a future version simply falls through rather than misfiring.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::stroll::Facing;

/// If `action` is a movement the app grants, place it on `nudge` for the wander
/// loop to pick up, and report that it was handled here. Anything else —
/// `listen`, `sleep`, `none`, or a verb this build does not know — returns
/// false and is left untouched for the engine.
pub fn route_walk(action: &str, nudge: &Arc<Mutex<Option<Facing>>>) -> bool {
    let dir = match action {
        "walk_left" => Facing::Left,
        "walk_right" => Facing::Right,
        // A bare "take a walk" names no side. Pick one off the clock so he does
        // not always set off the same way; `far_end` falls back to the desktop
        // floor when there is no ledge that way, so either direction is still a
        // real trip across the screen rather than a shuffle into the wall.
        "walk" => coin(),
        _ => return false,
    };
    *nudge.lock() = Some(dir);
    true
}

fn coin() -> Facing {
    let odd = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() & 1 == 1)
        .unwrap_or(false);
    if odd {
        Facing::Right
    } else {
        Facing::Left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot() -> Arc<Mutex<Option<Facing>>> {
        Arc::new(Mutex::new(None))
    }

    #[test]
    fn directional_walks_land_on_the_nudge() {
        let n = slot();
        assert!(route_walk("walk_left", &n));
        assert_eq!(*n.lock(), Some(Facing::Left));
        assert!(route_walk("walk_right", &n));
        assert_eq!(*n.lock(), Some(Facing::Right));
    }

    #[test]
    fn a_bare_walk_still_sets_off() {
        // No side to it, but it must pick one — a "take a walk" that does
        // nothing is the whole failure this feature exists to avoid.
        let n = slot();
        assert!(route_walk("walk", &n));
        assert!(n.lock().is_some(), "a bare walk must choose a direction");
    }

    #[test]
    fn non_movement_actions_are_left_for_the_engine() {
        // These are the engine's (listen, sleep) or nobody's (none, an invented
        // verb). The app must not claim them, or it would swallow the action
        // before the engine could act on it.
        let n = slot();
        for a in ["listen", "sleep", "none", "camera", "home", "dance", ""] {
            assert!(!route_walk(a, &n), "'{a}' is not the app's to grant");
        }
        assert!(n.lock().is_none(), "nothing should have touched the nudge");
    }
}
