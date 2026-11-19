//! Motion Blur, the Volt port of Natural Motion Blur (`naturalmotionblur`,
//! 1.4.5+mc26.3).
//!
//! The mod's own surface is kept: the two blur profiles (`UNIFORM` and
//! `WEIGHTED`, the Blackman-sinc weighted taps) and its blur algorithms
//! (`VELOCITY_BASED`, `FRAME_BLENDING`, `HYBRID_BLENDING`, `ACCUMULATION_MAX`
//! and `ACCUMULATION_MIX`), driven by a strength slider and the refresh-rate
//! scaling toggle.
//!
//! Two passes, so the world (and only the world: the HUD is drawn after) is
//! smeared:
//!
//! 1. `fs_blur`, the velocity pass. The camera's turn becomes one screen-space
//!    velocity, and the frame is resampled along it with `samples` taps, each
//!    one jittered with the same interleaved-gradient noise the mod uses, and
//!    each one weighted by the mod's Blackman-sinc window when the weighted
//!    profile is on. The taps run in sRGB space, so a uniform profile averages
//!    `c * c` (which is the mod's `sqrt(sum / total)` output).
//! 2. `fs_blend`, the accumulation pass. The smeared frame is mixed with the
//!    previous output, either as `max(curr, prev * blend)` (the mod's
//!    `accumulation_max.fsh`) or as a linear-space mix (`mix(curr, prev,
//!    blend)` after the same gamma trip `accumulation_mix.fsh` makes).
//!
//! `VELOCITY_BASED` runs the first pass only, `FRAME_BLENDING` the second,
//! and `HYBRID_BLENDING` both. Nothing is smeared when the camera does not
//! turn: at zero velocity every tap lands on the same texel.

/// Which of the mod's algorithms is driving the blur.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Algorithm {
    /// `VELOCITY_BASED`: taps along the camera's turn, no history.
    #[default]
    VelocityBased,
    /// `FRAME_BLENDING`: no velocity taps, the previous frame blended in.
    FrameBlending,
    /// `HYBRID_BLENDING`: velocity taps, then blended with the previous frame.
    HybridBlending,
    /// `ACCUMULATION_MAX`: the brighter of this frame and the faded last one.
    AccumulationMax,
    /// `ACCUMULATION_MIX`: a linear-space mix of the two frames.
    AccumulationMix,
}

impl Algorithm {
    pub const ALL: [Self; 5] = [
        Self::VelocityBased,
        Self::FrameBlending,
        Self::HybridBlending,
        Self::AccumulationMax,
        Self::AccumulationMix,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::VelocityBased => "Velocity based",
            Self::FrameBlending => "Frame blending",
            Self::HybridBlending => "Hybrid blending",
            Self::AccumulationMax => "Accumulation max",
            Self::AccumulationMix => "Accumulation mix",
        }
    }

    /// The mod's short names, for the settings rows.
    pub fn slug(self) -> &'static str {
        match self {
            Self::VelocityBased => "VELOCITY_BASED",
            Self::FrameBlending => "FRAME_BLENDING",
            Self::HybridBlending => "HYBRID_BLENDING",
            Self::AccumulationMax => "ACCUMULATION_MAX",
            Self::AccumulationMix => "ACCUMULATION_MIX",
        }
    }

    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|&a| a == self).unwrap_or(0) as u8
    }

    pub fn from_index(index: u8) -> Self {
        Self::ALL
            .get(index as usize)
            .copied()
            .unwrap_or(Self::VelocityBased)
    }

    pub fn parse(text: &str) -> Option<Self> {
        let needle = text.trim().to_ascii_uppercase().replace('-', "_");
        Self::ALL
            .into_iter()
            .find(|a| a.slug() == needle || a.label().eq_ignore_ascii_case(text.trim()))
    }

    /// Whether the velocity (smeared taps) pass runs.
    pub fn velocity_pass(self) -> bool {
        matches!(
            self,
            Self::VelocityBased | Self::HybridBlending
        )
    }

    /// Whether the accumulation (previous frame) pass runs.
    pub fn history_pass(self) -> bool {
        !matches!(self, Self::VelocityBased)
    }

    /// The `accumulation_max.fsh` shape instead of `accumulation_mix.fsh`.
    pub fn max_blend(self) -> bool {
        matches!(self, Self::AccumulationMax)
    }
}

