use crate::{PiProfile, ProcessExit, Result, mcp::Registration};
use serde_json::Value;
use std::{
    io::{self, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) struct Session {
    child: Child,
    stdin: Option<SyncSender<WriteRequest>>,
    writer: Option<JoinHandle<()>>,
    rx: Receiver<Result<Value>>,
    stdout: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
    closed: bool,
    forced: bool,
    registration: Option<Registration>,
}

struct WriteRequest {
    bytes: Vec<u8>,
    ack: mpsc::SyncSender<io::Result<()>>,
}

impl Session {
    pub(crate) fn spawn(profile: PiProfile) -> Result<Self> {
        let registration = profile.mcp.as_ref().map(Registration::create).transpose()?;
        let mut command = Command::new(profile.executable);
        command
            .current_dir(profile.workspace)
            .arg("--mode")
            .arg("rpc")
            .arg("--provider")
            .arg(profile.provider)
            .arg("--model")
            .arg(profile.model)
            .arg("--session-dir")
            .arg(profile.session_dir);
        if let Some(session) = profile.resume {
            command.arg("--session").arg(session);
        }
        if profile.offline {
            command.arg("--offline");
        }
        if let (Some(mcp), Some(registration)) = (&profile.mcp, &registration) {
            command.arg("--extension").arg(registration.path());
            command.env("METLDR_PI_BROWSER_TOKEN", mcp.token());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Pi stdin was not piped"))?;
        let out = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Pi stdout was not piped"))?;
        let err = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("Pi stderr was not piped"))?;
        let (tx, rx) = mpsc::channel();
        let (stdin_tx, stdin_rx) = mpsc::sync_channel(1);
        let writer = thread::spawn(move || write_stdin(stdin, stdin_rx));
        let stdout = thread::spawn(move || crate::rpc::read_stdout(out, tx));
        let stderr = thread::spawn(move || {
            let _ = io::copy(&mut io::BufReader::new(err), &mut io::sink());
        });
        Ok(Self {
            child,
            stdin: Some(stdin_tx),
            writer: Some(writer),
            rx,
            stdout: Some(stdout),
            stderr: Some(stderr),
            closed: false,
            forced: false,
            registration,
        })
    }

    pub(crate) fn send(&mut self, value: Value) -> Result<()> {
        let stdin = self
            .stdin
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Pi session is closed"))?;
        let mut bytes = serde_json::to_vec(&value)?;
        if bytes.len() > 32 * 1024 {
            return Err(
                io::Error::new(io::ErrorKind::InvalidInput, "Pi command exceeds 32 KiB").into(),
            );
        }
        bytes.push(b'\n');
        let (ack, result) = mpsc::sync_channel(1);
        match stdin.try_send(WriteRequest { bytes, ack }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                return Err(
                    io::Error::new(io::ErrorKind::WouldBlock, "Pi stdin writer is busy").into(),
                );
            }
            Err(TrySendError::Disconnected(_)) => {
                return self.poison_writer(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Pi stdin writer stopped",
                ));
            }
        }
        match result.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => self.poison_writer(error),
            Err(mpsc::RecvTimeoutError::Timeout) => self.poison_writer(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out writing to Pi stdin; session was terminated",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => self.poison_writer(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Pi stdin writer stopped before acknowledging the command",
            )),
        }
    }

    pub(crate) fn recv(&self, timeout: Duration) -> Result<Value> {
        match self.rx.recv_timeout(timeout) {
            Ok(value) => value,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for Pi RPC output",
            )
            .into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Pi RPC output closed").into())
            }
        }
    }

    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    fn poison_writer<T>(&mut self, error: io::Error) -> Result<T> {
        self.stdin.take();
        self.forced = true;
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
            self.child.wait()?;
        }
        self.closed = true;
        self.registration.take();
        Err(error.into())
    }

    pub(crate) fn close(&mut self) -> Result<ProcessExit> {
        self.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            while let Ok(_) = self.rx.try_recv() {}
            if let Some(status) = self.child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                self.child.kill()?;
                self.forced = true;
                break self.child.wait()?;
            }
            thread::sleep(Duration::from_millis(20));
        };
        self.closed = true;
        self.registration.take();
        let deadline = Instant::now() + Duration::from_millis(200);
        while [&self.writer, &self.stdout, &self.stderr]
            .iter()
            .any(|reader| reader.as_ref().is_some_and(|t| !t.is_finished()))
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(5));
        }
        let mut drained = true;
        for reader in [&mut self.writer, &mut self.stdout, &mut self.stderr] {
            if reader.as_ref().is_some_and(|thread| thread.is_finished()) {
                reader
                    .take()
                    .unwrap()
                    .join()
                    .map_err(|_| io::Error::other("Pi pipe reader panicked"))?;
            } else if reader.is_some() {
                // shortcut: descendant cleanup belongs to the caller when inherited pipes remain open.
                drained = false;
            }
        }
        Ok(ProcessExit {
            code: status.code(),
            forced: self.forced,
            drained,
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.closed {
            self.stdin.take();
            let _ = self.child.kill();
            let _ = self.child.wait();
            if self.writer.as_ref().is_some_and(JoinHandle::is_finished) {
                let _ = self.writer.take().unwrap().join();
            }
        }
    }
}

