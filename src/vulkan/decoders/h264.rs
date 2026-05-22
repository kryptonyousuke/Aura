use crate::vulkan::decoder::Decoder;
use crate::vulkan::vk_init::Aura;
use ash::khr::video_queue;
use ash::vk::TaggedStructure;
use ash::{Device, vk};
use rayon::prelude::*;
use std::mem::MaybeUninit;
use std::u64;
pub trait H264Decoder {
    fn decode_frame(&mut self, bitstream_data: &[u8], is_first_frame: bool);
    unsafe fn create_h264_session_parameters(
        device: &Device,
        video_loader: &video_queue::Device,
        session: vk::VideoSessionKHR,
    ) -> vk::VideoSessionParametersKHR;
}
impl H264Decoder for Aura {
    fn decode_frame(&mut self, bitstream_data: &[u8], is_first_frame: bool) {
        let frame_idx = (self.current_frame_index % self.dpb_pool_size) as usize;
        let (_, _, dst_view) = self.dst_pool[frame_idx];
        let (_, _, dpb_view) = self.dpb_pool[frame_idx];
        log::debug!("current_frame_index: {}", self.current_frame_index);
        log::debug!("dpb_pool_size: {}", self.dpb_pool_size);
        log::debug!("frame_idx: {}", frame_idx);
        unsafe {
            let swapchain_sync_idx =
                (self.current_frame_index % self.frames_in_flight as usize) as usize;
            log::debug!("swapchain_sync_idx: {}", swapchain_sync_idx);
            let _ = self
                .device
                .wait_for_fences(&[self.render_fences[swapchain_sync_idx]], true, u64::MAX)
                .unwrap();
            let _ = self
                .device
                .reset_fences(&[self.render_fences[swapchain_sync_idx]]);
            let (image_index, _is_suboptimal) = self
                .swapchain_loader
                .acquire_next_image(
                    self.swapchain,
                    u64::MAX,
                    self.present_complete_semaphores[swapchain_sync_idx],
                    vk::Fence::null(),
                )
                .unwrap();

            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

            self.device
                .begin_command_buffer(self.video_command_buffers[swapchain_sync_idx], &begin_info)
                .unwrap();

            let image_barriers = [
                vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::NONE)
                    .src_access_mask(vk::AccessFlags2::NONE)
                    .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
                    .dst_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::VIDEO_DECODE_DST_KHR)
                    .image(self.dst_pool[frame_idx].0)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }),
                vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::NONE)
                    .src_access_mask(vk::AccessFlags2::NONE)
                    .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
                    .dst_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
                    .image(self.dpb_pool[frame_idx].0)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }),
            ];
            let dependency_info =
                vk::DependencyInfo::default().image_memory_barriers(&image_barriers);

            self.device.cmd_pipeline_barrier2(
                self.video_command_buffers[swapchain_sync_idx],
                &dependency_info,
            );

            let mut std_pic_info: vk::native::StdVideoDecodeH264PictureInfo =
                MaybeUninit::zeroed().assume_init();
            std_pic_info.flags.set_IdrPicFlag(is_first_frame as u32);
            std_pic_info.flags.set_is_reference(1);
            std_pic_info.frame_num = (self.current_frame_index % 16) as u16;
            std_pic_info.PicOrderCnt = [self.current_frame_index as i32, 0];

            let mut h264_decode_info = vk::VideoDecodeH264PictureInfoKHR::default()
                .std_picture_info(&std_pic_info)
                .slice_offsets(&[0]);

            let mut std_setup_info: vk::native::StdVideoDecodeH264ReferenceInfo =
                MaybeUninit::zeroed().assume_init();
            std_setup_info.FrameNum = std_pic_info.frame_num;

            let mut h264_setup_slot_info_decode =
                vk::VideoDecodeH264DpbSlotInfoKHR::default().std_reference_info(&std_setup_info);

            let mut h264_setup_slot_info_begin =
                vk::VideoDecodeH264DpbSlotInfoKHR::default().std_reference_info(&std_setup_info);

            let setup_resource = vk::VideoPictureResourceInfoKHR::default()
                .image_view_binding(dpb_view)
                .coded_extent(self.extent)
                .base_array_layer(0);

            let setup_slot_decode = vk::VideoReferenceSlotInfoKHR::default()
                .slot_index(frame_idx as i32)
                .picture_resource(&setup_resource)
                .push(&mut h264_setup_slot_info_decode);

            let setup_slot_begin = vk::VideoReferenceSlotInfoKHR::default()
                .slot_index(-1)
                .picture_resource(&setup_resource)
                .push(&mut h264_setup_slot_info_begin);

            let mut refs_std = Vec::new();
            let mut refs_resource = Vec::new();
            let mut refs_h264: Vec<vk::VideoDecodeH264DpbSlotInfoKHR>;

            let mut reference_slots: Vec<vk::VideoReferenceSlotInfoKHR> = Vec::new();

            let mut coding_reference_slots: Vec<vk::VideoReferenceSlotInfoKHR> =
                vec![setup_slot_begin];

            if !is_first_frame {
                let max_refs = std::cmp::min(
                    self.current_frame_index as u32,
                    (self.dpb_pool_size as u32) - 1,
                );
                for i in 0..max_refs {
                    let ref_offset = (i + 1) as usize;
                    let ref_idx =
                        (self.current_frame_index.wrapping_sub(ref_offset)) % self.dpb_pool_size;

                    if ref_idx < self.current_frame_index {
                        let (_, _, view) = self.dpb_pool[ref_idx];

                        let mut std_ref: vk::native::StdVideoDecodeH264ReferenceInfo =
                            MaybeUninit::zeroed().assume_init();
                        std_ref.FrameNum = ((self.current_frame_index - ref_offset) % 16) as u16;
                        refs_std.push(std_ref);

                        refs_resource.push(
                            vk::VideoPictureResourceInfoKHR::default()
                                .image_view_binding(view)
                                .coded_extent(self.extent)
                                .base_array_layer(0),
                        );
                    }
                }

                refs_h264 = refs_std
                    .par_iter()
                    .map(|std_ref| {
                        vk::VideoDecodeH264DpbSlotInfoKHR::default().std_reference_info(std_ref)
                    })
                    .collect();

                for (i, h264_info) in refs_h264.iter_mut().enumerate() {
                    let ref_offset = i + 1;
                    let ref_idx =
                        (self.current_frame_index.wrapping_sub(ref_offset)) % self.dpb_pool_size;

                    let slot = vk::VideoReferenceSlotInfoKHR::default()
                        .slot_index(ref_idx as i32)
                        .picture_resource(&refs_resource[i])
                        .push(h264_info);

                    reference_slots.push(slot);
                    coding_reference_slots.push(slot);
                }
            }

            // 5. Begin Coding
            let begin_coding_info = vk::VideoBeginCodingInfoKHR::default()
                .video_session(self.session)
                .video_session_parameters(self.session_parameters)
                .reference_slots(&coding_reference_slots);

            self.video_loader.cmd_begin_video_coding(
                self.video_command_buffers[swapchain_sync_idx],
                &begin_coding_info,
            );

            if is_first_frame {
                let control_info = vk::VideoCodingControlInfoKHR::default()
                    .flags(vk::VideoCodingControlFlagsKHR::RESET);
                self.video_loader.cmd_control_video_coding(
                    self.video_command_buffers[swapchain_sync_idx],
                    &control_info,
                );
            }

            // 6. Decode
            let aligned_size = (bitstream_data.len() as u64 + 127) & !127;

            let dst_resource = vk::VideoPictureResourceInfoKHR::default()
                .image_view_binding(dst_view)
                .coded_extent(self.extent)
                .base_array_layer(0);

            let decode_info = vk::VideoDecodeInfoKHR::default()
                .src_buffer(self.bitstream_buffer)
                .src_buffer_offset(0)
                .src_buffer_range(aligned_size)
                .dst_picture_resource(dst_resource)
                .setup_reference_slot(&setup_slot_decode)
                .reference_slots(&reference_slots)
                .push(&mut h264_decode_info);

            self.decode_loader
                .cmd_decode_video(self.video_command_buffers[swapchain_sync_idx], &decode_info);

            // 7. End Coding & Submit Execution
            self.video_loader.cmd_end_video_coding(
                self.video_command_buffers[swapchain_sync_idx],
                &vk::VideoEndCodingInfoKHR::default(),
            );

            // Aura::transition_dpb_to_graphic(
            //     &self.device,
            //     self.video_command_buffers[swapchain_sync_idx],
            //     self.dpb_pool[image_index as usize].0,
            //     0,
            //     self._video_queue_family_index,
            //     self._graphics_queue_family_index,
            // );

            self.device
                .end_command_buffer(self.video_command_buffers[swapchain_sync_idx])
                .expect("Erro buffer");
            let video_command_buffer_submit_info = &[vk::CommandBufferSubmitInfo::default()
                .command_buffer(self.video_command_buffers[swapchain_sync_idx])];
            let render_semaphores_submit_info = &[vk::SemaphoreSubmitInfo::default()
                .semaphore(self.render_complete_semaphores[image_index as usize])
                .stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)];

            let present_semaphores_submit_info = &[vk::SemaphoreSubmitInfo::default()
                .semaphore(self.present_complete_semaphores[swapchain_sync_idx as usize])
                .stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)];

            let submit_info = vk::SubmitInfo2::default()
                .command_buffer_infos(video_command_buffer_submit_info)
                .wait_semaphore_infos(present_semaphores_submit_info)
                .signal_semaphore_infos(render_semaphores_submit_info);
            self.device
                .queue_submit2(
                    self.video_queue,
                    &[submit_info],
                    self.render_fences[swapchain_sync_idx],
                )
                .unwrap();
            let swapchains = [self.swapchain];
            let image_indices = [image_index];
            let present_wait_semaphores = [self.render_complete_semaphores[image_index as usize]];

            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&present_wait_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            self.swapchain_loader
                .queue_present(self.graphics_queue, &present_info)
                .unwrap();

            log::debug!("Frame was sent to vulkan!");
            self.current_frame_index += 1;
        }
    }

    unsafe fn create_h264_session_parameters(
        _device: &Device,
        video_loader: &video_queue::Device,
        session: vk::VideoSessionKHR,
    ) -> vk::VideoSessionParametersKHR {
        let mut sps_flags: vk::native::StdVideoH264SpsFlags =
            unsafe { MaybeUninit::zeroed().assume_init() };
        sps_flags.set_frame_mbs_only_flag(1);
        sps_flags.set_direct_8x8_inference_flag(1);

        let mut std_sps: vk::native::StdVideoH264SequenceParameterSet =
            unsafe { MaybeUninit::zeroed().assume_init() };
        std_sps.flags = sps_flags;
        std_sps.profile_idc = vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN;
        std_sps.level_idc = vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_0;
        std_sps.chroma_format_idc =
            vk::native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_420;
        std_sps.pic_width_in_mbs_minus1 = (1920 / 16) - 1;
        std_sps.pic_height_in_map_units_minus1 = (1080 / 16) - 1;
        std_sps.max_num_ref_frames = 16;

        let std_pps: vk::native::StdVideoH264PictureParameterSet =
            unsafe { MaybeUninit::zeroed().assume_init() };

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
