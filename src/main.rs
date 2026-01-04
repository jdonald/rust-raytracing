mod vulkan;
mod renderer;
mod camera;
mod scene;

use winit::{
    event::{Event, WindowEvent, KeyEvent, DeviceEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    keyboard::{PhysicalKey},
};
use renderer::Renderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let event_loop = EventLoop::new()?;
    let window = WindowBuilder::new()
        .with_title("Rust Vulkan Raytracing Demo")
        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))
        .build(&event_loop)?;

    window.set_cursor_visible(false);
    if let Err(_) = window.set_cursor_grab(winit::window::CursorGrabMode::Locked) {
         let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
    }

    let mut renderer = Renderer::new(&window)?;

    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::Resized(size) => {
                    renderer.resize(size.width, size.height);
                }
                WindowEvent::KeyboardInput { event: KeyEvent { physical_key: PhysicalKey::Code(key), state, .. }, .. } => {
                    renderer.handle_input(key, state);
                }
                WindowEvent::RedrawRequested => {
                    if let Err(e) = renderer.render(&window) {
                        log::error!("Render error: {}", e);
                        elwt.exit();
                    }
                }
                _ => {
                    renderer.handle_window_event(&event);
                }
            },
            Event::AboutToWait => {
                window.request_redraw();
            }
            Event::DeviceEvent { event: DeviceEvent::MouseMotion { delta }, .. } => {
                renderer.camera.handle_mouse_input(delta.0, delta.1);
            }
            _ => (),
        }
    })?;

    Ok(())
}
