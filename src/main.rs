use std::{
    collections::HashMap,
    env,
    ffi::CString,
    fs::{self, File},
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use doascii::{AsciiCell, FRAME_WIDTH, Frame, GameObservation};
use serde_json::{Value, json};

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
    let vey_enabled = args.iter().any(|arg| arg == "--vey");
    let vey_python = env::var_os("DOASCII_VEY_PYTHON").unwrap_or_else(|| "python".into());
    run_terminal_game(vey_enabled, &vey_python)
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
        "Doom engine produced a {}x{} frame from {} ({} visible enemies, {} level kills, health {}, armor {}, ammo {}, clearances F/L/R/B={}/{}/{}/{})",
        frame.width,
        frame.height,
        wad.display(),
        frame.observation.enemies.len(),
        frame.observation.total_kills,
        frame.observation.health,
        frame.observation.armor,
        frame.observation.ammo,
        frame.observation.forward_clearance,
        frame.observation.left_clearance,
        frame.observation.right_clearance,
        frame.observation.back_clearance
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

fn run_terminal_game(
    vey_enabled: bool,
    vey_python: &std::ffi::OsStr,
) -> Result<(), Box<dyn std::error::Error>> {
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
    let mut vey = if vey_enabled {
        match VeySession::spawn(vey_python) {
            Ok(session) => Some(session),
            Err(error) => {
                eprintln!("Could not start Vey adapter: {error}");
                None
            }
        }
    } else {
        None
    };
    let mut vey_log = if vey_enabled {
        match VeyDecisionLog::open() {
            Ok(log) => Some(log),
            Err(error) => {
                eprintln!("Could not open Vey decision log: {error}");
                None
            }
        }
    } else {
        None
    };
    let mut vey_last_request: Option<Value> = None;
    let mut vey_status = match (vey_enabled, vey.is_some()) {
        (true, true) => "VEY WATCHING",
        (true, false) => "VEY ADAPTER UNAVAILABLE",
        (false, _) => "",
    }
    .to_owned();
    let mut vey_sequence = 0_u64;
    let mut vey_pending = None;
    let mut vey_retry_after = Instant::now();
    let mut vey_next_request = Instant::now();
    let mut latest_observation = GameObservation::default();
    let mut last_player_pose: Option<(i32, i32, u32)> = None;
    let mut last_player_progress = Instant::now();
    let mut vey_stuck = false;
    let mut ai_pressed: Option<(u8, Instant)> = None;
    let _terminal = TerminalSession::enter()?;
    let mut stdout = io::stdout();
    let mut last_ascii: Vec<Vec<AsciiCell>> = Vec::new();
    let mut held: HashMap<KeyCode, (u8, Instant)> = HashMap::new();
    let mut held_counts: HashMap<u8, usize> = HashMap::new();
    let mut terminal_size = terminal::size()?;

    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        'game: loop {
            while event::poll(Duration::from_millis(4))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Release && key.code == KeyCode::Char('q') {
                        break 'game;
                    }
                    let now = Instant::now();
                    for (_, last_event) in held.values_mut() {
                        *last_event = now;
                    }
                    if let Some(doom_key) = to_doom_key(key.code) {
                        let was_held = held.contains_key(&key.code);
                        match key.kind {
                            KeyEventKind::Press | KeyEventKind::Repeat => {
                                held.insert(key.code, (doom_key, now));
                                if !was_held {
                                    let count = held_counts.entry(doom_key).or_default();
                                    if *count == 0 {
                                        send_key(&mut child_stdin, true, doom_key)?;
                                    }
                                    *count += 1;
                                }
                            }
                            KeyEventKind::Release => {
                                if let Some((released_key, _)) = held.remove(&key.code) {
                                    release_doom_key(
                                        &mut child_stdin,
                                        &mut held_counts,
                                        released_key,
                                    )?;
                                }
                            }
                        }
                    }
                }
            }

            let now = Instant::now();
            if ai_pressed.is_some_and(|(_, deadline)| now >= deadline) {
                if let Some((doom_key, _)) = ai_pressed.take() {
                    release_doom_key(&mut child_stdin, &mut held_counts, doom_key)?;
                }
            }
            let released: Vec<KeyCode> = held
                .iter()
                .filter_map(|(code, (_, last_event))| {
                    (now.duration_since(*last_event) > Duration::from_millis(750)).then_some(*code)
                })
                .collect();
            for code in released {
                if let Some((doom_key, _)) = held.remove(&code) {
                    release_doom_key(&mut child_stdin, &mut held_counts, doom_key)?;
                }
            }

            if let Some(session) = vey.as_mut() {
                while let Ok(response) = session.responses.try_recv() {
                    match response {
                        Ok(response) => {
                            if let Some(log) = vey_log.as_mut() {
                                let request = vey_last_request.take();
                                let _ = log.record(request.as_ref(), &response);
                            }
                            let response_sequence =
                                response.get("sequence").and_then(Value::as_u64);
                            if response_sequence != vey_pending {
                                vey_pending = None;
                                vey_status = "VEY STALE RESPONSE IGNORED".into();
                                vey_retry_after = Instant::now() + Duration::from_secs(3);
                                continue;
                            }
                            vey_pending = None;
                            if response.get("schema").and_then(Value::as_u64) != Some(1) {
                                vey_status = "VEY BAD RESPONSE SCHEMA".into();
                                vey_retry_after = Instant::now() + Duration::from_secs(3);
                                continue;
                            }
                            if let Some(error) = response.get("error").and_then(Value::as_str) {
                                vey_status = format!(
                                    "VEY ERROR: {}",
                                    error.chars().take(40).collect::<String>()
                                );
                                vey_retry_after = Instant::now() + Duration::from_secs(3);
                            } else if response.get("decision_mode").and_then(Value::as_str)
                                != Some("structured")
                            {
                                vey_status = "VEY UNSUPPORTED LANE IGNORED".into();
                                vey_retry_after = Instant::now() + Duration::from_secs(3);
                            } else if response
                                .get("trust")
                                .and_then(|trust| trust.get("state"))
                                .and_then(Value::as_str)
                                != Some("confident")
                            {
                                vey_status = "VEY ABSTAINED".into();
                                vey_retry_after = Instant::now() + Duration::from_secs(3);
                            } else if let Some(action) =
                                response.get("action").and_then(Value::as_str)
                            {
                                let mapped_key = action_key(action);
                                if action == "wait" {
                                    vey_status = "VEY WAIT".into();
                                } else if let Some(key) = mapped_key.filter(|_| {
                                    current_vey_candidates(
                                        &latest_observation,
                                        FRAME_WIDTH,
                                        vey_stuck,
                                    )
                                    .contains(&action)
                                }) {
                                    if let Some((old_key, _)) = ai_pressed.take() {
                                        release_doom_key(
                                            &mut child_stdin,
                                            &mut held_counts,
                                            old_key,
                                        )?;
                                    }
                                    let count = held_counts.entry(key).or_default();
                                    if *count == 0 {
                                        send_key(&mut child_stdin, true, key)?;
                                    }
                                    *count += 1;
                                    let pulse_ms = match action {
                                        "advance" | "retreat" => 240,
                                        "turn_left" | "turn_right"
                                            if latest_observation.enemies.is_empty() =>
                                        {
                                            240
                                        }
                                        "turn_left" | "turn_right" => 90,
                                        _ => 90,
                                    };
                                    ai_pressed = Some((
                                        key,
                                        Instant::now() + Duration::from_millis(pulse_ms),
                                    ));
                                    vey_status = format!("VEY {}", action.to_ascii_uppercase());
                                } else {
                                    vey_status = "VEY ACTION REJECTED".into();
                                }
                            } else {
                                vey_status = "VEY ABSTAINED".into();
                                vey_retry_after = Instant::now() + Duration::from_secs(3);
                            }
                        }
                        Err(error) => {
                            if let Some(log) = vey_log.as_mut() {
                                let request = vey_last_request.take();
                                let _ = log.record_error(request.as_ref(), &error);
                            }
                            vey_pending = None;
                            vey_status = format!(
                                "VEY ERROR: {}",
                                error.chars().take(40).collect::<String>()
                            );
                            vey_retry_after = Instant::now() + Duration::from_secs(3);
                        }
                    }
                }
            }

            let mut newest_frame = None;
            while let Ok(message) = frame_rx.try_recv() {
                newest_frame = Some(message?);
            }
            if let Some(frame) = newest_frame {
                latest_observation = frame.observation.clone();
                let pose = (
                    latest_observation.world_x,
                    latest_observation.world_y,
                    latest_observation.angle,
                );
                if last_player_pose.is_some_and(|previous| previous != pose) {
                    last_player_progress = Instant::now();
                }
                last_player_pose = Some(pose);
                vey_stuck = Instant::now().duration_since(last_player_progress)
                    >= Duration::from_millis(550);
                if let Some(session) = vey.as_mut() {
                    if vey_pending.is_none()
                        && Instant::now() >= vey_retry_after
                        && Instant::now() >= vey_next_request
                    {
                        if let Some(request) = make_vey_request(
                            vey_sequence + 1,
                            &latest_observation,
                            frame.width,
                            vey_stuck,
                        ) {
                            vey_sequence += 1;
                            match session.requests.try_send(request.clone()) {
                                Ok(()) => {
                                    vey_last_request = Some(request);
                                    vey_pending = Some(vey_sequence);
                                    vey_next_request = Instant::now() + Duration::from_millis(300);
                                    vey_status = "VEY THINKING".into();
                                }
                                Err(mpsc::TrySendError::Disconnected(_)) => {
                                    vey_status = "VEY OFFLINE".into();
                                    vey_retry_after = Instant::now() + Duration::from_secs(3);
                                }
                                Err(mpsc::TrySendError::Full(_)) => {}
                            }
                        } else {
                            vey_status = "VEY WATCHING".into();
                        }
                    }
                }
                let size = terminal::size()?;
                if size != terminal_size {
                    terminal_size = size;
                    last_ascii.clear();
                    queue!(stdout, Clear(ClearType::All))?;
                }
                let ascii = frame.to_ascii_cells_fit(
                    size.0.max(1) as usize,
                    size.1.saturating_sub(1).max(1) as usize,
                    _terminal.character_aspect(),
                );
                draw_changed(&mut stdout, &ascii, &mut last_ascii)?;
                queue!(
                    stdout,
                    ResetColor,
                    cursor::MoveTo(0, size.1.saturating_sub(1)),
                    Print(controls_line(size.0, &vey_status))
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

struct VeyDecisionLog {
    output: BufWriter<File>,
}

impl VeyDecisionLog {
    fn open() -> io::Result<Self> {
        let directory = env::current_dir()?.join(".doascii");
        fs::create_dir_all(&directory)?;
        let path = directory.join("vey-decisions.log");
        let mut output = BufWriter::new(File::create(&path)?);
        writeln!(output, "DOASCII · Vey decision trace")?;
        writeln!(output, "Log: {}", path.display())?;
        output.flush()?;
        #[cfg(windows)]
        {
            let _ = open_vey_log_window(&path);
        }
        Ok(Self { output })
    }

    fn record(&mut self, request: Option<&Value>, response: &Value) -> io::Result<()> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let trust = response.get("trust");
        let action = response
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("ABSTAIN");
        let lane = response
            .get("decision_mode")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let trust_state = trust
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let margin = trust
            .and_then(|value| value.get("primary_margin"))
            .map(Value::to_string)
            .unwrap_or_else(|| "n/a".to_owned());
        let sequence = response
            .get("sequence")
            .map(Value::to_string)
            .unwrap_or_else(|| "?".to_owned());
        writeln!(
            self.output,
            "\n[{}.{:03}] #{sequence}  {action}  lane={lane}  trust={trust_state}  margin={margin}",
            elapsed.as_secs(),
            elapsed.subsec_millis()
        )?;
        if let Some(error) = response.get("error").and_then(Value::as_str) {
            writeln!(self.output, "  error: {error}")?;
        }
        if let Some(request) = request {
            if let Some(question) = request.get("question").and_then(Value::as_str) {
                writeln!(self.output, "  policy: {question}")?;
            }
            if let Some(state) = request.get("state").and_then(Value::as_str) {
                writeln!(self.output, "  state: {state}")?;
            }
            if let Some(candidates) = request.get("candidates").and_then(Value::as_object) {
                for (id, description) in candidates {
                    writeln!(self.output, "  candidate {id}: {description}")?;
                }
            }
        }
        if let Some(certificate) = response.get("certificate") {
            writeln!(self.output, "  evidence: {}", certificate)?;
        }
        self.output.flush()
    }

    fn record_error(&mut self, request: Option<&Value>, error: &str) -> io::Result<()> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        writeln!(
            self.output,
            "\n[{}.{:03}] Vey adapter error: {error}",
            elapsed.as_secs(),
            elapsed.subsec_millis()
        )?;
        if let Some(sequence) = request.and_then(|value| value.get("sequence")) {
            writeln!(self.output, "  request #{sequence}")?;
        }
        self.output.flush()
    }
}

