//! # Decoder implementation.
//! Provides required functions for any decoder instance.


use std::ffi::CStr;

use crate::vulkan::photon::types::VideoCodecsProfiles::VideoProfile;
use crate::vulkan::photon::h264::H264Decoder;
use crate::vulkan::vk_init::Aura;
use super::types::PhotonError;
use ash::khr::video_queue;
use ash::vk::{TaggedStructure, make_api_version};
use ash::{Device, Instance, vk, khr::{video_decode_queue::Device as VideoDecodeLoader}};
use anyhow::Result;


pub struct DecodingSession {
    pub(crate) session: vk::VideoSessionKHR,
    pub(crate) _session_memories: Vec<vk::DeviceMemory>,
    pub(crate) video_loader: video_queue::Device,
    pub(crate) decode_loader: VideoDecodeLoader,
    pub(crate) session_parameters: vk::VideoSessionParametersKHR,
}

pub trait Decoder {
    fn create_video_session(
        instance: &Instance,
        device: &Device,
        video_queue_index: u32,
        codec_operation: vk::VideoCodecOperationFlagsKHR,
        profile_idc: Option<impl VideoProfile>,
        chroma_subsampling: vk::VideoChromaSubsamplingFlagsKHR,
        picture_format: vk::Format,
        luma_depth: vk::VideoComponentBitDepthFlagsKHR,
        chroma_depth: vk::VideoComponentBitDepthFlagsKHR,
        reference_picture_format: vk::Format
    ) -> Result<vk::VideoSessionKHR, PhotonError>;
    /// Checks wether or not a memmory type is supported and returns its index.
    fn find_memory_type_index(
        instance: &Instance,
        pdevice: vk::PhysicalDevice,
        type_filter: u32,
        props: vk::MemoryPropertyFlags,
    ) -> u32;
    fn allocate_video_session_memories(
        instance: &Instance,
        pd: vk::PhysicalDevice,
        device: &Device,
        loader: &video_queue::Device,
        session: vk::VideoSessionKHR,
    ) -> Vec<vk::DeviceMemory>;
    /// Creates a memory buffer with `VIDEO_DECODE_SRC_KHR` usage flag and `HOST_VISIBLE` | `HOST_COHERENT` property flags.
    fn create_bitstream_buffer(
        instance: &Instance,
        video_instance_ext: &video_queue::Instance,
        pd: vk::PhysicalDevice,
        device: &Device,
        prof: &vk::VideoProfileInfoKHR,
    ) -> (vk::Buffer, vk::DeviceMemory, u32);

    /// Uploads the current bitstream buffer (RAM) into a vulkan buffer (VRAM).
    fn upload_bitstream_packet(&self, data: &[u8], swapchain_sync_idx: usize);
    
    fn setup_decoder(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
        extradata: &[u8],
        queue_family_index: u32,
        codec_operation: vk::VideoCodecOperationFlagsKHR,
        profile_idc: Option<impl VideoProfile>,
        luma_depth: vk::VideoComponentBitDepthFlagsKHR,
        chroma_depth: vk::VideoComponentBitDepthFlagsKHR,
        chroma_subsampling: vk::VideoChromaSubsamplingFlagsKHR,
        picture_format: vk::Format,
        reference_picture_format: vk::Format
    ) -> DecodingSession;
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
    );
    unsafe fn acquire_image_dst_on_graphic(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        video_queue_family: u32,
        graphics_queue_family: u32,
    );
    unsafe fn release_dst_on_graphic(
        device: &ash::Device,
        cmd_buf_video: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        video_queue_family: u32,
        graphics_queue_family: u32,
    );
    unsafe fn release_graphic_on_dst(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        video_queue_family: u32,
        graphics_queue_family: u32,
    );
    unsafe fn acquire_swapchain_barrier(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        graphics_queue_family: u32,
    );

    unsafe fn release_swapchain_barrier(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        graphics_queue_family: u32,
    );

    fn copy_image(
        device: &ash::Device,
        command_buffer: vk::CommandBuffer,
        src_image: vk::Image,
        dst_texture: vk::Image,
    );
}

