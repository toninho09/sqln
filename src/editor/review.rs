//! Inline review of proposed changes: line changes against the editor text, or ghost
//! text at its anchor. The editor stays editable during a ghost and is read only
//! during a diff.

use std::time::{Duration, Instant};

use crossterm::event::{MouseEvent, MouseEventKind};
use edtui::{EditorMode, Index2};
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_width::UnicodeWidthChar;

use super::{SqlEditor, buffer};
use crate::agent::{Edit, Error, Position};
use crate::ui::theme::Theme;
use edtui::Lines;

/// Lines above the hunk that a scroll keeps visible.
const CONTEXT_ROWS: usize = 2;
/// Paired lines that are longer get no word marks.
const WORD_LIMIT: usize = 16384;

/// The origin of a review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Ai,
    /// The identifier of an agent proposal.
    Agent(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkState {
    Pending,
    Accepted,
    Rejected,
}

/// One change: a run of current lines that a run of new lines replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The first changed line of the current text.
    pub line: usize,
    /// The count of current lines that the change removes.
    pub old: usize,
    pub new: Vec<String>,
    /// The changed character ranges of each removed line, for the lines with a pair.
    pub old_marks: Vec<Vec<(usize, usize)>>,
    /// The changed character ranges of each added line, for the lines with a pair.
    pub new_marks: Vec<Vec<(usize, usize)>>,
    pub state: HunkState,
}

impl Hunk {
    /// Returns the bytes that the hunk keeps.
    pub fn bytes(&self) -> usize {
        let marks = |marks: &Vec<Vec<(usize, usize)>>| {
            marks.iter().map(|m| m.len() * 16 + 24).sum::<usize>()
        };
        self.new
            .iter()
            .map(|line| line.capacity() + 24)
            .sum::<usize>()
            + marks(&self.old_marks)
            + marks(&self.new_marks)
            + 96
    }
}

/// Returns the changes between two texts. `first_line` moves them to their place in
/// the editor, when the texts are a part of it.
pub fn diff_hunks(before: &str, after: &str, first_line: usize, timeout: Duration) -> Vec<Hunk> {
    // An empty text has no line to remove, so its change only adds lines.
    let old: Vec<&str> = if before.is_empty() {
        Vec::new()
    } else {
        before.split('\n').collect()
    };
    let new: Vec<&str> = after.split('\n').collect();
    let deadline = Instant::now() + timeout;
    let ops = similar::capture_diff_slices_deadline(
        similar::Algorithm::Myers,
        &old,
        &new,
        Some(deadline),
    );
    let mut ranges: Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> = Vec::new();
    for op in ops {
        let (tag, a, b) = op.as_tag_tuple();
        if tag == similar::DiffTag::Equal {
            continue;
        }
        match ranges.last_mut() {
            Some((last_a, last_b)) if last_a.end == a.start && last_b.end == b.start => {
                last_a.end = a.end;
                last_b.end = b.end;
            }
            _ => ranges.push((a, b)),
        }
    }
    ranges
        .into_iter()
        .map(|(a, b)| {
            let pairs = a.len().min(b.len());
            let (old_marks, new_marks) = (0..pairs)
                .map(|i| word_marks(old[a.start + i], new[b.start + i]))
                .unzip();
            Hunk {
                line: first_line + a.start,
                old: a.len(),
                new: new[b].iter().map(|line| (*line).to_owned()).collect(),
                old_marks,
                new_marks,
                state: HunkState::Pending,
            }
        })
        .collect()
}

/// Returns the changed character ranges of two lines, with an exclusive end.
/// Changed character ranges of one line.
type Marks = Vec<(usize, usize)>;

fn word_marks(old: &str, new: &str) -> (Marks, Marks) {
    if old.len() + new.len() >= WORD_LIMIT {
        return (Vec::new(), Vec::new());
    }
    let diff = similar::TextDiff::configure()
        .timeout(Duration::from_millis(30))
        .diff_words(old, new);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let (mut at_a, mut at_b) = (0, 0);
    let push = |marks: &mut Vec<(usize, usize)>, start: usize, end: usize| match marks.last_mut() {
        Some(last) if last.1 == start => last.1 = end,
        _ => marks.push((start, end)),
    };
    for change in diff.iter_all_changes() {
        let count = change.value().chars().count();
        match change.tag() {
            similar::ChangeTag::Equal => {
                at_a += count;
                at_b += count;
            }
            similar::ChangeTag::Delete => {
                push(&mut a, at_a, at_a + count);
                at_a += count;
            }
            similar::ChangeTag::Insert => {
                push(&mut b, at_b, at_b + count);
                at_b += count;
            }
        }
    }
    (a, b)
}

/// The keys that the bar and the buttons show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub accept: String,
    pub reject: String,
    pub accept_all: String,
    pub reject_all: String,
    pub close: String,
    /// The label of the close key: `Later` or `Reject`.
    pub close_label: &'static str,
}

impl Keys {
    /// Returns the label of the close key for the hint row.
    pub fn close_label_lower(&self) -> &'static str {
        if self.close_label == "Later" {
            "later"
        } else {
            "reject"
        }
    }
}

