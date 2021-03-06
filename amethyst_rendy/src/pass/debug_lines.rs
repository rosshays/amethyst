use crate::{
    debug_drawing::{DebugLine, DebugLines, DebugLinesComponent, DebugLinesParams},
    pipeline::{PipelineDescBuilder, PipelinesBuilder},
    pod::ViewArgs,
    submodules::{gather::CameraGatherer, DynamicUniform, DynamicVertexBuffer},
    types::Backend,
    util,
};
use amethyst_core::ecs::{Join, Read, Resources, SystemData, Write, WriteStorage};
use derivative::Derivative;
use glsl_layout::*;
use rendy::{
    command::{QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{
        render::{PrepareResult, RenderGroup, RenderGroupDesc},
        GraphContext, NodeBuffer, NodeImage,
    },
    hal::{self, device::Device, pso},
    mesh::AsVertex,
    shader::Shader,
};

#[cfg(feature = "profiler")]
use thread_profiler::profile_scope;

#[derive(Debug, Clone, AsStd140)]
struct DebugLinesArgs {
    screen_space_thickness: vec2,
}

/// Draw opaque sprites without lighting.
#[derive(Clone, Debug, PartialEq, Derivative)]
#[derivative(Default(bound = ""))]
pub struct DrawDebugLinesDesc;

impl DrawDebugLinesDesc {
    /// Create instance of `DrawDebugLines` render group
    pub fn new() -> Self {
        Default::default()
    }
}

impl<B: Backend> RenderGroupDesc<B, Resources> for DrawDebugLinesDesc {
    fn build(
        self,
        _ctx: &GraphContext<B>,
        factory: &mut Factory<B>,
        _queue: QueueId,
        _aux: &Resources,
        framebuffer_width: u32,
        framebuffer_height: u32,
        subpass: hal::pass::Subpass<'_, B>,
        _buffers: Vec<NodeBuffer>,
        _images: Vec<NodeImage>,
    ) -> Result<Box<dyn RenderGroup<B, Resources>>, failure::Error> {
        #[cfg(feature = "profiler")]
        profile_scope!("build");

        let env = DynamicUniform::new(factory, pso::ShaderStageFlags::VERTEX)?;
        let args = DynamicUniform::new(factory, pso::ShaderStageFlags::VERTEX)?;
        let vertex = DynamicVertexBuffer::new();

        let (pipeline, pipeline_layout) = build_lines_pipeline(
            factory,
            subpass,
            framebuffer_width,
            framebuffer_height,
            vec![env.raw_layout(), args.raw_layout()],
        )?;

        Ok(Box::new(DrawDebugLines::<B> {
            pipeline: pipeline,
            pipeline_layout,
            env,
            args,
            vertex,
            framebuffer_width: framebuffer_width as f32,
            framebuffer_height: framebuffer_height as f32,
            lines: Vec::new(),
            change: Default::default(),
        }))
    }
}

#[derive(Debug)]
pub struct DrawDebugLines<B: Backend> {
    pipeline: B::GraphicsPipeline,
    pipeline_layout: B::PipelineLayout,
    env: DynamicUniform<B, ViewArgs>,
    args: DynamicUniform<B, DebugLinesArgs>,
    vertex: DynamicVertexBuffer<B, DebugLine>,
    framebuffer_width: f32,
    framebuffer_height: f32,
    lines: Vec<DebugLine>,
    change: util::ChangeDetection,
}

impl<B: Backend> RenderGroup<B, Resources> for DrawDebugLines<B> {
    fn prepare(
        &mut self,
        factory: &Factory<B>,
        _queue: QueueId,
        index: usize,
        _subpass: hal::pass::Subpass<'_, B>,
        resources: &Resources,
    ) -> PrepareResult {
        #[cfg(feature = "profiler")]
        profile_scope!("prepare");

        let (lines_comps, lines_res, line_params) = <(
            WriteStorage<DebugLinesComponent>,
            Option<Write<DebugLines>>,
            Option<Read<DebugLinesParams>>,
        )>::fetch(resources);

        let old_len = self.lines.len();
        self.lines.clear();
        for lines_component in (&lines_comps).join() {
            self.lines.extend_from_slice(lines_component.lines());
        }

        if let Some(mut lines_res) = lines_res {
            self.lines.extend(lines_res.drain());
        };

        let cam = CameraGatherer::gather(resources);
        let line_width = line_params
            .map(|p| p.line_width)
            .unwrap_or(DebugLinesParams::default().line_width);

        self.env.write(factory, index, cam.projview);
        self.args.write(
            factory,
            index,
            DebugLinesArgs {
                screen_space_thickness: [
                    (line_width * 2.0) / self.framebuffer_width,
                    (line_width * 2.0) / self.framebuffer_height,
                ]
                .into(),
            }
            .std140(),
        );

        {
            #[cfg(feature = "profiler")]
            profile_scope!("write");
            self.vertex
                .write(factory, index, self.lines.len() as u64, Some(&self.lines));
        }

        let changed = old_len != self.lines.len();
        self.change.prepare_result(index, changed)
    }

    fn draw_inline(
        &mut self,
        mut encoder: RenderPassEncoder<'_, B>,
        index: usize,
        _subpass: hal::pass::Subpass<'_, B>,
        _resources: &Resources,
    ) {
        #[cfg(feature = "profiler")]
        profile_scope!("draw");

        if self.lines.len() == 0 {
            return;
        }

        let layout = &self.pipeline_layout;
        encoder.bind_graphics_pipeline(&self.pipeline);
        self.env.bind(index, layout, 0, &mut encoder);
        self.args.bind(index, layout, 1, &mut encoder);
        self.vertex.bind(index, 0, 0, &mut encoder);
        encoder.draw(0..4, 0..self.lines.len() as u32);
    }

    fn dispose(self: Box<Self>, factory: &mut Factory<B>, _aux: &Resources) {
        unsafe {
            factory.device().destroy_graphics_pipeline(self.pipeline);
            factory
                .device()
                .destroy_pipeline_layout(self.pipeline_layout);
        }
    }
}

fn build_lines_pipeline<B: Backend>(
    factory: &Factory<B>,
    subpass: hal::pass::Subpass<'_, B>,
    framebuffer_width: u32,
    framebuffer_height: u32,
    layouts: Vec<&B::DescriptorSetLayout>,
) -> Result<(B::GraphicsPipeline, B::PipelineLayout), failure::Error> {
    let pipeline_layout = unsafe {
        factory
            .device()
            .create_pipeline_layout(layouts, None as Option<(_, _)>)
    }?;

    let shader_vertex = unsafe { super::DEBUG_LINES_VERTEX.module(factory).unwrap() };
    let shader_fragment = unsafe { super::DEBUG_LINES_FRAGMENT.module(factory).unwrap() };

    let pipes = PipelinesBuilder::new()
        .with_pipeline(
            PipelineDescBuilder::new()
                .with_vertex_desc(&[(DebugLine::vertex(), pso::VertexInputRate::Instance(1))])
                .with_input_assembler(pso::InputAssemblerDesc::new(hal::Primitive::TriangleStrip))
                .with_shaders(util::simple_shader_set(
                    &shader_vertex,
                    Some(&shader_fragment),
                ))
                .with_layout(&pipeline_layout)
                .with_subpass(subpass)
                .with_framebuffer_size(framebuffer_width, framebuffer_height)
                .with_blend_targets(vec![pso::ColorBlendDesc(
                    pso::ColorMask::ALL,
                    pso::BlendState::ALPHA,
                )])
                .with_depth_test(pso::DepthTest::On {
                    fun: pso::Comparison::LessEqual,
                    write: true,
                }),
        )
        .build(factory, None);

    unsafe {
        factory.destroy_shader_module(shader_vertex);
        factory.destroy_shader_module(shader_fragment);
    }

    match pipes {
        Err(e) => {
            unsafe {
                factory.device().destroy_pipeline_layout(pipeline_layout);
            }
            Err(e)
        }
        Ok(mut pipes) => Ok((pipes.remove(0), pipeline_layout)),
    }
}
