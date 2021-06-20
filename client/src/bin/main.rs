use client::*;

fn main() -> eyre::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()?;

    let (width, height) = (1280, 720);
    let event_loop = glutin::event_loop::EventLoop::new();
    let wb = glutin::window::WindowBuilder::new()
        .with_title("TENSION")
        .with_inner_size(glutin::dpi::PhysicalSize::new(width, height));
    let (glow_ctx, window) = window::init_ctx(wb, &event_loop);
    let mut ctx = solstice_2d::solstice::Context::new(glow_ctx);
    let mut gfx = solstice_2d::Graphics::new(&mut ctx, width as f32, height as f32)?;

    let now = {
        let epoch = std::time::Instant::now();
        move || epoch.elapsed()
    };

    let mut game = sim::Sim::new();

    let mut prev_t = now();
    let (mut mx, mut my) = (0., 0.);

    event_loop.run(move |event, _, cf| {
        use glutin::{event::*, event_loop::ControlFlow};
        match event {
            Event::NewEvents(_) => {}
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::Resized(size) => {
                    use solstice_2d::solstice::viewport::Viewport;
                    let (win_width, win_height) = (size.width, size.height);
                    let vw = Viewport::new(0, 0, win_width as _, win_height as _);
                    ctx.set_viewport(0, 0, win_width as _, win_height as _);
                    gfx.set_viewport(vw);
                }
                WindowEvent::CloseRequested => {
                    *cf = ControlFlow::Exit;
                }
                WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            state: ElementState::Pressed,
                            virtual_keycode: Some(key_code),
                            ..
                        },
                    ..
                } => match key_code {
                    VirtualKeyCode::Q => game = sim::Sim::new(),
                    VirtualKeyCode::W => game = sim::Sim::pyramid(),
                    VirtualKeyCode::E => game = sim::Sim::tower(),
                    VirtualKeyCode::R => game = sim::Sim::thin(),
                    _ => {}
                },
                WindowEvent::MouseInput { state, button, .. } => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        let [x, y] = crate::sim::Sim::screen_to_world(gfx.viewport(), mx, my);
                        if let Some(handle) = game.body_at_point(x, y) {
                            game.try_remove_body(handle);
                        }
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    mx = position.x as f32;
                    my = position.y as f32;
                }
                _ => {}
            },
            Event::DeviceEvent { .. } => {}
            Event::UserEvent(_) => {}
            Event::Suspended => {}
            Event::Resumed => {}
            Event::MainEventsCleared => {
                window.request_redraw();
            }
            Event::RedrawRequested(_) => {
                let t = now();
                let dt = t - prev_t;
                prev_t = t;
                game.step(dt);
                game.render(&mut gfx.lock(&mut ctx));
                window.swap_buffers().expect("omfg");
            }
            Event::RedrawEventsCleared => {}
            Event::LoopDestroyed => {}
        }
    });
}

mod window {
    mod native {
        use glutin as winit;
        use solstice_2d::solstice::glow::Context;
        use winit::{
            event_loop::EventLoop,
            window::{Window, WindowBuilder},
        };

        type WindowContext = winit::ContextWrapper<winit::PossiblyCurrent, winit::window::Window>;

        pub struct NativeWindow {
            inner: WindowContext,
        }

        impl NativeWindow {
            pub fn new(inner: WindowContext) -> Self {
                Self { inner }
            }

            pub fn swap_buffers(&self) -> eyre::Result<()> {
                self.inner.swap_buffers().map_err(eyre::Report::new)
            }
        }

        impl std::ops::Deref for NativeWindow {
            type Target = Window;

            fn deref(&self) -> &Self::Target {
                &self.inner.window()
            }
        }

        pub fn init_ctx(wb: WindowBuilder, el: &EventLoop<()>) -> (Context, NativeWindow) {
            let windowed_context = winit::ContextBuilder::new()
                .with_multisampling(16)
                .with_vsync(true)
                .build_windowed(wb, &el)
                .unwrap();
            let windowed_context = unsafe { windowed_context.make_current().unwrap() };
            let gfx = unsafe {
                Context::from_loader_function(|s| windowed_context.get_proc_address(s) as *const _)
            };
            (gfx, NativeWindow::new(windowed_context))
        }
    }

    pub use {
        glutin as winit,
        native::{init_ctx, NativeWindow as Window},
    };
}
