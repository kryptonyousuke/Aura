use crate::vulkan::vk_init::Aura;
use ash::vk;
use std::ffi::CStr;

#[macro_export]
macro_rules! create_shader {
    ($device:expr, $shader_name:literal) => {{
        let shader_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/", $shader_name));
        let shader_u32 = ash::util::read_spv(&mut std::io::Cursor::new(&shader_bytes[..]))
            .expect("Failed to read shader SPIR-V");
        unsafe {
            $device
                .create_shader_module(
                    &vk::ShaderModuleCreateInfo::default().code(&shader_u32),
                    None,
                )
                .expect("Failed to create shader module")
        }
    }};
}

pub trait Shaders {
    fn create_shader_stages(
        _device: &ash::Device,
        vert_module: vk::ShaderModule,
        frag_module: vk::ShaderModule,
    ) -> [vk::PipelineShaderStageCreateInfo<'_>; 2];
}

impl Shaders for Aura {
    fn create_shader_stages(
        _device: &ash::Device,
        vert_module: vk::ShaderModule,
        frag_module: vk::ShaderModule,
    ) -> [vk::PipelineShaderStageCreateInfo<'_>; 2] {
        let entry_point: &'static CStr = c"main";
        [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(entry_point),
        ]
    }
}
