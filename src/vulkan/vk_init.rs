use super::debug;
use crate::vulkan::decoder::Decoder;
use crate::vulkan::decoders::h264::H264Decoder;
use ash::vk::{DebugUtilsMessengerEXT, TaggedStructure};
use ash::{
    Entry, Instance,
    khr::{video_decode_queue::Device as VideoDecodeLoader, video_queue},
    vk,
};
use raw_window_handle::{self, HasDisplayHandle, HasWindowHandle};
use std::ffi::CStr;

pub struct DecodeExtensions;

impl DecodeExtensions {
    pub const H264: &'static CStr = c"VK_KHR_video_decode_h264";
    pub const H265: &'static CStr = c"VK_KHR_video_decode_h265";
    pub const AV1: &'static CStr = c"VK_KHR_video_decode_av1";
}

pub struct DecodingSession {
    pub(super) session: vk::VideoSessionKHR,
    pub(super) _session_memories: Vec<vk::DeviceMemory>,
    pub(super) video_loader: video_queue::Device,
    pub(super) decode_loader: VideoDecodeLoader,
    pub(super) session_parameters: vk::VideoSessionParametersKHR,
}

struct SupportedCodecs {
    h264: bool,
    h265: bool,
    av1: bool,
}
impl Default for SupportedCodecs {
    fn default() -> Self {
        Self {
            h264: false,
            h265: false,
            av1: false,
        }
    }
}

#[allow(dead_code)]
pub struct Aura {
    pub(super) _entry: Entry,
    pub _instance: Instance,
    pub video_instance_ext: ash::khr::video_queue::Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub _debug_utils_loader: ash::ext::debug_utils::Instance,
    pub _debug_messenger: DebugUtilsMessengerEXT,
    pub(super) _video_queue_family_index: u32,
    pub(super) _graphics_queue_family_index: u32,
    pub(super) _session_memories: Vec<vk::DeviceMemory>,
    pub(super) session: vk::VideoSessionKHR,
    pub(super) bitstream_buffer: vk::Buffer,
    pub(super) bitstream_memory: vk::DeviceMemory,
    pub(super) video_loader: video_queue::Device,
    pub(super) decode_loader: VideoDecodeLoader,
    pub(super) graphics_queue: vk::Queue,
    pub(super) video_queue: vk::Queue,
    pub(super) surface: vk::SurfaceKHR,
    pub(super) surface_loader: ash::khr::surface::Instance,
    pub(super) session_parameters: vk::VideoSessionParametersKHR,
    pub(super) dpb_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>,
    pub(super) dst_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>,
    pub(super) current_frame_index: usize,
    pub(super) dpb_pool_size: usize,
    pub graphics_command_pool: vk::CommandPool,
    pub graphics_command_buffer: vk::CommandBuffer,
    pub video_command_pool: vk::CommandPool,
    pub video_command_buffers: Vec<vk::CommandBuffer>,
    pub swapchain_loader: ash::khr::swapchain::Device,
    pub swapchain: vk::SwapchainKHR,
    pub swapchain_images: Vec<vk::Image>,
    pub swapchain_image_views: Vec<vk::ImageView>,
    pub swapchain_format: vk::Format,
    pub swapchain_extent: vk::Extent2D,
    pub present_complete_semaphores: Vec<vk::Semaphore>,
    pub render_complete_semaphores: Vec<vk::Semaphore>,
    pub render_fences: Vec<vk::Fence>,
    pub extent: vk::Extent2D,
    pub ycbcr_conversion: vk::SamplerYcbcrConversion,
    pub frames_in_flight: u8,
    supported_decoders: SupportedCodecs,
}

impl Aura {
    // Constants

