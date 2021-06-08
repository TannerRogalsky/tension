#[cfg(target_arch = "wasm32")]
pub mod web;

use solstice_2d::Draw;

#[cfg(not(target_arch = "wasm32"))]
pub use glutin as winit;
#[cfg(target_arch = "wasm32")]
pub use winit;

use winit::event::{ElementState, MouseButton};

pub enum MouseEvent {
    Button(ElementState, MouseButton),
    Moved(f32, f32),
}

pub struct Game {
    ctx: solstice_2d::solstice::Context,
    gfx: solstice_2d::Graphics,
    time: std::time::Duration,
    physics: physics::PhysicsContext,
    input_state: InputState,
}

impl Game {
    pub fn new(
        mut ctx: solstice_2d::solstice::Context,
        time: std::time::Duration,
        width: f32,
        height: f32,
    ) -> eyre::Result<Self> {
        let gfx = solstice_2d::Graphics::new(&mut ctx, width, height)?;
        let physics = physics::PhysicsContext::new(0., -9.81 * 0.1);

        Ok(Self {
            ctx,
            gfx,
            time,
            physics,
            input_state: Default::default(),
        })
    }

    pub fn update(&mut self, time: std::time::Duration) {
        let dt = time - self.time;
        self.time = time;

        self.physics.step(dt);

        let projection = self.projection();
        let mut g = self.gfx.lock(&mut self.ctx);
        g.clear([0.2, 0.2, 0.2, 1.]);
        g.set_projection_mode(Some(projection));

        g.draw_with_color(
            solstice_2d::Rectangle::new(-16. / 9. / 2., -0.5, 16. / 9. * 2., 1.),
            [0.3, 0.1, 0.3, 1.],
        );
        g.draw_with_color(
            solstice_2d::Rectangle::new(-0.5, -0.5, 1., 1.),
            [0.1, 0.1, 0.3, 1.],
        );
        g.draw_with_color(
            solstice_2d::Rectangle::new(-0.25, 0., 0.5, 0.1),
            [0.1, 0.2, 0.8, 1.],
        );

        self.physics.debug_render(&mut g);
    }

    fn projection(&self) -> solstice_2d::Projection {
        let vw = self.gfx.viewport();
        let aspect = vw.width() as f32 / vw.height() as f32;
        solstice_2d::Projection::Orthographic(Some(solstice_2d::Orthographic {
            left: -aspect / 2.,
            right: aspect / 2.,
            top: 0.5,
            bottom: -0.5,
            near: 0.0,
            far: 100.0,
        }))
    }

    fn screen_to_world(&self, x: f32, y: f32) -> [f32; 2] {
        let screen = self.gfx.viewport();
        let (width, height) = (screen.width() as f32, screen.height() as f32);
        let norm_x = x / width;
        let norm_y = y / height;
        [(norm_x - 0.5) * 16. / 9., 1.0 - norm_y - 0.5]
    }

    pub fn handle_mouse_event(&mut self, event: MouseEvent) {
        match event {
            MouseEvent::Button(state, _) => match state {
                ElementState::Pressed => {
                    let all_sleeping = self
                        .physics
                        .bodies
                        .iter_active_dynamic()
                        .all(|(_h, b)| b.is_sleeping());

                    if all_sleeping {
                        let (mx, my) = self.input_state.mouse_position;
                        let [x, y] = self.screen_to_world(mx, my);
                        let point = rapier2d::na::Point2::new(x, y);
                        let clicked = self.physics.colliders.iter().find_map(|(_h, c)| {
                            let c: &rapier2d::geometry::Collider = c;
                            let shape = c.shape();
                            let transform = c.position();
                            let clicked = rapier2d::parry::query::point::PointQuery::contains_point(
                                shape, transform, &point,
                            );
                            if clicked {
                                Some(c.parent())
                            } else {
                                None
                            }
                        });
                        if let Some(handle) = clicked {
                            self.physics.bodies.remove(
                                handle,
                                &mut self.physics.colliders,
                                &mut self.physics.joints,
                            );
                        }
                    }
                }
                ElementState::Released => {}
            },
            MouseEvent::Moved(x, y) => {
                let mut is = &mut self.input_state;
                if is.mouse_position == is.prev_mouse_position && is.mouse_position == (0., 0.) {
                    is.prev_mouse_position = (x, y);
                    is.mouse_position = (x, y);
                } else {
                    is.prev_mouse_position = is.mouse_position;
                    is.mouse_position = (x, y);
                }
            }
        }
    }

