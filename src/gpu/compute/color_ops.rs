// ============================================================================
// BRIGHTNESS / CONTRAST
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BcParams {
    width: u32,
    height: u32,
    brightness: f32,
    contrast: f32,
}

pub struct GpuBrightnessContrastPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
}

impl GpuBrightnessContrastPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bc_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::BRIGHTNESS_CONTRAST_SHADER.into()),
        });
        let bgl = filter_bgl(device, "bc_bgl");
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bc_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("bc_pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("cs_brightness_contrast"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, bgl }
    }

    pub fn apply(
        &self,
        ctx: &GpuContext,
        data: &[u8],
        w: u32,
        h: u32,
        brightness: f32,
        contrast: f32,
    ) -> Vec<u8> {
        let params = BcParams {
            width: w,
            height: h,
            brightness,
            contrast,
        };
        dispatch_simple_filter(
            ctx,
            &self.pipeline,
            &self.bgl,
            data,
            w,
            h,
            bytemuck::bytes_of(&params),
            "cs_brightness_contrast",
        )
    }
}

// ============================================================================
// HUE / SATURATION / LIGHTNESS
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HslParams {
    width: u32,
    height: u32,
    hue_shift: f32,    // normalised: hue_shift_degrees / 360.0
    sat_factor: f32,   // 1.0 + saturation / 100.0
    light_offset: f32, // lightness * 255.0 / 100.0 / 255.0 = lightness / 100.0
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

pub struct GpuHslPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
}

impl GpuHslPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hsl_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::HSL_ADJUST_SHADER.into()),
        });
        let bgl = filter_bgl(device, "hsl_bgl");
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hsl_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("hsl_pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("cs_hsl_adjust"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, bgl }
    }

    /// `hue_shift`: degrees (-180..180), `saturation`: -100..100, `lightness`: -100..100
    pub fn apply(
        &self,
        ctx: &GpuContext,
        data: &[u8],
        w: u32,
        h: u32,
        hue_shift: f32,
        saturation: f32,
        lightness: f32,
    ) -> Vec<u8> {
        let params = HslParams {
            width: w,
            height: h,
            hue_shift: hue_shift / 360.0,
            sat_factor: 1.0 + saturation / 100.0,
            light_offset: lightness / 100.0,
            _pad0: 0.0,
            _pad1: 0.0,
            _pad2: 0.0,
        };
        dispatch_simple_filter(
            ctx,
            &self.pipeline,
            &self.bgl,
            data,
            w,
            h,
            bytemuck::bytes_of(&params),
            "cs_hsl_adjust",
        )
    }
}

// ============================================================================
// INVERT
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InvParams {
    width: u32,
    height: u32,
    _pad0: u32,
    _pad1: u32,
}

pub struct GpuInvertPipeline {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
}

impl GpuInvertPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("invert_shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::INVERT_SHADER.into()),
        });
        let bgl = filter_bgl(device, "invert_bgl");
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("invert_pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("invert_pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("cs_invert"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, bgl }
    }

    pub fn apply(&self, ctx: &GpuContext, data: &[u8], w: u32, h: u32) -> Vec<u8> {
        let params = InvParams {
            width: w,
            height: h,
            _pad0: 0,
            _pad1: 0,
        };
        dispatch_simple_filter(
            ctx,
            &self.pipeline,
            &self.bgl,
            data,
            w,
            h,
            bytemuck::bytes_of(&params),
            "cs_invert",
        )
    }
}

