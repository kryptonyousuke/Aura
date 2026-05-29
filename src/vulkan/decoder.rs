use crate::vulkan::vk_init::Aura;
use ash::khr::video_queue;
use ash::vk::TaggedStructure;
use ash::{Device, Instance, vk, khr::{video_decode_queue::Device as VideoDecodeLoader}};
pub struct DecodeExtensions;
use std::ffi::CStr;

impl DecodeExtensions {
    pub const H264: &'static CStr = c"VK_KHR_video_decode_h264";
    pub const H265: &'static CStr = c"VK_KHR_video_decode_h265";
    pub const AV1: &'static CStr = c"VK_KHR_video_decode_av1";
}
pub struct SupportedCodecs {
    pub h264: bool,
    pub h265: bool,
    pub av1: bool,
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
pub struct DecodingSession {
    pub(super) session: vk::VideoSessionKHR,
    pub(super) _session_memories: Vec<vk::DeviceMemory>,
    pub(super) video_loader: video_queue::Device,
    pub(super) decode_loader: VideoDecodeLoader,
    pub(super) session_parameters: vk::VideoSessionParametersKHR,
}

pub trait Decoder {
    fn create_video_session(
        instance: &Instance,
        device: &Device,
        video_queue_index: u32,
    ) -> vk::VideoSessionKHR;
    fn bind_video_session_memory(
        instance: &Instance,
        pd: vk::PhysicalDevice,
        device: &Device,
        loader: &video_queue::Device,
        session: vk::VideoSessionKHR,
    ) -> Vec<vk::DeviceMemory>;
    fn create_bitstream_buffer(
        instance: &Instance,
        video_instance_ext: &video_queue::Instance,
        pd: vk::PhysicalDevice,
        device: &Device,
        prof: &vk::VideoProfileInfoKHR,
    ) -> (vk::Buffer, vk::DeviceMemory, u32);
    fn upload_bitstream_packet(&self, data: &[u8], swapchain_sync_idx: usize);
    unsafe fn create_ycbcr_conversion(
        device: &ash::Device,
        format: vk::Format,
    ) -> vk::SamplerYcbcrConversion;
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
    ) -> vk::VideoSessionKHR {
        // Will be a general use function.
        let mut h264_profile = vk::VideoDecodeH264ProfileInfoKHR::default()
            .std_profile_idc(vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN);
        let video_profile = vk::VideoProfileInfoKHR::default()
            .video_codec_operation(vk::VideoCodecOperationFlagsKHR::DECODE_H264)
            .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::TYPE_420)
            .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
            .push(&mut h264_profile);
        let mut header_version = vk::ExtensionProperties::default();
        let name = c"VK_STD_vulkan_video_codec_h264_decode";
        for (dest, &src) in header_version
            .extension_name
            .iter_mut()
            .zip(name.to_bytes_with_nul())
        {
            *dest = src as i8;
        }
        header_version.spec_version = vk::make_api_version(0, 1, 0, 0);
        let create_info = vk::VideoSessionCreateInfoKHR::default()
            .queue_family_index(video_queue_index)
            .video_profile(&video_profile)
            .picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .reference_picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .max_coded_extent(vk::Extent2D {
                width: 4096,
                height: 4096,
            })
            .max_dpb_slots(16)
            .max_active_reference_pictures(16)
            .std_header_version(&header_version);

        let loader = video_queue::Device::load(instance, device);
        unsafe {
            let session = loader.create_video_session(&create_info, None).unwrap();
            session
        }
    }

    fn bind_video_session_memory(
        instance: &Instance,
        pd: vk::PhysicalDevice,
        device: &Device,
        loader: &video_queue::Device,
        session: vk::VideoSessionKHR,
    ) -> Vec<vk::DeviceMemory> {
        unsafe {
            let mut reqs = vec![
                vk::VideoSessionMemoryRequirementsKHR::default();
                loader
                    .get_video_session_memory_requirements_len(session)
                    .unwrap()
            ];
            let _ = loader
                .get_video_session_memory_requirements(session, &mut reqs)
                .unwrap();
            let mut memories = Vec::new();
            let mut binds = Vec::new();
            for req in reqs {
                let index = Self::find_memory_type(
                    instance,
                    pd,
                    req.memory_requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                );
                let memory = device
                    .allocate_memory(
                        &vk::MemoryAllocateInfo::default()
                            .allocation_size(req.memory_requirements.size)
                            .memory_type_index(index),
                        None,
                    )
                    .unwrap();
                memories.push(memory);
                binds.push(
                    vk::BindVideoSessionMemoryInfoKHR::default()
                        .memory_bind_index(req.memory_bind_index)
                        .memory(memory)
                        .memory_size(req.memory_requirements.size),
                );
            }
            let _ = loader.bind_video_session_memory(session, &binds).unwrap();
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
            let index = Self::find_memory_type(
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
            (buffer, memory, final_alloc_size as u32)
        }
    }

    fn upload_bitstream_packet(&self, data: &[u8], swapchain_sync_idx: usize) {
        unsafe {
            let ptr = self
                .device
                .map_memory(
                    self.bitstream_memories[swapchain_sync_idx],
                    0,
                    self.bitstream_sizes[swapchain_sync_idx] as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .unwrap();
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
            self.device
                .unmap_memory(self.bitstream_memories[swapchain_sync_idx]);
        }
    }

    unsafe fn create_ycbcr_conversion(
        device: &ash::Device,
        format: vk::Format,
    ) -> vk::SamplerYcbcrConversion {
        unsafe {
            let ycbcr_info = vk::SamplerYcbcrConversionCreateInfo::default()
                .format(format)
                .ycbcr_model(vk::SamplerYcbcrModelConversion::YCBCR_709)
                .ycbcr_range(vk::SamplerYcbcrRange::ITU_NARROW)
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
     * Well... There are two ways to show decoded frames in Vulkan:
     *      - no-copy (DPB -> ImageLayout conversion [VIDEO_DECODE_DPB_KHR -> SHADER_READ_ONLY_OPTIMAL] ->
     *           SamplerYcbcr -> Fragment Shader -> ImageLayout Reversion -> back to DPB);
     *      - copy (DPB -> 2 independent image copies [Image A - Y, Image B - U & V] considering the YUV format -> Fragment Shader).
     *
     * The first one is obviously what we want to do because the performance is better and it will decrease the needed VRAM size,
     * but it doesn't allow us to use vkCmdBlitImage. Its direct consequence is that on some GPUs that don't support
     * VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT, we can't maintain the original frame quality if the user
     * resizes the window (color artifacting).
     *
     * For now, we won't use the copy method on purpose. In the future, this player will verify the window extent and use
     * the copy or no-copy approach depending on whether it matches.
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
