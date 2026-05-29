use super::debug;
use crate::vulkan::decoder::{Decoder, DecodingSession, SupportedCodecs, DecodeExtensions};
use crate::vulkan::decoders::h264::H264Decoder;
use crate::vulkan::pipeline::Pipeline;
use crate::vulkan::sampler::Sampler;
use crate::vulkan::shaders::Shaders;
use ash::{
    Entry, Instance,
    khr::{video_decode_queue::Device as VideoDecodeLoader, video_queue},
    vk,
    vk::{DebugUtilsMessengerEXT, TaggedStructure}
};
use raw_window_handle::{self, HasDisplayHandle, HasWindowHandle};
use std::ffi::CStr;
pub const FRAMES_IN_FLIGHT: u8 = 3;
const DPB_POOL_SIZE: usize = 16;
#[allow(dead_code)]
pub struct Aura {
    pub _entry: Entry,
    pub _instance: Instance,
    pub _debug_utils_loader: ash::ext::debug_utils::Instance,
    pub _debug_messenger: DebugUtilsMessengerEXT,
    pub _video_queue_family_index: u32,
    pub _graphics_queue_family_index: u32,
    pub _session_memories: Vec<vk::DeviceMemory>,

    
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,

    pub session: vk::VideoSessionKHR,
    pub video_instance_ext: ash::khr::video_queue::Instance,
    pub bitstream_buffers: [vk::Buffer; FRAMES_IN_FLIGHT as usize],
    pub bitstream_memories: [vk::DeviceMemory; FRAMES_IN_FLIGHT as usize],
    pub bitstream_sizes: [u32; FRAMES_IN_FLIGHT as usize],
    pub video_loader: video_queue::Device,
    pub decode_loader: VideoDecodeLoader,
    
    pub graphics_queue: vk::Queue,
    pub video_queue: vk::Queue,
    pub surface: vk::SurfaceKHR,
    pub surface_loader: ash::khr::surface::Instance,
    pub session_parameters: vk::VideoSessionParametersKHR,
    
    /*
     * My VCN 2.0 doesn't support a single pool for dst and dpb at the same time.
     * This should be changed in the future.
     */
    pub dpb_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>, // Decoded Pictures Buffer used as reference to decode P-frames and B-frames.
    pub dst_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>, // Stores the current decoded image.
    pub dpb_and_dst_format: vk::Format,
    pub dpb_pocs: Vec<i32>,
    pub dpb_pool_size: usize,
    pub dpb_slot_valid: Vec<bool>,
    pub dpb_frame_nums: [u16; DPB_POOL_SIZE],
    pub current_frame_count_idx: usize,
    pub graphics_command_pool: vk::CommandPool,
    pub graphics_command_buffers: Vec<vk::CommandBuffer>,
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
    pub graphics_complete_semaphores: Vec<vk::Semaphore>,
    pub render_fences: [vk::Fence; FRAMES_IN_FLIGHT as usize],

    pub pipeline_layout: vk::PipelineLayout,
    pub pipeline: vk::Pipeline,
    pub descriptor_set_layouts: Vec<vk::DescriptorSetLayout>,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,
    pub ycbcr_conversion: vk::SamplerYcbcrConversion,
    video_sampler: vk::Sampler,

    pub viewport: vk::Viewport,
    pub scissor: vk::Rect2D,
    pub video_extent: vk::Extent2D,
    pub frames_in_flight: u8,
    supported_decoders: SupportedCodecs,
}

impl Aura {
    // Constants

    pub fn new(window: &winit::window::Window, extradata: &Vec<u8>) -> Self {

        let entry = unsafe { Entry::load().expect("Failed to load vulkan driver.") };
        match unsafe { entry.try_enumerate_instance_version().unwrap() } {
            Some(version) => {
                let major = vk::api_version_major(version);
                let minor = vk::api_version_minor(version);
                let patch = vk::api_version_patch(version);
                log::info!("Vulkan {}.{}.{}", major, minor, patch);
            }
            None => log::info!("Vulkan 1.0"),
        }
        let validation_layer = c"VK_LAYER_KHRONOS_validation";
        let layer_names: Vec<*const std::os::raw::c_char> = if cfg!(debug_assertions) {
            vec![validation_layer.as_ptr()]
        } else {
            vec![]
        };
        let layers_pointers: Vec<*const std::os::raw::c_char> =
            layer_names.iter().map(|&name| name).collect();
        let mut required_instance_extensions: Vec<&CStr> =
            ash_window::enumerate_required_extensions(window.display_handle().unwrap().as_raw())
                .expect("Failed to retrieve window extensions.")
                .iter()
                .map(|&ptr| unsafe { CStr::from_ptr(ptr) })
                .collect();
        required_instance_extensions.push(ash::ext::debug_utils::NAME);
        required_instance_extensions.push(vk::KHR_SURFACE_MAINTENANCE1_NAME);
        required_instance_extensions.push(vk::KHR_GET_SURFACE_CAPABILITIES2_NAME);
        Self::log_instance_extensions(&entry, &required_instance_extensions);
        let extension_pointers: Vec<*const i8> = required_instance_extensions
            .iter()
            .map(|cstr| cstr.as_ptr())
            .collect();
        log::info!("Creating Vulkan instance.");
        let app_name = c"Aura";
        let app_info = vk::ApplicationInfo::default()
            .application_name(app_name)
            .api_version(vk::make_api_version(0, 1, 4, 0));

        let instance_create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_layer_names(&layers_pointers)
            .enabled_extension_names(&extension_pointers);
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
            vk::KHR_SWAPCHAIN_MAINTENANCE1_NAME.as_ptr(),
        ];