/// A control of the review that the mouse can click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    AcceptAll,
    RejectAll,
    Close,
    /// A row of a pending hunk. A click makes it the current hunk.
    Hunk(usize),
    /// The accept or reject button of one pending hunk.
    Decide {
        hunk: usize,
        accept: bool,
    },
    /// The accept or reject button of one ghost.
    GhostDecide {
        ghost: usize,
        accept: bool,
    },
}

pub enum Kind {
    Diff { hunks: Vec<Hunk>, current: usize },
}

/// One row of the review on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Text(usize),
    /// A current line that a pending hunk removes.
    Removed {
        line: usize,
        hunk: usize,
    },
    Added {
        hunk: usize,
        index: usize,
    },
    /// The buttons of the current hunk.
    Controls(usize),
    /// A row of the answer of the ghost `ghost`: its line `line`.
    Ghost {
        ghost: usize,
        line: usize,
    },
    /// The buttons after the last row of the ghost.
    GhostControls(usize),
}

/// A pending AI insertion: ghost text at its anchor. The editor stays editable, and
/// each answer waits for the decision of the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ghost {
    /// The anchor of the insertion in the current text.
    pub at: Index2,
    /// The lines of the answer. The first line completes the anchor line.
    pub lines: Vec<String>,
    /// The text of the anchor line at the last sync.
    pub anchor: String,
    /// The count of editor lines at the last sync.
    pub line_count: usize,
    /// The revision of the editor at the last sync.
    pub revision: u64,
}

/// The view of the editor while ghost text shows: the scroll and the last cursor
/// position that moved it.
pub struct GhostView {
    pub top: usize,
    pub left: usize,
    pub seen: Index2,
}

pub struct Review {
    pub source: Source,
    pub kind: Kind,
    /// The name of the origin in the bar, such as `AI` or the client of the agent.
    pub label: String,
    /// An extra text of the bar, such as the reduced-context label.
    pub note: String,
    pub keys: Keys,
    revision: u64,
    /// The last cursor that moved the view. The wheel scrolls without it.
    seen: Index2,
    pub top: usize,
    pub left: usize,
    pub targets: Vec<(Control, Rect)>,
}

/// The decisions of a finished review.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub accepted: usize,
    pub rejected: usize,
}

impl Review {
    pub fn diff(source: Source, label: String, hunks: Vec<Hunk>) -> Self {
        let current = hunks
            .iter()
            .position(|hunk| hunk.state == HunkState::Pending)
            .unwrap_or(0);
        Self::new(source, label, Kind::Diff { hunks, current })
    }

    fn new(source: Source, label: String, kind: Kind) -> Self {
        Self {
            source,
            kind,
            label,
            note: String::new(),
            keys: Keys::default(),
            revision: 0,
            seen: Index2::default(),
            top: 0,
            left: 0,
            targets: Vec::new(),
        }
    }

    pub fn hunks(&self) -> &[Hunk] {
        match &self.kind {
            Kind::Diff { hunks, .. } => hunks,
        }
    }

    /// Takes the hunks, with their decisions.
    pub fn into_hunks(self) -> Vec<Hunk> {
        match self.kind {
            Kind::Diff { hunks, .. } => hunks,
        }
    }

    /// Returns the index of the current hunk, when it waits for a decision.
    pub fn current(&self) -> Option<usize> {
        match &self.kind {
            Kind::Diff { hunks, current } => {
                (hunks.get(*current)?.state == HunkState::Pending).then_some(*current)
            }
        }
    }

    /// Returns the place of the current hunk among the pending hunks, and their count.
    pub fn place(&self) -> (usize, usize) {
        let pending: Vec<_> = self.pending().collect();
        let place = self
            .current()
            .and_then(|current| pending.iter().position(|index| *index == current))
            .map_or(0, |index| index + 1);
        (place, pending.len())
    }

