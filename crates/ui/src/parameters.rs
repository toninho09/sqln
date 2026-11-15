//! Per-console parameter fields. Values stay in memory and never change editor text.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use crate::line_input::{Edit, LineInput};
use crate::sql::Dialect;
use crate::sql::parameters::{self, Kind, Parameter};

#[derive(Debug, Default)]
pub struct Field {
    pub name: String,
    pub kind: Kind,
    pub input: LineInput,
    pub set: bool,
    pub invalid: bool,
}

impl Field {
    pub fn parameter(&self) -> Result<Parameter, String> {
        let value = self.input.text();
        let reason = if !self.set {
            Some("Set the value. Press Enter to use empty text.")
        } else {
            match self.kind {
                Kind::Number if !parameters::is_number(value) => Some("Enter a valid number."),
                Kind::Boolean if !matches!(value, "true" | "false") => Some("Enter true or false."),
                _ => None,
            }
        };
        if let Some(reason) = reason {
            return Err(format!(":{}: {reason}", self.name));
        }
        Ok(Parameter {
            name: self.name.clone(),
            kind: self.kind,
            value: value.to_owned(),
        })
    }
}

#[derive(Default)]
pub struct Panel {
    pub fields: Vec<Field>,
    pub selected: usize,
    /// Type, value, or Set/Unset.
    pub column: usize,
    pub collapsed: bool,
    pub scroll: usize,
    pub error: Option<String>,
    observed: Option<(u64, Dialect)>,
    scanned: Option<(u64, Dialect)>,
    changed: Option<Instant>,
    scan: Option<(u64, Dialect, Receiver<Vec<String>>)>,
}

impl Panel {
    /// Replaces the fields and returns true when the name list changed.
    pub fn update_names(&mut self, names: Vec<String>) -> bool {
        let changed = self
            .fields
            .iter()
            .map(|field| field.name.clone())
            .collect::<Vec<_>>()
            != names;
        let selected = self
            .fields
            .get(self.selected)
            .map(|field| field.name.clone());
        let mut old: std::collections::HashMap<_, _> = std::mem::take(&mut self.fields)
            .into_iter()
            .map(|field| (field.name.clone(), field))
            .collect();
        self.fields = names
            .into_iter()
            .map(|name| {
                old.remove(&name).unwrap_or(Field {
                    name,
                    ..Field::default()
                })
            })
            .collect();
        self.selected = selected
            .and_then(|name| self.fields.iter().position(|field| field.name == name))
            .unwrap_or(self.selected)
            .min(self.fields.len().saturating_sub(1));
        self.scroll = self.scroll.min(self.fields.len().saturating_sub(1));
        if self.fields.is_empty() {
            self.error = None;
        }
        changed
    }

    /// Runs lexical detection off the interface thread after 200 ms without an edit.
    pub fn poll(
        &mut self,
        revision: u64,
        dialect: Dialect,
        now: Instant,
        text: impl FnOnce() -> String,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send>),
    ) -> bool {
        if dialect.is_command_line() {
            let changed = !self.fields.is_empty();
            self.update_names(Vec::new());
            self.scan = None;
            return changed;
        }
        let current = (revision, dialect);
        if self.observed != Some(current) {
            self.observed = Some(current);
            self.changed = Some(now);
        }
        let mut changed = false;
        if let Some((revision, dialect, receiver)) = &self.scan {
            match receiver.try_recv() {
                Ok(names) => {
                    let scanned = (*revision, *dialect);
                    self.scan = None;
                    if scanned == current {
                        changed |= self.update_names(names);
                        self.scanned = Some(current);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.scan = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.scan.is_none()
            && self.scanned != Some(current)
            && self
                .changed
                .is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_millis(200))
        {
            let sql = text();
            let (sender, receiver) = mpsc::channel();
            self.scan = Some((revision, dialect, receiver));
            spawn(Box::new(move || {
                let _ = sender.send(parameters::names(&sql, dialect));
            }));
        }
        changed
    }

    /// Captures only the names in the selected execution scope.
    pub fn values(
        &mut self,
        statements: &[String],
        dialect: Dialect,
    ) -> Result<Vec<Parameter>, String> {
        for field in &mut self.fields {
            field.invalid = false;
        }
        let mut values = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut failure = None;
        for sql in statements {
            for name in parameters::names(sql, dialect) {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let Some(index) = self.fields.iter().position(|field| field.name == name) else {
                    return Err(format!("Set parameter :{name} in the Parameters panel."));
                };
                match self.fields[index].parameter() {
                    Ok(value) => values.push(value),
                    Err(reason) => {
                        self.fields[index].invalid = true;
                        if failure.is_none() {
                            self.selected = index;
                            failure = Some(reason);
                        }
                    }
                }
            }
        }
        self.error.clone_from(&failure);
        if let Some(reason) = failure {
            self.collapsed = false;
            self.column = 1;
            Err(reason)
        } else {
            // Validate driver-specific number ranges and marker combinations before execution.
            for sql in statements {
                if let Err(reason) = parameters::bind(sql, dialect, &values) {
                    self.error = Some(reason.clone());
                    self.collapsed = false;
                    return Err(reason);
                }
            }
            Ok(values)
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        let count = self.fields.len();
        if count == 0 {
            return;
        }
        match key.code {
            KeyCode::Tab => {
                let next = (self.selected * 3 + self.column + 1) % (count * 3);
                self.selected = next / 3;
                self.column = next % 3;
            }
            KeyCode::BackTab => {
                let next = (self.selected * 3 + self.column + count * 3 - 1) % (count * 3);
                self.selected = next / 3;
                self.column = next % 3;
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(count - 1),
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(4),
            KeyCode::PageDown => self.selected = (self.selected + 4).min(count - 1),
            KeyCode::Enter | KeyCode::Char(' ') if self.column != 1 => {
                let field = &mut self.fields[self.selected];
                if self.column == 0 {
                    field.kind = field.kind.next();
                    field.set = field.kind == Kind::Null || !field.input.text().is_empty();
                } else {
                    field.set = !field.set;
                }
                field.invalid = false;
            }
            KeyCode::Enter => {
                let field = &mut self.fields[self.selected];
                field.set = true;
                field.invalid = field.parameter().is_err();
                self.error = field.parameter().err();
            }
            _ if self.column == 1 => {
                let field = &mut self.fields[self.selected];
                if field.kind != Kind::Null
                    && matches!(
                        field.input.on_key(key),
                        Edit::Wrote | Edit::Removed | Edit::Typed
                    )
                {
                    field.set = true;
                    field.invalid = false;
                    self.error = None;
                }
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, text: &str) {
        if self.column == 1
            && let Some(field) = self.fields.get_mut(self.selected)
            && field.kind != Kind::Null
        {
            field.input.insert(text);
            field.set = true;
            field.invalid = false;
            self.error = None;
        }
    }
}
