use ffmpeg_next as ffmpeg;
mod video;
mod vulkan;
use anyhow::Result;
use clap::Parser;
use winit::event_loop::EventLoop;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    file_name: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    ffmpeg::init()?;
    pretty_env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut app = vulkan::wsi::wsi::App::new(args.file_name);
    let () = event_loop.run_app(&mut app)?;
    Ok(())
}
