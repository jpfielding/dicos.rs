//! wgpu ray-casting volume renderer.
//!
//! Manages the GPU pipeline: shader compilation, texture upload, bind groups,
//! and per-frame rendering. The fragment shader performs ray marching through
//! a 3D `RGBA16Unorm` texture with transfer function lookup and Phong lighting.

use crate::camera::Camera;
use crate::transfer::{TransferFunction, TRANSFER_SIZE};
use crate::volume::{ThreatBox, Volume};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::util::DeviceExt;

/// Uniform buffer layout matching the WGSL `Uniforms` struct.
/// Must be 16-byte aligned per WebGPU spec.
#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
struct Uniforms {
    cam_pos: [f32; 3],
    fov: f32,
    cam_forward: [f32; 3],
    aspect_ratio: f32,
    cam_right: [f32; 3],
    step_size: f32,
    cam_up: [f32; 3],
    scale_z: f32,
    window_min: f32,
    window_range: f32,
    alpha_scale: f32,
    rescale_intercept: f32,
    density_threshold: f32,
    ambient_intensity: f32,
    diffuse_intensity: f32,
    specular_intensity: f32,
}

/// Rendering quality preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Fast,
    Medium,
    High,
}

/// GPU volume renderer state.
pub struct VolumeRenderer {
    pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    uniform_buffer: wgpu::Buffer,
    line_vertex_buffer: wgpu::Buffer,
    line_vertex_capacity: usize,
    volume_texture: Option<wgpu::Texture>,
    transfer_texture: Option<wgpu::Texture>,
    volume_sampler: wgpu::Sampler,
    transfer_sampler: wgpu::Sampler,

    // Rendering parameters.
    pub window_center: f32,
    pub window_width: f32,
    pub alpha_scale: f32,
    pub rescale_intercept: f32,
    pub density_threshold: f32,
    pub quality: Quality,

    // Lighting.
    pub ambient: f32,
    pub diffuse: f32,
    pub specular: f32,

    // Volume dimensions for scale_z computation.
    dim_x: u32,
    dim_y: u32,
    dim_z: u32,
    voxel_spacing: [f64; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
struct LineVertex {
    pos: [f32; 2],
    color: [f32; 4],
}

impl VolumeRenderer {
    /// Create a new renderer with the given device and output texture format.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("raycast_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("raycast.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raycast_bind_group_layout"),
            entries: &[
                // Uniforms
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Volume 3D texture
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                // Volume sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Transfer function texture (2D 1024x1 to avoid 1D sampling issues)
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Transfer function sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raycast_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("raycast_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay_lines_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay_lines.wgsl").into()),
        });

        const LINE_VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

