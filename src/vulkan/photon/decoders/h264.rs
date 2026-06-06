//! # H264 decoder
//! Deals with the logic of the decoding pípeline.

use super::super::decoder::Decoder;
use super::super::lib::DecodingInstance;
use crate::vulkan::pipeline::Pipeline;
use crate::vulkan::vk_init::Aura;
use anyhow::Result;
use ash::khr::video_queue;
use ash::vk::TaggedStructure;
use ash::{Device, vk};
use std::mem::MaybeUninit;
pub trait H264Decoder {
    fn decode_frame(
        &mut self,
        bitstream_data: &[u8],
        slice_offsets: &[u32],
        is_first_frame: bool,
        sps: &vk::native::StdVideoH264SequenceParameterSet,
    ) -> Result<()>;
    fn present_swapchain(&mut self);
    unsafe fn create_h264_session_parameters(
        device: &Device,
        video_loader: &video_queue::Device,
        extradata: &[u8],
        session: vk::VideoSessionKHR,
    ) -> vk::VideoSessionParametersKHR;
}
impl H264Decoder for DecodingInstance {
    /// Decodes a h264 frame and write it into a target image.
    fn decode_frame(
        &mut self,
        bitstream_data: &[u8],
        slice_offsets: &[u32],
        is_first_frame: bool,
        sps: &vk::native::StdVideoH264SequenceParameterSet,
    ) -> Result<()> {
        let frame_idx = self.current_frame_count_idx % self.dpb_pool_size;
        let (dst_image, _, dst_view) = self.dst_pool[frame_idx];
        let (_dpb_image, _, dpb_view) = self.dpb_pool[frame_idx];
        log::debug!("current_frame_count_idx: {}", self.current_frame_count_idx);
        log::debug!("dpb_pool_size: {}", self.dpb_pool_size);
        log::debug!("frame_idx: {frame_idx}");
        let frames_in_flight_sync_idx =
            (self.current_frame_count_idx % self.frames_in_flight as usize) as usize;
        let aligned_size = self.bitstream_sizes[frames_in_flight_sync_idx];
        unsafe {
            self.upload_bitstream_packet(bitstream_data, frames_in_flight_sync_idx);

            log::debug!("frames_in_flight_sync_idx: {frames_in_flight_sync_idx}");
            let () = self
                .device
                .wait_for_fences(
                    &[self.video_fences[frames_in_flight_sync_idx]],
                    true,
                    u64::MAX,
                )
                ?;
            let () = self
                .device
                .reset_fences(&[self.video_fences[frames_in_flight_sync_idx]])
                ?;
            let color_attachment_info = vk::RenderingAttachmentInfoKHR::default()
                .image_view(
                    self.target_image_views[self
                        .target_available_image_idx
                        .expect("Can't get the target available image index.")
                        as usize],
                )
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [0.0, 0.0, 0.0, 1.0],
                    },
                });

            let color_attachments = [color_attachment_info];

            let rendering_info = vk::RenderingInfoKHR::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: self.render_extent,
                })
                .layer_count(1)
                .color_attachments(&color_attachments);
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

            self.device
                .begin_command_buffer(
                    self.video_command_buffers[frames_in_flight_sync_idx],
                    &begin_info,
                )
                ?;

            let subresource_range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(u32::try_from(frame_idx)?)
                .layer_count(1);
            let swapchain_subresource_range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1);

            // Barriers
            let buffer_barriers = [vk::BufferMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::HOST)
                .src_access_mask(vk::AccessFlags2::HOST_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
                .dst_access_mask(vk::AccessFlags2::VIDEO_DECODE_READ_KHR)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(self.bitstream_buffers[frames_in_flight_sync_idx])
                .offset(0)
                .size(vk::WHOLE_SIZE)];
            let image_barriers = [
                vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::NONE)
                    .src_access_mask(vk::AccessFlags2::NONE)
                    .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
                    .dst_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::VIDEO_DECODE_DST_KHR)
                    .image(self.dst_pool[frame_idx].0)
                    .subresource_range(subresource_range),
                vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::NONE)
                    .src_access_mask(vk::AccessFlags2::NONE)
                    .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
                    .dst_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
                    .image(self.dpb_pool[frame_idx].0)
                    .subresource_range(subresource_range),
            ];
            let dependency_info = vk::DependencyInfo::default()
                .image_memory_barriers(&image_barriers)
                .buffer_memory_barriers(&buffer_barriers);

            self.device.cmd_pipeline_barrier2(
                self.video_command_buffers[frames_in_flight_sync_idx],
                &dependency_info,
            );
            let slice_offset = slice_offsets[0] as usize;
            let slice_data = &bitstream_data[slice_offset..];
            let mut std_pic_info: vk::native::StdVideoDecodeH264PictureInfo =
                MaybeUninit::zeroed().assume_init();
            let mut real_frame_num = 0;
            let mut real_poc = 0;
            if let Some(nalu_header) = super::super::util::converter::NaluHeader::parse(slice_data)
            {
                log::debug!("NALU Parsed: {nalu_header:?}");
                let is_reference = nalu_header.nal_ref_idc > 0;
                let is_idr = nalu_header.nal_unit_type == 5;
                std_pic_info.flags.set_IdrPicFlag(u32::from(is_idr));
                std_pic_info.flags.set_is_reference(u32::from(is_reference));
                let sps_info = super::super::util::converter::SpsInfo {
                    log2_max_frame_num_minus4: sps.log2_max_frame_num_minus4,
                    frame_mbs_only_flag: sps.flags.frame_mbs_only_flag() != 0,
                    pic_order_cnt_type: u8::try_from(sps.pic_order_cnt_type)?,
                    log2_max_pic_order_cnt_lsb_minus4: sps.log2_max_pic_order_cnt_lsb_minus4,
                };

                if let Some(slice_header) = super::super::util::converter::parse_slice_header(
                    &slice_data[nalu_header.slice_header_offset..],
                    nalu_header.nal_unit_type,
                    &sps_info,
                ) {
                    real_frame_num = slice_header.frame_num;
                    self.dpb_frame_nums[frame_idx] = real_frame_num;
                    real_poc = match sps.pic_order_cnt_type {
                        0 => slice_header.pic_order_cnt_lsb.cast_signed(),
                        2 => i32::from(real_frame_num) * 2,
                        _ => {
                            log::warn!(
                                "pic_order_cnt_type {} does not exist, using fallback.",
                                sps.pic_order_cnt_type
                            );
                            i32::from(real_frame_num) * 2
                        }
                    };
                    log::debug!(
                        "Slice Header successfully decoded. FrameNum: {real_frame_num}, POC: {real_poc}",
                    );
                } else {
                    log::warn!("Failed to parse slice_header, using linear fallback.");
                    real_frame_num = u16::try_from(self.current_frame_count_idx % 16)?;
                    real_poc = i32::try_from(self.current_frame_count_idx)?;
                }

                std_pic_info.frame_num = real_frame_num;
                std_pic_info.PicOrderCnt = [real_poc, 0];
            }
            let mut h264_decode_info = vk::VideoDecodeH264PictureInfoKHR::default()
                .std_picture_info(&std_pic_info)
                .slice_offsets(slice_offsets);

            let mut std_setup_info: vk::native::StdVideoDecodeH264ReferenceInfo =
                MaybeUninit::zeroed().assume_init();
            std_setup_info.FrameNum = std_pic_info.frame_num;
            std_setup_info.PicOrderCnt = std_pic_info.PicOrderCnt;

            let mut h264_setup_slot_info_decode =
                vk::VideoDecodeH264DpbSlotInfoKHR::default().std_reference_info(&std_setup_info);

            let mut h264_setup_slot_info_begin =
                vk::VideoDecodeH264DpbSlotInfoKHR::default().std_reference_info(&std_setup_info);
            let setup_resource = vk::VideoPictureResourceInfoKHR::default()
                .image_view_binding(dpb_view)
                .coded_extent(self.video_extent)
                .base_array_layer(0);
            let setup_slot_decode = vk::VideoReferenceSlotInfoKHR::default()
                .slot_index(i32::try_from(frame_idx)?)
                .picture_resource(&setup_resource)
                .push(&mut h264_setup_slot_info_decode);
            let setup_slot_begin = vk::VideoReferenceSlotInfoKHR::default()
                .slot_index(-1)
                .picture_resource(&setup_resource)
                .push(&mut h264_setup_slot_info_begin);

            let reference_slots: Vec<vk::VideoReferenceSlotInfoKHR> = Vec::new();
            let coding_reference_slots: Vec<vk::VideoReferenceSlotInfoKHR> = vec![setup_slot_begin];

            // Start the coding session
            let begin_coding_info = vk::VideoBeginCodingInfoKHR::default()
                .video_session(self.video_session.session)
                .video_session_parameters(self.video_session.session_parameters)
                .reference_slots(&coding_reference_slots);

            self.video_session.video_loader.cmd_begin_video_coding(
                self.video_command_buffers[frames_in_flight_sync_idx],
                &begin_coding_info,
            );

            if is_first_frame {
                let control_info = vk::VideoCodingControlInfoKHR::default()
                    .flags(vk::VideoCodingControlFlagsKHR::RESET);
                self.video_session.video_loader.cmd_control_video_coding(
                    self.video_command_buffers[frames_in_flight_sync_idx],
                    &control_info,
                );
            }

            // Decode the bitstream
            let dst_resource = vk::VideoPictureResourceInfoKHR::default()
                .image_view_binding(dst_view)
                .coded_extent(self.video_extent)
                .base_array_layer(0);

            let decode_info = vk::VideoDecodeInfoKHR::default()
                .src_buffer(self.bitstream_buffers[frames_in_flight_sync_idx])
                .src_buffer_offset(0)
                .src_buffer_range(u64::from(aligned_size))
                .dst_picture_resource(dst_resource)
                .setup_reference_slot(&setup_slot_decode)
                .reference_slots(&reference_slots)
                .push(&mut h264_decode_info);

            self.video_session.decode_loader.cmd_decode_video(
                self.video_command_buffers[frames_in_flight_sync_idx],
                &decode_info,
            );

            // End coding session and submit execution
            self.video_session.video_loader.cmd_end_video_coding(
                self.video_command_buffers[frames_in_flight_sync_idx],
                &vk::VideoEndCodingInfoKHR::default(),
            );
            DecodingInstance::release_dst_on_graphic(
                &self.device,
                self.video_command_buffers[frames_in_flight_sync_idx],
                dst_image,
                subresource_range,
                self._video_queue_family_index,
                self._graphics_queue_family_index,
            );
            self.device
                .end_command_buffer(self.video_command_buffers[frames_in_flight_sync_idx])
                .expect("Erro buffer");

            let video_command_buffer_submit_info = &[vk::CommandBufferSubmitInfo::default()
                .command_buffer(self.video_command_buffers[frames_in_flight_sync_idx])];
            let render_semaphores_submit_info = &[vk::SemaphoreSubmitInfo::default()
                .semaphore(
                    self.decode_complete_semaphores[self
                        .target_available_image_idx
                        .expect("Can't get the target available image index.")
                        as usize],
                )
                .stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)];
            let wait_to_decode_semaphores_submit_info = &[vk::SemaphoreSubmitInfo::default()
                .semaphore(self.wait_to_decode_semaphores[frames_in_flight_sync_idx as usize])];
            let submit_info = vk::SubmitInfo2::default()
                .command_buffer_infos(video_command_buffer_submit_info)
                .wait_semaphore_infos(wait_to_decode_semaphores_submit_info)
                .signal_semaphore_infos(render_semaphores_submit_info);

            self.device
                .queue_submit2(self.video_queue, &[submit_info], vk::Fence::null())
                ?;
            self.device
                .begin_command_buffer(
                    self.graphics_command_buffers[frames_in_flight_sync_idx],
                    &vk::CommandBufferBeginInfo::default(),
                )
                ?;

            DecodingInstance::acquire_image_dst_on_graphic(
                &self.device,
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                dst_image,
                subresource_range,
                self._video_queue_family_index,
                self._graphics_queue_family_index,
            );

            self.device.cmd_bind_pipeline(
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );
            let descriptor_sets = [self.descriptor_sets[frames_in_flight_sync_idx]];
            let bind_descriptor_sets_info = vk::BindDescriptorSetsInfo::default()
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .descriptor_sets(&descriptor_sets)
                .layout(self.pipeline_layout);

            self.device.cmd_bind_descriptor_sets2(
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                &bind_descriptor_sets_info,
            );
            Aura::update_video_descriptor_set(
                &self.device,
                self.descriptor_sets[frames_in_flight_sync_idx],
                dst_view,
            );

            DecodingInstance::acquire_target_barrier(
                &self.device,
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                self.target_images[self
                    .target_available_image_idx
                    .expect("Can't get the target available image index.")
                    as usize],
                swapchain_subresource_range,
                self._graphics_queue_family_index,
            );

            // Dynamic Rendering
            self.device.cmd_begin_rendering(
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                &rendering_info,
            );
            let viewport = [self.viewport];
            let scissor = [self.scissor];
            self.device.cmd_set_viewport(
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                0,
                &viewport,
            );
            self.device.cmd_set_scissor(
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                0,
                &scissor,
            );

            self.device.cmd_draw(
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                3,
                1,
                0,
                0,
            );

            self.device
                .cmd_end_rendering(self.graphics_command_buffers[frames_in_flight_sync_idx]);

            let cmd_buf_graphics_info = [vk::CommandBufferSubmitInfo::default()
                .command_buffer(self.graphics_command_buffers[frames_in_flight_sync_idx])];
            let cmd_buf_graphics_wait_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(
                    self.decode_complete_semaphores[self
                        .target_available_image_idx
                        .expect("Can't get the target available image index.")
                        as usize],
                )
                .stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)];

            DecodingInstance::release_graphic_on_dst(
                &self.device,
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                dst_image,
                subresource_range,
                self._video_queue_family_index,
                self._graphics_queue_family_index,
            );

            DecodingInstance::release_target_barrier(
                &self.device,
                self.graphics_command_buffers[frames_in_flight_sync_idx],
                self.target_images[self
                    .target_available_image_idx
                    .expect("Can't get the target available image index.")
                    as usize],
                swapchain_subresource_range,
                self._graphics_queue_family_index,
            );

            self.device
                .end_command_buffer(self.graphics_command_buffers[frames_in_flight_sync_idx])
                ?;
            let cmd_buf_graphics_complete_infos = [vk::SemaphoreSubmitInfo::default().semaphore(
                self.graphics_complete_semaphores[self
                    .target_available_image_idx
                    .expect("Can't get the target available image index.")
                    as usize],
            )];
            let graphics_submit = vk::SubmitInfo2::default()
                .command_buffer_infos(&cmd_buf_graphics_info)
                .wait_semaphore_infos(&cmd_buf_graphics_wait_infos)
                .signal_semaphore_infos(&cmd_buf_graphics_complete_infos);
            self.device
                .queue_submit2(
                    self.graphics_queue,
                    &[graphics_submit],
                    self.video_fences[frames_in_flight_sync_idx],
                )
                ?;

            log::debug!("Frame was sent to vulkan!");
            self.present_swapchain();
            self.current_frame_count_idx += 1;
            Ok(())
        }
    }

    fn present_swapchain(&mut self) {
        if let Some(swapchain) = self.swapchain
            && let Some(swapchain_loader) = &self.swapchain_loader
            && let Some(target_available_image_idx) = &self.target_available_image_idx
        {
            let swapchains = [swapchain];
            let image_indices_available_for_present =
                std::slice::from_ref(target_available_image_idx);
            let present_wait_semaphores =
                [self.graphics_complete_semaphores[*target_available_image_idx as usize]];

            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&present_wait_semaphores)
                .swapchains(&swapchains)
                .image_indices(image_indices_available_for_present);

            unsafe {
                swapchain_loader
                    .queue_present(self.graphics_queue, &present_info)
                    .unwrap()
            };
        }
    }

    /// Make h264 session params.
    unsafe fn create_h264_session_parameters(
        _device: &Device,
        video_loader: &video_queue::Device,
        extradata: &[u8],
        session: vk::VideoSessionKHR,
    ) -> vk::VideoSessionParametersKHR {
        let std_sps = super::h264_parser::parse_sps(extradata).expect("Failed to parse SPS");

        let std_pps = super::h264_parser::parse_pps(extradata).expect("Failed to parse PPS");
        log::info!(
            "Resolution: {}x{}",
            (std_sps.pic_width_in_mbs_minus1 + 1) * 16,
            (std_sps.pic_height_in_map_units_minus1 + 1) * 16
        );
        log::info!(
            "log2_max_pic_order_cnt_lsb_minus4: {}",
            std_sps.log2_max_pic_order_cnt_lsb_minus4
        );
        log::info!("max_num_ref_frames: {}", std_sps.max_num_ref_frames);
        log::info!("CABAC: {}", std_pps.flags.entropy_coding_mode_flag());
        let add_info = vk::VideoDecodeH264SessionParametersAddInfoKHR::default()
            .std_sp_ss(std::slice::from_ref(&std_sps))
            .std_pp_ss(std::slice::from_ref(&std_pps));

        let mut h264_create = vk::VideoDecodeH264SessionParametersCreateInfoKHR::default()
            .max_std_sps_count(1)
            .max_std_pps_count(1)
            .parameters_add_info(&add_info);

        let params_info = vk::VideoSessionParametersCreateInfoKHR::default()
            .video_session(session)
            .push(&mut h264_create);
        unsafe {
            video_loader
                .create_video_session_parameters(&params_info, None)
                .unwrap()
        }
    }
}
