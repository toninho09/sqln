use super::{Output, number, position};
use crate::grid::{ResultTable, Value, printable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
    Bar,
    Line,
}

struct Point {
    row: usize,
    y: f64,
}

pub(super) struct Chart {
    x: usize,
    y: usize,
    kind: ChartKind,
    complete: bool,
    row: usize,
    points: Vec<Point>,
    min: f64,
    max: f64,
    rendered: usize,
    header: bool,
}

impl Chart {
    pub fn new(x: usize, y: usize, kind: ChartKind, complete: bool) -> Self {
        Self {
            x,
            y,
            kind,
            complete,
            row: 0,
            points: Vec::new(),
            min: 0.0,
            max: 0.0,
            rendered: 0,
            header: false,
        }
    }

    pub fn step(&mut self, table: &ResultTable, output: &mut Output) -> Result<bool, &'static str> {
        if self.row < table.rows.len() {
            let row = &table.rows[self.row];
            if let (Some(_), Some(y)) = (
                row.get(self.x).and_then(number),
                row.get(self.y).and_then(number),
            ) {
                output.memory.reserve(std::mem::size_of::<Point>() * 2)?;
                self.min = self.min.min(y);
                self.max = self.max.max(y);
                self.points.push(Point { row: self.row, y });
            }
            self.row += 1;
            return Ok(false);
        }
        if !self.header {
            self.header = true;
            if self.points.is_empty() {
                return Err("Chart canceled: no finite numeric pairs. Select two numeric columns.");
            }
            output.line(&format!(
                "X: {}",
                printable(table.columns.get(self.x).ok_or("X column not found.")?)
            ))?;
            output.line(&format!(
                "Y: {}",
                printable(table.columns.get(self.y).ok_or("Y column not found.")?)
            ))?;
            output.line("Order: result rows. Repeated X values stay separate.")?;
            output.line("X positions show occurrences, not numeric distances.")?;
            output.line("Plot positions use approximate numbers.")?;
            output.line(&format!(
                "Points: {} / {} loaded rows; skipped: {}",
                self.points.len(),
                table.rows.len(),
                table.rows.len() - self.points.len()
            ))?;
            if !self.complete {
                output.line("Scope: loaded rows only (incomplete result)")?;
            }
            output.line(&format!("Y scale: {:.4e} .. {:.4e}", self.min, self.max))?;
            return Ok(false);
        }
        if self.rendered == self.points.len() {
            return Ok(true);
        }
        match self.kind {
            ChartKind::Bar => {
                let point = &self.points[self.rendered];
                let zero = position(0.0, self.min, self.max, 32);
                let value = position(point.y, self.min, self.max, 32);
                let mut bar = vec![b' '; 32];
                for cell in &mut bar[zero.min(value)..=zero.max(value)] {
                    *cell = b'#';
                }
                bar[zero] = b'|';
                bar[value] = b'*';
                output.line(&format!(
                    "Row {}: [{}]",
                    point.row + 1,
                    String::from_utf8(bar).unwrap()
                ))?;
                point_label(output, table, point.row, self.x, self.y)?;
                self.rendered += 1;
            }
            ChartKind::Line => {
                let end = (self.rendered + 48).min(self.points.len());
                let points = &self.points[self.rendered..end];
                output.line(&format!(
                    "Line: points {}..{} (one position per occurrence)",
                    self.rendered + 1,
                    end
                ))?;
                let mut plot = vec![vec![b' '; points.len()]; 10];
                let mut last = None;
                for (x, point) in points.iter().enumerate() {
                    let y = 9 - position(point.y, self.min, self.max, 10);
                    if let Some(previous) = last {
                        for row in plot
                            .iter_mut()
                            .take(y.max(previous) + 1)
                            .skip(y.min(previous))
                        {
                            row[x] = b':';
                        }
                    }
                    plot[y][x] = b'*';
                    last = Some(y);
                }
                for row in plot {
                    output.line(&format!("|{}|", String::from_utf8(row).unwrap()))?;
                }
                for point in points {
                    point_label(output, table, point.row, self.x, self.y)?;
                }
                self.rendered = end;
            }
        }
        Ok(false)
    }
}

fn point_label(
    output: &mut Output,
    table: &ResultTable,
    row: usize,
    x: usize,
    y: usize,
) -> Result<(), &'static str> {
    let value = |column| table.rows[row].get(column).unwrap_or(Value::Null).display();
    output.line(&format!("  Row {}: X={} Y={}", row + 1, value(x), value(y)))
}