    pub fn handle_resize(&mut self, win_width: f32, win_height: f32) {
        let vw =
            solstice_2d::solstice::viewport::Viewport::new(0, 0, win_width as _, win_height as _);
        self.ctx.set_viewport(0, 0, win_width as _, win_height as _);
        self.gfx.set_viewport(vw);

        let width = 16. / 9.;
        let height = 1.;

        let scale_x = win_width / width;
        let scale_y = win_height / height;
        let scale = scale_x.min(scale_y);

        let x = (win_width - width * scale) / 2.;
        let y = (win_height - height * scale) / 2.;

        // let w = width * scale_x / scale;
        // let h = height * scale_y / scale;
        let scissor = solstice_2d::solstice::viewport::Viewport::new(
            x as _,
            y as _,
            (width * scale) as _,
            (height * scale) as _,
        );
        self.gfx.set_scissor(Some(scissor));
    }
}

#[derive(Default)]
pub struct InputState {
    prev_mouse_position: (f32, f32),
    mouse_position: (f32, f32),
}

struct RepeatingTimer {
    time: std::time::Duration,
    elapsed: std::time::Duration,
}

impl RepeatingTimer {
    pub fn new(time: std::time::Duration) -> Self {
        Self {
            time,
            elapsed: Default::default(),
        }
    }

    pub fn update(&mut self, dt: std::time::Duration) -> bool {
        self.elapsed += dt;
        if self.elapsed >= self.time {
            self.elapsed -= self.time;
            true
        } else {
            false
        }
    }
}

mod physics {
    use super::RepeatingTimer as Timer;

    use rapier2d::dynamics::{
        CCDSolver, IntegrationParameters, JointSet, RigidBodyBuilder, RigidBodySet,
    };
    use rapier2d::geometry::{
        BroadPhase, ColliderBuilder, ColliderHandle, ColliderSet, ContactEvent, IntersectionEvent,
        NarrowPhase, TypedShape,
    };
    use rapier2d::na::{Point2, Vector2};
    use rapier2d::pipeline::{ChannelEventCollector, PhysicsPipeline, QueryPipeline};
    use solstice_2d::Stroke;

    pub struct PhysicsContext {
        pipeline: PhysicsPipeline,
        gravity: Vector2<f32>,
        integration_parameters: IntegrationParameters,
        broad_phase: BroadPhase,
        narrow_phase: NarrowPhase,
        pub bodies: RigidBodySet,
        pub colliders: ColliderSet,
        pub joints: JointSet,
        pub query_pipeline: QueryPipeline,
        ccd_solver: CCDSolver,

        event_handler: ChannelEventCollector,
        pub contact_events: crossbeam_channel::Receiver<ContactEvent>,
        pub intersection_events: crossbeam_channel::Receiver<IntersectionEvent>,
        kill_sensor: ColliderHandle,

        update_timer: Timer,
    }