    pub fn new(window: &winit::window::Window) -> Self {
        const FRAMES_IN_FLIGHT: u8 = 3;
        let dpb_pool_size = 16;
        let entry = unsafe { Entry::load().expect("Failed to load vulkan driver.") };
        let validation_layer = c"VK_LAYER_KHRONOS_validation";
        let layer_names: Vec<*const std::os::raw::c_char> = if cfg!(debug_assertions) {
            vec![validation_layer.as_ptr()]
        } else {
            vec![]
        };
        let layers_pointers: Vec<*const std::os::raw::c_char> =
            layer_names.iter().map(|&name| name).collect();
        let mut instance_extensions =
            ash_window::enumerate_required_extensions(window.display_handle().unwrap().as_raw())
                .expect("Failed to retrieve window extensions.")
                .to_vec();
        instance_extensions.push(ash::ext::debug_utils::NAME.as_ptr());
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
        for extension_ptr in &instance_extensions {
            unsafe {
                let c_str = std::ffi::CStr::from_ptr(*extension_ptr);
                let name = c_str.to_string_lossy();
                log::info!("Extensão: {}", name);
            }
        }
        log::info!("------------------------------------------------------");

        log::info!("Creating Vulkan instance.");
        let app_name = c"Aura";
        let app_info = vk::ApplicationInfo::default()
            .application_name(app_name)
            .api_version(vk::make_api_version(0, 1, 4, 0));

        let instance_create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_layer_names(&layers_pointers)
            .enabled_extension_names(&instance_extensions);

        let instance = unsafe {
            entry
                .create_instance(&instance_create_info, None)
                .expect("Failed to create a new vulkan instance.")
        };
        let surface_loader = ash::khr::surface::Instance::load(&entry, &instance);
        let surface_factory = ash_window::SurfaceFactory::new(
            &entry,
            &instance,
            window.display_handle().unwrap().as_raw(),
        )
        .unwrap();

        let surface = unsafe {
            ash_window::SurfaceFactory::create_surface(
                &surface_factory,
                window.window_handle().unwrap().as_raw(),
                None,
            )
            .unwrap()
        };
        let video_instance_ext = video_queue::Instance::load(&entry, &instance);
        let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(debug::vulkan_debug_callback));

        let _debug_utils_loader = ash::ext::debug_utils::Instance::load(&entry, &instance);
        let _debug_messenger = unsafe {
            _debug_utils_loader
                .create_debug_utils_messenger(&debug_info, None)
                .expect("Failed to create a debug vulkan messenger.")
        };

        let (
            physical_device,
            (graphics_queue_family_index, decode_queue_family_index),
            supported_decoders,
        ) = Self::select_physical_device(&instance, &surface_loader, surface);
        let device_extensions = [
            ash::khr::swapchain::NAME.as_ptr(),
            vk::KHR_VIDEO_QUEUE_NAME.as_ptr(),
            vk::KHR_VIDEO_DECODE_QUEUE_NAME.as_ptr(),
            vk::KHR_VIDEO_DECODE_H264_NAME.as_ptr(),
        ];

        let mut sync2_features =
            vk::PhysicalDeviceSynchronization2Features::default().synchronization2(true);
        let mut video_maintenance =
            vk::PhysicalDeviceVideoMaintenance1FeaturesKHR::default().video_maintenance1(true);
        let mut sampler_ycbcr_conversion =
            vk::PhysicalDeviceSamplerYcbcrConversionFeaturesKHR::default()
                .sampler_ycbcr_conversion(true);

