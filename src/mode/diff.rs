use crate::{
    mode::*,
    platform::Key,
    ui::{Drawer, RESERVED_LINES_COUNT},
};

pub enum Response {
    Refresh(String),
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
    from: ModeKind,
}

impl ModeTrait for Mode {
    fn on_enter(&mut self, _ctx: &ModeContext, info: ModeChangeInfo) {
        if let State::Waiting = self.state {
            return;
        }
        self.state = State::Waiting;
        self.from = info.from;
        self.output.set(String::new());
    }

    fn on_key(&mut self, ctx: &ModeContext, key: Key) -> ModeStatus {
        match self.state {
            State::Idle => {
                if self.output.line_count() > 1 {
                    let available_height = (ctx.viewport_size.1 as usize).saturating_sub(RESERVED_LINES_COUNT);
                    self.output.on_key(available_height, key);
                }
            }
            _ => (),
        }

        ModeStatus { pending_input: false }
    }

    fn on_response(&mut self, _ctx: &ModeContext, response: ModeResponse) {
        let response = as_variant!(response, ModeResponse::Diff).unwrap();
        match response {
            Response::Refresh(info) => {
                if let State::Waiting = self.state {
                    self.state = State::Idle;
                }
                if let State::Idle = self.state {
                    self.output.set(info);
                }
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
        ("details", "", "[Left]back [arrows]move")
    }

    fn draw(&self, drawer: &mut Drawer) {
        //log(format!("start to draw diff: \n"));
        drawer.diff(&self.output);
    }
}
