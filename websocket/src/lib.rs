#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod websys;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use websys::*;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Error(std::borrow::Cow<'static, str>);

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Message {
    /// A text WebSocket message
    Text(String),
    /// A binary WebSocket message
    Binary(Vec<u8>),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CloseCode {
    /// Indicates a normal closure, meaning that the purpose for
    /// which the connection was established has been fulfilled.
    Normal,
    /// Indicates that an endpoint is "going away", such as a server
    /// going down or a browser having navigated away from a page.
    Away,
    /// Indicates that an endpoint is terminating the connection due
    /// to a protocol error.
    Protocol,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a type of data it cannot accept (e.g., an
    /// endpoint that understands only text data MAY send this if it
    /// receives a binary message).
    Unsupported,
    /// Indicates that no status code was included in a closing frame. This
    /// close code makes it possible to use a single method, `on_close` to
    /// handle even cases where no close code was provided.
    Status,
    /// Indicates an abnormal closure. If the abnormal closure was due to an
    /// error, this close code will not be used. Instead, the `on_error` method
    /// of the handler will be called with the error. However, if the connection
    /// is simply dropped, without an error, this close code will be sent to the
    /// handler.
    Abnormal,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received data within a message that was not
    /// consistent with the type of the message (e.g., non-UTF-8 [RFC3629]
    /// data within a text message).
    Invalid,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that violates its policy.  This
    /// is a generic status code that can be returned when there is no
    /// other more suitable status code (e.g., Unsupported or Size) or if there
    /// is a need to hide specific details about the policy.
    Policy,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that is too big for it to
    /// process.
    Size,
    /// Indicates that an endpoint (client) is terminating the
    /// connection because it has expected the server to negotiate one or
    /// more extension, but the server didn't return them in the response
    /// message of the WebSocket handshake.  The list of extensions that
    /// are needed should be given as the reason for closing.
    /// Note that this status code is not used by the server, because it
    /// can fail the WebSocket handshake instead.
    Extension,
    /// Indicates that a server is terminating the connection because
    /// it encountered an unexpected condition that prevented it from
    /// fulfilling the request.
    Error,
    /// Indicates that the server is restarting. A client may choose to reconnect,
    /// and if it does, it should use a randomized delay of 5-30 seconds between attempts.
    Restart,
    /// Indicates that the server is overloaded and the client should either connect
    /// to a different IP (when multiple targets exist), or reconnect to the same IP
    /// when a user has performed an action.
    Again,
    #[doc(hidden)]
    Tls,
    #[doc(hidden)]
    Empty,
    #[doc(hidden)]
    Other(u16),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum WebSocketEvent {
    Open,
    Message(Message),
    Error(WebSocketError),
    Close(CloseCode),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, thiserror::Error)]
pub enum WebSocketError {
    #[error("could not connect websocket")]
    CreationError,
    #[error("could not send message")]
    SendError,
    #[error("could not receive message")]
    ReceiveError,
}

impl futures::stream::Stream for WebSocket {
    type Item = WebSocketEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        match self.as_mut().poll() {
            None => Poll::Pending,
            Some(WebSocketEvent::Close(_code)) => Poll::Ready(None),
            Some(event) => Poll::Ready(Some(event)),
        }
    }
}

pub struct WsSend {
    socket: std::sync::Arc<WebSocket>,
}

impl WsSend {
    pub fn send(&self, msg: Message) -> Result<(), WebSocketError> {
        self.socket.send(msg)
    }
}

pub struct WsRecv {
    socket: std::sync::Arc<WebSocket>,
}

impl WsRecv {
    pub fn try_recv(&self) -> Result<Message, WebSocketError> {
        if let Some(event) = self.socket.poll() {
            match event {
                WebSocketEvent::Message(msg) => Ok(msg),
                _ => Err(WebSocketError::ReceiveError),
            }
        } else {
            Err(WebSocketError::ReceiveError)
        }
    }
}

impl WebSocket {
    pub fn into_channels(self) -> (WsSend, WsRecv) {
        let socket = std::sync::Arc::new(self);

        let send = WsSend {
            socket: socket.clone(),
        };
        let recv = WsRecv { socket };

        (send, recv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::TryFutureExt;

    #[test]
    fn validate_api() {
        let connection_fut = WebSocket::connect("/test");
        let fut = connection_fut.map_ok(|mut connection| {
            assert!(connection.send(Message::Text("test".to_owned())).is_err());
            assert!(connection.poll().is_none());
            connection
        });
        let result = futures::executor::block_on(fut);
        assert_eq!(result.unwrap_err(), WebSocketError::CreationError);
    }
}
