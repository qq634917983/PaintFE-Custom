// ============================================================================
// GAUSSIAN BLUR (shared-memory optimised)
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurParams {
    radius: u32,
    direction: u32,
    width: u32,
    height: u32,
}

pub struct GpuBlurPipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl GpuBlurPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur_compute_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::GAUSSIAN_BLUR_SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("blur_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("cs_blur"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }

    /// Two-pass separable Gaussian blur on a GPU texture.
    /// Updated dispatch for the shared-memory workgroup_size(256,1,1) shader.
    pub fn blur(
        &self,
        ctx: &GpuContext,
        input: &wgpu::Texture,
        width: u32,
        height: u32,
        sigma: f32,
    ) -> wgpu::Texture {
        let device = &ctx.device;
        let queue = &ctx.queue;

        let kernel = Self::build_kernel(sigma);
        // Cap radius to 127 to stay within shared memory limits (MAX_SHARED=512, TILE_W=256, max apron=256+2*127=510)
        let radius = (kernel.len() / 2).min(127) as u32;
        let kernel = if kernel.len() > (radius as usize * 2 + 1) {
            // Truncate kernel to capped radius and renormalize
            let center = kernel.len() / 2;
            let start = center - radius as usize;
            let end = center + radius as usize + 1;
            let mut truncated: Vec<f32> = kernel[start..end].to_vec();
            let sum: f32 = truncated.iter().sum();
            if sum > 0.0 {
                for v in &mut truncated {
                    *v /= sum;
                }
            }
            truncated
        } else {
            kernel
        };

        let kernel_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("blur_kernel_buf"),
            contents: bytemuck::cast_slice(&kernel),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let temp_tex = create_rw_texture(device, width, height, "blur_temp");
        let output_tex = create_rw_texture(device, width, height, "blur_output");

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("blur_encoder"),
        });

        // ---- Pass 1: Horizontal blur (input → temp) ----
        // Dispatch: one workgroup of 256 threads per tile-row, one row per Y.
        {
            let params = BlurParams {
                radius,
                direction: 0,
                width,
                height,
            };
            let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("blur_params_h"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

            let input_view = input.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            });
            let temp_view = temp_tex.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            });

            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blur_bg_h"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&input_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&temp_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: kernel_buffer.as_entire_binding(),
                    },
                ],
            });

            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("blur_h_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bg, &[]);
            // X workgroups: ceil(width / 256), Y: one per row
            pass.dispatch_workgroups(width.div_ceil(256), height, 1);
        }

        // ---- Pass 2: Vertical blur (temp → output) ----
        {
            let params = BlurParams {
                radius,
                direction: 1,
                width,
                height,
            };
            let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("blur_params_v"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

            let temp_view = temp_tex.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            });
            let output_view = output_tex.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            });

            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blur_bg_v"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&temp_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&output_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: kernel_buffer.as_entire_binding(),
                    },
                ],
            });

            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("blur_v_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bg, &[]);
            // Vertical: X workgroups = ceil(height/256), Y = width (one column per Y)
            pass.dispatch_workgroups(height.div_ceil(256), width, 1);
        }

        queue.submit(std::iter::once(encoder.finish()));
        output_tex
    }

    /// CPU ↔ GPU convenience: upload, blur, read back.
    pub fn blur_image(
        &self,
        ctx: &GpuContext,
        input_data: &[u8],
        width: u32,
        height: u32,
        sigma: f32,
    ) -> Vec<u8> {
        let src_tex = upload_rgba(
            &ctx.device,
            &ctx.queue,
            input_data,
            width,
            height,
            "blur_src",
        );
        let output_tex = self.blur(ctx, &src_tex, width, height, sigma);
        super::compositor::Compositor::readback_texture(ctx, &output_tex, width, height, &mut None)
    }

    fn build_kernel(sigma: f32) -> Vec<f32> {
        let radius = (sigma * 3.0).ceil() as usize;
        if radius == 0 {
            return vec![1.0];
        }
        let len = radius * 2 + 1;
        let mut kernel = vec![0.0f32; len];
        let s2 = 2.0 * sigma * sigma;
        let mut sum = 0.0f32;
        for (i, item) in kernel.iter_mut().enumerate() {
            let x = i as f32 - radius as f32;
            let v = (-x * x / s2).exp();
            *item = v;
            sum += v;
        }
        let inv = 1.0 / sum;
        for v in &mut kernel {
            *v *= inv;
        }
        kernel
    }
}

