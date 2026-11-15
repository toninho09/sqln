//! Bounded literal search in a cell value, including its hexadecimal form.

use std::collections::VecDeque;
use std::ops::Range;
use std::time::Instant;

use crate::find::Matcher;
use crate::line_input::LineInput;

use super::Content;

const MAX_MATCHES: usize = 1_000_000;

#[derive(Debug, Default)]
pub struct TextSearch {
    pub input: LineInput,
    matcher: Matcher,
    starts: VecDeque<usize>,
    length: usize,
    offset: usize,
    pub matches: Vec<Range<usize>>,
    pub current: usize,
    pub partial: bool,
    done: bool,
}

impl TextSearch {
    pub fn restart(&mut self) {
        self.matcher = Matcher::new(self.input.text());
        self.length = self.input.text().chars().count();
        self.starts.clear();
        self.offset = 0;
        self.matches.clear();
        self.current = 0;
        self.done = self.length == 0;
        self.partial = !self.done;
    }

    pub fn has_work(&self) -> bool {
        !self.done
    }

    pub fn current_match(&self) -> Option<Range<usize>> {
        self.matches.get(self.current).cloned()
    }

    pub fn advance(&mut self, forward: bool) -> Option<Range<usize>> {
        if !self.matches.is_empty() {
            self.current = (self.current + if forward { 1 } else { self.matches.len() - 1 })
                % self.matches.len();
        }
        self.current_match()
    }

    pub(super) fn scan(&mut self, source: &Content, deadline: Instant) -> Option<usize> {
        let first = self.matches.is_empty();
        while !self.done && Instant::now() < deadline {
            for _ in 0..512 {
                let Some((c, next)) = source.search_char(self.offset) else {
                    self.done = true;
                    self.partial = false;
                    break;
                };
                let Some(found) = self.matcher.push(c) else {
                    continue;
                };
                self.starts.push_back(self.offset);
                if self.starts.len() > self.length {
                    self.starts.pop_front();
                }
                self.offset = next;
                if found {
                    self.matches.push(*self.starts.front().unwrap()..next);
                    self.matcher.reset();
                    if first {
                        return self.matches.first().map(|range| range.start);
                    }
                    if self.matches.len() == MAX_MATCHES {
                        self.done = true;
                        break;
                    }
                }
            }
        }
        first
            .then(|| self.current_match().map(|range| range.start))
            .flatten()
    }
}
