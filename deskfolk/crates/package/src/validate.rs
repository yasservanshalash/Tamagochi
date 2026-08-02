//! Manifest validation.
//!
//! This runs on every load, not just in the Workshop. A third-party package
//! that half-loads and then shows a character with missing limbs is worse
//! than one that refuses to load with a clear reason — so anything that would
//! break rendering is an `Error`, and anything merely suspect is a `Warning`
//! the author can see in the Workshop.

use std::fmt;
use std::path::Path;

use crate::Manifest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub severity: Severity,
    /// Dotted path into the manifest, e.g. `clips.dance.frames[2].img`.
    pub where_: String,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.where_, self.message)
    }
}

fn err(where_: impl Into<String>, message: impl Into<String>) -> Problem {
    Problem { severity: Severity::Error, where_: where_.into(), message: message.into() }
}

fn warn(where_: impl Into<String>, message: impl Into<String>) -> Problem {
    Problem { severity: Severity::Warning, where_: where_.into(), message: message.into() }
}

pub(crate) fn check(m: &Manifest, root: &Path) -> Vec<Problem> {
    let mut out = Vec::new();

    if m.id.trim().is_empty() {
        out.push(err("id", "must not be empty"));
    } else if !m.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        // The id becomes a directory name for save data.
        out.push(err("id", "may only contain a-z, 0-9, '-' and '_'"));
    }
    if m.name.trim().is_empty() {
        out.push(err("name", "must not be empty"));
    }
    if m.stage.width == 0 || m.stage.height == 0 {
        out.push(err("stage", "width and height must be non-zero"));
    }
    if m.clips.is_empty() {
        out.push(err("clips", "a character needs at least one clip"));
    }

    let sprite_dir = root.join(&m.sprites.dir);

    // Every frame must name a clip-local sprite that actually exists on disk.
    for (name, clip) in &m.clips {
        if clip.frames.is_empty() {
            out.push(err(format!("clips.{name}.frames"), "clip has no frames"));
        }
        for (i, f) in clip.frames.iter().enumerate() {
            let at = format!("clips.{name}.frames[{i}]");
            if f.ms == 0 {
                out.push(err(format!("{at}.ms"), "frame duration must be > 0"));
            }
            check_sprite(&mut out, &sprite_dir, &f.img, format!("{at}.img"));
        }
        for (i, fx) in clip.fx.iter().enumerate() {
            check_sprite(&mut out, &sprite_dir, fx, format!("clips.{name}.fx[{i}]"));
        }
        if clip.fx.len() > 1 && clip.fx_ms == 0 {
            out.push(warn(
                format!("clips.{name}.fx_ms"),
                "multiple fx frames but fx_ms is 0, so the overlay will never cycle",
            ));
        }
    }

    // Emotions must point at real clips.
    for (name, e) in &m.emotions {
        if !m.clips.contains_key(&e.clip) {
            out.push(err(
                format!("emotions.{name}.clip"),
                format!("no clip named '{}'", e.clip),
            ));
        }
        // Reverting "to idle" is meaningless for the idle state itself — it is
        // the thing everything else reverts *to*, so a zero hold is correct.
        if e.base && e.hold_ms == 0 && e.clip != m.roles.idle {
            out.push(warn(
                format!("emotions.{name}.hold_ms"),
                "base emotion with hold_ms 0 will never revert to idle on its own",
            ));
        }
    }

    // Roles are the engine's only handle on the package — all must resolve.
    let r = &m.roles;
    for (field, clip) in [
        ("idle", &r.idle),
        ("blink", &r.blink),
        ("talk", &r.talk),
        ("think", &r.think),
        ("sleep", &r.sleep),
        ("startle", &r.startle),
    ] {
        if !m.clips.contains_key(clip) {
            out.push(err(format!("roles.{field}"), format!("no clip named '{clip}'")));
        }
    }
    if let Some(g) = &r.glitch {
        if !m.clips.contains_key(g) {
            out.push(err("roles.glitch", format!("no clip named '{g}'")));
        }
    }
    for (i, g) in r.glitch_fx.iter().enumerate() {
        check_sprite(&mut out, &sprite_dir, g, format!("roles.glitch_fx[{i}]"));
    }

    for (i, v) in m.visemes.iter().enumerate() {
        check_sprite(&mut out, &sprite_dir, v, format!("visemes[{i}]"));
    }
    if !m.visemes.is_empty() && m.visemes.len() < 2 {
        out.push(warn("visemes", "fewer than 2 visemes — the mouth cannot animate"));
    }

    for (i, f) in m.life.fidgets.iter().enumerate() {
        if !m.clips.contains_key(f) {
            out.push(err(format!("life.fidgets[{i}]"), format!("no clip named '{f}'")));
        }
    }
    if m.life.fidgets.is_empty() {
        out.push(warn(
            "life.fidgets",
            "no idle fidgets — the character will look frozen between events",
        ));
    }

    let [start, end] = m.life.night_hours;
    if start > 23 || end > 23 {
        out.push(err("life.night_hours", "hours must be 0..=23"));
    }

    if m.personality.prompt.trim().is_empty() {
        out.push(warn("personality.prompt", "no character brief — the mind has nothing to go on"));
    }

    out
}

