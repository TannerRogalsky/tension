use super::StateContext;
use shared::viewer::{ChangeType, InitialRoomState, User};
use shared::CustomMessage;

pub struct Main {
    sim: crate::sim::Sim,
    local_user: User,
    room: InitialRoomState,
    current_user: usize,
    local_click_in_flight: bool,
}

impl Main {
    pub fn new(local_user: User, room: InitialRoomState) -> Self {
        let sim = crate::sim::Sim::new();
        Self {
            sim,
            local_user,
            room,
            current_user: 0,
            local_click_in_flight: false,
        }
    }

    pub fn update(&mut self, dt: std::time::Duration, ctx: StateContext) {
        for msg in ctx.ws.try_recv_iter() {
            match msg.ty {
                ChangeType::Custom(cmd) => match cmd {
                    CustomMessage::Click(x, y) => {
                        log::debug!("CLICK ({}, {})", x, y);
                        self.local_click_in_flight = false;
                        if let Some(handle) = self.sim.body_at_point(x, y) {
                            self.sim.try_remove_body(handle);
                        }

                        self.current_user = (self.current_user + 1) % self.room.users.len();
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        self.sim.step(dt);
    }

    pub fn handle_mouse_event(&mut self, event: crate::MouseEvent, ctx: StateContext) {
        if let crate::MouseEvent::Button(crate::ElementState::Pressed, crate::MouseButton::Left) =
            event
        {
            let is_local = self.local_is_current();
            let can_click = !self.local_click_in_flight && is_local && self.sim.all_sleeping();

            if can_click {
                let (mx, my) = ctx.input_state.mouse_position;
                let [x, y] = crate::sim::Sim::screen_to_world(ctx.g.gfx().viewport(), mx, my);
                let clicked = self.sim.body_at_point(x, y).is_some();
                if clicked {
                    self.local_click_in_flight = true;
                    ctx.ws.send(shared::viewer::Command::Custom(
                        self.room.id,
                        shared::CustomMessage::Click(x, y),
                    ));
                }
            }
        }
    }

    pub fn render(&self, mut ctx: StateContext) {
        ctx.g.clear([0.2, 0.2, 0.2, 1.]);
        self.sim.render(&mut ctx.g);

        {
            let vw = ctx.g.gfx().viewport();
            let bounds = solstice_2d::Rectangle {
                x: vw.x() as f32,
                y: vw.y() as f32,
                width: vw.width() as f32,
                height: vw.height() as f32,
            };
            for (index, user) in self.room.users.iter().enumerate() {
                let color = if index == self.current_user {
                    [1., 1., 0., 1.]
                } else {
                    [1., 1., 1., 1.]
                };
                ctx.g.set_color(color);

                let text = if user.id == self.local_user.id {
                    format!("{}. *{}*", index + 1, user.name)
                } else {
                    format!("{}. {}", index + 1, user.name)
                };
                let scale = 16.;
                ctx.g.print(
                    text,
                    ctx.resources.sans_font,
                    scale,
                    solstice_2d::Rectangle {
                        y: (scale * 1.1 * index as f32 + 8.).round(),
                        ..bounds
                    },
                )
            }
        }
    }

    fn local_is_current(&self) -> bool {
        self.room
            .users
            .get(self.current_user)
            .map(|user| user.id == self.local_user.id)
            .unwrap_or(false)
    }
}
