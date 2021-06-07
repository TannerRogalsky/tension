use futures::TryFutureExt;
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

    pub fn create_room(&self, player: shared::PlayerName) -> Result<js_sys::Promise, JsValue> {
        let future = self
            .inner
            .create_room(player)
            .map_err(to_js)?
            .map_ok(|room_id| JsValue::from_str(&room_id.to_string()))
            .map_err(to_js);
        Ok(wasm_bindgen_futures::future_to_promise(future))
    }

    pub fn join_room(
        &self,
        player: shared::PlayerName,
        room_id: String,
    ) -> Result<js_sys::Promise, JsValue> {
        let room_id = std::str::FromStr::from_str(&room_id).map_err(to_js)?;
        let join_info = shared::RoomJoinInfo {
            room_id,
            player_name: player,
        };
        let future = self
            .inner
            .join_room(&join_info)
            .map_err(to_js)?
            .map_ok(|room_id| JsValue::from_str(&room_id.to_string()))
            .map_err(to_js);
        Ok(wasm_bindgen_futures::future_to_promise(future))
    }

    pub fn players(&mut self, room_id: String) -> Option<js_sys::Array> {
        self.inner.inner.update();
        let room_id = std::str::FromStr::from_str(&room_id).ok()?;
        self.inner.inner.get_room(&room_id).map(|room| {
            room.players
                .iter()
                .map(|player| {
                    log::debug!("{}", player.name);
                    JsValue::from_str(&player.name)
                })
                .collect()
        })
    }
}
