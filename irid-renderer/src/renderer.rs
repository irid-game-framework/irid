//= USES ===========================================================================================

use std::marker::PhantomData;

use bytemuck::Pod;
use thiserror::Error;

use irid_assets::{DiffuseImageSize, DiffuseTexture, ImageSize, Texture, Vertex};
use irid_utils::log2;

use crate::{Adapter, Camera, CameraController, CameraMetadatas, Device, FragmentStateBuilder,
            Instance, InstanceRaw, PipelineLayoutBuilder, Queue, RenderPipeline,
            RenderPipelineBuilder, Surface, VertexStateBuilder};
use crate::texture_metadatas::{TextureBindGroupMetadatas, TextureDepthMetadatas, TextureImageMetadatas};

//= ERRORS =========================================================================================

///
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum RendererError {
    #[error("unable to get a Surface or Adapter")]
    SurfaceAdapterRequest,
    #[error("unable to get a Device")]
    DeviceRequest {
        #[from]
        source: wgpu::RequestDeviceError,
    },
}

//= CONSTS =========================================================================================

const NUM_INSTANCES_PER_ROW: u32 = 10;
const INSTANCE_DISPLACEMENT: cgmath::Vector3<f32> = cgmath::Vector3::new(
    NUM_INSTANCES_PER_ROW as f32 * 0.5,
    0.0,
    NUM_INSTANCES_PER_ROW as f32 * 0.5
);

//= RENDERER BUILDER ===============================================================================

///
#[derive(Clone, Debug)]
pub struct RendererBuilder<
    'a,
    P: AsRef<std::path::Path>,
    V: Vertex<'a> + Pod,
    S: ImageSize = DiffuseImageSize,
    T: Texture<S> = DiffuseTexture
> {
    window: &'a winit::window::Window,

    clear_color: Option<wgpu::Color>,
    shader_module: Option<&'a wgpu::ShaderModule>,
    texture_path: Option<P>,
    vertices: Option<&'a [V]>,  // TODO: Probably better to encapsulate the [V] logic
    indices: Option<&'a [u32]>,

    generic_size: PhantomData<S>,
    generic_texture: PhantomData<T>,
}

