//= USES ===========================================================================================

use std::fmt::Debug;
use std::path::{Path, PathBuf};

use thiserror::Error;
use winit::window::Fullscreen;

use irid_assets::{DiffuseImageSize, DiffuseTexture, ModelVertex};
use irid_renderer::{Renderer, RendererBuilder, RendererError};

use crate::{ApplicationConfig, Listener};

//= ERRORS =========================================================================================

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("can’t identify any monitor as a primary one")]
    PrimaryMonitorNotFound,
    #[error("the OS cannot perform the requested operation")]
    WindowOsError {
        #[from] source: winit::error::OsError,
    },
    #[error("the Renderer cannot be built")]
    RendererError {
        #[from] source: RendererError,
    },
}

//= APPLICATION BUILDER ============================================================================

/// Build a new [Application] with wanted values.
#[derive(Debug)]
pub struct ApplicationBuilder<'a, L: Listener, P: AsRef<Path>> {
    listener: L,
    config: Option<ApplicationConfig>,
    title: Option<String>,

    // Renderer stuff
    shader_paths: Option<Vec<P>>,
    texture_path: Option<P>,
    vertices: Option<&'a [ModelVertex]>,
    indices: Option<&'a [u32]>,

    // Renderer specific options
    clear_color: Option<wgpu::Color>,
}

impl<'a, L: Listener, P> ApplicationBuilder<'a, L, P>
    where P : AsRef<std::path::Path>
{
    //- Constructors -------------------------------------------------------------------------------

    ///
    pub fn new(listener: L) -> Self {
        Self {
            listener,
            config: None,
            title: None,
            shader_paths: None,
            texture_path: None,
            vertices: None,
            indices: None,
            clear_color: None
        }
    }

    //- Setters ------------------------------------------------------------------------------------

    ///
    pub fn with_listener(mut self, listener: L) -> Self {
        self.listener = listener;
        self
    }

    ///
    pub fn with_config_path(mut self, filepath: P) -> Self {
        self.config = Some(ApplicationConfig::new(filepath));
        self
    }

    ///
    pub fn with_config<AC: Into<Option<ApplicationConfig>>>(mut self, config: AC) -> Self {
        self.config = config.into();
        self
    }

    /// Set the window title.
    pub fn with_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    /// TODO "I have to refactor all the assets and pipeline management"
    pub fn with_shader_paths(mut self, shader_paths: Vec<P>) -> Self {
        self.shader_paths = Some(shader_paths);
        self
    }

    /// TODO "I have to refactor all the assets and pipeline management"
    pub fn with_texture_path(mut self, texture_path: P) -> Self {
        self.texture_path = Some(texture_path);
        self
    }

    /// TODO "I have to refactor all the assets and pipeline management"
    pub fn with_vertices(mut self, vertices: &'a [ModelVertex]) -> Self {
        self.vertices = Some(vertices);
        self
    }

    /// TODO "I have to refactor all the assets and pipeline management"
    pub fn with_indices(mut self, indices: &'a [u32]) -> Self {
        self.indices = Some(indices);
        self
    }

    /// Color used by a [render pass color attachment](wgpu::RenderPassColorAttachment)
    /// to perform a [clear operation](wgpu::LoadOp).
    pub fn with_clear_color(mut self, clear_color: wgpu::Color) -> Self {
        self.clear_color = Some(clear_color);
        self
    }

    //- Build --------------------------------------------------------------------------------------

    /// Build a new [Application] with given values.
    // TODO I have to manage the Nones values for every unwrap
    pub fn build(self) -> Application<'a, L, P> {
        Application {
            listener: self.listener,
            config: self.config.unwrap_or_default(),
            title: self.title.unwrap_or_else(|| String::from("Irid Application")),
            shader_paths: self.shader_paths,
            texture_path: self.texture_path,
            vertices: self.vertices,
            indices: self.indices,
            clear_color: self.clear_color,
        }
    }
}

//= APPLICATION ====================================================================================

