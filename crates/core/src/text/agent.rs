//! MCP agents.

pub const AGENT_CONSOLE_TEMPORARY: &str =
    "Agent consoles are temporary. Copy SQL to a user console to save it.";
pub const AGENT_NO_EDITOR: &str = "This agent tab shows SQL only when the agent places SQL for you. Open a user console to write SQL.";
pub const AGENT_SQL_TITLE: &str = "SQL from agent";
/// Marks an agent tab with SQL that the user did not see yet.
pub const SQL_PREFIX: &str = "SQL · ";
pub const CONTROL_NEW_CONSOLE: &str = "New console";
pub const CONTROL_CLEAR: &str = "Clear";
pub const AGENT_PREFIX: &str = "MCP · ";
pub(super) const AGENT_OFF_PREFIX: &str = "MCP off · ";
pub const DIRECT_PREFIX: &str = "Direct · ";
/// The mark of a console with a pending agent proposal.
pub const PROPOSAL_PREFIX: &str = "± ";
pub const AGENT_REJECT: &str = "Reject";
pub const AGENT_ALLOW: &str = "Allow";
pub const AGENT_ALLOW_ALL: &str = "Allow all";
pub const AGENT_FULL: &str = "Full access";
pub const AGENT_FORGET: &str = "Forget";
pub const AGENT_KEEP: &str = "Keep access";
pub const AGENT_REVOKE: &str = "Revoke";
pub const AGENT_REVIEW: &str = "Review";
pub const AGENT_STOP: &str = "Stop";
pub const AGENT_PROPOSAL_GONE: &str =
    "This agent proposal is no longer available. Ask the agent to propose again.";

/// Writes the status bar label of the sessions with access, such as `MCP 1 full`.
pub fn agent_status(count: usize, mark: Option<&str>) -> String {
    match mark {
        Some(mark) => format!("MCP {count} {mark}"),
        None => format!("MCP {count}"),
    }
}

pub fn agent_console_name(client: &str, session: &str) -> String {
    format!("{AGENT_PREFIX}{client} [{session}]")
}

pub fn agent_tab_label(label: &str) -> String {
    format!("{AGENT_PREFIX}{label}")
}

/// Marks the console of a session that lost its access.
pub fn agent_inactive_label(label: &str) -> String {
    format!(
        "{AGENT_OFF_PREFIX}{}",
        label.strip_prefix(AGENT_PREFIX).unwrap_or(label)
    )
}

pub(super) fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// Writes the items of the MCP row: a long text and a short text for a narrow row.
pub fn agent_bar(access: usize, proposals: usize, tasks: usize) -> (String, String) {
    let mut long = Vec::new();
    let mut short = Vec::new();
    if access != 0 {
        long.push(counted(access, "access request", "access requests"));
        short.push(format!("{access} access"));
    }
    if proposals != 0 {
        long.push(counted(proposals, "edit proposal", "edit proposals"));
        short.push(counted(proposals, "edit", "edits"));
    }
    if tasks != 0 {
        long.push(format!("{} running", counted(tasks, "task", "tasks")));
        short.push(counted(tasks, "task", "tasks"));
    }
    (long.join(" · "), short.join(" · "))
}

/// Adds the place of an item in its queue to a title, when the queue has more items.
pub fn agent_queue_title(title: &str, position: usize, total: usize) -> String {
    if total > 1 {
        format!("{title} ({position} of {total})")
    } else {
        title.to_owned()
    }
}

pub const AGENT_ACCESS_TITLE: &str = "MCP access request";
pub const AGENT_LATER: &str = "Later";
pub const AGENT_HIDE: &str = "Hide";
pub const AGENT_TASK_TITLE: &str = "Agent task running";

/// Writes the first line of a proposal card, such as `claude edits console-1: +2 −1 lines.`
pub fn agent_proposal_card(
    client: &str,
    console: &str,
    (added, removed): (usize, usize),
    position: usize,
    total: usize,
) -> String {
    agent_queue_title(
        &format!("{client} edits {console}: +{added} −{removed} lines."),
        position,
        total,
    )
}

/// Writes the first line of a task card, such as `claude: Query 3f2a9c1e · 12s`.
/// Writes the access button of an agent tab. A session without access shows `off`.
pub fn agent_access_button(level: Option<&str>) -> String {
    format!("Access: {}", level.unwrap_or("off"))
}

/// Writes the tasks button of an agent tab, such as `Tasks 2`.
pub fn agent_tasks_button(count: usize) -> String {
    format!("Tasks {count}")
}

pub const AGENT_TASKS_TITLE: &str = "Agent tasks";
pub const AGENT_CLOSE_TITLE: &str = "Close the agent tab";
pub const AGENT_CLOSE_REVOKE: &str = "Revoke and close";

/// The question before the close of the last agent tab of a connected session.
pub fn agent_close_body(client: &str) -> String {
    format!(
        "{client} is connected, and this is its last tab.\n\n\
         Revoke and close removes its access and stops its tasks.\n\
         Cancel keeps the tab and the access."
    )
}
pub const AGENT_TASK_KEEP: &str = "Keep running";
pub const AGENT_TASK_GONE: &str =
    "This agent task ended. Open Agent tasks again to see the current tasks.";