        let line_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay_lines_pipeline_layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay_lines_pipeline"),
            layout: Some(&line_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &LINE_VERTEX_ATTRIBUTES,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("raycast_uniforms"),
            contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Holds projected 3D threat-box line segments (in NDC).
        // 2 MiB covers thousands of boxes at 12 edges each.
        let line_vertex_capacity = (2 * 1024 * 1024) / std::mem::size_of::<LineVertex>();
        let line_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay_lines_vertex_buffer"),
            size: (line_vertex_capacity * std::mem::size_of::<LineVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let volume_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("volume_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let transfer_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("transfer_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            pipeline,
            line_pipeline,
            bind_group_layout,
            bind_group: None,
            uniform_buffer,
            line_vertex_buffer,
            line_vertex_capacity,
            volume_texture: None,
            transfer_texture: None,
            volume_sampler,
            transfer_sampler,
            window_center: 32768.0,
            window_width: 65536.0,
            alpha_scale: 1.0,
            rescale_intercept: 0.0,
            density_threshold: 0.0,
            quality: Quality::Medium,
            ambient: 0.3,
            diffuse: 0.6,
            specular: 0.3,
            dim_x: 1,
            dim_y: 1,
            dim_z: 1,
            voxel_spacing: [1.0, 1.0, 1.0],
        }
    }

    fn compute_scale_z(&self) -> f32 {
        // Compute Z scale from physical dimensions (voxel count * spacing).
        let phys_x = self.dim_x as f64 * self.voxel_spacing[0];
        let phys_y = self.dim_y as f64 * self.voxel_spacing[1];
        let phys_z = self.dim_z as f64 * self.voxel_spacing[2];
        let phys_xy = phys_x.max(phys_y);
        if phys_xy > 0.0 {
            (phys_z / phys_xy).clamp(0.1, 10.0) as f32
        } else {
            1.0
        }
    }

    fn project_threat_point(
        &self,
        point: Vec3,
        camera: &Camera,
        aspect: f32,
        scale_z: f32,
    ) -> Option<[f32; 2]> {
        // Match the ray shader's camera model exactly:
        // ray_dir = normalize(fwd + right * ux * fov * aspect + up * uy * fov)
        // and note that shader `uy` maps to NDC with inverted Y.
        let world_point = Vec3::new(point.x, point.y, point.z * scale_z);
        let cam_pos = camera.position();
        let to_point = world_point - cam_pos;

        let forward = camera.forward();
        let right = camera.right();
        let up = camera.up();

        let depth = to_point.dot(forward);
        if depth <= 1e-5 {
            return None;
        }

        let denom_x = depth * camera.fov * aspect.max(0.01);
        let denom_y = depth * camera.fov;
        if denom_x.abs() <= 1e-8 || denom_y.abs() <= 1e-8 {
            return None;
        }

        let shader_uv_x = to_point.dot(right) / denom_x;
        let shader_uv_y = to_point.dot(up) / denom_y;
        if !shader_uv_x.is_finite() || !shader_uv_y.is_finite() {
            return None;
        }

        // Shader builds `uv.y` from `1 - y_ndc`; invert to get clip-space Y.
        Some([shader_uv_x, -shader_uv_y])
    }

    fn build_threat_line_vertices(
        &self,
        threats: &[ThreatBox],
        camera: &Camera,
        aspect: f32,
        scale_z: f32,
    ) -> Vec<LineVertex> {
        if threats.is_empty() || self.dim_x == 0 || self.dim_y == 0 || self.dim_z == 0 {
            return Vec::new();
        }

        let dx = (self.dim_x.saturating_sub(1)).max(1) as f32;
        let dy = (self.dim_y.saturating_sub(1)).max(1) as f32;
        let dz = (self.dim_z.saturating_sub(1)).max(1) as f32;

        const EDGES: [(usize, usize); 12] = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];

        let mut vertices = Vec::with_capacity(threats.len() * EDGES.len() * 2);
        for threat in threats {
            if !threat.enabled {
                continue;
            }

            let x0 = (threat.min[0] as f32 / dx).clamp(0.0, 1.0);
            let y0 = (threat.min[1] as f32 / dy).clamp(0.0, 1.0);
            let z0 = (threat.min[2] as f32 / dz).clamp(0.0, 1.0);
            let x1 = (threat.max[0] as f32 / dx).clamp(0.0, 1.0);
            let y1 = (threat.max[1] as f32 / dy).clamp(0.0, 1.0);
            let z1 = (threat.max[2] as f32 / dz).clamp(0.0, 1.0);

            let corners = [
                Vec3::new(x0, y0, z0),
                Vec3::new(x1, y0, z0),
                Vec3::new(x1, y1, z0),
                Vec3::new(x0, y1, z0),
                Vec3::new(x0, y0, z1),
                Vec3::new(x1, y0, z1),
                Vec3::new(x1, y1, z1),
                Vec3::new(x0, y1, z1),
            ];

            let color = [
                threat.color[0] as f32 / 255.0,
                threat.color[1] as f32 / 255.0,
                threat.color[2] as f32 / 255.0,
                1.0,
            ];

            for (a, b) in EDGES {
                let Some(pa) = self.project_threat_point(corners[a], camera, aspect, scale_z)
                else {
                    continue;
                };
                let Some(pb) = self.project_threat_point(corners[b], camera, aspect, scale_z)
                else {
                    continue;
                };

                vertices.push(LineVertex { pos: pa, color });
                vertices.push(LineVertex { pos: pb, color });
            }
        }
        vertices
    }

    /// Upload volume data to a 3D GPU texture.
    pub fn upload_volume(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, volume: &Volume) {
        self.dim_x = volume.dim_x as u32;
        self.dim_y = volume.dim_y as u32;
        self.dim_z = volume.dim_z as u32;

        self.window_center = volume.window_center as f32;
        self.window_width = volume.window_width as f32;
        self.rescale_intercept = volume.rescale_intercept as f32;
        self.voxel_spacing = volume.voxel_spacing;

        let packed = volume.pack_for_gpu();

        let size = wgpu::Extent3d {
            width: self.dim_x,
            height: self.dim_y,
            depth_or_array_layers: self.dim_z,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("volume_texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&packed),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.dim_x * 4 * 2), // 4 u16 channels
                rows_per_image: Some(self.dim_y),
            },
            size,
        );

        self.volume_texture = Some(texture);
        self.rebuild_bind_group(device);
    }

