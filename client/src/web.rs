use futures::{FutureExt, TryFutureExt};
use wasm_bindgen::prelude::*;

fn to_js<E: std::fmt::Display>(v: E) -> JsValue {
    JsValue::from_str(&format!("{}", v))
}

#[wasm_bindgen(start)]
pub fn js_main() {
    // #[cfg(debug_assertions)]
    let level = log::Level::Debug;
    // #[cfg(not(debug_assertions))]
    // let level = log::Level::Error;
    wasm_logger::init(wasm_logger::Config::new(level));
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
}

#[wasm_bindgen(js_name = Resources)]
pub struct ResourcesWrapper {
    sans_font_data: Option<Vec<u8>>,
}

#[wasm_bindgen(js_class = Resources)]
impl ResourcesWrapper {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            sans_font_data: None,
        }
    }

    pub fn set_sans_font_data(&mut self, data: Vec<u8>) {
        self.sans_font_data = Some(data);
    }
}

#[wasm_bindgen(js_name = Tension)]
pub struct GameWrapper {
    inner: super::Game,
}

#[wasm_bindgen(js_class = Tension)]
impl GameWrapper {
    #[wasm_bindgen(constructor)]
    pub fn new(
        canvas: web_sys::HtmlCanvasElement,
        time_ms: f64,
        network: NetworkWrapper,
        resources: ResourcesWrapper,
    ) -> Result<GameWrapper, JsValue> {
        let webgl_context = {
            use wasm_bindgen::JsCast;
            canvas
                .get_context("webgl")
                .unwrap()
                .unwrap()
                .dyn_into::<web_sys::WebGlRenderingContext>()
                .unwrap()
        };
        let ctx = solstice_2d::solstice::glow::Context::from_webgl1_context(webgl_context);
        let ctx = solstice_2d::solstice::Context::new(ctx);

        let resources = crate::resources::Resources {
            sans_font_data: resources
                .sans_font_data
                .ok_or(JsValue::from_str("missing debug font data"))?,
        };

        let width = canvas.width();
        let height = canvas.height();

        let time = duration_from_f64(time_ms);
        let inner = super::Game::new(ctx, time, width as _, height as _, network.inner, resources)
            .map_err(to_js)?;
        Ok(Self { inner })
    }

    pub fn step(&mut self, time_ms: f64) {
        self.inner.update(duration_from_f64(time_ms));
    }

    pub fn handle_mouse_down(&mut self, is_left_button: bool) {
        let state = winit::event::ElementState::Pressed;
        let button = match is_left_button {
            true => winit::event::MouseButton::Left,
            false => winit::event::MouseButton::Right,
        };
        let event = crate::MouseEvent::Button(state, button);
        self.inner.handle_mouse_event(event);
    }

    pub fn handle_mouse_up(&mut self, is_left_button: bool) {
        let state = winit::event::ElementState::Released;
        let button = match is_left_button {
            true => winit::event::MouseButton::Left,
            false => winit::event::MouseButton::Right,
        };
        let event = crate::MouseEvent::Button(state, button);
        self.inner.handle_mouse_event(event);
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        let event = crate::MouseEvent::Moved(x, y);
        self.inner.handle_mouse_event(event);
    }

    pub fn handle_room_state(&mut self, state: RoomStateWrapper) {
        self.inner
            .handle_new_room_state(state.room, state.local_user)
    }
}

#[wasm_bindgen(js_name = Network)]
pub struct NetworkWrapper {
    inner: super::net::Client,
}

#[wasm_bindgen(js_class = Network)]
impl NetworkWrapper {
    pub async fn connect(base_url: String) -> Result<NetworkWrapper, JsValue> {
        match super::net::Client::new(base_url).await {
            Ok(inner) => Ok(Self { inner }),
            Err(err) => Err(to_js(err)),
        }
    }

    pub fn create_room(
        &self,
        player_id: String,
        player_name: shared::PlayerName,
    ) -> Result<FutureWrapper, JsValue> {
        let player_id = std::str::FromStr::from_str(&player_id).map_err(to_js)?;
        self.inner
            .create_room(&player_name)
            .map_err(to_js)
            .map(|fut| FutureWrapper {
                fut: fut.boxed_local(),
                local_user: shared::viewer::User {
                    id: player_id,
                    name: player_name,
                },
            })
    }

    pub fn join_room(
        &self,
        player_id: String,
        player_name: shared::PlayerName,
        room_id: String,
    ) -> Result<FutureWrapper, JsValue> {
        let player_id = std::str::FromStr::from_str(&player_id).map_err(to_js)?;
        let room_id = std::str::FromStr::from_str(&room_id).map_err(to_js)?;
        let join_info = shared::RoomJoinInfo {
            room_id,
            player_name: player_name.clone(),
        };
        self.inner
            .join_room(&join_info)
            .map_err(to_js)
            .map(|fut| FutureWrapper {
                fut: fut.boxed_local(),
                local_user: shared::viewer::User {
                    id: player_id,
                    name: player_name,
                },
            })
    }
}

#[wasm_bindgen]
pub struct FutureWrapper {
    fut: futures::future::LocalBoxFuture<'static, eyre::Result<shared::viewer::InitialRoomState>>,
    local_user: shared::viewer::User,
}

#[wasm_bindgen]
impl FutureWrapper {
    #[wasm_bindgen(js_name = "await")]
    pub async fn process(self) -> Result<RoomStateWrapper, JsValue> {
        let local_user = self.local_user;
        self.fut
            .map_ok(move |room| RoomStateWrapper { room, local_user })
            .map_err(to_js)
            .await
    }
}

#[wasm_bindgen(js_name = RoomState)]
pub struct RoomStateWrapper {
    room: shared::viewer::InitialRoomState,
    local_user: shared::viewer::User,
}

fn duration_from_f64(millis: f64) -> std::time::Duration {
    std::time::Duration::from_millis(millis.trunc() as u64)
        + std::time::Duration::from_nanos((millis.fract() * 1.0e6) as u64)
}
