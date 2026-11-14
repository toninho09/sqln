//! The getting-started guide and the searchable groups of the key reference.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::line_input::LineInput;
use crate::ui::text;
use crate::ui::which_key::KeyGroup;

/// One topic of the guide. A step is an effective key and a text. An example is a
/// text and a statement.
pub struct GuideTopic {
    /// The short name in the sidebar.
    pub name: &'static str,
    pub title: &'static str,
    pub intro: &'static str,
    pub steps: Vec<(String, &'static str)>,
    pub examples: &'static [(&'static str, &'static str)],
}

/// The page that the help shows. The sidebar lists the topics, all keys, then each group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Topic(usize),
    All,
    Group(usize),
}

pub struct Help {
    pub title: String,
    pub guide: Vec<GuideTopic>,
    pub groups: Vec<KeyGroup>,
    pub input: LineInput,
    /// The selected row of the sidebar.
    pub page: usize,
    /// The first visible row of the sidebar.
    pub sidebar_first: usize,
    pub scroll: usize,
    pub visible_rows: usize,
    /// The drawn rows of the sidebar, with their page, for the mouse.
    pub page_rows: Vec<(usize, Rect)>,
}

fn rows(keys: &[(String, &'static str)]) -> String {
    keys.iter()
        .map(|(key, label)| format!("{key:<24}{label}"))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Help {
    pub fn new(title: String, guide: Vec<GuideTopic>, groups: Vec<KeyGroup>) -> Self {
        Self {
            title,
            guide,
            groups: groups
                .into_iter()
                .filter(|group| !group.keys.is_empty())
                .collect(),
            input: LineInput::default(),
            page: 0,
            sidebar_first: 0,
            scroll: 0,
            visible_rows: 1,
            page_rows: Vec::new(),
        }
    }

    pub fn view(&self) -> View {
        let topics = self.guide.len();
        match self.page {
            page if page < topics => View::Topic(page),
            page if page == topics => View::All,
            page => View::Group(page - topics - 1),
        }
    }

    /// Returns the titles of the sidebar rows, in page order.
    pub fn page_titles(&self) -> Vec<&'static str> {
        let topics = self.guide.iter().map(|topic| topic.name);
        let groups = self.groups.iter().map(|group| group.title);
        topics
            .chain(std::iter::once(text::HELP_ALL))
            .chain(groups)
            .collect()
    }

    /// Returns the first visible row of the sidebar. The sidebar scrolls the
    /// minimum number of rows that keeps the open page in view.
    pub fn sidebar_window(&mut self, rows: usize) -> usize {
        let total = self.guide.len() + self.groups.len() + 1;
        self.sidebar_first = self.sidebar_first.min(total.saturating_sub(rows));
        if self.page < self.sidebar_first {
            self.sidebar_first = self.page;
        }
        self.sidebar_first = if self.page >= self.sidebar_first + rows {
            self.page + 1 - rows
        } else {
            self.sidebar_first
        };
        self.sidebar_first
    }

    /// Opens a page of the sidebar. The filter stays.
    pub fn open_page(&mut self, page: usize) {
        if page < self.guide.len() + self.groups.len() + 1 {
            self.page = page;
            self.scroll = 0;
        }
    }

    /// Returns the text of the visible page for the clipboard.
    pub fn text(&self) -> String {
        match self.view() {
            View::Topic(index) => {
                let topic = &self.guide[index];
                let examples = topic
                    .examples
                    .iter()
                    .map(|(label, sql)| format!("{label}\n  {sql}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                [topic.title, topic.intro, &rows(&topic.steps), &examples]
                    .iter()
                    .filter(|part| !part.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
            view => self
                .groups
                .iter()
                .enumerate()
                .filter(|(index, _)| view == View::All || view == View::Group(*index))
                .map(|(_, group)| format!("{}\n{}", group.title, rows(&group.keys)))
                .collect::<Vec<_>>()
                .join("\n\n"),
        }
    }

    /// Returns true when every search word is in the text, without case sensitivity.
    pub fn search(&self, text: &str) -> bool {
        let text = text.to_lowercase();
        self.input
            .text()
            .split_whitespace()
            .all(|word| text.contains(&word.to_lowercase()))
    }

    /// Returns true when every search word matches the group, key, or description.
    pub fn matches(&self, group: &KeyGroup, key: &str, label: &str) -> bool {
        self.search(&format!("{} {key} {label}", group.title))
    }

    pub fn paste(&mut self, text: &str) {
        self.input.insert(text);
        self.scroll = 0;
    }

    /// Opens the page of a sidebar row under the mouse. Returns true for a row.
    pub fn click(&mut self, column: u16, row: u16) -> bool {
        let page = self
            .page_rows
            .iter()
            .find(|(_, area)| area.contains((column, row).into()))
            .map(|(page, _)| *page);
        if let Some(page) = page {
            self.open_page(page);
        }
        page.is_some()
    }

    /// Returns true when the key closes the help. Other keys never run commands.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::F(1) => return true,
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(self.visible_rows),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(self.visible_rows),
            KeyCode::Char('u') if ctrl => {
                self.input.set("");
                self.scroll = 0;
            }
            KeyCode::Home if ctrl => self.scroll = 0,
            KeyCode::End if ctrl => self.scroll = usize::MAX,
            KeyCode::Tab | KeyCode::BackTab => {
                let count = self.guide.len() + self.groups.len() + 1;
                let previous =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                self.open_page(if previous {
                    (self.page + count - 1) % count
                } else {
                    (self.page + 1) % count
                });
            }
            _ => {
                let before = self.input.text().to_owned();
                self.input.on_key(key);
                if self.input.text() != before {
                    self.scroll = 0;
                }
            }
        }
        false
    }
}
