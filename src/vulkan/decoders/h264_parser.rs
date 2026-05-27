use ash::vk;
use std::mem::MaybeUninit;

fn unescape_rbsp(data: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if i + 2 < data.len() && data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x03 {
            rbsp.push(0x00);
            rbsp.push(0x00);
            i += 3;
        } else {
            rbsp.push(data[i]);
            i += 1;
        }
    }
    rbsp
}

struct BitReader {
    bytes: Vec<u8>,
    byte_idx: usize,
    bit_idx: usize,
}

impl BitReader {
    fn new(data: &[u8]) -> Self {
        Self {
            bytes: unescape_rbsp(data),
            byte_idx: 0,
            bit_idx: 0,
        }
    }

    fn read_bit(&mut self) -> Option<u32> {
        if self.byte_idx >= self.bytes.len() {
            return None;
        }
        let bit = (self.bytes[self.byte_idx] >> (7 - self.bit_idx)) & 1;
        self.bit_idx += 1;
        if self.bit_idx == 8 {
            self.bit_idx = 0;
            self.byte_idx += 1;
        }
        Some(bit as u32)
    }

    fn read_bits(&mut self, n: usize) -> Option<u32> {
        let mut res = 0;
        for _ in 0..n {
            res = (res << 1) | self.read_bit()?;
        }
        Some(res)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zeros = 0;
        while self.read_bit()? == 0 {
            leading_zeros += 1;
        }
        if leading_zeros == 0 {
            return Some(0);
        }
        let val = self.read_bits(leading_zeros)?;
        Some((1 << leading_zeros) - 1 + val)
    }

    fn read_se(&mut self) -> Option<i32> {
        let ue = self.read_ue()?;
        if ue % 2 == 0 {
            Some(-(ue as i32 / 2))
        } else {
            Some((ue as i32 + 1) / 2)
        }
    }

    fn has_more_rbsp_data(&self) -> bool {
        let mut last_one_bit_offset = None;
        for byte_idx in (self.byte_idx..self.bytes.len()).rev() {
            let byte = self.bytes[byte_idx];
            if byte != 0 {
                let trailing_zeros = byte.trailing_zeros();
                last_one_bit_offset = Some(byte_idx * 8 + (7 - trailing_zeros as usize));
                break;
            }
        }
        
        if let Some(last_one_pos) = last_one_bit_offset {
            let current_pos = self.byte_idx * 8 + self.bit_idx;
            current_pos < last_one_pos
        } else {
            false
        }
    }
}

pub fn extract_sps_bytes(extradata: &[u8]) -> Option<&[u8]> {
    if extradata.len() < 8 { return None; }
    let num_sps = extradata[5] & 0x1F;
    if num_sps == 0 { return None; }
    let sps_len = u16::from_be_bytes([extradata[6], extradata[7]]) as usize;
    if extradata.len() < 8 + sps_len { return None; }
    Some(&extradata[8..8 + sps_len])
}

pub fn extract_pps_bytes(extradata: &[u8]) -> Option<&[u8]> {
    if extradata.len() < 8 { return None; }
    let num_sps = (extradata[5] & 0x1F) as usize;
    
    let mut offset = 6;
    for _ in 0..num_sps {
        if offset + 2 > extradata.len() { return None; }
        let sps_len = u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
        offset += 2 + sps_len;
    }
    
    if offset >= extradata.len() { return None; }
    let num_pps = extradata[offset] as usize;
    if num_pps == 0 { return None; }
    
    offset += 1;
    if offset + 2 > extradata.len() { return None; }
    let pps_len = u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
    offset += 2;
    
    if offset + pps_len > extradata.len() { return None; }
    Some(&extradata[offset..offset + pps_len])
}

