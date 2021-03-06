use crate::{
    backend::{Backend, BackendResult, LogEntry},
    mode::*,
    platform::Key,
    ui::{Color, Drawer, SelectEntryDraw, RESERVED_LINES_COUNT},
};
use std::thread;

pub enum Response {
    Refresh(BackendResult<(usize, Vec<LogEntry>)>),
}

#[derive(Clone, Debug)]
enum WaitOperation {
    Refresh,
    Checkout,
    Merge,
    Fetch,
    Pull,
    Push,
    Reset,
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

impl SelectEntryDraw for LogEntry {
    fn draw(&self, drawer: &mut Drawer, hovered: bool, full: bool) -> usize {
        fn color(color: Color, hovered: bool) -> Color {
            if hovered {
                Color::White
            } else {
                color
            }
        }

        const MAX_AUTHOR_CHAR_COUNT: usize = 18;
        let author = match self.author.char_indices().nth(MAX_AUTHOR_CHAR_COUNT) {
            Some((i, _)) => &self.author[..i],
            None => &self.author,
        };

        let mut total_chars = self.graph.chars().count()
            + 1
            + self.hash.chars().count()
            + 1
            + self.date.chars().count()
            + 1
            + author.chars().count()
            + 1;

        if !self.refs.is_empty() {
            total_chars += self.refs.chars().count() + 3;
        }

        let (line_count, message) = if full {
            let mut line_count = 0;
            for line in self.message.lines() {
                let mut x = 0;
                for _ in line.chars() {
                    if x >= drawer.viewport_size.0 as _ {
                        x -= drawer.viewport_size.0 as usize;
                        line_count += 1;
                    }
                }

                line_count += 1;
            }
            (line_count, &self.message[..])
        } else {
            let available_width = (drawer.viewport_size.0 as usize).saturating_sub(total_chars);
            let message = self.message.lines().next().unwrap_or("");
            let message = match message.char_indices().nth(available_width) {
                Some((i, _)) => &message[..i],
                None => &message,
            };
            (0, message)
        };

        let (refs_begin, refs_end) = match &self.refs[..] {
            "" => ("", ""),
            _ => ("(", ") "),
        };

        drawer.fmt(format_args!(
            "{}{} {}{} {}{} {}{} {}{}{}{}{}",
            color(Color::White, hovered),
            &self.graph,
            color(Color::DarkYellow, hovered),
            &self.hash,
            color(Color::DarkBlue, hovered),
            &self.date,
            color(Color::DarkGreen, hovered),
            author,
            color(Color::DarkRed, hovered),
            refs_begin,
            &self.refs,
            refs_end,
            color(Color::White, hovered),
        ));

        if full {
            drawer.next_line();
        }

        let mut lines = message.lines();
        if let Some(line) = lines.next() {
            drawer.str(line);
        }
        for line in lines {
            drawer.next_line();
            drawer.str(line);
        }

        1 + line_count
    }
}

#[derive(Default, Clone, Debug)]
pub struct Mode {
    state: State,
    entries: Vec<LogEntry>,
    output: Output,
    select: SelectMenu,
    filter: Filter,
    show_full_hovered_message: bool,
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
        self.show_full_hovered_message = false;

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
        self.select.on_key(self.filter.visible_indices().len(), available_height, key);

        let current_entry_index = self.filter.get_visible_index(self.select.cursor);
        if matches!(self.state, State::Idle) && current_entry_index.map(|i| i + 1 == self.entries.len()).unwrap_or(false) {
            self.state = State::Waiting(WaitOperation::Refresh);
            let start = self.entries.len();
            let ctx = ctx.clone();
            thread::spawn(move || {
                let result = ctx.backend.log(start, available_height);
                ctx.event_sender.send_response(ModeResponse::Log(Response::Refresh(result)));
            });
        }