    fn pending(&self) -> impl Iterator<Item = usize> + '_ {
        self.hunks()
            .iter()
            .enumerate()
            .filter(|(_, hunk)| hunk.state == HunkState::Pending)
            .map(|(index, _)| index)
    }

    pub fn outcome(&self) -> Outcome {
        self.hunks()
            .iter()
            .fold(Outcome::default(), |mut outcome, hunk| {
                match hunk.state {
                    HunkState::Accepted => outcome.accepted += 1,
                    HunkState::Rejected => outcome.rejected += 1,
                    HunkState::Pending => {}
                }
                outcome
            })
    }

    pub fn finished(&self) -> bool {
        self.pending().next().is_none()
    }

    /// Returns the rows of a pending hunk, with its buttons.
    fn block(&self, index: usize) -> usize {
        let hunk = &self.hunks()[index];
        hunk.old + hunk.new.len() + usize::from(hunk.state == HunkState::Pending)
    }

    /// Returns the count of rows for a text with `lines` lines.
    pub fn total_rows(&self, lines: usize) -> usize {
        match &self.kind {
            Kind::Diff { .. } => {
                lines
                    + self
                        .pending()
                        .map(|index| self.block(index) - self.hunks()[index].old)
                        .sum::<usize>()
            }
        }
    }

    /// Returns the first row of a pending hunk.
    fn hunk_row(&self, target: usize) -> usize {
        let extra: usize = self
            .pending()
            .take_while(|index| *index < target)
            .map(|index| self.block(index) - self.hunks()[index].old)
            .sum();
        self.hunks()[target].line + extra
    }

    /// Returns `count` rows from row `top`, for a text with `lines` lines.
    pub fn rows(&self, lines: usize, top: usize, count: usize) -> Vec<Row> {
        let mut rows = Vec::with_capacity(count);
        match &self.kind {
            Kind::Diff { hunks, .. } => {
                let pending: Vec<usize> = self.pending().collect();
                let (mut index, mut line, mut next) = (0, 0, 0);
                while rows.len() < count {
                    if let Some(&hunk) = pending.get(next)
                        && hunks[hunk].line <= line
                    {
                        let size = self.block(hunk);
                        let old = hunks[hunk].old;
                        for offset in 0..size {
                            if index + offset >= top && rows.len() < count {
                                rows.push(if offset < old {
                                    Row::Removed {
                                        line: line + offset,
                                        hunk,
                                    }
                                } else if offset < old + hunks[hunk].new.len() {
                                    Row::Added {
                                        hunk,
                                        index: offset - old,
                                    }
                                } else {
                                    Row::Controls(hunk)
                                });
                            }
                        }
                        index += size;
                        line += old;
                        next += 1;
                        continue;
                    }
                    if line >= lines {
                        break;
                    }
                    let end = pending
                        .get(next)
                        .map_or(lines, |hunk| hunks[*hunk].line)
                        .min(lines);
                    let run = end - line;
                    if index + run <= top {
                        index += run;
                        line = end;
                        continue;
                    }
                    let skip = top.saturating_sub(index);
                    for offset in skip..run {
                        if rows.len() == count {
                            break;
                        }
                        rows.push(Row::Text(line + offset));
                    }
                    index += run;
                    line = end;
                }
            }
        }
        rows
    }

    /// Scrolls so the current hunk is visible in `height` rows.
    fn show_current(&mut self, height: usize) {
        let (first, size) = match &self.kind {
            Kind::Diff { .. } => match self.current() {
                Some(current) => (self.hunk_row(current), self.block(current)),
                None => return,
            },
        };
        let height = height.max(1);
        if first < self.top || first + size.min(height) > self.top + height {
            self.top = first.saturating_sub(CONTEXT_ROWS.min(height.saturating_sub(size)));
        }
    }
}

impl SqlEditor {
    pub fn review(&self) -> Option<&Review> {
        self.review.as_ref()
    }

    pub fn review_mut(&mut self) -> Option<&mut Review> {
        self.review.as_mut()
    }

    /// Opens a review of the current revision. It closes the suggestion list.
    pub fn start_review(&mut self, mut review: Review) {
        self.close_suggestions();
        self.break_undo_group();
        review.revision = self.revision;
        review.seen = self.state.cursor;
        let (left, top) = self.scroll();
        review.top = top;
        review.left = left;
        review.show_current(self.page_height);
        self.review = Some(review);
    }

    /// Closes the review. The editor shows the line at the top of the review.
    pub fn take_review(&mut self) -> Option<Review> {
        let review = self.review.take()?;
        let lines = self.state.lines.len();
        let line = match review.rows(lines, review.top, 1).first() {
            Some(Row::Text(line) | Row::Removed { line, .. }) => *line,
            Some(Row::Added { hunk, .. } | Row::Controls(hunk)) => review.hunks()[*hunk].line,
            Some(Row::Ghost { .. } | Row::GhostControls(_)) | None => {
                review.top.min(lines.saturating_sub(1))
            }
        };
        self.set_scroll(review.left, line);
        Some(review)
    }

    /// Returns true when the text has not changed since the review opened.
    pub fn review_current(&self) -> bool {
        self.review
            .as_ref()
            .is_some_and(|review| review.revision == self.revision)
    }

    /// Sets the keys that the buttons of the ghosts show.
    pub fn set_ghost_keys(&mut self, keys: Keys) {
        self.ghost_keys = keys;
    }

    /// Returns the keys that the buttons of the ghosts show.
    pub fn ghost_keys(&self) -> &Keys {
        &self.ghost_keys
    }

    /// Returns the ghosts that wait for a decision.
    pub fn ghosts(&self) -> &[Ghost] {
        &self.ghosts
    }

    /// Returns true when a ghost answer waits in the editor.
    pub fn has_ghosts(&self) -> bool {
        !self.ghosts.is_empty()
    }

    /// Returns the view of the editor while ghost text shows.
    pub fn ghost_view(&self) -> Option<&GhostView> {
        self.ghost_view.as_ref()
    }

    /// Sets the scroll of the ghost view after a draw.
    pub fn set_ghost_view(&mut self, top: usize, left: usize) {
        if let Some(view) = &mut self.ghost_view {
            view.top = top;
            view.left = left;
        }
    }

    /// Sets the places of the buttons of the ghosts.
    pub fn set_ghost_targets(&mut self, targets: Vec<(Control, Rect)>) {
        self.ghost_targets = targets;
    }

    /// Returns the places of the buttons of the ghosts.
    pub fn ghost_targets(&self) -> &[(Control, Rect)] {
        &self.ghost_targets
    }

