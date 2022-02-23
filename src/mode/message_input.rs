use crate::{mode::*, platform::Key, ui::Drawer};
use std::fmt;

pub enum Response {}

#[derive(Clone)]
pub struct OnSubmit(fn(ctx: &ModeContext, message: String));
impl Default for OnSubmit {
    fn default() -> Self {
        Self(|_ctx: &ModeContext, _message: String| {})
    }
}

impl fmt::Debug for OnSubmit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "on_submit: fn")
    }
}
#[derive(Clone, Debug)]
pub struct ModeInfo {
    pub not_empty: bool, // the submit string must be not empty
    pub placeholder: String,
    pub on_submit: OnSubmit,
}
impl ModeInfo {
    pub fn new(not_empty: bool, placeholder: String, on_submit: fn(ctx: &ModeContext, message: String)) -> Self {
        Self { not_empty, placeholder, on_submit: OnSubmit(on_submit) }
    }
}

#[derive(Default, Clone, Debug)]
pub struct Mode {
    readline: ReadLine,
    from: ModeKind,
    placeholder: String,
    on_submit: OnSubmit,
    not_empty: bool,
}

impl ModeTrait for Mode {
    fn on_enter(&mut self, _ctx: &ModeContext, info: ModeChangeInfo) {
        self.readline.clear();
        self.from = info.from;
        let mode_info = as_variant!(info.info.unwrap(), super::ModeInfo::MessageInput).unwrap();
        self.placeholder = mode_info.placeholder;
        self.on_submit = mode_info.on_submit;
        self.not_empty = mode_info.not_empty;
    }

    fn on_key(&mut self, ctx: &ModeContext, key: Key) -> ModeStatus {
        self.readline.on_key(key);

        if key.is_cancel() {
            ctx.event_sender.send_mode_revert();
        } else if key.is_submit() {
            let message = self.readline.input().to_string();
            // when submit should not be empty, just do nothing if no message input
            if !(message.is_empty() && self.not_empty) {
                ctx.event_sender.send_mode_revert();
                self.on_submit.0(ctx, message);
            }
        }

        ModeStatus { pending_input: true }
    }

    fn on_response(&mut self, _ctx: &ModeContext, _response: ModeResponse) {}

    fn is_waiting_response(&self) -> bool {
        false
    }

    fn header(&self) -> (&str, &str, &str) {
        ("message input", "[enter]submit [Esc]cancel", "[Left]back")
    }

    fn draw(&self, drawer: &mut Drawer) {
        drawer.readline(&self.readline, &self.placeholder);
    }
}
