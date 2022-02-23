use std::{
    io,
    io::Write,
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};

use crate::{
    backend::Backend,
    mode::*,
    platform::{Key, Platform, PlatformEventReader},
    tool::*,
    ui::Drawer,
};

enum Event {
    Key(Key),
    Resize(u16, u16),
    Response(ModeResponse),
    ModeChange(ModeKind, ModeChangeInfo),
    ModeRevert,
}

#[derive(Clone)]
pub struct EventSender(mpsc::Sender<Event>);
impl EventSender {
    pub fn send_response(&self, result: ModeResponse) {
        self.0.send(Event::Response(result)).unwrap();
    }

    pub fn send_mode_change(&self, mode: ModeKind, info: ModeChangeInfo) {
        self.0.send(Event::ModeChange(mode, info)).unwrap();
    }

    pub fn send_mode_revert(&self) {
        self.0.send(Event::ModeRevert).unwrap();
    }
}

#[derive(Default)]
struct Application {
    mode: ModeBuf,
    spinner_state: u8,
}
impl Application {
    pub fn current_mode(&mut self) -> &mut dyn ModeTrait {
        self.mode.mode()
    }

    pub fn on_key(&mut self, ctx: &ModeContext, key: Key) -> bool {
        let status = self.current_mode().on_key(ctx, key);

        if !status.pending_input {
            if key.is_exit() {
                return false;
            }

            if key.is_back() {
                self.mode.revert_mode(ctx);
                return true;
            }

            let target_mode_kind = match key {
                Key::Char('s') => Some(ModeKind::Status),
                Key::Char('l') => Some(ModeKind::Log),
                Key::Char('b') => Some(ModeKind::Branches),
                Key::Char('t') => Some(ModeKind::Tags),
                Key::Char('S') => Some(ModeKind::Stash),
                _ => None,
            };

            if let Some(target_mode_kind) = target_mode_kind {
                self.mode.enter_mode(ctx, target_mode_kind, ModeChangeInfo::new(self.mode.mode_kind()));
            }
        }

        true
    }

    pub fn on_response(&mut self, ctx: &ModeContext, response: ModeResponse) {
        if response.mode_kind() == self.mode.mode_kind() {
            //log(format!("kind same, {:?}\n", self.mode.mode_kind()));
            self.current_mode().on_response(ctx, response);
        } else {
            log(format!("kind different, {:?}\n", self.mode.mode_kind()));
        }
    }

    pub fn is_waiting_response(&mut self) -> bool {
        self.current_mode().is_waiting_response()
    }

    pub fn draw_header(&mut self, drawer: &mut Drawer) {
        let spinner = [b'-', b'\\', b'|', b'/'];
        self.spinner_state = (self.spinner_state + 1) % spinner.len() as u8;
        let spinner = match self.is_waiting_response() {
            true => spinner[self.spinner_state as usize],
            false => b' ',
        };

        let (mode_name, left_help, right_help) = self.current_mode().header();
        drawer.header(mode_name, left_help, right_help, spinner);
    }

    pub fn draw_body(&mut self, drawer: &mut Drawer) {
        //log(format!("draw body, mode:\n, {:?}\n", self.mode));

        self.current_mode().draw(drawer);
        drawer.clear_to_bottom();
    }
}

fn terminal_event_loop(mut event_reader: PlatformEventReader, sender: mpsc::Sender<Event>) {
    event_reader.init();

    let mut keys = Vec::new();
    loop {
        keys.clear();
        let mut resize = None;

        event_reader.read_terminal_events(&mut keys, &mut resize);

        for &key in &keys {
            if sender.send(Event::Key(key)).is_err() {
                break;
            }
        }
        if let Some(resize) = resize {
            if sender.send(Event::Resize(resize.0, resize.1)).is_err() {
                break;
            }
        }
    }
}

pub fn run(platform_event_reader: PlatformEventReader, backend: Arc<dyn Backend>) {
    let (event_sender, event_receiver) = mpsc::channel();

    let mut ctx =
        ModeContext { backend, event_sender: EventSender(event_sender.clone()), viewport_size: Platform::terminal_size() };

    let _ = thread::spawn(move || {
        terminal_event_loop(platform_event_reader, event_sender);
    });

    let mut application = Application::default();
    application.mode.enter_mode(&ctx, ModeKind::default(), ModeChangeInfo::new(ModeKind::default()));

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut stdout_buf = Vec::new();

    const TIMEOUT: Duration = Duration::from_millis(100);

    loop {
        let event = if application.is_waiting_response() {
            event_receiver.recv_timeout(TIMEOUT)
        } else {
            event_receiver.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };

        let mut draw_body = true;

        match event {
            Ok(Event::Key(key)) => {
                if !application.on_key(&ctx, key) {
                    break; // quit the appication
                }
            }
            Ok(Event::Resize(width, height)) => {
                ctx.viewport_size = (width, height);
            }
            Ok(Event::Response(response)) => application.on_response(&ctx, response),
            Ok(Event::ModeChange(mode, info)) => application.mode.enter_mode(&ctx, mode, info),
            Ok(Event::ModeRevert) => application.mode.revert_mode(&ctx),
            Err(mpsc::RecvTimeoutError::Timeout) => draw_body = false,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let mut drawer = Drawer::new(stdout_buf, ctx.viewport_size);
        application.draw_header(&mut drawer);
        if draw_body {
            application.draw_body(&mut drawer);
        }
        stdout_buf = drawer.take_buf();

        stdout.write_all(&stdout_buf).unwrap();
        stdout.flush().unwrap();
    }
}
