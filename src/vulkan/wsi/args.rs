use clap::Parser;
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The video file you want to watch
    #[arg(short, long)]
    pub file_name: String,

    /// The video runs at the decodification speed
    #[arg(short, long)]
    pub no_sync: bool,

    /// Fixed interval between frames in millis (zero means none)
    #[arg(short, long, default_value_t = 0)]
    pub custom_clock_interval: u64,
}
