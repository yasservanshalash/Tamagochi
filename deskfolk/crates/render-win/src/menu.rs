//! What a menu *is*, separate from how it is drawn.
//!
//! The companion's menu used to be a Win32 `TrackPopupMenu` — a grey system
//! panel with system fonts and system spacing, hanging off a character who
//! exists precisely because we refused to accept the system's window chrome.
//! It was the last piece of the OS visibly bolted onto him.
//!
//! So the menu is drawn by the same compositor that draws him (see
//! `flyout.rs`), and this module is only the description: rows, ids, icons,
//! and the flattening that turns nested submenus into the single list the
//! renderer animates.

/// A small glyph drawn beside a row. These are line art rather than bitmaps so
/// they stay sharp at any DPI, and so a character package does not have to
/// ship menu icons to get a menu that looks intentional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    None,
    Talk,
    Mic,
    Speaker,
    Sleep,
    Wake,
    Panel,
    Quit,
}

#[derive(Debug, Clone)]
pub enum MenuEntry {
    Item {
        id: String,
        label: String,
        enabled: bool,
        checked: bool,
        icon: Icon,
    },
    Separator,
    Submenu {
        label: String,
        icon: Icon,
        items: Vec<MenuEntry>,
    },
}

impl MenuEntry {
    pub fn item(id: impl Into<String>, label: impl Into<String>) -> Self {
        MenuEntry::Item {
            id: id.into(),
            label: label.into(),
            enabled: true,
            checked: false,
            icon: Icon::None,
        }
    }

    pub fn check(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        MenuEntry::Item {
            id: id.into(),
            label: label.into(),
            enabled: true,
            checked,
            icon: Icon::None,
        }
    }

    /// A label the user cannot act on — "No devices found" and friends.
    pub fn disabled(label: impl Into<String>) -> Self {
        MenuEntry::Item {
            id: String::new(),
            label: label.into(),
            enabled: false,
            checked: false,
            icon: Icon::None,
        }
    }

    pub fn submenu(label: impl Into<String>, icon: Icon, items: Vec<MenuEntry>) -> Self {
        MenuEntry::Submenu { label: label.into(), icon, items }
    }

    /// Builder-style icon, so the common case stays a one-liner.
    pub fn with_icon(mut self, i: Icon) -> Self {
        match &mut self {
            MenuEntry::Item { icon, .. } => *icon = i,
            MenuEntry::Submenu { icon, .. } => *icon = i,
            MenuEntry::Separator => {}
        }
        self
    }
}

/// What a flattened row is.
#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    Item { checked: bool },
    Separator,
    /// A row that expands the rows beneath it rather than opening a second
    /// panel. A flyout that spawns another flyout is a lot of window for a
    /// list of microphones, and it puts the choice further from the cursor.
    Submenu,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub label: String,
    pub kind: RowKind,
    pub icon: Icon,
    pub enabled: bool,
    /// Index of the submenu row this belongs to, if any.
    pub parent: Option<usize>,
}

impl Row {
    /// Can the user actually pick this row?
    pub fn selectable(&self) -> bool {
        self.enabled && !matches!(self.kind, RowKind::Separator)
    }
}

/// Flatten nested entries into the single list the renderer lays out.
///
/// Children keep a `parent` index so their height can be scaled by the
/// parent's expansion progress — which is what makes the panel grow smoothly
/// instead of snapping to a new size.
pub fn flatten(entries: &[MenuEntry]) -> Vec<Row> {
    let mut rows = Vec::new();
    push_all(entries, None, &mut rows);
    rows
}

fn push_all(entries: &[MenuEntry], parent: Option<usize>, rows: &mut Vec<Row>) {
    for entry in entries {
        match entry {
            MenuEntry::Separator => rows.push(Row {
                id: String::new(),
                label: String::new(),
                kind: RowKind::Separator,
                icon: Icon::None,
                enabled: false,
                parent,
            }),
            MenuEntry::Item { id, label, enabled, checked, icon } => rows.push(Row {
                id: id.clone(),
                label: label.clone(),
                kind: RowKind::Item { checked: *checked },
                icon: *icon,
                enabled: *enabled && !id.is_empty(),
                parent,
            }),
            MenuEntry::Submenu { label, icon, items } => {
                rows.push(Row {
                    id: String::new(),
                    label: label.clone(),
                    kind: RowKind::Submenu,
                    icon: *icon,
                    enabled: true,
                    parent,
                });
                let me = rows.len() - 1;
                push_all(items, Some(me), rows);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<MenuEntry> {
        vec![
            MenuEntry::item("talk", "Talk to him").with_icon(Icon::Talk),
            MenuEntry::Separator,
            MenuEntry::submenu(
                "Speakers",
                Icon::Speaker,
                vec![
                    MenuEntry::check("out::", "System default", true),
                    MenuEntry::check("out::Razer", "Razer", false),
                ],
            ),
            MenuEntry::item("quit", "Quit Deskfolk").with_icon(Icon::Quit),
        ]
    }

    #[test]
    fn children_point_back_at_the_row_that_owns_them() {
        // The parent link is what lets a child's height be scaled by the
        // parent's expansion; get it wrong and the panel grows the wrong rows.
        let rows = flatten(&sample());
        let speakers = rows.iter().position(|r| r.label == "Speakers").unwrap();
        assert_eq!(rows[speakers].parent, None);
        assert_eq!(rows[speakers + 1].parent, Some(speakers));
        assert_eq!(rows[speakers + 2].parent, Some(speakers));
    }

    #[test]
    fn top_level_rows_have_no_parent() {
        let rows = flatten(&sample());
        assert_eq!(rows[0].parent, None, "talk");
        assert_eq!(rows.last().unwrap().parent, None, "quit");
    }

    #[test]
    fn separators_are_never_selectable() {
        let rows = flatten(&sample());
        let sep = rows.iter().find(|r| r.kind == RowKind::Separator).unwrap();
        assert!(!sep.selectable());
    }

    #[test]
    fn a_row_without_an_id_cannot_be_picked() {
        // "No devices found" is a label, not an action. Letting it fire would
        // send an empty command id to the host.
        let rows = flatten(&[MenuEntry::disabled("No devices found")]);
        assert!(!rows[0].selectable());
        assert!(rows[0].id.is_empty());
    }

    #[test]
    fn checked_state_survives_flattening() {
        let rows = flatten(&sample());
        let default_row = rows.iter().find(|r| r.label == "System default").unwrap();
        assert_eq!(default_row.kind, RowKind::Item { checked: true });
    }

    #[test]
    fn icons_survive_the_builder() {
        let rows = flatten(&sample());
        assert_eq!(rows[0].icon, Icon::Talk);
        assert_eq!(rows.last().unwrap().icon, Icon::Quit);
    }

    #[test]
    fn an_empty_menu_flattens_to_nothing() {
        assert!(flatten(&[]).is_empty());
    }
}
