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
    let ctx = solstice_2d::solstice::Context::new(glow_ctx);

    let now = {
        let epoch = std::time::Instant::now();
        move || epoch.elapsed()
    };

    let mut game = Game::new(ctx, now(), width as _, height as _)?;

    event_loop.run(move |event, _, cf| {
        use glutin::{event::*, event_loop::ControlFlow};
        match event {
            Event::NewEvents(_) => {}
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::Resized(size) => {
                    game.handle_resize(size.width as _, size.height as _);
                }
                WindowEvent::CloseRequested => {
                    *cf = ControlFlow::Exit;
                }
                // WindowEvent::KeyboardInput {
                //     input:
                //     KeyboardInput {
                //         state,
                //         virtual_keycode: Some(key_code),
                //         ..
                //     },
                //     ..
                // } => game.handle_key_event(state, key_code),
                WindowEvent::MouseInput { state, button, .. } => {
                    game.handle_mouse_event(MouseEvent::Button(state, button));
                }
                WindowEvent::CursorMoved { position, .. } => {
                    game.handle_mouse_event(MouseEvent::Moved(position.x as _, position.y as _));
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
                game.update(now());
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
