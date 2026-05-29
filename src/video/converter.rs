pub fn avcc_to_annexb(
    data: &[u8],
    nalu_length_size: usize,
) -> Result<(Vec<u8>, Vec<u32>), &'static str> {
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut slice_offsets = Vec::new();
    let mut offset = 0;

    while offset + nalu_length_size <= data.len() {
        let mut nal_size: usize = 0;
        for k in 0..nalu_length_size {
            nal_size = (nal_size << 8) | data[offset + k] as usize;
        }

        let start_data = offset + nalu_length_size;
        let end_data = start_data + nal_size;

        if end_data > data.len() {
            return Err("Bad avcc packet. NALU's size is bigger than data.");
        }

        if nal_size == 0 {
            offset = end_data;
            continue;
        }

        let nalu_type = data[start_data] & 0x1F;

        if nalu_type >= 1 && nalu_type <= 5 {
            slice_offsets.push(out.len() as u32);

            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(&data[start_data..end_data]);
        } else {
            log::debug!("Non-VCL NALU detected. Type: {}", nalu_type);
        }

        offset = end_data;
    }

    if slice_offsets.is_empty() {
        return Err("No video slices (VCL) found in this packet.");
    }

    Ok((out, slice_offsets))
}

#[derive(Debug)]
pub struct NaluHeader {
    pub forbidden_zero_bit: u8,
    pub nal_ref_idc: u8,
    pub nal_unit_type: u8,
    pub slice_header_offset: usize,
}

impl NaluHeader {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        let mut nalu_start = 0;
        if data.len() >= 4 && data[0] == 0 && data[1] == 0 && data[2] == 0 && data[3] == 1 {
            nalu_start = 4;
        } else if data.len() >= 3 && data[0] == 0 && data[1] == 0 && data[2] == 1 {
            nalu_start = 3;
        }

        if nalu_start >= data.len() {
            return None;
        }

        let nalu_byte = data[nalu_start];
        let forbidden_zero_bit = (nalu_byte >> 7) & 0x01;
        let nal_ref_idc = (nalu_byte >> 5) & 0x03;
        let nal_unit_type = nalu_byte & 0x1F;

        Some(Self {
            forbidden_zero_bit,
            nal_ref_idc,
            nal_unit_type,
            slice_header_offset: nalu_start + 1,
        })
    }
}
pub struct SpsInfo {
    pub log2_max_frame_num_minus4: u8,
    pub frame_mbs_only_flag: bool,
    pub pic_order_cnt_type: u8,
    pub log2_max_pic_order_cnt_lsb_minus4: u8,
}

#[derive(Debug)]
pub struct SliceHeader {
    pub first_mb_in_slice: u32,
    pub slice_type: u32,
    pub pic_parameter_set_id: u32,
    pub frame_num: u16,
    pub pic_order_cnt_lsb: u32,
}

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
    zero_bytes: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
            zero_bytes: 0,
        }
    }

    fn read_bit(&mut self) -> Option<u8> {
        if self.byte_pos >= self.data.len() {
            return None;
        }
        if self.bit_pos == 0 {
            if self.zero_bytes == 2 && self.data[self.byte_pos] == 0x03 {
                self.byte_pos += 1;
                self.zero_bytes = 0;
                if self.byte_pos >= self.data.len() {
                    return None;
                }
            }

            if self.data[self.byte_pos] == 0x00 {
                self.zero_bytes += 1;
            } else {
                self.zero_bytes = 0;
            }
        }

        let bit = (self.data[self.byte_pos] >> (7 - self.bit_pos)) & 1;

        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }

        Some(bit)
    }

    fn read_u(&mut self, n: usize) -> Option<u32> {
        let mut val = 0;
        for _ in 0..n {
            val = (val << 1) | (self.read_bit()? as u32);
        }
        Some(val)
    }
    fn read_ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.read_bit()? == 0 {
            zeros += 1;
        }
        if zeros == 0 {
            return Some(0);
        }
        let val = self.read_u(zeros)?;
        Some((1 << zeros) - 1 + val)
    }
}

pub fn parse_slice_header(
    slice_data: &[u8],
    nal_unit_type: u8,
    sps: &SpsInfo,
) -> Option<SliceHeader> {
    let mut br = BitReader::new(slice_data);

    let first_mb_in_slice = br.read_ue()?;
    let slice_type = br.read_ue()?;
    let pic_parameter_set_id = br.read_ue()?;

    let frame_num_len = (sps.log2_max_frame_num_minus4 + 4) as usize;
    let frame_num = br.read_u(frame_num_len)? as u16;

    if !sps.frame_mbs_only_flag {
        let field_pic_flag = br.read_u(1)?;
        if field_pic_flag == 1 {
            let _bottom_field_flag = br.read_u(1)?;
        }
    }

    if nal_unit_type == 5 {
        let _idr_pic_id = br.read_ue()?;
    }

    let mut pic_order_cnt_lsb = 0;

    if sps.pic_order_cnt_type == 0 {
        let poc_len = (sps.log2_max_pic_order_cnt_lsb_minus4 + 4) as usize;
        pic_order_cnt_lsb = br.read_u(poc_len)?;
    }

    Some(SliceHeader {
        first_mb_in_slice,
        slice_type,
        pic_parameter_set_id,
        frame_num,
        pic_order_cnt_lsb,
    })
}
