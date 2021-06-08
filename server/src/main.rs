mod state;

use futures::{SinkExt, StreamExt};
use warp::{Filter, Reply};

type ArcRw<T> = std::sync::Arc<tokio::sync::RwLock<T>>;

type CustomMessageType = ();
type CustomMessage = shared::Message<CustomMessageType>;

type EventSink = futures::channel::mpsc::UnboundedSender<CustomMessage>;

type WsSink = futures::stream::SplitSink<warp::ws::WebSocket, warp::ws::Message>;
type PlayerConnections = ArcRw<std::collections::HashMap<shared::PlayerID, WsSink>>;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()?;

    // channels HTTP events into the Client's receiver
    let (event_send, mut event_recv) = futures::channel::mpsc::unbounded();

    // let (from_ws_http, game_in) = crossbeam_channel::unbounded();
    let (game_out, mut to_websocket) = tokio::sync::mpsc::unbounded_channel();

    // let rng: rand::rngs::StdRng = rand::SeedableRng::from_entropy();
    // let client = CustomClient::new(game_out.into(), game_in.into(), rng);
    // let game = std::sync::Arc::new(tokio::sync::RwLock::new(client));

    let state = state::State::new(game_out.clone());
    let connections = PlayerConnections::default();

    tokio::spawn({
        async move {
            // pump the async HTTP/WS messages into the sync game channel
            while let Some(msg) = event_recv.next().await {
                if let Err(err) = game_out.send(msg) {
                    log::error!("{:?}", err);
                }
            }
        }
    });

    tokio::spawn({
        let state = state.clone();
        let connections = connections.clone();
        async move {
            while let Some(msg) = to_websocket.recv().await {
                if let Some(players) = state.players(msg.target()).await {
                    match serde_json::to_string(&msg) {
                        Ok(msg) => {
                            let mut connections = connections.write().await;
                            let mut results = connections
                                .iter_mut()
                                .filter_map(|(player_id, sender)| {
                                    if players.contains(player_id) {
                                        Some(sender.send(warp::ws::Message::text(msg.clone())))
                                    } else {
                                        None
                                    }
                                })
                                .collect::<futures::stream::FuturesUnordered<_>>();
                            for result in results.next().await {
                                if let Err(err) = result {
                                    log::error!("{}", err);
                                }
                            }
                        }
                        Err(err) => {
                            log::error!("{}", err)
                        }
                    }
                } else {
                    log::error!(
                        "Tried to send message to non-existent room: {}",
                        msg.target()
                    );
                }
            }
        }
    });

    // event_recv + [ws_recvs, ...] -> game_recv
    // game_send -> predicate -> [ws_sends, ...]

    let client_state = warp::any().map(move || state.clone());
    let event_send = warp::any().map(move || event_send.clone());
    let connections = warp::any().map(move || connections.clone());
    let player_id_cookie = warp::cookie::cookie("game-player-id");

    let ws = warp::path(shared::ENDPOINT_WS)
        .and(warp::ws())
        .and(player_id_cookie)
        .and(event_send.clone())
        .and(connections)
        .and(client_state.clone())
        .map(
            |ws: warp::ws::Ws,
             id: String,
             event_sink: EventSink,
             connections: PlayerConnections,
             state: state::State| {
                use warp::Reply;
                match std::str::FromStr::from_str(&id) {
                    Ok(id) => ws
                        .on_upgrade(move |websocket| {
                            on_ws_connect(websocket, id, event_sink, connections, state)
                        })
                        .into_response(),
                    Err(_err) => {
                        warp::reply::with_status("Invalid ID", warp::hyper::StatusCode::BAD_REQUEST)
                            .into_response()
                    }
                }
            },
        );

    let create_room = warp::path(shared::ENDPOINT_CREATE_ROOM)
        .and(warp::post())
        .and(player_id_cookie)
        .and(client_state.clone())
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(create_room);

    let join_room = warp::path(shared::ENDPOINT_JOIN_ROOM)
        .and(warp::post())
        .and(player_id_cookie)
        .and(client_state.clone())
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(join_room);

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::PathBuf::from)
        .ok_or(eyre::Error::msg("there's no father to his style"))?;

    let routes = ws
        .or(create_room)
        .or(join_room)
        .or(warp::fs::dir(root.join("docs")));

    Ok(warp::serve(routes).run(([127, 0, 0, 1], 8000)).await)
}

