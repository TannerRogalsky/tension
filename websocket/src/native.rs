use std::sync::mpsc;
use ws::{Handler, Handshake};

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ConnectionFuture {
    rx: Option<mpsc::Receiver<super::WebSocketEvent>>,
    channel: futures::channel::oneshot::Receiver<Result<ws::Sender, ws::Error>>,
}

impl std::future::Future for ConnectionFuture {
    type Output = Result<WebSocket, super::WebSocketError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        use futures::FutureExt;
        self.channel
            .poll_unpin(cx)
            .map(|result| match result.unwrap() {
                Ok(sender) => Ok(WebSocket {
                    rx: self.rx.take().unwrap(),
                    sender,
                }),
                Err(_err) => Err(super::WebSocketError::CreationError),
            })
    }
}

impl futures::future::FusedFuture for ConnectionFuture {
    fn is_terminated(&self) -> bool {
        self.rx.is_none()
    }
}

#[derive(Debug)]
pub struct WebSocket {
    rx: mpsc::Receiver<super::WebSocketEvent>,
    sender: ws::Sender,
}

impl WebSocket {
    pub fn connect<S: AsRef<str>>(url: S) -> ConnectionFuture {
        let (tx, rx) = mpsc::channel();
        let (sx, trx) = mpsc::sync_channel(1);
        std::thread::spawn({
            let sx = sx.clone();
            let url = url.as_ref().to_owned();
            move || {
                let result = ws::connect(url.as_str(), {
                    let sx = sx.clone();
                    move |sender| {
                        sx.send(Ok(sender))
                            .expect("could not send connection to client.");
                        MyHandler {
                            tx: mpsc::Sender::clone(&tx),
                        }
                    }
                });
                if let Err(err) = result {
                    sx.send(Err(err)).expect("could not send error to client.");
                }
            }
        });

        let (notice_send, notice_recv) = futures::channel::oneshot::channel();
        std::thread::spawn(move || match trx.recv() {
            Ok(result) => notice_send.send(result),
            Err(err) => notice_send.send(Err(ws::Error::new(
                ws::ErrorKind::Internal,
                err.to_string(),
            ))),
        });

        ConnectionFuture {
            rx: Some(rx),
            channel: notice_recv,
        }
    }

    pub fn poll(&self) -> Option<super::WebSocketEvent> {
        self.rx.try_recv().ok()
    }

    pub fn send(&self, msg: super::Message) -> Result<(), super::WebSocketError> {
        self.sender
            .send(msg)
            .map_err(|_err| super::WebSocketError::SendError)
    }
}

struct MyHandler {
    tx: mpsc::Sender<super::WebSocketEvent>,
}

impl Handler for MyHandler {
    fn on_open(&mut self, _shake: Handshake) -> ws::Result<()> {
        self.tx
            .send(super::WebSocketEvent::Open)
            .map_err(|err| ws::Error::new(ws::ErrorKind::Custom(Box::new(err)), ""))
    }

    fn on_message(&mut self, msg: ws::Message) -> ws::Result<()> {
        self.tx
            .send(super::WebSocketEvent::Message(msg.into()))
            .map_err(|err| ws::Error::new(ws::ErrorKind::Custom(Box::new(err)), ""))
    }

    fn on_close(&mut self, code: ws::CloseCode, _reason: &str) {
        let _result = self.tx.send(super::WebSocketEvent::Close(code.into()));
    }

    fn on_error(&mut self, _err: ws::Error) {
        let _result = self.tx.send(super::WebSocketEvent::Error(
            super::WebSocketError::ReceiveError,
        ));
    }
}

impl Into<ws::Message> for super::Message {
    fn into(self) -> ws::Message {
        match self {
            super::Message::Text(text) => ws::Message::Text(text),
            super::Message::Binary(bin) => ws::Message::Binary(bin),
        }
    }
}

impl From<ws::Message> for super::Message {
    fn from(msg: ws::Message) -> Self {
        match msg {
            ws::Message::Text(text) => super::Message::Text(text),
            ws::Message::Binary(bin) => super::Message::Binary(bin),
        }
    }
}

impl From<ws::CloseCode> for super::CloseCode {
    fn from(code: ws::CloseCode) -> Self {
        match code {
            ws::CloseCode::Normal => super::CloseCode::Normal,
            ws::CloseCode::Away => super::CloseCode::Away,
            ws::CloseCode::Protocol => super::CloseCode::Protocol,
            ws::CloseCode::Unsupported => super::CloseCode::Unsupported,
            ws::CloseCode::Status => super::CloseCode::Status,
            ws::CloseCode::Abnormal => super::CloseCode::Abnormal,
            ws::CloseCode::Invalid => super::CloseCode::Invalid,
            ws::CloseCode::Policy => super::CloseCode::Policy,
            ws::CloseCode::Size => super::CloseCode::Size,
            ws::CloseCode::Extension => super::CloseCode::Extension,
            ws::CloseCode::Error => super::CloseCode::Error,
            ws::CloseCode::Restart => super::CloseCode::Restart,
            ws::CloseCode::Again => super::CloseCode::Again,
            ws::CloseCode::Tls => super::CloseCode::Tls,
            ws::CloseCode::Empty => super::CloseCode::Empty,
            ws::CloseCode::Other(code) => super::CloseCode::Other(code),
        }
    }
}
