//! A day's record of what he said, thought and did.
//!
//! The point of this file is not debugging — `tracing` already does that, into
//! a console nobody keeps. This is a record of the *character*, written to be
//! read back later (by a person or by a model) and turned into a better one:
//! what he was asked, what he answered, how he felt about it, how long he took,
//! and which stack produced all of that.
//!
//! Three decisions worth stating.
//!
//! **One file per day, sessions appended.** The question being asked of it is
//! "what was he like today", which does not respect process boundaries — a
//! restart at lunchtime should not split the day in two.
//!
//! **Written as it happens, not buffered until quit.** Quitting is when the
//! summary is *sealed*, but a journal that only exists if the process exits
//! cleanly would lose exactly the sessions worth reading: the ones that ended
//! in a crash.
//!
//! **Global, like `tracing`.** Every interesting moment is inside a callback
//! or a spawned task that already carries as much state as it wants to. A
//! journal threaded through all of them would be plumbing in a dozen
//! signatures to serve one concern.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use chrono::Local;
use parking_lot::Mutex;

static JOURNAL: OnceLock<Journal> = OnceLock::new();

/// One reply, with everything needed to judge it later.
#[derive(Debug, Clone, Default)]
pub struct Said {
    /// What prompted him — `user_speech`, `self_talk`, `wake_greet`, …
    pub event: String,
    /// What he was told, when he was told anything.
    pub heard: String,
    pub say: String,
    pub emotion: String,
    pub glitch: u8,
    pub action: String,
    /// Whether this reached the speakers or stayed a subtitle.
    pub spoken: bool,
    /// How long the mind took.
    pub took: Duration,
}

/// Begin (or continue) today's journal in `dir`, recording `stack`.
///
/// `stack` is a list of label/value pairs describing what is running — model,
/// voice, register. It is written into the session header verbatim, so callers
/// must pass log-safe strings; nothing here redacts.
pub fn start(dir: &Path, character: &str, stack: Vec<(String, String)>) -> Option<PathBuf> {
    let journal = Journal::open(dir, character, stack)?;
    let path = journal.path.clone();
    // A second call would mean two sessions in one process, which is not a
    // thing that happens; if it ever did, the first journal keeps the file.
    if JOURNAL.set(journal).is_err() {
        tracing::warn!("journal: already started; ignoring the second start");
    }
    Some(path)
}

/// Record a reply. Cheap and silent when no journal was started.
pub fn said(entry: Said) {
    if let Some(j) = JOURNAL.get() {
        j.said(entry);
    }
}

/// Record something his body did that the transcript alone would not show —
/// falling asleep, being woken, the ear opening. Behaviour is half of what
/// "what was he like today" means.
pub fn did(what: impl AsRef<str>) {
    if let Some(j) = JOURNAL.get() {
        j.line("did", what.as_ref());
    }
}

/// Record something that went wrong. Kept in the same stream as the rest,
/// because a silence in the transcript is only explicable next to the 429
/// that caused it.
pub fn trouble(what: impl AsRef<str>) {
    if let Some(j) = JOURNAL.get() {
        j.line("!", what.as_ref());
        j.count_trouble();
    }
}

/// Seal the session with its summary. Safe to call more than once; only the
/// first call writes.
pub fn finish() {
    if let Some(j) = JOURNAL.get() {
        j.finish();
    }
}

/// Where today's journal lives, for telling the user.
pub fn path() -> Option<PathBuf> {
    JOURNAL.get().map(|j| j.path.clone())
}

// ---------------------------------------------------------------------------

struct Journal {
    path: PathBuf,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Tallies for the closing summary. The transcript is already on disk, so
    /// nothing here needs to keep the lines themselves.
    replies: usize,
    spoken: usize,
    tripped: usize,
    troubles: usize,
    events: BTreeMap<String, usize>,
    emotions: BTreeMap<String, usize>,
    thinks_ms: Vec<u128>,
    sealed: bool,
}

/// A reply counts as tripping at the glitch level the character brief calls
/// one — matching it here keeps the summary honest about how often he does it.
const TRIPPING_AT: u8 = 40;

impl Journal {
    fn open(dir: &Path, character: &str, stack: Vec<(String, String)>) -> Option<Journal> {
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::warn!("journal: cannot create {}: {e}", dir.display());
            return None;
        }
        let now = Local::now();
        let path = dir.join(format!("deskfolk-{}.md", now.format("%Y-%m-%d")));
        let fresh = !path.exists();

