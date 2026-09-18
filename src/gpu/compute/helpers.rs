fn create_rw_texture(device: &wgpu::Device, w: u32, h: u32, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn upload_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &[u8],
    w: u32,
    h: u32,
    label: &str,
) -> wgpu::Texture {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
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
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
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
    tex
}

fn upload_r8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &[u8],
    w: u32,
    h: u32,
    label: &str,
) -> wgpu::Texture {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    tex
}

/// Standard bind group layout used by most filters: input tex, output storage tex, uniform buf.
fn filter_bgl(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
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
        ],
    })
}

fn dispatch_simple_filter(
    ctx: &GpuContext,
    pipeline: &wgpu::ComputePipeline,
    bgl: &wgpu::BindGroupLayout,
    input_data: &[u8],
    w: u32,
    h: u32,
    params_bytes: &[u8],
    _entry: &str,
) -> Vec<u8> {
    let device = &ctx.device;
    let queue = &ctx.queue;

    let src_tex = upload_rgba(device, queue, input_data, w, h, "filter_src");
    let dst_tex = create_rw_texture(device, w, h, "filter_dst");

    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("filter_params"),
        contents: params_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let src_view = src_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let dst_view = dst_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("filter_bg"),
        layout: bgl,
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
                resource: params_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("filter_encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("filter_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
    }
    queue.submit(std::iter::once(encoder.finish()));

    super::compositor::Compositor::readback_texture(ctx, &dst_tex, w, h, &mut None)
}

