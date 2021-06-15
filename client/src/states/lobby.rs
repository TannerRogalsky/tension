use super::StateContext;
use shared::viewer::*;

#[derive(Debug)]
pub struct Lobby {
    local_user: shared::viewer::User,
    room: shared::viewer::RoomState,
}

impl Lobby {
    pub fn new(local_user: User, room: RoomState) -> Self {
        Self { local_user, room }
    }

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
        ctx.g.clear([0., 1., 1., 1.]);

        let vw = ctx.g.gfx().viewport();
        let bounds = solstice_2d::Rectangle {
            x: vw.x() as f32,
            y: vw.y() as f32,
            width: vw.width() as f32,
            height: vw.height() as f32,
        };
        for (index, user) in self.room.users.iter().enumerate() {
            let text = format!("{}. {:?}", index, user);
            ctx.g.print(
                text,
                ctx.resources.sans_font,
                16.,
                solstice_2d::Rectangle {
                    y: 16. * 2. * index as f32,
                    ..bounds
                },
            )
        }
    }
}