/// The mod's `BlurProfile`: plain taps, or the Blackman-sinc weighted ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    /// `UNIFORM`: every tap counts the same.
    #[default]
    Uniform,
    /// `WEIGHTED`: Blackman-sinc weights, in gamma 2.2 space.
    Weighted,
}

impl Profile {
    pub const ALL: [Self; 2] = [Self::Uniform, Self::Weighted];

    pub fn label(self) -> &'static str {
        match self {
            Self::Uniform => "Uniform",
            Self::Weighted => "Weighted",
        }
    }

    pub fn index(self) -> u8 {
        match self {
            Self::Uniform => 0,
            Self::Weighted => 1,
        }
    }

    pub fn from_index(index: u8) -> Self {
        if index == 1 { Self::Weighted } else { Self::Uniform }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.label().eq_ignore_ascii_case(text.trim()))
    }
}

/// One frame's request: what the game asks the renderer to smear, and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Screen-space travel of the camera since the last frame, in UV units
    /// (1.0 spans the screen). Set from the yaw/pitch delta.
    pub velocity: [f32; 2],
    /// 0.0 (sharp) to 1.0 (full smear).
    pub strength: f32,
    /// Taps per pixel, 4..=16.
    pub samples: u32,
    pub profile: Profile,
    pub algorithm: Algorithm,
    /// How much of the previous frame the accumulation passes keep.
    pub blend: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            velocity: [0.0, 0.0],
            strength: 0.0,
            samples: 6,
            profile: Profile::Uniform,
            algorithm: Algorithm::VelocityBased,
            blend: 0.0,
        }
    }
}

impl Params {
    /// The mod's `BlurStrengthCalculator.calculate`: with refresh-rate scaling
    /// on, a frame that took longer than the panel's refresh period smears
    /// proportionally further, so a 60 Hz phone and a 144 Hz phone look alike.
    pub fn scaled_strength(&self, frame_seconds: f32, refresh_hz: f32, scale: bool) -> f32 {
        let strength = self.strength.clamp(0.0, 1.0);
        if !scale || !frame_seconds.is_finite() || frame_seconds <= 0.0 {
            return strength;
        }
        let refresh = if refresh_hz.is_finite() && refresh_hz >= 30.0 {
            refresh_hz
        } else {
            60.0
        };
        // A frame that took twice the panel's period carried twice the motion,
        // so it smears twice as far; a fast one smears less.
        let panels_per_frame = (refresh * frame_seconds).clamp(0.5, 2.0);
        (strength * panels_per_frame).clamp(0.0, 1.0)
    }