impl Decoder for Aura {
    fn create_video_session(
        instance: &Instance,
        device: &Device,
        video_queue_index: u32,
        codec_operation: vk::VideoCodecOperationFlagsKHR,
        profile_idc: Option<impl VideoProfile>,
        chroma_subsampling: vk::VideoChromaSubsamplingFlagsKHR,
        picture_format: vk::Format,
        luma_depth: vk::VideoComponentBitDepthFlagsKHR,
        chroma_depth: vk::VideoComponentBitDepthFlagsKHR,
        reference_picture_format: vk::Format
    ) -> Result<vk::VideoSessionKHR, PhotonError> {
        if let Some(profile_idc) = profile_idc {
            let extension_name: &CStr;
            
            let mut h264_profile = vk::VideoDecodeH264ProfileInfoKHR::default()
                .std_profile_idc(profile_idc.as_raw());
            let mut h265_profile = vk::VideoDecodeH265ProfileInfoKHR::default()
                .std_profile_idc(profile_idc.as_raw());
            let mut av1_profile = vk::VideoDecodeAV1ProfileInfoKHR::default()
                .std_profile(profile_idc.as_raw());
            let mut video_profile = vk::VideoProfileInfoKHR::default()
                .video_codec_operation(codec_operation)
                .chroma_subsampling(chroma_subsampling)
                .luma_bit_depth(luma_depth)
                .chroma_bit_depth(chroma_depth);
            if codec_operation == vk::VideoCodecOperationFlagsKHR::DECODE_H264 {
                video_profile = video_profile.push(&mut h264_profile);
                extension_name = c"VK_STD_vulkan_video_codec_h264_decode";
            } else if codec_operation == vk::VideoCodecOperationFlagsKHR::DECODE_H265 {
                video_profile = video_profile.push(&mut h265_profile);
                extension_name = c"VK_STD_vulkan_video_codec_h265_decode";
            } else if codec_operation == vk::VideoCodecOperationFlagsKHR::DECODE_AV1 {
                video_profile = video_profile.push(&mut av1_profile);
                extension_name = c"VK_STD_vulkan_video_codec_av1_decode";
            } else {
                return Err(PhotonError::InvalidCodecOperation)
            }
            let header_version = vk::ExtensionProperties::default()
                .spec_version(make_api_version(0, 1, 0, 0))
                .extension_name(extension_name).unwrap();
            let create_info = vk::VideoSessionCreateInfoKHR::default()
                .queue_family_index(video_queue_index)
                .video_profile(&video_profile)
                .picture_format(picture_format)
                .reference_picture_format(reference_picture_format)
                .max_coded_extent(vk::Extent2D {
                    width: 4096,
                    height: 4096,
                })
                .max_dpb_slots(16)
                .max_active_reference_pictures(16)
                .std_header_version(&header_version);
            
            let loader = video_queue::Device::load(instance, device);
            unsafe {
                let session = loader.create_video_session(&create_info, None)?;
                Ok(session)
            }
        } else {
            Err(PhotonError::NoProfileIndicator)
        }
    }
    fn find_memory_type_index(
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

    fn allocate_video_session_memories(
        instance: &Instance,
        pd: vk::PhysicalDevice,
        device: &Device,
        loader: &video_queue::Device,
        session: vk::VideoSessionKHR,
    ) -> Vec<vk::DeviceMemory> {
        unsafe {
            let mut memory_requirements = vec![
                vk::VideoSessionMemoryRequirementsKHR::default();
                loader
                    .get_video_session_memory_requirements_len(session)
                    .unwrap()
            ];
            let () = loader
                .get_video_session_memory_requirements(session, &mut memory_requirements)
                .unwrap();
            let mut memories = Vec::new();
            let mut binds = Vec::new();
            for memory_requirement in memory_requirements {
                let index = Self::find_memory_type_index(
                    instance,
                    pd,
                    memory_requirement.memory_requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                );
                let memory = device
                    .allocate_memory(
                        &vk::MemoryAllocateInfo::default()
                            .allocation_size(memory_requirement.memory_requirements.size)
                            .memory_type_index(index),
                        None,
                    )
                    .unwrap();
                memories.push(memory);
                binds.push(
                    vk::BindVideoSessionMemoryInfoKHR::default()
                        .memory_bind_index(memory_requirement.memory_bind_index)
                        .memory(memory)
                        .memory_size(memory_requirement.memory_requirements.size),
                );
            }
            let () = loader.bind_video_session_memory(session, &binds).unwrap();
            memories
        }
    }
    fn create_bitstream_buffer(
        instance: &Instance,
        video_instance_ext: &video_queue::Instance,
        pd: vk::PhysicalDevice,
        device: &Device,
        prof: &vk::VideoProfileInfoKHR,
    ) -> (vk::Buffer, vk::DeviceMemory, u32) {
        let mut profile_list =
            vk::VideoProfileListInfoKHR::default().profiles(std::slice::from_ref(prof));
        unsafe {
            let video_caps = vk::VideoCapabilitiesKHR::default();
            let mut decode_caps = vk::VideoDecodeCapabilitiesKHR::default();
            let mut h264decode_caps = vk::VideoDecodeH264CapabilitiesKHR::default();
            video_instance_ext
                .get_physical_device_video_capabilities(
                    pd,
                    prof,
                    &mut video_caps.push(&mut decode_caps).push(&mut h264decode_caps),
                )
                .unwrap();
            let alignment = video_caps.min_bitstream_buffer_offset_alignment;
            let alignment = if alignment == 0 { 256 } else { alignment };
            let size = (4 * 1024 * 1024 + alignment - 1) & !(alignment - 1);
            let buffer = device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .push(&mut profile_list)
                        .size(size)
                        .usage(vk::BufferUsageFlags::VIDEO_DECODE_SRC_KHR),
                    None,
                )
                .unwrap();
            let reqs = device.get_buffer_memory_requirements(buffer);
            let final_alloc_size = (reqs.size + reqs.alignment - 1) & !(reqs.alignment - 1);
            let index = Self::find_memory_type_index(
                instance,
                pd,
                reqs.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            );
            let memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(final_alloc_size)
                        .memory_type_index(index),
                    None,
                )
                .unwrap();
            device.bind_buffer_memory(buffer, memory, 0).unwrap();
            (buffer, memory, u32::try_from(final_alloc_size).unwrap())
        }
    }
    fn upload_bitstream_packet(&self, data: &[u8], swapchain_sync_idx: usize) {
        unsafe {
            let ptr = self
                .device
                .map_memory(
                    self.bitstream_memories[swapchain_sync_idx],
                    0,
                    u64::from(self.bitstream_sizes[swapchain_sync_idx]),
                    vk::MemoryMapFlags::empty(),
                )
                .unwrap();
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.cast::<u8>(), data.len());
            self.device
                .unmap_memory(self.bitstream_memories[swapchain_sync_idx]);
        }
    }
    
    fn setup_decoder(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
        extradata: &[u8],
        queue_family_index: u32,
        codec_operation: vk::VideoCodecOperationFlagsKHR,
        profile_idc: Option<impl VideoProfile>,
        luma_depth: vk::VideoComponentBitDepthFlagsKHR,
        chroma_depth: vk::VideoComponentBitDepthFlagsKHR,
        chroma_subsampling: vk::VideoChromaSubsamplingFlagsKHR,
        picture_format: vk::Format,
        reference_picture_format: vk::Format
    ) -> DecodingSession {
        let video_loader = video_queue::Device::load(instance, device);
        let decode_loader = VideoDecodeLoader::load(instance, device);

        let session = Aura::create_video_session(instance, device, queue_family_index, 
            codec_operation, 
            profile_idc, 
            chroma_subsampling, 
            picture_format, 
            luma_depth, 
            chroma_depth, 
            reference_picture_format).unwrap();

        let session_parameters = unsafe {
            Aura::create_h264_session_parameters(device, &video_loader, extradata, session)
        };

        let session_memories = Aura::allocate_video_session_memories(
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
            .extent(dpb_dst_extent)
            .mip_levels(1)
            .array_layers(u32::try_from(dpb_pool_size).unwrap())
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let dpb_image = unsafe { device.create_image(&dpb_image_info, None).unwrap() };

        let dst_image_info = vk::ImageCreateInfo::default()
            .push(&mut profile_list)
            .image_type(vk::ImageType::TYPE_2D)
            .format(dpb_format)
            .extent(dpb_dst_extent)
            .mip_levels(1)
            .array_layers(u32::try_from(dpb_pool_size).unwrap())
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
                    base_array_layer: u32::try_from(i).unwrap(),
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
                    base_array_layer: u32::try_from(i).unwrap(),
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

    unsafe fn acquire_image_dst_on_graphic(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        video_queue_family: u32,
        graphics_queue_family: u32,
    ) {
        let image_barrier = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::NONE)
            .src_access_mask(vk::AccessFlags2::NONE)
            .src_queue_family_index(video_queue_family)
            .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
            .dst_access_mask(vk::AccessFlags2::SHADER_READ)
            .dst_queue_family_index(graphics_queue_family)
            .subresource_range(subresource_range)
            .image(dst_image)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let barriers = [image_barrier];
        let dependency = vk::DependencyInfo::default().image_memory_barriers(&barriers);
        unsafe { device.cmd_pipeline_barrier2(cmd_buf_graphics, &dependency) };
    }
    unsafe fn release_dst_on_graphic(
        device: &ash::Device,
        cmd_buf_video: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        video_queue_family: u32,
        graphics_queue_family: u32,
    ) {
        let image_barrier = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
            .src_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
            .src_queue_family_index(video_queue_family)
            .dst_stage_mask(vk::PipelineStageFlags2::NONE)
            .dst_access_mask(vk::AccessFlags2::NONE)
            .dst_queue_family_index(graphics_queue_family)
            .subresource_range(subresource_range)
            .image(dst_image)
            .old_layout(vk::ImageLayout::VIDEO_DECODE_DST_KHR)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let barriers = [image_barrier];
        let dependency = vk::DependencyInfo::default().image_memory_barriers(&barriers);
        unsafe { device.cmd_pipeline_barrier2(cmd_buf_video, &dependency) };
    }
    unsafe fn release_graphic_on_dst(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        video_queue_family: u32,
        graphics_queue_family: u32,
    ) {
        let image_barrier = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::NONE)
            .src_access_mask(vk::AccessFlags2::NONE)
            .src_queue_family_index(graphics_queue_family)
            .dst_stage_mask(vk::PipelineStageFlags2::NONE)
            .dst_access_mask(vk::AccessFlags2::NONE)
            .dst_queue_family_index(video_queue_family)
            .subresource_range(subresource_range)
            .image(dst_image)
            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::VIDEO_DECODE_DST_KHR);
        let barriers = [image_barrier];
        let dependency = vk::DependencyInfo::default().image_memory_barriers(&barriers);
        unsafe { device.cmd_pipeline_barrier2(cmd_buf_graphics, &dependency) };
    }

    unsafe fn acquire_swapchain_barrier(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        graphics_queue_family: u32,
    ) {
        let image_barrier = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::NONE)
            .src_access_mask(vk::AccessFlags2::NONE)
            .src_queue_family_index(graphics_queue_family)
            .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .dst_queue_family_index(graphics_queue_family)
            .subresource_range(subresource_range)
            .image(dst_image)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        let barriers = [image_barrier];
        let dependency = vk::DependencyInfo::default().image_memory_barriers(&barriers);
        unsafe { device.cmd_pipeline_barrier2(cmd_buf_graphics, &dependency) };
    }

    unsafe fn release_swapchain_barrier(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dst_image: vk::Image,
        subresource_range: vk::ImageSubresourceRange,
        graphics_queue_family: u32,
    ) {
        let image_barrier = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .src_queue_family_index(graphics_queue_family)
            .dst_stage_mask(vk::PipelineStageFlags2::NONE)
            .dst_access_mask(vk::AccessFlags2::NONE)
            .dst_queue_family_index(graphics_queue_family)
            .subresource_range(subresource_range)
            .image(dst_image)
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::PRESENT_SRC_KHR);
        let barriers = [image_barrier];
        let dependency = vk::DependencyInfo::default().image_memory_barriers(&barriers);
        unsafe { device.cmd_pipeline_barrier2(cmd_buf_graphics, &dependency) };
    }

    /*
    *  Well... There are two ways to show decoded frames in Vulkan:
    *       - no-copy (DPB -> ImageLayout conversion [VIDEO_DECODE_DPB_KHR -> SHADER_READ_ONLY_OPTIMAL] ->
    *            SamplerYcbcr -> Fragment Shader -> ImageLayout Reversion -> back to DPB);
    *       - copy (DPB -> 2 independent image copies [Image A - Y, Image B - U & V] considering the YUV format -> Fragment Shader).
    * 
    *  The first one is obviously what we want to do because the performance is better and it will decrease the needed VRAM size,
    *  but it doesn't allow us to use vkCmdBlitImage. Its direct consequence is that on some GPUs that don't support
    *  VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT, we can't maintain the original frame quality if the user
    *  resizes the window (color artifacting).
    * 
    *  For now, we won't use the copy method on purpose. In the future, this player will verify the window extent and use
    *  the copy or no-copy approach depending on whether it matches.
    *
    */
    
     
    fn copy_image(
        device: &ash::Device,
        command_buffer: vk::CommandBuffer,
        src_image: vk::Image,
        dst_texture: vk::Image,
    ) {
        let copy_region = vk::ImageCopy::default()
            .src_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .dst_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .extent(vk::Extent3D {
                width: 1920,
                height: 1080,
                depth: 1,
            });

        unsafe {
            device.cmd_copy_image(
                command_buffer,
                src_image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                dst_texture,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                std::slice::from_ref(&copy_region),
            );
        }
    }
}