impl<'a, P, V, S, T> RendererBuilder<'a, P, V, S, T> where
    P: AsRef<std::path::Path>,
    V: Vertex<'a> + Pod,
    S: ImageSize,
    T: Texture<S> {
    //- Constructors -------------------------------------------------------------------------------

    ///
    pub fn new(window: &'a winit::window::Window) -> Self {
        Self {
            window,
            clear_color: None,
            shader_module: None,
            texture_path: None,
            vertices: None,
            indices: None,
            generic_size: Default::default(),
            generic_texture: Default::default()
        }
    }

    //- Setters ------------------------------------------------------------------------------------

    ///
    pub fn with_window(mut self, window: &'a winit::window::Window) -> Self {
        self.window = window;
        self
    }

    /// Color used by a [render pass color attachment](wgpu::RenderPassColorAttachment)
    /// to perform a [clear operation](wgpu::LoadOp).
    pub fn with_clear_color(mut self, clear_color: wgpu::Color) -> Self {
        self.clear_color = Some(clear_color);
        self
    }

    ///
    pub fn with_shader_module(mut self, shader_module: &'a wgpu::ShaderModule) -> Self {
        self.shader_module = Some(shader_module);
        self
    }

    ///
    pub fn with_texture_path(mut self, texture_path: P) -> Self {
        self.texture_path = Some(texture_path);
        self
    }

    ///
    pub fn with_vertices(mut self, vertices: &'a [V]) -> Self {
        self.vertices = Some(vertices);
        self
    }

    ///
    pub fn with_indices(mut self, indices: &'a [u32]) -> Self {
        self.indices = Some(indices);
        self
    }

    //- Build --------------------------------------------------------------------------------------

    ///
    pub fn build(self) -> Result<Renderer<'a>, RendererError> {
        //- Surface, Device, Queue -----------------------------------------------------------------

        let window_size = self.window.inner_size();

        let backends = wgpu::Backends::VULKAN | wgpu::Backends::DX12;  // TODO: choosable by user
        let (surface, adapter) = Surface::new(backends, self.window, window_size)
            .map_err(|_| RendererError::SurfaceAdapterRequest)?;  // TODO: probably better pass e as argument to SurfaceAdapterRequest for chaining error description

        let (device, queue) = pollster::block_on(Device::new(&adapter))?;

        surface.configure(&device);

        //- Camera ---------------------------------------------------------------------------------

        let camera = Camera::new(window_size.width as f32, window_size.height as f32);
        let camera_metadatas = camera.create_metadatas(&device);
        let camera_controller = CameraController::new(0.2);

        //- Texture Metadatas ----------------------------------------------------------------------

        let texture_image_metadatas = self.create_texture_image_metadatas(
            &device,
            surface.preferred_format()
        );

        let texture_bind_group_metadatas = self.create_texture_bind_group_metadatas(
            &device,
            &texture_image_metadatas,
        );

        let texture_depth_metadatas = TextureDepthMetadatas::new(&device, window_size);

        //- Pipeline -------------------------------------------------------------------------------

        let vertex = self.shader_module.map(
            |sm| self.create_vertex_state(sm)
        );

        let texture_bgl = texture_bind_group_metadatas[8][8].bind_group_layout();  // TODO: 256x256 texture, hardcoded for now :(
        let camera_bgl = camera_metadatas.bind_group_layout();
        let pipeline_layout = PipelineLayoutBuilder::new()
            .with_bind_group_layouts(&[texture_bgl, camera_bgl])
            .build(&device);

        let fragment = self.shader_module.map(
            |sm| self.create_fragment_state(sm, surface.preferred_format())
        );

        let mut renderer_pipeline_builder = RenderPipelineBuilder::new(vertex.unwrap())  // TODO: uhm... unwrap...
            .with_layout(&pipeline_layout);
        if fragment.is_some() {
            renderer_pipeline_builder = renderer_pipeline_builder.with_fragment(fragment.unwrap());
        }
        let renderer_pipeline = renderer_pipeline_builder.build::<V>(&device);

        //- Queue Schedule -------------------------------------------------------------------------

        if self.texture_path.is_some() {
            // TODO: here we use unwrap because texture loading will probably not be done at this point and therefore it is useless to add a new type of error
            queue.write_texture(&texture_image_metadatas, T::load(self.texture_path.unwrap()).unwrap());
        }

        //- Vertex and Index Buffers ---------------------------------------------------------------

        let vertex_buffer = self.vertices.map(
            |v| device.create_vertex_buffer_init("Vertex Buffer", v)
        );

        let index_buffer = self.indices.map(
            |i| device.create_indices_buffer_init("Index Buffer", i)
        );
        let num_indices = if self.indices.is_some() {
            self.indices.unwrap().len() as u32
        } else {
            0_u32
        };

        //- Instances ------------------------------------------------------------------------------

        let instances = self.vertices.map(
            |_| self.create_instances()
        );
        let instance_buffer = self.vertices.map(
            |_| self.create_instances_buffer(&device,&instances.unwrap())
        );

        //- Renderer Creation ----------------------------------------------------------------------

        Ok(Renderer {
            window_size,
            clear_color: self.clear_color.unwrap_or(wgpu::Color::WHITE),
            surface,
            adapter,
            device,
            queue,

            camera,
            camera_metadatas,
            camera_controller,

            texture_image_metadatas,
            texture_bind_group_metadatas,
            texture_depth_metadatas,

            shader_module: self.shader_module,
            pipeline_layout,
            renderer_pipeline,
            vertex_buffer,
            index_buffer,
            num_indices,
            instances,
            instance_buffer
        })
    }

    ///
    ///
    /// It can't cache zero sized textures.
    pub fn create_texture_image_metadatas(
        &self,
        device: &Device,
        preferred_format: wgpu::TextureFormat
    ) -> Vec<Vec<TextureImageMetadatas>> {
        // Better to check not the current limits but the default ones
        // so as to obtain consistent behavior on all devices.
        let qty = log2(wgpu::Limits::default().max_texture_dimension_2d as i32) as usize;
        let mut vec_w = Vec::<Vec<TextureImageMetadatas>>::with_capacity(qty);
        for width in 0..=qty {
            let mut vec_h = Vec::<TextureImageMetadatas>::with_capacity(qty);
            for height in 0..=qty {
                vec_h[height] = TextureImageMetadatas::new(
                    &device,
                    preferred_format,
                    2_u32.pow(width as u32),
                    2_u32.pow(height as u32),
                );
            }
            vec_w[width] = vec_h;
        }
        vec_w
    }

    ///
    pub fn create_texture_bind_group_metadatas(
        &self,
        device: &Device,
        texture_image_metadatas: &Vec<Vec<TextureImageMetadatas>>,
    ) -> Vec<Vec<TextureBindGroupMetadatas>> {
        let qty= texture_image_metadatas.len();
        let mut vec_w = Vec::<Vec<TextureBindGroupMetadatas>>::with_capacity(qty);
        for width in 0..=qty {
            let mut vec_h = Vec::<TextureBindGroupMetadatas>::with_capacity(qty);
            for height in 0..=qty {
                vec_h[height] = TextureBindGroupMetadatas::new(
                    &device,
                    &texture_image_metadatas[width][height].texture()
                );
            }
            vec_w[width] = vec_h;
        }
        vec_w
    }

    fn create_instances(&self) -> Vec<Instance> {
        (0..NUM_INSTANCES_PER_ROW).flat_map(|z| {
            use cgmath::{Zero, Rotation3, InnerSpace};

            (0..NUM_INSTANCES_PER_ROW).map(move |x| {
                let position =
                    cgmath::Vector3 { x: x as f32, y: 0.0, z: z as f32 } - INSTANCE_DISPLACEMENT;

                let rotation = if position.is_zero() {
                    // this is needed so an object at (0, 0, 0) won't get scaled to zero
                    // as Quaternions can effect scale if they're not created correctly
                    cgmath::Quaternion::from_axis_angle(cgmath::Vector3::unit_z(),
                                                        cgmath::Rad(0.0f32))
                } else {
                    cgmath::Quaternion::from_axis_angle(position.normalize(),
                                                        cgmath::Rad(std::f32::consts::PI / 4.0f32))
                };

                Instance {
                    position,
                    rotation,
                }
            })
        }).collect::<Vec<_>>()
    }

    fn create_instances_buffer(&self, device: &Device, instances: &Vec<Instance>) -> wgpu::Buffer {
        let instance_data = instances.iter().map(Instance::to_raw)
            .collect::<Vec<_>>();

        // TODO: when we will create the generics about Vertices we will use the Device.create_vertex_buffer_init instead
        device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Instance Buffer"),
                contents: bytemuck::cast_slice(&instance_data),
                usage: wgpu::BufferUsages::VERTEX,
            }
        )
    }

    fn create_vertex_state(&self, shader_module: &'a wgpu::ShaderModule) -> wgpu::VertexState<'a> {
        let buffers = [V::desc(), InstanceRaw::desc()];  // TODO: the instances must be optional
        VertexStateBuilder::new(shader_module)
            .with_buffers(&buffers)
            .build()
    }

    fn create_fragment_state(
        &self,
        shader_module: &'a wgpu::ShaderModule,
        preferred_format: wgpu::TextureFormat,
    ) -> wgpu::FragmentState<'a> {
        let targets = [wgpu::ColorTargetState {
            format: preferred_format,  //.unwrap_or(wgpu::TextureFormat::Rgba16Float),
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent::REPLACE,
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            write_mask: wgpu::ColorWrites::ALL,
        }];
        FragmentStateBuilder::new(shader_module)
            .with_targets(&targets)
            .build()
    }
}

