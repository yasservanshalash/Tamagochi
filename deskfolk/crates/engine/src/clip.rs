//! The clip player.
//!
//! Direct descendant of the Watcher firmware's `scr_idle.c`, by way of the
//! alpha's `clips.py`. The model is deliberately tiny and has survived two
//! ports, so it is not being "improved" here:
//!
//! * a **base** clip is the standing state and loops (or holds its last frame)
//! * a **shot** is a one-shot that plays over the base and pops back when done
//! * an **FX** overlay cycles independently of the character frames
//!
//! Everything is driven by named clips looked up in the package, so the player
//! has no idea who it is animating.

use deskfolk_package::{CharacterPackage, Clip};

/// What to draw this instant. The renderer gets this and nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub sprite: String,
    pub dx: i32,
    pub dy: i32,
    /// FX sprite drawn above-right of the head, if the clip has one.
    pub fx: Option<String>,
}

pub struct ClipPlayer {
    base: String,
    shot: Option<String>,
    frame_idx: usize,
    frame_ms_left: i64,
    fx_idx: usize,
    fx_ms_left: i64,
    /// FX forced by the caller (used while talking, so an emotional intro's
    /// hearts or anger marks persist across the switch to the talk clip).
    fx_override: Vec<String>,
}

impl ClipPlayer {
    pub fn new(pkg: &CharacterPackage, base: &str) -> Self {
        let mut p = Self {
            base: base.to_string(),
            shot: None,
            frame_idx: 0,
            frame_ms_left: 0,
            fx_idx: 0,
            fx_ms_left: 0,
            fx_override: Vec::new(),
        };
        p.reset_frame(pkg);
        p
    }

    pub fn base_name(&self) -> &str {
        &self.base
    }

    pub fn shot_name(&self) -> Option<&str> {
        self.shot.as_deref()
    }

    /// The clip currently on screen — the shot if one is playing, else the base.
    pub fn current_name(&self) -> &str {
        self.shot.as_deref().unwrap_or(&self.base)
    }

    pub fn is_playing_shot(&self) -> bool {
        self.shot.is_some()
    }

    pub fn set_base(&mut self, pkg: &CharacterPackage, name: &str) {
        if self.base == name && self.shot.is_none() {
            return; // re-setting the same base would stutter the animation
        }
        self.base = name.to_string();
        self.shot = None;
        self.frame_idx = 0;
        self.fx_idx = 0;
        self.reset_frame(pkg);
    }

    pub fn play_shot(&mut self, pkg: &CharacterPackage, name: &str) {
        self.shot = Some(name.to_string());
        self.frame_idx = 0;
        self.fx_idx = 0;
        self.reset_frame(pkg);
    }

    pub fn set_fx_override(&mut self, fx: Vec<String>) {
        self.fx_override = fx;
    }

