use std::thread;

use crate::{
    backend::{Backend, BackendResult, FileStatus, RevisionEntry, StatusInfo},
    mode::*,
    platform::Key,
    ui::{Color, Drawer, SelectEntryDraw, RESERVED_LINES_COUNT},
};

pub enum Response {
    Idle,
    Refresh(StatusInfo),
    Commit(String),
    Stash(String),
}

#[derive(Clone, Debug)]
enum WaitOperation {
    Refresh,
    Commit,
    Discard,
    Stash,
    ResolveTakingOurs,
    ResolveTakingTheirs,
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

impl SelectEntryDraw for RevisionEntry {
    fn draw(&self, drawer: &mut Drawer, _: bool, _: bool) -> usize {
        const NAME_TOO_LONG_PREFIX: &str = "...";

        let name_available_width = (drawer.viewport_size.0 as usize)
            .saturating_sub(2 + 1 + FileStatus::max_len() + 1 + 1 + NAME_TOO_LONG_PREFIX.len() + 1);

        let (name_prefix, trimmed_name) = match self.name.char_indices().nth_back(name_available_width) {
            Some((i, _)) => (NAME_TOO_LONG_PREFIX, &self.name[i..]),
            None => ("", &self.name[..]),
        };

        let selected_text = if self.selected { '+' } else { ' ' };
        drawer.fmt(format_args!(
            "{} [{:>width$}] {}{}",
            selected_text,
            self.status.as_str(),
            name_prefix,
            trimmed_name,
            width = FileStatus::max_len(),
        ));

        1
    }
}

#[derive(Default, Clone, Debug)]
pub struct Mode {
    state: State,
    entries: Vec<RevisionEntry>,
    output: Output,
    select: SelectMenu,
    filter: Filter,
    from: ModeKind,
}
impl Mode {
    fn get_selected_entries(&self) -> Vec<RevisionEntry> {
        let entries: Vec<_> = self.entries.iter().filter(|&e| e.selected).cloned().collect();
        entries
    }

    fn remove_selected_entries(&mut self) {
        let previous_len = self.entries.len();

        for i in (0..self.entries.len()).rev() {
            if self.entries[i].selected {
                self.entries.remove(i);
                self.filter.on_remove_entry(i);
                let i = match self.filter.visible_indices().binary_search(&i) {
                    Ok(i) => i,
                    Err(i) => i,
                };
                self.select.on_remove_entry(i);
            }
        }

        if self.entries.len() == previous_len {
            self.entries.clear();
            self.select.cursor = 0;
            self.filter.clear();
        }
    }