    /// Returns the place of the ghost `index` in shown rows: the blocks of the ghosts
    /// above add their rows.
    fn shown_row_of(&self, line: usize) -> usize {
        line + self
            .ghosts
            .iter()
            .filter(|ghost| ghost.at.row < line)
            .map(|ghost| ghost.lines.len())
            .sum::<usize>()
    }

    /// Opens a ghost answer at `at`. A ghost on the same line leaves first: the new
    /// answer replaces it.
    pub fn start_ghost(&mut self, at: Index2, text: &str) {
        self.ghosts.retain(|ghost| ghost.at.row != at.row);
        let anchor: String = buffer::row(&self.state.lines, at.row).iter().collect();
        if self.ghost_view.is_none() {
            let (left, top) = self.scroll();
            self.ghost_view = Some(GhostView {
                top,
                left,
                seen: self.state.cursor,
            });
        }
        self.ghosts.push(Ghost {
            at,
            lines: text.split('\n').map(str::to_owned).collect(),
            anchor,
            line_count: self.state.lines.len(),
            revision: self.revision,
        });
        let height = self.page_height;
        self.show_ghost(self.ghosts.len() - 1, height);
    }

    /// Scrolls the ghost view so the ghost `index` shows with its buttons.
    fn show_ghost(&mut self, index: usize, height: usize) {
        let Some(ghost) = self.ghosts.get(index) else {
            return;
        };
        let first = self.shown_row_of(ghost.at.row);
        let size = ghost.lines.len() + 1;
        let height = height.max(1);
        if let Some(view) = &mut self.ghost_view
            && (first < view.top || first + size.min(height) > view.top + height)
        {
            view.top = first.saturating_sub(CONTEXT_ROWS.min(height.saturating_sub(size)));
        }
    }

    /// Returns the count of shown rows of the editor with its ghosts.
    pub fn ghost_total_rows(&self) -> usize {
        self.state.lines.len()
            + self
                .ghosts
                .iter()
                .map(|ghost| ghost.lines.len())
                .sum::<usize>()
    }

    /// Returns `count` rows of the ghost view from row `top`. Each ghost replaces its
    /// anchor line with its answer and one row of buttons.
    pub fn ghost_rows(&self, top: usize, count: usize) -> Vec<Row> {
        let mut order: Vec<usize> = (0..self.ghosts.len()).collect();
        order.sort_by_key(|index| self.ghosts[*index].at.row);
        let total = self.state.lines.len();
        let mut rows = Vec::with_capacity(count.min(total + self.ghosts.len()));
        let (mut line, mut shown, mut next) = (0usize, 0usize, 0usize);
        while rows.len() < count {
            if let Some(&index) = order.get(next)
                && self.ghosts[index].at.row <= line
            {
                next += 1;
                let size = self.ghosts[index].lines.len() + 1;
                for offset in 0..size {
                    if shown >= top && rows.len() < count {
                        rows.push(if offset + 1 < size {
                            Row::Ghost {
                                ghost: index,
                                line: offset,
                            }
                        } else {
                            Row::GhostControls(index)
                        });
                    }
                    shown += 1;
                }
                line += 1;
                continue;
            }
            if line >= total {
                break;
            }
            if shown >= top {
                rows.push(Row::Text(line));
            }
            shown += 1;
            line += 1;
        }
        rows
    }

    /// Keeps the ghosts with the text after an edit. An edit of another line moves the
    /// anchors, and an edit of an anchor line closes only its ghost.
    pub fn sync_ghosts(&mut self) {
        let revision = self.revision;
        let total = self.state.lines.len();
        self.ghosts.retain_mut(|ghost| {
            let place = anchored_row(
                &self.state.lines,
                ghost.at.row,
                &ghost.anchor,
                ghost.line_count,
            );
            match place {
                Some(place) => {
                    ghost.at.row = place;
                    ghost.line_count = total;
                    ghost.revision = revision;
                    true
                }
                None => false,
            }
        });
        if self.ghosts.is_empty() {
            self.close_ghost_view();
        }
    }

    /// Removes all ghosts. Any undo takes them, as the user decided.
    pub fn take_ghosts(&mut self) {
        self.ghosts.clear();
        self.close_ghost_view();
    }

    /// Removes the ghosts anchored at `row`: a new answer on that line replaces them.
    pub fn close_ghosts_at_row(&mut self, row: usize) {
        self.ghosts.retain(|ghost| ghost.at.row != row);
        if self.ghosts.is_empty() {
            self.close_ghost_view();
        }
    }

    /// Ends the ghost view and gives the editor its scroll.
    fn close_ghost_view(&mut self) {
        if let Some(view) = self.ghost_view.take() {
            let total = self.state.lines.len();
            let top = view.top.min(total.saturating_sub(1));
            let line = match self.ghost_rows(top, 1).first() {
                Some(Row::Text(line)) => *line,
                _ => top,
            };
            self.set_scroll(view.left, line);
        }
    }

    /// Returns the ghost anchored at `row`, when one waits there.
    pub fn ghost_at(&self, row: usize) -> Option<usize> {
        self.ghosts.iter().position(|ghost| ghost.at.row == row)
    }

