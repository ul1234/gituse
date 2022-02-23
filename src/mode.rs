use bounded_vec_deque::BoundedVecDeque;
use std::sync::Arc;

use crate::{application::EventSender, backend::Backend, platform::Key, tool::*, ui::Drawer};

pub mod branches;
pub mod diff;
pub mod log;
pub mod message_input;
pub mod revision_details;
pub mod stash;
pub mod stash_details;
pub mod status;
pub mod tags;

pub enum ModeResponse {
    Status(status::Response),
    Log(log::Response),
    RevisionDetails(revision_details::Response),
    Branches(branches::Response),
    Tags(tags::Response),
    Stash(stash::Response),
    Diff(diff::Response),
    StashDetails(stash_details::Response),
    _MessageInput(message_input::Response),
}
impl ModeResponse {
    pub fn mode_kind(&self) -> ModeKind {
        match self {
            ModeResponse::Status(_) => ModeKind::Status,
            ModeResponse::Log(_) => ModeKind::Log,
            ModeResponse::RevisionDetails(_) => ModeKind::RevisionDetails,
            ModeResponse::Branches(_) => ModeKind::Branches,
            ModeResponse::Tags(_) => ModeKind::Tags,
            ModeResponse::Stash(_) => ModeKind::Stash,
            ModeResponse::Diff(_) => ModeKind::Diff,
            ModeResponse::StashDetails(_) => ModeKind::StashDetails,
            ModeResponse::_MessageInput(_) => ModeKind::MessageInput,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Mode {
    Status(status::Mode),
    Log(log::Mode),
    RevisionDetails(revision_details::Mode),
    Branches(branches::Mode),
    Tags(tags::Mode),
    Stash(stash::Mode),
    Diff(diff::Mode),
    StashDetails(stash_details::Mode),
    MessageInput(message_input::Mode),
}
impl Default for Mode {
    fn default() -> Self {
        Self::Status(status::Mode::default())
    }
}

impl Mode {
    fn default_from_mode_kind(mode_kind: ModeKind) -> Self {
        match mode_kind {
            ModeKind::Status => Self::Status(status::Mode::default()),
            ModeKind::Log => Self::Log(log::Mode::default()),
            ModeKind::RevisionDetails => Self::RevisionDetails(revision_details::Mode::default()),
            ModeKind::Branches => Self::Branches(branches::Mode::default()),
            ModeKind::Tags => Self::Tags(tags::Mode::default()),
            ModeKind::Stash => Self::Stash(stash::Mode::default()),
            ModeKind::Diff => Self::Diff(diff::Mode::default()),
            ModeKind::StashDetails => Self::StashDetails(stash_details::Mode::default()),
            ModeKind::MessageInput => Self::MessageInput(message_input::Mode::default()),
        }
    }

    fn mode(&mut self) -> &mut dyn ModeTrait {
        match self {
            Self::Status(mode) => mode,
            Self::Log(mode) => mode,
            Self::RevisionDetails(mode) => mode,
            Self::Branches(mode) => mode,
            Self::Tags(mode) => mode,
            Self::Stash(mode) => mode,
            Self::Diff(mode) => mode,
            Self::StashDetails(mode) => mode,
            Self::MessageInput(mode) => mode,
        }
    }

    pub fn mode_kind(&self) -> ModeKind {
        match self {
            Self::Status(_) => ModeKind::Status,
            Self::Log(_) => ModeKind::Log,
            Self::RevisionDetails(_) => ModeKind::RevisionDetails,
            Self::Branches(_) => ModeKind::Branches,
            Self::Tags(_) => ModeKind::Tags,
            Self::Stash(_) => ModeKind::Stash,
            Self::Diff(_) => ModeKind::Diff,
            Self::StashDetails(_) => ModeKind::StashDetails,
            Self::MessageInput(_) => ModeKind::MessageInput,
        }
    }
}

pub const BOUNDED_VEC_DEQUE_MAX_LEN: usize = 5;
#[derive(Debug)]
pub struct ModeBuf {
    mode: Mode,
    history: BoundedVecDeque<Mode>,
}
impl Default for ModeBuf {
    fn default() -> Self {
        Self { mode: Mode::default(), history: BoundedVecDeque::<Mode>::new(BOUNDED_VEC_DEQUE_MAX_LEN) }
    }
}

impl ModeBuf {
    pub fn mode(&mut self) -> &mut dyn ModeTrait {
        self.mode.mode()
    }

    pub fn mode_kind(&self) -> ModeKind {
        self.mode.mode_kind()
    }

    pub fn enter_mode(&mut self, ctx: &ModeContext, mode_kind: ModeKind, info: ModeChangeInfo) {
        if self.mode.mode_kind() != mode_kind {
            log(format!("before enter mode to {:?}:\n {:?}\n", mode_kind, self.mode));
            self.history.push_back(self.mode.clone());
        }
        self.mode = Mode::default_from_mode_kind(mode_kind);
        self.mode().on_enter(ctx, info);
    }

    pub fn revert_mode(&mut self, _ctx: &ModeContext) {
        //log(format!("revert: \n "));
        if let Some(mode) = self.history.pop_back() {
            log(format!("revert to mode: \n {:?}\n", mode));
            self.mode = mode;
        }
    }
}

pub struct ModeChangeInfo {
    from: ModeKind,
    info: Option<ModeInfo>,
}
pub enum ModeInfo {
    RevisionDetails(String),
    StashDetails(usize),
    MessageInput(message_input::ModeInfo),
}

impl ModeChangeInfo {
    pub fn new(from: ModeKind) -> Self {
        Self { from, info: None }
    }

    pub fn revision(from: ModeKind, revision: String) -> Self {
        Self { from, info: Some(ModeInfo::RevisionDetails(revision)) }
    }

    pub fn stash(from: ModeKind, stash_id: usize) -> Self {
        Self { from, info: Some(ModeInfo::StashDetails(stash_id)) }
    }

    pub fn message_input<S: Into<String>>(
        from: ModeKind,
        not_empty: bool,
        placeholder: S,
        on_submit: fn(&ModeContext, String),
    ) -> Self {
        Self {
            from,
            info: Some(ModeInfo::MessageInput(message_input::ModeInfo::new(not_empty, placeholder.into(), on_submit))),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum ModeKind {
    Status,
    Log,
    RevisionDetails,
    Branches,
    Tags,
    Stash,
    Diff,
    StashDetails,
    MessageInput,
}
impl Default for ModeKind {
    fn default() -> Self {
        Self::Status
    }
}

pub trait ModeTrait {
    fn on_enter(&mut self, ctx: &ModeContext, info: ModeChangeInfo);
    fn on_key(&mut self, ctx: &ModeContext, key: Key) -> ModeStatus;
    fn is_waiting_response(&self) -> bool;
    fn on_response(&mut self, ctx: &ModeContext, response: ModeResponse);
    fn header(&self) -> (&str, &str, &str);
    fn draw(&self, drawer: &mut Drawer);
}

#[derive(Clone)]
pub struct ModeContext {
    pub backend: Arc<dyn Backend>,
    pub event_sender: EventSender,
    pub viewport_size: (u16, u16),
}

pub struct ModeStatus {
    pub pending_input: bool,
}

#[derive(Default, Clone, Debug)]
pub struct Output {
    text: String,
    line_count: usize,
    scroll: usize,
}
impl Output {
    pub fn new(text: String) -> Self {
        let mut output = Output::default();
        output.set(text);
        output
    }

    pub fn set(&mut self, output: String) {
        self.text = output;
        self.line_count = self.text.lines().count();
        self.scroll = 0;
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line_count(&self) -> usize {
        self.line_count
    }

    pub fn lines_from_scroll<'a>(&'a self) -> impl 'a + Iterator<Item = &'a str> {
        self.text.lines().skip(self.scroll)
    }

    pub fn on_key(&mut self, available_height: usize, key: Key) {
        let half_height = available_height / 2;

        self.scroll = match key {
            Key::Down | Key::Char('j') => self.scroll + 1,
            Key::Up | Key::Char('k') => self.scroll.saturating_sub(1),
            Key::Ctrl('h') | Key::Home => 0,
            Key::Ctrl('e') | Key::End => usize::MAX,
            Key::Ctrl('d') | Key::PageDown => self.scroll + half_height,
            Key::Ctrl('u') | Key::PageUp => self.scroll.saturating_sub(half_height),
            _ => self.scroll,
        };

        self.scroll = self.line_count.saturating_sub(available_height).min(self.scroll);
    }
}

#[derive(Default, Clone, Debug)]
pub struct ReadLine {
    input: String,
}
impl ReadLine {
    pub fn clear(&mut self) {
        self.input.clear();
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn on_key(&mut self, key: Key) {
        match key {
            Key::Home | Key::Ctrl('u') => self.input.clear(),
            Key::Ctrl('w') => {
                fn is_word(c: char) -> bool {
                    c.is_alphanumeric() || c == '_'
                }

                fn rfind_boundary(mut chars: std::str::Chars, f: fn(&char) -> bool) -> usize {
                    match chars.rfind(f) {
                        Some(c) => chars.as_str().len() + c.len_utf8(),
                        None => 0,
                    }
                }

                let mut chars = self.input.chars();
                if let Some(c) = chars.next_back() {
                    let len = if is_word(c) {
                        rfind_boundary(chars, |&c| !is_word(c))
                    } else if c.is_ascii_whitespace() {
                        rfind_boundary(chars, |&c| is_word(c) || !c.is_ascii_whitespace())
                    } else {
                        rfind_boundary(chars, |&c| is_word(c) || c.is_ascii_whitespace())
                    };
                    self.input.truncate(len);
                }
            }
            Key::Backspace => {
                if let Some((last_char_index, _)) = self.input.char_indices().next_back() {
                    self.input.truncate(last_char_index);
                }
            }
            Key::Char(c) => self.input.push(c),
            _ => (),
        }
    }
}

pub enum SelectMenuAction {
    None,
    Toggle(usize),
    ToggleAll,
}

#[derive(Default, Clone, Debug)]
pub struct SelectMenu {
    pub cursor: usize,
    pub scroll: usize, // index of the first line when scrolling
}
impl SelectMenu {
    pub fn saturate_cursor(&mut self, entries_len: usize) {
        self.cursor = entries_len.saturating_sub(1).min(self.cursor);
    }

    pub fn on_remove_entry(&mut self, index: usize) {
        if index <= self.cursor {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    pub fn on_key(&mut self, entries_len: usize, available_height: usize, key: Key) -> SelectMenuAction {
        let half_height = available_height / 2;

        self.cursor = match key {
            Key::Down | Key::Ctrl('n') | Key::Char('j') => self.cursor + 1,
            Key::Up | Key::Ctrl('p') | Key::Char('k') => self.cursor.saturating_sub(1),
            Key::Ctrl('h') | Key::Home => 0,
            Key::Ctrl('e') | Key::End => usize::MAX,
            Key::Ctrl('d') | Key::PageDown => self.cursor + half_height,
            Key::Ctrl('u') | Key::PageUp => self.cursor.saturating_sub(half_height),
            _ => self.cursor,
        };

        self.saturate_cursor(entries_len);

        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + available_height {
            self.scroll = self.cursor + 1 - available_height;
        }

        match key {
            Key::Char(' ') if self.cursor < entries_len => SelectMenuAction::Toggle(self.cursor),
            Key::Char('a') => SelectMenuAction::ToggleAll,
            _ => SelectMenuAction::None,
        }
    }
}

pub trait FilterEntry {
    fn fuzzy_matches(&self, pattern: &str) -> bool;
}

#[derive(Default, Clone, Debug)]
pub struct Filter {
    has_focus: bool,
    readline: ReadLine,
    visible_indices: Vec<usize>,
}
impl Filter {
    pub fn clear(&mut self) {
        self.has_focus = false;
        self.readline.clear();
        self.visible_indices.clear();
    }

    pub fn enter(&mut self) {
        self.has_focus = true;
        self.readline.clear();
    }

    pub fn on_key(&mut self, key: Key) {
        if key.is_submit() || key == Key::Ctrl('f') {
            self.has_focus = false;
        } else if key.is_cancel() {
            self.has_focus = false;
            self.readline.clear();
        } else {
            self.readline.on_key(key);
        }
    }

    pub fn filter<'entries, I, E>(&mut self, entries: I)
    where
        I: 'entries + Iterator<Item = &'entries E>,
        E: 'entries + FilterEntry,
    {
        self.visible_indices.clear();
        for (i, entry) in entries.enumerate() {
            if entry.fuzzy_matches(self.as_str()) {
                self.visible_indices.push(i);
            }
        }
    }

    pub fn on_remove_entry(&mut self, entry_index: usize) {
        for i in (0..self.visible_indices.len()).rev() {
            if entry_index < i {
                self.visible_indices[i] -= 1;
            } else if entry_index == i {
                self.visible_indices.remove(i);
            } else {
                break;
            }
        }
    }

    pub fn get_visible_index(&self, index: usize) -> Option<usize> {
        self.visible_indices.get(index).cloned()
    }

    pub fn visible_indices(&self) -> &[usize] {
        &self.visible_indices
    }

    pub fn is_filtering(&self) -> bool {
        self.has_focus || !self.readline.input().is_empty()
    }

    pub fn has_focus(&self) -> bool {
        self.has_focus
    }

    pub fn as_str(&self) -> &str {
        self.readline.input()
    }
}

pub fn fuzzy_matches(text: &str, pattern: &str) -> bool {
    let mut pattern_chars = pattern.chars();
    let mut pattern_char = match pattern_chars.next() {
        Some(c) => c,
        None => return true,
    };

    let mut previous_matched_index = 0;
    let mut was_alphanumeric = false;

    for (i, text_char) in text.char_indices() {
        if text_char.eq_ignore_ascii_case(&pattern_char) {
            let is_alphanumeric = text_char.is_ascii_alphanumeric();
            let matched = !is_alphanumeric || !was_alphanumeric || previous_matched_index + 1 == i;
            was_alphanumeric = is_alphanumeric;

            if matched {
                previous_matched_index = i;
                pattern_char = match pattern_chars.next() {
                    Some(c) => c,
                    None => return true,
                };
            }
        }
    }

    false
}
