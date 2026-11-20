//! The order of the saved connections in the explorer, and their folders.
//!
//! The order is the order of the `connections` list of each file: the global file
//! first, then the project file. A folder belongs to one scope, and it stands at the place
//! of its first connection. Each change gives the new order of one file.

use crate::connection::Connection;
use crate::store::Scope;
use crate::store::config::FolderRef;
use crate::text;

/// One item at the top level of the tree. The numbers index the connection list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Connection(usize),
    Folder {
        folder: FolderRef,
        members: Vec<usize>,
    },
}

/// The item that a change moves, or the place where it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Connection(usize),
    Folder(FolderRef),
}

/// The new order of the connections of one file, with the folder of each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrangement {
    pub scope: Scope,
    pub order: Vec<(String, Option<String>)>,
}

/// Returns the folder of a saved connection.
pub fn folder_of(connection: &Connection) -> Option<FolderRef> {
    Some(FolderRef {
        scope: connection.scope()?,
        name: connection.folder()?.to_owned(),
    })
}

/// Returns the order of the connections of one scope, as the list holds them.
pub fn current(connections: &[Connection], scope: Scope) -> Arrangement {
    let order = connections
        .iter()
        .filter(|connection| connection.scope() == Some(scope))
        .filter_map(|connection| {
            let id = connection.key.as_ref()?.id.clone();
            Some((id, connection.folder().map(str::to_owned)))
        })
        .collect();
    Arrangement { scope, order }
}

/// Applies a new order to the list: the folders, then the places in the scope.
pub fn apply(connections: &mut [Connection], arrangement: &Arrangement) {
    let rank = |connection: &Connection| {
        let id = &connection.key.as_ref()?.id;
        arrangement.order.iter().position(|(own, _)| own == id)
    };
    for connection in connections.iter_mut() {
        if connection.scope() != Some(arrangement.scope) {
            continue;
        }
        let Some(place) = rank(connection) else {
            continue;
        };
        if let Some(definition) = &mut connection.definition {
            definition.folder.clone_from(&arrangement.order[place].1);
        }
    }
    let keys: std::collections::HashMap<_, _> = connections
        .iter()
        .enumerate()
        .map(|(place, connection)| {
            let within = if connection.scope() == Some(arrangement.scope) {
                rank(connection).unwrap_or(usize::MAX)
            } else {
                place
            };
            (connection.id, (scope_rank(connection.scope()), within))
        })
        .collect();
    connections.sort_by_key(|connection| keys[&connection.id]);
}

/// Keeps the connections without a file first, such as the target of the command line,
/// then the global connections, then the project connections. The order in each group
/// stays.
pub fn sort_by_scope(connections: &mut [Connection]) {
    connections.sort_by_key(|connection| scope_rank(connection.scope()));
}

fn scope_rank(scope: Option<Scope>) -> u8 {
    match scope {
        None => 0,
        Some(Scope::Global) => 1,
        Some(Scope::Project) => 2,
    }
}

/// Lists the items at the top level, in the order of the connection list.
pub fn top_items(connections: &[Connection]) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for (index, connection) in connections.iter().enumerate() {
        let Some(folder) = folder_of(connection) else {
            items.push(Item::Connection(index));
            continue;
        };
        let known = items.iter_mut().find_map(|item| match item {
            Item::Folder {
                folder: other,
                members,
            } if *other == folder => Some(members),
            _ => None,
        });
        match known {
            Some(members) => members.push(index),
            None => items.push(Item::Folder {
                folder,
                members: vec![index],
            }),
        }
    }
    items
}

/// Lists the names of the folders of a scope, in the order of the tree.
pub fn folder_names(connections: &[Connection], scope: Scope) -> Vec<String> {
    top_items(connections)
        .into_iter()
        .filter_map(|item| match item {
            Item::Folder { folder, .. } if folder.scope == scope => Some(folder.name),
            _ => None,
        })
        .collect()
}

/// Returns the name of a folder on screen. It holds the scope when the other scope has a
/// folder with the same name.
pub fn folder_label(connections: &[Connection], folder: &FolderRef) -> String {
    let shared = connections
        .iter()
        .filter_map(folder_of)
        .any(|other| other.name == folder.name && other.scope != folder.scope);
    crate::connection::scope_label(&folder.name, Some(folder.scope), shared)
}

/// Swaps an item with the item above or below it, at the same level.
pub fn move_by(
    connections: &[Connection],
    target: &Target,
    up: bool,
) -> Result<Arrangement, &'static str> {
    let mut tree = Tree::of(connections, scope_of(connections, target)?);
    let (node, member) = tree.locate(target)?;
    let swap = |list_len: usize, at: usize| -> Option<usize> {
        if up {
            at.checked_sub(1)
        } else {
            (at + 1 < list_len).then_some(at + 1)
        }
    };
    match member {
        Some(member) => {
            let Node::Folder(_, members) = &mut tree.nodes[node] else {
                unreachable!("a member is in a folder");
            };
            let other = swap(members.len(), member).ok_or(text::MOVE_FOLDER_EDGE)?;
            members.swap(member, other);
        }
        None => {
            let other = swap(tree.nodes.len(), node).ok_or(text::MOVE_SCOPE_EDGE)?;
            tree.nodes.swap(node, other);
        }
    }
    Ok(tree.arrangement(connections))
}

