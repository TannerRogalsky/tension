use futures::{SinkExt, StreamExt};
use warp::{Filter, Reply};

type ArcRw<T> = std::sync::Arc<tokio::sync::RwLock<T>>;

type CustomMessageType = shared::CustomMessage;
// type CustomMessage = shared::Message<CustomMessageType>;

// type EventSink = futures::channel::mpsc::UnboundedSender<CustomMessage>;

type WsSink = tokio::sync::mpsc::UnboundedSender<warp::ws::Message>;
type PlayerConnections = ArcRw<std::collections::HashMap<shared::PlayerID, WsSink>>;

type State = std::sync::Arc<tokio::sync::RwLock<shared::viewer::state::State<CustomMessageType>>>;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()?;

    // channels HTTP events into the Client's receiver
    // let (event_send, mut event_recv) = futures::channel::mpsc::unbounded();

    // let (from_ws_http, game_in) = crossbeam_channel::unbounded();
    // let (game_out, mut to_websocket) = tokio::sync::mpsc::unbounded_channel();

    // let rng: rand::rngs::StdRng = rand::SeedableRng::from_entropy();
    // let client = CustomClient::new(game_out.into(), game_in.into(), rng);
    // let game = std::sync::Arc::new(tokio::sync::RwLock::new(client));

    // let state = state::State::new(game_out.clone());
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(shared::viewer::state::State::new()));
    let connections = PlayerConnections::default();

    // tokio::spawn({
    //     async move {
    //         // pump the async HTTP/WS messages into the sync game channel
    //         while let Some(msg) = event_recv.next().await {
    //             if let Err(err) = game_out.send(msg) {
    //                 log::error!("{:?}", err);
    //             }
    //         }
    //     }
    // });

    // tokio::spawn({
    //     let state = state.clone();
    //     let connections = connections.clone();
    //     async move {
    //         while let Some(msg) = to_websocket.recv().await {
    //             if let Some(players) = state.players(msg.target()).await {
    //                 match serde_json::to_string(&msg) {
    //                     Ok(msg) => {
    //                         let mut connections = connections.write().await;
    //                         let mut results = connections
    //                             .iter_mut()
    //                             .filter_map(|(player_id, sender)| {
    //                                 if players.contains(player_id) {
    //                                     Some(sender.send(warp::ws::Message::text(msg.clone())))
    //                                 } else {
    //                                     None
    //                                 }
    //                             })
    //                             .collect::<futures::stream::FuturesUnordered<_>>();
    //                         for result in results.next().await {
    //                             if let Err(err) = result {
    //                                 log::error!("{}", err);
    //                             }
    //                         }
    //                     }
    //                     Err(err) => {
    //                         log::error!("{}", err)
    //                     }
    //                 }
    //             } else {
    //                 log::error!(
    //                     "Tried to send message to non-existent room: {}",
    //                     msg.target()
    //                 );
    //             }
    //         }
    //     }
    // });

    // event_recv + [ws_recvs, ...] -> game_recv
    // game_send -> predicate -> [ws_sends, ...]

    let client_state = warp::any().map(move || state.clone());
    // let event_send = warp::any().map(move || event_send.clone());
    let connections = warp::any().map(move || connections.clone());
    let player_id_cookie = warp::cookie::cookie("game-player-id");

    let ws = warp::path(shared::ENDPOINT_WS)
        .and(warp::ws())
        .and(player_id_cookie)
        // .and(event_send.clone())
        .and(connections.clone())
        .and(client_state.clone())
        .map(
            |ws: warp::ws::Ws,
             id: String,
             // event_sink: EventSink,
             connections: PlayerConnections,
             state: State| {
                use warp::Reply;
                match std::str::FromStr::from_str(&id) {
                    Ok(id) => ws
                        .on_upgrade(move |websocket| {
                            on_ws_connect(websocket, id, connections, state)
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
        .and(connections.clone())
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(create_room);

    let join_room = warp::path(shared::ENDPOINT_JOIN_ROOM)
        .and(warp::post())
        .and(player_id_cookie)
        .and(client_state.clone())
        .and(connections.clone())
        .and(warp::body::content_length_limit(1024 * 16))
        .and(warp::body::json())
        .and_then(join_room);

    let debug_state = warp::path("debug")
        .and(client_state.clone())
        .and_then(debug_state);

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::PathBuf::from)
        .ok_or(eyre::Error::msg("there's no father to his style"))?;

    let routes = ws
        .or(create_room)
        .or(join_room)
        .or(debug_state)
        .or(warp::fs::dir(root.join("docs")));

    Ok(warp::serve(routes).run(([127, 0, 0, 1], 8000)).await)
}

async fn on_ws_connect(
    ws: warp::ws::WebSocket,
    id: shared::PlayerID,
    // mut event_sink: EventSink,
    connections: PlayerConnections,
    state: State,
) {
    log::debug!("New WS connection for User {:?}", id);
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();

    let (sx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    tokio::task::spawn(async move {
        while let Some(msg) = rx.next().await {
            if let Err(err) = user_ws_tx.send(msg).await {
                eprintln!("websocket send error: {}", err);
            }
        }
    });

    connections.write().await.insert(id, sx);

    while let Some(result) = user_ws_rx.next().await {
        match result {
            Ok(msg) => {
                let parse_attempt: Result<shared::viewer::Command<CustomMessageType>, _> =
                    if let Ok(text) = msg.to_str() {
                        serde_json::from_str(text)
                    } else if msg.is_binary() {
                        serde_json::from_slice(msg.as_bytes())
                    } else {
                        continue;
                    };

                match parse_attempt {
                    Ok(cmd) => {
                        state.write().await.handle_command(cmd, &id);
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

    state.write().await.unregister_user(id);
    if let None = connections.write().await.remove(&id) {
        log::warn!("Attempted to remove player connection that was not present.");
    } else {
        log::debug!("Ended WS connection for User {:?}", id);
    }
}

async fn ws_forward(
    player_id: shared::PlayerID,
    channel: tokio::sync::broadcast::Receiver<shared::viewer::StateChange<shared::CustomMessage>>,
    connections: PlayerConnections,
) {
    let mut channel = tokio_stream::wrappers::BroadcastStream::new(channel);
    while let Some(Ok(msg)) = channel.next().await {
        match serde_json::to_string(&msg) {
            Ok(msg) => {
                let mut connections = connections.write().await;
                // this unwrap will panic if the player has dropped which is a convenient but probably bad
                // way to exit this loop
                let socket = connections.get_mut(&player_id).unwrap();
                if let Err(err) = socket.send(warp::ws::Message::text(msg)) {
                    log::error!("{}", err);
                }
            }
            Err(err) => {
                log::error!("{}", err);
            }
        }
    }
}

async fn create_room(
    player_id: String,
    state: State,
    connections: PlayerConnections,
    player_name: shared::PlayerName,
) -> Result<impl warp::Reply, warp::Rejection> {
    if let Ok(player_id) = std::str::FromStr::from_str(&player_id) {
        let mut state = state.write().await;
        let user = shared::viewer::User {
            id: player_id,
            name: player_name,
        };
        state.register_user(user.clone());
        let room_id = state.create_room();
        state.join(room_id, player_id);
        let (room_state, channel) = state.subscribe(room_id).unwrap();
        drop(state);

        tokio::spawn(ws_forward(player_id, channel, connections));
        Ok(warp::reply::json(&room_state))
    } else {
        Err(warp::reject())
    }
}

async fn join_room(
    player_id: String,
    state: State,
    connections: PlayerConnections,
    join_info: shared::RoomJoinInfo,
) -> Result<impl warp::Reply, std::convert::Infallible> {
    let room_id = std::convert::TryInto::<shared::RoomID>::try_into(join_info.room_id).ok();
    let player_id = std::str::FromStr::from_str(&player_id).ok();
    let result = match room_id.zip(player_id) {
        Some((room_id, player_id)) => {
            let mut state = state.write().await;
            let user = shared::viewer::User {
                id: player_id,
                name: join_info.player_name,
            };
            state.register_user(user.clone());
            state.join(room_id, player_id);
            let (room_state, channel) = state.subscribe(room_id).unwrap();
            drop(state);

            tokio::spawn(ws_forward(player_id, channel, connections));
            warp::reply::json(&room_state).into_response()
        }
        None => warp::reply::with_status(
            "could not parse room id",
            warp::hyper::StatusCode::BAD_REQUEST,
        )
        .into_response(),
    };
    Ok(result)
}

async fn debug_state(state: State) -> Result<impl warp::Reply, std::convert::Infallible> {
    let state = state.read().await;
    let state = state
        .rooms
        .values()
        .map(|room| {
            let users = room
                .state
                .users
                .iter()
                .filter_map(|user_id| state.users.get(user_id).cloned())
                .collect::<Vec<_>>();
            shared::viewer::InitialRoomState {
                id: room.state.id,
                users,
            }
        })
        .collect::<Vec<_>>();

    Ok(warp::reply::json(&state))
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
