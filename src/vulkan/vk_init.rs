use super::debug;
use crate::video::video_context::VideoContext;
use crate::vulkan::photon;
use crate::vulkan::photon::decoder::{Decoder, DecodingSession};
use crate::vulkan::photon::sampler::Sampler;
use crate::vulkan::photon::types::VideoCodecsProfiles::VideoProfile;
use crate::vulkan::photon::types::{DecodeExtensions, SupportedCodecs, VideoCodecsProfiles};
use crate::vulkan::pipeline::Pipeline;
use crate::vulkan::shaders::Shaders;
use ash::{
    Entry, Instance,
    khr::{video_decode_queue::Device as VideoDecodeLoader, video_queue},
    vk,
    vk::{DebugUtilsMessengerEXT, TaggedStructure},
};
use ffmpeg_next::ffi::AVCodecID;
use ffmpeg_next::ffi::{AVColorRange, AVPixelFormat};
use raw_window_handle::{self, HasDisplayHandle, HasWindowHandle};
use std::ffi::CStr;

pub const SWAPHAIN_IMAGE_COUNT: u8 = 4;
pub const FRAMES_IN_FLIGHT: u8 = SWAPHAIN_IMAGE_COUNT - 1;
const DPB_POOL_SIZE: usize = 16;

pub struct Aura {
    pub _entry: Entry,
    pub _instance: Instance,
    pub _debug_utils_loader: ash::ext::debug_utils::Instance,
    pub _debug_messenger: DebugUtilsMessengerEXT,
    pub _video_queue_family_index: u32,
    pub _graphics_queue_family_index: u32,

    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,

    pub graphics_queue: vk::Queue,
    pub video_queue: vk::Queue,
    pub surface: vk::SurfaceKHR,
    pub surface_loader: ash::khr::surface::Instance,

    pub dpb_pocs: Vec<i32>,
    pub dpb_pool_size: usize,
    pub dpb_slot_valid: Vec<bool>,
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

    pub present_complete_semaphores: [vk::Semaphore; SWAPHAIN_IMAGE_COUNT as usize],
    pub render_complete_semaphores: [vk::Semaphore; SWAPHAIN_IMAGE_COUNT as usize],
    pub graphics_complete_semaphores: [vk::Semaphore; SWAPHAIN_IMAGE_COUNT as usize],
    pub render_fences: [vk::Fence; FRAMES_IN_FLIGHT as usize],

    pub pipeline_layout: vk::PipelineLayout,
    pub pipeline: vk::Pipeline,
    pub descriptor_set_layouts: Vec<vk::DescriptorSetLayout>,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,

    pub viewport: vk::Viewport,
    pub scissor: vk::Rect2D,
    pub video_extent: vk::Extent2D,
    pub frames_in_flight: u8,
    pub supported_decoders: SupportedCodecs,
    pub photon: super::photon::lib::DecodingInstance,
}

impl Aura {
    // Constants

