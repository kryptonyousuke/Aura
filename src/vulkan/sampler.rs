use crate::vulkan::vk_init::Aura;
use ash::vk::{self, TaggedStructure};

pub trait Sampler {
    fn create_sampler(
        device: &ash::Device,
        ycbcr_conversion: vk::SamplerYcbcrConversion,
    ) -> vk::Sampler;
}

impl Sampler for Aura {
    fn create_sampler(
        device: &ash::Device,
        ycbcr_conversion: vk::SamplerYcbcrConversion,
    ) -> vk::Sampler {
        let mut conversion_info =
            vk::SamplerYcbcrConversionInfo::default().conversion(ycbcr_conversion);

        let sampler_create_info = vk::SamplerCreateInfo::default()
            .push(&mut conversion_info)
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        unsafe { device.create_sampler(&sampler_create_info, None).unwrap() }
    }
}
