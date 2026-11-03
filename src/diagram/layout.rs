//! Places the boxes of a diagram in layers and routes its lines between the layers.
//! The parent of a key stands to the left of the child. The result depends only on the
//! model, so the same schema always gives the same picture.

use std::collections::{BTreeSet, HashMap};

use unicode_width::UnicodeWidthStr;

use super::{Column, Entity, Model};
use crate::grid::printable;

/// The arms of a line cell. A glyph joins the arms of each line that passes the cell.
pub const ARM_UP: u8 = 1;
pub const ARM_RIGHT: u8 = 2;
pub const ARM_DOWN: u8 = 4;
pub const ARM_LEFT: u8 = 8;

const NAME_LIMIT: usize = 32;
const TYPE_LIMIT: usize = 24;
const TITLE_LIMIT: usize = 40;
/// The columns between two layers without lines.
const MIN_GUTTER: u32 = 6;
const ROW_GAP: u32 = 1;
/// The width at which the block of tables without keys starts a new row.
const LOOSE_WIDTH: u32 = 120;
const LOOSE_GAP: u32 = 3;
const SWEEPS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// The place of one box. Its rows are the columns that it shows, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Placed {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub rows: Vec<usize>,
    /// The width of the name of a column, after the mark.
    pub name_width: u32,
}

impl Placed {
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Returns the row of a column inside the box. A column that the box does not show
    /// uses the first row.
    fn attach(&self, column: Option<usize>) -> u32 {
        column
            .and_then(|column| self.rows.iter().position(|&row| row == column))
            .map_or(1, |place| 1 + place as u32)
    }
}

/// The part of one link in one cell. Several links can share a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinePiece {
    pub x: u32,
    pub arms: u8,
    pub link: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrow {
    pub x: u32,
    pub y: u32,
    /// True when the arrow points to the left.
    pub left: bool,
    pub link: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// One box per entity of the model, in the same order.
    pub boxes: Vec<Placed>,
    /// The pieces of the lines in each row, in the order of the column and then of the link.
    pub rows: Vec<Vec<LinePiece>>,
    pub arrows: Vec<Arrow>,
    pub width: u32,
    pub height: u32,
    /// The lines that could not keep their own track. The tests expect zero.
    pub conflicts: usize,
}

/// Returns the glyph that joins the arms of a line cell.
pub fn glyph(arms: u8) -> char {
    const VERTICAL: u8 = ARM_UP | ARM_DOWN;
    const HORIZONTAL: u8 = ARM_LEFT | ARM_RIGHT;
    match arms {
        0 => ' ',
        ARM_UP | ARM_DOWN | VERTICAL => '│',
        ARM_LEFT | ARM_RIGHT | HORIZONTAL => '─',
        x if x == ARM_DOWN | ARM_RIGHT => '┌',
        x if x == ARM_DOWN | ARM_LEFT => '┐',
        x if x == ARM_UP | ARM_RIGHT => '└',
        x if x == ARM_UP | ARM_LEFT => '┘',
        x if x == VERTICAL | ARM_RIGHT => '├',
        x if x == VERTICAL | ARM_LEFT => '┤',
        x if x == HORIZONTAL | ARM_DOWN => '┬',
        x if x == HORIZONTAL | ARM_UP => '┴',
        _ => '┼',
    }
}

/// The width of a name on the screen.
pub fn text_width(text: &str) -> usize {
    printable(text).width()
}

/// Returns the name of a stub box.
pub fn stub_name(entity: &Entity) -> String {
    format!("{}.{}", entity.relation.schema, entity.relation.relation)
}

fn shape(entity: &Entity, keys_only: bool, mark_width: u32) -> Placed {
    if entity.stub {
        let name = text_width(&stub_name(entity)).min(TITLE_LIMIT) as u32;
        return Placed {
            width: name + 4,
            height: 3,
            name_width: name,
            ..Placed::default()
        };
    }
    let rows = entity.shown(keys_only);
    let widest = |part: fn(&Column) -> &str, limit: usize| {
        rows.iter()
            .map(|&row| text_width(part(&entity.columns[row])))
            .max()
            .unwrap_or(0)
            .min(limit) as u32
    };
    let name_width = widest(|column| &column.name, NAME_LIMIT);
    let type_width = widest(|column| &column.ty, TYPE_LIMIT);
    // A row is " mark name  type ", between two borders.
    let inner = 1 + mark_width + 1 + name_width + 2 + type_width + 1;
    let title = text_width(&entity.relation.relation).min(TITLE_LIMIT) as u32 + 4;
    Placed {
        width: (inner + 2).max(title),
        height: 2 + (rows.len() as u32).max(1),
        rows,
        name_width,
        ..Placed::default()
    }
}

