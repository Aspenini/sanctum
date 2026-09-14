use crate::protocol;
use holyc::CompileOptions;
use std::io::{self, BufRead, BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug)]
pub enum RunnerEvent {
    Status(String),
    Frame {
        sequence: u64,
        width: u32,
        height: u32,
        indexed: Vec<u8>,
    },
    Menu(Option<String>),
    Log(String),
    Warning(String),
    Error(String),
    Exited(bool),
}

pub struct RunnerSession {
    child: Child,
    writer: Arc<Mutex<TcpStream>>,
    events: mpsc::Receiver<RunnerEvent>,
    stopping_since: Option<Instant>,
}

impl RunnerSession {
    pub fn spawn(entry: &Path, templeos_root: Option<&Path>, data_root: &Path) -> io::Result<Self> {
        Self::spawn_with_executable(&std::env::current_exe()?, entry, templeos_root, data_root)
    }

    /// Spawn a runner using an explicit Sanctum executable.
    ///
    /// This is primarily useful to exercise the real child protocol in tests.
    pub fn spawn_with_executable(
        executable: &Path,
        entry: &Path,
        templeos_root: Option<&Path>,
        data_root: &Path,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let token = Uuid::new_v4().to_string();
        let mut command = Command::new(executable);
        command
            .arg("--runner")
            .arg(entry)
            .env("SANCTUM_RUNNER_ADDR", address.to_string())
            .env("SANCTUM_RUNNER_TOKEN", &token)
            .env("SANCTUM_DATA_DIR", data_root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(root) = templeos_root {
            command.env("SANCTUM_TEMPLEOS_ROOT", root);
        } else {
            command.env_remove("SANCTUM_TEMPLEOS_ROOT");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    if child.try_wait()?.is_some() {
                        return Err(io::Error::other("runner exited before connecting"));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let _ = child.kill();
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "runner connection timed out",
                    ));
                }
                Err(error) => return Err(error),
            }
        };
        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        let (kind, hello) = protocol::read_message(&mut stream)?;
        let expected = format!("{}:{token}", protocol::VERSION);
        if kind != protocol::HELLO || hello != expected.as_bytes() {
            let _ = child.kill();
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "runner authentication failed",
            ));
        }

        let (tx, events) = mpsc::channel();
        let reader = stream.try_clone()?;
        let event_tx = tx.clone();
        thread::spawn(move || read_events(reader, event_tx));
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(stdout, tx.clone(), false);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(stderr, tx, true);
        }

        Ok(Self {
            child,
            writer: Arc::new(Mutex::new(stream)),
            events,
            stopping_since: None,
        })
    }

    pub fn try_recv(&self) -> Option<RunnerEvent> {
        self.events.try_recv().ok()
    }

    pub fn send_key(&self, ch: i64, scan: i64) -> io::Result<()> {
        let payload = protocol::encode_key(ch, scan);
        protocol::write_message(&mut *self.writer.lock().unwrap(), protocol::KEY, &payload)
    }

    pub fn send_mouse(&self, x: i64, y: i64, left: bool, right: bool) -> io::Result<()> {
        let payload = protocol::encode_mouse(x, y, left, right);
        protocol::write_message(&mut *self.writer.lock().unwrap(), protocol::MOUSE, &payload)
    }

    pub fn set_muted(&self, muted: bool) -> io::Result<()> {
        protocol::write_message(
            &mut *self.writer.lock().unwrap(),
            protocol::MUTE,
            &[u8::from(muted)],
        )
    }

    pub fn stop(&mut self) -> io::Result<()> {
        if self.stopping_since.is_none() {
            protocol::write_message(&mut *self.writer.lock().unwrap(), protocol::STOP, &[])?;
            self.stopping_since = Some(Instant::now());
        }
        Ok(())
    }

    pub fn update_lifecycle(&mut self) -> io::Result<bool> {
        if self.child.try_wait()?.is_some() {
            return Ok(false);
        }
        if self
            .stopping_since
            .is_some_and(|started| started.elapsed() >= Duration::from_secs(2))
        {
            self.child.kill()?;
            let _ = self.child.wait();
            return Ok(false);
        }
        Ok(true)
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for RunnerSession {
    fn drop(&mut self) {
        self.kill();
    }
}

fn spawn_log_reader(
    stream: impl Read + Send + 'static,
    tx: mpsc::Sender<RunnerEvent>,
    error: bool,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            let count = reader.read_until(b'\n', &mut bytes).unwrap_or(0);
            if count == 0 {
                break;
            }
            let text = holyc_syntax::decode_cp437(&bytes);
            let event = if error && text.starts_with("Sanctum audio unavailable:") {
                RunnerEvent::Warning(text)
            } else if error {
                RunnerEvent::Error(text)
            } else {
                RunnerEvent::Log(text)
            };
            if tx.send(event).is_err() {
                break;
            }
        }
    });
}

