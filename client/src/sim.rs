use solstice_2d::solstice::viewport::Viewport;

pub struct RoomType {
    pub name: &'static str,
    pub gen: fn() -> Sim,
}

pub const ROOM_TYPES: [RoomType; 3] = [
    RoomType {
        name: "standard",
        gen: Sim::new,
    },
    RoomType {
        name: "tower",
        gen: Sim::tower,
    },
    RoomType {
        name: "pyramid",
        gen: Sim::pyramid,
    },
];

pub struct Sim {
    physics: physics::PhysicsContext,
}

impl Sim {
    pub fn new() -> Self {
        let init = physics::PhysicsContext::special_tower;
        let physics = physics::PhysicsContext::new(0., -9.81 * 0.1, init);
        Self { physics }
    }

    pub fn tower() -> Self {
        let init = physics::PhysicsContext::tower;
        let physics = physics::PhysicsContext::new(0., -9.81 * 0.1, init);
        Self { physics }
    }

    pub fn pyramid() -> Self {
        let init = physics::PhysicsContext::pyramid;
        let physics = physics::PhysicsContext::new(0., -9.81 * 0.1, init);
        Self { physics }
    }

    pub fn step(&mut self, dt: std::time::Duration) {
        self.physics.step(dt);
    }

    pub fn render(&self, g: &mut solstice_2d::GraphicsLock) {
        use solstice_2d::Draw;
        let vw = g.gfx().viewport().clone();
        g.set_projection_mode(Some(Self::projection(&vw)));

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

        self.physics.debug_render(g);
        g.set_projection_mode(None);
    }

