use tracing_subscriber::{EnvFilter, fmt};

pub fn init(verbose: bool) {
    let default = if verbose {
        "jc=debug,jc_core=debug"
    } else {
        "jc=warn"
    };
    let filter = EnvFilter::try_from_env("JC_LOG").unwrap_or_else(|_| EnvFilter::new(default));
    fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .init();
}