    /// Taps climb with the strength, 4 at the bottom to 16 at the top, as the
    /// mod's `sampleAmount` does.
    pub fn samples_for(strength: f32) -> u32 {
        let s = strength.clamp(0.0, 1.0);
        (4.0 + s * 12.0).round() as u32
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurUniform {
    /// Viewport width, height, sample count, weighted (0/1).
    size: [f32; 4],
    /// UV step per tap, unused, unused, unused.
    step: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlendUniform {
    /// Blend factor, algorithm (0 mix, 1 max), history primed (0/1), unused.
    params: [f32; 4],
}

pub struct MotionBlur {
    blur_layout: wgpu::BindGroupLayout,
    blend_layout: wgpu::BindGroupLayout,
    blur_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    blur_uniform: wgpu::Buffer,
    blend_uniform: wgpu::Buffer,
    /// The copied frame, the smeared copy and the previous output.
    textures: Vec<(wgpu::Texture, wgpu::TextureView)>,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    /// Whether `hist` holds a real frame yet (the first frame after a resize
    /// or a camera cut must not blend a black texture in).
    primed: bool,
}
const TEX_CUR: usize = 0;
const TEX_BLUR: usize = 1;
const TEX_HIST: usize = 2;

impl MotionBlur {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let format = format.remove_srgb_suffix();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Motion blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("motion_blur.wgsl").into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Motion blur"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Motion blur velocity bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let blend_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Motion blur accumulation bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let make_pipeline = |layout: &wgpu::BindGroupLayout, entry: &str, label: &str| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                ..Default::default()
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let blur_pipeline = make_pipeline(&blur_layout, "fs_blur", "Motion blur velocity pass");
        let blend_pipeline = make_pipeline(&blend_layout, "fs_blend", "Motion blur accumulation pass");
        let buffer = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            blur_layout,
            blend_layout,
            blur_pipeline,
            blend_pipeline,
            sampler,
            blur_uniform: buffer(
                "Motion blur velocity",
                std::mem::size_of::<BlurUniform>() as u64,
            ),
            blend_uniform: buffer(
                "Motion blur accumulation",
                std::mem::size_of::<BlendUniform>() as u64,
            ),
            textures: Vec::new(),
            size: [0, 0],
            format,
            primed: false,
        }
    }

    fn ensure(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.size == size && !self.textures.is_empty() {
            return;
        }
        self.size = size;
        self.primed = false;
        self.textures = (0..3)
            .map(|_| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Motion blur target"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = texture.create_view(&Default::default());
                (texture, view)
            })
            .collect();
    }

    /// Forget the frame history, so the next frame is drawn sharp: the camera
    /// jumped (teleport, respawn, perspective switch) instead of turning.
    pub fn reset(&mut self) {
        self.primed = false;
    }

