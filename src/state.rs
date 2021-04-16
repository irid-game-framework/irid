//= USES ===========================================================================================

use std::iter;

use futures::executor::block_on;

use winit::{
	window::Window,
	event::WindowEvent,
};


//= STATE STRUCT AND IMPL ==========================================================================

pub struct State {
	surface: wgpu::Surface,
	device: wgpu::Device,
	queue: wgpu::Queue,
	swap_chain_desc: wgpu::SwapChainDescriptor,
	swap_chain: wgpu::SwapChain,
	size: winit::dpi::PhysicalSize<u32>,
	clear_color: wgpu::Color,
}


impl State {
	pub fn new(window: &Window) -> Self {
		let size = window.inner_size();

		// The instance is a handle to our GPU
		// BackendBit::PRIMARY => Vulkan + Metal + DX12 + Browser WebGPU
		let instance = wgpu::Instance::new(wgpu::BackendBit::PRIMARY);
		let surface = unsafe { instance.create_surface(window) };
		let adapter = block_on(async {
			instance.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::HighPerformance,
				compatible_surface: Some(&surface),
			}).await
		}).unwrap();

		let (device, queue) = block_on(async {
			adapter.request_device(
				&wgpu::DeviceDescriptor {
					label: None,
					features: wgpu::Features::empty(),
					limits: wgpu::Limits::default(),
				},
				None, // Trace path
			).await
		}).unwrap();

		let swap_chain_desc = wgpu::SwapChainDescriptor {
			usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
			format: adapter.get_swap_chain_preferred_format(&surface),
			width: size.width,
			height: size.height,
			present_mode: wgpu::PresentMode::Fifo,
		};
		let swap_chain = device.create_swap_chain(&surface, &swap_chain_desc);

		let clear_color = wgpu::Color::BLACK;

		Self {
			surface,
			device,
			queue,
			swap_chain_desc,
			swap_chain,
			size,
			clear_color,
		}
	}

	pub fn refresh_size(&mut self) {
		self.swap_chain_desc.width = self.size.width;
		self.swap_chain_desc.height = self.size.height;
		self.swap_chain = self.device.create_swap_chain(&self.surface, &self.swap_chain_desc);
	}

	pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
		self.size = new_size;
		self.refresh_size();
	}

	#[allow(unused_variables)]
	pub fn input(&mut self, event: &WindowEvent) -> bool {
		match event {
			WindowEvent::CursorMoved { position, .. } => {
				self.clear_color = wgpu::Color {
					r: position.x as f64 / self.size.width as f64,
					g: position.y as f64 / self.size.height as f64,
					b: 1.0,
					a: 1.0,
				};
				true
			}
			_ => false,
		}
	}

	pub fn update(&mut self) {}

	pub fn render(&mut self) -> Result<(), wgpu::SwapChainError> {
		let frame = self.swap_chain.get_current_frame()?.output;

		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("Render Encoder"),
			});

		{
			let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("Render Pass"),
				color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
					attachment: &frame.view,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(self.clear_color),
						store: true,
					},
				}],
				depth_stencil_attachment: None,
			});
		}

		self.queue.submit(iter::once(encoder.finish()));

		Ok(())
	}
}