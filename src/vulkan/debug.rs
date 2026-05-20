use ash::vk;
use std::ffi::CStr;
pub unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    unsafe {
        let callback_data = *p_callback_data;
        let message = CStr::from_ptr(callback_data.p_message).to_string_lossy();
        println!("[Vulkan Validation Layers] {:?} - {:?}", message_severity, message);
    }
    vk::FALSE
}
