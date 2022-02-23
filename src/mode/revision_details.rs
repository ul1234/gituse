use std::thread;

use crate::{
    backend::{RevisionEntry, RevisionInfo},
    mode::*,
    platform::Key,
    ui::{Drawer, RESERVED_LINES_COUNT},
};

pub enum Response {
    Info(RevisionInfo),
}

#[derive(Clone, Debug)]
enum State {
    Idle,
    Waiting,
}
impl Default for State {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Default, Clone, Debug)]
pub struct Mode {
    state: State,
    entries: Vec<RevisionEntry>,
    output: Output,
    select: SelectMenu,
    filter: Filter,
    show_full_message: bool,
    revision: String,
    from: ModeKind,
}
impl Mode {
    fn get_selected_entries(&self) -> Vec<RevisionEntry> {
        self.entries.iter().filter(|&e| e.selected).cloned().collect()
    }
}

impl ModeTrait for Mode {
    fn on_enter(&mut self, ctx: &ModeContext, info: ModeChangeInfo) {
        if let State::Waiting = self.state {
            return;
        }
        self.state = State::Waiting;

        self.output.set(String::new());
        self.filter.clear();
        self.select.cursor = 0;
        self.show_full_message = false;
        self.from = info.from;
        self.revision = as_variant!(info.info.unwrap(), ModeInfo::RevisionDetails).unwrap();

        let ctx = ctx.clone();
        let revision = self.revision.clone();
        thread::spawn(move || {
            let mut info = match ctx.backend.revision_details(&revision) {
                Ok(info) => info,
                Err(error) => RevisionInfo { message: error, entries: Vec::new() },
            };
            info.entries.sort_unstable_by(|a, b| a.status.cmp(&b.status));

            ctx.event_sender.send_response(ModeResponse::RevisionDetails(Response::Info(info)));
        });
    }

    fn on_key(&mut self, ctx: &ModeContext, key: Key) -> ModeStatus {
        if self.filter.has_focus() {
            self.filter.on_key(key);
            self.filter.filter(self.entries.iter());
            self.select.saturate_cursor(self.filter.visible_indices().len());

            return ModeStatus { pending_input: true };
        }

        if let State::Idle = self.state {
            let available_height = (ctx.viewport_size.1 as usize).saturating_sub(RESERVED_LINES_COUNT);
            let line_count = if self.show_full_message { self.output.line_count() } else { 1 };

            match self.select.on_key(
                self.filter.visible_indices().len(),
                available_height.saturating_sub(line_count + 1),
                key,
            ) {
                SelectMenuAction::None => (),
                SelectMenuAction::Toggle(i) => {
                    if let Some(i) = self.filter.get_visible_index(i) {
                        self.entries[i].selected = !self.entries[i].selected
                    }
                }
                SelectMenuAction::ToggleAll => {
                    let all_selected = self.filter.visible_indices().iter().all(|&i| self.entries[i].selected);
                    for &i in self.filter.visible_indices() {
                        self.entries[i].selected = !all_selected;
                    }
                }
            }

            match key {
                Key::Ctrl('f') => self.filter.enter(),
                Key::Tab => {
                    self.show_full_message = !self.show_full_message;
                }
                Key::Enter => {
                    if !self.entries.is_empty() {
                        let entries = self.get_selected_entries();
                        let ctx = ctx.clone();
                        let revision = self.revision.clone();

                        thread::spawn(move || {
                            ctx.event_sender.send_mode_change(ModeKind::Diff, ModeChangeInfo::new(ModeKind::RevisionDetails));

                            let output = match ctx.backend.diff(Some(&revision), &entries) {
                                Ok(output) => output,
                                Err(error) => error,
                            };
                            ctx.event_sender.send_response(ModeResponse::Diff(diff::Response::Refresh(output)));
                        });
                    }
                }
                _ => (),
            }
        }

        ModeStatus { pending_input: false }
    }

    fn on_response(&mut self, _ctx: &ModeContext, response: ModeResponse) {
        let response = as_variant!(response, ModeResponse::RevisionDetails).unwrap();
        match response {
            Response::Info(info) => {
                if let State::Waiting = self.state {
                    self.state = State::Idle;
                }

                self.output.set(info.message);
                self.entries = info.entries;

                self.filter.filter(self.entries.iter());
                self.select.saturate_cursor(self.filter.visible_indices().len());
            }
        }
    }

    fn is_waiting_response(&self) -> bool {
        match self.state {
            State::Idle => false,
            State::Waiting => true,
        }
    }

    fn header(&self) -> (&str, &str, &str) {
        (
            "revision details",
            "[enter]diff",
            "[tab]full message [Left]back [arrows]move [space]toggle [a]toggle all [ctrl+f]filter",
        )
    }

    fn draw(&self, drawer: &mut Drawer) {
        let filter_line_count = drawer.filter(&self.filter);

        let line_count = if self.show_full_message {
            drawer.output(&self.output)
        } else {
            let output = self.output.text().lines().next().unwrap_or("");
            let output = match output.char_indices().nth(drawer.viewport_size.0.saturating_sub(1) as _) {
                Some((i, c)) => &output[..i + c.len_utf8()],
                None => output,
            };
            drawer.str(output);
            drawer.next_line();
            1
        };

        let line_count = filter_line_count + line_count;

        if let State::Idle = self.state {
            drawer.next_line();
            drawer.select_menu(
                &self.select,
                (line_count + 1).min(u16::MAX as _) as _,
                false,
                self.filter.visible_indices().iter().map(|&i| &self.entries[i]),
            );
        }
    }
}
