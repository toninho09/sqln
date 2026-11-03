//! A diagram tab: the read of the two queries, the model, the picture, the view, and the
//! selected table.

use super::layout::{Direction, Layout};
use super::sql::{diagram_sql, parse_column, parse_key};
use super::{Focus, Model, mermaid};
use crate::catalog::RelationRef;
use crate::console::{Message, MessageLevel, Phase, RunClock};
use crate::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, StatementStatus,
};
use crate::grid::{Budget, Row};
use crate::sql::Dialect;
use crate::ui::text;

/// The columns and the rows of one step of `Shift` and an arrow.
const PAN_COLUMNS: i64 = 8;
const PAN_ROWS: i64 = 4;

pub struct DiagramTab {
    /// The place of the tab in the order of opening.
    pub opened: u64,
    /// The identity of the task of this tab. It shares the numbers of the consoles.
    pub id: ConsoleId,
    pub connection: ConnectionId,
    pub focus: Focus,
    pub phase: Phase,
    pub run: RunClock,
    pub message: Option<Message>,
    pub model: Model,
    pub layout: Layout,
    pub keys_only: bool,
    /// The entity of the selected box.
    pub selected: Option<usize>,
    /// The first column of the picture on screen.
    pub left: u32,
    /// The first row of the picture on screen.
    pub top: u32,
    /// True when a limit stopped the read, so the diagram can miss tables or keys.
    pub partial: bool,
    /// The size of the view in the last frame.
    view: (u32, u32),
    mark_width: u32,
    execution: Option<ExecutionId>,
    columns: Vec<Row>,
    keys: Vec<Row>,
    /// The budget of the rows that wait for the end of the read.
    bytes: usize,
}

impl DiagramTab {
    pub fn new(id: ConsoleId, connection: ConnectionId, focus: Focus, opened: u64) -> Self {
        Self {
            opened,
            id,
            connection,
            focus,
            phase: Phase::Idle,
            run: RunClock::default(),
            message: None,
            model: Model::default(),
            layout: Layout::default(),
            keys_only: false,
            selected: None,
            left: 0,
            top: 0,
            partial: false,
            view: (0, 0),
            mark_width: 1,
            execution: None,
            columns: Vec::new(),
            keys: Vec::new(),
            bytes: 0,
        }
    }

    /// Returns true when the tab shows this diagram.
    pub fn is(&self, connection: ConnectionId, focus: &Focus) -> bool {
        self.connection == connection && self.focus == *focus
    }

    pub fn label(&self) -> String {
        text::diagram_label(self.focus.name())
    }

    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Returns the two queries of the read. Redis has no diagram.
    pub fn statements(&self, dialect: Dialect) -> Option<Vec<String>> {
        diagram_sql(dialect, self.focus.schema()).map(Vec::from)
    }

    /// Records the execution that the manager accepted for the read.
    pub fn started(&mut self, execution: ExecutionId) {
        self.execution = Some(execution);
        self.phase = Phase::Waiting;
        self.message = None;
        self.partial = false;
        self.run.start();
    }

    fn is_current(&self, execution: ExecutionId) -> bool {
        self.execution == Some(execution)
    }

    pub fn on_event(&mut self, event: ExecEvent, budget: &Budget) {
        match event {
            ExecEvent::Started { execution, .. } if self.is_current(execution) => {
                if self.phase == Phase::Waiting {
                    self.phase = Phase::Running;
                }
            }
            ExecEvent::Rows { key, rows, bytes } => {
                // An old execution has no place, so its data leaves the budget.
                if !self.is_current(key.execution) || key.index > 1 {
                    budget.release(bytes);
                    return;
                }
                self.bytes += bytes;
                if key.index == 0 {
                    self.columns.extend(rows);
                } else {
                    self.keys.extend(rows);
                }
            }
            ExecEvent::ResultDone { key, limit, .. } if self.is_current(key.execution) => {
                self.partial |= limit != ReadLimit::None;
            }
            ExecEvent::StatementFinished {
                key,
                status: StatementStatus::Error(message),
                ..
            } if self.is_current(key.execution) => {
                self.message = Some(Message {
                    level: MessageLevel::Error,
                    text: message,
                });
            }
            ExecEvent::ExecutionFinished {
                execution,
                end,
                message,
                ..
            } if self.is_current(execution) => self.finish(end, message, budget),
            _ => {}
        }
    }

    fn finish(&mut self, end: ExecutionEnd, message: Option<String>, budget: &Budget) {
        self.execution = None;
        self.phase = Phase::Idle;
        self.run.finish(end);
        let columns = std::mem::take(&mut self.columns);
        let keys = std::mem::take(&mut self.keys);
        let note = |level, text: &str| {
            Some(Message {
                level,
                text: text.to_owned(),
            })
        };
        let note = match end {
            ExecutionEnd::Completed => {
                let columns = columns.iter().filter_map(parse_column).collect();
                let keys = keys.iter().filter_map(parse_key).collect();
                self.show(Model::build(columns, keys, &self.focus));
                if self.model.entities.is_empty() && self.message.is_none() {
                    note(MessageLevel::Info, text::DIAGRAM_EMPTY)
                } else {
                    None
                }
            }
            // The error of a statement is already in `message`.
            ExecutionEnd::Refused => None,
            ExecutionEnd::Failed => message.map(|text| Message {
                level: MessageLevel::Error,
                text,
            }),
            ExecutionEnd::Stopped => note(MessageLevel::Info, text::DIAGRAM_STOPPED),
            ExecutionEnd::TimedOut => note(MessageLevel::Error, text::TABLE_TIMEOUT),
            ExecutionEnd::CanceledBeforeStart => note(MessageLevel::Info, text::TABLE_CANCELED),
            ExecutionEnd::ConnectionClosed => note(MessageLevel::Error, text::TABLE_CLOSED),
        };
        if note.is_some() {
            self.message = note;
        }
        budget.release(std::mem::take(&mut self.bytes));
    }

