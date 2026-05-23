use crate::vulkan::vk_init::Aura;
use ash::khr::video_queue;
use ash::vk::TaggedStructure;
use ash::{Device, Instance, vk};

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
    fn upload_bitstream_packet(&self, data: &[u8]);
    unsafe fn create_ycbcr_conversion(
        device: &ash::Device,
        format: vk::Format,
    ) -> vk::SamplerYcbcrConversion;
    unsafe fn create_video_sampler(
        device: &ash::Device,
        conversion: vk::SamplerYcbcrConversion,
    ) -> vk::Sampler;
    unsafe fn transition_dpb_to_graphic(
        device: &ash::Device,
        command_buffer: vk::CommandBuffer,
        image: vk::Image,
        current_layer: u32,
        video_queue_index: u32,
        graphics_queue_index: u32,
    );
    unsafe fn create_video_descriptor_set_layout(
        device: &Device,
        video_sampler: vk::Sampler,
    ) -> vk::DescriptorSetLayout;
    unsafe fn update_video_descriptor_set(
        device: &Device,
        descriptor_set: vk::DescriptorSet,
        current_dpb_image_view: vk::ImageView,
    );
    unsafe fn transition_graphic_to_dpb(
        device: &ash::Device,
        command_buffer: vk::CommandBuffer,
        image: vk::Image,
        current_layer: u32,
        video_queue_index: u32,
        graphics_queue_index: u32,
    );
    unsafe fn acquire_dpb_on_graphic(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dpb_image: vk::Image,
        decode_layout: vk::ImageLayout,
        video_queue_family: u32,
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
        unsafe {
            let name = b"VK_STD_vulkan_video_codec_h264_decode\0";
            header_version.extension_name[..name.len()]
                .copy_from_slice(std::mem::transmute::<&[u8], &[i8]>(name));
            header_version.spec_version = vk::make_api_version(0, 1, 0, 0);
        }
        let create_info = vk::VideoSessionCreateInfoKHR::default()
            .queue_family_index(video_queue_index)
            .video_profile(&video_profile)
            .picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .reference_picture_format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .max_coded_extent(vk::Extent2D {
                width: 1920,
                height: 1080,
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
            let index = Self::find_memory_type(
                instance,
                pd,
                reqs.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            );
            let memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(reqs.size)
                        .memory_type_index(index),
                    None,
                )
                .unwrap();
            device.bind_buffer_memory(buffer, memory, 0).unwrap();
            (buffer, memory, size as u32)
        }
    }

    fn upload_bitstream_packet(&self, data: &[u8]) {
        unsafe {
            let ptr = self
                .device
                .map_memory(
                    self.bitstream_memory,
                    0,
                    data.len() as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .unwrap();
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
            self.device.unmap_memory(self.bitstream_memory);
        }
    }
    unsafe fn create_video_descriptor_set_layout(
        device: &Device,
        video_sampler: vk::Sampler,
    ) -> vk::DescriptorSetLayout {
        unsafe {
            let immutable_samplers = [video_sampler];

            let layout_binding = vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .immutable_samplers(&immutable_samplers);

            let layout_info = vk::DescriptorSetLayoutCreateInfo::default()
                .bindings(std::slice::from_ref(&layout_binding));

            device
                .create_descriptor_set_layout(&layout_info, None)
                .expect("Failed to create Descriptor Set Layout to the video.")
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
    unsafe fn create_video_sampler(
        device: &ash::Device,
        conversion: vk::SamplerYcbcrConversion,
    ) -> vk::Sampler {
        unsafe {
            let mut conversion_info =
                vk::SamplerYcbcrConversionInfo::default().conversion(conversion);

            let sampler_info = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .push(&mut conversion_info);

            device
                .create_sampler(&sampler_info, None)
                .expect("Failed to create a Vulkan Video Sampler.")
        }
    }

    unsafe fn transition_dpb_to_graphic(
        device: &ash::Device,
        command_buffer: vk::CommandBuffer,
        image: vk::Image,
        current_layer: u32,
        video_queue_index: u32,
        graphics_queue_index: u32,
    ) {
        unsafe {
            let barrier = [vk::ImageMemoryBarrier2::default()
                .src_queue_family_index(video_queue_index)
                .src_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
                .src_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
                .dst_queue_family_index(graphics_queue_index)
                .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_access_mask(vk::AccessFlags2::SHADER_READ_KHR)
                .old_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image(image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: current_layer,
                    layer_count: 1,
                })];

            let dependency_info = vk::DependencyInfo::default().image_memory_barriers(&barrier);
            device.cmd_pipeline_barrier2(command_buffer, &dependency_info);
        }
    }
    unsafe fn update_video_descriptor_set(
        device: &Device,
        descriptor_set: vk::DescriptorSet,
        current_dpb_image_view: vk::ImageView,
    ) {
        unsafe {
            let image_info = vk::DescriptorImageInfo::default()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(current_dpb_image_view)
                .sampler(vk::Sampler::null());

            let write_set = vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(std::slice::from_ref(&image_info));

            device.update_descriptor_sets(&[write_set], &[]);
        }
    }

    unsafe fn transition_graphic_to_dpb(
        device: &ash::Device,
        command_buffer: vk::CommandBuffer,
        image: vk::Image,
        current_layer: u32,
        video_queue_index: u32,
        graphics_queue_index: u32,
    ) {
        unsafe {
            let barrier = [vk::ImageMemoryBarrier2::default()
                .src_queue_family_index(graphics_queue_index)
                .src_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .src_access_mask(vk::AccessFlags2::SHADER_READ_KHR)
                .dst_queue_family_index(video_queue_index)
                .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
                .dst_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
                .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .new_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
                .image(image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: current_layer,
                    layer_count: 1,
                })];

            let dependency_info = vk::DependencyInfo::default().image_memory_barriers(&barrier);
            device.cmd_pipeline_barrier2(command_buffer, &dependency_info);
        }
    }
    unsafe fn acquire_dpb_on_graphic(
        device: &ash::Device,
        cmd_buf_graphics: vk::CommandBuffer,
        dpb_image: vk::Image,
        decode_layout: vk::ImageLayout,
        video_queue_family: u32,
        graphics_queue_family: u32,
    ) {
        let acquire_barrier = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::NONE)
            .src_access_mask(vk::AccessFlags2::NONE)
            .dst_stage_mask(vk::PipelineStageFlags2::TRANSFER)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
            .old_layout(decode_layout)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_queue_family_index(video_queue_family)
            .dst_queue_family_index(graphics_queue_family)
            .image(dpb_image)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });

        let dependency_info = vk::DependencyInfo::default()
            .image_memory_barriers(std::slice::from_ref(&acquire_barrier));

        // Grava a barreira no command buffer de gráficos
        unsafe {
            device.cmd_pipeline_barrier2(cmd_buf_graphics, &dependency_info);
        }
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
