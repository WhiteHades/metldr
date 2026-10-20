use crate::Result;
use crate::flow::Event;
use crate::model::Request;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::thread::JoinHandle;

#[cfg(unix)]
mod socket;

pub(crate) enum Input {
    StdioRequest(Request),
    ControlClosed(Option<String>),
    #[cfg(unix)]
    Socket {
        epoch: u64,
        message: socket::SocketInput,
    },
}

pub(crate) enum Action {
    Connected,
    Request(Request),
    Detached,
    Shutdown(Option<String>),
    Ignore,
}

pub(crate) struct Transport {
    mode: Mode,
    readers: Vec<JoinHandle<()>>,
    control_reader: Option<JoinHandle<()>>,
    started: bool,
    #[cfg(test)]
    captured_frames: Option<std::sync::Arc<std::sync::Mutex<Vec<u8>>>>,
}

enum Mode {
    Stdio,
    #[cfg(unix)]
    Socket(socket::SocketTransport),
}

impl Transport {
    pub(crate) fn stdio() -> Self {
        Self {
            mode: Mode::Stdio,
            readers: Vec::new(),
            control_reader: None,
            started: false,
            #[cfg(test)]
            captured_frames: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn captured() -> (Self, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
        let frames = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let transport = Self {
            mode: Mode::Stdio,
            readers: Vec::new(),
            control_reader: None,
            started: false,
            captured_frames: Some(frames.clone()),
        };
        (transport, frames)
    }

    pub(crate) fn socket(path: PathBuf, capability: String, origin: String) -> Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                mode: Mode::Socket(socket::SocketTransport::new(path, capability, origin)?),
                readers: Vec::new(),
                control_reader: None,
                started: false,
                #[cfg(test)]
                captured_frames: None,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (path, capability, origin);
            Err("socket transport is unsupported on this platform".into())
        }
    }

    pub(crate) fn start(&mut self, tx: SyncSender<Event>) -> Result<()> {
        if self.started {
            return Err("transport already started".into());
        }
        match &mut self.mode {
            Mode::Stdio => self.readers.push(stdio_reader(tx)),
            #[cfg(unix)]
            Mode::Socket(_) => self.control_reader = Some(socket::control_reader(tx)),
        }
        self.started = true;
        Ok(())
    }

    pub(crate) fn poll(&mut self, tx: &SyncSender<Event>) -> Result<()> {
        if !self.started {
            return Err("transport not started".into());
        }
        reap(&mut self.readers);
        #[cfg(unix)]
        if let Mode::Socket(socket) = &mut self.mode {
            if let Some(reader) = socket.poll(tx, self.readers.is_empty())? {
                self.readers.push(reader);
            }
        }
        Ok(())
    }

    pub(crate) fn input(&mut self, input: Input) -> Result<Action> {
        match (&mut self.mode, input) {
            (Mode::Stdio, Input::StdioRequest(request)) => Ok(Action::Request(request)),
            (Mode::Stdio, Input::ControlClosed(error)) => Ok(Action::Shutdown(error)),
            #[cfg(unix)]
            (Mode::Socket(_), Input::ControlClosed(error)) => Ok(Action::Shutdown(error)),
            #[cfg(unix)]
            (Mode::Socket(socket), Input::Socket { epoch, message }) => {
                Ok(socket.input(epoch, message))
            }
            _ => Ok(Action::Ignore),
        }
    }

    pub(crate) fn emit(&mut self, value: Value) -> Result<()> {
        #[cfg(test)]
        if let Some(frames) = &self.captured_frames {
            return crate::wire::write_frame(&mut *frames.lock().unwrap(), &value);
        }
        match &mut self.mode {
            Mode::Stdio => crate::wire::write_frame(&mut std::io::stdout().lock(), &value),
            #[cfg(unix)]
            Mode::Socket(socket) => socket.emit(&value),
        }
    }

    pub(crate) fn finish(&mut self) -> Result<()> {
        #[cfg(unix)]
        if let Mode::Socket(socket) = &mut self.mode {
            socket.finish();
        }
        for reader in self.readers.drain(..) {
            reader.join().map_err(|_| "transport reader panicked")?;
        }
        if let Some(reader) = self.control_reader.take() {
            reader
                .join()
                .map_err(|_| "transport control reader panicked")?;
        }
        Ok(())
    }
}

fn stdio_reader(tx: SyncSender<Event>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            match crate::wire::read_frame(&mut stdin) {
                Ok(Some(request)) => {
                    if tx.send(Event::Input(Input::StdioRequest(request))).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = tx.send(Event::Input(Input::ControlClosed(None)));
                    break;
                }
                Err(error) => {
                    let _ = tx.send(Event::Input(Input::ControlClosed(Some(error.to_string()))));
                    break;
                }
            }
        }
    })
}

fn reap(readers: &mut Vec<JoinHandle<()>>) {
    let mut i = 0;
    while i < readers.len() {
        if readers[i].is_finished() {
            let reader = readers.swap_remove(i);
            let _ = reader.join();
        } else {
            i += 1;
        }
    }
}

#[cfg(unix)]
pub(crate) fn bridge(path: &std::path::Path, capability: &str, origin: &str) -> Result<()> {
    socket::bridge(path, capability, origin)
}

#[cfg(not(unix))]
pub(crate) fn bridge(_path: &std::path::Path, _capability: &str, _origin: &str) -> Result<()> {
    Err("socket bridge is unsupported on this platform".into())
}
