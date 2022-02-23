use std::thread;

use crate::{
    backend::{Backend, BackendResult, StashEntry},
    mode::*,
    platform::Key,
    ui::{Color, Drawer, SelectEntryDraw, RESERVED_LINES_COUNT},
};

pub enum Response {
    Refresh(BackendResult<Vec<StashEntry>>),
}

#[derive(Clone, Debug)]
enum WaitOperation {
    Refresh,
    Discard,
}

#[derive(Clone, Debug)]
enum State {
    Idle,
    Waiting(WaitOperation),
}
impl Default for State {
    fn default() -> Self {
        Self::Idle
    }
}

impl SelectEntryDraw for StashEntry {
    fn draw(&self, drawer: &mut Drawer, hovered: bool, _: bool) -> usize {
        fn color(color: Color, hovered: bool) -> Color {
            if hovered {
                Color::White
            } else {
                color
            }
        }

        drawer.fmt(format_args!(
            "{}[{}] {}{} {}{}",
            color(Color::DarkYellow, hovered),
            self.id,
            color(Color::DarkGreen, hovered),
            &self.branch,
            color(Color::White, hovered),
            &self.message
        ));
        1
    }
}
#[derive(Default, Clone, Debug)]
pub struct Mode {
    state: State,
    entries: Vec<StashEntry>,
    output: Output,
    select: SelectMenu,
    filter: Filter,
}

impl ModeTrait for Mode {
    fn on_enter(&mut self, ctx: &ModeContext, _info: ModeChangeInfo) {
        if let State::Waiting(_) = self.state {
            return;
        }
        self.state = State::Waiting(WaitOperation::Refresh);

        self.output.set(String::new());
        self.filter.filter(self.entries.iter());
        self.select.saturate_cursor(self.filter.visible_indices().len());

        request(ctx, |_| Ok(()));
    }

    fn on_key(&mut self, ctx: &ModeContext, key: Key) -> ModeStatus {
        if self.filter.has_focus() {
            self.filter.on_key(key);
            self.filter.filter(self.entries.iter());
            self.select.saturate_cursor(self.filter.visible_indices().len());

            return ModeStatus { pending_input: true };
        }

        let available_height = (ctx.viewport_size.1 as usize).saturating_sub(RESERVED_LINES_COUNT);
        if self.output.text().is_empty() {
            self.select.on_key(self.filter.visible_indices().len(), available_height, key);
        } else {
            self.output.on_key(available_height, key);
        }

        let current_entry_index = self.filter.get_visible_index(self.select.cursor);
        match key {
            Key::Ctrl('f') => self.filter.enter(),
            Key::Enter => {
                if let Some(current_entry_index) = current_entry_index {
                    let entry = &self.entries[current_entry_index];
                    let id = entry.id;

                    ctx.event_sender.send_mode_change(ModeKind::StashDetails, ModeChangeInfo::stash(ModeKind::Log, id));
                }
            }
            Key::Char('p') => {
                if let Some(current_entry_index) = current_entry_index {
                    let entry = &self.entries[current_entry_index];
                    let id = entry.id;
                    let ctx = ctx.clone();

                    thread::spawn(move || match ctx.backend.stash_pop(id) {
                        Ok(()) => {
                            ctx.event_sender.send_mode_change(ModeKind::Status, ModeChangeInfo::new(ModeKind::Stash));
                        }
                        Err(error) => ctx.event_sender.send_response(ModeResponse::Stash(Response::Refresh(Err(error)))),
                    });
                }
            }
            Key::Char('D') => {
                if let Some(current_entry_index) = current_entry_index {
                    self.state = State::Waiting(WaitOperation::Discard);
                    let entry = &self.entries[current_entry_index];
                    let id = entry.id;

                    request(ctx, move |b| b.stash_drop(id));
                }
            }
            _ => (),
        }

        ModeStatus { pending_input: false }
    }

    fn on_response(&mut self, _ctx: &ModeContext, response: ModeResponse) {
        let response = as_variant!(response, ModeResponse::Stash).unwrap();
        match response {
            Response::Refresh(result) => {
                self.entries = Vec::new();
                self.output.set(String::new());

                if let State::Waiting(_) = self.state {
                    self.state = State::Idle;
                }
                if let State::Idle = self.state {
                    match result {
                        Ok(entries) => self.entries = entries,
                        Err(error) => self.output.set(error),
                    }
                }

                self.filter.filter(self.entries.iter());
                self.select.saturate_cursor(self.filter.visible_indices().len());
            }
        }
    }

    fn is_waiting_response(&self) -> bool {
        match self.state {
            State::Idle => false,
            State::Waiting(_) => true,
        }
    }

    fn header(&self) -> (&str, &str, &str) {
        let name = match self.state {
            State::Idle | State::Waiting(WaitOperation::Refresh) => "stash list",
            State::Waiting(WaitOperation::Discard) => "discard",
        };

        let (left_help, right_help) = ("[p]pop [enter]details [D]discard", "[arrows]move [ctrl+f]filter");
        (name, left_help, right_help)
    }

    fn draw(&self, drawer: &mut Drawer) {
        let filter_line_count = drawer.filter(&self.filter);
        if self.output.text.is_empty() {
            if self.entries.is_empty() {
                if let State::Idle = self.state {
                    drawer.output(&Output::new("No Stashes!".to_owned()));
                }
            } else {
                drawer.select_menu(
                    &self.select,
                    filter_line_count,
                    false,
                    self.filter.visible_indices().iter().map(|&i| &self.entries[i]),
                );
            }
        } else {
            drawer.output(&self.output);
        }
    }
}

fn request<F>(ctx: &ModeContext, f: F)
where
    F: 'static + Send + Sync + FnOnce(&dyn Backend) -> BackendResult<()>,
{
    let ctx = ctx.clone();
    thread::spawn(move || {
        use std::ops::Deref;

        let result = f(ctx.backend.deref()).and_then(|_| ctx.backend.stash_list());

        ctx.event_sender.send_response(ModeResponse::Stash(Response::Refresh(result)));
    });
}
