use rayon::prelude::*;
pub fn avcc_to_annexb(data: &[u8], extradata: &[u8]) -> Vec<u8> {
    let nalu_length_size = if extradata.len() > 4 {
        ((extradata[4] & 0x03) + 1) as usize
    } else {
        4 // Default fallback whenver got a weird extradata
    };

    let mut out = Vec::with_capacity(data.len() + extradata.len() + 32);

    if extradata.len() > 6 {
        let mut i = 5;

        if let Some(&sps_byte) = extradata.get(i) {
            let num_sps = sps_byte & 0x1f;
            i += 1;

            for _ in 0..num_sps {
                if i + 2 > extradata.len() {
                    log::error!("The file needs at least 2 bytes to read the size.");
                    std::process::exit(1);
                }
                let size = ((extradata[i] as usize) << 8) | extradata[i + 1] as usize;
                i += 2;

                if i + size > extradata.len() {
                    log::error!("The extracted size doesn't match the known vec size.");
                    std::process::exit(1);
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
                    log::error!("There's no space to read this PPS size.");
                    std::process::exit(1);
                }
                let size = ((extradata[i] as usize) << 8) | extradata[i + 1] as usize;
                i += 2;

                if i + size > extradata.len() {
                    log::error!("The PPS data doesn't exist in the vector.");
                    std::process::exit(1);
                }

                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(&extradata[i..i + size]);
                i += size;
            }
        }
    }

    // AVCC -> Annex-B
    let mut nalu_slices = Vec::new();
    let mut offset = 0;

    while offset + nalu_length_size <= data.len() {
        let mut nal_size: usize = 0;
        for k in 0..nalu_length_size {
            nal_size = (nal_size << 8) | data[offset + k] as usize;
        }
        let start_data = offset + nalu_length_size;
        let end_data = start_data + nal_size;

        if end_data > data.len() {
            log::error!("Malformed AVCC packet: NALU size extends beyond data limits.");
            std::process::exit(1);
        }

        nalu_slices.push(&data[start_data..end_data]);
        offset = end_data;
    }

    let converted_nalus: Vec<Vec<u8>> = nalu_slices
        .par_iter()
        .map(|nalu| {
            let mut chunk = Vec::with_capacity(4 + nalu.len());
            chunk.extend_from_slice(&[0, 0, 0, 1]);
            chunk.extend_from_slice(nalu);
            chunk
        })
        .collect();

    for nalu_vector in converted_nalus {
        out.extend_from_slice(&nalu_vector);
    }

    out
}
