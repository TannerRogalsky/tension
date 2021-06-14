use super::StateContext;

#[derive(Default)]
pub struct Lobby;

impl Lobby {
    pub fn handle_mouse_event(self, event: crate::MouseEvent) -> super::State {
        match event {
            crate::MouseEvent::Button(state, button) => match (state, button) {
                (crate::ElementState::Pressed, crate::MouseButton::Left) => {
                    super::State::Main(super::main::Main::new())
                }
                _ => super::State::Lobby(self),
            },
            _ => super::State::Lobby(self),
        }
    }

    pub fn render(&self, mut ctx: StateContext) {
        // let width = ctx.g.gfx().viewport().width() as f32;
        // let height = ctx.g.gfx().viewport().height() as f32;

        ctx.g.clear([1., 1., 1., 1.]);
    }
}
