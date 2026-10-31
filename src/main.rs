use std::{
    collections::HashMap,
    env,
    ffi::CString,
    io::{self, Read, Write},
    path::PathBuf,
    process::{ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use doascii::Frame;

const KEY_RIGHTARROW: u8 = 0xae;
const KEY_LEFTARROW: u8 = 0xac;
const KEY_UPARROW: u8 = 0xad;
const KEY_DOWNARROW: u8 = 0xaf;
const KEY_STRAFE_L: u8 = 0xa0;
const KEY_STRAFE_R: u8 = 0xa1;
const KEY_USE: u8 = 0xa2;
const KEY_FIRE: u8 = 0xa3;

unsafe extern "C" {
    fn doomgeneric_Create(argc: i32, argv: *mut *mut std::ffi::c_char);
    fn doomgeneric_Tick();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--doom-worker") {
        return run_doom_worker();
    }
    if args.first().is_some_and(|arg| arg == "--check-engine") {
        return check_engine_frame();
    }
    run_terminal_game()
}

fn check_engine_frame() -> Result<(), Box<dyn std::error::Error>> {
    let wad = find_iwad()?;
    if !wad.is_file() {
        return Err(format!("IWAD not found: {}", wad.display()).into());
    }
    let mut child = spawn_engine(&wad)?;
    let mut engine_stdout = child
        .stdout
        .take()
        .ok_or("Doom worker stdout was not piped")?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ =
            sender.send(Frame::from_reader(&mut engine_stdout).map_err(|error| error.to_string()));
    });
    let frame = match receiver.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(frame)) => frame,
        Ok(Err(error)) => {
            stop_child(&mut child);
            return Err(format!("Doom engine failed before its first frame: {error}").into());
        }
        Err(error) => {
            stop_child(&mut child);
            return Err(format!("Doom engine did not render a frame in time: {error}").into());
        }
    };
    stop_child(&mut child);
    println!(
        "Doom engine produced a {}x{} frame from {}",
        frame.width,
        frame.height,
        wad.display()
    );
    for line in frame.to_ascii(80, 25) {
        println!("{line}");
    }
    Ok(())
}

fn run_doom_worker() -> Result<(), Box<dyn std::error::Error>> {
    let mut doom_args = vec![CString::new("doascii-engine")?];
    let mut args = env::args_os().skip(2);
    while let Some(argument) = args.next() {
        if argument == "--iwad" {
            let path = args.next().ok_or("--iwad needs a path")?;
            doom_args.push(CString::new("-iwad")?);
            doom_args.push(CString::new(path.to_string_lossy().as_bytes())?);
        } else {
            doom_args.push(CString::new(argument.to_string_lossy().as_bytes())?);
        }
    }
    for argument in ["-warp", "1", "1"] {
        doom_args.push(CString::new(argument)?);
    }
    let mut pointers: Vec<*mut std::ffi::c_char> = doom_args
        .iter()
        .map(|argument| argument.as_ptr() as *mut std::ffi::c_char)
        .collect();
    unsafe {
        doomgeneric_Create(pointers.len() as i32, pointers.as_mut_ptr());
        loop {
            doomgeneric_Tick();
        }
    }
}

fn spawn_engine(wad: &std::path::Path) -> io::Result<std::process::Child> {
    Command::new(env::current_exe()?)
        .arg("--doom-worker")
        .arg("--iwad")
        .arg(wad)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
}