    fn clip<'a>(&self, pkg: &'a CharacterPackage) -> Option<&'a Clip> {
        pkg.clip(self.current_name())
    }

    fn reset_frame(&mut self, pkg: &CharacterPackage) {
        let Some(c) = self.clip(pkg) else {
            self.frame_ms_left = 1000;
            return;
        };
        if c.frames.is_empty() {
            self.frame_ms_left = 1000;
            return;
        }
        self.frame_idx = self.frame_idx.min(c.frames.len() - 1);
        self.frame_ms_left = c.frames[self.frame_idx].ms as i64;
        self.fx_ms_left = c.fx_ms as i64;
    }

    /// Advance by `dt` ms. Returns true if a one-shot finished on this tick,
    /// which the life loop uses to know a reaction has played out.
    pub fn tick(&mut self, pkg: &CharacterPackage, dt: i64) -> bool {
        let Some(c) = self.clip(pkg) else { return false };

        // FX cycles on its own clock, independent of character frames.
        if c.fx.len() > 1 && c.fx_ms > 0 {
            self.fx_ms_left -= dt;
            while self.fx_ms_left <= 0 {
                self.fx_ms_left += c.fx_ms as i64;
                self.fx_idx = self.fx_idx.wrapping_add(1);
            }
        }

        if c.frames.is_empty() {
            return false;
        }

        self.frame_ms_left -= dt;
        if self.frame_ms_left > 0 {
            return false;
        }

        let mut shot_finished = false;
        self.frame_idx += 1;
        if self.frame_idx >= c.frames.len() {
            if self.shot.is_some() {
                // One-shot done: fall back to whatever the base is.
                self.shot = None;
                self.frame_idx = 0;
                shot_finished = true;
            } else if c.looping {
                self.frame_idx = 0;
            } else {
                // Non-looping base parks on its final frame.
                self.frame_idx = c.frames.len() - 1;
            }
        }
        self.reset_frame(pkg);
        shot_finished
    }

    /// Resolve what to draw right now.
    pub fn frame(&self, pkg: &CharacterPackage) -> Frame {
        let Some(c) = self.clip(pkg) else {
            return Frame { sprite: String::new(), dx: 0, dy: 0, fx: None };
        };
        let Some(f) = c.frames.get(self.frame_idx.min(c.frames.len().saturating_sub(1)))
        else {
            return Frame { sprite: String::new(), dx: 0, dy: 0, fx: None };
        };

        // An override only applies while the base is showing; a one-shot
        // reaction brings its own FX and should win.
        let fx_list: &[String] = if !self.fx_override.is_empty() && self.shot.is_none() {
            &self.fx_override
        } else {
            &c.fx
        };
        let fx = if fx_list.is_empty() {
            None
        } else {
            Some(fx_list[self.fx_idx % fx_list.len()].clone())
        };

        Frame { sprite: f.img.clone(), dx: f.dx, dy: f.dy, fx }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::test_package;

    #[test]
    fn looping_base_wraps_around() {
        let pkg = test_package();
        let mut p = ClipPlayer::new(&pkg, "idle"); // 2 frames, 100ms each, loops
        assert_eq!(p.frame(&pkg).sprite, "a");
        p.tick(&pkg, 100);
        assert_eq!(p.frame(&pkg).sprite, "b");
        p.tick(&pkg, 100);
        assert_eq!(p.frame(&pkg).sprite, "a", "looping clip should wrap");
    }

    #[test]
    fn one_shot_returns_to_base_and_reports_finishing() {
        let pkg = test_package();
        let mut p = ClipPlayer::new(&pkg, "idle");
        p.play_shot(&pkg, "wave"); // 1 frame, 100ms, no loop
        assert_eq!(p.frame(&pkg).sprite, "w");
        let finished = p.tick(&pkg, 100);
        assert!(finished, "shot should report completion exactly once");
        assert_eq!(p.frame(&pkg).sprite, "a", "should fall back to the base clip");
        assert!(!p.is_playing_shot());
    }

    #[test]
    fn non_looping_base_parks_on_last_frame() {
        let pkg = test_package();
        let mut p = ClipPlayer::new(&pkg, "sleep"); // 1 frame, no loop
        for _ in 0..5 {
            p.tick(&pkg, 100);
        }
        assert_eq!(p.frame(&pkg).sprite, "z", "should hold, not blank out");
    }

    #[test]
    fn setting_the_same_base_does_not_restart_it() {
        let pkg = test_package();
        let mut p = ClipPlayer::new(&pkg, "idle");
        p.tick(&pkg, 100);
        assert_eq!(p.frame(&pkg).sprite, "b");
        p.set_base(&pkg, "idle");
        assert_eq!(p.frame(&pkg).sprite, "b", "re-setting the base would stutter");
    }

    #[test]
    fn fx_cycles_on_its_own_clock() {
        let pkg = test_package();
        let mut p = ClipPlayer::new(&pkg, "dance"); // fx [n1, n2], fx_ms 200
        assert_eq!(p.frame(&pkg).fx.as_deref(), Some("n1"));
        p.tick(&pkg, 200);
        assert_eq!(p.frame(&pkg).fx.as_deref(), Some("n2"));
        p.tick(&pkg, 200);
        assert_eq!(p.frame(&pkg).fx.as_deref(), Some("n1"));
    }

    #[test]
    fn large_dt_does_not_stall_the_fx_cycle() {
        // A stalled frame (GC pause, laptop resume) must not leave FX stuck.
        let pkg = test_package();
        let mut p = ClipPlayer::new(&pkg, "dance");
        p.tick(&pkg, 5_000);
        assert!(p.frame(&pkg).fx.is_some());
    }
}