/// One item of a layer: a box, or the place where a line crosses the layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Box(usize),
    Pass,
}

/// The part of a line inside one gutter.
struct Segment {
    link: usize,
    /// The rows where the line meets the left layer.
    left: Vec<u32>,
    /// The rows where the line meets the right layer.
    right: Vec<u32>,
    low: u32,
    high: u32,
}

/// A line between two layers, as a chain of items.
struct Chain {
    link: usize,
    items: Vec<usize>,
    /// True when the parent is the first item, so the arrow is on the left.
    parent_first: bool,
}

impl Layout {
    /// Places the boxes and routes the lines. `mark_width` is the width of a key mark.
    pub fn new(model: &Model, keys_only: bool, mark_width: u32) -> Self {
        let mut boxes: Vec<Placed> = model
            .entities
            .iter()
            .map(|entity| shape(entity, keys_only, mark_width))
            .collect();
        let count = boxes.len();
        // The row of each end of a link, inside its box.
        let ends: Vec<(u32, u32)> = model
            .links
            .iter()
            .map(|link| {
                (
                    boxes[link.child].attach(link.child_columns.first().copied()),
                    boxes[link.parent].attach(link.parent_columns.first().copied().flatten()),
                )
            })
            .collect();

        let mut linked = vec![false; count];
        let mut children: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); count];
        for link in &model.links {
            linked[link.child] = true;
            linked[link.parent] = true;
            if link.child != link.parent {
                children[link.parent].insert(link.child);
            }
        }
        let reversed = back_edges(&children, &linked);
        let mut next: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); count];
        for (parent, list) in children.iter().enumerate() {
            for &child in list {
                if reversed.contains(&(parent, child)) {
                    next[child].insert(parent);
                } else {
                    next[parent].insert(child);
                }
            }
        }
        let entity_layer = layers(&next, &linked);

        // Items: one per linked box, and one per layer that a line crosses.
        let mut items: Vec<Item> = Vec::new();
        let mut item_layer: Vec<usize> = Vec::new();
        let mut item_of = vec![usize::MAX; count];
        for entity in (0..count).filter(|&entity| linked[entity]) {
            item_of[entity] = items.len();
            items.push(Item::Box(entity));
            item_layer.push(entity_layer[entity]);
        }
        let mut chains: Vec<Chain> = Vec::new();
        for (index, link) in model.links.iter().enumerate() {
            if link.child == link.parent {
                continue;
            }
            let parent_first = !reversed.contains(&(link.parent, link.child));
            let (first, last) = if parent_first {
                (link.parent, link.child)
            } else {
                (link.child, link.parent)
            };
            let mut chain = vec![item_of[first]];
            for layer in entity_layer[first] + 1..entity_layer[last] {
                chain.push(items.len());
                items.push(Item::Pass);
                item_layer.push(layer);
            }
            chain.push(item_of[last]);
            chains.push(Chain {
                link: index,
                items: chain,
                parent_first,
            });
        }

        let layer_count = item_layer.iter().max().map_or(0, |last| last + 1);
        let order = order_layers(&items, &item_layer, &chains, layer_count);

        // The row of a link inside an item.
        let offset = |item: usize, link: usize| -> u32 {
            match items[item] {
                Item::Pass => 0,
                Item::Box(entity) => {
                    let (child, parent) = ends[link];
                    if model.links[link].parent == entity {
                        parent
                    } else {
                        child
                    }
                }
            }
        };
        let height = |item: usize| match items[item] {
            Item::Box(entity) => boxes[entity].height,
            Item::Pass => 1,
        };

        // Vertical places: each item tries to face the items on its left.
        let mut left_of: Vec<Vec<(usize, usize)>> = vec![Vec::new(); items.len()];
        for chain in &chains {
            for pair in chain.items.windows(2) {
                left_of[pair[1]].push((pair[0], chain.link));
            }
        }
        let mut top = vec![0u32; items.len()];
        for layer in &order {
            let mut cursor = 0u32;
            for &item in layer {
                let wanted = &left_of[item];
                let desired = if wanted.is_empty() {
                    0
                } else {
                    let sum: i64 = wanted
                        .iter()
                        .map(|&(before, link)| {
                            i64::from(top[before] + offset(before, link))
                                - i64::from(offset(item, link))
                        })
                        .sum();
                    (sum / wanted.len() as i64).max(0) as u32
                };
                top[item] = cursor.max(desired);
                cursor = top[item] + height(item) + ROW_GAP;
            }
        }
        let pin = |item: usize, link: usize| top[item] + offset(item, link);

        // Segments per gutter. The gutter `g` is on the right of the layer `g`.
        let mut gutters: Vec<Vec<Segment>> = (0..layer_count).map(|_| Vec::new()).collect();
        for chain in &chains {
            for pair in chain.items.windows(2) {
                let (left, right) = (pin(pair[0], chain.link), pin(pair[1], chain.link));
                gutters[item_layer[pair[0]]].push(Segment {
                    link: chain.link,
                    left: vec![left],
                    right: vec![right],
                    low: left.min(right),
                    high: left.max(right),
                });
            }
        }
        for (index, link) in model.links.iter().enumerate() {
            if link.child != link.parent {
                continue;
            }
            let item = item_of[link.child];
            let (from, to) = (top[item] + ends[index].0, top[item] + ends[index].1);
            gutters[item_layer[item]].push(Segment {
                link: index,
                left: vec![from, to],
                right: Vec::new(),
                low: from.min(to),
                high: from.max(to),
            });
        }
        let mut conflicts = 0;
        let mut tracks: Vec<HashMap<usize, u32>> = Vec::new();
        let mut gutter_width = Vec::new();
        for (index, segments) in gutters.iter().enumerate() {
            let (assigned, count, broken) = assign_tracks(segments);
            conflicts += broken;
            tracks.push(assigned);
            let last = index + 1 == layer_count;
            gutter_width.push(if last && segments.is_empty() {
                0
            } else {
                MIN_GUTTER.max(2 * count + 3)
            });
        }

        // Horizontal places.
        let mut layer_width = vec![0u32; layer_count];
        for (item, kind) in items.iter().enumerate() {
            if let Item::Box(entity) = kind {
                let layer = item_layer[item];
                layer_width[layer] = layer_width[layer].max(boxes[*entity].width);
            }
        }
        let mut layer_x = vec![0u32; layer_count];
        for layer in 1..layer_count {
            layer_x[layer] = layer_x[layer - 1] + layer_width[layer - 1] + gutter_width[layer - 1];
        }
        for (item, kind) in items.iter().enumerate() {
            if let Item::Box(entity) = kind {
                boxes[*entity].x = layer_x[item_layer[item]];
                boxes[*entity].y = top[item];
            }
        }
        let track_x = |layer: usize, link: usize| -> Option<u32> {
            let track = *tracks[layer].get(&link)?;
            Some(layer_x[layer] + layer_width[layer] + 2 + 2 * track)
        };

        let mut layout = Self {
            conflicts,
            ..Self::default()
        };
        for chain in &chains {
            let link = &model.links[chain.link];
            let first = &boxes[if chain.parent_first {
                link.parent
            } else {
                link.child
            }];
            let last = &boxes[if chain.parent_first {
                link.child
            } else {
                link.parent
            }];
            let start_y = pin(chain.items[0], chain.link);
            let end_y = pin(*chain.items.last().unwrap_or(&0), chain.link);
            let right = first.x + first.width;
            let start = if chain.parent_first {
                layout.arrow(right, start_y, true, chain.link);
                right + 1
            } else {
                layout.port(right - 1, start_y, chain.link);
                right - 1
            };
            let mut points = vec![(start, start_y)];
            for pair in chain.items.windows(2) {
                let (from, to) = (pin(pair[0], chain.link), pin(pair[1], chain.link));
                if from != to
                    && let Some(x) = track_x(item_layer[pair[0]], chain.link)
                {
                    points.push((x, from));
                    points.push((x, to));
                }
            }
            let end = if chain.parent_first {
                layout.port(last.x, end_y, chain.link);
                last.x
            } else {
                layout.arrow(last.x - 1, end_y, false, chain.link);
                last.x - 2
            };
            points.push((end, end_y));
            layout.trace(&points, chain.link);
        }
        for (index, link) in model.links.iter().enumerate() {
            if link.child != link.parent {
                continue;
            }
            let placed = &boxes[link.child];
            let (from, to) = (placed.y + ends[index].0, placed.y + ends[index].1);
            let item = item_of[link.child];
            let Some(x) = track_x(item_layer[item], index).filter(|_| from != to) else {
                continue;
            };
            let right = placed.x + placed.width;
            layout.port(right - 1, from, index);
            layout.arrow(right, to, true, index);
            layout.trace(
                &[(right - 1, from), (x, from), (x, to), (right + 1, to)],
                index,
            );
        }

        place_loose(&mut boxes, &linked);
        layout.boxes = boxes;
        layout.sort_rows();
        layout.measure();
        layout
    }

    fn add(&mut self, x: u32, y: u32, arms: u8, link: usize) {
        let row = y as usize;
        if self.rows.len() <= row {
            self.rows.resize_with(row + 1, Vec::new);
        }
        self.rows[row].push(LinePiece { x, arms, link });
    }

    /// Sorts the pieces of each row. The pieces of one link in one cell become one piece.
    fn sort_rows(&mut self) {
        for row in &mut self.rows {
            row.sort_unstable_by_key(|piece| (piece.x, piece.link));
            row.dedup_by(|later, kept| {
                let same = later.x == kept.x && later.link == kept.link;
                if same {
                    kept.arms |= later.arms;
                }
                same
            });
        }
    }

    /// Lists the line cells of a row: the column, the joined arms, and the pieces.
    pub fn cells(&self, y: u32) -> impl Iterator<Item = (u32, u8, &[LinePiece])> {
        let row = self.rows.get(y as usize).map_or(&[][..], Vec::as_slice);
        row.chunk_by(|a, b| a.x == b.x).map(|pieces| {
            let arms = pieces.iter().fold(0, |arms, piece| arms | piece.arms);
            (pieces[0].x, arms, pieces)
        })
    }

    /// Returns the line cell at a place of the picture.
    pub fn cell(&self, x: u32, y: u32) -> Option<(u8, &[LinePiece])> {
        self.cells(y)
            .find(|(column, ..)| *column == x)
            .map(|(_, arms, pieces)| (arms, pieces))
    }

    /// Marks the border of a child box where a line leaves it.
    fn port(&mut self, x: u32, y: u32, link: usize) {
        self.add(x, y, ARM_UP | ARM_DOWN, link);
    }

    fn arrow(&mut self, x: u32, y: u32, left: bool, link: usize) {
        self.arrows.push(Arrow { x, y, left, link });
    }

    /// Adds the arms of a line through the points, in order.
    fn trace(&mut self, points: &[(u32, u32)], link: usize) {
        for pair in points.windows(2) {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            if ay == by {
                let (low, high) = (ax.min(bx), ax.max(bx));
                for x in low..=high {
                    let arms =
                        if x > low { ARM_LEFT } else { 0 } | if x < high { ARM_RIGHT } else { 0 };
                    self.add(x, ay, arms, link);
                }
            } else {
                let (low, high) = (ay.min(by), ay.max(by));
                for y in low..=high {
                    let arms =
                        if y > low { ARM_UP } else { 0 } | if y < high { ARM_DOWN } else { 0 };
                    self.add(ax, y, arms, link);
                }
            }
        }
    }

    fn measure(&mut self) {
        let boxes = self
            .boxes
            .iter()
            .map(|placed| (placed.x + placed.width, placed.y + placed.height));
        let lines = self
            .rows
            .iter()
            .enumerate()
            .flat_map(|(y, row)| row.last().map(|piece| (piece.x + 1, y as u32 + 1)));
        let arrows = self.arrows.iter().map(|arrow| (arrow.x + 1, arrow.y + 1));
        for (right, bottom) in boxes.chain(lines).chain(arrows) {
            self.width = self.width.max(right);
            self.height = self.height.max(bottom);
        }
    }

    /// Returns the box nearest to a box in a direction. Only a box that starts past the
    /// edge of the first box in that direction counts. The gap to it and twice the side
    /// distance between the centers give its rank.
    pub fn nearest(&self, from: usize, direction: Direction) -> Option<usize> {
        let first = self.boxes.get(from)?;
        // The edges and the center, in units of half a cell.
        let span = |placed: &Placed| {
            let (x, y) = (i64::from(placed.x), i64::from(placed.y));
            let (width, height) = (i64::from(placed.width), i64::from(placed.height));
            (
                2 * x,
                2 * (x + width),
                2 * y,
                2 * (y + height),
                2 * x + width,
                2 * y + height,
            )
        };
        let (left, right, top, bottom, center_x, center_y) = span(first);
        self.boxes
            .iter()
            .enumerate()
            .filter(|&(index, _)| index != from)
            .filter_map(|(index, placed)| {
                let (l, r, t, b, x, y) = span(placed);
                let (gap, side) = match direction {
                    Direction::Up => (top - b, x - center_x),
                    Direction::Down => (t - bottom, x - center_x),
                    Direction::Left => (left - r, y - center_y),
                    Direction::Right => (l - right, y - center_y),
                };
                (gap >= 0).then_some((gap + 2 * side.abs(), index))
            })
            .min()
            .map(|(_, index)| index)
    }

    /// Returns the box at a cell of the picture.
    pub fn box_at(&self, x: u32, y: u32) -> Option<usize> {
        self.boxes.iter().position(|placed| placed.contains(x, y))
    }
}

