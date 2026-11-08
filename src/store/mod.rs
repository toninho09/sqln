//! The files of the application: the folders of the two scopes, the configuration,
//! the scripts of the consoles, and the state of the interface.
//!
//! The layout of the files is in `docs/ARCHITECTURE.md`.

pub mod config;
pub mod file;
pub mod history;
pub mod scripts;
pub mod themes;
pub mod workspace;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const PROJECT_FOLDER: &str = ".sqln";
const PROJECT_IGNORE: &str = "workspace.json\nlocks/\n*.tmp\n";

/// Where a connection, and its consoles, live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Global,
    Project,
}

/// The folder of one scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// The folder with `config.json`: the global folder, or `<project>/.sqln`.
    pub dir: PathBuf,
    /// The base of the relative paths of a connection: the global folder, or the
    /// root of the project. The base of a project is not `.sqln`.
    pub base: PathBuf,
}

impl Root {
    pub fn global(dir: PathBuf) -> Self {
        Self {
            base: dir.clone(),
            dir,
        }
    }

    /// Returns the scope of the project with this root folder.
    pub fn project(root: &Path) -> Self {
        Self {
            dir: root.join(PROJECT_FOLDER),
            base: root.to_owned(),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    /// The custom color themes, one file per theme.
    pub fn themes_dir(&self) -> PathBuf {
        self.dir.join("themes")
    }

    pub fn workspace_file(&self) -> PathBuf {
        self.dir.join("workspace.json")
    }

    /// The authorized plain text passwords. Only the global folder uses this file.
    pub fn credentials_file(&self) -> PathBuf {
        self.dir.join("credentials.json")
    }

    pub fn consoles_dir(&self) -> PathBuf {
        self.dir.join("consoles")
    }

    /// The query history. Only the global folder holds it, so no project file holds SQL.
    pub fn history_file(&self) -> PathBuf {
        self.dir.join("history.jsonl")
    }

    pub fn locks_dir(&self) -> PathBuf {
        self.dir.join("locks")
    }
}

/// The two scopes. The project scope exists only with a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    pub global: Root,
    pub project: Option<Root>,
}

impl Store {
    pub fn root(&self, scope: Scope) -> Option<&Root> {
        match scope {
            Scope::Global => Some(&self.global),
            Scope::Project => self.project.as_ref(),
        }
    }

    /// Lists the scopes in the order of the configuration: global, then project.
    pub fn scopes(&self) -> Vec<(Scope, &Root)> {
        let project = self.project.as_ref().map(|root| (Scope::Project, root));
        std::iter::once((Scope::Global, &self.global))
            .chain(project)
            .collect()
    }
}

/// Returns the global folder: `$XDG_CONFIG_HOME/sqln`, or `~/.config/sqln`.
pub fn global_dir(xdg: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    let xdg = xdg.map(PathBuf::from).filter(|path| path.is_absolute());
    match xdg {
        Some(path) => Some(path.join("sqln")),
        None => home.map(|home| home.join(".config").join("sqln")),
    }
}

/// Returns the global folder of this process.
pub fn default_global_dir() -> Option<PathBuf> {
    global_dir(std::env::var_os("XDG_CONFIG_HOME"), std::env::home_dir())
}

/// Returns the root of the nearest project, from `start` to the top.
/// A `.git` folder does not replace the `.sqln` folder.
pub fn find_project(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|folder| folder.join(PROJECT_FOLDER).is_dir())
        .map(Path::to_owned)
}

/// The project that a start uses.
#[derive(Debug, PartialEq, Eq)]
pub struct Located {
    /// The root of the project, when its `.sqln` folder exists.
    pub root: Option<PathBuf>,
    /// The folder where the application can offer a new project.
    pub offer: Option<PathBuf>,
}

/// Finds the project of a start. A folder from the command line replaces the search
/// in the upper folders. A relative folder starts at the working folder.
pub fn locate_project(working: &Path, folder: Option<&Path>) -> Located {
    match folder {
        Some(folder) => {
            let folder = working.join(folder);
            let exists = folder.join(PROJECT_FOLDER).is_dir();
            Located {
                root: exists.then(|| folder.clone()),
                offer: (!exists).then_some(folder),
            }
        }
        None => {
            let root = find_project(working);
            let offer = root.is_none().then(|| working.to_owned());
            Located { root, offer }
        }
    }
}

/// Creates the folder of a project and its `.gitignore`. It runs no Git command.
pub fn create_project(root: &Path) -> io::Result<Root> {
    let project = Root::project(root);
    std::fs::create_dir_all(&project.dir)?;
    let ignore = project.dir.join(".gitignore");
    if !ignore.exists() {
        file::write_atomic(&ignore, PROJECT_IGNORE.as_bytes(), file::Expect::Missing)
            .map_err(|error| io::Error::other(error.to_string()))?;
    }
    Ok(project)
}