pub const AGENT_NO_SESSION: &str =
    "The agent of this tab disconnected. Open Agent sessions to see the other sessions.";

/// Writes a row of the task list, such as `Query  3f2a9c1e  running  12s`.
pub fn agent_task_row(task: &str, id: &str, state: &str, seconds: u64) -> String {
    format!("{task:<8} {id}  {}  {seconds}s", state.replace('_', " "))
}

/// Writes the question that stops one agent task.
pub fn agent_task_body(client: &str, task: &str, id: &str, state: &str, seconds: u64) -> String {
    format!(
        "Client   {client}\n\
         Task     {task} {id}\n\
         State    {} · {seconds}s\n\n\
         Stop cancels this task only. The agent keeps its access.",
        state.replace('_', " ")
    )
}

pub fn agent_task_card(client: &str, task: &str, id: &str, seconds: u64, total: usize) -> String {
    let more = if total > 1 {
        format!(" · {total} running")
    } else {
        String::new()
    };
    format!("{client}: {task} {id} · {seconds}s{more}")
}

/// Writes the first line of the access card, such as `claude asks for access to SQLN.`
pub fn agent_card(client: &str, position: usize, total: usize) -> String {
    agent_queue_title(
        &format!("{client} asks for access to SQLN."),
        position,
        total,
    )
}
pub const AGENT_ACCESS_OPEN_TITLE: &str = "MCP access";

pub fn agent_access_body(client: &str, session: &str, age: &str) -> String {
    format!(
        "Client   {client} (reported, not verified)\n\
         Session  {session} · requested {age} ago\n\n\
         Allow: read consoles, data, and the catalog; run read-only queries; \
         propose edits for your review.\n\
         Allow all: also edit your consoles directly, without review. \
         Edits can save to script files.\n\
         Full access: also write to the database, without review. \
         Production runs and writes without WHERE still ask you.\n\
         SQLN remembers this choice for the client until it exits."
    )
}

/// The body of the dialog of a session with access. Its buttons change the level or revoke it.
pub fn agent_revoke_body(client: &str, session: &str, level: &str, tasks: usize) -> String {
    let mode = agent_level_text(level);
    format!(
        "Client   {client} (reported, not verified)\n\
         Session  {session}\n\
         Access   {mode}\n\n\
         Select another option to change the access. Full access also writes to the database.\n\
         Revoke stops {} and discards its pending edit proposals.",
        counted(tasks, "active task", "active tasks")
    )
}

/// Writes a row of the session list.
pub fn agent_session_row(
    client: &str,
    session: &str,
    state: &str,
    level: &str,
    tasks: usize,
) -> String {
    let state = match state {
        "pending" => "Waiting for you".to_owned(),
        "authorized" => {
            let mode = level;
            if tasks == 0 {
                mode.to_owned()
            } else {
                format!("{mode} · {} running", counted(tasks, "task", "tasks"))
            }
        }
        "denied" => "Rejected".to_owned(),
        "revoked" => "Revoked".to_owned(),
        other => other.to_owned(),
    };
    format!("{client}  {session}  {state}")
}

/// Describes an access level by the name of its option.
pub(super) fn agent_level_text(level: &str) -> &'static str {
    match level {
        AGENT_ALLOW_ALL => "Allow all: direct edits in all consoles.",
        AGENT_FULL => "Full access: direct edits and database writes.",
        _ => "Allow: edits need your review.",
    }
}

pub const AGENT_REMEMBERED_TITLE: &str = "MCP remembered access";

/// Writes a row of a client with remembered access and no live session.
pub fn agent_remembered_row(client: &str, level: &str) -> String {
    format!("{client}  remembered  {level}")
}

pub fn agent_remembered_body(client: &str, level: &str) -> String {
    format!(
        "Client   {client} (reported, not verified)\n\
         Access   {}\n\n\
         A new session of this client gets this access without a question, until SQLN exits. \
         Forget makes its next session ask you again.",
        agent_level_text(level)
    )
}
pub const AGENT_ASK: &str = "Ask";
pub const AGENT_MCP_OFF: &str = "MCP is off in Settings > IA.";
pub fn agent_mcp_closed() -> String {
    format!("{AGENT_MCP_OFF} SQLN revoked the MCP sessions and closed the endpoint.")
}
pub fn agent_access_lowered(client: &str, level: &str) -> String {
    format!("MCP {client}: access lowered to {level}, the maximum in Settings > IA.")
}
pub fn agent_default_access(client: &str, level: &str) -> String {
    format!("MCP {client} connected with the default access: {level}. Change it in Agent sessions.")
}

pub fn agent_remembered(client: &str, level: &str) -> String {
    format!(
        "MCP {client} reconnected with remembered access: {level}. Revoke it in Agent sessions."
    )
}

pub fn agent_forgotten(client: &str) -> String {
    format!("MCP {client}: remembered access removed. Its next session asks you again.")
}

