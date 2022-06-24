use futures::TryStreamExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

mod bot;
mod dispatcher;
mod downloader;
mod remuxer;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    ffmpeg_next::init().expect("Initializing ffmpeg");

    let filter = tracing_subscriber::EnvFilter::from_default_env();
    let fmt_layer = tracing_subscriber::fmt::layer().with_filter(filter);

    tracing_subscriber::registry().with(fmt_layer).init();

    let client = reqwest::ClientBuilder::new()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/92.0.4515.115 Safari/537.36")
        .build()?;

    let video_url = "https://rr2---sn-xguxaxjvh-8v1e.googlevideo.com/videoplayback?expire=1656053141&ei=NQm1YsNKmLTWAruApYgL&ip=5.3.162.204&id=o-ADyeXYKBT74wtSF4OZFka8RqPjNZEzWHqE-AyKm-qUxn&itag=136&source=youtube&requiressl=yes&mh=Cs&mm=31%2C29&mn=sn-xguxaxjvh-8v1e%2Csn-n8v7knel&ms=au%2Crdu&mv=m&mvi=2&pl=24&initcwndbps=2011250&vprv=1&mime=video%2Fmp4&gir=yes&clen=2845820&dur=24.232&lmt=1508465802781437&mt=1656031019&fvip=17&keepalive=yes&fexp=24001373%2C24007246&c=ANDROID&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cvprv%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRAIgPcwfqqUHdU2vrcpR3xM9IzKwX0GYdzUwOzPHMNdjh9ACICPIhLU-T-Cel5sV8yrzH9zIzAQMQ4yt7kMX-f4U2a67&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRgIhAMa73PyAYKy_d-g9FS-FTJvyT20u4p4HoyYTpIDLB1gbAiEAw8D73D4eN9cTn368-oT1DWePbAlmPF3Q1GCuK_iJ5MA%3D";
    let audio_url = "https://rr2---sn-xguxaxjvh-8v1e.googlevideo.com/videoplayback?expire=1656053186&ei=Ygm1Ypi2ENac8gO-yajoBg&ip=5.3.162.204&id=o-AHeO8Zee_IXOtLOhLB1RLrI6-Tehp5D6PQrJaVcoZtr3&itag=140&source=youtube&requiressl=yes&mh=Cs&mm=31%2C29&mn=sn-xguxaxjvh-8v1e%2Csn-n8v7znly&ms=au%2Crdu&mv=m&mvi=2&pl=24&initcwndbps=1991250&vprv=1&mime=audio%2Fmp4&gir=yes&clen=386822&dur=24.288&lmt=1508465769284246&mt=1656031258&fvip=3&keepalive=yes&fexp=24001373%2C24007246&c=ANDROID&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cvprv%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRAIgL6F0Z87epkdXSSTwiBaTM-6ElorxlRnhGxWEnpWZjPsCIEo_YdJ6WgaiWXtL5qWrEVLx3WZXLZYJ9zglILMchEBj&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRAIgF39vsYtHJKkPnkX1LvBeUrCEC8OhH5dwhH_SUXFI6Q4CIAN3PyhLaL39KtcozQ_Q9AeVr24xq6TmsrEY_Eftz739";

    let video_resp = client.execute(client.get(video_url).build()?).await?;
    let audio_resp = client.execute(client.get(audio_url).build()?).await?;

    let video_stream = video_resp.bytes_stream().map_err(anyhow::Error::new);

    let audio_stream = audio_resp.bytes_stream().map_err(anyhow::Error::new);

    let mut output = tokio::fs::File::create("output.mp4").await?;

    println!("Starting the remux...");

    remuxer::remux(video_stream, audio_stream, &mut output).await?;

    println!("DONE!!");

    Ok(())
}