        if let Key::Enter = key {
            if let Some(current_entry_index) = current_entry_index {
                let entry = &self.entries[current_entry_index];
                ctx.event_sender
                    .send_mode_change(ModeKind::RevisionDetails, ModeChangeInfo::revision(ModeKind::Log, entry.hash.clone()));
            }
        } else if let Key::Tab = key {
            self.show_full_hovered_message = !self.show_full_hovered_message;
        } else if let Key::Ctrl('f') = key {
            self.filter.enter();
        } else if let State::Idle = self.state {
            match key {
                Key::Char('c') => {
                    if let Some(current_entry_index) = current_entry_index {
                        let entry = &self.entries[current_entry_index];
                        self.state = State::Waiting(WaitOperation::Checkout);
                        let revision = entry.hash.clone();
                        request(ctx, move |b| b.checkout(&revision));
                    }
                }
                Key::Char('r') => {
                    if let Some(current_entry_index) = current_entry_index {
                        let entry = &self.entries[current_entry_index];
                        self.state = State::Waiting(WaitOperation::Reset);
                        let revision = entry.hash.clone();
                        request(ctx, move |b| b.reset(&revision));
                    }
                }
                Key::Char('R') => {
                    self.state = State::Waiting(WaitOperation::Reset);
                    request(ctx, move |b| b.reset(""));
                }
                Key::Char('m') => {
                    if let Some(current_entry_index) = current_entry_index {
                        let entry = &self.entries[current_entry_index];
                        self.state = State::Waiting(WaitOperation::Merge);
                        let revision = entry.hash.clone();
                        request(ctx, move |b| b.merge(&revision));
                    }
                }
                Key::Char('f') => {
                    self.state = State::Waiting(WaitOperation::Fetch);
                    request(ctx, Backend::fetch);
                }
                Key::Char('p') => {
                    self.state = State::Waiting(WaitOperation::Pull);
                    request(ctx, Backend::pull);
                }
                Key::Char('P') => {
                    self.state = State::Waiting(WaitOperation::Push);
                    request(ctx, Backend::push);
                }
                Key::Char('g') => {
                    self.state = State::Waiting(WaitOperation::Push);
                    request(ctx, Backend::push_gerrit); // push to gerrit
                }
                _ => (),
            }
        }

        ModeStatus { pending_input: false }
    }

    fn on_response(&mut self, _ctx: &ModeContext, response: ModeResponse) {
        let response = as_variant!(response, ModeResponse::Log).unwrap();
        match response {
            Response::Refresh(result) => {
                self.output.set(String::new());

                if let State::Waiting(_) = self.state {
                    self.state = State::Idle;
                }
                if let State::Idle = self.state {
                    match result {
                        Ok((start_index, entries)) => {
                            self.entries.truncate(start_index);
                            self.entries.extend(entries);
                        }
                        Err(error) => {
                            self.entries.clear();
                            self.output.set(error);
                        }
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
            State::Idle | State::Waiting(WaitOperation::Refresh) => "log",
            State::Waiting(WaitOperation::Reset) => "reset",
            State::Waiting(WaitOperation::Checkout) => "checkout",
            State::Waiting(WaitOperation::Merge) => "merge",
            State::Waiting(WaitOperation::Fetch) => "fetch",
            State::Waiting(WaitOperation::Pull) => "pull",
            State::Waiting(WaitOperation::Push) => "push",
        };

        let left_help = "[c]checkout [enter]details [f]fetch [p]pull [P]push [g]gerrit [r]reset [R]reset to remote";
        let right_help = "[tab]full message [Left]back [arrows]move [ctrl+f]filter";
        (name, left_help, right_help)
    }

    fn draw(&self, drawer: &mut Drawer) {
        let filter_line_count = drawer.filter(&self.filter);
        if self.output.text().is_empty() {
            drawer.select_menu(
                &self.select,
                filter_line_count,
                self.show_full_hovered_message,
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

        let available_height = (ctx.viewport_size.1 as usize).saturating_sub(RESERVED_LINES_COUNT);
        let result = f(ctx.backend.deref()).and_then(|_| ctx.backend.log(0, available_height));
        //println!("result: {:?}", result);
        ctx.event_sender.send_response(ModeResponse::Log(Response::Refresh(result)));
    });
}
