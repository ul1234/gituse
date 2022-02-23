use std::thread;

use crate::{
    backend::{Backend, BackendResult, BranchEntry},
    mode::*,
    platform::Key,
    ui::{Drawer, SelectEntryDraw, RESERVED_LINES_COUNT},
};

pub enum Response {
    Refresh(BackendResult<Vec<BranchEntry>>),
    Checkout(usize),
    New(String),
    Merge,
}

#[derive(Clone, Debug)]
enum WaitOperation {
    Refresh,
    New,
    Delete,
    Merge,
    Checkout,
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

impl SelectEntryDraw for BranchEntry {
    fn draw(&self, drawer: &mut Drawer, _: bool, _: bool) -> usize {
        let status = if self.checked_out { " (checked out)" } else { "" };
        drawer.fmt(format_args!("{}{}", self.name, status));
        1
    }
}

#[derive(Default, Clone, Debug)]
pub struct Mode {
    state: State,
    entries: Vec<BranchEntry>,
    output: Output,
    select: SelectMenu,
    filter: Filter,
}

impl Mode {
    fn set_checkout(&mut self, entry_index: usize) {
        for entry in &mut self.entries {
            entry.checked_out = false;
        }

        self.entries[entry_index].checked_out = true;
    }
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

                    if entry.checked_out {
                        ctx.event_sender.send_mode_change(ModeKind::Log, ModeChangeInfo::new(ModeKind::Branches));
                    } else {
                        self.state = State::Waiting(WaitOperation::Checkout);

                        thread::spawn(move || match ctx.backend.checkout(&name) {
                            Ok(()) => {
                                ctx.event_sender
                                    .send_response(ModeResponse::Branches(Response::Checkout(current_entry_index)));
                                ctx.event_sender.send_mode_change(ModeKind::Log, ModeChangeInfo::new(ModeKind::Branches));
                            }
                            Err(error) => {
                                ctx.event_sender.send_response(ModeResponse::Branches(Response::Refresh(Err(error))));
                            }
                        });
                    }
                }
            }
            Key::Char('n') => {
                let not_empty = true;
                let placeholder = "type in the branch name...";
                let on_submit = |ctx: &ModeContext, message: String| {
                    ctx.event_sender.send_response(ModeResponse::Branches(Response::New(message)));
                };
                ctx.event_sender.send_mode_change(
                    ModeKind::MessageInput,
                    ModeChangeInfo::message_input(ModeKind::Branches, not_empty, placeholder, on_submit),
                );
            }
            c @ Key::Char('D') | c @ Key::Char('d') => {
                if let Some(current_entry_index) = current_entry_index {
                    let entry = &self.entries[current_entry_index];
                    self.state = State::Waiting(WaitOperation::Delete);

                    let name = entry.name.clone();
                    self.entries.remove(current_entry_index);
                    self.filter.on_remove_entry(current_entry_index);
                    self.select.on_remove_entry(self.select.cursor);

                    let force = c == Key::Char('D'); // D means force delete

                    request(ctx, move |b| b.delete_branch(&name, force));
                }
            }
            Key::Char('m') => {
                if let Some(current_entry_index) = current_entry_index {
                    let entry = &self.entries[current_entry_index];
                    self.state = State::Waiting(WaitOperation::Merge);

                    let name = entry.name.clone();
                    let ctx = ctx.clone();
                    thread::spawn(move || match ctx.backend.merge(&name) {
                        Ok(()) => {
                            ctx.event_sender.send_response(ModeResponse::Branches(Response::Merge));
                            ctx.event_sender.send_mode_change(ModeKind::Log, ModeChangeInfo::new(ModeKind::Branches));
                        }
                        Err(error) => {
                            ctx.event_sender.send_mode_change(ModeKind::Log, ModeChangeInfo::new(ModeKind::Branches));
                            ctx.event_sender.send_response(ModeResponse::Branches(Response::Refresh(Err(error))));
                        }
                    });
                }
            }
            _ => (),
        }

        ModeStatus { pending_input: false }
    }

    fn on_response(&mut self, ctx: &ModeContext, response: ModeResponse) {
        let response = as_variant!(response, ModeResponse::Branches).unwrap();
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

                if let Some(i) = self.entries.iter().position(|e| e.checked_out) {
                    if let Ok(i) = self.filter.visible_indices().binary_search(&i) {
                        self.select.cursor = i;
                    }
                }
            }
            Response::Checkout(entry_index) => {
                self.state = State::Idle;
                self.set_checkout(entry_index);
            }
            Response::Merge => self.state = State::Idle,
            Response::New(message) => {
                self.state = State::Waiting(WaitOperation::New);
                request(ctx, move |b| b.new_branch(&message));
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
            State::Idle | State::Waiting(WaitOperation::Refresh) => "branches",
            State::Waiting(WaitOperation::New) => "new branch",
            State::Waiting(WaitOperation::Delete) => "delete branch",
            State::Waiting(WaitOperation::Merge) => "merge branch",
            State::Waiting(WaitOperation::Checkout) => "checkout",
        };
        let (left_help, right_help) =
            ("[enter]checkout [n]new [d]delete [D]force delete [m]merge", "[arrows]move [ctrl+f]filter");
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

        let mut result = f(ctx.backend.deref()).and_then(|_| ctx.backend.branches());
        if let Ok(entries) = &mut result {
            entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        }

        ctx.event_sender.send_response(ModeResponse::Branches(Response::Refresh(result)));
    });
}