#[cfg(windows)]
fn open_vey_log_window(path: &std::path::Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    let escaped_path = path.to_string_lossy().replace('\'', "''");
    let command = format!(
        "Write-Host 'Live DOASCII Vey decisions (Ctrl+C closes this viewer)'; Get-Content -LiteralPath '{escaped_path}' -Tail 100 -Wait"
    );
    Command::new("powershell.exe")
        .args(["-NoProfile", "-NoExit", "-Command", &command])
        .creation_flags(0x0000_0010)
        .spawn()
        .map(|_| ())
}

struct VeySession {
    requests: mpsc::SyncSender<Value>,
    responses: mpsc::Receiver<Result<Value, String>>,
    child: Arc<Mutex<Child>>,
    worker: Option<JoinHandle<()>>,
}

impl VeySession {
    fn spawn(python: &std::ffi::OsStr) -> io::Result<Self> {
        let script = env::current_dir()?.join("tools/vey_adapter.py");
        let mut child = Command::new(python)
            .arg("-u")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = BufWriter::new(
            child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("Vey adapter stdin was not piped"))?,
        );
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("Vey adapter stdout was not piped"))?,
        );
        let child = Arc::new(Mutex::new(child));
        let (request_tx, request_rx) = mpsc::sync_channel::<Value>(1);
        let (response_tx, response_rx) = mpsc::channel::<Result<Value, String>>();
        let worker = thread::spawn(move || run_vey_adapter(input, output, request_rx, response_tx));
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
            child,
            worker: Some(worker),
        })
    }
}