    /// Decides the ghost `index`: an acceptance inserts the answer at its anchor as
    /// one undo step. The cursor and the selection of the user stay in place.
    pub fn ghost_decide(&mut self, index: usize, accept: bool) -> Result<(), String> {
        let Some(ghost) = self.ghosts.get(index) else {
            return Ok(());
        };
        let (at, revision) = (ghost.at, ghost.revision);
        let text = ghost.lines.join("\n");
        if !accept {
            self.remove_ghost(index);
            return Ok(());
        }
        if revision != self.revision {
            return Err("The console changed. Request another completion.".into());
        }
        self.remove_ghost(index);
        self.apply_ghost(at, &text)
    }

    fn remove_ghost(&mut self, index: usize) {
        if self.ghosts.len() == 1 {
            self.take_ghosts();
        } else {
            self.ghosts.remove(index);
        }
    }

    /// Converts a screen cell to a text position while ghost text shows. The ghost
    /// rows and their button rows have no position.
    pub fn ghost_point(&self, column: u16, row: u16) -> Option<Index2> {
        let view = self.ghost_view.as_ref()?;
        let area = self.text_area?;
        if row < area.y || row >= area.bottom() || column >= area.right() {
            return None;
        }
        let shown = usize::from(row - area.y) + view.top;
        let line = match self.ghost_rows(shown, 1).first() {
            Some(Row::Text(line)) => *line,
            _ => return None,
        };
        let left = view.left;
        let chars = buffer::row(&self.state.lines, line);
        let wanted = usize::from(column.saturating_sub(area.x));
        let mut width = 0;
        for (col, c) in chars.iter().enumerate().skip(left) {
            width += if *c == '\t' {
                super::TAB_WIDTH
            } else {
                c.width().unwrap_or(0)
            };
            if width > wanted {
                return Some(Index2::new(line, col));
            }
        }
        Some(Index2::new(line, chars.len()))
    }

