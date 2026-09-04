use clap::Parser;
use ffmpeg_next as ffmpeg;
mod video;
mod vulkan;
use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    let args = vulkan::wsi::args::Args::parse();
    ffmpeg::init()?;
    pretty_env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut app = vulkan::wsi::wsi::App::new(args);
    let () = event_loop.run_app(&mut app)?;
    Ok(())
}
