// ============================================================================
// MEDIAN FILTER
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MedianParams {
    width: u32,
    height: u32,
    radius: u32,
    _pad0: u32,
}

pub struct GpuMedianPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
}

impl GpuMedianPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("median_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::MEDIAN_SHADER.into()),
        });
        let bgl = filter_bgl(device, "median_bgl");
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("median_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("median_pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("cs_median"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, bgl }
    }

    /// GPU median filter.  `radius` is clamped to 7 max (window 15×15).
    /// Returns None if radius > 7 (caller should fall back to CPU).
    pub fn apply(
        &self,
        ctx: &GpuContext,
        data: &[u8],
        w: u32,
        h: u32,
        radius: u32,
    ) -> Option<Vec<u8>> {
        if radius > 7 {
            return None;
        }
        let params = MedianParams {
            width: w,
            height: h,
            radius,
            _pad0: 0,
        };
        Some(dispatch_simple_filter(
            ctx,
            &self.pipeline,
            &self.bgl,
            data,
            w,
            h,
            bytemuck::bytes_of(&params),
            "cs_median",
        ))
    }
}

// ============================================================================
// GRADIENT GENERATOR — GPU-accelerated gradient rasterizer
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GradientGpuParams {
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub width: u32,
    pub height: u32,
    pub shape: u32,     // 0=Linear, 1=LinearReflected, 2=Radial, 3=Diamond
    pub repeat: u32,    // 0=clamp, 1=repeat
    pub is_eraser: u32, // 0=color, 1=transparency/eraser
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

pub struct GpuGradientPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    // Cached GPU resources — reused when dimensions match
    cached_output_tex: Option<wgpu::Texture>,
    cached_staging_buf: Option<wgpu::Buffer>,
    cached_params_buf: Option<wgpu::Buffer>,
    cached_lut_buf: Option<wgpu::Buffer>,
    cached_w: u32,
    cached_h: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MagicWandGpuParams {
    pub width: u32,
    pub height: u32,
    pub threshold: u32,
    pub anti_aliased: u32,
    pub mode: u32,
    pub use_base: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

pub struct GpuMagicWandPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    cached_distance_tex: Option<wgpu::Texture>,
    cached_base_tex: Option<wgpu::Texture>,
    dummy_base_tex: Option<wgpu::Texture>,
    cached_output_tex: Option<wgpu::Texture>,
    cached_staging_buf: Option<wgpu::Buffer>,
    cached_params_buf: Option<wgpu::Buffer>,
    cached_w: u32,
    cached_h: u32,
    cached_distance_key: usize,
    cached_base_key: Option<usize>,
}

impl GpuMagicWandPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("magic_wand_mask_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::MAGIC_WAND_MASK_SHADER.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("magic_wand_mask_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
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
            label: Some("magic_wand_mask_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("magic_wand_mask_pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("cs_magic_wand_mask"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            pipeline,
            bgl,
            cached_distance_tex: None,
            cached_base_tex: None,
            dummy_base_tex: None,
            cached_output_tex: None,
            cached_staging_buf: None,
            cached_params_buf: None,
            cached_w: 0,
            cached_h: 0,
            cached_distance_key: 0,
            cached_base_key: None,
        }
    }

    fn ensure_cache(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32) {
        if self.cached_w != w || self.cached_h != h {
            self.cached_output_tex =
                Some(create_rw_texture(device, w, h, "magic_wand_mask_output"));

            let bytes_per_row = super::compositor::Compositor::aligned_bytes_per_row(w);
            let buffer_size = (bytes_per_row * h) as u64;
            self.cached_staging_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("magic_wand_mask_staging"),
                size: buffer_size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));

            self.cached_distance_tex = None;
            self.cached_base_tex = None;
            self.cached_distance_key = 0;
            self.cached_base_key = None;
            self.cached_w = w;
            self.cached_h = h;
        }

        if self.dummy_base_tex.is_none() {
            self.dummy_base_tex = Some(upload_r8(
                device,
                queue,
                &[0],
                1,
                1,
                "magic_wand_base_dummy",
            ));
        }
    }

    pub fn generate_into(
        &mut self,
        ctx: &GpuContext,
        distances: &[u8],
        distance_key: usize,
        base_mask: Option<&[u8]>,
        base_key: Option<usize>,
        w: u32,
        h: u32,
        threshold: u8,
        anti_aliased: bool,
        mode: u32,
        out: &mut Vec<u8>,
    ) {
        let device = &ctx.device;
        let queue = &ctx.queue;

        self.ensure_cache(device, queue, w, h);

        if self.cached_distance_tex.is_none() || self.cached_distance_key != distance_key {
            self.cached_distance_tex = Some(upload_r8(
                device,
                queue,
                distances,
                w,
                h,
                "magic_wand_distances",
            ));
            self.cached_distance_key = distance_key;
        }

        if let Some(base_data) = base_mask {
            if self.cached_base_tex.is_none() || self.cached_base_key != base_key {
                self.cached_base_tex = Some(upload_r8(
                    device,
                    queue,
                    base_data,
                    w,
                    h,
                    "magic_wand_base_mask",
                ));
                self.cached_base_key = base_key;
            }
        } else {
            self.cached_base_key = None;
        }

        let params = MagicWandGpuParams {
            width: w,
            height: h,
            threshold: threshold as u32,
            anti_aliased: if anti_aliased { 1 } else { 0 },
            mode,
            use_base: if base_mask.is_some() { 1 } else { 0 },
            _pad0: 0,
            _pad1: 0,
        };
        let params_bytes = bytemuck::bytes_of(&params);
        let params_buf = self.cached_params_buf.get_or_insert_with(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("magic_wand_mask_params"),
                contents: params_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        });
        queue.write_buffer(params_buf, 0, params_bytes);

        let dist_view = self
            .cached_distance_tex
            .as_ref()
            .unwrap()
            .create_view(&wgpu::TextureViewDescriptor::default());
        let base_view = if let Some(base_tex) = self.cached_base_tex.as_ref() {
            base_tex.create_view(&wgpu::TextureViewDescriptor::default())
        } else {
            self.dummy_base_tex
                .as_ref()
                .unwrap()
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let out_view = self
            .cached_output_tex
            .as_ref()
            .unwrap()
            .create_view(&wgpu::TextureViewDescriptor::default());

        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("magic_wand_mask_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&dist_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&base_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });

        let bytes_per_row = super::compositor::Compositor::aligned_bytes_per_row(w);
        let staging = self.cached_staging_buf.as_ref().unwrap();
        let out_tex = self.cached_output_tex.as_ref().unwrap();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("magic_wand_mask_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("magic_wand_mask_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: out_tex,
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
                eprintln!("[GPU] GpuMagicWandPipeline readback map error: {:?}", e);
                return;
            }
            Err(e) => {
                eprintln!("[GPU] GpuMagicWandPipeline readback channel error: {:?}", e);
                return;
            }
        }

        let mapped = slice.get_mapped_range();
        out.clear();
        out.resize((w * h) as usize, 0);
        for y in 0..h as usize {
            let src_row = y * bytes_per_row as usize;
            let dst_row = y * w as usize;
            for x in 0..w as usize {
                out[dst_row + x] = mapped[src_row + x * 4];
            }
        }

        drop(mapped);
        staging.unmap();
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct FillPreviewGpuParams {
    pub fill_color: [f32; 4],
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub region_x: u32,
    pub region_y: u32,
    pub region_width: u32,
    pub region_height: u32,
    pub threshold: u32,
    pub anti_aliased: u32,
    pub use_selection: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

pub struct GpuFillPreviewPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    cached_distance_tex: Option<wgpu::Texture>,
    cached_background_tex: Option<wgpu::Texture>,
    cached_selection_tex: Option<wgpu::Texture>,
    dummy_selection_tex: Option<wgpu::Texture>,
    cached_output_tex: Option<wgpu::Texture>,
    cached_staging_buf: Option<wgpu::Buffer>,
    cached_params_buf: Option<wgpu::Buffer>,
    cached_canvas_w: u32,
    cached_canvas_h: u32,
    cached_region_w: u32,
    cached_region_h: u32,
    cached_distance_key: usize,
    cached_background_key: usize,
    cached_selection_key: Option<usize>,
}

impl GpuFillPreviewPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fill_preview_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::FILL_PREVIEW_SHADER.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fill_preview_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
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
            label: Some("fill_preview_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("fill_preview_pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("cs_fill_preview"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            pipeline,
            bgl,
            cached_distance_tex: None,
            cached_background_tex: None,
            cached_selection_tex: None,
            dummy_selection_tex: None,
            cached_output_tex: None,
            cached_staging_buf: None,
            cached_params_buf: None,
            cached_canvas_w: 0,
            cached_canvas_h: 0,
            cached_region_w: 0,
            cached_region_h: 0,
            cached_distance_key: 0,
            cached_background_key: 0,
            cached_selection_key: None,
        }
    }

    fn ensure_cache(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas_w: u32,
        canvas_h: u32,
        region_w: u32,
        region_h: u32,
    ) {
        if self.cached_canvas_w != canvas_w || self.cached_canvas_h != canvas_h {
            self.cached_distance_tex = None;
            self.cached_background_tex = None;
            self.cached_selection_tex = None;
            self.cached_distance_key = 0;
            self.cached_background_key = 0;
            self.cached_selection_key = None;
            self.cached_canvas_w = canvas_w;
            self.cached_canvas_h = canvas_h;
        }

        if self.cached_region_w != region_w || self.cached_region_h != region_h {
            self.cached_output_tex = Some(create_rw_texture(
                device,
                region_w,
                region_h,
                "fill_preview_output",
            ));

            let bytes_per_row = super::compositor::Compositor::aligned_bytes_per_row(region_w);
            let buffer_size = (bytes_per_row * region_h) as u64;
            self.cached_staging_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fill_preview_staging"),
                size: buffer_size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));

            self.cached_region_w = region_w;
            self.cached_region_h = region_h;
        }

        if self.dummy_selection_tex.is_none() {
            self.dummy_selection_tex = Some(upload_r8(
                device,
                queue,
                &[255],
                1,
                1,
                "fill_preview_selection_dummy",
            ));
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn generate_into(
        &mut self,
        ctx: &GpuContext,
        distances: &[u8],
        distance_key: usize,
        background_rgba: &[u8],
        background_key: usize,
        selection_mask: Option<(&[u8], usize)>,
        canvas_w: u32,
        canvas_h: u32,
        region_x: u32,
        region_y: u32,
        region_w: u32,
        region_h: u32,
        threshold: u8,
        anti_aliased: bool,
        fill_color: [u8; 4],
        out: &mut Vec<u8>,
    ) {
        let device = &ctx.device;
        let queue = &ctx.queue;

        self.ensure_cache(device, queue, canvas_w, canvas_h, region_w, region_h);

        if self.cached_distance_tex.is_none() || self.cached_distance_key != distance_key {
            self.cached_distance_tex = Some(upload_r8(
                device,
                queue,
                distances,
                canvas_w,
                canvas_h,
                "fill_preview_distances",
            ));
            self.cached_distance_key = distance_key;
        }

        if self.cached_background_tex.is_none() || self.cached_background_key != background_key {
            self.cached_background_tex = Some(upload_rgba(
                device,
                queue,
                background_rgba,
                canvas_w,
                canvas_h,
                "fill_preview_background",
            ));
            self.cached_background_key = background_key;
        }

        if let Some((selection_data, selection_key)) = selection_mask {
            if self.cached_selection_tex.is_none()
                || self.cached_selection_key != Some(selection_key)
            {
                self.cached_selection_tex = Some(upload_r8(
                    device,
                    queue,
                    selection_data,
                    canvas_w,
                    canvas_h,
                    "fill_preview_selection",
                ));
                self.cached_selection_key = Some(selection_key);
            }
        } else {
            self.cached_selection_key = None;
        }

        let params = FillPreviewGpuParams {
            fill_color: [
                fill_color[0] as f32 / 255.0,
                fill_color[1] as f32 / 255.0,
                fill_color[2] as f32 / 255.0,
                fill_color[3] as f32 / 255.0,
            ],
            canvas_width: canvas_w,
            canvas_height: canvas_h,
            region_x,
            region_y,
            region_width: region_w,
            region_height: region_h,
            threshold: threshold as u32,
            anti_aliased: if anti_aliased { 1 } else { 0 },
            use_selection: if selection_mask.is_some() { 1 } else { 0 },
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };

        let params_bytes = bytemuck::bytes_of(&params);
        let params_buf = self.cached_params_buf.get_or_insert_with(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fill_preview_params"),
                contents: params_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        });
        queue.write_buffer(params_buf, 0, params_bytes);

        let dist_view = self
            .cached_distance_tex
            .as_ref()
            .unwrap()
            .create_view(&wgpu::TextureViewDescriptor::default());
        let bg_view = self
            .cached_background_tex
            .as_ref()
            .unwrap()
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sel_view = if let Some(selection_tex) = self.cached_selection_tex.as_ref() {
            selection_tex.create_view(&wgpu::TextureViewDescriptor::default())
        } else {
            self.dummy_selection_tex
                .as_ref()
                .unwrap()
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let out_view = self
            .cached_output_tex
            .as_ref()
            .unwrap()
            .create_view(&wgpu::TextureViewDescriptor::default());

        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fill_preview_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&dist_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&bg_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&sel_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });

        let bytes_per_row = super::compositor::Compositor::aligned_bytes_per_row(region_w);
        let staging = self.cached_staging_buf.as_ref().unwrap();
        let out_tex = self.cached_output_tex.as_ref().unwrap();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fill_preview_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fill_preview_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(region_w.div_ceil(16), region_h.div_ceil(16), 1);
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: out_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(region_h),
                },
            },
            wgpu::Extent3d {
                width: region_w,
                height: region_h,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(std::iter::once(encoder.finish()));

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
                eprintln!("[GPU] GpuFillPreviewPipeline readback map error: {:?}", e);
                return;
            }
            Err(e) => {
                eprintln!(
                    "[GPU] GpuFillPreviewPipeline readback channel error: {:?}",
                    e
                );
                return;
            }
        }

        let mapped = slice.get_mapped_range();
        let actual_row = region_w as usize * 4;
        out.clear();
        out.resize(actual_row * region_h as usize, 0);
        for y in 0..region_h as usize {
            let src_row = y * bytes_per_row as usize;
            let dst_row = y * actual_row;
            out[dst_row..dst_row + actual_row]
                .copy_from_slice(&mapped[src_row..src_row + actual_row]);
        }

        drop(mapped);
        staging.unmap();
    }
}