/// Puts a connection at the end of a folder, or at the top level after its folder.
pub fn move_to_folder(
    connections: &[Connection],
    connection: usize,
    folder: Option<&str>,
) -> Result<Arrangement, &'static str> {
    let target = Target::Connection(connection);
    let mut tree = Tree::of(connections, scope_of(connections, &target)?);
    let (node, member) = tree.locate(&target)?;
    match folder {
        Some(name) => {
            if let Some(existing) = tree.folder_index(name) {
                if member.is_none() || existing != node {
                    tree.take(node, member);
                    let existing = tree.folder_index(name).unwrap_or(existing);
                    if let Node::Folder(_, members) = &mut tree.nodes[existing] {
                        members.push(connection);
                    }
                }
            } else {
                // A new folder takes the place of the connection, or the place after
                // the folder of the connection.
                let place = node + usize::from(member.is_some());
                let removed = tree.take(node, member);
                let place = place - usize::from(removed && member.is_some());
                tree.nodes
                    .insert(place, Node::Folder(name.to_owned(), vec![connection]));
            }
        }
        None => {
            if member.is_some() {
                let removed = tree.take(node, member);
                let place = node + usize::from(!removed);
                tree.nodes.insert(place, Node::Loose(connection));
            }
        }
    }
    Ok(tree.arrangement(connections))
}

/// Gives a folder a new name. A name that another folder of the scope uses joins the two
/// folders, at the place of the other folder.
pub fn rename_folder(
    connections: &[Connection],
    folder: &FolderRef,
    name: &str,
) -> Result<Arrangement, &'static str> {
    let mut tree = Tree::of(connections, folder.scope);
    let (node, _) = tree.locate(&Target::Folder(folder.clone()))?;
    match tree.folder_index(name).filter(|other| *other != node) {
        Some(other) => {
            let Node::Folder(_, members) = tree.nodes.remove(node) else {
                unreachable!("the node is a folder");
            };
            let other = if other > node { other - 1 } else { other };
            if let Node::Folder(_, existing) = &mut tree.nodes[other] {
                existing.extend(members);
            }
        }
        None => {
            if let Node::Folder(old, _) = &mut tree.nodes[node] {
                name.clone_into(old);
            }
        }
    }
    Ok(tree.arrangement(connections))
}

/// Removes a folder. Its connections stay at its place, at the top level.
pub fn ungroup(
    connections: &[Connection],
    folder: &FolderRef,
) -> Result<Arrangement, &'static str> {
    let mut tree = Tree::of(connections, folder.scope);
    let (node, _) = tree.locate(&Target::Folder(folder.clone()))?;
    let Node::Folder(_, members) = tree.nodes.remove(node) else {
        unreachable!("the node is a folder");
    };
    for (offset, member) in members.into_iter().enumerate() {
        tree.nodes.insert(node + offset, Node::Loose(member));
    }
    Ok(tree.arrangement(connections))
}

/// Moves an item to the place of another row. A folder target takes a connection at its
/// end. Otherwise the item takes the place of the target: it goes above the target when it
/// moves up, and below it when it moves down. A connection takes the folder of a
/// connection target, and a folder takes the place of the top-level item of the target.
pub fn drop_on(
    connections: &[Connection],
    source: &Target,
    target: &Target,
) -> Result<Arrangement, &'static str> {
    let scope = scope_of(connections, source)?;
    if scope_of(connections, target)? != scope {
        return Err(text::MOVE_OTHER_SCOPE);
    }
    if let (Target::Connection(connection), Target::Folder(folder)) = (source, target) {
        if !leaves_folder(connections, *connection, folder) {
            return move_to_folder(connections, *connection, Some(&folder.name));
        }
        // The row of its own folder takes the connection out, at the place of the folder.
        let mut tree = Tree::of(connections, scope);
        let (node, member) = tree.locate(source)?;
        tree.take(node, member);
        tree.nodes.insert(node, Node::Loose(*connection));
        return Ok(tree.arrangement(connections));
    }
    let mut tree = Tree::of(connections, scope);
    let down = usize::from(tree.flat_index(source)? < tree.flat_index(target)?);
    let (node, member) = tree.locate(source)?;
    match source {
        Target::Connection(connection) => {
            if source == target {
                return Ok(tree.arrangement(connections));
            }
            tree.take(node, member);
            let (other, place) = tree.locate(target)?;
            match place {
                Some(place) => {
                    if let Node::Folder(_, members) = &mut tree.nodes[other] {
                        members.insert(place + down, *connection);
                    }
                }
                None => tree.nodes.insert(other + down, Node::Loose(*connection)),
            }
        }
        Target::Folder(_) => {
            let (other, _) = tree.locate(target)?;
            if other != node {
                let moved = tree.nodes.remove(node);
                let other = if other > node { other - 1 } else { other };
                tree.nodes.insert(other + down, moved);
            }
        }
    }
    Ok(tree.arrangement(connections))
}

