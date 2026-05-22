pub fn avcc_to_annexb(data: &[u8], extradata: &[u8]) -> Result<Vec<u8>, &'static str> {
    let nalu_length_size = if extradata.len() > 4 {
        ((extradata[4] & 0x03) + 1) as usize
    } else {
        4 // Fallback
    };

    let mut out = Vec::with_capacity(data.len() + extradata.len() + 32);

    if extradata.len() > 6 {
        let mut i = 5;

        if let Some(&sps_byte) = extradata.get(i) {
            let num_sps = sps_byte & 0x1f;
            i += 1;

            for _ in 0..num_sps {
                if i + 2 > extradata.len() {
                    return Err("Extradata needs 2 bytes at least to read SPS.");
                }
                let size = ((extradata[i] as usize) << 8) | extradata[i + 1] as usize;
                i += 2;

                if i + size > extradata.len() {
                    return Err("SPS's size is bigger than extradata.");
                }

                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(&extradata[i..i + size]);
                i += size;
            }
        }

        if let Some(&pps_byte) = extradata.get(i) {
            let num_pps = pps_byte;
            i += 1;

            for _ in 0..num_pps {
                if i + 2 > extradata.len() {
                    return Err("There's no enough space to read the data in the vector.");
                }
                let size = ((extradata[i] as usize) << 8) | extradata[i + 1] as usize;
                i += 2;

                if i + size > extradata.len() {
                    return Err("PPS's size is bigger than the vector.");
                }

                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(&extradata[i..i + size]);
                i += size;
            }
        }
    }

    // AVCC -> ANNEX-B
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

        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[start_data..end_data]);

        offset = end_data;
    }

    Ok(out)
}
