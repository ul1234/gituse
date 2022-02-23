use std::thread;

use crate::{
    backend::{Backend, BackendResult, TagEntry},
    mode::*,
    platform::Key,
    ui::{Drawer, SelectEntryDraw, RESERVED_LINES_COUNT},
};

pub enum Response {
    Refresh(BackendResult<Vec<TagEntry>>),
    Checkout,
    New(String),
}

#[derive(Clone, Debug)]
enum WaitOperation {
    Refresh,
    New,
    Delete,
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

impl SelectEntryDraw for TagEntry {
    fn draw(&self, drawer: &mut Drawer, _: bool, _: bool) -> usize {
        drawer.str(&self.name);
        1
    }
}

#[derive(Default, Clone, Debug)]
pub struct Mode {
    state: State,
    entries: Vec<TagEntry>,
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
                    let name = entry.name.clone();
                    let ctx = ctx.clone();
                    thread::spawn(move || match ctx.backend.checkout(&name) {
                        Ok(()) => {
                            ctx.event_sender.send_response(ModeResponse::Tags(Response::Checkout));
                            ctx.event_sender.send_mode_change(ModeKind::Log, ModeChangeInfo::new(ModeKind::Tags));
                        }
                        Err(error) => ctx.event_sender.send_response(ModeResponse::Tags(Response::Refresh(Err(error)))),
                    });
                }
            }
            Key::Char('n') => {
                let not_empty = true;
                let placeholder = "type in the tag name...";
                let on_submit = |ctx: &ModeContext, message: String| {
                    ctx.event_sender.send_response(ModeResponse::Tags(Response::New(message)));
                };
                ctx.event_sender.send_mode_change(
                    ModeKind::MessageInput,
                    ModeChangeInfo::message_input(ModeKind::Branches, not_empty, placeholder, on_submit),
                );
            }
            Key::Char('D') => {
                if let Some(current_entry_index) = current_entry_index {
                    let entry = &self.entries[current_entry_index];
                    self.state = State::Waiting(WaitOperation::Delete);

                    let name = entry.name.clone();
                    self.entries.remove(current_entry_index);
                    self.filter.on_remove_entry(current_entry_index);
                    self.select.on_remove_entry(self.select.cursor);
                    request(ctx, move |b| b.delete_tag(&name));
                }
            }
            _ => (),
        }

        ModeStatus { pending_input: false }
    }

    fn on_response(&mut self, ctx: &ModeContext, response: ModeResponse) {
        let response = as_variant!(response, ModeResponse::Tags).unwrap();
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
            Response::Checkout => self.state = State::Idle,
            Response::New(name) => {
                self.state = State::Waiting(WaitOperation::New);
                request(ctx, move |b| b.new_tag(&name));
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
            State::Idle | State::Waiting(WaitOperation::Refresh) => "tags",
            State::Waiting(WaitOperation::New) => "new tag",
            State::Waiting(WaitOperation::Delete) => "delete tag",
        };
        let (left_help, right_help) = ("[enter]checkout [n]new [D]delete", "[arrows]move [ctrl+f]filter");
        (name, left_help, right_help)
    }

    fn draw(&self, drawer: &mut Drawer) {
        let filter_line_count = drawer.filter(&self.filter);
        if self.output.text.is_empty() {
            drawer.select_menu(
                &self.select,
                filter_line_count,
                false,
                self.filter.visible_indices().iter().map(|&i| &self.entries[i]),
            );
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

        let mut result = f(ctx.backend.deref()).and_then(|_| ctx.backend.tags());
        if let Ok(entries) = &mut result {
            entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        }

        ctx.event_sender.send_response(ModeResponse::Tags(Response::Refresh(result)));
    });
}
