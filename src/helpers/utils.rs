use log::LevelFilter;
use fern::{Dispatch, colors::{Color, ColoredLevelConfig}};
use eyre::Result;
use crate::constants::const_types::*;


pub fn setup_logger() -> Result<()> {
    let colors = ColoredLevelConfig {
        trace: Color::Cyan,
        debug: Color::Magenta,
        info: Color::Green,
        warn: Color::Red,
        error: Color::BrightRed,
        ..ColoredLevelConfig::new()
    };

    Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}",
                chrono::Local::now().format("[%H:%M:%S]"),
                colors.color(record.level()),
                message
            ))
        })
        .chain(std::io::stdout())
        .level(log::LevelFilter::Info)
        .level_for(PROJECT_NAME, LevelFilter::Info)
        .apply()?;

    Ok(())
}