fn read_events(mut stream: TcpStream, tx: mpsc::Sender<RunnerEvent>) {
    while let Ok((kind, payload)) = protocol::read_message(&mut stream) {
        let event = match kind {
            protocol::STATUS => RunnerEvent::Status(String::from_utf8_lossy(&payload).into_owned()),
            protocol::FRAME if payload.len() >= 16 => {
                let sequence = u64::from_le_bytes(payload[..8].try_into().unwrap());
                let width = u32::from_le_bytes(payload[8..12].try_into().unwrap());
                let height = u32::from_le_bytes(payload[12..16].try_into().unwrap());
                if usize::try_from(width).ok().and_then(|width| {
                    usize::try_from(height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                }) != Some(payload.len() - 16)
                {
                    RunnerEvent::Error("runner sent an invalid frame".into())
                } else {
                    RunnerEvent::Frame {
                        sequence,
                        width,
                        height,
                        indexed: payload[16..].to_vec(),
                    }
                }
            }
            protocol::MENU => RunnerEvent::Menu(
                (!payload.is_empty()).then(|| String::from_utf8_lossy(&payload).into_owned()),
            ),
            protocol::LOG => RunnerEvent::Log(String::from_utf8_lossy(&payload).into_owned()),
            protocol::ERROR => RunnerEvent::Error(String::from_utf8_lossy(&payload).into_owned()),
            protocol::EXITED => RunnerEvent::Exited(payload.first() == Some(&1)),
            _ => RunnerEvent::Error("runner sent an unknown message".into()),
        };
        if tx.send(event).is_err() {
            break;
        }
    }
}

fn queue_message(
    writer: &mpsc::SyncSender<(u8, Vec<u8>)>,
    kind: u8,
    payload: impl Into<Vec<u8>>,
) -> Result<(), String> {
    writer
        .send((kind, payload.into()))
        .map_err(|_| "runner IPC writer disconnected".to_string())
}

pub fn run_child(entry: PathBuf) -> Result<(), String> {
    let address = std::env::var("SANCTUM_RUNNER_ADDR").map_err(|_| "missing runner address")?;
    let token = std::env::var("SANCTUM_RUNNER_TOKEN").map_err(|_| "missing runner token")?;
    let templeos_root = std::env::var_os("SANCTUM_TEMPLEOS_ROOT").map(PathBuf::from);
    let stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    let mut writer_stream = stream.try_clone().map_err(|error| error.to_string())?;
    let (writer, outbound) = mpsc::sync_channel::<(u8, Vec<u8>)>(8);
    let writer_thread = thread::spawn(move || {
        while let Ok((kind, payload)) = outbound.recv() {
            if let Err(error) = protocol::write_message(&mut writer_stream, kind, &payload) {
                eprintln!("runner IPC writer failed: {error}");
                break;
            }
        }
    });
    queue_message(
        &writer,
        protocol::HELLO,
        format!("{}:{token}", protocol::VERSION).into_bytes(),
    )?;

    queue_message(&writer, protocol::STATUS, b"Compiling".to_vec())?;
    queue_message(
        &writer,
        protocol::LOG,
        format!("Compiling {}\n", entry.display()).into_bytes(),
    )?;
    let options = CompileOptions {
        system_root: templeos_root.clone(),
        ..CompileOptions::default()
    };
    let program = match holyc::compile_file(&entry, &options) {
        Ok(program) => program,
        Err(error) => {
            let message = error.to_string();
            let _ = queue_message(&writer, protocol::ERROR, message.clone().into_bytes());
            let _ = queue_message(&writer, protocol::EXITED, vec![0]);
            drop(writer);
            let _ = writer_thread.join();
            return Err(message);
        }
    };
    queue_message(&writer, protocol::LOG, b"Compilation finished\n".to_vec())?;
    templeos_compat::host::set_file_roots(entry.parent().map(PathBuf::from), templeos_root.clone());
    let mut command_reader = stream;
    thread::spawn(move || {
        while let Ok((kind, payload)) = protocol::read_message(&mut command_reader) {
            match kind {
                protocol::KEY => {
                    if let Some((ch, scan)) = protocol::decode_key(&payload) {
                        templeos_compat::host::push_key_event(ch, scan);
                    }
                }
                protocol::MOUSE => {
                    if let Some((x, y, left, right)) = protocol::decode_mouse(&payload) {
                        templeos_compat::host::set_mouse(x, y, left, right);
                    }
                }
                protocol::STOP => templeos_compat::host::request_exit(),
                protocol::MUTE => templeos_compat::host::set_muted(payload.first() == Some(&1)),
                _ => {}
            }
        }
        templeos_compat::host::request_exit();
    });
    queue_message(&writer, protocol::STATUS, b"Running".to_vec())?;
    let done = Arc::new(AtomicBool::new(false));
    let frame_done = done.clone();
    let frame_writer = writer.clone();
    let frame_thread = thread::spawn(move || {
        let mut sequence = 0;
        let mut menu = None;
        while !frame_done.load(Ordering::Acquire) {
            if let Some(frame) = templeos_compat::host::frame_snapshot_after(sequence) {
                sequence = frame.sequence;
                let mut payload = Vec::with_capacity(16 + frame.indexed.len());
                payload.extend_from_slice(&frame.sequence.to_le_bytes());
                payload.extend_from_slice(&frame.width.to_le_bytes());
                payload.extend_from_slice(&frame.height.to_le_bytes());
                payload.extend_from_slice(&frame.indexed);
                // Frames are replaceable. Never let a slow GUI stall HolyC.
                let _ = frame_writer.try_send((protocol::FRAME, payload));
                if frame.menu != menu {
                    menu = frame.menu;
                    let source = menu.as_deref().unwrap_or_default();
                    let _ = queue_message(&frame_writer, protocol::MENU, source.to_vec());
                }
            }
            thread::sleep(Duration::from_millis(16));
        }
    });
    let result = holyc::run_program(program, templeos_compat::HostMode::External);
    done.store(true, Ordering::Release);
    let _ = frame_thread.join();
    if let Err(error) = &result {
        let _ = queue_message(&writer, protocol::ERROR, error.to_string().into_bytes());
    }
    let _ = queue_message(&writer, protocol::EXITED, vec![u8::from(result.is_ok())]);
    drop(writer);
    let _ = writer_thread.join();
    result.map_err(|error| error.to_string())
}
