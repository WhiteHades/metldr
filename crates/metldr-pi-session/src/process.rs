use crate::{PiProfile, ProcessExit, Result, mcp::Registration};
use serde_json::Value;
use std::{
    io::{self, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Result<Value>>,
    stdout: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
    closed: bool,
    forced: bool,
    registration: Option<Registration>,
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
        let stdout = thread::spawn(move || crate::rpc::read_stdout(out, tx));
        let stderr = thread::spawn(move || {
            let _ = io::copy(&mut io::BufReader::new(err), &mut io::sink());
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
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
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Pi session is closed"))?;
        let mut bytes = serde_json::to_vec(&value)?;
        if bytes.len() > 32 * 1024 {
            return Err(
                io::Error::new(io::ErrorKind::InvalidInput, "Pi command exceeds 32 KiB").into(),
            );
        }
        bytes.push(b'\n');
        // shortcut: synchronous writes rely on the owned OS deadline until companion supervision is integrated.
        stdin.write_all(&bytes)?;
        stdin.flush()?;
        Ok(())
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
        while [&self.stdout, &self.stderr]
            .iter()
            .any(|reader| reader.as_ref().is_some_and(|t| !t.is_finished()))
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(5));
        }
        let mut drained = true;
        for reader in [&mut self.stdout, &mut self.stderr] {
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
}
