//! Bounded reads of the state already present in SQLN.

use super::{App, Error, Reply, id, missing};
use crate::agent::{PAGE_BYTES, TEXT_BYTES};
use crate::grid::Value as Cell;
use crate::result::ResultTab;
use base64::Engine;
use serde_json::{Value, json};

impl App {
    pub(super) fn agent_active_tab(&self) -> String {
        if let Some(index) = self.compare_active {
            self.compares[index].sides[0].id.to_string()
        } else if let Some(index) = self.diagram_active {
            self.diagrams[index].id.to_string()
        } else if let Some(index) = self.table_active {
            self.tables[index].id.to_string()
        } else {
            self.console().id.to_string()
        }
    }

    pub(super) fn agent_connections(&self, offset: usize, limit: usize) -> Reply {
        let connections:Vec<_> = self.connections.iter().skip(offset).take(limit.clamp(1,1000)).map(|connection|json!({"connection_id":connection.id.to_string(),"name":connection.plain_name(),"dialect":format!("{:?}",connection.dialect),"capabilities":{"namespace":format!("{:?}",connection.capabilities.namespace),"parameters":connection.capabilities.parameters,"transactions":connection.capabilities.transactions,"data_edit":false}})).collect();
        let next = offset.saturating_add(connections.len());
        Ok(
            json!({"connections":connections,"next_offset":(next < self.connections.len()).then_some(next),"total":self.connections.len()}),
        )
    }

    pub(super) fn agent_tabs(&self, offset: usize, limit: usize) -> Reply {
        let consoles = self.consoles.iter().filter(|console|!console.hidden).map(|console|json!({
            "tab_id":console.id.to_string(),"console_id":console.id.to_string(),"kind":"console","name":self.agent.console_label(&self.consoles,console.id,&console.name),
            "connection_id":console.connection.map(|id|id.to_string()),"schema":console.schema,
            "revision":console.editor.revision(),"phase":format!("{:?}",console.phase),"transaction":format!("{:?}",console.transaction),
            "result_count":console.results.len(),"selected_result":console.active,"agent_owned":self.agent.owned.contains_key(&console.id),"direct_edit":self.agent.direct.contains(&console.id),
        }));
        let tables = self.tables.iter().map(|tab|json!({"tab_id":tab.id.to_string(),"kind":tab.kind(),"name":tab.label(),"connection_id":tab.connection.to_string(),"filter":tab.applied_filter(),"order":tab.applied_order(),"page":tab.page(),"has_more":tab.has_more,"busy":tab.is_busy()}));
        let diagrams = self.diagrams.iter().map(|tab|json!({"tab_id":tab.id.to_string(),"kind":"diagram","name":tab.label(),"busy":tab.is_busy(),"partial":tab.partial}));
        let comparisons = self.compares.iter().map(|tab|json!({"tab_id":tab.sides[0].id.to_string(),"kind":"compare","name":tab.label(),"busy":tab.is_busy(),"done":tab.done}));
        let total = self
            .consoles
            .iter()
            .filter(|console| !console.hidden)
            .count()
            + self.tables.len()
            + self.diagrams.len()
            + self.compares.len();
        let tabs: Vec<_> = consoles
            .chain(tables)
            .chain(diagrams)
            .chain(comparisons)
            .skip(offset)
            .take(limit.clamp(1, 1000))
            .collect();
        let next = offset.saturating_add(tabs.len());
        Ok(
            json!({"active_tab":self.agent_active_tab(),"tabs":tabs,"next_offset":(next < total).then_some(next),"total":total}),
        )
    }

    pub(super) fn agent_read_console(
        &self,
        console: u64,
        offset: usize,
        revision: Option<u64>,
    ) -> Reply {
        let console = self
            .consoles
            .iter()
            .find(|item| item.id == console && !item.hidden)
            .ok_or_else(missing)?;
        if (offset > 0 && revision.is_none())
            || revision.is_some_and(|revision| revision != console.editor.revision())
        {
            return Err(super::revision_conflict());
        }
        let mut page = console.editor.agent_text_page(offset)?;
        let cursor = console.editor.cursor();
        page["console_id"] = json!(console.id.to_string());
        page["cursor"] = json!({"line":cursor.row,"column":cursor.col});
        page["schema"] = json!(console.schema);
        page["selection"] = console.editor.agent_selection();
        page["direct_edit"] = json!(self.agent.direct.contains(&console.id));
        Ok(page)
    }

    pub(super) fn agent_messages(&self, console: u64, offset: usize, text_offset: usize) -> Reply {
        let console = self
            .consoles
            .iter()
            .find(|item| item.id == console && !item.hidden)
            .ok_or_else(missing)?;
        let mut messages = Vec::new();
        let mut bytes = 0;
        let records = console.messages.agent_messages().count();
        for message in console
            .messages
            .agent_messages()
            .skip(offset)
            .take(if text_offset == 0 { 100 } else { 1 })
        {
            let page = crate::agent::text_page(&message.text, text_offset)?;
            let item = json!({"level":format!("{:?}",message.level),"contents":page});
            let size = serde_json::to_vec(&item).map_err(|_| missing())?.len();
            if bytes + size > PAGE_BYTES / 2 {
                break;
            }
            bytes += size;
            messages.push(item);
        }
        let next = offset.saturating_add(messages.len());
        Ok(
            json!({"messages":messages,"next_offset":(next < records).then_some(next),"records":records}),
        )
    }

    fn agent_tab_busy(&self, tab: &str) -> bool {
        let Ok(tab) = id(tab) else { return false };
        self.consoles
            .iter()
            .any(|console| console.id == tab && console.is_busy())
            || self
                .tables
                .iter()
                .any(|table| table.id == tab && table.is_busy())
            || self
                .compares
                .iter()
                .any(|compare| compare.sides[0].id == tab && compare.is_busy())
    }

