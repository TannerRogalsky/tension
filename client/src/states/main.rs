use super::StateContext;
use shared::viewer::{ChangeType, InitialRoomState, User};
use shared::CustomMessage;
use solstice_2d::Stroke;

const TEXT_SCALE: f32 = 16.;

pub struct Main {
    sim: crate::sim::Sim,
    local_user: User,
    room: InitialRoomState,
    current_user: usize,
    local_click_in_flight: bool,
    click_queue: std::collections::VecDeque<(shared::PlayerID, u32)>,
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
            click_queue: Default::default(),
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

                        if let Some(count) = self.click_queue.front_mut().map(|(_, count)| {
                            *count -= 1;
                            *count
                        }) {
                            if count == 0 {
                                self.click_queue.pop_front();
                            }
                        }
                    }
                    CustomMessage::AssignClick(player_id, count) => {
                        self.click_queue.push_back((player_id, count));
                    }
                    CustomMessage::StartGame => {
                        log::error!("unimplemented");
                    }
                },
                _ => {}
            }
        }

        self.sim.step(dt);
    }

    pub fn handle_mouse_event(&mut self, event: crate::MouseEvent, ctx: StateContext) {
        if self.is_dm(&self.local_user) {
            if event.is_left_click() {
                let (mx, my) = ctx.input_state.mouse_position;
                let clicked = self.room.users[1..].iter().find(|user| {
                    let bbox = self.username_bbox(user).unwrap();
                    collides([mx, my], &bbox)
                });
                if let Some(user) = clicked {
                    ctx.ws.send(shared::viewer::Command::Custom(
                        self.room.id,
                        shared::CustomMessage::AssignClick(user.id, 1),
                    ));
                }
            }
        } else {
            if event.is_left_click() {
                let is_next = self.is_next(&self.local_user);
                let can_click = !self.local_click_in_flight && is_next && self.sim.all_sleeping();

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
    }

    pub fn render(&self, mut ctx: StateContext) {
        ctx.g.clear([0.2, 0.2, 0.2, 1.]);
        self.sim.render(&mut ctx.g);

        {
            let vw = ctx.g.gfx().viewport();
            let bounds = solstice_2d::Rectangle {
                x: vw.x() as f32 + 8.,
                y: vw.y() as f32,
                width: vw.width() as f32,
                height: vw.height() as f32,
            };
            let font_id = ctx.resources.sans_font;
            if let Some(dm) = self.room.users.first() {
                let text = format!("DM: {}", dm.name);
                ctx.g.print(
                    text,
                    font_id,
                    TEXT_SCALE,
                    solstice_2d::Rectangle { y: 8., ..bounds },
                );
            }
            for (index, user) in self.room.users[1..].iter().enumerate() {
                let color = if self.is_next(user) {
                    [1., 1., 0., 1.]
                } else {
                    [1., 1., 1., 1.]
                };
                ctx.g.set_color(color);

                let click_count = self
                    .click_queue
                    .iter()
                    .filter_map(
                        |(id, count)| {
                            if id == &user.id {
                                Some(*count)
                            } else {
                                None
                            }
                        },
                    )
                    .sum::<u32>();
                let text = if user.id == self.local_user.id {
                    format!("{}. *{}*: {}", index + 1, user.name, click_count)
                } else {
                    format!("{}. {}: {}", index + 1, user.name, click_count)
                };
                let bounds = self.username_bbox(user).unwrap();
                ctx.g.print(text, font_id, TEXT_SCALE, bounds);
                if self.is_dm(&self.local_user) {
                    ctx.g.stroke(bounds);
                }
            }
        }
    }

    fn username_bbox(&self, user: &User) -> Option<solstice_2d::Rectangle> {
        self.room.users[1..]
            .iter()
            .position(|other| user.id == other.id)
            .map(|index| solstice_2d::Rectangle {
                x: 8.,
                y: (TEXT_SCALE * 1.1 * (index + 1) as f32 + 8.).round(),
                width: 100.,
                height: TEXT_SCALE,
            })
    }

    #[allow(unused)]
    fn is_local_current(&self) -> bool {
        self.room
            .users
            .get(self.current_user)
            .map(|user| user.id == self.local_user.id)
            .unwrap_or(false)
    }

    fn is_next(&self, user: &User) -> bool {
        self.click_queue
            .front()
            .map(|(id, _)| id == &user.id)
            .unwrap_or(false)
    }

    fn is_dm(&self, user: &User) -> bool {
        if let Some(first) = self.room.users.first() {
            first.id == user.id
        } else {
            false
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