    /// Upload a transfer function to a 1D GPU texture.
    pub fn upload_transfer_function(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tf: &TransferFunction,
    ) {
        let size = wgpu::Extent3d {
            width: TRANSFER_SIZE as u32,
            height: 1,
            depth_or_array_layers: 1,
        };

        let needs_bind_group_rebuild = if self.transfer_texture.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("transfer_texture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.transfer_texture = Some(texture);
            true
        } else {
            false
        };

        let float_data = tf.as_rgba_f32();
        let texture = self
            .transfer_texture
            .as_ref()
            .expect("transfer texture must exist");
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&float_data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TRANSFER_SIZE as u32 * 4 * 4),
                rows_per_image: None,
            },
            size,
        );

        if needs_bind_group_rebuild {
            self.rebuild_bind_group(device);
        }
    }

    /// Rebuild the bind group after texture changes.
    fn rebuild_bind_group(&mut self, device: &wgpu::Device) {
        let (vol_tex, tf_tex) = match (&self.volume_texture, &self.transfer_texture) {
            (Some(v), Some(t)) => (v, t),
            _ => {
                self.bind_group = None;
                return;
            }
        };

        let vol_view = vol_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let tf_view = tf_tex.create_view(&wgpu::TextureViewDescriptor::default());

        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("raycast_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&vol_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.volume_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&tf_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.transfer_sampler),
                },
            ],
        }));
    }

    /// Render a frame to the given render pass.
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        camera: &Camera,
        width: u32,
        height: u32,
        threats: &[ThreatBox],
        show_threats: bool,
    ) {
        let bind_group = match &self.bind_group {
            Some(bg) => bg,
            None => return, // No volume loaded yet.
        };

        let max_dim = self.dim_x.max(self.dim_y).max(self.dim_z) as f32;
        let base_voxel_size = if max_dim > 0.0 { 1.0 / max_dim } else { 0.01 };
        let zoom_factor = 1.0 / camera.distance.clamp(0.5, 5.0);

        let step_size = match self.quality {
            Quality::Fast => base_voxel_size * zoom_factor * 1.5,
            Quality::Medium => base_voxel_size * zoom_factor * 1.0,
            Quality::High => base_voxel_size * zoom_factor * 0.5,
        }
        .clamp(0.0005, 0.02);

        let scale_z = self.compute_scale_z();

        let window_min = self.window_center - self.window_width * 0.5;
        let aspect = if height > 0 {
            width as f32 / height as f32
        } else {
            1.0
        };

        let pos = camera.position();
        let fwd = camera.forward();
        let right = camera.right();
        let up = camera.up();

        let uniforms = Uniforms {
            cam_pos: pos.into(),
            fov: camera.fov,
            cam_forward: fwd.into(),
            aspect_ratio: aspect,
            cam_right: right.into(),
            step_size,
            cam_up: up.into(),
            scale_z,
            window_min,
            window_range: self.window_width,
            alpha_scale: self.alpha_scale,
            rescale_intercept: self.rescale_intercept,
            density_threshold: self.density_threshold,
            ambient_intensity: self.ambient,
            diffuse_intensity: self.diffuse,
            specular_intensity: self.specular,
        };

        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let line_vertices = if show_threats {
            self.build_threat_line_vertices(threats, camera, aspect, scale_z)
        } else {
            Vec::new()
        };
        let line_vertex_count = line_vertices.len().min(self.line_vertex_capacity);
        if line_vertex_count > 0 {
            queue.write_buffer(
                &self.line_vertex_buffer,
                0,
                bytemuck::cast_slice(&line_vertices[..line_vertex_count]),
            );
        }

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("raycast_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.84,
                            g: 0.84,
                            b: 0.84,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, bind_group, &[]);
            rpass.draw(0..3, 0..1); // Full-screen triangle.

            if line_vertex_count > 1 {
                rpass.set_pipeline(&self.line_pipeline);
                rpass.set_vertex_buffer(0, self.line_vertex_buffer.slice(..));
                rpass.draw(0..line_vertex_count as u32, 0..1);
            }
        }
    }

    /// Returns true if a volume has been uploaded and the renderer is ready.
    pub fn is_ready(&self) -> bool {
        self.bind_group.is_some()
    }
}