    /// Shows a new model. The selected table stays selected when it is still there. A new
    /// diagram of a table selects that table, and a new diagram of a schema selects the
    /// box at the top left.
    fn show(&mut self, model: Model) {
        let chosen = self.selected_relation().or(match &self.focus {
            Focus::Table(table) => Some(table.clone()),
            Focus::Schema(_) => None,
        });
        self.model = model;
        self.selected = None;
        self.relayout();
        let found = chosen.and_then(|chosen| {
            self.model
                .entities
                .iter()
                .position(|entity| entity.relation == chosen)
        });
        let boxes = self.layout.boxes.iter().enumerate();
        let first = boxes.min_by_key(|(_, placed)| (placed.y, placed.x));
        self.selected = found.or(first.map(|(entity, _)| entity));
        self.keep_visible();
    }

    /// Returns the rows of a read in progress to the budget.
    pub fn release(&mut self, budget: &Budget) {
        self.columns.clear();
        self.keys.clear();
        budget.release(std::mem::take(&mut self.bytes));
    }

    fn relayout(&mut self) {
        self.layout = Layout::new(&self.model, self.keys_only, self.mark_width);
        self.keep_visible();
    }

    /// Gives the width of the key marks of the theme. A new width places the boxes again.
    pub fn set_mark_width(&mut self, width: u32) {
        if width != self.mark_width {
            self.mark_width = width;
            self.relayout();
        }
    }

    /// Shows only the key columns, or all the columns.
    pub fn toggle_keys_only(&mut self) {
        self.keys_only = !self.keys_only;
        self.relayout();
    }

    /// Selects the nearest box in a direction. Without a selection, the first box.
    pub fn select(&mut self, direction: Direction) {
        let next = match self.selected {
            Some(from) => self.layout.nearest(from, direction),
            None => (!self.layout.boxes.is_empty()).then_some(0),
        };
        if next.is_some() {
            self.selected = next;
            self.keep_visible();
        }
    }

    /// Selects the box at a cell of the view. It returns true when a box is there.
    pub fn select_at(&mut self, column: u16, row: u16) -> bool {
        let x = self.left + u32::from(column);
        let y = self.top + u32::from(row);
        match self.layout.box_at(x, y) {
            Some(entity) => {
                self.selected = Some(entity);
                true
            }
            None => false,
        }
    }

    /// Moves the view by a number of columns and rows, inside the picture.
    pub fn pan(&mut self, columns: i64, rows: i64) {
        let move_by = |start: u32, delta: i64| {
            u32::try_from((i64::from(start) + delta).max(0)).unwrap_or(u32::MAX)
        };
        self.left = move_by(self.left, columns);
        self.top = move_by(self.top, rows);
        self.clamp();
    }

    /// Moves the view by one step of `Shift` and an arrow.
    pub fn pan_step(&mut self, direction: Direction) {
        let (columns, rows) = match direction {
            Direction::Up => (0, -PAN_ROWS),
            Direction::Down => (0, PAN_ROWS),
            Direction::Left => (-PAN_COLUMNS, 0),
            Direction::Right => (PAN_COLUMNS, 0),
        };
        self.pan(columns, rows);
    }

    /// Moves the view by its height.
    pub fn page(&mut self, down: bool) {
        let rows = i64::from(self.view.1.max(1));
        self.pan(0, if down { rows } else { -rows });
    }

    /// Records the size of the view on screen.
    pub fn set_view(&mut self, width: u16, height: u16) {
        let view = (u32::from(width), u32::from(height));
        if view != self.view {
            self.view = view;
            self.keep_visible();
        }
    }

    /// Moves the view as little as possible to show the selected box. A box larger than
    /// the view shows its top left corner.
    fn keep_visible(&mut self) {
        let Some(placed) = self
            .selected
            .and_then(|entity| self.layout.boxes.get(entity))
        else {
            self.clamp();
            return;
        };
        let (width, height) = self.view;
        let fit = |start: u32, size: u32, place: u32, length: u32| {
            if place < start || length > size {
                place
            } else if place + length > start + size {
                place + length - size
            } else {
                start
            }
        };
        self.left = fit(self.left, width, placed.x, placed.width);
        self.top = fit(self.top, height, placed.y, placed.height);
        self.clamp();
    }

    fn clamp(&mut self) {
        let (width, height) = self.view;
        self.left = self.left.min(self.layout.width.saturating_sub(width));
        self.top = self.top.min(self.layout.height.saturating_sub(height));
    }

    /// Returns the table of the selected box.
    pub fn selected_relation(&self) -> Option<RelationRef> {
        let entity = self.model.entities.get(self.selected?)?;
        Some(entity.relation.clone())
    }

    /// Returns the Mermaid text of the boxes on screen.
    pub fn mermaid(&self) -> String {
        mermaid::to_mermaid(&self.model, self.keys_only)
    }
}
