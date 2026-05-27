use crate::video::converter::avcc_to_annexb;
use crate::vulkan::decoder::Decoder;
use crate::vulkan::decoders::h264::H264Decoder;
use crate::vulkan::vk_init::Aura;
use ffmpeg_next as ffmpeg;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

pub struct VideoContext {
    ictx: ffmpeg::format::context::Input,
    video_stream_index: usize,
    extradata: Vec<u8>,
    is_first_frame: bool,
}

#[derive(Default)]
pub struct App {
    window: Option<Window>,
    pub aura: Option<Aura>,
    video_ctx: Option<VideoContext>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window_attributes = Window::default_attributes()
                .with_title("Aura")
                .with_inner_size(winit::dpi::LogicalSize::new(1920.0, 1080.0));

            let window = event_loop.create_window(window_attributes).unwrap();

            let video_path = "test_h264.mp4";
            let ictx = ffmpeg::format::input(&video_path).unwrap();
            let input_stream = ictx.streams().best(ffmpeg::media::Type::Video).unwrap();
            let video_stream_index = input_stream.index();
            let params = input_stream.parameters();

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
            let aura = Aura::new(&window, &extradata);

            log::info!(
                "Vídeo file successfuly loaded. Extradata size: {}",
                extradata.len()
            );
            self.window = Some(window);
            self.aura = Some(aura);
            self.video_ctx = Some(VideoContext {
                ictx,
                video_stream_index,
                extradata,
                is_first_frame: true,
            });

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
                                let conversion = if v_ctx.is_first_frame {
                                    avcc_to_annexb(data, &v_ctx.extradata)
                                } else {
                                    avcc_to_annexb(data, &[])
                                };

                                match conversion {
                                    Ok((annexb, slice_offsets)) => {
                                        aura.decode_frame(
                                            &annexb,
                                            &slice_offsets,
                                            v_ctx.is_first_frame,
                                        );
                                        if v_ctx.is_first_frame {
                                            v_ctx.is_first_frame = false;
                                        }
                                    }
                                    Err(err) => {
                                        log::error!("Skip frame due to a parser error: {}", err);
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
