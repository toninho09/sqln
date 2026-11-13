//! Mouse input for the editor scrollbars.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use std::ops::Range;

use super::{EditorMode, SqlEditor, buffer, cells};

#[derive(Clone, Debug)]
pub struct Scrollbar {
    area: Rect,
    horizontal: bool,
    maximum: usize,
    position: usize,
    thumb: Range<usize>,
}

impl Scrollbar {
    pub(crate) fn new(
        area: Rect,
        horizontal: bool,
        maximum: usize,
        position: usize,
        thumb: Range<usize>,
    ) -> Self {
        Self {
            area,
            horizontal,
            maximum,
            position,
            thumb,
        }
    }

    fn coordinate(&self, mouse: MouseEvent) -> usize {
        if self.horizontal {
            usize::from(mouse.column.saturating_sub(self.area.x))
        } else {
            usize::from(mouse.row.saturating_sub(self.area.y))
        }
    }

    fn target(&self, mouse: MouseEvent, grab: usize) -> usize {
        let length = usize::from(if self.horizontal {
            self.area.width
        } else {
            self.area.height
        });
        let travel = length.saturating_sub(self.thumb.len());
        if travel == 0 {
            return 0;
        }
        let place = self.coordinate(mouse).saturating_sub(grab).min(travel);
        if place == self.thumb.start {
            return self.position.min(self.maximum);
        }
        place.saturating_mul(self.maximum) / travel
    }
}

#[derive(Clone)]
pub(super) struct Drag {
    track: Scrollbar,
    grab: usize,
}

impl SqlEditor {
    pub fn set_scrollbars(&mut self, vertical: Option<Scrollbar>, horizontal: Option<Scrollbar>) {
        if self.scroll_drag.as_ref().is_some_and(|drag| {
            let next = if drag.track.horizontal {
                &horizontal
            } else {
                &vertical
            };
            next.as_ref()
                .is_none_or(|track| track.area != drag.track.area)
        }) {
            self.scroll_drag = None;
        }
        self.scrollbars = [vertical, horizontal];
    }

    pub fn dragging_scrollbar(&self) -> bool {
        self.scroll_drag.is_some()
    }

    pub(super) fn on_scrollbar_mouse(&mut self, mouse: MouseEvent) -> bool {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let position = Position::new(mouse.column, mouse.row);
                let Some(track) = self
                    .scrollbars
                    .iter()
                    .flatten()
                    .find(|track| track.area.contains(position))
                    .cloned()
                else {
                    return false;
                };
                let coordinate = track.coordinate(mouse);
                let on_thumb = track.thumb.contains(&coordinate);
                let grab = if on_thumb {
                    coordinate - track.thumb.start
                } else {
                    track.thumb.len() / 2
                };
                if !on_thumb {
                    self.scroll_to(&track, track.target(mouse, grab));
                }
                self.scroll_drag = Some(Drag { track, grab });
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(drag) = self.scroll_drag.clone() else {
                    return false;
                };
                self.scroll_to(&drag.track, drag.track.target(mouse, drag.grab));
                true
            }
            MouseEventKind::Up(MouseButton::Left) => self.scroll_drag.take().is_some(),
            _ => false,
        }
    }

    pub(super) fn restore_scroll_selection(&mut self) {
        if let Some(cursor) = self.scroll_selection_end.take() {
            self.state.cursor = cursor;
        }
    }

    fn scroll_to(&mut self, track: &Scrollbar, target: usize) {
        if self.keyboard == super::KeyboardMode::Conventional && self.anchor.is_some() {
            self.scroll_selection_end.get_or_insert(self.state.cursor);
        }
        let (left, _) = self.scroll();
        if track.horizontal {
            let line = buffer::row(&self.state.lines, self.state.cursor.row);
            let mut width = 0;
            let column = line
                .iter()
                .take_while(|c| {
                    width += cells(**c);
                    width <= target
                })
                .count();
            self.scroll_sideways(column as isize - left as isize);
        } else {
            let height = self
                .text_area
                .map_or(1, |area| usize::from(area.height).max(1));
            let top = target.min(buffer::last_row(&self.state.lines));
            let row = self.state.cursor.row.clamp(
                top,
                (top + height - 1).min(buffer::last_row(&self.state.lines)),
            );
            let len = buffer::row(&self.state.lines, row).len();
            let last = if self.state.mode == EditorMode::Normal {
                len.saturating_sub(1)
            } else {
                len
            };
            self.state.cursor.row = row;
            self.state.cursor.col = self.state.cursor.col.min(last);
            self.preferred_col = None;
            self.state.set_viewport_offset(left, top);
        }
    }
}