        let mut sync2_features =
            vk::PhysicalDeviceSynchronization2Features::default().synchronization2(true);
        let mut video_maintenance =
            vk::PhysicalDeviceVideoMaintenance2FeaturesKHR::default().video_maintenance2(true);
        let mut dynamic_rendering =
            vk::PhysicalDeviceDynamicRenderingFeatures::default().dynamic_rendering(true);
        let mut sampler_ycbcr_conversion =
            vk::PhysicalDeviceSamplerYcbcrConversionFeaturesKHR::default()
                .sampler_ycbcr_conversion(true);
        let mut swapchain_maintenance1 =
            vk::PhysicalDeviceSwapchainMaintenance1FeaturesEXT::default()
                .swapchain_maintenance1(true);

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
            .push(&mut sampler_ycbcr_conversion)
            .push(&mut dynamic_rendering)
            .push(&mut swapchain_maintenance1);

        let device = unsafe {
            instance
                .create_device(physical_device, &device_create_info, None)
                .expect("Failed to create a logical device.")
        };

        // hardcoded extents and formats for DST and DPB
        let dpb_and_dst_format = vk::Format::G8_B8R8_2PLANE_420_UNORM;
        let video_extent = vk::Extent2D {
            width: 1920,
            height: 1080u32.div_ceil(16) * 16, // h264 macroblocks are multiple of 16, 1088 is needed.
        };

