pub mod resources;
pub mod sim;
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

impl MouseEvent {
    pub fn is_left_click(&self) -> bool {
        match self {
            Self::Button(ElementState::Pressed, MouseButton::Left) => true,
            _ => false,
        }
    }
}

pub struct Game {
    ctx: solstice_2d::solstice::Context,
    gfx: solstice_2d::Graphics,
    time: std::time::Duration,
    input_state: InputState,
    ws: net::Client,
    resources: resources::LoadedResources,
    state: Option<states::State>,
}

impl Game {
    pub fn new(
        mut ctx: solstice_2d::solstice::Context,
        time: std::time::Duration,
        width: f32,
        height: f32,
        ws: net::Client,
        resources: resources::Resources,
    ) -> eyre::Result<Self> {
        let mut gfx = solstice_2d::Graphics::new(&mut ctx, width, height)?;
        let resources = resources.try_into_loaded(&mut ctx, &mut gfx)?;

        Ok(Self {
            ctx,
            gfx,
            time,
            input_state: Default::default(),
            ws,
            resources,
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
                    resources: &self.resources,
                    ws: &self.ws,
                    input_state: &self.input_state,
                    time: &self.time,
                },
            )
        });
        self.state
            .get_or_insert_with(Default::default)
            .render(states::StateContext {
                g: self.gfx.lock(&mut self.ctx),
                resources: &self.resources,
                ws: &self.ws,
                input_state: &self.input_state,
                time: &self.time,
            });
    }

    pub fn handle_new_room_state(
        &mut self,
        room: shared::viewer::InitialRoomState,
        local_user: shared::viewer::User,
    ) {
        self.state = Some(states::State::lobby(local_user, room))
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
                    resources: &self.resources,
                    ws: &self.ws,
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

        // let width = 16. / 9.;
        // let height = 1.;
        //
        // let scale_x = win_width / width;
        // let scale_y = win_height / height;
        // let scale = scale_x.min(scale_y);
        //
        // let x = (win_width - width * scale) / 2.;
        // let y = (win_height - height * scale) / 2.;
        //
        // let scissor = Viewport::new(x as _, y as _, (width * scale) as _, (height * scale) as _);
        // self.gfx.set_scissor(Some(scissor));
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

    // could guard against polling the websocket buffer while a create/join request is in flight
    pub struct Client {
        base_url: reqwest::Url,
        sx: websocket::WsSend,
        rx: websocket::WsRecv,
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
            Ok(Self { base_url, sx, rx })
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

        pub fn try_recv_iter(
            &self,
        ) -> impl Iterator<Item = shared::viewer::StateChange<shared::CustomMessage>> + '_ {
            std::iter::from_fn(move || {
                while let Ok(msg) = self.rx.try_recv() {
                    let parsed = match msg {
                        websocket::Message::Text(text) => serde_json::from_str(&text),
                        websocket::Message::Binary(bin) => serde_json::from_slice(&bin),
                    };

                    if let Ok(cmd) = parsed {
                        return Some(cmd);
                    } else {
                        continue;
                    }
                }

                None
            })
        }

        pub fn create_room(
            &self,
            player: &shared::PlayerName,
        ) -> eyre::Result<impl Future<Output = eyre::Result<shared::viewer::InitialRoomState>>>
        {
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
        ) -> eyre::Result<impl Future<Output = eyre::Result<shared::viewer::InitialRoomState>>>
        {
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
}

fn collides(p: [f32; 2], rect: &solstice_2d::Rectangle) -> bool {
    type Point = [f32; 2];
    fn vec(a: Point, b: Point) -> Point {
        [b[0] - a[0], b[1] - a[1]]
    }

    fn dot(u: Point, v: Point) -> f32 {
        u[0] * v[0] + u[1] * v[1]
    }

    let rect = [
        [rect.x, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
        [rect.x + rect.width, rect.y],
    ];

    let ab = vec(rect[0], rect[1]);
    let am = vec(rect[0], p);
    let bc = vec(rect[1], rect[2]);
    let bm = vec(rect[1], p);

    let dot_abam = dot(ab, am);
    let dot_abab = dot(ab, ab);
    let dot_bcbm = dot(bc, bm);
    let dot_bcbc = dot(bc, bc);

    0. <= dot_abam && dot_abam <= dot_abab && 0. <= dot_bcbm && dot_bcbm <= dot_bcbc
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
