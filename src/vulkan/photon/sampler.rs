//! # Ycbcr sampler
//! Creates and configure a ycbcr sampler.


use crate::vulkan::vk_init::Aura;
use ash::vk::{self, TaggedStructure};

pub trait Sampler {
    /// Setups a yuv -> rgba conversion info for a sampler.
    unsafe fn create_ycbcr_conversion(
        device: &ash::Device,
        format: vk::Format,
        color_range: vk::SamplerYcbcrRange
    ) -> vk::SamplerYcbcrConversion;
    
    /// Create a ycbcr sampler.
    fn create_sampler(
        device: &ash::Device,
        ycbcr_conversion: vk::SamplerYcbcrConversion,
    ) -> vk::Sampler;

}

impl Sampler for Aura {
    
    unsafe fn create_ycbcr_conversion(
        device: &ash::Device,
        format: vk::Format,
        color_range: vk::SamplerYcbcrRange
    ) -> vk::SamplerYcbcrConversion {
        unsafe {
            let ycbcr_info = vk::SamplerYcbcrConversionCreateInfo::default()
                .format(format)
                .ycbcr_model(vk::SamplerYcbcrModelConversion::YCBCR_709) // hardcoded!
                .ycbcr_range(color_range)
                .components(vk::ComponentMapping {
                    r: vk::ComponentSwizzle::IDENTITY,
                    g: vk::ComponentSwizzle::IDENTITY,
                    b: vk::ComponentSwizzle::IDENTITY,
                    a: vk::ComponentSwizzle::IDENTITY,
                })
                .chroma_filter(vk::Filter::LINEAR)
                .force_explicit_reconstruction(false);

            device
                .create_sampler_ycbcr_conversion(&ycbcr_info, None)
                .expect("Failed to create YCbCr conversion.")
        }
    }
    
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
