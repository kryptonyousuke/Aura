use crate::vulkan::vk_init::Aura;
use ash::vk::{self, TaggedStructure};
use owo_colors::OwoColorize;
use std::ffi::CStr;

pub extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let callback_data = unsafe { *p_callback_data };
    if callback_data.p_message.is_null() {
        return vk::FALSE;
    }

    let raw_message = unsafe { CStr::from_ptr(callback_data.p_message).to_string_lossy() };

    let (severity_label, message_color) = match message_severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => {
            ("❌ VULKAN ERROR", owo_colors::AnsiColors::Red)
        }
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => {
            ("⚠️  VULKAN WARNING", owo_colors::AnsiColors::Yellow)
        }
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => {
            ("ℹ️  VULKAN INFO", owo_colors::AnsiColors::Green)
        }
        _ => ("🔍 VULKAN VERBOSE", owo_colors::AnsiColors::BrightBlack),
    };
    let vuid = if let Some(start) = raw_message.find("[ ") {
        if let Some(end) = raw_message[start..].find(" ]") {
            Some(&raw_message[start + 2..start + end])
        } else {
            None
        }
    } else {
        None
    };
    println!("{}", "─".repeat(80).dimmed());

    if let Some(id) = vuid {
        println!(
            "{} : {}",
            severity_label.color(message_color).bold(),
            id.cyan().bold()
        );
    } else {
        println!("{}", severity_label.color(message_color).bold());
    }

    println!(
        "{}\n  {:?}\n{}",
        "Type:".bright_black().bold(),
        message_type.magenta(),
        "Details:".bright_black().bold()
    );
    for line in raw_message.lines() {
        println!("  {}", line);
    }

    println!("{}", "─".repeat(80).dimmed());

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
    pub fn log_instance_extensions(entry: &ash::Entry, required_instance_extensions: &Vec<&CStr>){
        let available_extensions = unsafe {
            entry
                .enumerate_instance_extension_properties(None)
                .unwrap()
                .into_iter()
        };
        log::info!("------------ Available Instance Extensions -----------");
        for extension in available_extensions {
            log::info!("{:?}", extension.extension_name_as_c_str().unwrap());
        }
        log::info!("------------ Required Instance Extensions -----------");
        for extension_c_str in required_instance_extensions {
            log::info!("Extension: {:#?}", extension_c_str);
        }
        log::info!("------------------------------------------------------");
    }
}
