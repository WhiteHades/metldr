use super::Input;
use crate::flow::Event;
use crate::model::Request;
use crate::{FRAME_MAX, Result, WIRE_VERSION};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const HELLO_TIMEOUT: Duration = Duration::from_secs(3);
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(2);
const HELLO_MAX: usize = 4096;

pub(crate) enum SocketInput {
    Hello { origin: String, capability: String },
    Request(Request),
    Closed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    v: u8,
    kind: String,
    origin: String,
    capability: String,
}

pub(super) struct SocketTransport {
    listener: UnixListener,
    path: PathBuf,
    identity: (u64, u64),
    capability: String,
    origin: String,
    next_epoch: u64,
    pending: Option<u64>,
    active: Option<u64>,
    stream: Option<UnixStream>,
}

impl SocketTransport {
    pub(super) fn new(path: PathBuf, capability: String, origin: String) -> Result<Self> {
        if !path.is_absolute()
            || path.as_os_str().len() > 100
            || capability.len() != 64
            || !capability
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid socket transport configuration".into());
        }
        let parent = path.parent().ok_or("socket path has no parent")?;
        let metadata = fs::metadata(parent)?;
        if !metadata.is_dir() || metadata.mode() & 0o077 != 0 {
            return Err("socket parent must be a private directory".into());
        }
        let listener = UnixListener::bind(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        let identity = (metadata.dev(), metadata.ino());
        let setup = (|| -> Result<()> {
            if !metadata.file_type().is_socket() {
                return Err("created socket endpoint changed type".into());
            }
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;
            Ok(())
        })();
        if let Err(error) = setup {
            remove_if_same(&path, identity);
            return Err(error);
        }
        Ok(Self {
            listener,
            path,
            identity,
            capability,
            origin,
            next_epoch: 1,
            pending: None,
            active: None,
            stream: None,
        })
    }

