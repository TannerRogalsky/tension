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

#[wasm_bindgen(js_name = Tension)]
pub struct GameWrapper {
    inner: super::Game,
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

    pub fn create_room(&self, player: shared::PlayerName) -> Result<FutureWrapper, JsValue> {
        self.inner
            .create_room(player)
            .map_err(to_js)
            .map(|fut| FutureWrapper(fut.boxed_local()))
    }

    pub fn join_room(
        &self,
        player: shared::PlayerName,
        room_id: String,
    ) -> Result<FutureWrapper, JsValue> {
        let room_id = std::str::FromStr::from_str(&room_id).map_err(to_js)?;
        let join_info = shared::RoomJoinInfo {
            room_id,
            player_name: player,
        };
        self.inner
            .join_room(&join_info)
            .map_err(to_js)
            .map(|fut| FutureWrapper(fut.boxed_local()))
    }

    pub fn use_initial_state(&mut self, room_state: RoomStateWrapper) {
        self.inner.handle_new_room_state(room_state.0);
        self.inner.update();
    }

    pub fn debug_state(&mut self) {
        log::debug!("{:#?}", self.inner.view());
        for room in self.inner.view().rooms.keys() {
            log::debug!("{}", room);
        }
    }
}

#[wasm_bindgen]
pub struct FutureWrapper(
    futures::future::LocalBoxFuture<'static, eyre::Result<shared::viewer::RoomState>>,
);

#[wasm_bindgen]
impl FutureWrapper {
    #[wasm_bindgen(js_name = "await")]
    pub async fn process(self) -> Result<RoomStateWrapper, JsValue> {
        self.0
            .map_ok(|room| RoomStateWrapper(room))
            .map_err(to_js)
            .await
    }
}

#[wasm_bindgen(js_name = RoomState)]
pub struct RoomStateWrapper(shared::viewer::RoomState);