/// Finds the keys that close a cycle, with a depth search in entity order.
fn back_edges(children: &[BTreeSet<usize>], linked: &[bool]) -> BTreeSet<(usize, usize)> {
    const NEW: u8 = 0;
    const OPEN: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![NEW; children.len()];
    let mut back = BTreeSet::new();
    for root in 0..children.len() {
        if !linked[root] || state[root] != NEW {
            continue;
        }
        state[root] = OPEN;
        let mut stack: Vec<(usize, Vec<usize>, usize)> =
            vec![(root, children[root].iter().copied().collect(), 0)];
        while let Some((node, list, at)) = stack.last_mut() {
            let Some(&next) = list.get(*at) else {
                state[*node] = DONE;
                stack.pop();
                continue;
            };
            *at += 1;
            let node = *node;
            match state[next] {
                NEW => {
                    state[next] = OPEN;
                    stack.push((next, children[next].iter().copied().collect(), 0));
                }
                OPEN => {
                    back.insert((node, next));
                }
                _ => {}
            }
        }
    }
    back
}

/// Gives each linked entity the length of its longest path from a source.
fn layers(next: &[BTreeSet<usize>], linked: &[bool]) -> Vec<usize> {
    let mut entering = vec![0usize; next.len()];
    for list in next {
        for &target in list {
            entering[target] += 1;
        }
    }
    let mut ready: BTreeSet<usize> = (0..next.len())
        .filter(|&entity| linked[entity] && entering[entity] == 0)
        .collect();
    let mut layer = vec![0usize; next.len()];
    while let Some(entity) = ready.pop_first() {
        for &target in &next[entity] {
            layer[target] = layer[target].max(layer[entity] + 1);
            entering[target] -= 1;
            if entering[target] == 0 {
                ready.insert(target);
            }
        }
    }
    layer
}