impl Drop for VeySession {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_vey_adapter(
    mut input: BufWriter<std::process::ChildStdin>,
    mut output: BufReader<ChildStdout>,
    requests: mpsc::Receiver<Value>,
    responses: mpsc::Sender<Result<Value, String>>,
) {
    for request in requests {
        let sequence = request.get("sequence").and_then(Value::as_u64);
        let result = (|| -> Result<Value, String> {
            serde_json::to_writer(&mut input, &request).map_err(|error| error.to_string())?;
            input.write_all(b"\n").map_err(|error| error.to_string())?;
            input.flush().map_err(|error| error.to_string())?;
            let mut line = String::new();
            if output
                .read_line(&mut line)
                .map_err(|error| error.to_string())?
                == 0
            {
                return Err("Vey adapter closed its response stream".into());
            }
            serde_json::from_str(&line)
                .map_err(|error| format!("invalid Vey JSON response: {error}"))
        })();
        if responses.send(result).is_err() {
            return;
        }
        if sequence.is_none() {
            return;
        }
    }
}

fn current_vey_candidates(
    observation: &GameObservation,
    frame_width: usize,
    stuck: bool,
) -> Vec<&'static str> {
    let mut actions = vec!["wait"];
    if observation.player_state != 0 {
        return actions;
    }
    if observation.enemies.is_empty() {
        if observation.forward_clearance >= 3 {
            actions.push("advance");
        } else if observation.left_clearance > observation.right_clearance {
            actions.push("turn_left");
        } else if observation.right_clearance > observation.left_clearance {
            actions.push("turn_right");
        } else if observation.back_clearance >= 2 {
            actions.push("retreat");
        } else if stuck {
            actions.push(if observation.game_tic % 2 == 0 {
                "turn_left"
            } else {
                "turn_right"
            });
        } else {
            actions.push("turn_right");
        }
        return actions;
    }
    let center = frame_width / 2;
    let nearest = observation
        .enemies
        .iter()
        .min_by_key(|enemy| enemy.x.saturating_add(enemy.width / 2).abs_diff(center))
        .unwrap();
    let aligned = observation
        .enemies
        .iter()
        .any(|enemy| enemy.x <= center && center < enemy.x.saturating_add(enemy.width));
    if aligned {
        let centered_enemy = observation
            .enemies
            .iter()
            .filter(|enemy| enemy.x <= center && center < enemy.x.saturating_add(enemy.width))
            .max_by_key(|enemy| enemy.width)
            .unwrap();
        let melee_weapon = matches!(observation.weapon, 0 | 7);
        let melee_in_range = centered_enemy.width >= frame_width / 5;
        if observation.ammo > 0 || (observation.ammo == -1 && melee_weapon && melee_in_range) {
            actions.push("fire");
        }
        if observation.health < 45
            && centered_enemy.width >= frame_width / 4
            && observation.back_clearance >= 2
        {
            actions.push("retreat");
        } else if centered_enemy.width < frame_width / 10 && observation.forward_clearance >= 3 {
            actions.push("advance");
        }
    } else {
        let target = nearest.x.saturating_add(nearest.width / 2);
        actions.push(if target < center {
            "turn_left"
        } else {
            "turn_right"
        });
    }
    actions
}

