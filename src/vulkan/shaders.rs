use crate::vulkan::vk_init::Aura;
use std::ffi::{CStr};
use ash::vk;
impl Aura {
    fn create_shader_stages(device: &ash::Device) -> [vk::PipelineShaderStageCreateInfo<'_>; 2]{
        let frag_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/show_texture.frag.spv"));
        let vert_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/full_screen.vert.spv"));
        let vert_u32 = ash::util::read_spv(&mut std::io::Cursor::new(&vert_bytes[..]))
            .expect("Failed to read vertex SPIR-V");
        let frag_u32 = ash::util::read_spv(&mut std::io::Cursor::new(&frag_bytes[..]))
            .expect("Failed to read fragment SPIR-V");

        let vert_module = unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&vert_u32), None)
                .expect("Failed to create vertex shader module")
        };

        let frag_module = unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&frag_u32), None)
                .expect("Failed to create fragment shader module")
        };
        let entry_point: &'static CStr = c"main";
        [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(&entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(&entry_point),
        ]
    }
}