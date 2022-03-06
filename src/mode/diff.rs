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
                    let info = format_files_diff(&info);
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
        drawer.diff_format(&self.output);
    }
}

pub struct LineDiff {
    line_number: u32,
    text: String,
}
impl LineDiff {
    fn new(line_number: u32) -> Self {
        Self { line_number, text: String::new() }
    }
}

pub const DIFF_FORMAT_FILE_HEADER_LINE: &str = "@@@L";
pub const DIFF_FORMAT_FILE_HEADER_CONTENT: &str = "@@@H";
pub const DIFF_FORMAT_LINE_HEADER: &str = "@@@N";

#[derive(Debug, Clone)]
pub enum FileMode {
    Modified,
    Deleted,
}

pub struct FileDiff {
    filename: String,
    mode: FileMode,
    lines: Vec<LineDiff>,
}
impl FileDiff {
    fn new<S: Into<String>>(filename: S, mode: FileMode) -> Self {
        Self { filename: filename.into(), mode, lines: Vec::new() }
    }

    fn new_line(&mut self, line_number: u32) {
        let line_diff = LineDiff::new(line_number);
        self.lines.push(line_diff);
    }
}
pub struct FilesDiff {
    files: Vec<FileDiff>,
}
impl FilesDiff {
    fn new() -> Self {
        Self { files: Vec::new() }
    }

    fn new_file(&mut self, filename: String, mode: FileMode) {
        let file_diff = FileDiff::new(filename, mode);
        self.files.push(file_diff);
    }

    fn file_mode(&mut self, mode: FileMode) {
        let file_diff = self.files.last_mut().unwrap();
        file_diff.mode = mode;
    }

    fn new_line(&mut self, line_number: u32) {
        self.files.last_mut().unwrap().new_line(line_number);
    }

    fn add_text(&mut self, text: &str) {
        let file_diff = self.files.last_mut().unwrap();
        let line_diff = file_diff.lines.last_mut().unwrap();
        line_diff.text.push_str(text);
    }

    fn output(&self) -> String {
        let mut text = String::new();
        for file_diff in self.files.iter() {
            text.push_str(&format!("{}\n", DIFF_FORMAT_FILE_HEADER_LINE));
            text.push_str(&format!("{}{:?}: {}\n", DIFF_FORMAT_FILE_HEADER_CONTENT, file_diff.mode, file_diff.filename));
            text.push_str(&format!("{}\n", DIFF_FORMAT_FILE_HEADER_LINE));

            for line_diff in file_diff.lines.iter() {
                text.push_str(&format!(
                    "{}@--- {}:Line {} ---@\n",
                    DIFF_FORMAT_LINE_HEADER, file_diff.filename, line_diff.line_number
                ));
                text.push_str(&line_diff.text);
            }
        }

        text
    }
}

#[derive(Clone)]
enum ParseState {
    Start,
    FileHeader(String, FileMode), // filename
    FileMode(FileMode),
    FileContent,
    FileEnd,
    LineHeader(u32), // line number
    LineContent,
}

enum ParseEvent {
    FileDiffStart(String, FileMode), // filename
    FileDiffMode(FileMode),
    FileDiffContent,
    FileDiffEnd,
    LineDiffStart(u32), // line number
    LineDiffContent,
}
impl ParseEvent {
    fn new(state: &ParseState, line: &str) -> Self {
        if line.starts_with("diff --git") {
            // diff --git a/xxx/xxx.c b/xxx/xxx.c
            let pos = line.find(" b/").unwrap();
            let filename = line.get(pos + 3..).unwrap();
            return Self::FileDiffStart(filename.to_string(), FileMode::Modified);
        } else if line.starts_with("---") {
            // --- a/xxx/xxx.c
            return Self::FileDiffContent;
        } else if line.starts_with("+++") {
            // +++ b/xxx/xxx.c
            return Self::FileDiffEnd;
        } else if line.starts_with("@@ ") {
            // @@ -xx,xx +xx,xx @@
            let pos = line.find(" +").unwrap();
            let line_number = line.get(pos + 2..).unwrap();
            let line_number = match line_number.find(",") {
                Some(pos) => {
                    let line_number = line_number.get(..pos).unwrap();
                    line_number.parse::<u32>().unwrap()
                }
                None => 0,
            };
            return Self::LineDiffStart(line_number);
        }

        match state {
            ParseState::FileHeader(..) => {
                if line.starts_with("deleted") {
                    Self::FileDiffMode(FileMode::Deleted)
                } else {
                    Self::FileDiffContent
                }
            }
            ParseState::FileContent => Self::FileDiffContent,
            _ => Self::LineDiffContent,
        }
    }
}

impl ParseState {
    fn line(&mut self, line: &str) -> Self {
        let parse_event = ParseEvent::new(self, line);
        match self {
            ParseState::Start => {
                if let ParseEvent::FileDiffStart(filename, mode) = parse_event {
                    Self::FileHeader(filename, mode)
                } else {
                    panic!("Invalid!\n");
                }
            }
            ParseState::FileHeader(..) => {
                if let ParseEvent::FileDiffMode(mode) = parse_event {
                    Self::FileMode(mode)
                } else {
                    Self::FileContent
                }
            }
            ParseState::FileMode(_) => Self::FileContent,
            ParseState::FileContent => {
                if let ParseEvent::FileDiffEnd = parse_event {
                    Self::FileEnd
                } else {
                    Self::FileContent
                }
            }
            ParseState::FileEnd => {
                if let ParseEvent::LineDiffStart(line_number) = parse_event {
                    Self::LineHeader(line_number)
                } else {
                    panic!("Invalid!\n");
                }
            }
            ParseState::LineHeader(_) => {
                if let ParseEvent::LineDiffContent = parse_event {
                    Self::LineContent
                } else {
                    panic!("Invalid!\n");
                }
            }
            ParseState::LineContent => {
                if let ParseEvent::LineDiffContent = parse_event {
                    Self::LineContent
                } else if let ParseEvent::FileDiffStart(filename, mode) = parse_event {
                    Self::FileHeader(filename, mode)
                } else if let ParseEvent::LineDiffStart(line_number) = parse_event {
                    Self::LineHeader(line_number)
                } else {
                    panic!("Invalid!\n");
                }
            }
        }
    }

    fn output(&mut self, line: &str, files_diff: &mut FilesDiff) {
        match self {
            ParseState::FileHeader(filename, mode) => files_diff.new_file(filename.clone(), mode.clone()),
            ParseState::FileMode(mode) => files_diff.file_mode(mode.clone()),
            ParseState::LineHeader(line_number) => {
                files_diff.new_line(*line_number);
                // the line content after "@@ -xx,xx +xx,xx @@"
                if let Some(pos) = line.find(" @@ ") {
                    let text = format!("{}\n", line.get(pos + 4..).unwrap());
                    files_diff.add_text(&text);
                }
            }
            ParseState::LineContent => {
                let text = format!("{}\n", line);
                files_diff.add_text(&text);
            }
            _ => (),
        }
    }
}

fn format_files_diff(text: &str) -> String {
    let mut files_diff = FilesDiff::new();
    let mut parse_state = ParseState::Start;
    for line in text.lines() {
        parse_state = parse_state.line(line);
        parse_state.output(line, &mut files_diff);
    }

    files_diff.output()
}
