// ============================================================================
// MESH WARP DISPLACEMENT PIPELINE — GPU Catmull-Rom displacement field generation
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MeshWarpGpuParams {
    pub width: u32,
    pub height: u32,
    pub grid_cols: u32,
    pub grid_rows: u32,
}

pub struct GpuMeshWarpDisplacementPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    cached_points_buf: Option<wgpu::Buffer>,
    cached_disp_out_buf: Option<wgpu::Buffer>,
    cached_staging_buf: Option<wgpu::Buffer>,
    cached_params_buf: Option<wgpu::Buffer>,
    cached_w: u32,
    cached_h: u32,
}

impl GpuMeshWarpDisplacementPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh_warp_displacement_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::MESH_WARP_DISPLACEMENT_SHADER.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh_warp_disp_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh_warp_disp_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("mesh_warp_disp_pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("cs_mesh_warp_displacement"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            pipeline,
            bgl,
            cached_points_buf: None,
            cached_disp_out_buf: None,
            cached_staging_buf: None,
            cached_params_buf: None,
            cached_w: 0,
            cached_h: 0,
        }
    }

    fn ensure_cache(&mut self, device: &wgpu::Device, w: u32, h: u32, num_points: usize) {
        if self.cached_w != w || self.cached_h != h {
            let disp_size = (w as usize * h as usize * 2 * std::mem::size_of::<f32>()) as u64;
            self.cached_disp_out_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mw_disp_out"),
                size: disp_size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }));
            self.cached_staging_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mw_disp_staging"),
                size: disp_size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.cached_params_buf = None;
            self.cached_w = w;
            self.cached_h = h;
        }

        let pts_bytes = (num_points * 2 * std::mem::size_of::<f32>()) as u64;
        let need_pts = match &self.cached_points_buf {
            Some(b) => b.size() < pts_bytes,
            None => true,
        };
        if need_pts {
            self.cached_points_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mw_points"),
                size: pts_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }

    /// Generate displacement field on GPU from deformed control points.
    pub fn generate_displacement(
        &mut self,
        ctx: &GpuContext,
        deformed_points: &[[f32; 2]],
        grid_cols: u32,
        grid_rows: u32,
        w: u32,
        h: u32,
        out: &mut Vec<f32>,
    ) {
        let device = &ctx.device;
        let queue = &ctx.queue;
        let num_points = deformed_points.len();

        self.ensure_cache(device, w, h, num_points);

        let pts_buf = match self.cached_points_buf.as_ref() {
            Some(b) => b,
            None => {
                eprintln!(
                    "[GPU] GpuMeshWarpDisplacementPipeline: cached_points_buf not initialised — skipping"
                );
                return;
            }
        };
        let disp_buf = match self.cached_disp_out_buf.as_ref() {
            Some(b) => b,
            None => {
                eprintln!(
                    "[GPU] GpuMeshWarpDisplacementPipeline: cached_disp_out_buf not initialised — skipping"
                );
                return;
            }
        };
        let staging = match self.cached_staging_buf.as_ref() {
            Some(b) => b,
            None => {
                eprintln!(
                    "[GPU] GpuMeshWarpDisplacementPipeline: cached_staging_buf not initialised — skipping"
                );
                return;
            }
        };

        queue.write_buffer(pts_buf, 0, bytemuck::cast_slice(deformed_points));

        let params = MeshWarpGpuParams {
            width: w,
            height: h,
            grid_cols,
            grid_rows,
        };
        let params_bytes = bytemuck::bytes_of(&params);
        let params_buf = self.cached_params_buf.get_or_insert_with(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mw_params"),
                contents: params_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        });
        queue.write_buffer(params_buf, 0, params_bytes);

        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mw_disp_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pts_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: disp_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
            ],
        });

        let disp_byte_size = (w as usize * h as usize * 2 * std::mem::size_of::<f32>()) as u64;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mw_disp_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("mw_disp_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(disp_buf, 0, staging, 0, disp_byte_size);
        queue.submit(std::iter::once(encoder.finish()));

        let slice = staging.slice(..disp_byte_size);
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
                eprintln!(
                    "[GPU] GpuMeshWarpDisplacementPipeline readback map error: {:?}",
                    e
                );
                return;
            }
            Err(e) => {
                eprintln!(
                    "[GPU] GpuMeshWarpDisplacementPipeline readback channel error: {:?}",
                    e
                );
                return;
            }
        }

        let mapped = slice.get_mapped_range();
        let float_count = w as usize * h as usize * 2;
        out.resize(float_count, 0.0);
        let src_floats: &[f32] = bytemuck::cast_slice(&mapped);
        out.copy_from_slice(&src_floats[..float_count]);

        drop(mapped);
        staging.unmap();
    }
}