fn stop_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn run_terminal_game() -> Result<(), Box<dyn std::error::Error>> {
    let wad = find_iwad()?;
    if !wad.is_file() {
        return Err(format!("IWAD not found: {}", wad.display()).into());
    }
    let mut child = spawn_engine(&wad)?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or("Doom worker stdout was not piped")?;
    let mut child_stdin = child
        .stdin
        .take()
        .ok_or("Doom worker stdin was not piped")?;
    let (frame_tx, frame_rx) = mpsc::channel::<Result<Frame, String>>();
    thread::spawn(move || read_frames(child_stdout, frame_tx));
    let _terminal = TerminalSession::enter()?;
    let mut stdout = io::stdout();
    let mut last_ascii = Vec::new();
    let mut held: HashMap<KeyCode, (u8, Instant)> = HashMap::new();
    let mut terminal_size = terminal::size()?;

    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            if event::poll(Duration::from_millis(4))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Release && key.code == KeyCode::Char('q') {
                        break;
                    }
                    if let Some(doom_key) = to_doom_key(key.code) {
                        match key.kind {
                            KeyEventKind::Press => {
                                if !held.contains_key(&key.code) {
                                    send_key(&mut child_stdin, true, doom_key)?;
                                }
                                held.insert(key.code, (doom_key, Instant::now()));
                            }
                            KeyEventKind::Repeat => {
                                if !held.contains_key(&key.code) {
                                    send_key(&mut child_stdin, true, doom_key)?;
                                }
                                held.insert(key.code, (doom_key, Instant::now()));
                            }
                            KeyEventKind::Release => {
                                if held.remove(&key.code).is_some() {
                                    send_key(&mut child_stdin, false, doom_key)?;
                                }
                            }
                        }
                    }
                }
            }

            let now = Instant::now();
            let released: Vec<KeyCode> = held
                .iter()
                .filter_map(|(code, (_, last_event))| {
                    (now.duration_since(*last_event) > Duration::from_millis(750)).then_some(*code)
                })
                .collect();
            for code in released {
                if let Some((doom_key, _)) = held.remove(&code) {
                    send_key(&mut child_stdin, false, doom_key)?;
                }
            }

            let mut newest_frame = None;
            while let Ok(message) = frame_rx.try_recv() {
                newest_frame = Some(message?);
            }
            if let Some(frame) = newest_frame {
                let size = terminal::size()?;
                if size != terminal_size {
                    terminal_size = size;
                    last_ascii.clear();
                    queue!(stdout, Clear(ClearType::All))?;
                }
                let ascii = frame.to_ascii_fit(
                    size.0.max(1) as usize,
                    size.1.saturating_sub(1).max(1) as usize,
                    _terminal.character_aspect(),
                );
                draw_changed(&mut stdout, &ascii, &mut last_ascii)?;
                queue!(
                    stdout,
                    cursor::MoveTo(0, size.1.saturating_sub(1)),
                    Print("WASD move  arrows look  Space fire  E use  Q quit")
                )?;
                stdout.flush()?;
            }
            if child.try_wait()?.is_some() {
                return Err(
                    "Doom engine exited. Check the IWAD and Doom worker diagnostics.".into(),
                );
            }
        }
        Ok(())
    })();
    stop_child(&mut child);
    result
}

fn find_iwad() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--iwad" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--iwad needs a path".into());
        }
    }
    if let Some(path) = env::var_os("DOASCII_IWAD") {
        return Ok(PathBuf::from(path));
    }
    let freedoom = env::current_dir()?.join("assets/freedoom/freedoom-0.13.0/freedoom1.wad");
    if freedoom.is_file() {
        return Ok(freedoom);
    }
    Err("Set DOASCII_IWAD or run with --iwad <path>; Freedoom IWAD not found in assets".into())
}

fn read_frames(mut reader: impl Read, sender: mpsc::Sender<Result<Frame, String>>) {
    loop {
        match Frame::from_reader(&mut reader) {
            Ok(frame) => {
                if sender.send(Ok(frame)).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(format!("Doom framebuffer stream ended: {error}")));
                return;
            }
        }
    }
}

fn send_key(writer: &mut ChildStdin, pressed: bool, key: u8) -> io::Result<()> {
    writer.write_all(&[u8::from(pressed), key])?;
    writer.flush()
}

fn to_doom_key(code: KeyCode) -> Option<u8> {
    match code {
        KeyCode::Char('w') | KeyCode::Up => Some(KEY_UPARROW),
        KeyCode::Char('s') | KeyCode::Down => Some(KEY_DOWNARROW),
        KeyCode::Char('a') => Some(KEY_STRAFE_L),
        KeyCode::Char('d') => Some(KEY_STRAFE_R),
        KeyCode::Left => Some(KEY_LEFTARROW),
        KeyCode::Right => Some(KEY_RIGHTARROW),
        KeyCode::Char(' ') | KeyCode::Char('f') => Some(KEY_FIRE),
        KeyCode::Char('e') => Some(KEY_USE),
        KeyCode::Char(character) if character.is_ascii() => {
            Some(character.to_ascii_lowercase() as u8)
        }
        KeyCode::Enter => Some(13),
        KeyCode::Tab => Some(9),
        KeyCode::Backspace => Some(127),
        KeyCode::Esc => Some(27),
        _ => None,
    }
}