pub fn parse_sps(extradata: &[u8]) -> Option<vk::native::StdVideoH264SequenceParameterSet> {
    let raw_sps = extract_sps_bytes(extradata)?;
    let mut reader = BitReader::new(raw_sps);

    reader.read_bits(8)?; // NAL Header

    let profile_idc = reader.read_bits(8)?;
    let _constraint_flags = reader.read_bits(8)?;
    let level_idc = reader.read_bits(8)?;
    let sps_id = reader.read_ue()?;

    let mut chroma_format_idc = 1; // Default to 4:2:0 for baseline/main
    let mut separate_colour_plane_flag = 0;
    let mut bit_depth_luma_minus8 = 0;
    let mut bit_depth_chroma_minus8 = 0;
    let mut qpprime_y_zero_transform_bypass_flag = 0;
    let mut seq_scaling_matrix_present_flag = 0;

    let is_high_profile = matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    );

    if is_high_profile {
        chroma_format_idc = reader.read_ue()?;
        if chroma_format_idc == 3 {
            separate_colour_plane_flag = reader.read_bit()?;
        }
        bit_depth_luma_minus8 = reader.read_ue()?;
        bit_depth_chroma_minus8 = reader.read_ue()?;
        qpprime_y_zero_transform_bypass_flag = reader.read_bit()?;
        seq_scaling_matrix_present_flag = reader.read_bit()?;
        if seq_scaling_matrix_present_flag == 1 {
            let limit = if chroma_format_idc != 3 { 8 } else { 12 };
            for i in 0..limit {
                if reader.read_bit()? == 1 {
                    let size = if i < 6 { 16 } else { 64 };
                    let mut next_scale = 8;
                    let mut last_scale = 8;
                    for _ in 0..size {
                        if next_scale != 0 {
                            let delta_scale = reader.read_se()?;
                            next_scale = (last_scale + delta_scale + 256) % 256;
                        }
                        last_scale = if next_scale == 0 { last_scale } else { next_scale };
                    }
                }
            }
        }
    }

    let log2_max_frame_num_minus4 = reader.read_ue()?;
    let pic_order_cnt_type = reader.read_ue()?;
    
    let mut log2_max_pic_order_cnt_lsb_minus4 = 0;
    let mut delta_pic_order_always_zero_flag = 0;
    let mut offset_for_non_ref_pic = 0;
    let mut offset_for_top_to_bottom_field = 0;
    let mut num_ref_frames_in_pic_order_cnt_cycle = 0;

    if pic_order_cnt_type == 0 {
        log2_max_pic_order_cnt_lsb_minus4 = reader.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        delta_pic_order_always_zero_flag = reader.read_bit()?;
        offset_for_non_ref_pic = reader.read_se()?;
        offset_for_top_to_bottom_field = reader.read_se()?;
        num_ref_frames_in_pic_order_cnt_cycle = reader.read_ue()?;
        for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
            reader.read_se()?; 
            // Note: Vulkan uses a `pOffsetForRefFrame` pointer if this loop executes. 
            // Leaving it null is fine if count is 0, but you will need an allocation otherwise.
        }
    }

    let max_num_ref_frames = reader.read_ue()?;
    let gaps_in_frame_num_value_allowed_flag = reader.read_bit()?;
    let pic_width_in_mbs_minus1 = reader.read_ue()?;
    let pic_height_in_map_units_minus1 = reader.read_ue()?;
    let frame_mbs_only_flag = reader.read_bit()?;
    
    let mut mb_adaptive_frame_field_flag = 0;
    if frame_mbs_only_flag == 0 {
        mb_adaptive_frame_field_flag = reader.read_bit()?;
    }
    
    let direct_8x8_inference_flag = reader.read_bit()?;
    let frame_cropping_flag = reader.read_bit()?;
    
    let mut frame_crop_left_offset = 0;
    let mut frame_crop_right_offset = 0;
    let mut frame_crop_top_offset = 0;
    let mut frame_crop_bottom_offset = 0;

    if frame_cropping_flag == 1 {
        frame_crop_left_offset = reader.read_ue()?;
        frame_crop_right_offset = reader.read_ue()?;
        frame_crop_top_offset = reader.read_ue()?;
        frame_crop_bottom_offset = reader.read_ue()?;
    }

    let vui_parameters_present_flag = reader.read_bit().unwrap_or(0);

    let mut sps_flags: vk::native::StdVideoH264SpsFlags = unsafe { MaybeUninit::zeroed().assume_init() };
    sps_flags.set_direct_8x8_inference_flag(direct_8x8_inference_flag);
    sps_flags.set_mb_adaptive_frame_field_flag(mb_adaptive_frame_field_flag);
    sps_flags.set_frame_mbs_only_flag(frame_mbs_only_flag);
    sps_flags.set_delta_pic_order_always_zero_flag(delta_pic_order_always_zero_flag);
    sps_flags.set_separate_colour_plane_flag(separate_colour_plane_flag);
    sps_flags.set_gaps_in_frame_num_value_allowed_flag(gaps_in_frame_num_value_allowed_flag);
    sps_flags.set_qpprime_y_zero_transform_bypass_flag(qpprime_y_zero_transform_bypass_flag);
    sps_flags.set_frame_cropping_flag(frame_cropping_flag);
    sps_flags.set_seq_scaling_matrix_present_flag(seq_scaling_matrix_present_flag);
    sps_flags.set_vui_parameters_present_flag(vui_parameters_present_flag);

    let mut std_sps: vk::native::StdVideoH264SequenceParameterSet = unsafe { MaybeUninit::zeroed().assume_init() };
    std_sps.flags = sps_flags;
    
    std_sps.profile_idc = match profile_idc {
        66 => vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_BASELINE,
        77 => vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN,
        100 => vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_HIGH,
        244 => vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_HIGH_444_PREDICTIVE,
        _ => vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_INVALID,
    };
    
    std_sps.level_idc = match level_idc {
        10 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_0,
        11 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_1,
        12 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_2,
        13 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_3,
        20 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_2_0,
        21 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_2_1,
        22 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_2_2,
        30 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_3_0,
        31 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_3_1,
        32 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_3_2,
        40 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_0,
        41 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_1,
        42 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_2,
        50 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_0,
        51 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_1,
        52 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_2,
        60 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_0,
        61 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_1,
        62 => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_2,
        _ => vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_INVALID,
    };
    
    std_sps.chroma_format_idc = match chroma_format_idc {
        0 => vk::native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_MONOCHROME,
        1 => vk::native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_420,
        2 => vk::native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_422,
        3 => vk::native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_444,
        _ => vk::native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_INVALID,
    };

    std_sps.pic_order_cnt_type = match pic_order_cnt_type {
        0 => vk::native::StdVideoH264PocType_STD_VIDEO_H264_POC_TYPE_0,
        1 => vk::native::StdVideoH264PocType_STD_VIDEO_H264_POC_TYPE_1,
        2 => vk::native::StdVideoH264PocType_STD_VIDEO_H264_POC_TYPE_2,
        _ => vk::native::StdVideoH264PocType_STD_VIDEO_H264_POC_TYPE_INVALID,
    };

    std_sps.seq_parameter_set_id = sps_id as u8;
    std_sps.bit_depth_luma_minus8 = bit_depth_luma_minus8 as u8;
    std_sps.bit_depth_chroma_minus8 = bit_depth_chroma_minus8 as u8;
    std_sps.log2_max_frame_num_minus4 = log2_max_frame_num_minus4 as u8;
    std_sps.log2_max_pic_order_cnt_lsb_minus4 = log2_max_pic_order_cnt_lsb_minus4 as u8;
    std_sps.offset_for_non_ref_pic = offset_for_non_ref_pic;
    std_sps.offset_for_top_to_bottom_field = offset_for_top_to_bottom_field;
    std_sps.num_ref_frames_in_pic_order_cnt_cycle = num_ref_frames_in_pic_order_cnt_cycle as u8;
    std_sps.max_num_ref_frames = max_num_ref_frames as u8;
    std_sps.pic_width_in_mbs_minus1 = pic_width_in_mbs_minus1;
    std_sps.pic_height_in_map_units_minus1 = pic_height_in_map_units_minus1;
    std_sps.frame_crop_left_offset = frame_crop_left_offset;
    std_sps.frame_crop_right_offset = frame_crop_right_offset;
    std_sps.frame_crop_top_offset = frame_crop_top_offset;
    std_sps.frame_crop_bottom_offset = frame_crop_bottom_offset;

    Some(std_sps)
}