    impl PhysicsContext {
        pub fn new(gx: f32, gy: f32) -> Self {
            let mut bodies = RigidBodySet::new();
            let mut colliders = ColliderSet::new();
            let joints = JointSet::new();

            let kill_sensor = {
                let ground_size = 0.4;
                let ground_thickness = 0.05;
                let camera_offset = -0.5;

                let collider = ColliderBuilder::cuboid(ground_size, ground_thickness).build();
                let body = RigidBodyBuilder::new_static()
                    .translation(0., camera_offset)
                    .build();
                let parent_handle = bodies.insert(body);
                colliders.insert(collider, parent_handle, &mut bodies);

                let num = 9;
                let rad = 0.05;

                let shift = rad * 2.0;
                let center_x = shift * ((num - 1) as f32) / 2.0;
                let center_y = shift / 2.0 + ground_thickness + rad * 1.5 + camera_offset;

                for i in 0usize..num {
                    for j in i..num {
                        let fj = j as f32;
                        let fi = i as f32;
                        let x = (fi * shift / 2.0) + (fj - fi) * shift - center_x;
                        let y = fi * shift + center_y;

                        let rigid_body = RigidBodyBuilder::new_dynamic().translation(x, y).build();
                        let handle = bodies.insert(rigid_body);
                        let collider = ColliderBuilder::cuboid(rad, rad).build();
                        colliders.insert(collider, handle, &mut bodies);
                    }
                }

                let kill_sensor = bodies.insert(
                    RigidBodyBuilder::new_static()
                        .translation(0.0, camera_offset * 1.5)
                        .build(),
                );
                let kill_sensor = colliders.insert(
                    ColliderBuilder::cuboid(ground_size * 4., ground_thickness)
                        .sensor(true)
                        .build(),
                    kill_sensor,
                    &mut bodies,
                );
                kill_sensor
            };

            let (contact_send, contact_recv) = crossbeam_channel::unbounded();
            let (intersection_send, intersection_recv) = crossbeam_channel::unbounded();
            let event_handler = ChannelEventCollector::new(intersection_send, contact_send);

            Self {
                pipeline: PhysicsPipeline::new(),
                gravity: Vector2::new(gx, gy),
                integration_parameters: Default::default(),
                broad_phase: BroadPhase::new(),
                narrow_phase: NarrowPhase::new(),
                bodies,
                colliders,
                joints,
                query_pipeline: Default::default(),
                ccd_solver: CCDSolver::new(),
                event_handler,
                contact_events: contact_recv,
                intersection_events: intersection_recv,
                kill_sensor,
                update_timer: Timer::new(std::time::Duration::from_secs_f32(1. / 60.)),
            }
        }

        pub fn step(&mut self, dt: std::time::Duration) {
            if self.update_timer.update(dt) {
                self.pipeline.step(
                    &self.gravity,
                    &self.integration_parameters,
                    &mut self.broad_phase,
                    &mut self.narrow_phase,
                    &mut self.bodies,
                    &mut self.colliders,
                    &mut self.joints,
                    &mut self.ccd_solver,
                    &(),
                    &self.event_handler,
                );
                self.query_pipeline.update(&self.bodies, &self.colliders);

                while let Ok(intersection_event) = self.intersection_events.try_recv() {
                    if intersection_event.collider1 == self.kill_sensor {
                        if let Some(other) = self.colliders.get(intersection_event.collider2) {
                            self.bodies.remove(
                                other.parent(),
                                &mut self.colliders,
                                &mut self.joints,
                            );
                        }
                    }

                    if intersection_event.collider2 == self.kill_sensor {
                        if let Some(other) = self.colliders.get(intersection_event.collider1) {
                            self.bodies.remove(
                                other.parent(),
                                &mut self.colliders,
                                &mut self.joints,
                            );
                        }
                    }
                }

                while let Ok(contact_event) = self.contact_events.try_recv() {
                    // println!("{:?}", contact_event);
                }
            }
        }

        pub fn debug_render(&self, g: &mut solstice_2d::GraphicsLock) {
            const AWAKE_BODY_COLOR: [f32; 4] = [0., 1., 0., 1.];
            const ASLEEP_BODY_COLOR: [f32; 4] = [0., 0., 1., 1.];

            for (_body_handle, body) in self.bodies.iter() {
                let position = body.position();
                for collider_handle in body.colliders() {
                    if let Some(collider) = self.colliders.get(*collider_handle) {
                        match collider.shape().as_typed_shape() {
                            TypedShape::Ball(_) => {}
                            TypedShape::Cuboid(shape) => {
                                let half = shape.half_extents;
                                let quad =
                                    solstice_2d::solstice::quad_batch::Quad::<(f32, f32)>::from(
                                        solstice_2d::Rectangle::new(
                                            -half.x,
                                            -half.y,
                                            half.x * 2.,
                                            half.y * 2.,
                                        ),
                                    )
                                    .map(|(x, y)| {
                                        let p = Point2::new(x, y);
                                        let p = position.transform_point(&p);
                                        solstice_2d::Vertex2D {
                                            position: [p.x, p.y],
                                            uv: [x + 0.5, y + 0.5],
                                            ..Default::default()
                                        }
                                    });
                                let color = if body.is_sleeping() {
                                    ASLEEP_BODY_COLOR
                                } else {
                                    AWAKE_BODY_COLOR
                                };
                                g.stroke_with_color(quad, color);
                            }
                            TypedShape::Capsule(_) => {}
                            TypedShape::Segment(_) => {}
                            TypedShape::Triangle(_) => {}
                            TypedShape::TriMesh(_) => {}
                            TypedShape::Polyline(_) => {}
                            TypedShape::HalfSpace(_) => {}
                            TypedShape::HeightField(_) => {}
                            TypedShape::Compound(_) => {}
                            TypedShape::ConvexPolygon(_) => {}
                            TypedShape::RoundCuboid(_) => {}
                            TypedShape::RoundTriangle(_) => {}
                            TypedShape::RoundConvexPolygon(_) => {}
                            TypedShape::Custom(_) => {}
                        }
                    }
                }
            }
        }
    }
}

