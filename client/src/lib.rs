mod states;
#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use glutin as winit;
#[cfg(target_arch = "wasm32")]
pub use winit;

use winit::event::{ElementState, MouseButton};

pub enum MouseEvent {
    Button(ElementState, MouseButton),
    Moved(f32, f32),
}

pub struct Game {
    ctx: solstice_2d::solstice::Context,
    gfx: solstice_2d::Graphics,
    time: std::time::Duration,
    input_state: InputState,
    state: Option<states::State>,
}

impl Game {
    pub fn new(
        mut ctx: solstice_2d::solstice::Context,
        time: std::time::Duration,
        width: f32,
        height: f32,
    ) -> eyre::Result<Self> {
        let gfx = solstice_2d::Graphics::new(&mut ctx, width, height)?;

        Ok(Self {
            ctx,
            gfx,
            time,
            input_state: Default::default(),
            state: Default::default(),
        })
    }

    pub fn update(&mut self, time: std::time::Duration) {
        let dt = time - self.time;
        self.time = time;

        self.state = self.state.take().map(|state| {
            state.update(
                dt,
                states::StateContext {
                    g: self.gfx.lock(&mut self.ctx),
                    input_state: &self.input_state,
                    time: &self.time,
                },
            )
        });
        self.state
            .get_or_insert_with(Default::default)
            .render(states::StateContext {
                g: self.gfx.lock(&mut self.ctx),
                input_state: &self.input_state,
                time: &self.time,
            });
    }

    pub fn handle_mouse_event(&mut self, event: MouseEvent) {
        match event {
            MouseEvent::Moved(x, y) => {
                let mut is = &mut self.input_state;
                if is.mouse_position == is.prev_mouse_position && is.mouse_position == (0., 0.) {
                    is.prev_mouse_position = (x, y);
                    is.mouse_position = (x, y);
                } else {
                    is.prev_mouse_position = is.mouse_position;
                    is.mouse_position = (x, y);
                }
            }
            _ => {}
        }
        self.state = self.state.take().map(|state| {
            state.handle_mouse_event(
                event,
                states::StateContext {
                    g: self.gfx.lock(&mut self.ctx),
                    input_state: &self.input_state,
                    time: &self.time,
                },
            )
        });
    }

    pub fn handle_resize(&mut self, win_width: f32, win_height: f32) {
        use solstice_2d::solstice::viewport::Viewport;
        let vw = Viewport::new(0, 0, win_width as _, win_height as _);
        self.ctx.set_viewport(0, 0, win_width as _, win_height as _);
        self.gfx.set_viewport(vw);

        let width = 16. / 9.;
        let height = 1.;

        let scale_x = win_width / width;
        let scale_y = win_height / height;
        let scale = scale_x.min(scale_y);

        let x = (win_width - width * scale) / 2.;
        let y = (win_height - height * scale) / 2.;

        let scissor = Viewport::new(x as _, y as _, (width * scale) as _, (height * scale) as _);
        self.gfx.set_scissor(Some(scissor));
    }
}

#[derive(Default)]
pub struct InputState {
    prev_mouse_position: (f32, f32),
    mouse_position: (f32, f32),
}

struct RepeatingTimer {
    time: std::time::Duration,
    elapsed: std::time::Duration,
}

impl RepeatingTimer {
    pub fn new(time: std::time::Duration) -> Self {
        Self {
            time,
            elapsed: Default::default(),
        }
    }

    pub fn update(&mut self, dt: std::time::Duration) -> bool {
        self.elapsed += dt;
        if self.elapsed >= self.time {
            self.elapsed -= self.time;
            true
        } else {
            false
        }
    }
}

pub mod net {
    use futures::{Future, FutureExt, TryFutureExt};
    use shared::CustomMessage;

    // could guard against polling the websocket buffer while a create/join request is in flight
    pub struct Client {
        base_url: reqwest::Url,
        sx: websocket::WsSend,
        rx: websocket::WsRecv,
        state: State,
    }

    impl Client {
        pub async fn new(base_url: String) -> eyre::Result<Self> {
            let base_url = reqwest::Url::parse(&base_url)?;
            let mut ws_url = base_url.clone();
            match base_url.scheme() {
                "http" => {
                    ws_url.set_scheme("ws").expect("set scheme failure");
                }
                "https" => {
                    ws_url.set_scheme("wss").expect("set scheme failure");
                }
                _ => {
                    return Err(eyre::Report::msg(format!(
                        "Unrecognized scheme {}",
                        base_url.scheme()
                    )));
                }
            }
            let ws_url = ws_url.join(shared::ENDPOINT_WS)?;
            let ws = websocket::WebSocket::connect(ws_url.as_str()).await?;
            let (sx, rx) = ws.into_channels();
            Ok(Self {
                base_url,
                sx,
                rx,
                state: Default::default(),
            })
        }

        pub fn view(&self) -> &State {
            &self.state
        }

        pub fn handle_new_room_state(&mut self, room_state: shared::viewer::RoomState) {
            self.state.rooms.insert(
                room_state.id,
                Room {
                    net_state: room_state,
                    game_state: GameState::Lobby,
                },
            );
        }

