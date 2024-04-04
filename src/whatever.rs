use snafu::{Backtrace, Snafu};

// Just like `snafu::Whatever` but `+ Send + Sync`
#[derive(Debug, Snafu)]
#[snafu(whatever)]
#[snafu(display("{message:?}"))]
#[snafu(provide(opt, ref, chain, dyn std::error::Error + Send + Sync => source.as_deref()))]
pub struct Whatever {
    #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, Some)))]
    #[snafu(provide(false))]
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
    message: String,
    backtrace: Backtrace,
}