/// Returns true when a drop of the connection on the folder takes it out of the folder:
/// the folder is the folder of the connection.
pub fn leaves_folder(connections: &[Connection], connection: usize, folder: &FolderRef) -> bool {
    connections
        .get(connection)
        .and_then(folder_of)
        .is_some_and(|own| &own == folder)
}

/// Moves an item to the end of its scope, at the top level. A connection leaves its
/// folder, and a folder moves with its connections.
pub fn drop_at_end(
    connections: &[Connection],
    source: &Target,
) -> Result<Arrangement, &'static str> {
    let mut tree = Tree::of(connections, scope_of(connections, source)?);
    let (node, member) = tree.locate(source)?;
    let moved = match source {
        Target::Connection(connection) => {
            tree.take(node, member);
            Node::Loose(*connection)
        }
        Target::Folder(_) => tree.nodes.remove(node),
    };
    tree.nodes.push(moved);
    Ok(tree.arrangement(connections))
}

/// Returns the scope of an item, or the refusal for a connection that no file holds.
pub fn scope_of(connections: &[Connection], target: &Target) -> Result<Scope, &'static str> {
    match target {
        Target::Connection(index) => connections
            .get(*index)
            .and_then(Connection::scope)
            .ok_or(text::MOVE_NOT_SAVED),
        Target::Folder(folder) => Ok(folder.scope),
    }
}

/// The top level of one scope, with connections in place of their positions.
#[derive(Debug)]
enum Node {
    Loose(usize),
    Folder(String, Vec<usize>),
}

#[derive(Debug)]
struct Tree {
    scope: Scope,
    nodes: Vec<Node>,
}

impl Tree {
    fn of(connections: &[Connection], scope: Scope) -> Self {
        let nodes = top_items(connections)
            .into_iter()
            .filter_map(|item| match item {
                Item::Connection(index) => {
                    (connections[index].scope() == Some(scope)).then_some(Node::Loose(index))
                }
                Item::Folder { folder, members } => {
                    (folder.scope == scope).then_some(Node::Folder(folder.name, members))
                }
            })
            .collect();
        Self { scope, nodes }
    }

    /// Returns the node of the target, and its place in the folder of a connection.
    fn locate(&self, target: &Target) -> Result<(usize, Option<usize>), &'static str> {
        for (index, node) in self.nodes.iter().enumerate() {
            match (node, target) {
                (Node::Loose(own), Target::Connection(wanted)) if own == wanted => {
                    return Ok((index, None));
                }
                (Node::Folder(_, members), Target::Connection(wanted)) => {
                    if let Some(member) = members.iter().position(|own| own == wanted) {
                        return Ok((index, Some(member)));
                    }
                }
                (Node::Folder(name, _), Target::Folder(folder)) if *name == folder.name => {
                    return Ok((index, None));
                }
                _ => {}
            }
        }
        Err(text::MOVE_NOT_SAVED)
    }

    /// Returns the place of an item in the order of the file. A folder gives the place of
    /// its first connection.
    fn flat_index(&self, target: &Target) -> Result<usize, &'static str> {
        let (node, member) = self.locate(target)?;
        let before: usize = self.nodes[..node]
            .iter()
            .map(|node| match node {
                Node::Loose(_) => 1,
                Node::Folder(_, members) => members.len(),
            })
            .sum();
        Ok(before + member.unwrap_or(0))
    }

    fn folder_index(&self, name: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| matches!(node, Node::Folder(own, _) if own == name))
    }

    /// Removes a connection. It returns true when its folder became empty and left.
    fn take(&mut self, node: usize, member: Option<usize>) -> bool {
        match member {
            None => {
                self.nodes.remove(node);
                false
            }
            Some(member) => {
                let Node::Folder(_, members) = &mut self.nodes[node] else {
                    unreachable!("a member is in a folder");
                };
                members.remove(member);
                let empty = members.is_empty();
                if empty {
                    self.nodes.remove(node);
                }
                empty
            }
        }
    }

    fn arrangement(&self, connections: &[Connection]) -> Arrangement {
        let id = |index: usize| {
            connections[index]
                .key
                .as_ref()
                .map_or_else(String::new, |key| key.id.clone())
        };
        let order = self
            .nodes
            .iter()
            .flat_map(|node| match node {
                Node::Loose(index) => vec![(id(*index), None)],
                Node::Folder(name, members) => members
                    .iter()
                    .map(|index| (id(*index), Some(name.clone())))
                    .collect(),
            })
            .collect();
        Arrangement {
            scope: self.scope,
            order,
        }
    }
}
