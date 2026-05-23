use crate::vulkan::vk_init::Aura;
use ash::vk::{self, TaggedStructure};

#[allow(dead_code)]
impl Aura {
    fn create_video_descriptor_set_layout(
        device: &ash::Device,
        video_sampler: &vk::Sampler,
    ) -> vk::DescriptorSetLayout {
        let layout_binding = vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            .immutable_samplers(std::slice::from_ref(video_sampler));
        let layout_info = vk::DescriptorSetLayoutCreateInfo::default()
            .bindings(std::slice::from_ref(&layout_binding));
        unsafe {
            device
                .create_descriptor_set_layout(&layout_info, None)
                .expect("Failed to create Descriptor Set Layout to the video.")
        }
    }

    fn update_video_descriptor_set(
        device: &ash::Device,
        descriptor_set: vk::DescriptorSet,
        current_dpb_image_view: vk::ImageView,
    ) {
        let image_info = vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(current_dpb_image_view)
            .sampler(vk::Sampler::null());
        let write_set = vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(std::slice::from_ref(&image_info));
        unsafe { device.update_descriptor_sets(&[write_set], &[]) };
    }
    fn create_pipeline_layout(
        device: &ash::Device,
        descriptor_set_layouts: &[vk::DescriptorSetLayout],
    ) -> vk::PipelineLayout {
        let pipeline_layout_create_info =
            vk::PipelineLayoutCreateInfo::default().set_layouts(descriptor_set_layouts);
        unsafe {
            device
                .create_pipeline_layout(&pipeline_layout_create_info, None)
                .unwrap()
        }
    }
    fn create_pipeline(
        device: &ash::Device,
        pipeline_layout: vk::PipelineLayout,
        shader_stages: &[vk::PipelineShaderStageCreateInfo],
    ) -> vk::Pipeline {
        let mut rendering_info = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&[vk::Format::B8G8R8A8_SRGB])
            .depth_attachment_format(vk::Format::UNDEFINED)
            .stencil_attachment_format(vk::Format::UNDEFINED);
        let input_assembly_info = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        let rasterization_info = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::BACK)
            .front_face(vk::FrontFace::CLOCKWISE)
            .line_width(1.0);

        let multisample_info = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        let color_blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(false);
        let color_blend_info = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(std::slice::from_ref(&color_blend_attachment));

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let pipeline_create_info = vk::GraphicsPipelineCreateInfo::default()
            .push(&mut rendering_info)
            .stages(shader_stages)
            .color_blend_state(&color_blend_info)
            .input_assembly_state(&input_assembly_info)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterization_info)
            .multisample_state(&multisample_info)
            .render_pass(vk::RenderPass::null())
            .layout(pipeline_layout)
            .subpass(0)
            .dynamic_state(&dynamic_state_info);
        unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_create_info], None)
                .unwrap()[0]
        }
    }
}
