use super::StateContext;
use shared::viewer::*;
use shared::CustomMessage;
use solstice_2d::Stroke;

#[derive(Debug)]
pub struct Lobby {
    local_user: shared::viewer::User,
    room: InitialRoomState,
}

impl Lobby {
    pub fn new(local_user: User, room: InitialRoomState) -> Self {
        Self { local_user, room }
    }

    pub fn update(mut self, _dt: std::time::Duration, ctx: StateContext) -> super::State {
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
                    ChangeType::Custom(cmd) => match cmd {
                        CustomMessage::StartGame(index) => {
                            let sim = (crate::sim::ROOM_TYPES[index as usize].gen)();
                            let main = super::main::Main::new(self.local_user, self.room, sim);
                            return super::State::Main(main);
                        }
                        _ => {
                            log::error!("Discarded a command!")
                        }
                    },
                }
            } else {
                log::error!(
                    "Received msg for Room {}! Our room is {}.",
                    msg.target,
                    self.room.id
                );
            }
        }
        super::State::Lobby(self)
    }

    pub fn handle_mouse_event(&self, event: crate::MouseEvent, ctx: StateContext) {
        if self.is_dm(&self.local_user) && event.is_left_press() {
            let (mx, my) = ctx.input_state.mouse_position;
            for (index, _) in crate::sim::ROOM_TYPES.iter().enumerate() {
                if crate::collides([mx, my], &Self::room_type_bounds(index)) {
                    ctx.ws.send(shared::viewer::Command::Custom(
                        self.room.id,
                        shared::CustomMessage::StartGame(index as _),
                    ));
                    break;
                }
            }
        }
    }

    pub fn render(&self, mut ctx: StateContext) {
        ctx.g.clear([1., 1., 1., 1.]);

        let font_id = ctx.resources.sans_font;
        let vw = ctx.g.gfx().viewport();
        let bounds = solstice_2d::Rectangle {
            x: vw.x() as f32,
            y: vw.y() as f32,
            width: vw.width() as f32,
            height: vw.height() as f32,
        };
        ctx.g.set_color([0., 0., 0., 1.]);
        ctx.g
            .print(format!("Room: {}", self.room.id), font_id, 32., bounds);
        for (index, user) in self.room.users.iter().enumerate() {
            let text = format!("{}. {}", index + 1, user.name);
            let scale = 16.;
            ctx.g.print(
                text,
                font_id,
                scale,
                solstice_2d::Rectangle {
                    y: (scale * 1.1 * index as f32 + 32.).round(),
                    ..bounds
                },
            );
        }

        if self.is_dm(&self.local_user) {
            for (index, room_ty) in crate::sim::ROOM_TYPES.iter().enumerate() {
                let bounds = Self::room_type_bounds(index);
                ctx.g.print(room_ty.name, font_id, 32., bounds);
                ctx.g.stroke(bounds);
            }
        } else {
            ctx.g.print(
                "Waiting For DM to start room.",
                font_id,
                32.,
                solstice_2d::Rectangle {
                    y: bounds.height - 32.,
                    height: 32.,
                    ..bounds
                },
            )
        }

        ctx.g.set_color([1., 1., 1., 1.]);
    }

    fn room_type_bounds(index: usize) -> solstice_2d::Rectangle {
        solstice_2d::Rectangle {
            x: 720.,
            y: index as f32 * 32. * 1.5 + 32.,
            width: 480.,
            height: 32.,
        }
    }

    fn is_dm(&self, user: &User) -> bool {
        if let Some(first) = self.room.users.first() {
            first.id == user.id
        } else {
            false
        }
    }
}