/// Manages the whole game setup and logic.
#[derive(Debug)]
pub struct Application<'a, L: Listener, P: AsRef<Path>> {
    listener: L,
    config: ApplicationConfig,
    title: String,

    // Renderer stuffs
    shader_paths: Option<Vec<P>>,
    texture_path: Option<P>,
    vertices: Option<&'a [ModelVertex]>,
    indices: Option<&'a [u32]>,

    // Renderer specific options
    clear_color: Option<wgpu::Color>,
}

impl<'a, L, P> Application<'a, L, P> where
    L: Listener,
    P: AsRef<std::path::Path> + Clone + Debug
{
    /// Starts the
    /// [event loop](https://docs.rs/winit/0.25.0/winit/event_loop/struct.EventLoop.html).
    ///
    /// # Notes:
    ///
    /// The event loop uses the winit
    /// [run_return](https://docs.rs/winit/0.25.0/winit/platform/run_return/trait.EventLoopExtRunReturn.html#tymethod.run_return)
    /// method.
    ///
    /// I know I should use winit
    /// [run](https://docs.rs/winit/0.25.0/winit/event_loop/struct.EventLoop.html#method.run)
    /// method instead but all those static variables are a bore to handle.
    ///
    /// To remember that the resize is not managed perfectly with run_return.
    pub fn start(self) -> Result<(), ApplicationError> {
        let mut event_loop = winit::event_loop::EventLoop::new();
        let primary_monitor = event_loop.primary_monitor()
            .ok_or(ApplicationError::PrimaryMonitorNotFound)?;  // TODO: Wayland return always None, maybe we have to handle it here instead of returning an error

        // The window starts with visibility set to false because just before maximizing it,
        // for a moment, a window with the size set in the inner_size values is displayed,
        // generating an unpleasant flickering visual effect.
        let window = winit::window::WindowBuilder::new()
            .with_inner_size(self.config.window_inner_size())
            .with_min_inner_size(self.config.window_min_inner_size())
            .with_resizable(true)
            .with_title(&self.title)
            .with_visible(false)
            //.with_window_icon() // TODO: because yes!
            .build(&event_loop)?;
        window.set_maximized(false);

        let mut renderer_builder = RendererBuilder::<P, DiffuseImageSize, DiffuseTexture>::new(&window);
        if self.clear_color.is_some() {
            renderer_builder = renderer_builder.with_clear_color(self.clear_color);  // TODO: no, we have to have the with_clear_color only on RenderBuilder and not also in ApplicationBuilder, so we can ride with this unwrap
        }
        if self.shader_paths.is_some() {
            renderer_builder = renderer_builder.with_shader_path(self.shader_paths.as_ref().map(|v| v.as_slice()[0].clone()).unwrap());  // TODO: remove unwrap and clone
        }
        if self.texture_path.is_some() {
            renderer_builder = renderer_builder.with_texture_path(self.texture_path.as_ref().unwrap().clone());  // TODO: remove the clone
        }
        if self.vertices.is_some() {
            renderer_builder = renderer_builder.with_vertices(self.vertices.unwrap());
        }
        if self.indices.is_some() {
            renderer_builder = renderer_builder.with_indices(self.indices.unwrap());
        }
        let mut renderer = renderer_builder.build()?;

        // It is preferable to maximize the windows after the surface and renderer setup,
        // but is not mandatory.
        // TODO: Vulkan issue https://github.com/gfx-rs/wgpu/issues/1958 gives false positives
        //if self.config.window_starts_maximized() {
            /*for vm in primary_monitor.video_modes() {
                println!("{:?}", vm);
            }*/
            //let video_mode = primary_monitor.video_modes().nth(0).unwrap();

            window.set_fullscreen(Some(Fullscreen::Exclusive(video_mode)));  // TODO: doesn't work the ALT+TAB on Windows 10
            //window.set_fullscreen(Some(Fullscreen::Borderless(Some(primary_monitor))));
        //}

        // Now is a good time to make the window visible, lessening the flicker explained above,
        // on WindowsBuilder lines.
        // TODO: check if there's another place inside one event below, maybe resized?
        window.set_visible(true);

        use winit::platform::run_return::EventLoopExtRunReturn;
        event_loop.run_return(move |event, _, control_flow| match event {
            winit::event::Event::NewEvents(start_cause) => {
                self.on_new_events(start_cause);
            },

            winit::event::Event::WindowEvent {
                event: window_event,
                window_id,
            } => if window_id == window.id() {  // TODO: multi-monitor support
                match window_event {
                    winit::event::WindowEvent::Resized(physical_size) => {
                        self.on_window_resize(&mut renderer, physical_size);
                    },

                    winit::event::WindowEvent::Moved(physical_position) => {
                        self.on_window_move(physical_position);
                    },

                    winit::event::WindowEvent::CloseRequested => {
                        self.on_window_close(control_flow);
                    },

                    winit::event::WindowEvent::Destroyed => {
                        self.on_window_destroy();
                    },

                    winit::event::WindowEvent::DroppedFile(path) => {
                        self.on_window_drop_file(path);
                    },

                    winit::event::WindowEvent::HoveredFile(path) => {
                        self.on_window_hover_file(path);
                    },

                    winit::event::WindowEvent::HoveredFileCancelled => {
                        self.on_window_hover_file_cancelled();
                    },

                    winit::event::WindowEvent::ReceivedCharacter(c) => {
                        self.on_window_receive_character(c);
                    },

                    winit::event::WindowEvent::Focused(gained_focus) => {
                        self.on_window_focus(gained_focus);
                    },

                    winit::event::WindowEvent::KeyboardInput {
                        device_id,
                        input,
                        is_synthetic,
                    } => if !is_synthetic && input.virtual_keycode.is_some() {
                        self.on_window_keyboard_input(
                            control_flow,
                            device_id,
                            &mut renderer,
                            &input);
                    },

                    winit::event::WindowEvent::ModifiersChanged(state) => {
                        self.on_window_modifiers_change(state);
                    },

                    winit::event::WindowEvent::CursorMoved {
                        device_id,
                        position ,
                        ..
                    } => {
                        self.on_window_cursor_move(device_id, position);
                    },

                    winit::event::WindowEvent::CursorEntered { device_id } => {
                        self.on_window_cursor_enter(device_id);
                    },

                    winit::event::WindowEvent::CursorLeft { device_id } => {
                        self.on_window_cursor_left(device_id);
                    },

                    winit::event::WindowEvent::MouseWheel {
                        device_id,
                        delta,
                        phase,
                        ..
                    } => {
                        self.on_window_mouse_wheel(device_id, delta, phase);
                    },

                    winit::event::WindowEvent::MouseInput {
                        device_id,
                        state,
                        button,
                        ..
                    } => {
                        self.on_window_mouse_input(device_id, state, button);
                    },

                    winit::event::WindowEvent::TouchpadPressure {
                        device_id,
                        pressure,
                        stage
                    } => {
                        self.on_window_touchpad_pressure(device_id, pressure, stage);
                    },

                    winit::event::WindowEvent::AxisMotion {
                        device_id,
                        axis,
                        value
                    } => {
                        self.on_window_axis_motion(device_id, axis, value);
                    },

                    winit::event::WindowEvent::Touch(touch) => {
                        self.on_window_touch(touch);
                    },

                    // The window's scale factor has changed.
                    winit::event::WindowEvent::ScaleFactorChanged {
                        scale_factor,
                        new_inner_size
                    } => {
                        self.on_window_scale_change(
                            &mut renderer,
                            scale_factor,
                            new_inner_size
                        );
                    },

                    winit::event::WindowEvent::ThemeChanged(theme) => {
                        self.on_window_theme_change(theme);
                    },
                }
            },

            winit::event::Event::DeviceEvent { device_id: _device_id, event: ref _device_event } => {
                // TODO: Currently we don't have to manage it
            },

            winit::event::Event::UserEvent(event) => {
                self.on_user_event(&event);
            },

            winit::event::Event::Suspended => {
                self.on_suspend();
            },

            winit::event::Event::Resumed => {
                self.on_resume();
            },

            winit::event::Event::MainEventsCleared => {
                self.on_redraw(&mut renderer, control_flow);
            },

            winit::event::Event::RedrawRequested(window_id) => {
                self.on_redraw_request(&window_id);
            },

            winit::event::Event::RedrawEventsCleared => {
                self.on_redraw_clear();
            },

            winit::event::Event::LoopDestroyed => {
                self.on_destroy();
            },
        });

        Ok(())
    }

    //- Generic Events -----------------------------------------------------------------------------

    fn on_new_events(&self, start_cause: winit::event::StartCause) {
        let _use_default_behaviour = self.listener.on_new_events(start_cause);
    }

    fn on_user_event(&self, event: &()) {
        let _use_default_behaviour = self.listener.on_user_event(event);
    }

    fn on_suspend(&self) {
        let _use_default_behaviour = self.listener.on_suspend();
    }

    fn on_resume(&self) {
        let _use_default_behaviour = self.listener.on_resume();
    }

    // This method is probably one of the few that must always be inline.
    #[inline(always)]
    fn on_redraw(
        &self,
        renderer: &mut Renderer,
        control_flow: &mut winit::event_loop::ControlFlow
    ) {
        let use_default_behaviour = self.listener.on_redraw();
        if use_default_behaviour {
            match renderer.redraw() {
                Ok(_) => {},
                Err(error) => match error {
                    // These errors should be resolved by the next frame
                    wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Outdated =>
                        eprintln!("{:?}", error),  // TODO: better error messages?

                    // Recreate the swap chain if lost
                    wgpu::SurfaceError::Lost => renderer.refresh_current_size(),

                    // The system is out of memory, we should probably quit
                    wgpu::SurfaceError::OutOfMemory =>
                        *control_flow = winit::event_loop::ControlFlow::Exit,
                }
            }
        }
    }

    fn on_redraw_request(&self, window_id: &winit::window::WindowId) {
        let _use_default_behaviour = self.listener.on_redraw_request(window_id);
    }

    fn on_redraw_clear(&self) {
        let _use_default_behaviour = self.listener.on_redraw_clear();
    }

    fn on_destroy(&self) {
        let _use_default_behaviour = self.listener.on_destroy();
    }

    //- Window Events ------------------------------------------------------------------------------

    fn on_window_resize(
        &self,
        renderer: &mut Renderer,
        physical_size: winit::dpi::PhysicalSize<u32>
    ) {
        let use_default_behaviour = self.listener.on_window_resize(physical_size);
        if use_default_behaviour {
            // TODO I have to choose if I have to to keep this method here or move it to render
            //  or window struct.
            renderer.resize(physical_size);
        }
    }

    fn on_window_move(&self, physical_position: winit::dpi::PhysicalPosition<i32>) {
        let _use_default_behaviour = self.listener.on_window_move(physical_position);
    }

    fn on_window_close(&self, control_flow: &mut winit::event_loop::ControlFlow) {
        let use_default_behaviour = self.listener.on_window_close();
        if use_default_behaviour {
            *control_flow = winit::event_loop::ControlFlow::Exit;
        }
    }

    fn on_window_destroy(&self) {
        let _use_default_behaviour = self.listener.on_window_destroy();
    }

    fn on_window_drop_file(&self, path: PathBuf) {
        let _use_default_behaviour = self.listener.on_window_drop_file(path);
    }

    fn on_window_hover_file(&self, path: PathBuf) {
        let _use_default_behaviour = self.listener.on_window_hover_file(path);
    }

    fn on_window_hover_file_cancelled(&self) {
        let _use_default_behaviour = self.listener.on_window_hover_file_cancelled();
    }

    fn on_window_receive_character(&self, c: char) {
        let _use_default_behaviour = self.listener.on_window_receive_character(c);
    }

    fn on_window_focus(&self, gained_focus: bool) {
        let _use_default_behaviour = self.listener.on_window_focus(gained_focus);
    }

    // Triggered then an user press a key upon this active window.
    // Currently it's preferred to avoid the synthetic event's keys because they
    // works only on Windows and X11 OS; also because the API it's easier and less
    // breakable if used on listener's events.
    // Also then the window is restored (from minimize state) where are
    // input.virtual_keycode KeyboardInput events equals to None.
    fn on_window_keyboard_input(
        &self,
        control_flow: &mut winit::event_loop::ControlFlow,
        device_id: winit::event::DeviceId,
        renderer: &mut Renderer,
        input: &winit::event::KeyboardInput
    ) {
        // First call a generic method to manage the key events
        let use_default_behaviour = self.listener.on_window_keyboard_input(
            device_id,
            input.state,
            input.virtual_keycode.unwrap(),
        );

        // Then check the input's type for default behaviours
        if use_default_behaviour {
            // Check the camera controller
            renderer.process_camera_events(input);  // TODO Enhance this system

            if let winit::event::KeyboardInput {
                    state: winit::event::ElementState::Pressed,
                    virtual_keycode: Some(winit::event::VirtualKeyCode::Escape),
                    ..
                } = input { *control_flow = winit::event_loop::ControlFlow::Exit }
        }
    }

    fn on_window_modifiers_change(&self, state: winit::event::ModifiersState) {
        let _use_default_behaviour = self.listener.on_window_modifiers_change(state);
    }

    fn on_window_cursor_move(
        &self,
        device_id: winit::event::DeviceId,
        position: winit::dpi::PhysicalPosition<f64>
    ) {
        let _use_default_behaviour = self.listener.on_window_cursor_move(device_id, position);
    }

    fn on_window_cursor_enter(&self, device_id: winit::event::DeviceId) {
        let _use_default_behaviour = self.listener.on_window_cursor_enter(device_id);
    }

    fn on_window_cursor_left(&self, device_id: winit::event::DeviceId) {
        let _use_default_behaviour = self.listener.on_window_cursor_left(device_id);
    }

    fn on_window_mouse_wheel(
        &self,
        device_id: winit::event::DeviceId,
        delta: winit::event::MouseScrollDelta,
        phase: winit::event::TouchPhase
    ) {
        let _use_default_behaviour = self.listener.on_window_mouse_wheel(device_id, delta, phase);
    }

    fn on_window_mouse_input(
        &self,
        device_id: winit::event::DeviceId,
        state: winit::event::ElementState,
        button: winit::event::MouseButton
    ) {
        let _use_default_behaviour = self.listener.on_window_mouse_input(device_id, state, button);
    }

    fn on_window_touchpad_pressure(
        &self,
        device_id: winit::event::DeviceId,
        pressure: f32,
        stage: i64
    ) {
        let _use_default_behaviour = self.listener.on_window_touchpad_pressure(
            device_id,
            pressure,
            stage
        );
    }

    fn on_window_axis_motion(
        &self,
        device_id: winit::event::DeviceId,
        axis: u32,
        value: f64
    ) {
        let _use_default_behaviour = self.listener.on_window_axis_motion(device_id, axis, value);
    }

    fn on_window_touch(&self, touch: winit::event::Touch) {
        let _use_default_behaviour = self.listener.on_window_touch(touch);
    }

    fn on_window_scale_change(
        &self,
        renderer: &mut Renderer,
        scale_factor: f64,
        new_inner_size: &mut winit::dpi::PhysicalSize<u32>// TODO Probably I have to pass it without & (also below)
    ) {
        let use_default_behaviour = self.listener.on_window_scale_change(
            scale_factor,
            new_inner_size
        );

        if use_default_behaviour {
            renderer.resize(*new_inner_size);
        }
    }

    fn on_window_theme_change(&self, theme: winit::window::Theme) {
        let _use_default_behaviour = self.listener.on_window_theme_change(theme);
    }
}
