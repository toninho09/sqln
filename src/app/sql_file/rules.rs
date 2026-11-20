//! The decisions of the open of a `.sql` file that need no `App`: its connection and
//! the name of its console.

use std::path::Path;

use crate::connection::Connection;
use crate::exec::ConnectionId;

/// The connection of a file: the connection of the console, then the only connection.
/// `None` asks the user.
pub(super) fn connection_for(
    connections: &[Connection],
    active: Option<ConnectionId>,
) -> Option<ConnectionId> {
    let active = active.filter(|id| connections.iter().any(|item| item.id == *id));
    let single = (connections.len() == 1).then(|| connections[0].id);
    active.or(single)
}

/// The name of the console of a file: the name of the file without its extension.
pub(super) fn console_name(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::sql::Dialect;
    use sqln_testkit::mock::{MockConnector, RunHandler};

    fn connection(id: ConnectionId) -> Connection {
        let run: RunHandler = Arc::new(|_, _| Box::pin(async { Ok(Default::default()) }));
        Connection::new(
            id,
            format!("c{id}"),
            None,
            &MockConnector::new(Dialect::Sqlite, run),
        )
    }

    #[test]
    fn the_console_connection_comes_first_then_the_only_one() {
        let one = [connection(1)];
        assert_eq!(connection_for(&one, None), Some(1));
        assert_eq!(
            connection_for(&one, Some(9)),
            Some(1),
            "a gone connection is ignored"
        );
        let two = [connection(1), connection(2)];
        assert_eq!(connection_for(&two, Some(2)), Some(2));
        assert_eq!(connection_for(&two, None), None, "the user chooses");
    }

    #[test]
    fn the_console_takes_the_name_of_the_file() {
        assert_eq!(
            console_name(Path::new("/a/report.sql")).as_deref(),
            Some("report")
        );
        assert_eq!(console_name(Path::new("/a/.sql")).as_deref(), Some(".sql"));
        assert_eq!(console_name(Path::new("/")), None);
    }
}