    pub fn projection(vw: &Viewport<i32>) -> solstice_2d::Projection {
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

    pub fn screen_to_world(screen: &Viewport<i32>, x: f32, y: f32) -> [f32; 2] {
        let (width, height) = (screen.width() as f32, screen.height() as f32);
        let norm_x = x / width;
        let norm_y = y / height;
        [(norm_x - 0.5) * 16. / 9., 1.0 - norm_y - 0.5]
    }

    pub fn all_sleeping(&self) -> bool {
        self.physics
            .bodies
            .iter_active_dynamic()
            .all(|(_h, b)| b.is_sleeping())
    }

    pub fn body_at_point(&self, x: f32, y: f32) -> Option<rapier2d::dynamics::RigidBodyHandle> {
        let point = rapier2d::na::Point2::new(x, y);
        self.physics.colliders.iter().find_map(|(_h, c)| {
            let c: &rapier2d::geometry::Collider = c;
            if let Some(true) = self.physics.bodies.get(c.parent()).map(|b| b.is_dynamic()) {
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
            } else {
                None
            }
        })
    }

    pub fn try_remove_body(
        &mut self,
        handle: rapier2d::dynamics::RigidBodyHandle,
    ) -> Option<rapier2d::dynamics::RigidBody> {
        self.physics.bodies.remove(
            handle,
            &mut self.physics.colliders,
            &mut self.physics.joints,
        )
    }

    pub fn kill_triggered(&self) -> bool {
        self.physics.kill_triggered()
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
        kill_triggered: bool,
    }

    pub trait GenResult: Iterator<Item = (ColliderBuilder, RigidBodyBuilder)> {}
    impl<T> GenResult for T where T: Iterator<Item = (ColliderBuilder, RigidBodyBuilder)> {}
    pub type Gen<I> = fn(usize, f32, f32) -> I;

    impl PhysicsContext {
        pub fn new(gx: f32, gy: f32, init: Gen<impl GenResult>) -> Self {
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
                let rad = 0.025;
                let offset_y = ground_thickness + camera_offset;

                for (collider, rigid_body) in init(num, rad, offset_y) {
                    let handle = bodies.insert(rigid_body.build());
                    colliders.insert(collider.build(), handle, &mut bodies);
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
                kill_triggered: false,
            }
        }

        pub fn special_tower(num: usize, rad: f32, offset_y: f32) -> impl GenResult {
            type Gen = Box<dyn Fn(usize) -> (ColliderBuilder, RigidBodyBuilder)>;
            (0usize..num).flat_map(move |y| {
                let yf = y as f32;
                if y % 2 == 0 {
                    let shift = rad * 2.0;
                    let center_x = shift * ((num - 1) as f32) / 2.0;
                    let center_y = shift / 2.0 + offset_y;
                    (0..num).map(Box::new(move |x: usize| {
                        let xf = x as f32;

                        let x_offset = 0.;
                        let x = (xf * shift) - center_x + x_offset * shift;
                        let y = yf * shift + center_y;

                        let c = ColliderBuilder::cuboid(rad, rad);
                        let b = RigidBodyBuilder::new_dynamic().translation(x, y);
                        (c, b)
                    }) as Gen)
                } else {
                    let num = num / 2;
                    let shift = rad * 2.;
                    let center_x = shift * 2.5 * ((num - 1) as f32) / 2.0;
                    let center_y = rad + offset_y;
                    (0..num).map(Box::new(move |x: usize| {
                        let xf = x as f32;

                        let x_offset = 0.;
                        let x = (xf * shift * 2.5) - center_x + x_offset * rad * 2.;
                        let y = yf * shift + center_y;

                        let c = ColliderBuilder::cuboid(rad * 2., rad);
                        let b = RigidBodyBuilder::new_dynamic().translation(x, y);
                        (c, b)
                    }) as Gen)
                }
            })
        }

        pub fn tower(num: usize, rad: f32, offset_y: f32) -> impl GenResult {
            let shift = rad * 2.0;
            let center_x = shift * ((num - 1) as f32) / 2.0;
            let center_y = shift / 2.0 + offset_y;

            let colliders = std::iter::repeat_with(move || ColliderBuilder::cuboid(rad, rad));
            let bodies = (0usize..num).flat_map(move |y| {
                let x_count = if y % 2 == 0 { num } else { num - 1 };
                let yf = y as f32;
                (0..x_count).map(move |x| {
                    let xf = x as f32;

                    let x_offset = (num - x_count) as f32 / 2.;
                    let x = (xf * shift) - center_x + x_offset * shift;
                    let y = yf * shift + center_y;

                    RigidBodyBuilder::new_dynamic().translation(x, y)
                })
            });
            colliders.zip(bodies)
        }

        pub fn pyramid(num: usize, rad: f32, offset_y: f32) -> impl GenResult {
            let shift = rad * 2.0;
            let center_x = shift * ((num - 1) as f32) / 2.0;
            let center_y = shift / 2.0 + offset_y;

            let colliders = std::iter::repeat_with(move || ColliderBuilder::cuboid(rad, rad));
            let bodies = (0usize..num).flat_map(move |i| {
                (i..num).map(move |j| {
                    let fj = j as f32;
                    let fi = i as f32;
                    let x = (fi * shift / 2.0) + (fj - fi) * shift - center_x;
                    let y = fi * shift + center_y;

                    RigidBodyBuilder::new_dynamic().translation(x, y)
                })
            });
            colliders.zip(bodies)
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
                            self.kill_triggered = true;
                            self.bodies.remove(
                                other.parent(),
                                &mut self.colliders,
                                &mut self.joints,
                            );
                        }
                    }

                    if intersection_event.collider2 == self.kill_sensor {
                        if let Some(other) = self.colliders.get(intersection_event.collider1) {
                            self.kill_triggered = true;
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

        pub fn kill_triggered(&self) -> bool {
            self.kill_triggered
        }

        pub fn debug_render(&self, g: &mut solstice_2d::GraphicsLock) {
            use solstice_2d::Draw;

            const AWAKE_BODY_COLOR: [f32; 4] = [0., 0.8, 0., 1.];
            const ASLEEP_BODY_COLOR: [f32; 4] = [0., 0., 0.8, 1.];
            const AWAKE_BODY_OUTLINE: [f32; 4] = [0., 0., 0., 1.];
            const ASLEEP_BODY_OUTLINE: [f32; 4] = [0., 0., 0., 1.];

            let mut rects = Vec::with_capacity(self.bodies.len());

            for (_body_handle, body) in self.bodies.iter() {
                let position = body.position();
                for collider_handle in body.colliders() {
                    if let Some(collider) = self.colliders.get(*collider_handle) {
                        match collider.shape().as_typed_shape() {
                            TypedShape::Ball(_) => {}
                            TypedShape::Cuboid(shape) => {
                                let half = shape.half_extents;
                                let (color, outline) = if body.is_sleeping() {
                                    (ASLEEP_BODY_COLOR, ASLEEP_BODY_OUTLINE)
                                } else {
                                    (AWAKE_BODY_COLOR, AWAKE_BODY_OUTLINE)
                                };
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
                                            color,
                                        }
                                    });
                                rects.push((quad, outline));
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

            let outlines = rects
                .iter()
                .flat_map(|(quad, color)| {
                    std::iter::once(solstice_2d::LineVertex {
                        position: [
                            quad.vertices[0].position[0],
                            quad.vertices[0].position[1],
                            0.,
                        ],
                        width: 0.0,
                        color: [0., 0., 0., 0.],
                    })
                    .chain(std::array::IntoIter::new(quad.vertices).map(move |v| {
                        solstice_2d::LineVertex {
                            position: [v.position[0], v.position[1], 0.],
                            width: 2.,
                            color: *color,
                        }
                    }))
                    .chain(std::array::IntoIter::new([
                        solstice_2d::LineVertex {
                            position: [
                                quad.vertices[0].position[0],
                                quad.vertices[0].position[1],
                                0.,
                            ],
                            width: 2.0,
                            color: *color,
                        },
                        solstice_2d::LineVertex {
                            position: [
                                quad.vertices[3].position[0],
                                quad.vertices[3].position[1],
                                0.,
                            ],
                            width: 0.0,
                            color: [0., 0., 0., 0.],
                        },
                    ]))
                })
                .collect::<Vec<_>>();

            let indices = rects
                .iter()
                .enumerate()
                .flat_map(|(index, _)| {
                    let offset = index as u32 * 4;
                    std::array::IntoIter::new(solstice_2d::solstice::quad_batch::INDICES)
                        .map(move |i| i as u32 + offset)
                })
                .collect::<Vec<_>>();
            let vertices = rects
                .into_iter()
                .flat_map(|(quad, _)| std::array::IntoIter::new(quad.vertices))
                .collect::<Vec<_>>();
            g.draw(solstice_2d::Geometry::new(vertices, Some(indices)));
            g.line_2d(outlines);
        }
    }
}
