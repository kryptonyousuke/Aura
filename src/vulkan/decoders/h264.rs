use crate::vulkan::decoder::Decoder;
use crate::vulkan::pipeline::Pipeline;
use crate::vulkan::vk_init::Aura;
use ash::khr::video_queue;
use ash::vk::TaggedStructure;
use ash::{Device, vk};
use std::mem::MaybeUninit;
use std::u64;
pub trait H264Decoder {
    fn decode_frame(&mut self, bitstream_data: &[u8], slice_offsets: &[u32], is_first_frame: bool);
    unsafe fn create_h264_session_parameters(
        device: &Device,
        video_loader: &video_queue::Device,
        extradata: &[u8],
        session: vk::VideoSessionKHR,
    ) -> vk::VideoSessionParametersKHR;
}
impl H264Decoder for Aura {
    fn decode_frame(&mut self, bitstream_data: &[u8], slice_offsets: &[u32], is_first_frame: bool) {
        // std::thread::sleep(std::time::Duration::from_millis(200));
        let frame_idx = (self.current_frame_index % self.dpb_pool_size) as usize;
        let (dst_image, _, dst_view) = self.dst_pool[frame_idx];
        let (_dpb_image, _, dpb_view) = self.dpb_pool[frame_idx];
        log::debug!("current_frame_index: {}", self.current_frame_index);
        log::debug!("dpb_pool_size: {}", self.dpb_pool_size);
        log::debug!("frame_idx: {}", frame_idx);
        let aligned_size = (bitstream_data.len() as u64 + 127) & !127;
        unsafe {
            let swapchain_sync_idx =
                (self.current_frame_index % self.frames_in_flight as usize) as usize;
            self.upload_bitstream_packet(bitstream_data, swapchain_sync_idx);

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
            let color_attachment_info = vk::RenderingAttachmentInfoKHR::default()
                .image_view(self.swapchain_image_views[image_index as usize])
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
                    extent: self.swapchain_extent,
                })
                .layer_count(1)
                .color_attachments(&color_attachments);
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

