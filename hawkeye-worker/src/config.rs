use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "video-slate-detector",
    about = "Detects slate image and triggers URL request."
)]
pub struct AppConfig {
    // Path to the watcher configuration
    #[structopt(parse(from_os_str))]
    pub watcher_path: PathBuf,
}
