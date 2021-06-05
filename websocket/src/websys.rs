use futures::{FutureExt, StreamExt};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent};

pub struct WebSocket {
    inner: web_sys::WebSocket,
    event_queue: std::sync::mpsc::Receiver<super::WebSocketEvent>,
    on_message_callback: Closure<dyn FnMut(MessageEvent)>,
    on_open_callback: Closure<dyn FnMut(JsValue)>,
    on_error_callback: Closure<dyn FnMut(ErrorEvent)>,
    on_close_callback: Closure<dyn FnMut(CloseEvent)>,
    on_open_notification: futures::channel::mpsc::Receiver<Result<(), super::WebSocketError>>,
}

impl WebSocket {
    pub fn connect<S: AsRef<str>>(url: S) -> ConnectionFuture {
        match web_sys::WebSocket::new(url.as_ref()) {
            Ok(ws) => ConnectionFuture::Connecting(Some(ws.into())),
            Err(_err) => ConnectionFuture::Error(futures::future::ready(
                super::WebSocketError::CreationError,
            )),
        }
    }

    pub fn poll(&self) -> Option<super::WebSocketEvent> {
        self.event_queue.try_recv().ok()
    }

    pub fn send(&self, msg: super::Message) -> Result<(), super::WebSocketError> {
        match msg {
            super::Message::Text(text) => self.inner.send_with_str(text.as_str()),
            super::Message::Binary(mut bin) => self.inner.send_with_u8_array(bin.as_mut_slice()),
        }
        .map_err(|_err| super::WebSocketError::SendError)
    }
}

impl Drop for WebSocket {
    fn drop(&mut self) {
        self.inner
            .remove_event_listener_with_callback(
                "message",
                self.on_message_callback.as_ref().unchecked_ref(),
            )
            .expect("failed to remove message event listener");
        self.inner
            .remove_event_listener_with_callback(
                "open",
                self.on_open_callback.as_ref().unchecked_ref(),
            )
            .expect("failed to remove open event listener");
        self.inner
            .remove_event_listener_with_callback(
                "error",
                self.on_error_callback.as_ref().unchecked_ref(),
            )
            .expect("failed to remove error event listener");
        self.inner
            .remove_event_listener_with_callback(
                "close",
                self.on_close_callback.as_ref().unchecked_ref(),
            )
            .expect("failed to remove close event listener");
    }
}

impl From<web_sys::WebSocket> for WebSocket {
    fn from(inner: web_sys::WebSocket) -> Self {
        let (sx, rx) = std::sync::mpsc::channel();
        let (mut on_open_sender, on_open_recver) = futures::channel::mpsc::channel(1);
        let on_message_callback = {
            let queue = sx.clone();
            Closure::wrap(Box::new(move |e: MessageEvent| {
                let result = match e.data().as_string() {
                    Some(response) => queue.send(super::WebSocketEvent::Message(
                        super::Message::Text(response),
                    )),
                    None => queue.send(super::WebSocketEvent::Error(
                        super::WebSocketError::ReceiveError,
                    )),
                };
                if let Err(err) = result {
                    log::error!("{}", err);
                }
            }) as Box<dyn FnMut(MessageEvent)>)
        };
        inner
            .add_event_listener_with_callback(
                "message",
                on_message_callback.as_ref().unchecked_ref(),
            )
            .unwrap();

        let on_open_callback = {
            let mut on_open_sender = on_open_sender.clone();
            Closure::wrap(Box::new(move |_| {
                if let Err(_) = on_open_sender.try_send(Ok(())) {
                    log::error!("Failed to send WebSocket open event notification");
                }
            }) as Box<dyn FnMut(JsValue)>)
        };
        inner
            .add_event_listener_with_callback("open", on_open_callback.as_ref().unchecked_ref())
            .unwrap();

        let on_error_callback = {
            Closure::wrap(Box::new(move |_error_event| {
                if let Err(err) = on_open_sender.try_send(Err(super::WebSocketError::CreationError))
                {
                    log::error!("{}", err)
                }
            }) as Box<dyn FnMut(ErrorEvent)>)
        };
        inner
            .add_event_listener_with_callback("error", on_error_callback.as_ref().unchecked_ref())
            .unwrap();

        let on_close_callback = {
            let queue = sx.clone();
            Closure::wrap(Box::new(move |close_event: CloseEvent| {
                if let Err(e) = queue.send(super::WebSocketEvent::Close(close_event.code().into()))
                {
                    log::error!("{}", e)
                }
            }) as Box<dyn FnMut(CloseEvent)>)
        };
        inner
            .add_event_listener_with_callback("close", on_close_callback.as_ref().unchecked_ref())
            .unwrap();

        WebSocket {
            inner,
            event_queue: rx,
            on_message_callback,
            on_open_callback,
            on_error_callback,
            on_close_callback,
            on_open_notification: on_open_recver,
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub enum ConnectionFuture {
    Error(futures::future::Ready<super::WebSocketError>),
    Connecting(Option<WebSocket>),
}

impl futures::future::Future for ConnectionFuture {
    type Output = Result<WebSocket, super::WebSocketError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        match &mut *self {
            ConnectionFuture::Error(err) => err.poll_unpin(cx).map(|err| Err(err)),
            ConnectionFuture::Connecting(maybe_ws) => {
                if let Some(ws) = maybe_ws {
                    if let std::task::Poll::Ready(result) =
                        ws.on_open_notification.next().poll_unpin(cx)
                    {
                        match result {
                            Some(Ok(_)) => std::task::Poll::Ready(Ok(maybe_ws.take().unwrap())),
                            _ => std::task::Poll::Ready(Err(super::WebSocketError::CreationError)),
                        }
                    } else {
                        std::task::Poll::Pending
                    }
                } else {
                    std::task::Poll::Ready(Err(super::WebSocketError::CreationError))
                }
            }
        }
    }
}

impl futures::future::FusedFuture for ConnectionFuture {
    fn is_terminated(&self) -> bool {
        match self {
            ConnectionFuture::Error(inner) => inner.is_terminated(),
            ConnectionFuture::Connecting(inner) => inner.is_some(),
        }
    }
}

impl From<u16> for super::CloseCode {
    fn from(code: u16) -> super::CloseCode {
        match code {
            1000 => super::CloseCode::Normal,
            1001 => super::CloseCode::Away,
            1002 => super::CloseCode::Protocol,
            1003 => super::CloseCode::Unsupported,
            1005 => super::CloseCode::Status,
            1006 => super::CloseCode::Abnormal,
            1007 => super::CloseCode::Invalid,
            1008 => super::CloseCode::Policy,
            1009 => super::CloseCode::Size,
            1010 => super::CloseCode::Extension,
            1011 => super::CloseCode::Error,
            1012 => super::CloseCode::Restart,
            1013 => super::CloseCode::Again,
            1015 => super::CloseCode::Tls,
            0 => super::CloseCode::Empty,
            _ => super::CloseCode::Other(code),
        }
    }
}
