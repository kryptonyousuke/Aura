use crate::vulkan::photon::decoder::DecodingSession;
use ash::{
    self,
    vk
};

struct DecodingInstance {
    _video_queue_family_index: u32,
    _graphics_queue_family_index: u32,
    device: ash::Device,
    video_session: DecodingSession,
    video_command_buffers: Vec<vk::CommandBuffer>,
    graphics_command_buffers: Vec<vk::CommandBuffer>,
    bitstream_buffers: Vec<vk::Buffer>,
    current_frame_count_idx: usize,
    
    dpb_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>, // Decoded Pictures Buffer used as reference to decode P-frames and B-frames.
    dst_pool: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>, // Stores the current decoded image.
    dpb_pool_size: usize,
    
    video_fences: Vec<vk::Fence>,

    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    descriptor_sets: Vec<vk::DescriptorSet>,
    decode_complete_semaphores: Vec<vk::Semaphore>,
    video_extent: vk::Extent2D,
    
}
