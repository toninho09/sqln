//! The order of the connections in the explorer and their folders: the keys that move
//! an item, the folder list, the folder names, and the removal of a folder.

use super::{App, DialogAction, Picker, PickerKind};
use crate::arrange::{self, Target};
use crate::exec::ConnectionId;
use crate::input::Action;
use crate::navigator::Row;
use crate::store::config::{self, FolderRef};
use crate::ui::name::{NamePrompt, NameTarget};
use crate::ui::text;

/// One row of the folder list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FolderChoice {
    New,
    NoFolder,
    Folder(String),
}

impl App {
    /// Returns the item that a move of a row changes: its folder, or its connection.
    fn tree_target(&self, row: &Row) -> Target {
        row.folder(&self.connections)
            .map_or(Target::Connection(row.connection), Target::Folder)
    }

    /// Returns false, with the reason, when the tree cannot change its order now.
    fn can_arrange(&mut self) -> bool {
        if self.navigator.is_filtered() {
            self.status = Some(text::MOVE_FILTERED.to_owned());
            return false;
        }
        true
    }

    /// The keys that move a connection or a folder up or down.
    pub(super) fn move_tree_item(&mut self, row: &Row, up: bool) {
        if !self.can_arrange() {
            return;
        }
        let target = self.tree_target(row);
        let arrangement = arrange::move_by(&self.connections, &target, up);
        self.arrange_and_follow(&target, arrangement);
    }

    /// Moves an item to the place of the row under the mouse, or to the end of its scope
    /// when the mouse is below the tree.
    pub(super) fn drop_tree_item(&mut self, source: &Target, target: Option<&Row>) {
        if !self.can_arrange() {
            return;
        }
        let arrangement = match target {
            Some(row) => arrange::drop_on(&self.connections, source, &self.tree_target(row)),
            None => arrange::drop_at_end(&self.connections, source),
        };
        self.arrange_and_follow(source, arrangement);
    }

    /// Applies a new order, and the cursor follows the moved item.
    fn arrange_and_follow(
        &mut self,
        item: &Target,
        arrangement: Result<arrange::Arrangement, &'static str>,
    ) {
        let id = match item {
            Target::Connection(index) => self.connections.get(*index).map(|item| item.id),
            Target::Folder(_) => None,
        };
        if !self.arrange(arrangement) {
            return;
        }
        match (item, id) {
            (Target::Folder(folder), _) => self.select_folder(folder.clone()),
            (Target::Connection(_), Some(id)) => self.navigator.select_connection(id),
            (Target::Connection(_), None) => {}
        }
    }

    /// Opens the folder list of the connection of a row.
    pub(super) fn open_folder_list(&mut self, row: &Row) {
        if !self.can_arrange() {
            return;
        }
        let item = &self.connections[row.connection];
        let Some(scope) = item.scope() else {
            self.status = Some(text::MOVE_NOT_SAVED.to_owned());
            return;
        };
        let own = item.folder().map(str::to_owned);
        let mut choices = vec![FolderChoice::New];
        if own.is_some() {
            choices.push(FolderChoice::NoFolder);
        }
        choices.extend(
            arrange::folder_names(&self.connections, scope)
                .into_iter()
                .filter(|name| Some(name) != own.as_ref())
                .map(FolderChoice::Folder),
        );
        let items = choices
            .iter()
            .map(|choice| match choice {
                FolderChoice::New => text::FOLDER_NEW.to_owned(),
                FolderChoice::NoFolder => text::FOLDER_NONE.to_owned(),
                FolderChoice::Folder(name) => name.clone(),
            })
            .collect();
        self.modals.picker = Some(Picker {
            kind: PickerKind::Folder {
                connection: item.id,
                choices,
            },
            title: text::title_move_to_folder(item.plain_name()),
            items,
            cursor: 0,
            loading: false,
        });
    }

    /// Applies the row of the folder list.
    pub(super) fn choose_folder(&mut self, connection: ConnectionId, choice: FolderChoice) {
        let Some(index) = self.connection_index(connection) else {
            return;
        };
        let folder = match choice {
            FolderChoice::New => {
                self.modals.rename = Some(NamePrompt::new(NameTarget::NewFolder(connection), ""));
                return;
            }
            FolderChoice::NoFolder => None,
            FolderChoice::Folder(name) => Some(name),
        };
        let arrangement = arrange::move_to_folder(&self.connections, index, folder.as_deref());
        if self.arrange(arrangement) {
            self.navigator.select_connection(connection);
        }
    }

    /// The commands of a folder row. A command for a connection explains the refusal.
    pub(super) fn on_folder_action(&mut self, action: Action, row: &Row) {
        let Some(folder) = row.folder(&self.connections) else {
            return;
        };
        match action {
            Action::NavigatorEdit => {
                let prompt = NamePrompt::new(NameTarget::Folder(folder.clone()), &folder.name);
                self.modals.rename = Some(prompt);
            }
            Action::NavigatorDelete => self.confirm_ungroup(folder),
            Action::NavigatorMoveUp => self.move_tree_item(row, true),
            Action::NavigatorMoveDown => self.move_tree_item(row, false),
            Action::NavigatorCopyName => self.copy(&folder.name),
            _ => self.status = Some(text::NOT_FOR_FOLDER.to_owned()),
        }
    }

    fn confirm_ungroup(&mut self, folder: FolderRef) {
        if !self.can_arrange() {
            return;
        }
        let body = text::remove_folder(&folder.name);
        self.show_dialog(
            text::TITLE_REMOVE_FOLDER,
            body,
            vec![
                (text::OPTION_CANCEL, DialogAction::Dismiss),
                (text::OPTION_REMOVE_FOLDER, DialogAction::Ungroup(folder)),
            ],
        );
    }

    /// Removes a folder. Its connections stay at its place.
    pub(super) fn ungroup(&mut self, folder: &FolderRef) {
        let first = self.first_of(folder);
        if self.arrange(arrange::ungroup(&self.connections, folder))
            && let Some(first) = first
        {
            self.navigator.select_connection(first);
        }
    }

    /// Applies the name of a new folder, or the new name of a folder. It returns false
    /// when the name prompt is not about a folder.
    pub(super) fn accept_folder_name(&mut self, name: &str) -> bool {
        let Some(target) = self.modals.rename.as_ref().map(|prompt| &prompt.target) else {
            return false;
        };
        if !matches!(target, NameTarget::NewFolder(_) | NameTarget::Folder(_)) {
            return false;
        }
        let name = name.trim();
        if let Some(error) = config::folder_error(name) {
            self.name_note(error);
            return true;
        }
        let Some(prompt) = self.modals.rename.take() else {
            return true;
        };
        match prompt.target {
            NameTarget::NewFolder(connection) => {
                self.choose_folder(connection, FolderChoice::Folder(name.to_owned()));
            }
            NameTarget::Folder(old) => {
                let new = FolderRef {
                    scope: old.scope,
                    name: name.to_owned(),
                };
                if self.arrange(arrange::rename_folder(&self.connections, &old, name)) {
                    self.navigator.rename_folder(&old, &new);
                    self.select_folder(new);
                }
            }
            NameTarget::Console(_) | NameTarget::Script(_) => {}
        }
        true
    }

    fn select_folder(&mut self, folder: FolderRef) {
        if let Some(first) = self.first_of(&folder) {
            self.navigator.select_folder(folder, first);
        }
    }

    fn first_of(&self, folder: &FolderRef) -> Option<ConnectionId> {
        self.connections
            .iter()
            .find(|item| arrange::folder_of(item).as_ref() == Some(folder))
            .map(|item| item.id)
    }
}