fn make_vey_request(
    sequence: u64,
    observation: &GameObservation,
    frame_width: usize,
    stuck: bool,
) -> Option<Value> {
    let actions = current_vey_candidates(observation, frame_width, stuck);
    if actions.len() < 2 {
        return None;
    }
    let candidates = actions
        .iter()
        .map(|action| {
            let priority = match *action {
                "wait" => 0,
                "turn_left" if observation.enemies.is_empty() => {
                    50 + 20 * observation.left_clearance as i32
                }
                "turn_right" if observation.enemies.is_empty() => {
                    50 + 20 * observation.right_clearance as i32
                }
                "turn_left" | "turn_right" => 100,
                "advance" if stuck => 0,
                "advance" => 20 + 20 * observation.forward_clearance as i32,
                "fire" if observation.health < 45 => 50,
                "fire"
                    if observation
                        .enemies
                        .iter()
                        .any(|enemy| enemy.width < frame_width / 10) =>
                {
                    60
                }
                "fire" => 90,
                "retreat" => 100,

                _ => 0,
            };
            let ammo_cost = if *action == "fire" { 1 } else { 0 };
            (
                *action,
                format!("tactical_priority {priority}; ammo_cost {ammo_cost}"),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let enemy_state = observation
        .enemies
        .iter()
        .map(|enemy| {
            format!(
                "[{}, {}, {}, {}]",
                enemy.x, enemy.y, enemy.width, enemy.height
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Some(json!({
        "schema": 1,
        "sequence": sequence,
        "question": "Choose the action with the greatest tactical_priority, then the lowest ammo_cost.",
        "state": format!(
            "player_state={}; health={}; armor={}; weapon={}; ammo={}; world_xy=({},{}); angle={}; clearance_f_l_r_b={}/{}/{}/{}; stuck={}; visible_enemy_boxes=[{}]",
            observation.player_state, observation.health, observation.armor,
            observation.weapon, observation.ammo, observation.world_x,
            observation.world_y, observation.angle, observation.forward_clearance,
            observation.left_clearance, observation.right_clearance,
            observation.back_clearance, stuck, enemy_state
        ),
        "candidates": candidates,
    }))
}

fn action_key(action: &str) -> Option<u8> {
    match action {
        "fire" => Some(KEY_FIRE),
        "turn_left" => Some(KEY_LEFTARROW),
        "turn_right" => Some(KEY_RIGHTARROW),
        "advance" => Some(KEY_UPARROW),
        "retreat" => Some(KEY_DOWNARROW),
        _ => None,
    }
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

fn release_doom_key(
    writer: &mut ChildStdin,
    held_counts: &mut HashMap<u8, usize>,
    key: u8,
) -> io::Result<()> {
    if let Some(count) = held_counts.get_mut(&key) {
        *count -= 1;
        if *count == 0 {
            held_counts.remove(&key);
            send_key(writer, false, key)?;
        }
    }
    Ok(())
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

fn controls_line(columns: u16, status: &str) -> String {
    let controls = controls_for_width(columns);
    if status.is_empty() {
        return controls.to_owned();
    }
    format!("{controls} | {status}")
        .chars()
        .take(columns as usize)
        .collect()
}

fn controls_for_width(columns: u16) -> &'static str {
    match columns {
        64.. => "W/S move  A/D strafe  Left/Right turn  Space fire  E use  Q quit",
        57.. => "W/S move  A/D strafe  Left/Right turn  Space fire  Q quit",
        40.. => "W/S move  A/D strafe  Space fire  Q quit",
        17.. => "WASD move  Q quit",
        _ => "Q",
    }
}

fn draw_changed(
    stdout: &mut io::Stdout,
    lines: &[Vec<AsciiCell>],
    previous: &mut Vec<Vec<AsciiCell>>,
) -> io::Result<()> {
    if previous.len() != lines.len()
        || previous.first().map(Vec::len) != lines.first().map(Vec::len)
    {
        queue!(stdout, Clear(ClearType::All), ResetColor)?;
        previous.clear();
        previous.resize_with(lines.len(), Vec::new);
    }
    for (row, line) in lines.iter().enumerate() {
        let old_line = &previous[row];
        let mut last_color = None;
        for (column, cell) in line.iter().enumerate() {
            if old_line.get(column) != Some(cell) {
                if last_color != Some(cell.color) {
                    queue!(
                        stdout,
                        SetForegroundColor(Color::Rgb {
                            r: cell.color.0,
                            g: cell.color.1,
                            b: cell.color.2,
                        })
                    )?;
                    last_color = Some(cell.color);
                }
                queue!(
                    stdout,
                    cursor::MoveTo(column as u16, row as u16),
                    Print(cell.character)
                )?;
            }
        }
        previous[row] = line.clone();
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

#[cfg(test)]
mod vey_tests {
    use super::*;
    use doascii::EnemyRect;

    fn observation(enemy_x: usize, health: i16, ammo: i16) -> GameObservation {
        GameObservation {
            player_state: 0,
            health,
            armor: 0,
            weapon: 1,
            ammo,
            forward_clearance: 3,
            enemies: vec![EnemyRect {
                x: enemy_x,
                y: 120,
                width: 80,
                height: 80,
            }],
            ..GameObservation::default()
        }
    }

    #[test]
    fn vey_only_receives_fire_when_an_ammo_backed_enemy_is_centered() {
        let centered = observation(300, 100, 10);
        assert_eq!(
            current_vey_candidates(&centered, FRAME_WIDTH, false),
            ["wait", "fire"]
        );
        assert_eq!(action_key("fire"), Some(KEY_FIRE));

        let empty = observation(300, 100, 0);
        assert_eq!(current_vey_candidates(&empty, FRAME_WIDTH, false), ["wait"]);
    }

    #[test]
    fn vey_turns_toward_visible_off_center_enemies() {
        let left = observation(100, 100, 10);
        assert_eq!(
            current_vey_candidates(&left, FRAME_WIDTH, false),
            ["wait", "turn_left"]
        );
        let right = observation(500, 100, 10);
        assert_eq!(
            current_vey_candidates(&right, FRAME_WIDTH, false),
            ["wait", "turn_right"]
        );
    }

    #[test]
    fn vey_explores_when_no_enemy_is_visible_and_turns_if_stuck() {
        let mut empty = observation(0, 100, 10);
        empty.enemies.clear();
        assert_eq!(
            current_vey_candidates(&empty, FRAME_WIDTH, false),
            ["wait", "advance"]
        );
        empty.forward_clearance = 0;
        empty.left_clearance = 2;
        empty.right_clearance = 1;
        assert_eq!(
            current_vey_candidates(&empty, FRAME_WIDTH, true),
            ["wait", "turn_left"]
        );
        assert_eq!(action_key("advance"), Some(KEY_UPARROW));
    }

    #[test]
    fn vey_never_controls_dead_players_or_accepts_unknown_actions() {
        let mut dead = observation(300, 0, 10);
        dead.player_state = 1;
        assert_eq!(current_vey_candidates(&dead, FRAME_WIDTH, false), ["wait"]);
        assert_eq!(action_key("open_console"), None);
    }

    #[test]
    fn vey_request_contains_explicit_structured_action_facts() {
        let request = make_vey_request(17, &observation(300, 100, 10), FRAME_WIDTH, false).unwrap();
        assert_eq!(request["schema"], 1);
        assert_eq!(request["sequence"], 17);
        assert_eq!(
            request["candidates"]["fire"].as_str(),
            Some("tactical_priority 90; ammo_cost 1")
        );
    }
}
