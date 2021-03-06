use crate::dispatcher::DownloadDispatcher;
use crate::downloader::tiktok::TikTokDownloader;
use crate::downloader::youtube::YoutubeDownloader;
use futures::{StreamExt, TryStreamExt};
use std::sync::Arc;
use std::time::Duration;
use teloxide::requests::RequesterExt;
use teloxide::Bot;
use tokio::io::AsyncWriteExt;
use tokio::pin;
use tracing::debug;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

mod bot;
mod dispatcher;
mod downloader;
#[allow(unused)]
mod remuxer;

fn init() {
    let filter = tracing_subscriber::EnvFilter::from_default_env();
    let fmt_layer = tracing_subscriber::fmt::layer()
        .event_format(tracing_subscriber::fmt::format().compact())
        .with_filter(filter);

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(console_subscriber::spawn())
        .init();

    debug!("Logging initialized!");

    ffmpeg_next::init().expect("Initializing ffmpeg");
}

#[tracing::instrument]
async fn remux_example() -> anyhow::Result<()> {
    let client = reqwest::ClientBuilder::new()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/92.0.4515.115 Safari/537.36")
        .build()?;

    let video_url = "https://rr1---sn-xguxaxjvh-8v1e.googlevideo.com/videoplayback?expire=1656138395&ei=O1a2YumEDJqn1gKWi5noCw&ip=5.3.162.204&id=o-AKV_SXG8stglh-ay6GcLwt2g8_n9Z_-0P2ig_3ePOUYG&itag=136&source=youtube&requiressl=yes&mh=z8&mm=31%2C29&mn=sn-xguxaxjvh-8v1e%2Csn-n8v7znsy&ms=au%2Crdu&mv=m&mvi=1&pl=22&initcwndbps=1536250&vprv=1&mime=video%2Fmp4&gir=yes&clen=18685498&dur=90.399&lmt=1579533885693773&mt=1656116466&fvip=2&keepalive=yes&fexp=24001373%2C24007246&c=ANDROID&txp=1316222&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cvprv%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRQIgJsWH0s9_cu8urBc_MplZ5UfpfWPmYthsixfGhEDW4i8CIQC76GXuws_QC4ENSzifnZIxgeZkPnAV1R5RDiaY1NMW4Q%3D%3D&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRQIgHBITziGocRprHOU4WOdDuFntewoCYUjU507HBeAuM_kCIQDlj5wFwNyXv3hUPaHcWbp5Dy1hVaaRF9MrCoYk6he5Sg%3D%3D";
    let audio_url = "https://rr1---sn-xguxaxjvh-8v1e.googlevideo.com/videoplayback?expire=1656138415&ei=T1a2YrSLDryix_APura1QA&ip=5.3.162.204&id=o-AIF1QbedyHpYsWPdxc5OQ6KzOaDtTx8XGe4lT5HJAx3z&itag=140&source=youtube&requiressl=yes&mh=z8&mm=31%2C29&mn=sn-xguxaxjvh-8v1e%2Csn-n8v7znsy&ms=au%2Crdu&mv=m&mvi=1&pl=22&initcwndbps=1536250&vprv=1&mime=audio%2Fmp4&gir=yes&clen=1464847&dur=90.464&lmt=1579533878395083&mt=1656116466&fvip=2&keepalive=yes&fexp=24001373%2C24007246&c=ANDROID&txp=1311222&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cvprv%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRgIhAIBPs4twuW1b7wQLDUWNl61A5WiNz5QOAgJsDLbc7mtcAiEAhx-bQvRG15Lv1gHe_la_uYNdcTlN_qZBZe6UMhAiSUE%3D&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRgIhAObaFfKag-QO7_55hp-r0n1qz20j-3csyCXSob30wuSOAiEAiV95Ies8k7ykXo0UozH1hv_-NRJH3oMg9D4LoQ-Uz5M%3D";

    let video_resp = client.execute(client.get(video_url).build()?).await?;
    let audio_resp = client.execute(client.get(audio_url).build()?).await?;

    let video_stream = video_resp.bytes_stream().map_err(anyhow::Error::new);

    let audio_stream = audio_resp.bytes_stream().map_err(anyhow::Error::new);

    let mut output = tokio::fs::File::create("output.mp4").await?;

    println!("Starting the remux...");

    let out_stream = remuxer::remux(video_stream, audio_stream).await?;

    pin!(out_stream);
    while let Some(b) = out_stream.next().await {
        let b = b?;

        output.write_all(b.as_ref()).await?;
    }

    println!("DONE!!");

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    init();

    let dispatcher = DownloadDispatcher::new(vec![
        Arc::new(YoutubeDownloader::new()),
        Arc::new(TikTokDownloader::new()),
    ]);
    let dispatcher = Arc::new(dispatcher);

    let client = teloxide::net::default_reqwest_settings()
        .timeout(Duration::from_secs(120)) // TODO: this is a "feature" of teloxide unfortunately =(
        // See https://github.com/teloxide/teloxide/issues/529
        .build()?;
    let bot = Bot::from_env_with_client(client).auto_send();
    bot::run_bot(bot, dispatcher).await;

    // remux_example().await?;

    Ok(())
}
