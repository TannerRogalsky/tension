mod lobby;
mod main;
mod no_room;

pub enum State {
    NoRoom(no_room::NoRoom),
    Lobby(lobby::Lobby),
    Main(main::Main),
}

impl Default for State {
    fn default() -> Self {
        // Self::Main(main::Main::new())
        Self::NoRoom(Default::default())
    }
}

impl State {
    pub fn update(mut self, dt: std::time::Duration, _ctx: StateContext) -> Self {
        match self {
            Self::NoRoom(ref mut inner) => {
                inner.update(dt);
                self
            }
            Self::Main(ref mut inner) => {
                inner.update(dt);
                self
            }
            _ => self,
        }
    }

    pub fn handle_mouse_event(mut self, event: crate::MouseEvent, ctx: StateContext) -> State {
        match self {
            Self::Lobby(inner) => inner.handle_mouse_event(event),
            Self::Main(ref mut inner) => {
                inner.handle_mouse_event(event, ctx);
                self
            }
            _ => self,
        }
    }

    pub fn render(&self, ctx: StateContext) {
        match self {
            State::NoRoom(inner) => {
                inner.render(ctx);
            }
            State::Lobby(inner) => {
                inner.render(ctx);
            }
            State::Main(inner) => {
                inner.render(ctx);
            }
        }
    }
}

pub struct StateContext<'a, 'b, 'c> {
    pub g: solstice_2d::GraphicsLock<'b, 'c>,
    pub input_state: &'a super::InputState,
    pub time: &'a std::time::Duration,
}