fn draw_changed(
    stdout: &mut io::Stdout,
    lines: &[String],
    previous: &mut Vec<String>,
) -> io::Result<()> {
    if previous.len() != lines.len()
        || previous.first().map(String::len) != lines.first().map(String::len)
    {
        queue!(stdout, Clear(ClearType::All))?;
        previous.clear();
        previous.resize(lines.len(), String::new());
    }
    for (row, (line, old_line)) in lines.iter().zip(previous.iter_mut()).enumerate() {
        let current: Vec<char> = line.chars().collect();
        let old: Vec<char> = old_line.chars().collect();
        for (column, character) in current.iter().enumerate() {
            if old.get(column) != Some(character) {
                queue!(
                    stdout,
                    cursor::MoveTo(column as u16, row as u16),
                    Print(character)
                )?;
            }
        }
        *old_line = line.clone();
    }
    Ok(())
}

struct TerminalSession {
    #[cfg(windows)]
    _font: windows_console_font::FontGuard,
    character_aspect: f32,
}

impl TerminalSession {
    fn character_aspect(&self) -> f32 {
        self.character_aspect
    }

    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(
            io::stdout(),
            terminal::EnterAlternateScreen,
            cursor::Hide,
            Clear(ClearType::All)
        ) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        #[cfg(windows)]
        let font = windows_console_font::FontGuard::shrink();
        #[cfg(windows)]
        let target_size = font.target_grid();
        #[cfg(windows)]
        let character_aspect = font.character_aspect();
        #[cfg(not(windows))]
        let target_size = (160, 45);
        #[cfg(not(windows))]
        let character_aspect = 0.5;
        let _ = execute!(
            io::stdout(),
            terminal::SetSize(target_size.0, target_size.1)
        );
        Ok(Self {
            #[cfg(windows)]
            _font: font,
            character_aspect,
        })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(windows)]
mod windows_console_font {
    use std::{ffi::c_void, mem::size_of};
    const STD_OUTPUT_HANDLE: i32 = -11;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Coord {
        x: i16,
        y: i16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ConsoleFontInfoEx {
        cb_size: u32,
        n_font: u32,
        font_size: Coord,
        font_family: u32,
        font_weight: u32,
        face_name: [u16; 32],
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(handle: i32) -> *mut c_void;
        fn GetCurrentConsoleFontEx(
            output: *mut c_void,
            maximum_window: i32,
            info: *mut ConsoleFontInfoEx,
        ) -> i32;
        fn SetCurrentConsoleFontEx(
            output: *mut c_void,
            maximum_window: i32,
            info: *const ConsoleFontInfoEx,
        ) -> i32;
    }

    pub struct FontGuard {
        output: *mut c_void,
        original: Option<ConsoleFontInfoEx>,
        cell_width: u16,
        cell_height: u16,
    }

    impl FontGuard {
        pub fn shrink() -> Self {
            let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
            let mut current = ConsoleFontInfoEx {
                cb_size: size_of::<ConsoleFontInfoEx>() as u32,
                n_font: 0,
                font_size: Coord { x: 0, y: 0 },
                font_family: 0,
                font_weight: 0,
                face_name: [0; 32],
            };
            let valid = !output.is_null() && output as isize != -1;
            if valid && unsafe { GetCurrentConsoleFontEx(output, 0, &mut current) != 0 } {
                let original = current;
                let old_width = current.font_size.x.max(1) as u16;
                let old_height = current.font_size.y.max(1) as u16;
                current.font_size = Coord { x: 4, y: 6 };
                let changed = unsafe { SetCurrentConsoleFontEx(output, 0, &current) != 0 };
                Self {
                    output,
                    original: changed.then_some(original),
                    cell_width: if changed { 4 } else { old_width },
                    cell_height: if changed { 6 } else { old_height },
                }
            } else {
                Self {
                    output,
                    original: None,
                    cell_width: 8,
                    cell_height: 16,
                }
            }
        }
    }

    impl FontGuard {
        pub fn target_grid(&self) -> (u16, u16) {
            (
                (1280 / self.cell_width as u32).min(u16::MAX as u32) as u16,
                (720 / self.cell_height as u32).min(u16::MAX as u32) as u16,
            )
        }

        pub fn character_aspect(&self) -> f32 {
            self.cell_width as f32 / self.cell_height as f32
        }
    }

    impl Drop for FontGuard {
        fn drop(&mut self) {
            if let Some(original) = self.original {
                unsafe {
                    SetCurrentConsoleFontEx(self.output, 0, &original);
                }
            }
        }
    }
}