        let queue_priorities = [1.0f32];
        let queue_info = [
            vk::DeviceQueueCreateInfo::default()
                .queue_family_index(decode_queue_family_index)
                .queue_priorities(&queue_priorities),
            vk::DeviceQueueCreateInfo::default()
                .queue_family_index(graphics_queue_family_index)
                .queue_priorities(&queue_priorities),
        ];

        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&device_extensions)
            .push(&mut sync2_features)
            .push(&mut video_maintenance)
            .push(&mut sampler_ycbcr_conversion);

        let device = unsafe {
            instance
                .create_device(physical_device, &device_create_info, None)
                .expect("Failed to create a logical device.")
        };

        // Ycbcr Sampler
        let ycbcr_conversion =
            unsafe { Self::create_ycbcr_conversion(&device, vk::Format::G8_B8R8_2PLANE_420_UNORM) };
        let mut h264_profile = vk::VideoDecodeH264ProfileInfoKHR::default()
            .std_profile_idc(vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN);
        let mut video_profile = vk::VideoProfileInfoKHR::default()
            .push(&mut h264_profile)
            .video_codec_operation(vk::VideoCodecOperationFlagsKHR::DECODE_H264)
            .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::TYPE_420)
            .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8);

        log::info!(
            "\nThis is being shown just for your knowledge, this code won't work with a lot of cards for now."
        );

        Self::log_formats(
            physical_device,
            &video_profile,
            &video_instance_ext,
            vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR | vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR,
            "VIDEO_DECODE_DPB_KHR | VIDEO_DECODE_DST_KHR",
        );
        Self::log_formats(
            physical_device,
            &video_profile,
            &video_instance_ext,
            vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR,
            "VIDEO_DECODE_DPB_KHR",
        );
        Self::log_formats(
            physical_device,
            &video_profile,
            &video_instance_ext,
            vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR,
            "VIDEO_DECODE_DST_KHR",
        );

        let DecodingSession {
            session,
            _session_memories,
            video_loader,
            decode_loader,
            session_parameters,
        } = Self::setup_decoder(
            &instance,
            physical_device,
            &device,
            decode_queue_family_index,
        );

        let (bitstream_buffer, bitstream_memory, _) = Aura::create_bitstream_buffer(
            &instance,
            &video_instance_ext,
            physical_device,
            &device,
            &video_profile,
        );
        let (
            swapchain_loader,
            swapchain,
            swapchain_images,
            swapchain_image_views,
            swapchain_format,
            swapchain_extent,
        ) = unsafe {
            Self::create_swapchain(
                &instance,
                &surface_loader,
                surface,
                physical_device,
                &device,
                window,
            )
        };

        let video_queue = unsafe { device.get_device_queue(decode_queue_family_index, 0) };
        let graphics_queue = unsafe { device.get_device_queue(graphics_queue_family_index, 0) };
        let graphics_pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(graphics_queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let graphics_command_pool = unsafe {
            device
                .create_command_pool(&graphics_pool_info, None)
                .expect("Failed to create a Graphics Command Pool.")
        };

        let video_pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(decode_queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);

        let video_command_pool = unsafe {
            device
                .create_command_pool(&video_pool_info, None)
                .expect("Failed to create a Video Command Pool.")
        };
        let graphics_cmd_alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(graphics_command_pool)
            .command_buffer_count(1);
        let graphics_command_buffer = unsafe {
            device
                .allocate_command_buffers(&graphics_cmd_alloc)
                .unwrap()[0]
        };

        let video_alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(video_command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT as u32);
        let video_command_buffers =
            unsafe { device.allocate_command_buffers(&video_alloc_info).unwrap() };

        let (dpb_pool, dst_pool) = Self::create_dpb_dst_pool(
            &instance,
            physical_device,
            &device,
            &mut video_profile,
            ycbcr_conversion,
            dpb_pool_size,
        );
        let semaphore_create_info = vk::SemaphoreCreateInfo::default();
        let frames_in_flight_fences_info =
            vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

        let mut present_complete_semaphores = Vec::new();
        let mut render_complete_semaphores = Vec::new();

        for _ in 0..swapchain_images.len() {
            unsafe {
                present_complete_semaphores.push(
                    device
                        .create_semaphore(&semaphore_create_info, None)
                        .unwrap(),
                );
                render_complete_semaphores.push(
                    device
                        .create_semaphore(&semaphore_create_info, None)
                        .unwrap(),
                );
            }
        }

        let mut frames_in_flight_fences = Vec::with_capacity(FRAMES_IN_FLIGHT as usize);

        for _ in 0..FRAMES_IN_FLIGHT {
            unsafe {
                frames_in_flight_fences.push(
                    device
                        .create_fence(&frames_in_flight_fences_info, None)
                        .unwrap(),
                );
            }
        }

        Self {
            _entry: entry,
            _instance: instance,
            device: device,
            surface: surface,
            surface_loader: surface_loader,
            _video_queue_family_index: decode_queue_family_index,
            _graphics_queue_family_index: graphics_queue_family_index,

            video_instance_ext: video_instance_ext,
            physical_device: physical_device,
            session: session,
            graphics_command_pool: graphics_command_pool,
            video_command_pool: video_command_pool,
            _session_memories: _session_memories,
            _debug_utils_loader: _debug_utils_loader,
            _debug_messenger: _debug_messenger,
            bitstream_buffer: bitstream_buffer,
            bitstream_memory: bitstream_memory,
            video_loader: video_loader,
            decode_loader: decode_loader,
            ycbcr_conversion: ycbcr_conversion,
            graphics_command_buffer: graphics_command_buffer,
            video_command_buffers: video_command_buffers,
            graphics_queue: graphics_queue,
            video_queue: video_queue,
            session_parameters: session_parameters,
            dpb_pool: dpb_pool,
            dst_pool: dst_pool,
            current_frame_index: 0,
            dpb_pool_size: dpb_pool_size,
            swapchain_loader: swapchain_loader,
            swapchain: swapchain,
            swapchain_images: swapchain_images,
            swapchain_image_views: swapchain_image_views,
            swapchain_format: swapchain_format,
            swapchain_extent: swapchain_extent,
            present_complete_semaphores: present_complete_semaphores,
            render_complete_semaphores: render_complete_semaphores,
            render_fences: frames_in_flight_fences,
            extent: vk::Extent2D {
                width: 1920,
                height: 1080,
            },
            frames_in_flight: FRAMES_IN_FLIGHT,
            supported_decoders: supported_decoders,
        }
    }

    fn create_dpb_dst_pool(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
        video_profile: &mut vk::VideoProfileInfoKHR,
        ycbcr_conversion: vk::SamplerYcbcrConversion,
        dpb_pool_size: usize,
    ) -> (
        Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>,
        Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>,
    ) {
        let _output_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)> =
            Vec::with_capacity(dpb_pool_size);
        let dpb_format = vk::Format::G8_B8R8_2PLANE_420_UNORM;
        let mut profile_list =
            vk::VideoProfileListInfoKHR::default().profiles(std::slice::from_ref(&video_profile));
        let dpb_image_info = vk::ImageCreateInfo::default()
            .push(&mut profile_list)
            .image_type(vk::ImageType::TYPE_2D)
            .format(dpb_format)
            .extent(vk::Extent3D {
                width: 1920,
                height: 1080,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(dpb_pool_size as u32)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let dpb_image = unsafe { device.create_image(&dpb_image_info, None).unwrap() };

        let dst_image_info = vk::ImageCreateInfo::default()
            .push(&mut profile_list)
            .image_type(vk::ImageType::TYPE_2D)
            .format(dpb_format)
            .extent(vk::Extent3D {
                width: 1920,
                height: 1080,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(dpb_pool_size as u32)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR | vk::ImageUsageFlags::SAMPLED)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let dst_image = unsafe { device.create_image(&dst_image_info, None).unwrap() };

        let dpb_mem_requirements = unsafe { device.get_image_memory_requirements(dpb_image) };
        let dst_mem_requirements = unsafe { device.get_image_memory_requirements(dst_image) };
        let mem_properties =
            unsafe { instance.get_physical_device_memory_properties(physical_device) };

        let dpb_memory_type_index = (0..mem_properties.memory_type_count)
            .find(|&i| {
                let supported = (dpb_mem_requirements.memory_type_bits & (1 << i)) != 0;
                let flags = mem_properties.memory_types[i as usize].property_flags;
                supported && flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            })
            .expect("Your GPU doesn't support the needed memory requirements for a DPB creation.");

        let dst_memory_type_index = (0..mem_properties.memory_type_count)
            .find(|&i| {
                let supported = (dst_mem_requirements.memory_type_bits & (1 << i)) != 0;
                let flags = mem_properties.memory_types[i as usize].property_flags;
                supported && flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            })
            .expect("Your GPU doesn't support the needed memory requirements for a DST creation.");

        let dpb_alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(dpb_mem_requirements.size)
            .memory_type_index(dpb_memory_type_index);
        let dpb_memory = unsafe { device.allocate_memory(&dpb_alloc_info, None).unwrap() };

        let dst_alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(dst_mem_requirements.size)
            .memory_type_index(dst_memory_type_index);
        let dst_memory = unsafe { device.allocate_memory(&dst_alloc_info, None).unwrap() };

        unsafe {
            device.bind_image_memory(dpb_image, dpb_memory, 0).unwrap();
            device.bind_image_memory(dst_image, dst_memory, 0).unwrap();
        }

        let mut dst_pool = Vec::with_capacity(dpb_pool_size);
        let mut dpb_pool = Vec::with_capacity(dpb_pool_size);

        for i in 0..dpb_pool_size {
            let dpb_view_info = vk::ImageViewCreateInfo::default()
                .image(dpb_image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: i as u32,
                    layer_count: 1,
                });

            let mut local_ycbcr_info =
                vk::SamplerYcbcrConversionInfo::default().conversion(ycbcr_conversion);

            let dst_view_info = vk::ImageViewCreateInfo::default()
                .push(&mut local_ycbcr_info)
                .image(dst_image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: i as u32,
                    layer_count: 1,
                });

            let dpb_image_view = unsafe { device.create_image_view(&dpb_view_info, None).unwrap() };
            let dst_image_view = unsafe { device.create_image_view(&dst_view_info, None).unwrap() };

            let dpb_mem_handle = if i == 0 {
                dpb_memory
            } else {
                vk::DeviceMemory::null()
            };
            let dst_mem_handle = if i == 0 {
                dst_memory
            } else {
                vk::DeviceMemory::null()
            };

            dpb_pool.push((dpb_image, dpb_mem_handle, dpb_image_view));
            dst_pool.push((dst_image, dst_mem_handle, dst_image_view));
        }
        (dpb_pool, dst_pool)
    }

    fn setup_decoder(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
        queue_family_index: u32,
    ) -> DecodingSession {
        let video_loader = video_queue::Device::load(instance, device);
        let decode_loader = VideoDecodeLoader::load(instance, device);

        let session = Aura::create_video_session(instance, device, queue_family_index);

        let session_parameters =
            unsafe { Aura::create_h264_session_parameters(device, &video_loader, session) };

        let session_memories = Aura::bind_video_session_memory(
            instance,
            physical_device,
            device,
            &video_loader,
            session,
        );

        DecodingSession {
            session: session,
            _session_memories: session_memories,
            video_loader: video_loader,
            decode_loader: decode_loader,
            session_parameters: session_parameters,
        }
    }

    unsafe fn create_swapchain(
        instance: &ash::Instance,
        surface_loader: &ash::khr::surface::Instance,
        surface: vk::SurfaceKHR,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
        window: &winit::window::Window,
    ) -> (
        ash::khr::swapchain::Device,
        vk::SwapchainKHR,
        Vec<vk::Image>,
        Vec<vk::ImageView>,
        vk::Format,
        vk::Extent2D,
    ) {
        let capabilities = unsafe {
            surface_loader
                .get_physical_device_surface_capabilities(physical_device, surface)
                .unwrap()
        };
        let formats = unsafe {
            surface_loader
                .get_physical_device_surface_formats(physical_device, surface)
                .unwrap()
        };
        let present_modes = unsafe {
            surface_loader
                .get_physical_device_surface_present_modes(physical_device, surface)
                .unwrap()
        };

        let format = formats
            .iter()
            .cloned()
            .find(|f| {
                f.format == vk::Format::B8G8R8A8_SRGB
                    && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .unwrap_or(formats[0]);

        let present_mode = if present_modes.contains(&vk::PresentModeKHR::MAILBOX) {
            vk::PresentModeKHR::MAILBOX
        } else {
            vk::PresentModeKHR::FIFO
        };

        let extent = if capabilities.current_extent.width != u32::MAX {
            capabilities.current_extent
        } else {
            let size = window.inner_size();
            vk::Extent2D {
                width: size.width.clamp(
                    capabilities.min_image_extent.width,
                    capabilities.max_image_extent.width,
                ),
                height: size.height.clamp(
                    capabilities.min_image_extent.height,
                    capabilities.max_image_extent.height,
                ),
            }
        };

        let mut image_count = capabilities.min_image_count + 1;
        if capabilities.max_image_count > 0 && image_count > capabilities.max_image_count {
            image_count = capabilities.max_image_count;
        }

        let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true);

        let swapchain_loader = ash::khr::swapchain::Device::load(instance, device);
        let swapchain = unsafe {
            swapchain_loader
                .create_swapchain(&swapchain_create_info, None)
                .unwrap()
        };

        let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(swapchain).unwrap() };

        let swapchain_image_views = swapchain_images
            .iter()
            .map(|&image| {
                let create_view_info = vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format.format)
                    .components(vk::ComponentMapping {
                        r: vk::ComponentSwizzle::IDENTITY,
                        g: vk::ComponentSwizzle::IDENTITY,
                        b: vk::ComponentSwizzle::IDENTITY,
                        a: vk::ComponentSwizzle::IDENTITY,
                    })
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    });

                unsafe { device.create_image_view(&create_view_info, None).unwrap() }
            })
            .collect::<Vec<_>>();

        (
            swapchain_loader,
            swapchain,
            swapchain_images,
            swapchain_image_views,
            format.format,
            extent,
        )
    }

    fn select_physical_device(
        instance: &Instance,
        surface_loader: &ash::khr::surface::Instance,
        surface: vk::SurfaceKHR,
    ) -> (vk::PhysicalDevice, (u32, u32), SupportedCodecs) {
        log::info!("Selecting device.");
        let pdevices = unsafe { instance.enumerate_physical_devices().unwrap() };

        for pdevice in pdevices {
            let props = unsafe { instance.get_physical_device_queue_family_properties(pdevice) };

            let mut decode_index = None;
            let mut graphics_index = None;
            let mut supported_codecs = SupportedCodecs::default();

            for (index, prop) in props.iter().enumerate() {
                let idx = index as u32;

                if prop.queue_flags.contains(vk::QueueFlags::VIDEO_DECODE_KHR) {
                    decode_index = Some(idx);

                    let device_extensions_name = unsafe {
                        instance
                            .enumerate_device_extension_properties(pdevice)
                            .unwrap()
                    };
                    for ext_name in device_extensions_name {
                        unsafe {
                            let ext_cstr = CStr::from_ptr(ext_name.extension_name.as_ptr());
                            if ext_cstr == DecodeExtensions::H264 {
                                supported_codecs.h264 = true
                            } else if ext_cstr == DecodeExtensions::H265 {
                                supported_codecs.h265 = true
                            } else if ext_cstr == DecodeExtensions::AV1 {
                                supported_codecs.av1 = true
                            };
                        }
                    }
                }

                let supports_present = unsafe {
                    surface_loader
                        .get_physical_device_surface_support(pdevice, idx, surface)
                        .unwrap_or(false)
                };

                if prop.queue_flags.contains(vk::QueueFlags::GRAPHICS) && supports_present {
                    graphics_index = Some(idx);
                }
            }

            if let (Some(g_idx), Some(v_idx)) = (graphics_index, decode_index) {
                log::info!(
                    "GPU successfully detected! Graphic queue: {}, Video Queue: {}",
                    g_idx,
                    v_idx
                );
                return (pdevice, (g_idx, v_idx), supported_codecs);
            }
        }

        log::error!(
            "Your GPU needs to support both video decode queue and graphics queue with surface support."
        );
        std::process::abort();
    }

    pub fn find_memory_type(
        instance: &Instance,
        pdevice: vk::PhysicalDevice,
        type_filter: u32,
        props: vk::MemoryPropertyFlags,
    ) -> u32 {
        let mem_props = unsafe { instance.get_physical_device_memory_properties(pdevice) };
        for i in 0..mem_props.memory_type_count {
            if (type_filter & (1 << i)) != 0
                && (mem_props.memory_types[i as usize].property_flags & props) == props
            {
                return i;
            }
        }
        0
    }

    fn log_formats(
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

impl Drop for Aura {
    fn drop(&mut self) {
        unsafe {
            log::info!("Cleaning Vulkan instance...");

            if self.device.handle() != vk::Device::null() {
                let _ = self.device.queue_wait_idle(self.graphics_queue);
                let _ = self.device.queue_wait_idle(self.video_queue);
                let _ = self.device.device_wait_idle();
            }

            for &view in &self.swapchain_image_views {
                self.device.destroy_image_view(view, None);
            }
            log::debug!("Swapchain's Image Views were successfully destroyed.");

            if self.swapchain != vk::SwapchainKHR::null() {
                self.swapchain_loader
                    .destroy_swapchain(self.swapchain, None);
                self.swapchain = vk::SwapchainKHR::null();
                log::debug!("SwapchainKHR was succcessfully destroyed.");
            }

            if self.session_parameters != vk::VideoSessionParametersKHR::null() {
                self.video_loader
                    .destroy_video_session_parameters(self.session_parameters, None);
                self.session_parameters = vk::VideoSessionParametersKHR::null();
            }
            if self.session != vk::VideoSessionKHR::null() {
                self.video_loader.destroy_video_session(self.session, None);
                self.session = vk::VideoSessionKHR::null();
            }
            for mem in &self._session_memories {
                self.device.free_memory(*mem, None);
            }
            self.device.destroy_buffer(self.bitstream_buffer, None);
            self.device.free_memory(self.bitstream_memory, None);
            log::debug!("Video decoding resources were successfully freed and destroyed.");

            for i in 0..self.frames_in_flight {
                self.device
                    .destroy_fence(self.render_fences[i as usize], None);
            }
            for &semaphore in &self.present_complete_semaphores {
                self.device.destroy_semaphore(semaphore, None);
            }
            for &semaphore in &self.render_complete_semaphores {
                self.device.destroy_semaphore(semaphore, None);
            }
            log::debug!("Sync resources successfully destroyed.");

            self.device
                .destroy_command_pool(self.graphics_command_pool, None);
            self.device
                .destroy_command_pool(self.video_command_pool, None);
            log::debug!("Successfully destroyed all command pools.");

            for (_, _, view) in &self.dpb_pool {
                self.device.destroy_image_view(*view, None);
            }
            for (_, _, view) in &self.dst_pool {
                self.device.destroy_image_view(*view, None);
            }

            if let Some((image, memory, _)) = self.dpb_pool.first() {
                self.device.destroy_image(*image, None);
                self.device.free_memory(*memory, None);
            }
            if let Some((image, memory, _)) = self.dst_pool.first() {
                self.device.destroy_image(*image, None);
                self.device.free_memory(*memory, None);
            }
            log::debug!("DPB/DST pools were freed.");

            self.device
                .destroy_sampler_ycbcr_conversion(self.ycbcr_conversion, None);

            self.device.destroy_device(None);
            log::info!("VkDevice successfully destroyed.");

            self.surface_loader.destroy_surface(self.surface, None);
            self._debug_utils_loader
                .destroy_debug_utils_messenger(self._debug_messenger, None);
            self._instance.destroy_instance(None);

            log::info!("Your GPU is totally free of this program.");
        }
    }
}