fn write_stdin(mut stdin: ChildStdin, commands: Receiver<WriteRequest>) {
    while let Ok(command) = commands.recv() {
        let result = stdin.write_all(&command.bytes).and_then(|()| stdin.flush());
        let failed = result.is_err();
        let _ = command.ack.send(result);
        if failed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_close_waits_for_unfinished_readers() {
        if std::env::var_os("METLDR_PI_CLOSE_FIXTURE").is_some() {
            thread::sleep(Duration::from_secs(60));
            return;
        }
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process::tests::repeated_close_waits_for_unfinished_readers",
            ])
            .env("METLDR_PI_CLOSE_FIXTURE", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let (release, waiting) = mpsc::channel::<()>();
        let reader = thread::spawn(move || {
            let _ = waiting.recv();
        });
        let (_, rx) = mpsc::channel();
        let mut session = Session {
            child,
            stdin: None,
            writer: None,
            rx,
            stdout: Some(reader),
            stderr: None,
            closed: false,
            forced: false,
            registration: None,
        };
        let first = session.close().unwrap();
        assert!(!first.drained && first.forced);
        let second = session.close().unwrap();
        assert!(!second.drained && second.forced);
        release.send(()).unwrap();
        let third = session.close().unwrap();
        assert!(third.drained && third.forced);
        let fourth = session.close().unwrap();
        assert!(fourth.drained && fourth.forced);
    }

    #[test]
    fn stalled_stdin_times_out_poisons_and_kills_the_child() {
        if std::env::var_os("METLDR_PI_STDIN_STALL_FIXTURE").is_some() {
            thread::sleep(Duration::from_secs(2));
            return;
        }
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process::tests::stalled_stdin_times_out_poisons_and_kills_the_child",
            ])
            .env("METLDR_PI_STDIN_STALL_FIXTURE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let (writer_tx, writer_rx) = mpsc::sync_channel(1);
        let writer = thread::spawn(move || write_stdin(stdin, writer_rx));
        let mut session = Session {
            child,
            stdin: Some(writer_tx),
            writer: Some(writer),
            rx: mpsc::channel().1,
            stdout: None,
            stderr: None,
            closed: false,
            forced: false,
            registration: None,
        };
        let empty = serde_json::to_vec(&serde_json::json!({"payload":""}))
            .unwrap()
            .len();
        let command = serde_json::json!({"payload":"x".repeat(32 * 1024 - empty)});
        assert_eq!(serde_json::to_vec(&command).unwrap().len(), 32 * 1024);
        let started = Instant::now();
        let error = (0..64)
            .find_map(|_| session.send(command.clone()).err())
            .expect("stalled pipe accepted every bounded command");
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_millis(1500));
        assert!(
            session
                .send(serde_json::json!({"type":"get_state"}))
                .is_err()
        );
        let exit = session.close().unwrap();
        assert!(exit.forced && exit.drained);
        assert!(session.child.try_wait().unwrap().is_some());
    }
}