    /// Points the cursor at a screen cell of the ghost view, as a click or a drag.
    pub fn point_ghost(&mut self, mouse: MouseEvent) {
        let Some(position) = self.ghost_point(mouse.column, mouse.row) else {
            return;
        };
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.anchor = None;
        } else if self.anchor.is_none() {
            self.anchor = Some(self.state.cursor);
        }
        self.state.cursor = position;
        self.preferred_col = None;
        self.break_undo_group();
        self.sync_selection();
    }

    /// Scrolls the ghost view so the cursor stays visible when it moves. The wheel
    /// scrolls the view without moving the cursor, so it keeps its place.
    pub fn ghost_follow(&mut self, width: usize) {
        let Some(view) = &mut self.ghost_view else {
            return;
        };
        let cursor = self.state.cursor;
        if cursor == view.seen {
            return;
        }
        view.seen = cursor;
        let height = self.page_height.max(1);
        let row = cursor.row
            + self
                .ghosts
                .iter()
                .filter(|ghost| ghost.at.row < cursor.row)
                .map(|ghost| ghost.lines.len())
                .sum::<usize>();
        if row < view.top {
            view.top = row;
        } else if row >= view.top + height {
            view.top = row + 1 - height;
        }
        let left = view.left;
        let followed = self.follow_left(cursor, left, width);
        self.ghost_view.as_mut().expect("open ghost view").left = followed;
    }

    /// Returns the first visible column that keeps the cursor inside `width` cells.
    fn follow_left(&self, cursor: Index2, left: usize, width: usize) -> usize {
        let line = buffer::row(&self.state.lines, cursor.row);
        let last = match self.state.mode {
            EditorMode::Normal => line.len().saturating_sub(1),
            _ => line.len(),
        };
        let cursor = cursor.col.min(last);
        if cursor < left {
            return cursor;
        }
        if cursor <= super::last_column(line, left, width) {
            return left;
        }
        let mut used = 0;
        let fitting = (0..=cursor).rev().take_while(|col| {
            used += line.get(*col).map_or(1, |c| super::cells(*c));
            used < width
        });
        cursor.saturating_sub(fitting.count())
    }

    /// Accepts or rejects the current hunk, or all pending hunks. An acceptance changes
    /// the text as one undo step.
    pub fn review_decide(&mut self, accept: bool, all: bool) -> Result<(), Error> {
        let revision = self.revision;
        let Some(review) = self.review.as_mut() else {
            return Ok(());
        };
        if review.revision != revision {
            return Err(Error::new(
                "revision_conflict",
                "The console changed. Read it again and submit a new edit.",
            ));
        }
        let chosen: Vec<usize> = if all {
            review.pending().collect()
        } else {
            review.current().into_iter().collect()
        };
        if chosen.is_empty() {
            return Ok(());
        }
        if accept {
            let edits: Vec<Edit> = chosen
                .iter()
                .map(|index| line_edit(&self.state.lines, &review.hunks()[*index]))
                .collect();
            self.apply_agent_edits(revision, &edits)?;
        }
        let review = self.review.as_mut().expect("checked review");
        review.revision = self.revision;
        let Kind::Diff { hunks, current } = &mut review.kind;
        let mut shift = 0_isize;
        for (index, hunk) in hunks.iter_mut().enumerate() {
            hunk.line = hunk.line.saturating_add_signed(shift);
            if chosen.contains(&index) {
                if accept {
                    hunk.state = HunkState::Accepted;
                    shift += hunk.new.len() as isize - hunk.old as isize;
                    hunk.old = hunk.new.len();
                } else {
                    hunk.state = HunkState::Rejected;
                }
            }
        }
        let after = *current;
        let next = (after + 1..hunks.len())
            .chain(0..after)
            .find(|index| hunks[*index].state == HunkState::Pending);
        if let Some(next) = next {
            *current = next;
        }
        let height = self.page_height;
        self.review
            .as_mut()
            .expect("checked review")
            .show_current(height);
        Ok(())
    }

    /// Selects the next or the previous pending hunk.
    pub fn review_step(&mut self, forward: bool) {
        let height = self.page_height;
        let Some(review) = self.review.as_mut() else {
            return;
        };
        let pending: Vec<usize> = review.pending().collect();
        let Kind::Diff { current, .. } = &mut review.kind;
        let next = if forward {
            pending
                .iter()
                .find(|index| **index > *current)
                .or(pending.first())
        } else {
            pending
                .iter()
                .rev()
                .find(|index| **index < *current)
                .or(pending.last())
        };
        if let Some(next) = next {
            *current = *next;
            review.show_current(height);
        }
    }

    /// Makes a pending hunk the current hunk.
    pub fn review_select(&mut self, hunk: usize) {
        if let Some(review) = self.review.as_mut()
            && let Kind::Diff { hunks, current } = &mut review.kind
            && hunks
                .get(hunk)
                .is_some_and(|h| h.state == HunkState::Pending)
        {
            *current = hunk;
        }
    }

    /// Moves the view of the review or of the ghost text by rows and columns.
    pub fn review_scroll(&mut self, rows: isize, columns: isize) {
        let lines = self.state.lines.len();
        let height = self.page_height;
        if let Some(review) = self.review.as_mut() {
            let last = review.total_rows(lines).saturating_sub(height.max(1));
            review.top = review.top.saturating_add_signed(rows).min(last);
            review.left = review.left.saturating_add_signed(columns);
        } else if self.ghost_view.is_some() {
            let total = self.ghost_total_rows();
            if let Some(view) = self.ghost_view.as_mut() {
                let last = total.saturating_sub(height.max(1));
                view.top = view.top.saturating_add_signed(rows).min(last);
                view.left = view.left.saturating_add_signed(columns);
            }
        }
    }

    /// Sets the rows of the review view, without the bar.
    pub fn set_review_height(&mut self, height: usize) {
        self.page_height = height;
    }

    /// Returns the count of characters of a line.
    pub fn review_row_length(&self, line: usize) -> usize {
        buffer::row(&self.state.lines, line).len()
    }

    /// Returns up to `count` characters of a line from column `left`.
    pub fn review_row_chars(&self, line: usize, left: usize, count: usize) -> Vec<char> {
        buffer::row(&self.state.lines, line)
            .iter()
            .skip(left)
            .take(count)
            .copied()
            .collect()
    }

    /// Returns the syntax style of each character of a row, from column `left` for
    /// `count` characters.
    pub fn review_row_styles(
        &mut self,
        row: usize,
        left: usize,
        count: usize,
        theme: &Theme,
    ) -> Vec<Option<Style>> {
        let length = buffer::row(&self.state.lines, row).len();
        let end = (left + count).min(length);
        let mut styles = vec![None; end.saturating_sub(left)];
        if styles.is_empty() {
            return styles;
        }
        let dialect = self.dialect;
        let snapshot = self.snapshot();
        for (token, first, last) in snapshot.row_tokens(row, left, end) {
            let word = &snapshot.text[token.start()..token.end()];
            if let Some(style) = theme.sql_token_style(token.kind, word, dialect) {
                for column in first.max(left)..=last.min(end - 1) {
                    styles[column - left] = Some(style);
                }
            }
        }
        styles
    }
}

/// Returns the edit that replaces the lines of a hunk with its new lines.
fn line_edit(lines: &edtui::Lines, hunk: &Hunk) -> Edit {
    let total = lines.len().max(1);
    let length = |row: usize| buffer::row(lines, row).len();
    let at = |line: usize, column: usize| Position { line, column };
    let joined = hunk.new.join("\n");
    let empty = total == 1 && length(0) == 0;
    let (start, end, text) = if hunk.old == 0 {
        if empty {
            (at(0, 0), at(0, 0), joined)
        } else if hunk.line < total {
            let point = at(hunk.line, 0);
            (point, point, format!("{joined}\n"))
        } else {
            let point = at(total - 1, length(total - 1));
            (point, point, format!("\n{joined}"))
        }
    } else {
        let last = hunk.line + hunk.old - 1;
        if !hunk.new.is_empty() {
            (at(hunk.line, 0), at(last, length(last)), joined)
        } else if last + 1 < total {
            (at(hunk.line, 0), at(last + 1, 0), String::new())
        } else if hunk.line > 0 {
            let before = hunk.line - 1;
            (
                at(before, length(before)),
                at(last, length(last)),
                String::new(),
            )
        } else {
            (at(0, 0), at(last, length(last)), String::new())
        }
    };
    Edit { start, end, text }
}

