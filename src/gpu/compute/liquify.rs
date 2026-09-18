// ============================================================================
// LIQUIFY WARP PIPELINE — GPU displacement warp for Liquify tool
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct LiquifyGpuParams {
    pub width: u32,
    pub height: u32,
}

pub struct GpuLiquifyPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    // Cached GPU resources — reused when dimensions match
    cached_source_tex: Option<wgpu::Texture>,
    cached_output_tex: Option<wgpu::Texture>,
    cached_staging_buf: Option<wgpu::Buffer>,
    cached_disp_buf: Option<wgpu::Buffer>,
    cached_params_buf: Option<wgpu::Buffer>,
    cached_w: u32,
    cached_h: u32,
    /// When true, the cached source texture needs re-uploading
    /// (e.g. new stroke started with a fresh snapshot).
    source_dirty: bool,
}

impl GpuLiquifyPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquify_warp_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::LIQUIFY_WARP_SHADER.into()),
        });

        // Bind group layout:
        //  0: source texture (texture_2d<f32>, read)
        //  1: output storage texture (write-only, rgba8unorm)
        //  2: displacement storage buffer (read-only, array<f32>)
        //  3: uniform buffer (LiquifyGpuParams)
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquify_bgl"),
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
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquify_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("liquify_pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("cs_liquify_warp"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            pipeline,
            bgl,
            cached_source_tex: None,
            cached_output_tex: None,
            cached_staging_buf: None,
            cached_disp_buf: None,
            cached_params_buf: None,
            cached_w: 0,
            cached_h: 0,
            source_dirty: true,
        }
    }

    /// Ensure cached resources match the requested dimensions.
    fn ensure_cache(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if self.cached_w != w || self.cached_h != h {
            // Source texture (will be written via queue.write_texture)
            self.cached_source_tex = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("liquify_source"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));

            // Output texture
            self.cached_output_tex = Some(create_rw_texture(device, w, h, "liquify_output"));

            // Staging buffer for readback (256-byte aligned rows)
            let bytes_per_row = super::compositor::Compositor::aligned_bytes_per_row(w);
            let buffer_size = (bytes_per_row * h) as u64;
            self.cached_staging_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("liquify_staging"),
                size: buffer_size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));

            // Displacement storage buffer: width * height * 2 floats
            let disp_size = (w as usize * h as usize * 2 * std::mem::size_of::<f32>()) as u64;
            self.cached_disp_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("liquify_disp"),
                size: disp_size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));

            // Params buffer will be created/reused on first dispatch
            self.cached_params_buf = None;

            self.cached_w = w;
            self.cached_h = h;
            self.source_dirty = true; // dimensions changed, need re-upload
        }
    }

    /// Mark the source texture as needing re-upload (call when a new stroke starts).
    pub fn invalidate_source(&mut self) {
        self.source_dirty = true;
    }

    /// Run the displacement warp on GPU and write the result into `out`.
    ///
    /// `source_rgba` — raw RGBA pixels of the source snapshot.
    /// `displacement_data` — flat (dx,dy) pairs from DisplacementField.data.
    /// `w, h` — canvas dimensions.
    /// `out` — destination buffer; resized to `w * h * 4`.
    pub fn warp_into(
        &mut self,
        ctx: &GpuContext,
        source_rgba: &[u8],
        displacement_data: &[f32],
        w: u32,
        h: u32,
        out: &mut Vec<u8>,
    ) {
        let device = &ctx.device;
        let queue = &ctx.queue;

        self.ensure_cache(device, w, h);

        let src_tex = match self.cached_source_tex.as_ref() {
            Some(t) => t,
            None => {
                eprintln!("[GPU] GpuLiquifyPipeline: cached_source_tex not initialised — skipping");
                return;
            }
        };
        let dst_tex = match self.cached_output_tex.as_ref() {
            Some(t) => t,
            None => {
                eprintln!("[GPU] GpuLiquifyPipeline: cached_output_tex not initialised — skipping");
                return;
            }
        };
        let staging = match self.cached_staging_buf.as_ref() {
            Some(b) => b,
            None => {
                eprintln!(
                    "[GPU] GpuLiquifyPipeline: cached_staging_buf not initialised — skipping"
                );
                return;
            }
        };
        let disp_buf = match self.cached_disp_buf.as_ref() {
            Some(b) => b,
            None => {
                eprintln!("[GPU] GpuLiquifyPipeline: cached_disp_buf not initialised — skipping");
                return;
            }
        };

        // Upload source image (only when dirty — stays constant during a stroke)
        if self.source_dirty {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: src_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                source_rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            self.source_dirty = false;
        }

        // Upload displacement field every frame
        queue.write_buffer(disp_buf, 0, bytemuck::cast_slice(displacement_data));

        // Params uniform
        let params = LiquifyGpuParams {
            width: w,
            height: h,
        };
        let params_bytes = bytemuck::bytes_of(&params);
        let params_buf = self.cached_params_buf.get_or_insert_with(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("liquify_params"),
                contents: params_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        });
        queue.write_buffer(params_buf, 0, params_bytes);

        // Build bind group
        let src_view = src_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let dst_view = dst_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquify_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&dst_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: disp_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });

        let bytes_per_row = super::compositor::Compositor::aligned_bytes_per_row(w);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("liquify_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("liquify_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }

        // Copy output texture → staging buffer
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: dst_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(std::iter::once(encoder.finish()));

        // Map and readback
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        match rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                eprintln!("[GPU] GpuLiquifyPipeline readback map error: {:?}", e);
                return;
            }
            Err(e) => {
                eprintln!("[GPU] GpuLiquifyPipeline readback channel error: {:?}", e);
                return;
            }
        }

        let mapped = slice.get_mapped_range();
        let actual_row = w as usize * 4;
        let total = actual_row * h as usize;

        out.clear();
        out.resize(total, 0);
        for y in 0..h as usize {
            let src_start = y * bytes_per_row as usize;
            let dst_start = y * actual_row;
            out[dst_start..dst_start + actual_row]
                .copy_from_slice(&mapped[src_start..src_start + actual_row]);
        }

        drop(mapped);
        staging.unmap();
    }
}