    pub(super) fn poll(
        &mut self,
        tx: &SyncSender<Event>,
        reader_available: bool,
    ) -> Result<Option<JoinHandle<()>>> {
        let (stream, _) = match self.listener.accept() {
            Ok(client) => client,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if self.pending.is_some() || self.active.is_some() || !reader_available {
            drop(stream);
            return Ok(None);
        }
        let reader = stream.try_clone()?;
        let epoch = self.next_epoch;
        self.next_epoch = self
            .next_epoch
            .checked_add(1)
            .ok_or("socket epoch capacity reached")?;
        self.pending = Some(epoch);
        self.stream = Some(stream);
        let sender = tx.clone();
        Ok(Some(thread::spawn(move || {
            read_client(reader, epoch, sender)
        })))
    }

    pub(super) fn input(&mut self, epoch: u64, message: SocketInput) -> super::Action {
        if self.pending != Some(epoch) && self.active != Some(epoch) {
            return super::Action::Ignore;
        }
        match message {
            SocketInput::Hello { origin, capability } => {
                if self.pending != Some(epoch)
                    || origin != self.origin
                    || capability != self.capability
                {
                    self.disconnect(epoch);
                    return super::Action::Ignore;
                }
                self.pending = None;
                self.active = Some(epoch);
                super::Action::Connected
            }
            SocketInput::Request(request) if self.active == Some(epoch) => {
                super::Action::Request(request)
            }
            SocketInput::Closed => {
                let was_active = self.active == Some(epoch);
                self.disconnect(epoch);
                if was_active {
                    super::Action::Detached
                } else {
                    super::Action::Ignore
                }
            }
            _ => super::Action::Ignore,
        }
    }

    pub(super) fn emit(&mut self, value: &Value) -> Result<()> {
        if self.active.is_none() {
            return Ok(());
        }
        let Some(stream) = self.stream.as_mut() else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(value)?;
        if bytes.is_empty() || bytes.len() > FRAME_MAX {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Ok(());
        }
        if write_raw_deadline(stream, &bytes, Instant::now() + OUTPUT_TIMEOUT).is_err() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        Ok(())
    }

    fn disconnect(&mut self, epoch: u64) {
        if self.pending == Some(epoch) {
            self.pending = None;
        }
        if self.active == Some(epoch) {
            self.active = None;
        }
        if self.pending.is_none() && self.active.is_none() {
            if let Some(stream) = self.stream.take() {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    pub(super) fn finish(&mut self) {
        self.pending = None;
        self.active = None;
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        remove_if_same(&self.path, self.identity);
    }
}

fn remove_if_same(path: &Path, identity: (u64, u64)) {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.dev(), metadata.ino()) == identity
    {
        let _ = fs::remove_file(path);
    }
}

impl Drop for SocketTransport {
    fn drop(&mut self) {
        self.finish();
    }
}

fn read_client(mut stream: UnixStream, epoch: u64, tx: SyncSender<Event>) {
    let result = (|| -> Result<()> {
        let hello_bytes =
            read_raw_deadline(&mut stream, HELLO_MAX, Instant::now() + HELLO_TIMEOUT)?
                .ok_or("socket closed before hello")?;
        let hello: Hello = serde_json::from_slice(&hello_bytes)?;
        if hello.v != WIRE_VERSION || hello.kind != "bridge_hello" {
            return Err("invalid bridge hello".into());
        }
        send(
            &tx,
            epoch,
            SocketInput::Hello {
                origin: hello.origin,
                capability: hello.capability,
            },
        )?;
        stream.set_read_timeout(None)?;
        loop {
            let Some(bytes) = crate::wire::read_raw(&mut stream, FRAME_MAX)? else {
                break;
            };
            let request = crate::wire::decode_request(&bytes)?;
            send(&tx, epoch, SocketInput::Request(request))?;
        }
        Ok(())
    })();
    let _ = result;
    let _ = send(&tx, epoch, SocketInput::Closed);
}

fn write_raw_deadline(stream: &mut UnixStream, bytes: &[u8], deadline: Instant) -> Result<()> {
    if bytes.is_empty() || bytes.len() > FRAME_MAX {
        return Err("outgoing native frame exceeds bound".into());
    }
    let header = (bytes.len() as u32).to_ne_bytes();
    write_all_deadline(stream, &header, deadline)?;
    write_all_deadline(stream, bytes, deadline)
}

fn write_all_deadline(
    stream: &mut UnixStream,
    mut remaining: &[u8],
    deadline: Instant,
) -> Result<()> {
    while !remaining.is_empty() {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err("socket output deadline exceeded".into());
        }
        stream.set_write_timeout(Some(timeout))?;
        match stream.write(remaining) {
            Ok(0) => return Err("socket closed during frame write".into()),
            Ok(n) => remaining = &remaining[n..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn send(tx: &SyncSender<Event>, epoch: u64, message: SocketInput) -> Result<()> {
    tx.send(Event::Input(Input::Socket { epoch, message }))
        .map_err(|_| "owner input receiver closed".into())
}

fn read_raw_deadline(
    stream: &mut UnixStream,
    max: usize,
    deadline: Instant,
) -> Result<Option<Vec<u8>>> {
    let mut header = [0; 4];
    if !read_exact_deadline(stream, &mut header[..1], deadline, true)? {
        return Ok(None);
    }
    read_exact_deadline(stream, &mut header[1..], deadline, false)?;
    let size = u32::from_ne_bytes(header) as usize;
    if size == 0 || size > max || size > FRAME_MAX {
        return Err("invalid native frame length".into());
    }
    let mut bytes = vec![0; size];
    read_exact_deadline(stream, &mut bytes, deadline, false)?;
    Ok(Some(bytes))
}

fn read_exact_deadline(
    stream: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
    eof_ok: bool,
) -> Result<bool> {
    let mut first = true;
    while !bytes.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("bridge hello deadline exceeded".into());
        }
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(bytes) {
            Ok(0) if eof_ok && first => return Ok(false),
            Ok(0) => return Err("truncated native frame".into()),
            Ok(count) => {
                first = false;
                bytes = &mut bytes[count..];
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

pub(super) fn control_reader(tx: SyncSender<Event>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut byte = [0];
        let event = match stdin.read(&mut byte) {
            Ok(0) => Input::ControlClosed(None),
            Ok(_) => Input::ControlClosed(Some("unexpected owner control input".into())),
            Err(error) => Input::ControlClosed(Some(error.to_string())),
        };
        let _ = tx.send(Event::Input(event));
    })
}

pub(super) fn bridge(path: &Path, capability: &str, origin: &str) -> Result<()> {
    let mut socket = UnixStream::connect(path)?;
    crate::wire::write_raw(
        &mut socket,
        &serde_json::to_vec(&json!({
            "v": WIRE_VERSION,
            "kind": "bridge_hello",
            "origin": origin,
            "capability": capability,
        }))?,
    )?;
    let mut input = socket.try_clone()?;
    let writer = thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            let bytes = match crate::wire::read_raw(&mut stdin, FRAME_MAX) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    let _ = input.shutdown(std::net::Shutdown::Write);
                    break;
                }
                Err(_) => {
                    let _ = input.shutdown(std::net::Shutdown::Both);
                    break;
                }
            };
            if crate::wire::decode_request(&bytes).is_err() {
                let _ = input.shutdown(std::net::Shutdown::Both);
                break;
            }
            if crate::wire::write_raw(&mut input, &bytes).is_err() {
                break;
            }
        }
    });
    let mut stdout = std::io::stdout().lock();
    loop {
        let Some(bytes) = crate::wire::read_raw(&mut socket, FRAME_MAX)? else {
            break;
        };
        let value: Value = serde_json::from_slice(&bytes)?;
        crate::wire::write_raw(&mut stdout, &serde_json::to_vec(&value)?)?;
    }
    drop(stdout);
    drop(socket);
    drop(writer);
    Ok(())
}