    pub fn new(
        window: &winit::window::Window,
        extradata: &[u8],
        v_ctx: Option<&VideoContext>,
    ) -> Self {
        let entry = unsafe { Entry::load().expect("Failed to load vulkan driver.") };
        match unsafe { entry.try_enumerate_instance_version().unwrap() } {
            Some(version) => {
                let major = vk::api_version_major(version);
                let minor = vk::api_version_minor(version);
                let patch = vk::api_version_patch(version);
                log::info!("Vulkan {major}.{minor}.{patch}");
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
            layer_names.iter().copied().collect();
        let mut required_instance_extensions: Vec<&CStr> =
            ash_window::enumerate_required_extensions(window.display_handle().unwrap().as_raw())
                .expect("Failed to retrieve window extensions.")
                .iter()
                .map(|&ptr| unsafe { CStr::from_ptr(ptr) })
                .collect();
        required_instance_extensions.push(vk::EXT_DEBUG_UTILS_NAME);
        required_instance_extensions.push(vk::KHR_SURFACE_MAINTENANCE1_NAME);
        required_instance_extensions.push(vk::KHR_GET_SURFACE_CAPABILITIES2_NAME);
        Self::log_instance_extensions(&entry, &required_instance_extensions);
        let extension_pointers: Vec<*const std::os::raw::c_char> = required_instance_extensions
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

        let dpb_and_dst_format: vk::Format = vk::Format::G8_B8R8_2PLANE_420_UNORM;
        let video_color_range: vk::SamplerYcbcrRange = if let Some(v_ctx) = v_ctx {
            unsafe {
                let raw_params = v_ctx.params.as_ptr();
                match (*raw_params).color_range {
                    AVColorRange::AVCOL_RANGE_MPEG => vk::SamplerYcbcrRange::ITU_NARROW,
                    AVColorRange::AVCOL_RANGE_JPEG => vk::SamplerYcbcrRange::ITU_FULL,
                    _ => vk::SamplerYcbcrRange::ITU_NARROW_KHR, // _KHR to avoid clippy errors, this won't change anything.
                }
            }
        } else {
            vk::SamplerYcbcrRange::ITU_NARROW_KHR
        };
        let video_codec: vk::VideoCodecOperationFlagsKHR = if let Some(v_ctx) = v_ctx {
            unsafe {
                let raw_params = v_ctx.params.as_ptr();
                match (*raw_params).codec_id {
                    AVCodecID::AV_CODEC_ID_H264 => vk::VideoCodecOperationFlagsKHR::DECODE_H264,
                    AVCodecID::AV_CODEC_ID_HEVC => vk::VideoCodecOperationFlagsKHR::DECODE_H265,
                    AVCodecID::AV_CODEC_ID_AV1 => vk::VideoCodecOperationFlagsKHR::DECODE_AV1,
                    _ => vk::VideoCodecOperationFlagsKHR::NONE,
                }
            }
        } else {
            vk::VideoCodecOperationFlagsKHR::NONE
        };

        let (video_chroma_flags, video_luma_depth, video_chroma_depth) = if let Some(v_ctx) = v_ctx
        {
            unsafe {
                let raw_params = v_ctx.params.as_ptr();
                log::debug!("Pixel format indicator: {}", (*raw_params).format);
                match (*raw_params).format {
                    format
                        if format == AVPixelFormat::AV_PIX_FMT_YUV420P as i32
                            || format == AVPixelFormat::AV_PIX_FMT_YUVJ420P as i32
                            || format == AVPixelFormat::AV_PIX_FMT_NV12 as i32 =>
                    {
                        (
                            vk::VideoChromaSubsamplingFlagsKHR::TYPE_420,
                            vk::VideoComponentBitDepthFlagsKHR::TYPE_8,
                            vk::VideoComponentBitDepthFlagsKHR::TYPE_8,
                        )
                    }
                    format if format == AVPixelFormat::AV_PIX_FMT_YUV422P as i32 => (
                        vk::VideoChromaSubsamplingFlagsKHR::TYPE_422,
                        vk::VideoComponentBitDepthFlagsKHR::TYPE_8,
                        vk::VideoComponentBitDepthFlagsKHR::TYPE_8,
                    ),
                    format if format == AVPixelFormat::AV_PIX_FMT_YUV444P as i32 => (
                        vk::VideoChromaSubsamplingFlagsKHR::TYPE_444,
                        vk::VideoComponentBitDepthFlagsKHR::TYPE_8,
                        vk::VideoComponentBitDepthFlagsKHR::TYPE_8,
                    ),
                    _ => (
                        vk::VideoChromaSubsamplingFlagsKHR::INVALID,
                        vk::VideoComponentBitDepthFlagsKHR::INVALID,
                        vk::VideoComponentBitDepthFlagsKHR::INVALID,
                    ),
                }
            }
        } else {
            (
                vk::VideoChromaSubsamplingFlagsKHR::INVALID,
                vk::VideoComponentBitDepthFlagsKHR::INVALID,
                vk::VideoComponentBitDepthFlagsKHR::INVALID,
            )
        };

        let video_profile_indicator = if let Some(v_ctx) = v_ctx {
            unsafe {
                let raw_params = v_ctx.params.as_ptr();
                match (*raw_params).profile {
                    // H264 profiles
                    profile if profile == VideoCodecsProfiles::H264Profiles::Baseline as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::H264(
                            VideoCodecsProfiles::H264Profiles::Baseline,
                        )
                    }
                    profile if profile == VideoCodecsProfiles::H264Profiles::Main as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::H264(
                            VideoCodecsProfiles::H264Profiles::Main,
                        )
                    }
                    profile if profile == VideoCodecsProfiles::H264Profiles::High as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::H264(
                            VideoCodecsProfiles::H264Profiles::High,
                        )
                    }
                    profile if profile == VideoCodecsProfiles::H264Profiles::High444 as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::H264(
                            VideoCodecsProfiles::H264Profiles::High444,
                        )
                    }

                    // H265 profiles
                    profile if profile == VideoCodecsProfiles::H265Profiles::Main as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::H265(
                            VideoCodecsProfiles::H265Profiles::Main,
                        )
                    }
                    profile if profile == VideoCodecsProfiles::H265Profiles::Main10 as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::H265(
                            VideoCodecsProfiles::H265Profiles::Main10,
                        )
                    }