//= RENDERER OBJECT ================================================================================

///
pub struct Renderer<'a> {
    window_size: winit::dpi::PhysicalSize<u32>,
    clear_color: wgpu::Color,
    surface: Surface,
    #[allow(dead_code)]
    adapter: Adapter,
    device: Device,
    queue: Queue,

    camera: Camera,
    camera_metadatas: CameraMetadatas,
    camera_controller: CameraController,

    texture_image_metadatas: Vec<Vec<TextureImageMetadatas>>,
    texture_bind_group_metadatas: Vec<Vec<TextureBindGroupMetadatas>>,
    texture_depth_metadatas: TextureDepthMetadatas,

    shader_module: Option<&'a wgpu::ShaderModule>,
    pipeline_layout: wgpu::PipelineLayout,
    renderer_pipeline: RenderPipeline,  // TODO: probably also optional?
    vertex_buffer: Option<wgpu::Buffer>,  // TODO: maybe this is better to move, this buffer, and the index buffer, inside the render_pass or pipeline object
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    instances: Option<Vec<Instance>>,
    instance_buffer: Option<wgpu::Buffer>,
}

impl<'a> Renderer<'a> {
    //- Surface (Re)size ---------------------------------------------------------------------------

    /// Getter for the windows's physical size attribute.
    pub fn get_size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.window_size
    }

    /// Calculate the aspect ratio of the window's inner size.
    pub fn calc_aspect_ratio(&self) -> f32 {
        self.window_size.width as f32 / self.window_size.height as f32
    }

    /// Resize the renderer window.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.window_size = new_size;
        self.texture_depth_metadatas = TextureDepthMetadatas::new(&self.device, self.window_size);
        self.refresh_current_size();
    }

    ///
    pub fn refresh_current_size(&mut self) {
        self.surface.update(&self.device, self.window_size);
    }

    //- Camera -------------------------------------------------------------------------------------

    ///
    pub fn process_camera_events(&mut self, input: &winit::event::KeyboardInput) -> bool {
        self.camera_controller.process_events(input)
    }

    //- Command Encoder ----------------------------------------------------------------------------

    ///
    pub fn create_command_encoder(&self, label_text: &str) -> wgpu::CommandEncoder {
        self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some(label_text),
            }
        )
    }

    //- Rendering ----------------------------------------------------------------------------------

    ///
    pub fn redraw(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.camera_controller.update_camera(&mut self.camera);
        self.queue.write_camera_buffer(&self.camera, &self.camera_metadatas);

        let frame = self.surface.get_current_texture()?;
        let texture = &frame.texture;
        let frame_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            format: None,
            dimension: None,
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None
        });

        let mut encoder = self.create_command_encoder("Render Encoder");

        {
            let mut render_pass = encoder.begin_render_pass(
                &wgpu::RenderPassDescriptor {
                    label: Some("Render Pass"),
                    color_attachments: &[wgpu::RenderPassColorAttachment {
                        view: &frame_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(self.clear_color),
                            store: true,
                        },
                    }],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: self.texture_depth_metadatas.view(),
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: true,
                        }),
                        stencil_ops: None,
                    }),
                }
            );

            render_pass.set_pipeline(self.renderer_pipeline.expose_wrapped_render_pipeline());  // TODO: to remove this expose call creating an RenderPass wrapper
            render_pass.set_bind_group(0, self.texture_bind_group_metadatas[8][8].bind_group(), &[]);  // TODO: hardcoded :(
            render_pass.set_bind_group(1, self.camera_metadatas.bind_group(), &[]);
            if self.vertex_buffer.is_some() {
                render_pass.set_vertex_buffer(0, self.vertex_buffer.unwrap().slice(..));
            }
            if self.instance_buffer.is_some() {
                render_pass.set_vertex_buffer(1, self.instance_buffer.unwrap().slice(..));
            }
            if self.index_buffer.is_some() {
                render_pass.set_index_buffer(
                    self.index_buffer.unwrap().slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                render_pass.draw_indexed(
                    0..self.num_indices,
                    0,
                    0..self.instances.unwrap().len() as _,
                );
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }
}