    fn agent_grid(&self, tab: &str, result: usize) -> Result<&ResultTab, Error> {
        let tab = id(tab)?;
        if let Some(console) = self
            .consoles
            .iter()
            .find(|console| console.id == tab && !console.hidden)
        {
            return console.results.get(result).ok_or_else(missing);
        }
        if result != 0 {
            return Err(missing());
        }
        if let Some(table) = self.tables.iter().find(|table| table.id == tab) {
            return Ok(&table.result);
        }
        if let Some(compare) = self
            .compares
            .iter()
            .find(|compare| compare.sides[0].id == tab)
        {
            return Ok(&compare.result);
        }
        Err(missing())
    }

    pub(super) fn agent_result(
        &self,
        tab: &str,
        result: usize,
        offset: usize,
        limit: usize,
        generation: Option<&str>,
    ) -> Reply {
        let grid = self.agent_grid(tab, result)?;
        let version = grid.table.generation.to_string();
        if generation.is_some_and(|generation| generation != version) {
            return Err(changed());
        }
        if offset > 0 && generation.is_none() {
            return Err(Error::new(
                "generation_required",
                "Use the generation from the first result page.",
            ));
        }
        if offset > grid.table.rows.len() {
            return Err(Error::new(
                "invalid_offset",
                "The row offset is outside this result.",
            ));
        }
        let mut rows = Vec::new();
        let mut bytes = 0;
        for (index, row) in grid
            .table
            .rows
            .iter()
            .enumerate()
            .skip(offset)
            .take(limit.clamp(1, 1000))
        {
            let cells: Vec<_> = row
                .cells()
                .enumerate()
                .map(|(column, value)| {
                    let mut value = cell(value);
                    if grid.edits.changed(index, column) {
                        value["pending_edit"] = json!(true);
                    }
                    value
                })
                .collect();
            let item = json!({"row":index,"cells":cells});
            let size = serde_json::to_vec(&item).map_err(|_| missing())?.len();
            if bytes + size > PAGE_BYTES / 4 {
                break;
            }
            bytes += size;
            rows.push(item);
        }
        if rows.is_empty() && offset < grid.table.rows.len() {
            return Err(Error::new(
                "response_too_large",
                "This row has too many values. Read its cells with read_result, row, and column.",
            ));
        }
        let next = offset + rows.len();
        let selection = grid.view.selection().map(|range| json!({"top":range.top,"bottom":range.bottom,"left":range.left,"right":range.right}));
        Ok(
            json!({"tab_id":tab,"result":result,"generation":version,"columns":grid.table.columns,"rows":rows,
            "next_offset":(next < grid.table.rows.len()).then_some(next),"loaded_rows":grid.table.rows.len(),"complete":grid.is_complete() && !self.agent_tab_busy(tab), "running":self.agent_tab_busy(tab),
            "limit":format!("{:?}",grid.limit),"cut_short":grid.cut_short,"pending_edits":!grid.edits.is_empty(),
            "view":{"top":grid.view.top,"left":grid.view.left,"row":grid.view.row,"column":grid.view.col,"selection":selection}}),
        )
    }

    pub(super) fn agent_cell(
        &self,
        tab: &str,
        result: usize,
        row: usize,
        column: usize,
        generation: &str,
        offset: usize,
    ) -> Reply {
        let grid = self.agent_grid(tab, result)?;
        if grid.table.generation.to_string() != generation {
            return Err(changed());
        }
        let value = grid
            .table
            .rows
            .get(row)
            .and_then(|row| row.get(column))
            .ok_or_else(missing)?;
        let mut page = match value {
            Cell::Null => json!({"kind":"null","value":null,"next_offset":null}),
            Cell::Text(text) | Cell::Number(text) => {
                let mut page = crate::agent::text_page(text, offset)?;
                page["kind"] = json!(if matches!(value, Cell::Number(_)) {
                    "number"
                } else {
                    "text"
                });
                page
            }
            Cell::Binary(bytes) => {
                if offset > bytes.len() {
                    return Err(Error::new(
                        "invalid_offset",
                        "Use the next_offset from the previous cell block.",
                    ));
                }
                let end = offset.saturating_add(TEXT_BYTES).min(bytes.len());
                json!({"kind":"binary","base64":base64::engine::general_purpose::STANDARD.encode(&bytes[offset..end]),"offset":offset,"next_offset":(end < bytes.len()).then_some(end),"total_bytes":bytes.len()})
            }
        };
        page["generation"] = json!(generation);
        page["pending_edit"] = json!(grid.edits.changed(row, column));
        Ok(page)
    }
}
fn changed() -> Error {
    Error::new(
        "result_changed",
        "The result changed. Read its first page again.",
    )
}
fn cell(value: Cell<'_>) -> Value {
    let size = match value {
        Cell::Null => 0,
        Cell::Text(text) | Cell::Number(text) => text.len(),
        Cell::Binary(bytes) => bytes.len(),
    };
    let kind = match value {
        Cell::Null => "null",
        Cell::Text(_) => "text",
        Cell::Number(_) => "number",
        Cell::Binary(_) => "binary",
    };
    if size > 4096 {
        return json!({"kind":kind,"bytes":size,"needs_cell_read":true});
    }
    match value {
        Cell::Null => json!({"kind":"null","value":null}),
        Cell::Text(text) | Cell::Number(text) => json!({"kind":kind,"value":text}),
        Cell::Binary(bytes) => {
            json!({"kind":"binary","base64":base64::engine::general_purpose::STANDARD.encode(&*bytes)})
        }
    }
}