                    // AV1 profiles
                    profile if profile == VideoCodecsProfiles::AV1Profiles::Main as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::AV1(
                            VideoCodecsProfiles::AV1Profiles::Main,
                        )
                    }
                    profile if profile == VideoCodecsProfiles::AV1Profiles::High as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::AV1(
                            VideoCodecsProfiles::AV1Profiles::High,
                        )
                    }
                    profile if profile == VideoCodecsProfiles::AV1Profiles::Professional as i32 => {
                        VideoCodecsProfiles::UnifiedVideoProfile::AV1(
                            VideoCodecsProfiles::AV1Profiles::Professional,
                        )
                    }
                    _ => VideoCodecsProfiles::UnifiedVideoProfile::Unknown(
                        VideoCodecsProfiles::UnknownProfile::Unknown,
                    ),
                }
            }
        } else {
            VideoCodecsProfiles::UnifiedVideoProfile::Unknown(
                VideoCodecsProfiles::UnknownProfile::Unknown,
            )
        };

        let video_height: i32 = if let Some(v_ctx) = v_ctx {
            unsafe {
                let raw_params = v_ctx.params.as_ptr();
                (*raw_params).height
            }
        } else {
            1920
        };

        let video_width: i32 = if let Some(v_ctx) = v_ctx {
            unsafe {
                let raw_params = v_ctx.params.as_ptr();
                (*raw_params).width
            }
        } else {
            1080
        };
        let video_extent = vk::Extent2D {
            // keeps multiple of 16 for now just for h264 compatibility
            width: video_width.cast_unsigned().div_ceil(16) * 16,
            height: video_height.cast_unsigned().div_ceil(16) * 16,
        };

        // Ycbcr Sampler
        let ycbcr_conversion = unsafe {
            Self::create_ycbcr_conversion(&device, dpb_and_dst_format, video_color_range)
        };
        let mut video_profile_info = vk::VideoDecodeH264ProfileInfoKHR::default()
            .std_profile_idc(video_profile_indicator.as_raw());
        let mut video_profile = vk::VideoProfileInfoKHR::default()
            .push(&mut video_profile_info)
            .video_codec_operation(video_codec)
            .chroma_subsampling(video_chroma_flags)
            .luma_bit_depth(video_luma_depth)
            .chroma_bit_depth(video_chroma_depth);

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
            .command_buffer_count(u32::from(FRAMES_IN_FLIGHT));
        let graphics_command_buffers = unsafe {
            device
                .allocate_command_buffers(&graphics_cmd_alloc)
                .unwrap()
        };

        let video_alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(video_command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(u32::from(FRAMES_IN_FLIGHT));
        let video_command_buffers =
            unsafe { device.allocate_command_buffers(&video_alloc_info).unwrap() };

        let semaphore_create_info = vk::SemaphoreCreateInfo::default();
        let frames_in_flight_fences_info =
            vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

        let mut present_complete_semaphores =
            [vk::Semaphore::null(); SWAPHAIN_IMAGE_COUNT as usize];
        let mut render_complete_semaphores = [vk::Semaphore::null(); SWAPHAIN_IMAGE_COUNT as usize];
        let mut graphics_complete_semaphores =
            [vk::Semaphore::null(); SWAPHAIN_IMAGE_COUNT as usize];

        for i in 0..swapchain_images.len() {
            unsafe {
                present_complete_semaphores[i] = device
                    .create_semaphore(&semaphore_create_info, None)
                    .unwrap();
                render_complete_semaphores[i] = device
                    .create_semaphore(&semaphore_create_info, None)
                    .unwrap();
                graphics_complete_semaphores[i] = device
                    .create_semaphore(&semaphore_create_info, None)
                    .unwrap();
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
        let vert_module = crate::create_shader!(&device, "full_screen.vert.spv");
        let frag_module = crate::create_shader!(&device, "show_texture.frag.spv");
        let shader_stages = Self::create_shader_stages(&device, vert_module, frag_module);

        let video_sampler = Self::create_sampler(&device, ycbcr_conversion);
        let mut descriptor_set_layouts = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT {
            descriptor_set_layouts.push(Self::create_video_descriptor_set_layout(
                &device,
                video_sampler,
                1,
            ));
        }
        let descriptor_pool = Self::create_descriptor_pool(&device, u32::from(FRAMES_IN_FLIGHT));
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
        let photon = photon::lib::DecodingInstance::new(
            decode_queue_family_index,
            graphics_queue_family_index,
            &instance,
            physical_device,
            device.clone(),
            video_instance_ext.clone(),
            Some(swapchain_loader.clone()),
            graphics_queue,
            video_queue,
            pipeline,
            pipeline_layout,
            &present_complete_semaphores,
            viewport,
            scissor,
            usize::from(FRAMES_IN_FLIGHT),
            swapchain_extent,
            vk::Offset2D { x: 0, y: 0 },
            video_extent,
            0,
            swapchain_image_views.clone(),
            descriptor_sets.clone(),
            video_command_buffers.clone(),
            graphics_command_buffers.clone(),
            swapchain_images.clone(),
            DPB_POOL_SIZE,
            &graphics_complete_semaphores,
            &render_complete_semaphores,
            &frames_in_flight_fences,
            Some(swapchain),
            dpb_and_dst_format,
            dpb_and_dst_format,
            video_codec,
            video_profile_indicator,
            &mut video_profile,
            ycbcr_conversion,
            video_sampler,
            extradata,
        );
        Self {
            _entry: entry,
            _instance: instance,
            _video_queue_family_index: decode_queue_family_index,
            _graphics_queue_family_index: graphics_queue_family_index,
            _debug_utils_loader: _debug_utils_loader,
            _debug_messenger: _debug_messenger,

            device: device,
            surface: surface,
            surface_loader: surface_loader,
            physical_device: physical_device,

            video_command_pool: video_command_pool,
            video_command_buffers: video_command_buffers,
            graphics_queue: graphics_queue,
            video_queue: video_queue,

            graphics_command_pool: graphics_command_pool,
            graphics_command_buffers: graphics_command_buffers,
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
            descriptor_set_layouts: descriptor_set_layouts,
            current_frame_count_idx: 0,
            dpb_pool_size: DPB_POOL_SIZE,
            dpb_slot_valid: vec![false; DPB_POOL_SIZE],
            dpb_pocs: vec![0; DPB_POOL_SIZE],
            descriptor_pool: descriptor_pool,
            descriptor_sets: descriptor_sets,

            viewport: viewport,
            scissor: scissor,
            video_extent: video_extent,
            frames_in_flight: FRAMES_IN_FLIGHT,
            supported_decoders: supported_decoders,
            photon: photon.unwrap(),
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
                let idx = u32::try_from(index).unwrap();

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
                                supported_codecs.h264 = true;
                            } else if ext_cstr == DecodeExtensions::H265 {
                                supported_codecs.h265 = true;
                            } else if ext_cstr == DecodeExtensions::AV1 {
                                supported_codecs.av1 = true;
                            }
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
                    "GPU successfully detected! Graphic queue: {g_idx}, Video Queue: {v_idx}",
                );
                return (pdevice, (g_idx, v_idx), supported_codecs);
            }
        }

        log::error!(
            "Your GPU needs to support both video decode queue and graphics queue with surface support."
        );
        std::process::abort();
    }
}