        // Ycbcr Sampler
        let ycbcr_conversion =
            unsafe { Self::create_ycbcr_conversion(&device, dpb_and_dst_format) };
        let mut h264_profile = vk::VideoDecodeH264ProfileInfoKHR::default()
            .std_profile_idc(vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN);
        let mut video_profile = vk::VideoProfileInfoKHR::default()
            .push(&mut h264_profile)
            .video_codec_operation(vk::VideoCodecOperationFlagsKHR::DECODE_H264)
            .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::TYPE_420)
            .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8);



        // Logs
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
            extradata,
            decode_queue_family_index,
        );
        let mut bitstream_buffers = [vk::Buffer::null(); FRAMES_IN_FLIGHT as usize];
        let mut bitstream_memories = [vk::DeviceMemory::null(); FRAMES_IN_FLIGHT as usize];
        let mut bitstream_sizes = [0 as u32; FRAMES_IN_FLIGHT as usize];
        for i in 0..FRAMES_IN_FLIGHT {
            let (bitstream_buffer, bitstream_memory, bitstream_size) =
                Aura::create_bitstream_buffer(
                    &instance,
                    &video_instance_ext,
                    physical_device,
                    &device,
                    &video_profile,
                );
            bitstream_buffers[i as usize] = bitstream_buffer;
            bitstream_memories[i as usize] = bitstream_memory;
            bitstream_sizes[i as usize] = bitstream_size
        }
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
            .command_buffer_count(FRAMES_IN_FLIGHT as u32);
        let graphics_command_buffers = unsafe {
            device
                .allocate_command_buffers(&graphics_cmd_alloc)
                .unwrap()
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
            DPB_POOL_SIZE,
            dpb_and_dst_format,
            video_extent
        );
        let semaphore_create_info = vk::SemaphoreCreateInfo::default();
        let frames_in_flight_fences_info =
            vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

        let mut present_complete_semaphores = Vec::new();
        let mut render_complete_semaphores = Vec::new();
        let mut graphics_complete_semaphores = Vec::new();

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
                graphics_complete_semaphores.push(
                    device
                        .create_semaphore(&semaphore_create_info, None)
                        .unwrap(),
                );
            }
        }

        let mut frames_in_flight_fences = [vk::Fence::null(); FRAMES_IN_FLIGHT as usize];

        for i in 0..FRAMES_IN_FLIGHT {
            unsafe {
                frames_in_flight_fences[i as usize] = device
                    .create_fence(&frames_in_flight_fences_info, None)
                    .unwrap();
            }
        }
        let vert_module = crate::create_shader!(device, "full_screen.vert.spv");
        let frag_module = crate::create_shader!(device, "show_texture.frag.spv");
        let shader_stages = Self::create_shader_stages(&device, vert_module, frag_module);

        let video_sampler = Self::create_sampler(&device, ycbcr_conversion);
        let mut descriptor_set_layouts = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT {
            descriptor_set_layouts.push(Self::create_video_descriptor_set_layout(
                &device,
                &video_sampler,
                1,
            ));
        }
        let descriptor_pool = Self::create_descriptor_pool(&device, FRAMES_IN_FLIGHT as u32);
        let descriptor_sets =
            Self::allocate_descriptor_sets(&device, &descriptor_set_layouts, descriptor_pool);
        let pipeline_layout = Self::create_pipeline_layout(&device, &descriptor_set_layouts);
        let pipeline = Self::create_pipeline(&device, pipeline_layout, &shader_stages);
        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        };
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: swapchain_extent,
        };
        log::debug!("Descriptor sets size: {}", descriptor_sets.len());
        Self {
            _entry: entry,
            _instance: instance,
            _video_queue_family_index: decode_queue_family_index,
            _graphics_queue_family_index: graphics_queue_family_index,
            _session_memories: _session_memories,
            _debug_utils_loader: _debug_utils_loader,
            _debug_messenger: _debug_messenger,

            device: device,
            surface: surface,
            surface_loader: surface_loader,
            video_instance_ext: video_instance_ext,
            physical_device: physical_device,

            graphics_command_pool: graphics_command_pool,
            video_command_pool: video_command_pool,
            session: session,
            bitstream_buffers: bitstream_buffers,
            bitstream_memories: bitstream_memories,
            bitstream_sizes: bitstream_sizes,
            video_loader: video_loader,
            decode_loader: decode_loader,
            ycbcr_conversion: ycbcr_conversion,
            graphics_command_buffers: graphics_command_buffers,
            video_command_buffers: video_command_buffers,
            graphics_queue: graphics_queue,
            video_queue: video_queue,
            session_parameters: session_parameters,

            swapchain_loader: swapchain_loader,
            swapchain: swapchain,
            swapchain_images: swapchain_images,
            swapchain_image_views: swapchain_image_views,
            swapchain_format: swapchain_format,
            swapchain_extent: swapchain_extent,

            present_complete_semaphores: present_complete_semaphores,
            render_complete_semaphores: render_complete_semaphores,
            graphics_complete_semaphores: graphics_complete_semaphores,
            render_fences: frames_in_flight_fences,

            pipeline_layout: pipeline_layout,
            pipeline: pipeline,
            video_sampler: video_sampler,
            descriptor_set_layouts: descriptor_set_layouts,
            
            dpb_pool: dpb_pool,
            dst_pool: dst_pool,
            dpb_and_dst_format: dpb_and_dst_format,
            current_frame_count_idx: 0,
            dpb_pool_size: DPB_POOL_SIZE,
            dpb_frame_nums: [0 as u16; DPB_POOL_SIZE as usize],
            dpb_slot_valid: vec![false; DPB_POOL_SIZE],
            dpb_pocs: vec![0; DPB_POOL_SIZE],
            descriptor_pool: descriptor_pool,
            descriptor_sets: descriptor_sets,

            viewport: viewport,
            scissor: scissor,
            video_extent: video_extent,
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
        dpb_format: vk::Format,
        video_extent: vk::Extent2D
    ) -> (
        Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>,
        Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>,
    ) {
        let _output_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)> =
            Vec::with_capacity(dpb_pool_size);
        let mut profile_list =
            vk::VideoProfileListInfoKHR::default().profiles(std::slice::from_ref(&video_profile));
        let dpb_dst_extent = vk::Extent3D {
            width: video_extent.width,
            height: video_extent.height,
            depth: 1,
        };
        let dpb_image_info = vk::ImageCreateInfo::default()
            .push(&mut profile_list)
            .image_type(vk::ImageType::TYPE_2D)
            .format(dpb_format)
            .extent(dpb_dst_extent.clone())
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
            .extent(dpb_dst_extent.clone())
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
        let mut local_ycbcr_info =
            vk::SamplerYcbcrConversionInfo::default().conversion(ycbcr_conversion);

        for i in 0..dpb_pool_size {
            let dpb_view_info = vk::ImageViewCreateInfo::default()
                .push(&mut local_ycbcr_info)
                .image(dpb_image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(dpb_format)
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
                .format(dpb_format)
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
        extradata: &Vec<u8>,
        queue_family_index: u32,
    ) -> DecodingSession {
        let video_loader = video_queue::Device::load(instance, device);
        let decode_loader = VideoDecodeLoader::load(instance, device);

        let session = Aura::create_video_session(instance, device, queue_family_index);

        let session_parameters = unsafe {
            Aura::create_h264_session_parameters(device, &video_loader, extradata, session)
        };

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
            for i in 0..FRAMES_IN_FLIGHT {
                self.device
                    .destroy_buffer(self.bitstream_buffers[i as usize], None);
                self.device
                    .free_memory(self.bitstream_memories[i as usize], None);
            }
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
            for &semaphore in &self.graphics_complete_semaphores {
                self.device.destroy_semaphore(semaphore, None);
            }
            log::debug!("Sync resources successfully destroyed.");

            self.device.destroy_pipeline(self.pipeline, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            for i in 0..self.descriptor_set_layouts.len() {
                self.device
                    .destroy_descriptor_set_layout(self.descriptor_set_layouts[i], None);
            }
            self.device.destroy_sampler(self.video_sampler, None);

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
            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
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