        let journal = Journal { path: path.clone(), inner: Mutex::new(Inner::default()) };
        let mut head = String::new();
        if fresh {
            head.push_str(&format!("# Deskfolk — {}\n", now.format("%A %-d %B %Y")));
        }
        head.push_str(&format!(
            "\n## Session · started {}\n\n**Character:** {character}\n",
            now.format("%H:%M:%S")
        ));
        for (label, value) in stack {
            head.push_str(&format!("**{label}:** {value}\n"));
        }
        head.push_str("\n### What he said and did\n\n");
        journal.write(&head);
        Some(journal)
    }

    /// Append to the file, reopening each time.
    ///
    /// Holding the handle open would be faster, but this runs a handful of
    /// times a minute at most, and a closed file is one a user can read, copy
    /// or hand to a model *while he is still running* — which is most of the
    /// point of writing it as it happens.
    fn write(&self, s: &str) {
        let opened = OpenOptions::new().create(true).append(true).open(&self.path);
        match opened {
            Ok(mut f) => {
                if let Err(e) = f.write_all(s.as_bytes()) {
                    tracing::warn!("journal: cannot write: {e}");
                }
            }
            Err(e) => tracing::warn!("journal: cannot open {}: {e}", self.path.display()),
        }
    }

    fn line(&self, kind: &str, what: &str) {
        let stamp = Local::now().format("%H:%M:%S");
        let mark = if kind == "!" { "**!**" } else { "·" };
        self.write(&format!("- `{stamp}` {mark} {}\n", one_line(what)));
    }

    fn said(&self, e: Said) {
        let stamp = Local::now().format("%H:%M:%S");
        // Both halves of the exchange get their own labelled line. Hanging
        // what was said off the event on one line technically recorded it,
        // but a transcript is read for the conversation, and the human half
        // of it should not be the part you have to go looking for.
        let mut out = format!("- `{stamp}` **{}**\n", e.event);
        if !e.heard.trim().is_empty() {
            out.push_str(&format!("  - **You:** \"{}\"\n", one_line(&e.heard)));
        }
        out.push_str("  - **Him:** *[");
        out.push_str(if e.emotion.is_empty() { "?" } else { &e.emotion });
        if e.glitch >= TRIPPING_AT {
            out.push_str(&format!(" glitch {}", e.glitch));
        }
        out.push_str(&format!("]* \"{}\"\n", one_line(&e.say)));

        let mut tags = vec![if e.spoken { "spoken".to_string() } else { "subtitle".to_string() }];
        if !e.action.is_empty() && e.action != "none" {
            tags.push(format!("then {}", e.action));
        }
        if !e.took.is_zero() {
            tags.push(format!("{:.1}s", e.took.as_secs_f32()));
        }
        out.push_str(&format!("    ({})\n", tags.join(" · ")));
        self.write(&out);

        let mut inner = self.inner.lock();
        inner.replies += 1;
        if e.spoken {
            inner.spoken += 1;
        }
        if e.glitch >= TRIPPING_AT {
            inner.tripped += 1;
        }
        *inner.events.entry(nonempty(&e.event, "unknown")).or_default() += 1;
        *inner.emotions.entry(nonempty(&e.emotion, "unknown")).or_default() += 1;
        if !e.took.is_zero() {
            inner.thinks_ms.push(e.took.as_millis());
        }
    }

    fn count_trouble(&self) {
        self.inner.lock().troubles += 1;
    }

    fn finish(&self) {
        let summary = {
            let mut inner = self.inner.lock();
            if inner.sealed {
                return;
            }
            inner.sealed = true;
            summarise(&inner)
        };
        self.write(&summary);
        tracing::info!("journal: written to {}", self.path.display());
    }
}

/// The closing summary — the part that answers "what was he like today"
/// without reading every line.
fn summarise(inner: &Inner) -> String {
    let mut s = format!("\n### Summary · ended {}\n\n", Local::now().format("%H:%M:%S"));
    if inner.replies == 0 {
        s.push_str("He never said anything this session.\n");
        return s;
    }
    s.push_str(&format!(
        "- {} replies, {} of them spoken aloud\n",
        inner.replies, inner.spoken
    ));
    s.push_str(&format!("- prompted by: {}\n", tally(&inner.events)));
    s.push_str(&format!("- emotions: {}\n", tally(&inner.emotions)));
    s.push_str(&format!(
        "- tripping (glitch {TRIPPING_AT}+): {} of {} ({}%)\n",
        inner.tripped,
        inner.replies,
        percent(inner.tripped, inner.replies)
    ));
    if let Some(mid) = median(&inner.thinks_ms) {
        s.push_str(&format!("- median time to answer: {:.1}s\n", mid as f64 / 1000.0));
    }
    s.push_str(&format!("- trouble: {}\n", inner.troubles));
    s
}

/// Counts, largest first, so the shape is visible at a glance.
fn tally(counts: &BTreeMap<String, usize>) -> String {
    let mut pairs: Vec<_> = counts.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    if pairs.is_empty() {
        return "none".into();
    }
    pairs.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
}

fn median(v: &[u128]) -> Option<u128> {
    if v.is_empty() {
        return None;
    }
    let mut sorted = v.to_vec();
    sorted.sort_unstable();
    Some(sorted[sorted.len() / 2])
}

fn percent(part: usize, whole: usize) -> usize {
    if whole == 0 { 0 } else { part * 100 / whole }
}

fn nonempty(s: &str, fallback: &str) -> String {
    let t = s.trim();
    if t.is_empty() { fallback.to_string() } else { t.to_string() }
}