pub fn parse_pps(extradata: &[u8]) -> Option<vk::native::StdVideoH264PictureParameterSet> {
    let raw_pps = extract_pps_bytes(extradata)?;
    let mut reader = BitReader::new(raw_pps);

    reader.read_bits(8)?; // NAL Header

    let pic_parameter_set_id = reader.read_ue()?;
    let seq_parameter_set_id = reader.read_ue()?;
    let entropy_coding_mode_flag = reader.read_bit()?;
    let bottom_field_pic_order_in_frame_present_flag = reader.read_bit()?;
    
    let num_slice_groups_minus1 = reader.read_ue()?;
    if num_slice_groups_minus1 > 0 {
        let slice_group_map_type = reader.read_ue()?;
        if slice_group_map_type == 0 {
            for _ in 0..=num_slice_groups_minus1 { reader.read_ue()?; }
        } else if slice_group_map_type == 2 {
            for _ in 0..num_slice_groups_minus1 { reader.read_ue()?; reader.read_ue()?; }
        } else if slice_group_map_type == 3 || slice_group_map_type == 4 || slice_group_map_type == 5 {
            reader.read_bit()?; reader.read_ue()?;
        } else if slice_group_map_type == 6 {
            let pic_size_in_map_units_minus1 = reader.read_ue()?;
            let mut bits = 0;
            let mut val = num_slice_groups_minus1;
            while val > 0 { bits += 1; val >>= 1; }
            if bits == 0 { bits = 1; }
            for _ in 0..=pic_size_in_map_units_minus1 { reader.read_bits(bits as usize)?; }
        }
    }

    let num_ref_idx_l0_default_active_minus1 = reader.read_ue()?;
    let num_ref_idx_l1_default_active_minus1 = reader.read_ue()?;
    let weighted_pred_flag = reader.read_bit()?;
    let weighted_bipred_idc = reader.read_bits(2)?;
    let pic_init_qp_minus26 = reader.read_se()?;
    let pic_init_qs_minus26 = reader.read_se()?;
    let chroma_qp_index_offset = reader.read_se()?;
    let deblocking_filter_control_present_flag = reader.read_bit()?;
    let constrained_intra_pred_flag = reader.read_bit()?;
    let redundant_pic_cnt_present_flag = reader.read_bit()?;

    let mut transform_8x8_mode_flag = 0;
    let mut pic_scaling_matrix_present_flag = 0;
    let mut second_chroma_qp_index_offset = chroma_qp_index_offset;

    if reader.has_more_rbsp_data() {
        transform_8x8_mode_flag = reader.read_bit()?;
        pic_scaling_matrix_present_flag = reader.read_bit()?;
        
        if !(pic_scaling_matrix_present_flag == 1) {
            second_chroma_qp_index_offset = reader.read_se().unwrap_or(chroma_qp_index_offset);
        }
    }

    let mut pps_flags: vk::native::StdVideoH264PpsFlags = unsafe { MaybeUninit::zeroed().assume_init() };
    pps_flags.set_entropy_coding_mode_flag(entropy_coding_mode_flag);
    pps_flags.set_bottom_field_pic_order_in_frame_present_flag(bottom_field_pic_order_in_frame_present_flag);
    pps_flags.set_weighted_pred_flag(weighted_pred_flag);
    pps_flags.set_deblocking_filter_control_present_flag(deblocking_filter_control_present_flag);
    pps_flags.set_constrained_intra_pred_flag(constrained_intra_pred_flag);
    pps_flags.set_redundant_pic_cnt_present_flag(redundant_pic_cnt_present_flag);
    pps_flags.set_transform_8x8_mode_flag(transform_8x8_mode_flag);
    pps_flags.set_pic_scaling_matrix_present_flag(pic_scaling_matrix_present_flag);

    let mut std_pps: vk::native::StdVideoH264PictureParameterSet = unsafe { MaybeUninit::zeroed().assume_init() };
    std_pps.flags = pps_flags;
    std_pps.pic_parameter_set_id = pic_parameter_set_id as u8;
    std_pps.seq_parameter_set_id = seq_parameter_set_id as u8;
    std_pps.num_ref_idx_l0_default_active_minus1 = num_ref_idx_l0_default_active_minus1 as u8;
    std_pps.num_ref_idx_l1_default_active_minus1 = num_ref_idx_l1_default_active_minus1 as u8;
    
    std_pps.weighted_bipred_idc = match weighted_bipred_idc {
        0 => vk::native::StdVideoH264WeightedBipredIdc_STD_VIDEO_H264_WEIGHTED_BIPRED_IDC_DEFAULT,
        1 => vk::native::StdVideoH264WeightedBipredIdc_STD_VIDEO_H264_WEIGHTED_BIPRED_IDC_EXPLICIT,
        2 => vk::native::StdVideoH264WeightedBipredIdc_STD_VIDEO_H264_WEIGHTED_BIPRED_IDC_IMPLICIT,
        _ => vk::native::StdVideoH264WeightedBipredIdc_STD_VIDEO_H264_WEIGHTED_BIPRED_IDC_INVALID,
    };
    
    std_pps.pic_init_qp_minus26 = pic_init_qp_minus26 as i8;
    std_pps.pic_init_qs_minus26 = pic_init_qs_minus26 as i8;
    std_pps.chroma_qp_index_offset = chroma_qp_index_offset as i8;
    std_pps.second_chroma_qp_index_offset = second_chroma_qp_index_offset as i8;

    Some(std_pps)
}