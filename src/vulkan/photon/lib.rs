use super::decoder::{self, Decoder, DecodingSession};
use crate::vulkan::photon::sampler;
use crate::vulkan::photon::types::VideoCodecsProfiles::UnifiedVideoProfile;
use crate::vulkan::{photon::types::PhotonError, pipeline};
use anyhow::Result;
use ash::{self, khr, vk};

pub struct DecodingInstance {
    pub(crate) _video_queue_family_index: u32,
    pub(crate) _graphics_queue_family_index: u32,
    pub(crate) device: ash::Device,
    pub(crate) video_ext: khr::video_queue::Instance,
    pub(crate) swapchain_loader: Option<khr::swapchain::Device>,
    pub(crate) graphics_queue: vk::Queue,
    pub(crate) video_queue: vk::Queue,

    pub(crate) video_session: DecodingSession,
    pub(crate) video_sampler: vk::Sampler,
    pub(crate) ycbcr_conversion: vk::SamplerYcbcrConversion,
    pub(crate) video_command_buffers: Vec<vk::CommandBuffer>,
    pub(crate) graphics_command_buffers: Vec<vk::CommandBuffer>,
    pub(crate) bitstream_buffers: Vec<vk::Buffer>,
    pub(crate) current_frame_count_idx: usize,
    pub(crate) bitstream_sizes: Vec<u32>,

    pub(crate) frames_in_flight_sync_idx: usize,
    pub(crate) target_available_image_idx: u32,
    pub(crate) bitstream_memories: Vec<vk::DeviceMemory>,
    pub(crate) target_image_views: Vec<vk::ImageView>,
    pub(crate) render_extent: vk::Extent2D,
    pub(crate) target_images: Vec<vk::Image>,

    pub(crate) swapchain: Option<vk::SwapchainKHR>, // For our simple present implementation.
    pub(crate) render_offsets: vk::Offset2D,
    pub(crate) frames_in_flight: usize, // How many frames will be decoded at the same time.
    pub(crate) dpb_frame_nums: Vec<u16>,

    pub(crate) dpb_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>, // Decoded Pictures Buffer used as reference to decode P-frames and B-frames.
    pub(crate) dst_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>, // Stores the current decoded images.
    pub(crate) dpb_pool_size: usize,
    pub(crate) viewport: vk::Viewport,
    pub(crate) scissor: vk::Rect2D,

    pub(crate) graphics_complete_semaphores: Vec<vk::Semaphore>,
    pub(crate) wait_to_decode_semaphores: Vec<vk::Semaphore>,
    pub(crate) decode_complete_semaphores: Vec<vk::Semaphore>,
    pub(crate) video_fences: Vec<vk::Fence>,

