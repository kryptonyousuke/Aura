use crate::video::converter::avcc_to_annexb;
use crate::video::video_clock::VideoClock;
use crate::video::video_context::VideoContext;
use crate::vulkan::photon::h264::H264Decoder;
use crate::vulkan::vk_init::Aura;
use ffmpeg_next as ffmpeg;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

#[derive(Default)]
pub struct App {
    file_name: String,
    window: Option<Window>,
    pub aura: Option<Aura>,
    video_ctx: Option<VideoContext>,
}

impl App {
    pub fn new(file_name: String) -> Self {
        Self {
            file_name,
            window: None,
            aura: None,
            video_ctx: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window_attributes = Window::default_attributes()
                .with_title("Aura")
                .with_inner_size(winit::dpi::LogicalSize::new(1920.0, 1080.0));

            let window = event_loop.create_window(window_attributes).unwrap();
            let video_path = self.file_name.clone();
            let ictx = ffmpeg::format::input(&video_path).unwrap();
            let input_stream = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
            let video_stream_index = input_stream.index();
            let params = input_stream.parameters();
            let rational_tb = input_stream.time_base();
            let time_base_f64 = rational_tb.numerator() as f64 / rational_tb.denominator() as f64;
            let clock = VideoClock::new(time_base_f64);
            let extradata = unsafe {
                let raw_params = params.as_ptr();
                if (*raw_params).extradata.is_null() {
                    vec![]
                } else {
                    std::slice::from_raw_parts(
                        (*raw_params).extradata,
                        (*raw_params).extradata_size as usize,
                    )
                    .to_vec()
                }
            };
            let nalu_length_size = if extradata.len() > 4 {
                ((extradata[4] & 0x03) + 1) as usize
            } else {
                4 // Fallback
            };
            log::info!(
                "Vídeo file successfuly loaded. Extradata size: {}",
                extradata.len()
            );
            let v_ctx = VideoContext {
                ictx: ictx,
                video_stream_index: video_stream_index,
                extradata: extradata.clone(),
                nalu_length_size: nalu_length_size,
                is_first_frame: true,
                clock: clock,
                params: params
            };

            
            let aura = Aura::new(&window, &extradata, Some(&v_ctx));
            self.video_ctx = Some(v_ctx);

            self.window = Some(window);
            self.aura = Some(aura);

            self.window.as_ref().unwrap().request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("The close button was pressed; stopping");

                if let Some(vulkan_ctx) = self.aura.take() {
                    std::mem::drop(vulkan_ctx);
                    log::info!("Vunkan Context dropped safely.");
                }

                self.video_ctx = None;
                self.window = None;
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let (Some(aura), Some(v_ctx)) = (&mut self.aura, &mut self.video_ctx) {
                    if let Some((stream, packet)) = v_ctx.ictx.packets().next() {
                        if stream.index() == v_ctx.video_stream_index {
                            if let Some(data) = packet.data() {
                                let conversion = avcc_to_annexb(data, v_ctx.nalu_length_size);

                                match conversion {
                                    Ok((annexb, slice_offsets)) => {
                                        if let Some(pts) = packet.pts().or_else(|| packet.dts()) {
                                            if let Some(wait_duration) =
                                                v_ctx.clock.time_till_next_frame(pts)
                                            {
                                                std::thread::sleep(wait_duration);
                                            }
                                        }
                                        let std_sps =
                                            crate::vulkan::photon::h264_parser::parse_sps(
                                                &v_ctx.extradata,
                                            )
                                            .expect("Failed to parse SPS");
                                        let _std_pps =
                                            crate::vulkan::photon::h264_parser::parse_pps(
                                                &v_ctx.extradata,
                                            )
                                            .expect("Failed to parse PPS");
                                        aura.decode_frame(
                                            &annexb,
                                            &slice_offsets,
                                            v_ctx.is_first_frame,
                                            &std_sps,
                                        );
                                        if v_ctx.is_first_frame {
                                            v_ctx.is_first_frame = false;
                                        }
                                    }
                                    Err(err) => {
                                        log::error!("Skip frame due to a parser error: {err}");
                                    }
                                }
                            }
                        }

                        if let Some(ref w) = self.window {
                            w.request_redraw();
                        }
                    } else {
                        log::info!("Reached the end of the file.");
                        if let Some(vulkan_ctx) = self.aura.take() {
                            std::mem::drop(vulkan_ctx);
                            log::info!("Vunkan Context dropped safely.");
                            event_loop.exit();
                        }
                    }
                }
            }
            _ => (),
        }
    }
}
