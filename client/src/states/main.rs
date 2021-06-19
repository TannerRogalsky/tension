use super::StateContext;
use crate::winit::event::ElementState;
use crate::MouseEvent;
use shared::viewer::{ChangeType, InitialRoomState, User};
use shared::CustomMessage;
use solstice_2d::{Draw, Stroke};

const TEXT_SCALE: f32 = 16.;

pub struct Main {
    sim: crate::sim::Sim,
    local_user: User,
    room: InitialRoomState,
    local_click_in_flight: bool,
    click_queue: std::collections::VecDeque<(shared::PlayerID, u32)>,
    previous_click: Option<shared::PlayerID>,
    moving: Option<crate::sim::PhysicsTuple>,
}

impl Main {
    pub fn new(local_user: User, room: InitialRoomState, sim: crate::sim::Sim) -> Self {
        Self {
            sim,
            local_user,
            room,
            local_click_in_flight: false,
            click_queue: Default::default(),
            previous_click: None,
            moving: None,
        }
    }

    pub fn update(mut self, dt: std::time::Duration, ctx: StateContext) -> super::State {
        for msg in ctx.ws.try_recv_iter() {
            match msg.ty {
                ChangeType::Custom(cmd) => match cmd {
                    CustomMessage::RemoveBody(x, y) => {
                        log::debug!("CLICK ({}, {})", x, y);
                        self.local_click_in_flight = false;
                        if let Some(handle) = self.sim.body_at_point(x, y) {
                            self.moving = self.sim.try_remove_body(handle);
                        }

                        self.previous_click = self.click_queue.front().map(|(user, _)| *user);
                    }
                    CustomMessage::MoveBody(x, y) => {
                        if let Some((body, _)) = &mut self.moving {
                            let translation =
                                rapier2d::na::Translation2::from(rapier2d::na::Vector2::new(x, y));
                            let mut position = body.position().clone();
                            position.translation = translation;
                            body.set_position(position, false);
                        }
                    }
                    CustomMessage::DropBody(x, y) => {
                        if let Some((mut body, colliders)) = self.moving.take() {
                            let mut position = body.position().clone();
                            position.translation =
                                rapier2d::na::Translation2::from(rapier2d::na::Vector2::new(x, y));
                            body.set_position(position, false);
                            self.sim.add_body((body, colliders));
                        }
                        if let Some(count) = self.click_queue.front_mut().map(|(_user, count)| {
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
                    CustomMessage::StartGame(index) => {
                        let sim = (crate::sim::ROOM_TYPES[index as usize].gen)();
                        return super::State::Main(Self::new(self.local_user, self.room, sim));
                    }
                },
                ChangeType::UserJoin(user) => {
                    // users can join the room but they will be lobbied until the next game starts
                    self.room.users.push(user);
                }
                ChangeType::UserLeave(user_id) => {
                    let index = self.room.users.iter().position(|user| user.id == user_id);
                    if let Some(index) = index {
                        if index == 0 {
                            log::debug!("DM lefted room!");
                            return super::State::NoRoom(Default::default());
                        } else {
                            let user = self.room.users.remove(index);
                            self.click_queue.retain(|(id, _count)| id != &user.id);
                        }
                    }
                }
            }
        }

        self.sim.step(dt);

        super::State::Main(self)
    }

    pub fn handle_mouse_event(&mut self, event: crate::MouseEvent, ctx: StateContext) {
        if self.is_dm(&self.local_user) {
            if event.is_left_press() {
                let (mx, my) = ctx.input_state.mouse_position;
                let clicked = crate::sim::ROOM_TYPES
                    .iter()
                    .enumerate()
                    .find_map(|(index, _)| {
                        if crate::collides([mx, my], &Self::room_type_bounds(index)) {
                            Some(index)
                        } else {
                            None
                        }
                    });
                if let Some(index) = clicked {
                    ctx.ws.send(shared::viewer::Command::Custom(
                        self.room.id,
                        shared::CustomMessage::StartGame(index as _),
                    ));
                } else {
                    let (mx, my) = ctx.input_state.mouse_position;
                    let clicked = self.room.users[1..].iter().find(|user| {
                        let bbox = self.username_bbox(user).unwrap();
                        crate::collides([mx, my], &bbox)
                    });
                    if let Some(user) = clicked {
                        ctx.ws.send(shared::viewer::Command::Custom(
                            self.room.id,
                            shared::CustomMessage::AssignClick(user.id, 1),
                        ));
                    }
                }
            }
        } else {
            if self.is_next(&self.local_user) {
                match event {
                    MouseEvent::Button(state, crate::MouseButton::Left) => match state {
                        ElementState::Pressed => {
                            let can_click = !self.local_click_in_flight && self.sim.all_sleeping();

                            if can_click {
                                let (mx, my) = ctx.input_state.mouse_position;
                                let [x, y] = crate::sim::Sim::screen_to_world(
                                    ctx.g.gfx().viewport(),
                                    mx,
                                    my,
                                );
                                let clicked = self.sim.body_at_point(x, y).is_some();
                                if clicked {
                                    self.local_click_in_flight = true;
                                    ctx.ws.send(shared::viewer::Command::Custom(
                                        self.room.id,
                                        shared::CustomMessage::RemoveBody(x, y),
                                    ));
                                }
                            }
                        }
                        ElementState::Released => {
                            if self.local_click_in_flight || self.moving.is_some() {
                                let (mx, my) = ctx.input_state.mouse_position;
                                let [x, y] = crate::sim::Sim::screen_to_world(
                                    ctx.g.gfx().viewport(),
                                    mx,
                                    my,
                                );
                                ctx.ws.send(shared::viewer::Command::Custom(
                                    self.room.id,
                                    shared::CustomMessage::DropBody(x, y),
                                ));
                            }
                        }
                    },
                    MouseEvent::Moved(mx, my) => {
                        if self.local_click_in_flight || self.moving.is_some() {
                            let [x, y] =
                                crate::sim::Sim::screen_to_world(ctx.g.gfx().viewport(), mx, my);
                            ctx.ws.send(shared::viewer::Command::Custom(
                                self.room.id,
                                shared::CustomMessage::MoveBody(x, y),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn render(&self, mut ctx: StateContext) {
        ctx.g.clear([0.2, 0.2, 0.2, 1.]);
        self.sim.render(&mut ctx.g);

        if let Some((body, colliders)) = &self.moving {
            let position = body.position();
            for collider in colliders {
                if let Some(shape) = collider.shape().as_cuboid() {
                    let half = shape.half_extents;
                    let quad = solstice_2d::solstice::quad_batch::Quad::<(f32, f32)>::from(
                        solstice_2d::Rectangle::new(-half.x, -half.y, half.x * 2., half.y * 2.),
                    )
                    .map(|(x, y)| {
                        let p = rapier2d::na::Point2::new(x, y);
                        let p = position.transform_point(&p);
                        solstice_2d::Vertex2D {
                            position: [p.x, p.y],
                            uv: [x + 0.5, y + 0.5],
                            color: [1., 0.2, 0.2, 0.8],
                        }
                    });
                    ctx.g.draw(quad);
                } else {
                    log::debug!("unrecognized shape");
                }
            }
        }

        ctx.g.set_projection_mode(None);
        let font_id = ctx.resources.sans_font;
        if self.sim.kill_triggered() {
            let vw = ctx.g.gfx().viewport();
            let screen = solstice_2d::Rectangle {
                x: 0.,
                y: 0.,
                width: vw.width() as _,
                height: vw.height() as _,
            };
            ctx.g.draw_with_color(screen, [0., 0., 0., 0.4]);
            let clicker = self
                .previous_click
                .and_then(|id| self.room.users.iter().find(|user| user.id == id));
            if let Some(user) = clicker {
                let text = format!("{} knocked over the tower!", user.name);
                ctx.g.print(
                    text,
                    font_id,
                    TEXT_SCALE * 3.,
                    solstice_2d::Rectangle {
                        x: 38.0,
                        y: screen.height / 2. - TEXT_SCALE * 3. / 2.,
                        ..screen
                    },
                );
            }
        }

        {
            let vw = ctx.g.gfx().viewport();
            let bounds = solstice_2d::Rectangle {
                x: vw.x() as f32 + 8.,
                y: vw.y() as f32,
                width: vw.width() as f32,
                height: vw.height() as f32,
            };
            let room_code_text = format!("ROOM CODE: {}", self.room.id);
            ctx.g.print(
                room_code_text,
                font_id,
                TEXT_SCALE,
                solstice_2d::Rectangle { y: 8., ..bounds },
            );
            if let Some(dm) = self.room.users.first() {
                let text = format!("DM: {}", dm.name);
                ctx.g.print(
                    text,
                    font_id,
                    TEXT_SCALE,
                    solstice_2d::Rectangle {
                        y: 8. + TEXT_SCALE,
                        ..bounds
                    },
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

        if self.is_dm(&self.local_user) {
            ctx.g.set_color([1., 1., 1., 1.]);
            for (index, room_ty) in crate::sim::ROOM_TYPES.iter().enumerate() {
                let bounds = Self::room_type_bounds(index);
                ctx.g.print(room_ty.name, font_id, 32., bounds);
                ctx.g.stroke(bounds);
            }
        }
    }

    fn username_bbox(&self, user: &User) -> Option<solstice_2d::Rectangle> {
        self.room.users[1..]
            .iter()
            .position(|other| user.id == other.id)
            .map(|index| solstice_2d::Rectangle {
                x: 8.,
                y: (TEXT_SCALE * 1.1 * (index + 2) as f32 + 8.).round(),
                width: 200.,
                height: TEXT_SCALE,
            })
    }

    fn room_type_bounds(index: usize) -> solstice_2d::Rectangle {
        solstice_2d::Rectangle {
            x: 720.,
            y: index as f32 * 32. * 1.5 + 32.,
            width: 480.,
            height: 32.,
        }
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
