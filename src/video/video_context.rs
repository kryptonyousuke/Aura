use crate::video::video_clock::VideoClock;

pub struct VideoContext {
    pub ictx: ffmpeg_next::format::context::Input,
    pub video_stream_index: usize,
    pub extradata: Vec<u8>,
    pub nalu_length_size: usize,
    pub is_first_frame: bool,
    pub clock: VideoClock,
}