async fn on_ws_connect(
    ws: warp::ws::WebSocket,
    id: shared::PlayerID,
    mut event_sink: EventSink,
    connections: PlayerConnections,
    mut state: state::State,
) {
    log::debug!("new ws connection");
    let (sx, mut rx) = ws.split();

    connections.write().await.insert(id, sx);

    while let Some(result) = rx.next().await {
        match result {
            Ok(msg) => {
                let parse_attempt: Result<CustomMessage, _> = if let Ok(text) = msg.to_str() {
                    serde_json::from_str(text)
                } else if msg.is_binary() {
                    serde_json::from_slice(msg.as_bytes())
                } else {
                    continue;
                };

                match parse_attempt {
                    Ok(msg) => {
                        if let Err(err) = event_sink.send(msg).await {
                            log::error!("{}", err);
                        }
                    }
                    Err(err) => {
                        log::error!("{:?}", err);
                    }
                }
            }
            Err(err) => {
                log::error!("Websocket Recv Error: {}", err);
            }
        }
    }

    state.disconnect(id).await;
    if let None = connections.write().await.remove(&id) {
        log::warn!("Attempted to remove player connection that was not present.");
    }
}

async fn create_room(
    player_id: String,
    mut state: state::State,
    player: shared::PlayerName,
) -> Result<impl warp::Reply, warp::Rejection> {
    if let Ok(player_id) = std::str::FromStr::from_str(&player_id) {
        let player = shared::Player {
            id: player_id,
            name: player,
        };
        let room_id = state.create_room(player).await;
        Ok(warp::reply::json(&room_id))
    } else {
        Err(warp::reject())
    }
}

async fn join_room(
    player_id: String,
    mut state: state::State,
    join_info: shared::RoomJoinInfo,
) -> Result<impl warp::Reply, std::convert::Infallible> {
    let room_id = std::convert::TryInto::<shared::RoomID>::try_into(join_info.room_id).ok();
    let player_id = std::str::FromStr::from_str(&player_id).ok();
    let result = match room_id.zip(player_id) {
        Some((room_id, player_id)) => {
            let player = shared::Player {
                id: player_id,
                name: join_info.player_name,
            };
            let room = state.join_room(player, room_id).await;
            warp::reply::json(&room).into_response()
        }
        None => warp::reply::with_status(
            "could not parse room id",
            warp::hyper::StatusCode::BAD_REQUEST,
        )
        .into_response(),
    };
    Ok(result)
}

mod net {
    use crossbeam_channel::Receiver;
    use shared::Message;
    use tokio::sync::mpsc::UnboundedSender as Sender;

    #[derive(Debug)]
    pub struct Recv<T>(Receiver<Message<T>>);
    #[derive(Debug)]
    pub struct Send<T>(Sender<Message<T>>);

    impl<T> shared::Receiver<T> for Recv<T> {
        fn try_recv(&self) -> Result<Message<T>, ()> {
            self.0.try_recv().map_err(|_| ())
        }
    }

    impl<T> From<Receiver<Message<T>>> for Recv<T> {
        fn from(inner: Receiver<Message<T>>) -> Self {
            Self(inner)
        }
    }

    impl<T> shared::Sender<T> for Send<T> {
        fn send(&self, msg: Message<T>) -> Result<(), ()> {
            self.0.send(msg).map_err(|_| ())
        }
    }

    impl<T> From<Sender<Message<T>>> for Send<T> {
        fn from(inner: Sender<Message<T>>) -> Self {
            Self(inner)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
