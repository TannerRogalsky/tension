use super::StateContext;
use shared::viewer::*;

#[derive(Debug)]
struct Room {
    id: shared::RoomID,
    users: Vec<User>,
}

#[derive(Debug)]
pub struct Lobby {
    local_user: shared::viewer::User,
    room: Room,
}

impl Lobby {
    pub fn new(local_user: User, room: InitialRoomState) -> Self {
        Self {
            local_user,
            room: Room {
                id: room.id,
                users: room.users,
            },
        }
    }

    pub fn update(&mut self, dt: std::time::Duration, ctx: StateContext) {
        for msg in ctx.ws.try_recv_iter() {
            if msg.target == self.room.id {
                match msg.ty {
                    ChangeType::UserJoin(user) => {
                        self.room.users.push(user);
                    }
                    ChangeType::UserLeave(user) => {
                        if let Some(index) = self.room.users.iter().position(|u| u.id == user) {
                            self.room.users.remove(index);
                        }
                    }
                    ChangeType::Custom(_) => {}
                }
            } else {
                log::error!(
                    "Received msg for Room {}! Our room is {}.",
                    msg.target,
                    self.room.id
                );
            }
        }
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
        ctx.g.clear([1., 1., 1., 1.]);

        let vw = ctx.g.gfx().viewport();
        let bounds = solstice_2d::Rectangle {
            x: vw.x() as f32,
            y: vw.y() as f32,
            width: vw.width() as f32,
            height: vw.height() as f32,
        };
        ctx.g.set_color([0., 0., 0., 1.]);
        ctx.g.print(
            format!("Room: {}", self.room.id),
            ctx.resources.sans_font,
            32.,
            bounds,
        );
        for (index, user) in self.room.users.iter().enumerate() {
            let text = format!("{}. {:?}", index, user.name);
            let scale = 16.;
            ctx.g.print(
                text,
                ctx.resources.sans_font,
                scale,
                solstice_2d::Rectangle {
                    y: (scale * 1.1 * index as f32 + 32.).round(),
                    ..bounds
                },
            )
        }
        ctx.g.set_color([1., 1., 1., 1.]);
    }
}
