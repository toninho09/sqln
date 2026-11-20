//! Keys, mouse, and drawing of the AI prompts and of the ghost text.

use super::*;

impl App {
    /// Decides the ghost at the line of the cursor: the cursor line names the answer.
    /// A line without a ghost explains the rule.
    pub(in crate::app) fn decide_ghost(&mut self, accept: bool) {
        let row = self.console().editor.cursor().row;
        let Some(index) = self.console_mut().editor.ghost_at(row) else {
            self.status = Some("No AI answer at the line of the cursor.".into());
            return;
        };
        if let Err(error) = self.console_mut().editor.ghost_decide(index, accept) {
            self.status = Some(error);
        }
    }

    /// Escapes at the line of the cursor: the run of that line ends, and a ghost
    /// there closes. A line without either keeps everything.
    pub(in crate::app) fn escape_ghost(&mut self) {
        if self.cancel_ai_explanation(self.console().id) {
            return;
        }
        let row = self.console().editor.cursor().row;
        let console = self.console().id;
        let ids: Vec<u64> = self
            .ai
            .runs
            .iter()
            .filter_map(|r| match (&r.kind, r.console == console) {
                (RunKind::Completion(target), true) => Some((r.id, target)),
                _ => None,
            })
            .filter(|(_, target)| {
                self.tabs
                    .consoles
                    .iter()
                    .find(|c| c.id == console)
                    .is_some_and(|c| c.editor.ai_line_row(target) == Some(row))
            })
            .map(|(id, _)| id)
            .collect();
        if !ids.is_empty() {
            for id in ids {
                self.cancel_ai_id(id);
            }
            return;
        }
        if let Some(index) = self.console_mut().editor.ghost_at(row) {
            let _ = self.console_mut().editor.ghost_decide(index, false);
        }
    }

    pub(super) fn ai_obscured(&self) -> bool {
        self.top_modal().is_some()
            || self.input.command_page.is_some()
            || self.input.pending_key.is_some()
    }

    /// Returns true when a field on the current screen receives text input.
    pub(super) fn ai_input_active(&self) -> bool {
        if self.navigator_focused {
            return self.navigator.filter_is_editing();
        }
        if !self.console_on_screen() {
            return false;
        }
        let console = self.console();
        match console.focus {
            crate::console::Focus::Editor => console.editor.search().is_some(),
            crate::console::Focus::Results => console.search_is_open(),
            crate::console::Focus::Parameters => true,
        }
    }

    pub(in crate::app) fn on_ai_key(&mut self, key: KeyEvent) -> bool {
        if let Some(prompt) = &mut self.ai.request {
            match prompt.on_key(key) {
                TextAction::Cancel => self.ai.request = None,
                TextAction::Accept(text) => {
                    self.ai.request = None;
                    let instruction = if text.trim().is_empty() {
                        None
                    } else {
                        Some(text)
                    };
                    self.start_ai_with(instruction);
                }
                _ => {}
            }
            return true;
        }
        if let Some(prompt) = &mut self.ai.prompt {
            match prompt.on_key(key, false) {
                PasswordAction::Cancel => self.cancel_ai(),
                PasswordAction::Submit { password, .. } => {
                    self.ai.prompt = None;
                    if let Some(run) = self.ai.runs.iter_mut().find(|r| r.session.is_none()) {
                        run.session = Some(password);
                        run.sending = false;
                    }
                    self.continue_ai();
                }
                _ => {}
            }
            return true;
        }
        if self.ai_obscured() || self.ai_input_active() {
            return false;
        }
        if let crate::input::Lookup::Action(action) = self.keymap.lookup(
            crate::input::Region::Editor,
            crate::input::Chord::from_event(key),
        ) && matches!(
            action,
            Action::EditorAiAccept | Action::EditorAiReject | Action::EditorAiCancel
        ) && self.ai_action_available(action)
        {
            match action {
                Action::EditorAiAccept => self.decide_ghost(true),
                Action::EditorAiReject => self.decide_ghost(false),
                _ => self.cancel_ai(),
            }
            return true;
        }
        if key.code == KeyCode::Esc
            && self.ai.runs.iter().any(|r| r.console == self.console().id)
            && (self.editor_has_focus() || self.ai_explanation_running(self.console().id))
        {
            self.escape_ghost();
            return true;
        }
        false
    }

    pub(in crate::app) fn on_ai_mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.ai.request.is_some() {
            // The request input is a dialog: clicks under it start nothing.
            return true;
        }
        if self.ai.prompt.is_some() {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && let Some((field, _)) = self
                    .layout
                    .password
                    .iter()
                    .find(|(_, r)| r.contains((mouse.column, mouse.row).into()))
                && let Some(prompt) = self.ai.prompt.as_mut()
            {
                let action = prompt.click(*field);
                match action {
                    PasswordAction::Cancel => self.cancel_ai(),
                    PasswordAction::Submit { password, .. } => {
                        self.ai.prompt = None;
                        if let Some(run) = self.ai.runs.iter_mut().find(|r| r.session.is_none()) {
                            run.session = Some(password);
                            run.sending = false;
                        }
                        self.continue_ai();
                    }
                    _ => {}
                }
            }
            return true;
        }
        if self.ai_obscured() {
            return false;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self
                .ai
                .cancel_button
                .contains((mouse.column, mouse.row).into())
            && self.ai.runs.iter().any(|r| r.console == self.console().id)
        {
            // The bar belongs to the console on screen, so its button ends the runs
            // of that console.
            let console = self.console().id;
            let ids: Vec<u64> = self
                .ai
                .runs
                .iter()
                .filter(|r| r.console == console)
                .map(|r| r.id)
                .collect();
            for id in ids {
                self.cancel_ai_id(id);
            }
            return true;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self.editor_has_focus()
            && self
                .console_mut()
                .editor
                .click_suggestion(mouse.column, mouse.row)
        {
            return true;
        }
        false
    }

    pub(in crate::app) fn ai_paste(&mut self, text: &str) -> bool {
        if let Some(prompt) = &mut self.ai.request {
            prompt.paste(text);
            return true;
        }
        if let Some(prompt) = &mut self.ai.prompt {
            prompt.paste(text);
            return true;
        }
        false
    }

    pub(in crate::app) fn draw_ai(&mut self, frame: &mut Frame) {
        self.ai.cancel_button = Rect::default();
        let console = self.console().id;
        let runs: Vec<&Run> = self
            .ai
            .runs
            .iter()
            .filter(|r| r.console == console)
            .collect();
        let Some(last) = runs.last().copied() else {
            return;
        };
        let area = self.layout.console.editor;
        if area.width > 0 && area.height > 0 {
            self.ai.cancel_button =
                Rect::new(area.right().saturating_sub(10), area.bottom() - 1, 10, 1);
            let spin = ["|", "/", "-", "\\"][self.time.run_frame % 4];
            // One run names its step; several show their count.
            let state = if runs.len() == 1 {
                last.message.to_owned()
            } else {
                format!("thinking ×{}", runs.len())
            };
            frame.render_widget(
                Paragraph::new(format!(" AI {spin} {state} · Esc: Cancel ")).style(self.theme.hint),
                Rect {
                    x: area.x,
                    y: area.bottom() - 1,
                    width: area.width,
                    height: 1,
                },
            );
            frame.render_widget(
                Paragraph::new(self.theme.button("Cancel")).style(self.theme.dialog_option),
                self.ai.cancel_button,
            );
        }
    }
}