        pub fn send(&self, cmd: shared::viewer::Command<shared::CustomMessage>) {
            match serde_json::to_string(&cmd) {
                Ok(payload) => {
                    if let Err(err) = self.sx.send(websocket::Message::Text(payload)) {
                        log::error!("{}", err);
                    }
                }
                Err(err) => {
                    log::error!("{}", err);
                }
            }
        }

        pub fn update(&mut self) {
            use shared::viewer::StateChange;
            while let Ok(msg) = self.rx.try_recv() {
                let parsed_msg = match msg {
                    websocket::Message::Text(text) => {
                        serde_json::from_str::<StateChange<shared::CustomMessage>>(&text)
                    }
                    websocket::Message::Binary(bin) => {
                        serde_json::from_slice::<StateChange<shared::CustomMessage>>(&bin)
                    }
                };
                match parsed_msg {
                    Ok(msg) => {
                        self.state.handle_msg(msg);
                    }
                    Err(err) => {
                        log::error!("{}", err);
                    }
                }
            }
        }

        pub fn create_room(
            &self,
            player: shared::PlayerName,
        ) -> eyre::Result<impl Future<Output = eyre::Result<shared::viewer::RoomState>>> {
            let body = serde_json::to_string(&player)?;
            let url = self.base_url.join(shared::ENDPOINT_CREATE_ROOM)?;

            let client = reqwest::Client::new();
            Ok(client
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .map_err(eyre::Report::from)
                .and_then(|response| response.text().map_err(eyre::Report::from))
                .map(|result: eyre::Result<String>| {
                    result.and_then(|text| serde_json::from_str(&text).map_err(eyre::Report::from))
                }))
        }

        pub fn join_room(
            &self,
            join_info: &shared::RoomJoinInfo,
        ) -> eyre::Result<impl Future<Output = eyre::Result<shared::viewer::RoomState>>> {
            let body = serde_json::to_string(&join_info)?;
            let url = self.base_url.join(shared::ENDPOINT_JOIN_ROOM)?;

            let client = reqwest::Client::new();
            Ok(client
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .map_err(eyre::Report::from)
                .and_then(|response| response.text().map_err(eyre::Report::from))
                .map(|result: eyre::Result<String>| {
                    result.and_then(|text| serde_json::from_str(&text).map_err(eyre::Report::from))
                }))
        }
    }

    #[derive(Debug, Clone, Eq, PartialEq)]
    pub enum GameState {
        Lobby,
        Game,
    }

    #[derive(Debug)]
    pub struct Room {
        pub net_state: shared::viewer::RoomState,
        pub game_state: GameState,
    }

    #[derive(Debug)]
    pub struct Player {
        pub inner: shared::viewer::User,
    }

    #[derive(Debug, Default)]
    pub struct State {
        pub rooms: std::collections::HashMap<shared::RoomID, Room>,
        pub users: std::collections::HashMap<shared::PlayerID, Player>,
    }

    impl State {
        pub fn handle_msg(&mut self, msg: shared::viewer::StateChange<shared::CustomMessage>) {
            if let Some(room) = self.rooms.get_mut(&msg.target) {
                use shared::viewer::ChangeType;
                match msg.ty {
                    ChangeType::UserJoin(user) => {
                        if room.game_state == GameState::Lobby {
                            room.net_state.users.push(user.id);
                            self.users.insert(user.id, Player { inner: user });
                        } else {
                            log::warn!("Tried to add user {:?} while not in lobby.", user);
                        }
                    }
                    ChangeType::UserLeave(user_id) => {
                        room.net_state.users.retain(|user| user != &user_id);
                        self.users.remove(&user_id);
                    }
                    ChangeType::Custom(payload) => match payload {
                        CustomMessage::StartGame => {
                            room.game_state = GameState::Game;
                        }
                    },
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use shared::{viewer::*, *};
        use std::str::FromStr;

        #[test]
        fn state_test() {
            let room1 = RoomID::from_str("ABCD").unwrap();
            let user1 = User {
                id: PlayerID::from_str("0").unwrap(),
                name: "Alice".to_string(),
            };
            let user2 = User {
                id: PlayerID::from_str("1").unwrap(),
                name: "Bob".to_string(),
            };

            let mut state = State::default();
            state.rooms.insert(
                room1,
                super::Room {
                    net_state: viewer::RoomState {
                        id: room1,
                        users: vec![],
                    },
                    game_state: GameState::Lobby,
                },
            );

            state.handle_msg(StateChange {
                target: room1,
                ty: ChangeType::UserJoin(user1.clone()),
            });
            state.handle_msg(StateChange {
                target: room1,
                ty: ChangeType::UserJoin(user2.clone()),
            });

            state.handle_msg(StateChange {
                target: room1,
                ty: ChangeType::Custom(CustomMessage::StartGame),
            });

            let room = state.rooms.get(&room1).unwrap();
            assert_eq!(room.game_state, GameState::Game);
            assert_eq!(room.net_state.users.len(), 2);
            assert!(room.net_state.users.contains(&user1.id));
            assert!(room.net_state.users.contains(&user2.id));
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
