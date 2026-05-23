use crate::vulkan::vk_init::Aura;
use ash::vk;
impl Aura {
    fn create_pipeline(device: &ash::Device) -> vk::Pipeline {

        let pipeline_layout_create_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&[]);
        let pipeline_create_info = vk::GraphicsPipelineCreateInfo::default();
        unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_create_info], None).unwrap()[0] }
    }
}