/// Returns the current row of an anchored line: the row that follows a change of
/// the line count, then the recorded row. None when the line no longer has its text.
pub fn anchored_row(lines: &Lines, row: usize, text: &str, old_total: usize) -> Option<usize> {
    let total = lines.len();
    let line = |index: usize| buffer::row(lines, index).iter().collect::<String>();
    let moved = row.saturating_add_signed(total as isize - old_total as isize);
    if moved < total && line(moved) == text {
        Some(moved)
    } else if row < total && line(row) == text {
        Some(row)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::MemoryClipboard;
    use crate::editor::KeyboardMode;
    use crate::sql::Dialect;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn editor(text: &str) -> SqlEditor {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        SqlEditor::new(text, KeyboardMode::Conventional, Dialect::Sqlite, clipboard)
    }

    /// Opens a review of `after`, accepts all changes, and returns the text.
    fn accept_all(before: &str, after: &str) -> String {
        let mut editor = editor(before);
        let hunks = diff_hunks(before, after, 0, Duration::from_millis(100));
        editor.start_review(Review::diff(Source::Ai, "AI".into(), hunks));
        editor.review_decide(true, true).unwrap();
        editor.text()
    }

    #[test]
    fn accepted_changes_give_the_proposed_text_at_each_edge() {
        for (before, after) in [
            ("a\nb\nc", "a\nc"),
            ("a\nb\nc", "a\nb"),
            ("a\nb\nc", "b\nc"),
            ("a\nb", "a\nb\nc\nd"),
            ("a\nb", "z\na\nb"),
            ("a", ""),
            ("", "x\ny\n"),
            ("a\nb\nc\nd\ne", "A\nb\nc\nd\nE"),
        ] {
            assert_eq!(accept_all(before, after), after, "{before:?} -> {after:?}");
        }
    }

    #[test]
    fn rows_put_added_lines_and_buttons_after_the_removed_lines() {
        let before = "one\ntwo\nthree\nfour\nfive";
        let after = "one\nTWO\nthree\nfour\nFIVE\nsix";
        let mut editor = editor(before);
        let hunks = diff_hunks(before, after, 0, Duration::from_millis(100));
        assert_eq!(hunks.len(), 2);
        editor.start_review(Review::diff(Source::Ai, "AI".into(), hunks));
        let review = editor.review().unwrap();
        assert_eq!(
            review.rows(5, 0, 20),
            [
                Row::Text(0),
                Row::Removed { line: 1, hunk: 0 },
                Row::Added { hunk: 0, index: 0 },
                Row::Controls(0),
                Row::Text(2),
                Row::Text(3),
                Row::Removed { line: 4, hunk: 1 },
                Row::Added { hunk: 1, index: 0 },
                Row::Added { hunk: 1, index: 1 },
                Row::Controls(1),
            ]
        );
        assert_eq!(review.total_rows(5), 10);
        assert_eq!(
            review.rows(5, 6, 3),
            [
                Row::Removed { line: 4, hunk: 1 },
                Row::Added { hunk: 1, index: 0 },
                Row::Added { hunk: 1, index: 1 },
            ]
        );
        // A rejection keeps the text, and the next change becomes current.
        editor.review_decide(false, false).unwrap();
        assert_eq!(editor.text(), before);
        assert_eq!(editor.review().unwrap().current(), Some(1));
        assert_eq!(
            editor.review().unwrap().rows(5, 0, 2),
            [Row::Text(0), Row::Text(1)]
        );
        editor.review_decide(true, false).unwrap();
        let review = editor.review().unwrap();
        assert!(review.finished());
        assert_eq!(
            review.outcome(),
            Outcome {
                accepted: 1,
                rejected: 1
            }
        );
        assert_eq!(editor.text(), "one\ntwo\nthree\nfour\nFIVE\nsix");
    }

    #[test]
    fn each_accepted_change_is_one_undo_step_and_moves_the_next_change() {
        let before = "a\nb\nc\nd\ne";
        let after = "a\nB1\nB2\nc\nd\nE";
        let mut editor = editor(before);
        let hunks = diff_hunks(before, after, 0, Duration::from_millis(100));
        editor.start_review(Review::diff(Source::Ai, "AI".into(), hunks));
        editor.review_decide(true, false).unwrap();
        assert_eq!(editor.review().unwrap().hunks()[1].line, 5);
        editor.review_decide(true, false).unwrap();
        assert_eq!(editor.text(), after);
        editor.take_review();
        editor.on_command(crate::input::Action::EditorUndo, None);
        assert_eq!(editor.text(), "a\nB1\nB2\nc\nd\ne");
        editor.on_command(crate::input::Action::EditorUndo, None);
        assert_eq!(editor.text(), before);
    }

    #[test]
    fn a_changed_text_refuses_a_decision() {
        let mut editor = editor("a");
        let hunks = diff_hunks("a", "b", 0, Duration::from_millis(100));
        editor.start_review(Review::diff(Source::Ai, "AI".into(), hunks));
        editor.on_paste("x");
        assert!(!editor.review_current());
        assert_eq!(
            editor.review_decide(true, true).unwrap_err().code,
            "revision_conflict"
        );
    }

    #[test]
    fn ghost_rows_replace_the_anchor_line_and_add_buttons() {
        let mut editor = editor("SELECT\nFROM t");
        editor.start_ghost(Index2::new(0, 6), " a,\n  b");
        assert_eq!(editor.ghost_total_rows(), 4);
        assert_eq!(
            editor.ghost_rows(0, 5),
            [
                Row::Ghost { ghost: 0, line: 0 },
                Row::Ghost { ghost: 0, line: 1 },
                Row::GhostControls(0),
                Row::Text(1)
            ]
        );
        assert_eq!(
            editor.ghost_rows(2, 2),
            [Row::GhostControls(0), Row::Text(1)]
        );
    }

    #[test]
    fn each_ghost_has_its_rows_and_its_buttons() {
        let mut editor = editor("SELECT\nFROM t\nWHERE x");
        editor.start_ghost(Index2::new(0, 6), " a");
        editor.start_ghost(Index2::new(2, 7), " AND y");
        assert_eq!(editor.ghost_total_rows(), 5);
        assert_eq!(
            editor.ghost_rows(0, 5),
            [
                Row::Ghost { ghost: 0, line: 0 },
                Row::GhostControls(0),
                Row::Text(1),
                Row::Ghost { ghost: 1, line: 0 },
                Row::GhostControls(1),
            ]
        );
        // A ghost on the same line replaces the older one.
        editor.start_ghost(Index2::new(2, 7), " AND z");
        assert_eq!(editor.ghosts().len(), 2);
        assert_eq!(editor.ghosts()[1].lines, [" AND z"]);
    }

    #[test]
    fn an_edit_on_another_line_keeps_or_moves_the_anchor() {
        let mut editor = editor("SELECT\nFROM t");
        editor.start_ghost(Index2::new(0, 6), " 1");
        // An edit on another line keeps the anchor.
        editor.set_cursor(Index2::new(1, 5));
        editor.insert_at_cursor("x");
        assert_eq!(editor.ghosts()[0].at.row, 0);
        // A line above moves the anchor with the text.
        editor.set_cursor(Index2::new(0, 0));
        editor.on_key(
            crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Enter),
            None,
        );
        assert_eq!(editor.ghosts()[0].at.row, 1);
        assert_eq!(editor.text(), "\nSELECT\nFROM xt");
        // An edit on the anchor line closes only that ghost, in silence.
        editor.start_ghost(Index2::new(2, 6), " 2");
        assert_eq!(editor.ghosts().len(), 2);
        editor.set_cursor(Index2::new(1, 6));
        editor.insert_at_cursor("!");
        assert_eq!(editor.ghosts().len(), 1);
        assert_eq!(editor.ghosts()[0].at.row, 2);
    }

    #[test]
    fn an_undo_takes_every_ghost() {
        let mut editor = editor("SELECT\nFROM t\nWHERE x");
        editor.start_ghost(Index2::new(0, 6), " 1");
        editor.start_ghost(Index2::new(2, 7), " AND y");
        // An edit on a line without a ghost keeps them, and any undo takes them all.
        editor.set_cursor(Index2::new(1, 6));
        editor.insert_at_cursor("x");
        assert_eq!(editor.ghosts().len(), 2);
        editor.on_command(crate::input::Action::EditorUndo, None);
        assert!(!editor.has_ghosts());
        assert_eq!(editor.text(), "SELECT\nFROM t\nWHERE x");
    }

    #[test]
    fn accepting_a_ghost_inserts_at_the_anchor_as_one_undo_step() {
        let mut editor = editor("SELECT\nFROM t");
        editor.start_ghost(Index2::new(0, 6), " 1, 2\n  b");
        // The cursor moves to another line; the acceptance still uses the anchor.
        editor.set_cursor(Index2::new(1, 5));
        editor.ghost_decide(0, true).unwrap();
        assert_eq!(editor.text(), "SELECT 1, 2\n  b\nFROM t");
        editor.on_command(crate::input::Action::EditorUndo, None);
        assert_eq!(editor.text(), "SELECT\nFROM t");
    }

    #[test]
    fn rejecting_a_ghost_keeps_the_text() {
        let mut editor = editor("SELECT\nFROM t");
        editor.start_ghost(Index2::new(0, 6), " 1");
        editor.ghost_decide(0, false).unwrap();
        assert!(!editor.has_ghosts());
        assert_eq!(editor.text(), "SELECT\nFROM t");
    }

    #[test]
    fn an_edit_on_another_line_keeps_the_ghost_acceptable() {
        let mut editor = editor("SELECT\nFROM t");
        editor.start_ghost(Index2::new(0, 6), " 1");
        // An edit of another line syncs the ghost to the new revision, so the
        // acceptance still applies.
        editor.set_cursor(Index2::new(1, 6));
        editor.insert_at_cursor("x");
        editor.ghost_decide(0, true).unwrap();
        assert_eq!(editor.text(), "SELECT 1\nFROM tx");
    }

    #[test]
    fn hunks_group_adjacent_changes_and_mark_words() {
        let hunks = diff_hunks(
            "SELECT 1;\nFROM a\nWHERE x\nORDER BY 1;",
            "SELECT 1;\nFROM b\nWHERE y\nAND z\nORDER BY 1;",
            3,
            Duration::from_millis(100),
        );
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].line, 4);
        assert_eq!(hunks[0].old, 2);
        assert_eq!(hunks[0].new, ["FROM b", "WHERE y", "AND z"]);
        assert_eq!(hunks[0].old_marks[0], [(5, 6)]);
        assert_eq!(hunks[0].new_marks[0], [(5, 6)]);
    }
}
