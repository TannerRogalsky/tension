use futures::{SinkExt, StreamExt};
use warp::{Filter, Reply};

type ArcRw<T> = std::sync::Arc<tokio::sync::RwLock<T>>;
type CustomMessageType = shared::CustomMessage;

type WsSink = tokio::sync::mpsc::UnboundedSender<warp::ws::Message>;
type PlayerConnections = ArcRw<std::collections::HashMap<shared::PlayerID, WsSink>>;

type State = std::sync::Arc<tokio::sync::RwLock<shared::viewer::state::State<CustomMessageType>>>;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()?;

    log::debug!("Server version: {}", env!("CARGO_PKG_VERSION"));

    let state = std::sync::Arc::new(tokio::sync::RwLock::new(shared::viewer::state::State::new()));
    let connections = PlayerConnections::default();

    let client_state = warp::any().map(move || state.clone());
    let connections = warp::any().map(move || connections.clone());
    let player_id_cookie = warp::cookie::cookie("game-player-id");

    let ws = warp::path(shared::ENDPOINT_WS)
        .and(warp::ws())
        .and(player_id_cookie)
        .and(connections.clone())
        .and(client_state.clone())
        .map(
            |ws: warp::ws::Ws, id: String, connections: PlayerConnections, state: State| {
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

    let health_check = warp::path("health").map(|| "OK");

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::PathBuf::from)
        .ok_or(eyre::Error::msg("there's no father to his style"))?;

    let api = ws
        .or(create_room)
        .or(join_room)
        .or(debug_state)
        .or(health_check);
    #[cfg(debug_assertions)]
    let api = warp::path("api").and(api);

    let routes = api.or(warp::fs::dir(root.join("docs")));

    Ok(warp::serve(routes).run(([0, 0, 0, 0], 8000)).await)
}

async fn on_ws_connect(
    ws: warp::ws::WebSocket,
    id: shared::PlayerID,
    connections: PlayerConnections,
    state: State,
) {
    log::debug!("New WS connection for User {:?}", id);
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();

    let (sx, rx) = tokio::sync::mpsc::unbounded_channel();
    let rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    tokio::task::spawn(async move {
        let interval = tokio::time::interval(std::time::Duration::from_secs(1));
        let interval = tokio_stream::wrappers::IntervalStream::new(interval);
        let interval = interval.map(|_t| warp::ws::Message::ping(vec![]));

        let mut stream = futures::stream::select(rx, interval);

        while let Some(msg) = stream.next().await {
            if let Err(err) = user_ws_tx.send(msg).await {
                log::error!("websocket send error: {}", err);
                break;
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
    while let Some(msg) = channel.next().await {
        match msg {
            Ok(msg) => match serde_json::to_string(&msg) {
                Ok(msg) => {
                    let mut connections = connections.write().await;
                    if let Some(socket) = connections.get_mut(&player_id) {
                        if let Err(err) = socket.send(warp::ws::Message::text(msg)) {
                            log::error!("{}", err);
                        }
                    } else {
                        log::info!("Connection for {:?} has been dropped.", player_id);
                        break;
                    }
                }
                Err(err) => {
                    log::error!("{}", err);
                }
            },
            Err(err) => {
                log::error!("BROADCAST RECV ERROR: {}", err);
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

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