impl Drop for Aura {
    fn drop(&mut self) {
        unsafe {
            log::info!("Cleaning Vulkan instance...");

            if self.device.handle() != vk::Device::null() {
                let () = self.device.queue_wait_idle(self.graphics_queue).unwrap();
                let () = self.device.queue_wait_idle(self.video_queue).unwrap();
                let () = self.device.device_wait_idle().unwrap();
            }

            for &view in &self.swapchain_image_views {
                self.device.destroy_image_view(view, None);
            }
            log::debug!("Swapchain's Image Views were successfully destroyed.");

            for i in 0..self.frames_in_flight {
                self.device
                    .destroy_buffer(self.photon.bitstream_buffers[i as usize], None);
                self.device
                    .free_memory(self.photon.bitstream_memories[i as usize], None);
            }

            if self.photon.video_session.session_parameters != vk::VideoSessionParametersKHR::null()
            {
                self.photon
                    .video_session
                    .video_loader
                    .destroy_video_session_parameters(
                        self.photon.video_session.session_parameters,
                        None,
                    );
                self.photon.video_session.session_parameters =
                    vk::VideoSessionParametersKHR::null();
            }
            if self.photon.video_session.session != vk::VideoSessionKHR::null() {
                self.photon
                    .video_session
                    .video_loader
                    .destroy_video_session(self.photon.video_session.session, None);
                self.photon.video_session.session = vk::VideoSessionKHR::null();
            }
            for mem in &self.photon.video_session._session_memories {
                self.device.free_memory(*mem, None);
            }

            self.device.destroy_sampler(self.photon.video_sampler, None);
            for (_, _, view) in &self.photon.dpb_pool {
                self.device.destroy_image_view(*view, None);
            }
            for (_, _, view) in &self.photon.dst_pool {
                self.device.destroy_image_view(*view, None);
            }

            if let Some((image, memory, _)) = self.photon.dpb_pool.first() {
                self.device.destroy_image(*image, None);
                self.device.free_memory(*memory, None);
            }
            if let Some((image, memory, _)) = self.photon.dst_pool.first() {
                self.device.destroy_image(*image, None);
                self.device.free_memory(*memory, None);
            }
            log::debug!("DPB/DST pools were freed.");

            self.device
                .destroy_sampler_ycbcr_conversion(self.photon.ycbcr_conversion, None);

            self.swapchain_loader
                .destroy_swapchain(self.swapchain, None);
            log::debug!("SwapchainKHR was succcessfully destroyed.");

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

            self.device
                .destroy_command_pool(self.graphics_command_pool, None);
            self.device
                .destroy_command_pool(self.video_command_pool, None);
            log::debug!("Successfully destroyed all command pools.");

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
