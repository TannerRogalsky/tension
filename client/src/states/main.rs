use super::StateContext;
use solstice_2d::Draw;

pub struct Main {
    physics: physics::PhysicsContext,
}

impl Main {
    pub fn new() -> Self {
        let physics = physics::PhysicsContext::new(0., -9.81 * 0.1);
        Self { physics }
    }

    pub fn update(&mut self, dt: std::time::Duration) {
        self.physics.step(dt);
    }

    pub fn handle_mouse_event(&mut self, event: crate::MouseEvent, ctx: StateContext) {
        if let crate::MouseEvent::Button(crate::ElementState::Pressed, crate::MouseButton::Left) =
            event
        {
            let all_sleeping = self
                .physics
                .bodies
                .iter_active_dynamic()
                .all(|(_h, b)| b.is_sleeping());

            if all_sleeping {
                let (mx, my) = ctx.input_state.mouse_position;
                let [x, y] = Self::screen_to_world(&ctx, mx, my);
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
    }

    pub fn render(&self, mut ctx: StateContext) {
        let projection = Self::projection(&ctx);
        ctx.g.clear([0.2, 0.2, 0.2, 1.]);
        ctx.g.set_projection_mode(Some(projection));

        ctx.g.draw_with_color(
            solstice_2d::Rectangle::new(-16. / 9. / 2., -0.5, 16. / 9. * 2., 1.),
            [0.3, 0.1, 0.3, 1.],
        );
        ctx.g.draw_with_color(
            solstice_2d::Rectangle::new(-0.5, -0.5, 1., 1.),
            [0.1, 0.1, 0.3, 1.],
        );
        ctx.g.draw_with_color(
            solstice_2d::Rectangle::new(-0.25, 0., 0.5, 0.1),
            [0.1, 0.2, 0.8, 1.],
        );

        self.physics.debug_render(&mut ctx.g);
    }

    fn projection(ctx: &StateContext) -> solstice_2d::Projection {
        let vw = ctx.g.gfx().viewport();
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

    fn screen_to_world(ctx: &StateContext, x: f32, y: f32) -> [f32; 2] {
        let screen = ctx.g.gfx().viewport();
        let (width, height) = (screen.width() as f32, screen.height() as f32);
        let norm_x = x / width;
        let norm_y = y / height;
        [(norm_x - 0.5) * 16. / 9., 1.0 - norm_y - 0.5]
    }
}

mod physics {
    use crate::RepeatingTimer as Timer;

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

                while let Ok(_contact_event) = self.contact_events.try_recv() {
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
