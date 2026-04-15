use std::process::ExitCode;

mod cli;
mod commands;
mod config;
mod logging;
mod markdown_images;
mod markdown_mentions;
mod output;
mod preview;
mod sanitize;

#[tokio::main]
async fn main() -> ExitCode {
    let args = <cli::Cli as clap::Parser>::parse();
    logging::init(args.verbose);
    match commands::dispatch(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            output::emit_error(&e);
            e.exit_code()
        }
    }
}