    /// Smear `frame` (the swapchain texture) in place through `target`, its
    /// non-sRGB view.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::Texture,
        target: &wgpu::TextureView,
        size: [u32; 2],
        params: &Params,
    ) {
        if size[0] < 2 || size[1] < 2 || frame.width() == 0 || frame.height() == 0 {
            return;
        }
        self.ensure(device, size);
        let copy_width = size[0].min(frame.width());
        let copy_height = size[1].min(frame.height());
        encoder.copy_texture_to_texture(
            frame.as_image_copy(),
            self.textures[TEX_CUR].0.as_image_copy(),
            wgpu::Extent3d {
                width: copy_width,
                height: copy_height,
                depth_or_array_layers: 1,
            },
        );

        // 1. The velocity pass. The step is the whole travel divided by the
        // tap count; both components are in UV space.
        let velocity_pass = params.algorithm.velocity_pass();
        if velocity_pass {
            let samples = params.samples.clamp(2, 32) as f32;
            let strength = params.strength.clamp(0.0, 1.0);
            let step = [
                params.velocity[0] * strength / samples,
                params.velocity[1] * strength / samples,
            ];
            queue.write_buffer(
                &self.blur_uniform,
                0,
                bytemuck::bytes_of(&BlurUniform {
                    size: [
                        size[0] as f32,
                        size[1] as f32,
                        samples,
                        if params.profile == Profile::Weighted {
                            1.0
                        } else {
                            0.0
                        },
                    ],
                    step: [step[0], step[1], 0.0, 0.0],
                }),
            );
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Motion blur velocity pass"),
                layout: &self.blur_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.textures[TEX_CUR].1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.blur_uniform.as_entire_binding(),
                    },
                ],
            });
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &self.textures[TEX_BLUR].1,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Motion blur velocity pass"),
                color_attachments: &attachments,
                ..Default::default()
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        // Frame blending reads the raw frame; the velocity pass reads its
        // own output.
        let blurred = if velocity_pass { TEX_BLUR } else { TEX_CUR };

        // 2. The output pass. It is always a render pass into the frame, so
        // the swapchain texture only ever needs COPY_SRC (Android's images
        // have no COPY_DST): with no history it writes the smeared frame
        // straight back, with one it accumulates.
        let keep_history = params.algorithm.history_pass() && self.primed;
        queue.write_buffer(
            &self.blend_uniform,
            0,
            bytemuck::bytes_of(&BlendUniform {
                params: [
                    params.blend.clamp(0.0, 0.98),
                    if params.algorithm.max_blend() { 1.0 } else { 0.0 },
                    if keep_history { 1.0 } else { 0.0 },
                    0.0,
                ],
            }),
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Motion blur accumulation pass"),
            layout: &self.blend_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.textures[blurred].1),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&self.textures[TEX_HIST].1),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: self.blend_uniform.as_entire_binding(),
                },
            ],
        });
        let attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Motion blur accumulation pass"),
            color_attachments: &attachments,
            ..Default::default()
        });
        pass.set_pipeline(&self.blend_pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);

        // 3. Keep the finished frame as the next one's history.
        encoder.copy_texture_to_texture(
            frame.as_image_copy(),
            self.textures[TEX_HIST].0.as_image_copy(),
            wgpu::Extent3d {
                width: size[0].min(frame.width()),
                height: size[1].min(frame.height()),
                depth_or_array_layers: 1,
            },
        );
        self.primed = params.algorithm.history_pass();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("motion_blur.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn algorithms_round_trip() {
        for algorithm in Algorithm::ALL {
            assert_eq!(Algorithm::from_index(algorithm.index()), algorithm);
            assert_eq!(Algorithm::parse(algorithm.slug()), Some(algorithm));
            assert_eq!(Algorithm::parse(algorithm.label()), Some(algorithm));
        }
        for profile in Profile::ALL {
            assert_eq!(Profile::from_index(profile.index()), profile);
            assert_eq!(Profile::parse(profile.label()), Some(profile));
        }
    }

    #[test]
    fn only_velocity_profiles_take_taps() {
        assert!(Algorithm::VelocityBased.velocity_pass());
        assert!(!Algorithm::VelocityBased.history_pass());
        assert!(Algorithm::HybridBlending.velocity_pass());
        assert!(Algorithm::HybridBlending.history_pass());
        for algorithm in [
            Algorithm::FrameBlending,
            Algorithm::AccumulationMax,
            Algorithm::AccumulationMix,
        ] {
            assert!(!algorithm.velocity_pass());
            assert!(algorithm.history_pass());
        }
        assert!(Algorithm::AccumulationMax.max_blend());
        assert!(!Algorithm::AccumulationMix.max_blend());
    }

    #[test]
    fn samples_climb_with_strength() {
        assert_eq!(Params::samples_for(0.0), 4);
        assert_eq!(Params::samples_for(1.0), 16);
        assert!(Params::samples_for(0.5) > Params::samples_for(0.1));
    }

    #[test]
    fn refresh_scaling_compares_a_frame_to_the_panel() {
        let mut params = Params {
            strength: 0.5,
            ..Default::default()
        };
        // 60 fps on a 60 Hz panel is the reference.
        let base = params.scaled_strength(1.0 / 60.0, 60.0, true);
        assert!((base - 0.5).abs() < 1.0e-5);
        // A 30 fps frame smears less than twice as far, never more.
        let slow = params.scaled_strength(1.0 / 30.0, 60.0, true);
        assert!(slow > base && slow <= 1.0);
        // Scaling off returns the slider untouched.
        assert_eq!(params.scaled_strength(1.0 / 12.0, 60.0, false), 0.5);
        params.strength = 1.0;
        assert_eq!(params.scaled_strength(1.0 / 12.0, 60.0, false), 1.0);
    }
}