/// Keep one entry on one line. Newlines inside a quoted reply would break the
/// list structure the file's readability depends on.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said_entry(emotion: &str, glitch: u8, spoken: bool, ms: u64) -> Said {
        Said {
            event: "user_speech".into(),
            heard: "yo".into(),
            say: "hey".into(),
            emotion: emotion.into(),
            glitch,
            action: "listen".into(),
            spoken,
            took: Duration::from_millis(ms),
        }
    }

    fn tallied(entries: Vec<Said>) -> Inner {
        let dir = std::env::temp_dir().join(format!("deskfolk-journal-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let j = Journal::open(&dir, "test", vec![]).expect("journal");
        for e in entries {
            j.said(e);
        }
        let inner = j.inner.lock();
        Inner {
            replies: inner.replies,
            spoken: inner.spoken,
            tripped: inner.tripped,
            troubles: inner.troubles,
            events: inner.events.clone(),
            emotions: inner.emotions.clone(),
            thinks_ms: inner.thinks_ms.clone(),
            sealed: inner.sealed,
        }
    }

    #[test]
    fn a_reply_lands_in_the_file_as_it_happens() {
        let dir = std::env::temp_dir().join("deskfolk-journal-live");
        let _ = std::fs::remove_dir_all(&dir);
        let j = Journal::open(&dir, "yasser", vec![("Mind".into(), "hermes".into())])
            .expect("journal");
        j.said(said_entry("think", 0, true, 1200));

        // Readable before anything has been finished or closed: the whole
        // reason the file is appended to rather than buffered.
        let text = std::fs::read_to_string(&j.path).expect("read");
        assert!(text.contains("**Mind:** hermes"), "stack in header: {text}");
        assert!(text.contains("user_speech"), "event: {text}");
        assert!(text.contains("**You:** \"yo\""), "what he was told: {text}");
        assert!(text.contains("**Him:** *[think]* \"hey\""), "reply: {text}");
        assert!(text.contains("spoken"), "voice status: {text}");
        assert!(text.contains("1.2s"), "latency: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unprompted_thought_has_no_you_line_to_invent() {
        let dir = std::env::temp_dir().join("deskfolk-journal-musing");
        let _ = std::fs::remove_dir_all(&dir);
        let j = Journal::open(&dir, "yasser", vec![]).expect("journal");
        j.said(Said {
            event: "self_talk".into(),
            say: "hm".into(),
            emotion: "idle".into(),
            ..Default::default()
        });
        let text = std::fs::read_to_string(&j.path).expect("read");
        assert!(!text.contains("**You:**"), "nobody spoke: {text}");
        assert!(text.contains("**Him:**"), "he did: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_day_gets_one_file_and_sessions_append() {
        let dir = std::env::temp_dir().join("deskfolk-journal-append");
        let _ = std::fs::remove_dir_all(&dir);
        let first = Journal::open(&dir, "yasser", vec![]).expect("first");
        first.said(said_entry("idle", 0, false, 100));
        first.finish();
        let second = Journal::open(&dir, "yasser", vec![]).expect("second");
        second.said(said_entry("happy", 0, true, 100));
        second.finish();

        assert_eq!(first.path, second.path, "a day is one file");
        let text = std::fs::read_to_string(&first.path).expect("read");
        assert_eq!(text.matches("## Session").count(), 2, "both sessions: {text}");
        // The day's title is written once, not per session.
        assert_eq!(text.matches("# Deskfolk —").count(), 1, "one title: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_summary_counts_what_it_claims_to() {
        let inner = tallied(vec![
            said_entry("think", 0, true, 1000),
            said_entry("glitch", 80, true, 3000),
            said_entry("think", 10, false, 2000),
        ]);
        let s = summarise(&inner);
        assert!(s.contains("3 replies, 2 of them spoken"), "{s}");
        // Only the reply at or above the tripping bar counts as one.
        assert!(s.contains("1 of 3 (33%)"), "trip rate: {s}");
        assert!(s.contains("think 2"), "emotions ranked: {s}");
        assert!(s.contains("median time to answer: 2.0s"), "{s}");
    }

    #[test]
    fn a_silent_session_says_so_rather_than_dividing_by_zero() {
        let s = summarise(&Inner::default());
        assert!(s.contains("never said anything"), "{s}");
    }

    #[test]
    fn sealing_twice_writes_one_summary() {
        let dir = std::env::temp_dir().join("deskfolk-journal-seal");
        let _ = std::fs::remove_dir_all(&dir);
        let j = Journal::open(&dir, "yasser", vec![]).expect("journal");
        j.said(said_entry("idle", 0, false, 100));
        j.finish();
        j.finish();
        let text = std::fs::read_to_string(&j.path).expect("read");
        assert_eq!(text.matches("### Summary").count(), 1, "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_multi_line_reply_stays_on_one_line() {
        assert_eq!(one_line("two\nlines  here"), "two lines here");
    }

    #[test]
    fn tallies_rank_by_count_then_name() {
        let mut m = BTreeMap::new();
        m.insert("idle".to_string(), 1);
        m.insert("think".to_string(), 5);
        m.insert("happy".to_string(), 5);
        assert_eq!(tally(&m), "happy 5, think 5, idle 1");
    }
}