            self.device
                .begin_command_buffer(self.video_command_buffers[swapchain_sync_idx], &begin_info)
                .unwrap();
            let subresource_range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(frame_idx as u32)
                .layer_count(1);
            let swapchain_subresource_range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1);
            let buffer_barriers = [vk::BufferMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::HOST)
                .src_access_mask(vk::AccessFlags2::HOST_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
                .dst_access_mask(vk::AccessFlags2::VIDEO_DECODE_READ_KHR)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(self.bitstream_buffers[swapchain_sync_idx])
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
                .slice_offsets(slice_offsets);

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
                    .iter()
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

            let dst_resource = vk::VideoPictureResourceInfoKHR::default()
                .image_view_binding(dst_view)
                .coded_extent(self.extent)
                .base_array_layer(0);

            let decode_info = vk::VideoDecodeInfoKHR::default()
                .src_buffer(self.bitstream_buffers[swapchain_sync_idx])
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
            Aura::release_dst_on_graphic(
                &self.device,
                self.video_command_buffers[swapchain_sync_idx],
                dst_image,
                subresource_range,
                self._video_queue_family_index,
                self._graphics_queue_family_index,
            );

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
                .queue_submit2(self.video_queue, &[submit_info], vk::Fence::null())
                .unwrap();
            self.device
                .begin_command_buffer(
                    self.graphics_command_buffers[swapchain_sync_idx],
                    &vk::CommandBufferBeginInfo::default(),
                )
                .unwrap();

            Aura::acquire_image_dst_on_graphic(
                &self.device,
                self.graphics_command_buffers[swapchain_sync_idx],
                dst_image,
                subresource_range,
                self._video_queue_family_index,
                self._graphics_queue_family_index,
            );

            self.device.cmd_bind_pipeline(
                self.graphics_command_buffers[swapchain_sync_idx],
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );
            let descriptor_sets = [self.descriptor_sets[swapchain_sync_idx]];
            let bind_descriptor_sets_info = vk::BindDescriptorSetsInfo::default()
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .descriptor_sets(&descriptor_sets)
                .layout(self.pipeline_layout);

            self.device.cmd_bind_descriptor_sets2(
                self.graphics_command_buffers[swapchain_sync_idx],
                &bind_descriptor_sets_info,
            );
            Aura::update_video_descriptor_set(
                &self.device,
                self.descriptor_sets[swapchain_sync_idx],
                dst_view,
            );

            Aura::acquire_swapchain_barrier(
                &self.device,
                self.graphics_command_buffers[swapchain_sync_idx],
                self.swapchain_images[image_index as usize],
                swapchain_subresource_range,
                self._graphics_queue_family_index,
            );
            self.device.cmd_begin_rendering(
                self.graphics_command_buffers[swapchain_sync_idx],
                &rendering_info,
            );
            let viewport = [self.viewport];
            let scissor = [self.scissor];
            self.device.cmd_set_viewport(
                self.graphics_command_buffers[swapchain_sync_idx],
                0,
                &viewport,
            );
            self.device.cmd_set_scissor(
                self.graphics_command_buffers[swapchain_sync_idx],
                0,
                &scissor,
            );

            self.device.cmd_draw(
                self.graphics_command_buffers[swapchain_sync_idx],
                3,
                1,
                0,
                0,
            );

            self.device
                .cmd_end_rendering(self.graphics_command_buffers[swapchain_sync_idx]);

            let cmd_buf_graphics_info = [vk::CommandBufferSubmitInfo::default()
                .command_buffer(self.graphics_command_buffers[swapchain_sync_idx])];
            let cmd_buf_graphics_wait_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.render_complete_semaphores[image_index as usize])
                .stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)];

            Aura::release_graphic_on_dst(
                &self.device,
                self.graphics_command_buffers[swapchain_sync_idx],
                dst_image,
                subresource_range,
                self._video_queue_family_index,
                self._graphics_queue_family_index,
            );

            Aura::release_swapchain_barrier(
                &self.device,
                self.graphics_command_buffers[swapchain_sync_idx],
                self.swapchain_images[image_index as usize],
                swapchain_subresource_range,
                self._graphics_queue_family_index,
            );

            self.device
                .end_command_buffer(self.graphics_command_buffers[swapchain_sync_idx])
                .unwrap();
            let cmd_buf_graphics_complete_infos = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.graphics_complete_semaphores[image_index as usize])];
            let graphics_submit = vk::SubmitInfo2::default()
                .command_buffer_infos(&cmd_buf_graphics_info)
                .wait_semaphore_infos(&cmd_buf_graphics_wait_infos)
                .signal_semaphore_infos(&cmd_buf_graphics_complete_infos);
            self.device
                .queue_submit2(
                    self.graphics_queue,
                    &[graphics_submit],
                    self.render_fences[swapchain_sync_idx],
                )
                .unwrap();

            let swapchains = [self.swapchain];
            let image_indices = [image_index];
            let present_wait_semaphores = [self.graphics_complete_semaphores[image_index as usize]];

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
        extradata: &[u8],
        session: vk::VideoSessionKHR,
    ) -> vk::VideoSessionParametersKHR {
        let std_sps = crate::vulkan::decoders::h264_parser::parse_sps(extradata)
            .expect("Failed to parse SPS");

        let std_pps = crate::vulkan::decoders::h264_parser::parse_pps(extradata)
            .expect("Failed to parse PPS");
        log::info!(
            "Resolution: {}x{}",
            (std_sps.pic_width_in_mbs_minus1 + 1) * 16,
            (std_sps.pic_height_in_map_units_minus1 + 1) * 16
        );
        log::info!(
            "log2_max_pic_order_cnt_lsb_minus4: {}",
            std_sps.log2_max_pic_order_cnt_lsb_minus4
        );
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
