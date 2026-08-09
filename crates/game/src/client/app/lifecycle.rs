use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::client::state::GameState;

use super::{bootstrap::initialize_renderer, App, WINDOW_TITLE};

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            log::debug!("ignoring resume event because the window already exists");
            return;
        }
        let startup_started = std::time::Instant::now();
        log::info!("creating 1280x720 application window");
        let attrs = WindowAttributes::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 720u32));
        let window = event_loop
            .create_window(attrs)
            .expect("Failed to create window");
        // Set the window icon from assets/icon.ico
        if let Ok(img) = image::load_from_memory(include_bytes!("../../../../../assets/icon.ico")) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            if let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), w, h) {
                window.set_window_icon(Some(icon));
            }
        }
        let window_size = window.inner_size();
        log::info!(
            "window created: physical_size={}x{}, scale_factor={:.3}",
            window_size.width,
            window_size.height,
            window.scale_factor()
        );
        // IME is enabled/disabled per-input (chat, sign editor, text fields)
        // to avoid showing the candidate window during gameplay.
        window.set_ime_allowed(false);
        let renderer = initialize_renderer(&window);
        self.renderer = Some(renderer);
        self.window = Some(window);
        self.reload_assets();
        log::info!(
            "window and renderer initialised in {:.2} ms",
            startup_started.elapsed().as_secs_f64() * 1000.0
        );
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("window close requested; shutting down client systems");
                // Drop the renderer before the window so backend surface resources
                // are destroyed while the window handle is still valid.
                self.renderer = None;
                // Also drop the network connection so the server is notified.
                self.connection = None;
                self.scripts.set_connection_active(false);
                self.scripts.shutdown();
                event_loop.exit();
            }
            WindowEvent::Focused(false) => {
                // Auto-pause when window loses focus while in-game (not in a menu)
                if matches!(self.state, GameState::Playing)
                    && self.mouse_captured
                    && !self.chat_open
                    && !self.inventory_open
                {
                    self.state = GameState::Paused;
                    self.mouse_captured = false;
                    self.set_cursor_captured(false);
                }
            }
            WindowEvent::Resized(size) => {
                self.player.update_aspect(size.width, size.height);
                if let Some(renderer) = &mut self.renderer {
                    renderer.notify_resize(size.width, size.height);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x;
                self.mouse_y = position.y;
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_gui_mouse_pos(position.x as f32, position.y as f32);
                }
                self.update_active_slider();
                self.update_creative_scroll_drag();
                if let Some(window) = &self.window {
                    // Slider drags must be visible immediately, independent of
                    // the frame limiter's next scheduled redraw.
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => self.handle_keyboard_input(event_loop, key_event),
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                self.handle_ime_commit(&text);
            }
            WindowEvent::MouseWheel { delta, .. } => self.handle_mouse_wheel(delta),
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_input(event_loop, button, state.is_pressed());
            }
            WindowEvent::DroppedFile(path) => {
                self.handle_dropped_file(path);
            }
            WindowEvent::RedrawRequested => self.handle_redraw(event_loop),
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if self.mouse_captured && matches!(self.state, GameState::Playing) {
            if let DeviceEvent::MouseMotion { delta } = event {
                self.mouse_dx += delta.0;
                self.mouse_dy += delta.1;
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Menus without a world render near-empty frames; uncapped they
        // busy-spin the CPU and GPU and starve the rest of the desktop
        // (IMMEDIATE present mode has no vsync backpressure). Vanilla also
        // limits the framerate while no world is loaded.
        let menu_cap = if self.state.has_world_background() {
            u32::MAX
        } else {
            60
        };
        let configured = if self.config.max_framerate >= crate::client::config::UNLIMITED_FRAMERATE
        {
            u32::MAX
        } else {
            self.config.max_framerate.max(30)
        };
        let cap = configured.min(menu_cap);
        if cap == u32::MAX {
            event_loop.set_control_flow(ControlFlow::Poll);
            self.last_redraw_request = std::time::Instant::now();
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            return;
        }
        let frame_interval = std::time::Duration::from_secs_f64(1.0 / cap as f64);
        let now = std::time::Instant::now();
        let next_redraw = self.last_redraw_request + frame_interval;
        if now < next_redraw {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_redraw));
            return;
        }
        event_loop.set_control_flow(ControlFlow::Poll);
        self.last_redraw_request = now;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

pub fn run() -> i32 {
    log::debug!("creating winit event loop");
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = App::new();
    log::info!("entering application event loop");
    match event_loop.run_app(&mut app) {
        Ok(()) => {
            log::info!("application event loop exited normally");
            0
        }
        Err(e) => {
            log::error!("application event loop failed: {e}");
            1
        }
    }
}
