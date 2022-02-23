use std::thread;

use crate::{
    backend::BackendResult,
    mode::*,
    platform::Key,
    ui::{Drawer, RESERVED_LINES_COUNT},
};

pub enum Response {
    Refresh(BackendResult<String>),
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
    output: Output,
    stash_id: usize,
    from: ModeKind,
}

impl ModeTrait for Mode {
    fn on_enter(&mut self, ctx: &ModeContext, info: ModeChangeInfo) {
        if let State::Waiting = self.state {
            return;
        }
        self.state = State::Waiting;

        self.output.set(String::new());
        self.from = info.from;
        self.stash_id = as_variant!(info.info.unwrap(), ModeInfo::StashDetails).unwrap();

        let stash_id = self.stash_id;
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = ctx.backend.stash_show(stash_id);
            ctx.event_sender.send_response(ModeResponse::StashDetails(Response::Refresh(result)));
        });
    }

    fn on_key(&mut self, ctx: &ModeContext, key: Key) -> ModeStatus {
        if let State::Idle = self.state {
            if self.output.line_count() > 1 {
                let available_height = (ctx.viewport_size.1 as usize).saturating_sub(RESERVED_LINES_COUNT);
                self.output.on_key(available_height, key);
            }

            match key {
                Key::Enter => {
                    let stash_id = self.stash_id;
                    let ctx = ctx.clone();
                    thread::spawn(move || {
                        ctx.event_sender.send_mode_change(ModeKind::Diff, ModeChangeInfo::new(ModeKind::StashDetails));

                        let output = match ctx.backend.stash_diff(stash_id) {
                            Ok(info) => info,
                            Err(error) => error,
                        };
                        ctx.event_sender.send_response(ModeResponse::Diff(diff::Response::Refresh(output)));
                    });
                }
                _ => (),
            }
        }

        ModeStatus { pending_input: false }
    }

    fn on_response(&mut self, _ctx: &ModeContext, response: ModeResponse) {
        let response = as_variant!(response, ModeResponse::StashDetails).unwrap();
        match response {
            Response::Refresh(result) => {
                if let State::Waiting = self.state {
                    self.state = State::Idle;
                }
                let info = match result {
                    Ok(info) => info,
                    Err(error) => error,
                };
                // if info.is_empty() {
                //     info.push('\n');
                // }
                self.output.set(info);
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
        ("stash details", "[enter]diff", "[Left]back [arrows]move")
    }

    fn draw(&self, drawer: &mut Drawer) {
        drawer.stash_details(&self.output);
    }
}
