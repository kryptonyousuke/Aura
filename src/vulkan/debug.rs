use ash::vk::{self, TaggedStructure};
use std::ffi::CStr;
use crate::vulkan::vk_init::Aura;

pub unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    unsafe {
        let callback_data = *p_callback_data;
        let message = CStr::from_ptr(callback_data.p_message).to_string_lossy();
        println!(
            "[Vulkan Validation Layers] {:?} - {:?}",
            message_severity, message
        );
    }
    vk::FALSE
}
impl Aura {
    pub fn log_formats(
        physical_device: vk::PhysicalDevice,
        video_profile: &vk::VideoProfileInfoKHR,
        video_instance_ext: &ash::khr::video_queue::Instance,
        image_usage_flags: vk::ImageUsageFlags,
        identifier: &str,
    ) {
        let mut profile_list =
            vk::VideoProfileListInfoKHR::default().profiles(std::slice::from_ref(video_profile));
        let format_info = vk::PhysicalDeviceVideoFormatInfoKHR::default()
            .image_usage(image_usage_flags)
            .push(&mut profile_list);

        let supported_formats_len_result = unsafe {
            video_instance_ext
                .get_physical_device_video_format_properties_len(physical_device, &format_info)
        };
        log::info!(
            "-------- GPU's supported formats for {} ---------",
            identifier
        );

        if supported_formats_len_result.is_ok() {
            let mut supported_formats: Vec<vk::VideoFormatPropertiesKHR> = vec![
                    vk::VideoFormatPropertiesKHR::default();
                    supported_formats_len_result.unwrap() as usize
                ];

            let result = unsafe {
                video_instance_ext.get_physical_device_video_format_properties(
                    physical_device,
                    &format_info,
                    &mut supported_formats,
                )
            };
            if result.is_ok() {
                for (i, prop) in supported_formats.iter().enumerate() {
                    log::info!("- Config #{}:", i);
                    log::info!("Image Type: {:?}", prop.image_type);
                    log::info!("Format: {:?}", prop.format);
                    log::info!("Tiling: {:?}", prop.image_tiling);
                    log::info!("Component Mapping: {:?}", prop.component_mapping);
                    log::info!("Flags: {:?}", prop.image_usage_flags);
                }
            } else {
                log::info!("Config #0: Format: None");
            }
        } else {
            log::info!("Config #0: Format: None");
        }
        log::info!("------------------------------------------");
    }
}