fn check_sprite(out: &mut Vec<Problem>, dir: &Path, name: &str, at: String) {
    if name.trim().is_empty() {
        out.push(Problem {
            severity: Severity::Error,
            where_: at,
            message: "sprite name is empty".into(),
        });
        return;
    }
    // Packages are third-party content: never let a manifest reach outside
    // its own sprite directory.
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        out.push(Problem {
            severity: Severity::Error,
            where_: at,
            message: format!("'{name}' must be a bare sprite name, not a path"),
        });
        return;
    }
    if !dir.join(format!("{name}.png")).exists() {
        out.push(Problem {
            severity: Severity::Error,
            where_: at,
            message: format!("sprite '{name}.png' not found in {}", dir.display()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_json() -> serde_json::Value {
        serde_json::json!({
            "format": 1,
            "id": "test",
            "name": "Test",
            "version": "0.1.0",
            "stage": { "width": 412, "height": 412, "anchor_x": 206, "anchor_y": 386 },
            "clips": { "idle": { "frames": [{ "img": "a", "ms": 100 }] } },
            "emotions": {},
            "roles": {
                "idle": "idle", "blink": "idle", "talk": "idle",
                "think": "idle", "sleep": "idle", "startle": "idle"
            }
        })
    }

    #[test]
    fn rejects_emotion_pointing_at_missing_clip() {
        let mut j = manifest_json();
        j["emotions"]["happy"] = serde_json::json!({ "clip": "nope" });
        let m: Manifest = serde_json::from_value(j).unwrap();
        let problems = check(&m, Path::new("/nonexistent"));
        assert!(problems
            .iter()
            .any(|p| p.severity == Severity::Error && p.where_ == "emotions.happy.clip"));
    }

    #[test]
    fn rejects_sprite_path_traversal() {
        let mut j = manifest_json();
        j["clips"]["idle"]["frames"][0]["img"] = serde_json::json!("../../secrets");
        let m: Manifest = serde_json::from_value(j).unwrap();
        let problems = check(&m, Path::new("/nonexistent"));
        assert!(problems
            .iter()
            .any(|p| p.severity == Severity::Error && p.message.contains("bare sprite name")));
    }

    #[test]
    fn rejects_unresolvable_role() {
        let mut j = manifest_json();
        j["roles"]["sleep"] = serde_json::json!("missing_clip");
        let m: Manifest = serde_json::from_value(j).unwrap();
        let problems = check(&m, Path::new("/nonexistent"));
        assert!(problems
            .iter()
            .any(|p| p.severity == Severity::Error && p.where_ == "roles.sleep"));
    }
}
