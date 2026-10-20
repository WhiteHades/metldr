use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use async_tungstenite::{
    tokio::accept_hdr_async_with_config,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
        protocol::WebSocketConfig,
    },
};
use futures::StreamExt;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpListener,
    sync::mpsc,
    task::JoinHandle,
    time::{Instant, timeout_at},
};

const HANDSHAKE_LIMIT: usize = 8192;

pub(crate) struct Socket {
    pub connection: chromiumoxide::Connection<super::RawEvent>,
    pub commands: mpsc::Receiver<super::WireCommand>,
    pub incoming: mpsc::Sender<serde_json::Value>,
    task: JoinHandle<super::Result<()>>,
}

impl Socket {
    pub(crate) async fn open(deadline: Instant) -> super::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|e| e.to_string())?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = format!("/{token}");
        let host = address.to_string();
        let url = format!("ws://{host}{path}");

        let server = async move {
            let (stream, peer) = listener.accept().await.map_err(|e| e.to_string())?;
            if peer.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
                return Err("WebSocket peer was not loopback".to_owned());
            }
            drop(listener);
            let stream = CappedRead {
                stream,
                read: 0,
                header: Vec::new(),
                handshake_done: false,
            };
            let callback = move |request: &Request, response: Response| {
                let host_count = request.headers().get_all("host").iter().count();
                let valid = request.method() == "GET"
                    && request.uri().path_and_query().map(|p| p.as_str()) == Some(path.as_str())
                    && host_count == 1
                    && request.headers().get("host").and_then(|v| v.to_str().ok())
                        == Some(host.as_str())
                    && !request.headers().contains_key("origin")
                    && !request.headers().contains_key("sec-websocket-extensions")
                    && !request.headers().contains_key("sec-websocket-protocol");
                if valid { Ok(response) } else { Err(reject()) }
            };
            let config = WebSocketConfig::default()
                .write_buffer_size(16 * 1024)
                .max_frame_size(Some(super::MAX_MESSAGE))
                .max_message_size(Some(super::MAX_MESSAGE))
                .max_write_buffer_size(super::MAX_MESSAGE * 2);
            accept_hdr_async_with_config(stream, callback, Some(config))
                .await
                .map_err(|e| e.to_string())
        };

        let (connection, websocket) = tokio::try_join!(
            async {
                timeout_at(
                    deadline,
                    chromiumoxide::Connection::<super::RawEvent>::connect(url),
                )
                .await
                .map_err(|_| "WebSocket client connection timed out".to_owned())?
                .map_err(|e| e.to_string())
            },
            async {
                timeout_at(deadline, server)
                    .await
                    .map_err(|_| "WebSocket server handshake timed out".to_owned())?
            },
        )?;

        let (command_tx, commands) = mpsc::channel(super::CAPACITY);
        let (incoming, mut incoming_rx) = mpsc::channel(super::CAPACITY);
        let task = tokio::spawn(async move {
            let (mut sink, mut stream) = websocket.split();
            loop {
                tokio::select! {
                    message = stream.next() => match message {
                        Some(Ok(Message::Text(text))) => {
                            if text.len() > super::MAX_MESSAGE { return Err("WebSocket command exceeded the message limit".into()); }
                            let command: super::WireCommand = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                            command.validate()?;
                            command_tx.try_send(command).map_err(|e| format!("WebSocket command queue is full or closed: {e}"))?;
                        }
                        Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(Message::Close(_))) | None => return Ok(()),
                        Some(Ok(_)) => return Err("WebSocket accepts text messages only".into()),
                        Some(Err(error)) => return Err(error.to_string()),
                    },
                    value = incoming_rx.recv() => match value {
                        Some(value) => {
                            let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
                            if text.len() > super::MAX_MESSAGE { return Err("WebSocket event exceeded the message limit".into()); }
                            timeout_at(Instant::now() + super::TIMEOUT, sink.send(Message::Text(text.into())))
                                .await.map_err(|_| "WebSocket send timed out".to_owned())?
                                .map_err(|e| e.to_string())?;
                        }
                        None => {
                            timeout_at(Instant::now() + super::TIMEOUT, sink.close(None)).await
                                .map_err(|_| "WebSocket close timed out".to_owned())?
                                .map_err(|e| e.to_string())?;
                            return Ok(());
                        }
                    }
                }
            }
        });

        Ok(Self {
            connection,
            commands,
            incoming,
            task,
        })
    }

    pub(crate) fn abort(&self) {
        self.task.abort();
    }

    pub(crate) async fn shutdown(&mut self) -> super::Result<()> {
        self.task.abort();
        match (&mut self.task).await {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn reject() -> async_tungstenite::tungstenite::handshake::server::ErrorResponse {
    use async_tungstenite::tungstenite::http::Response as HttpResponse;
    HttpResponse::builder()
        .status(400)
        .body(Some("Invalid WebSocket request".into()))
        .unwrap()
}

struct CappedRead<S> {
    stream: S,
    read: usize,
    header: Vec<u8>,
    handshake_done: bool,
}

impl<S: AsyncRead + Unpin> AsyncRead for CappedRead<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let handshake = !self.handshake_done;
        let remaining = if self.handshake_done {
            buffer.remaining()
        } else {
            HANDSHAKE_LIMIT.saturating_sub(self.read)
        };
        if remaining == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "handshake too large",
            )));
        }
        let limit = remaining.min(buffer.remaining());
        let (read, bytes) = {
            let mut capped = ReadBuf::new(buffer.initialize_unfilled_to(limit));
            match Pin::new(&mut self.stream).poll_read(cx, &mut capped) {
                Poll::Ready(Ok(())) => (
                    capped.filled().len(),
                    if handshake {
                        capped.filled().to_vec()
                    } else {
                        Vec::new()
                    },
                ),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        };
        if !self.handshake_done && read > 0 {
            self.read += read;
            self.header.extend_from_slice(&bytes);
            self.handshake_done = self.header.windows(4).any(|window| window == b"\r\n\r\n");
        }
        buffer.advance(read);
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for CappedRead<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