/// Orders the items of each layer to cut the crossings: the order of the model first, and
/// then sweeps that move each item to the mean place of its neighbors.
fn order_layers(
    items: &[Item],
    item_layer: &[usize],
    chains: &[Chain],
    layer_count: usize,
) -> Vec<Vec<usize>> {
    let mut order: Vec<Vec<usize>> = vec![Vec::new(); layer_count];
    for item in 0..items.len() {
        order[item_layer[item]].push(item);
    }
    let mut place = vec![0usize; items.len()];
    for layer in &order {
        for (index, &item) in layer.iter().enumerate() {
            place[item] = index;
        }
    }
    let mut before: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    for chain in chains {
        for pair in chain.items.windows(2) {
            after[pair[0]].push(pair[1]);
            before[pair[1]].push(pair[0]);
        }
    }
    for _ in 0..SWEEPS {
        for layer in order.iter_mut().skip(1) {
            sort_by_neighbors(layer, &mut place, &before);
        }
        for layer in order.iter_mut().rev().skip(1) {
            sort_by_neighbors(layer, &mut place, &after);
        }
    }
    order
}

fn sort_by_neighbors(layer: &mut Vec<usize>, place: &mut [usize], neighbors: &[Vec<usize>]) {
    let mut keyed: Vec<(f64, usize, usize)> = layer
        .iter()
        .map(|&item| {
            let list = &neighbors[item];
            let key = if list.is_empty() {
                place[item] as f64
            } else {
                list.iter().map(|&other| place[other] as f64).sum::<f64>() / list.len() as f64
            };
            (key, place[item], item)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    *layer = keyed.into_iter().map(|(_, _, item)| item).collect();
    for (index, &item) in layer.iter().enumerate() {
        place[item] = index;
    }
}

/// Gives each vertical part of a gutter a track, from the left. A line that meets the left
/// layer on a row where another line meets the right layer must take a track on the left
/// of that line, or the two horizontal parts would share cells. Returns the tracks, their
/// count, and the number of rules that a cycle broke.
fn assign_tracks(segments: &[Segment]) -> (HashMap<usize, u32>, u32, usize) {
    let vertical: Vec<usize> = {
        let mut list: Vec<usize> = (0..segments.len())
            .filter(|&index| segments[index].low != segments[index].high)
            .collect();
        list.sort_by_key(|&index| {
            let segment = &segments[index];
            (segment.low, segment.high, segment.link)
        });
        list
    };
    let mut on_right: HashMap<u32, Vec<usize>> = HashMap::new();
    for &index in &vertical {
        for &row in &segments[index].right {
            on_right.entry(row).or_default().push(index);
        }
    }
    let mut after: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut waiting: HashMap<usize, usize> = HashMap::new();
    for &index in &vertical {
        for row in &segments[index].left {
            for &other in on_right.get(row).into_iter().flatten() {
                if other != index {
                    after.entry(index).or_default().push(other);
                    *waiting.entry(other).or_default() += 1;
                }
            }
        }
    }

    let mut tracks = HashMap::new();
    let mut track = 0u32;
    let mut broken = 0;
    let mut left = vertical.len();
    while left > 0 {
        let mut placed = Vec::new();
        let mut end: Option<u32> = None;
        for &index in &vertical {
            let segment = &segments[index];
            if tracks.contains_key(&segment.link)
                || waiting.get(&index).copied().unwrap_or(0) > 0
                || end.is_some_and(|end| segment.low <= end + 1)
            {
                continue;
            }
            tracks.insert(segment.link, track);
            end = Some(segment.high);
            placed.push(index);
        }
        if placed.is_empty() {
            // A cycle of rules: the first waiting part takes this track anyway.
            if let Some(&index) = vertical
                .iter()
                .find(|&&index| !tracks.contains_key(&segments[index].link))
            {
                waiting.insert(index, 0);
                broken += 1;
            }
            continue;
        }
        for index in &placed {
            for other in after.get(index).into_iter().flatten() {
                if let Some(count) = waiting.get_mut(other) {
                    *count = count.saturating_sub(1);
                }
            }
        }
        left -= placed.len();
        track += 1;
    }
    (tracks, track, broken)
}

/// Places the boxes without keys in rows under the layers, in model order.
fn place_loose(boxes: &mut [Placed], linked: &[bool]) {
    let layered = |placed: &Placed| (placed.x + placed.width, placed.y + placed.height);
    let (width, height) = boxes
        .iter()
        .zip(linked)
        .filter(|(_, linked)| **linked)
        .fold((0, 0), |(width, height), (placed, _)| {
            let (right, bottom) = layered(placed);
            (width.max(right), height.max(bottom))
        });
    let wrap = width.max(LOOSE_WIDTH);
    let mut row_top = if height > 0 { height + 2 } else { 0 };
    let (mut x, mut row_height) = (0u32, 0u32);
    for (placed, _) in boxes.iter_mut().zip(linked).filter(|(_, linked)| !**linked) {
        if x > 0 && x + placed.width > wrap {
            row_top += row_height + ROW_GAP;
            x = 0;
            row_height = 0;
        }
        placed.x = x;
        placed.y = row_top;
        x += placed.width + LOOSE_GAP;
        row_height = row_height.max(placed.height);
    }
}