/// The question before a risky run of an agent with full access.
pub fn agent_run_question(client: &str, risk: &str) -> String {
    format!("MCP {client} asks to run this SQL.\n\n{risk}")
}

pub const AGENT_RUN_REFUSED: &str = "The user refused the run in SQLN.";
pub const AGENT_ROLLED_BACK: &str = "The agent SQL left a transaction open, so SQLN rolled it back. Send BEGIN and COMMIT in the same SQL.";

pub fn agent_proposal_row(console: &str, client: &str, revision: u64) -> String {
    format!("{console} — from {client} — revision {revision}")
}

pub const AGENT_PROPOSAL_TITLE: &str = "Edit proposal";

pub fn agent_access_done(client: &str, choice: &str) -> String {
    match choice {
        AGENT_ALLOW => format!("MCP {client}: access allowed. Edits need your review."),
        AGENT_ALLOW_ALL => {
            format!("MCP {client}: access allowed with direct edits. Revoke it in Agent sessions.")
        }
        AGENT_FULL => {
            format!(
                "MCP {client}: full access allowed, with database writes. Revoke it in Agent sessions."
            )
        }
        _ => format!("MCP {client}: access rejected."),
    }
}

pub fn agent_revoked(client: &str) -> String {
    format!("MCP {client}: access revoked. Its active tasks are cancelling.")
}

pub fn agent_edited(client: &str, console: &str, undo: Option<&str>) -> String {
    match undo {
        Some(key) => format!("MCP {client} edited {console}. {key} undoes it."),
        None => format!("MCP {client} edited {console}."),
    }
}

pub fn agent_proposed(client: &str, console: &str) -> String {
    format!("MCP {client} proposed an edit to {console}. Use Review in the MCP row.")
}

pub fn agent_placed(client: &str) -> String {
    format!("MCP {client} placed SQL in its tab. Review it and press Run.")
}

pub fn agent_placed_note(client: &str, run: Option<&str>) -> String {
    match run {
        Some(key) => format!("{client} placed SQL for you. Review it and press {key} to run it."),
        None => format!("{client} placed SQL for you. Review it and run it."),
    }
}

pub fn agent_sql_cleared(undo: Option<&str>) -> String {
    match undo {
        Some(key) => format!("Agent SQL cleared. {key} restores it."),
        None => "Agent SQL cleared.".into(),
    }
}

pub fn agent_rejected(console: &str) -> String {
    format!("Agent edit rejected for {console}.")
}
pub const AGENT_DISCARDED_REVISION: &str =
    "Agent edit discarded: the console changed or closed. Ask the agent to propose again.";
pub const AGENT_DISCARDED_BUDGET: &str =
    "Agent edit discarded: its diff exceeds the memory budget. Ask the agent for a smaller edit.";

pub fn agent_partial(console: &str, accepted: usize, rejected: usize) -> String {
    format!(
        "Agent edit partly applied to {console}: {accepted} accepted, {rejected} rejected. SQL does not execute automatically."
    )
}

pub fn agent_applied(console: &str) -> String {
    format!("Agent edit applied to {console}. SQL does not execute automatically.")
}

pub fn agent_ran(client: &str, console: &str, read_only: bool) -> String {
    if read_only {
        format!("MCP {client} ran {console} (read-only).")
    } else {
        format!("MCP {client} ran {console}.")
    }
}

pub fn agent_opened(client: &str, tab: &str) -> String {
    format!("MCP {client} opened the {tab} tab.")
}

pub fn agent_failed(client: &str, task: &str) -> String {
    format!("MCP {client}: {task} failed. See its console Messages.")
}

pub fn agent_task_note(task: &str, id: &str, state: &str, message: Option<&str>) -> String {
    match message {
        Some(message) => format!("{task} {id} {state}: {message}"),
        None => format!("{task} {id} {state}."),
    }
}

pub fn agent_stopped(count: usize) -> String {
    format!(
        "MCP: {} stopped.",
        counted(count, "agent task", "agent tasks")
    )
}

pub const AGENT_STOP_REASON: &str = "The user stopped agent tasks in SQLN.";
pub const AGENT_NOTHING: &str = "MCP: no pending items.";
pub const ONE_TARGET: &str = "Give one file or one URI.";

/// The next action of the agent after it placed SQL for the user.
pub const AGENT_PLACED_NEXT: &str = "The user reviews and runs this SQL in SQLN; nothing ran. After the user runs it, read its results with read_result and this console_id as tab_id, and its errors with read_console and messages. get_context shows the console phase and result_count.";

/// The error of a catalog read for a schema that the catalog does not hold.
pub fn agent_schema_not_found(names: &[String], total: usize) -> String {
    let more = if total > names.len() {
        format!(" and {} more", total - names.len())
    } else {
        String::new()
    };
    if names.is_empty() {
        return "The catalog has no schema. Call describe without schema, or with refresh.".into();
    }
    format!(
        "The catalog has no such schema. Schemas: {}{more}. Send one of these names, or call describe with refresh if the schema is new.",
        names.join(", ")
    )
}