mod net {
    use futures::{FutureExt, TryFutureExt};
    use shared::Message;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    pub struct Recv(websocket::WsRecv);
    pub struct Send(websocket::WsSend);

    impl From<websocket::WsRecv> for Recv {
        fn from(inner: websocket::WsRecv) -> Self {
            Self(inner)
        }
    }

    impl<T> shared::Receiver<T> for Recv
    where
        T: for<'a> serde::Deserialize<'a>,
    {
        fn try_recv(&self) -> Result<Message<T>, ()> {
            self.0.try_recv().map_err(|_| ()).and_then(|msg| {
                use websocket::Message;
                match msg {
                    Message::Text(text) => serde_json::from_str(&text),
                    Message::Binary(data) => serde_json::from_slice(&data),
                }
                .map_err(|_| ())
            })
        }
    }

    impl From<websocket::WsSend> for Send {
        fn from(inner: websocket::WsSend) -> Self {
            Self(inner)
        }
    }

    impl<T> shared::Sender<T> for Send
    where
        T: serde::Serialize,
    {
        fn send(&self, msg: Message<T>) -> Result<(), ()> {
            let data = serde_json::to_vec(&msg).map_err(|_| ())?;
            self.0
                .send(websocket::Message::Binary(data))
                .map_err(|_| ())
        }
    }

    pub struct Client {
        base_url: String,
        pub inner: shared::Client<(), Send, Recv>,
    }

    impl Client {
        pub async fn new(base_url: String) -> eyre::Result<Self> {
            let ws_url = String::from("ws://") + &base_url + shared::ENDPOINT_WS;
            let ws = websocket::WebSocket::connect(&ws_url).await?;
            let (sx, rx) = ws.into_channels();
            let inner = shared::Client::new(sx.into(), rx.into());
            Ok(Self { base_url, inner })
        }

        pub fn create_room(
            &self,
            player: shared::PlayerName,
        ) -> eyre::Result<NetFuture<shared::RoomID>> {
            let body = serde_json::to_string(&player)?;
            let url = String::from("http://") + &self.base_url + shared::ENDPOINT_CREATE_ROOM;

            let client = reqwest::Client::new();
            let inner = client
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .map_err(eyre::Report::from)
                .and_then(|response| response.text().map_err(eyre::Report::from))
                .map(|result: eyre::Result<String>| {
                    result.and_then(|text| serde_json::from_str(&text).map_err(eyre::Report::from))
                })
                .boxed_local();

            Ok(NetFuture { inner })
        }

        pub fn join_room(
            &self,
            join_info: &shared::RoomJoinInfo,
        ) -> eyre::Result<NetFuture<shared::RoomState>> {
            let body = serde_json::to_string(&join_info)?;
            let url = String::from("http://") + &self.base_url + shared::ENDPOINT_JOIN_ROOM;

            let client = reqwest::Client::new();
            let inner = client
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .map_err(eyre::Report::from)
                .and_then(|response| response.text().map_err(eyre::Report::from))
                .map(|result: eyre::Result<String>| {
                    result.and_then(|text| serde_json::from_str(&text).map_err(eyre::Report::from))
                })
                .boxed_local();

            Ok(NetFuture { inner })
        }
    }

    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub struct NetFuture<T> {
        inner: futures::future::LocalBoxFuture<'static, eyre::Result<T>>,
    }

    impl<T> Future for NetFuture<T> {
        type Output = eyre::Result<T>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.inner.poll_unpin(cx)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