    pub(crate) pipeline: vk::Pipeline,
    pub(crate) pipeline_layout: vk::PipelineLayout,
    pub(crate) descriptor_sets: Vec<vk::DescriptorSet>,
    pub(crate) video_extent: vk::Extent2D, // <- video extent proportion can usually be different of the render extent
                                           // because some codecs may need a truncated decode extent due to its intrisics
                                           // macroblocks decoding logic.
}
impl DecodingInstance {
    pub fn new(
        _video_queue_family_index: u32,
        _graphics_queue_family_index: u32,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: ash::Device,
        video_ext: khr::video_queue::Instance,
        swapchain_loader: Option<khr::swapchain::Device>,
        graphics_queue: vk::Queue,
        video_queue: vk::Queue,
        pipeline: vk::Pipeline,
        pipeline_layout: vk::PipelineLayout,
        wait_to_decode_semaphores: &[vk::Semaphore],
        viewport: vk::Viewport,
        scissor: vk::Rect2D,
        frames_in_flight: usize,
        render_extent: vk::Extent2D,
        render_offsets: vk::Offset2D,
        video_extent: vk::Extent2D,
        target_available_image_idx: u32,
        target_image_views: Vec<vk::ImageView>,
        descriptor_sets: Vec<vk::DescriptorSet>,
        video_command_buffers: Vec<vk::CommandBuffer>,
        graphics_command_buffers: Vec<vk::CommandBuffer>,
        target_images: Vec<vk::Image>,
        dpb_pool_size: usize,
        graphics_complete_semaphores: &[vk::Semaphore],
        decode_complete_semaphores: &[vk::Semaphore],
        video_fences: &[vk::Fence],
        swapchain: Option<vk::SwapchainKHR>,
        dpb_format: vk::Format,
        dst_format: vk::Format,
        video_codec: vk::VideoCodecOperationFlagsKHR,
        video_profile_indicator: UnifiedVideoProfile,
        video_profile: &mut vk::VideoProfileInfoKHR,
        ycbcr_conversion: vk::SamplerYcbcrConversion,
        video_sampler: vk::Sampler,
        extradata: &[u8],
    ) -> Result<Self> {
        let mut bitstream_buffers = vec![vk::Buffer::null(); frames_in_flight];
        let mut bitstream_memories = vec![vk::DeviceMemory::null(); frames_in_flight];
        let mut bitstream_sizes = vec![0_u32; frames_in_flight];
        for i in 0..frames_in_flight {
            let (bitstream_buffer, bitstream_memory, bitstream_size) =
                Self::create_bitstream_buffer(
                    instance,
                    &video_ext,
                    physical_device,
                    &device,
                    video_profile,
                );
            bitstream_buffers[i] = bitstream_buffer;
            bitstream_memories[i] = bitstream_memory;
            bitstream_sizes[i] = bitstream_size;
        }

        let decoding_session = Self::setup_decoder(
            instance,
            physical_device,
            &device,
            extradata,
            _video_queue_family_index,
            video_codec,
            Some(video_profile_indicator),
            vk::VideoComponentBitDepthFlagsKHR::TYPE_8,
            vk::VideoComponentBitDepthFlagsKHR::TYPE_8,
            vk::VideoChromaSubsamplingFlagsKHR::TYPE_420,
            dpb_format,
            dst_format,
        )?;

        let (dpb_pool, dst_pool) = Self::create_dpb_dst_pool(
            instance,
            physical_device,
            &device,
            video_profile,
            ycbcr_conversion,
            dpb_pool_size,
            dpb_format,
            dst_format,
            video_extent,
        )?;

        let decoding_instance = Self {
            _graphics_queue_family_index: _graphics_queue_family_index,
            _video_queue_family_index: _video_queue_family_index,
            device: device,
            pipeline: pipeline,
            pipeline_layout: pipeline_layout,
            wait_to_decode_semaphores: wait_to_decode_semaphores.to_vec(),
            video_ext: video_ext,
            swapchain_loader: swapchain_loader,

            graphics_queue: graphics_queue,

            video_queue: video_queue,
            video_session: decoding_session,
            bitstream_buffers: bitstream_buffers,
            bitstream_memories: bitstream_memories,
            bitstream_sizes: bitstream_sizes,

            video_command_buffers: video_command_buffers,
            graphics_command_buffers: graphics_command_buffers,
            current_frame_count_idx: 0usize,

            frames_in_flight_sync_idx: 0,
            target_available_image_idx: target_available_image_idx,
            target_image_views: target_image_views,
            render_extent: render_extent,
            render_offsets: render_offsets,
            target_images: target_images,

            swapchain: swapchain,
            frames_in_flight: frames_in_flight,

            dpb_frame_nums: vec![0u16; dpb_pool_size],
            dpb_pool: dpb_pool,
            dst_pool: dst_pool,
            dpb_pool_size: dpb_pool_size,

            graphics_complete_semaphores: graphics_complete_semaphores.to_vec(),
            decode_complete_semaphores: decode_complete_semaphores.to_vec(),
            video_fences: video_fences.to_vec(),
            ycbcr_conversion: ycbcr_conversion,
            viewport: viewport,
            scissor: scissor,
            video_extent: video_extent,
            video_sampler: video_sampler,
            descriptor_sets: descriptor_sets,
            
        };

        Ok(decoding_instance)
    }
    pub fn get_frames_in_flight_sync_idx(&self) -> usize {
        self.frames_in_flight_sync_idx
    }
    pub fn set_target_available_image_idx(&mut self, target_available_image_idx: u32) {
        self.target_available_image_idx = target_available_image_idx;
    }
}
// Draft, won't work.
impl Drop for DecodingInstance {
    fn drop(&mut self) {
        unsafe{
            log::debug!("Trying to exit safely.");
            for i in 0..self.frames_in_flight {
                self.device
                    .destroy_buffer(self.bitstream_buffers[i as usize], None);
                self.device
                    .free_memory(self.bitstream_memories[i as usize], None);
            }
            log::debug!("BLEH.");
            
            if self.video_session.session_parameters != vk::VideoSessionParametersKHR::null() {
                self.video_session.video_loader
                    .destroy_video_session_parameters(self.video_session.session_parameters, None);
                self.video_session.session_parameters = vk::VideoSessionParametersKHR::null();
            }
            if self.video_session.session != vk::VideoSessionKHR::null() {
                self.video_session.video_loader.destroy_video_session(self.video_session.session, None);
                self.video_session.session = vk::VideoSessionKHR::null();
            }
            for mem in &self.video_session._session_memories {
                self.device.free_memory(*mem, None);
            }
            
            self.device.destroy_sampler(self.video_sampler, None);
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
        }
    }
}
