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
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    log::info!("Starting Rust Vulkan Raytracing Demo");
    log::info!("Platform: {}", std::env::consts::OS);

    let event_loop = EventLoop::new()?;
    let window = WindowBuilder::new()
        .with_title("Rust Vulkan Raytracing Demo")
        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))
        .build(&event_loop)?;

    window.set_cursor_visible(false);
    if let Err(_) = window.set_cursor_grab(winit::window::CursorGrabMode::Locked) {
         let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
    }

    log::info!("Initializing Vulkan renderer...");
    let mut renderer = match Renderer::new(&window) {
        Ok(r) => {
            log::info!("Renderer initialized successfully");
            r
        }
        Err(e) => {
            log::error!("Failed to initialize renderer: {}", e);

            // Special handling for common errors
            if e.to_string().contains("INCOMPATIBLE_DRIVER") {
                log::error!("\nThis error typically means:");
                log::error!("  - On macOS: Native Vulkan is not supported. You need MoltenVK.");
                log::error!("  - On Linux/Windows: GPU drivers are outdated or incompatible.");
                log::error!("  - Ray tracing extensions may not be supported by your GPU.");
            } else if e.to_string().contains("OUT_OF_DEVICE_MEMORY") ||
                      e.to_string().contains("OUT_OF_HOST_MEMORY") {
                log::error!("\nMemory allocation failed. Possible causes:");
                log::error!("  - GPU does not have enough VRAM for ray tracing structures");
                log::error!("  - Integrated GPU was selected instead of discrete GPU");
                log::error!("  - Memory fragmentation or other applications using VRAM");
                log::error!("  - Try closing other GPU-intensive applications");
            }

            return Err(e);
        }
    };

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