    fn commit<S: Into<String>>(&mut self, ctx: &ModeContext, message: S, amend: bool) {
        self.state = State::Waiting(WaitOperation::Commit);

        let entries = self.get_selected_entries();
        self.remove_selected_entries();

        let message = message.into();
        //log(format!("amend: {}, commit message: \n {:?}, entries: {:?}\n", amend, message, entries));

        let ctx = ctx.clone();
        thread::spawn(move || match ctx.backend.commit(&message, &entries, amend) {
            Ok(()) => {
                log(format!("commit ok\n"));
                ctx.event_sender.send_response(ModeResponse::Status(Response::Idle));
                ctx.event_sender.send_mode_change(ModeKind::Log, ModeChangeInfo::new(ModeKind::Status));
            }
            Err(error) => ctx
                .event_sender
                .send_response(ModeResponse::Status(Response::Refresh(StatusInfo { header: error, entries: Vec::new() }))),
        });
    }
}

impl ModeTrait for Mode {
    fn on_enter(&mut self, ctx: &ModeContext, info: ModeChangeInfo) {
        if let State::Waiting(_) = self.state {
            return;
        }
        self.state = State::Waiting(WaitOperation::Refresh);

        self.output.set(String::new());
        self.filter.filter(self.entries.iter());
        self.select.saturate_cursor(self.filter.visible_indices().len());
        self.from = info.from;

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
        if self.output.line_count() > 1 {
            self.output.on_key(available_height, key);
        } else {
            match self.select.on_key(self.filter.visible_indices().len(), available_height.saturating_sub(2), key) {
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
        }

        match key {
            Key::Ctrl('f') => self.filter.enter(),
            Key::Char('c') => {
                if !self.entries.is_empty() {
                    let not_empty = true;
                    let placeholder = "type in the commit message...";
                    let on_submit = |ctx: &ModeContext, message: String| {
                        ctx.event_sender.send_response(ModeResponse::Status(Response::Commit(message)));
                    };
                    ctx.event_sender.send_mode_change(
                        ModeKind::MessageInput,
                        ModeChangeInfo::message_input(ModeKind::Status, not_empty, placeholder, on_submit),
                    );
                }
            }
            Key::Char('A') => {
                if !self.entries.is_empty() {
                    self.commit(ctx, "", true);
                }
            }
            Key::Char('D') => {
                if matches!(self.state, State::Idle) && !self.entries.is_empty() {
                    self.state = State::Waiting(WaitOperation::Discard);
                    let entries = self.get_selected_entries();
                    self.remove_selected_entries();

                    request(ctx, move |b| b.discard(&entries));
                }
            }
            Key::Char('O') => {
                if matches!(self.state, State::Idle) && !self.entries.is_empty() {
                    self.state = State::Waiting(WaitOperation::ResolveTakingOurs);
                    let entries = self.get_selected_entries();

                    request(ctx, move |b| b.resolve_taking_ours(&entries));
                }
            }
            Key::Char('T') => {
                if matches!(self.state, State::Idle) && !self.entries.is_empty() {
                    self.state = State::Waiting(WaitOperation::ResolveTakingTheirs);
                    let entries = self.get_selected_entries();

                    request(ctx, move |b| b.resolve_taking_theirs(&entries));
                }
            }
            Key::Ctrl('s') => {
                if !self.entries.is_empty() {
                    let not_empty = false;
                    let placeholder = "type in the stash message...";
                    let on_submit = |ctx: &ModeContext, message: String| {
                        ctx.event_sender.send_response(ModeResponse::Status(Response::Stash(message)));
                    };
                    ctx.event_sender.send_mode_change(
                        ModeKind::MessageInput,
                        ModeChangeInfo::message_input(ModeKind::Status, not_empty, placeholder, on_submit),
                    );
                }
            }
            Key::Enter => {
                if !self.entries.is_empty() {
                    let entries = self.get_selected_entries();

                    let ctx = ctx.clone();
                    thread::spawn(move || {
                        ctx.event_sender.send_mode_change(ModeKind::Diff, ModeChangeInfo::new(ModeKind::Status));

                        let output = match ctx.backend.diff(None, &entries) {
                            Ok(output) => output,
                            Err(error) => error,
                        };
                        ctx.event_sender.send_response(ModeResponse::Diff(diff::Response::Refresh(output)));
                    });
                }
            }
            _ => (),
        }

        ModeStatus { pending_input: false }
    }

    fn on_response(&mut self, ctx: &ModeContext, response: ModeResponse) {
        let response = as_variant!(response, ModeResponse::Status).unwrap();
        match response {
            Response::Refresh(info) => {
                if let State::Waiting(_) = self.state {
                    self.state = State::Idle;
                }
                if let State::Idle = self.state {
                    self.output.set(info.header);
                }

                self.entries = info.entries;

                self.filter.filter(self.entries.iter());
                self.select.saturate_cursor(self.filter.visible_indices().len());
            }
            Response::Commit(message) => self.commit(ctx, message, false),
            Response::Stash(message) => {
                self.state = State::Waiting(WaitOperation::Stash);

                let entries = self.get_selected_entries();
                self.remove_selected_entries();

                request(ctx, move |b| b.stash(&message, &entries));
            }
            Response::Idle => {
                self.state = State::Idle;
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
            State::Idle | State::Waiting(WaitOperation::Refresh) => "status",
            State::Waiting(WaitOperation::Commit) => "commit",
            State::Waiting(WaitOperation::Stash) => "stash",
            State::Waiting(WaitOperation::Discard) => "discard",
            State::Waiting(WaitOperation::ResolveTakingOurs) => "resolve taking ours",
            State::Waiting(WaitOperation::ResolveTakingTheirs) => "resolve taking theirs",
        };
        let (left_help, right_help) = (
            "[c]commit [A]amend [D]discard [ctrl+s]stash [enter]diff [O]take ours [T]take theirs",
            "[arrows]move [space]toggle [a]toggle all [ctrl+f]filter",
        );
        (name, left_help, right_help)
    }

    fn draw(&self, drawer: &mut Drawer) {
        //log(format!("start to draw status: \n {:?}:\n", self.output.text()));
        let filter_line_count = drawer.filter(&self.filter);

        if self.output.line_count() > 1 {
            drawer.output(&self.output);
        } else {
            let output = self.output.text();
            let output =
                match output.char_indices().nth((drawer.viewport_size.0 as usize).saturating_sub(RESERVED_LINES_COUNT)) {
                    Some((i, c)) => &output[..i + c.len_utf8()],
                    None => output,
                };

            drawer.str(output);
            drawer.next_line();
            drawer.next_line();
            drawer.select_menu(
                &self.select,
                2 + filter_line_count,
                false,
                self.filter.visible_indices().iter().map(|&i| &self.entries[i]),
            );

            if self.entries.is_empty() {
                let empty_message = match self.state {
                    State::Idle => "nothing to commit!",
                    _ => "working...",
                };
                drawer.fmt(format_args!("{}{}", Color::DarkYellow, empty_message));
            }
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

        let mut info = match f(ctx.backend.deref()).and_then(|_| ctx.backend.status()) {
            Ok(info) => info,
            Err(error) => StatusInfo { header: error, entries: Vec::new() },
        };
        info.entries.sort_unstable_by(|a, b| a.status.cmp(&b.status));

        ctx.event_sender.send_response(ModeResponse::Status(Response::Refresh(info)));
    });
}
