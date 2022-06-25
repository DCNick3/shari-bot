use crate::bot::Notifier;
use crate::downloader::Downloader;
use crate::{remuxer, StreamExt, TryStreamExt};
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::Stream;
use once_cell::sync::Lazy;
use regex::Regex;
use std::io::ErrorKind;
use std::sync::Arc;

static YOUTUBE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?x)^
                     (
                         (?:https?://|//)?                                    # http(s):// or protocol-independent URL or no scheme
                         (?:(?:(?:(?:\w+\.)?[yY][oO][uU][tT][uU][bB][eE](?:-nocookie|kids)?\.com|
                            (?:www\.)?deturl\.com/www\.youtube\.com|
                            (?:www\.)?pwnyoutube\.com|
                            (?:www\.)?hooktube\.com|
                            (?:www\.)?yourepeat\.com|
                            tube\.majestyc\.net|
                            youtube\.googleapis\.com)/                        # the various hostnames, with wildcard subdomains
                         (?:.*?\#/)?                                          # handle anchor (#/) redirect urls
                         (?:                                                  # the various things that can precede the ID:
                             (?:(?:v|embed|e|shorts)/)                        # v/ or embed/ or e/ or shorts/
                             |(?:                                             # or the v= param in all its forms
                                 (?:(?:watch|movie)(?:_popup)?(?:\.php)?/?)?  # preceding watch(_popup|.php) or nothing (like /?v=xxxx)
                                 (?:\?|\#!?)                                  # the params delimiter ? or # or #!
                                 (?:.*?[&;])??                                # any other preceding param (like /?s=tuff&v=xxxx or ?s=tuff&amp;v=V36LpHqtcDY)
                                 v=
                             )
                         ))
                         |(?:
                            youtu\.be|                                        # just youtu.be/xxxx
                            vid\.plus|                                        # or vid.plus/xxxx
                            zwearz\.com/watch|                                # or zwearz.com/watch/xxxx
                            %(invidious)s
                         )/
                         |(?:www\.)?cleanvideosearch\.com/media/action/yt/watch\?videoId=
                         )
                     )                                                        # don't allow naked IDs!
                     (?P<id>[0-9A-Za-z_-]{11})                                # here is it! the YouTube video ID
                     (.+)?
                     (?:\#|$)"#).unwrap()
});

#[derive(Debug)]
pub struct YoutubeDownloader {}

#[async_trait]
impl Downloader for YoutubeDownloader {
    fn probe_url(&self, url: &str) -> bool {
        if let Some(c) = YOUTUBE_REGEX.captures(url) {
            if let Some(_id) = c.name("id") {
                return true;
            }
        }

        false
    }

    async fn download(
        self: Arc<Self>,
        url: String,
        notifier: Notifier,
    ) -> anyhow::Result<BoxStream<'static, futures::io::Result<Bytes>>> {
        let client = reqwest::ClientBuilder::new()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/92.0.4515.115 Safari/537.36")
            .build()?;

        let video_url = "https://rr1---sn-xguxaxjvh-8v1e.googlevideo.com/videoplayback?expire=1656138395&ei=O1a2YumEDJqn1gKWi5noCw&ip=5.3.162.204&id=o-AKV_SXG8stglh-ay6GcLwt2g8_n9Z_-0P2ig_3ePOUYG&itag=136&source=youtube&requiressl=yes&mh=z8&mm=31%2C29&mn=sn-xguxaxjvh-8v1e%2Csn-n8v7znsy&ms=au%2Crdu&mv=m&mvi=1&pl=22&initcwndbps=1536250&vprv=1&mime=video%2Fmp4&gir=yes&clen=18685498&dur=90.399&lmt=1579533885693773&mt=1656116466&fvip=2&keepalive=yes&fexp=24001373%2C24007246&c=ANDROID&txp=1316222&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cvprv%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRQIgJsWH0s9_cu8urBc_MplZ5UfpfWPmYthsixfGhEDW4i8CIQC76GXuws_QC4ENSzifnZIxgeZkPnAV1R5RDiaY1NMW4Q%3D%3D&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRQIgHBITziGocRprHOU4WOdDuFntewoCYUjU507HBeAuM_kCIQDlj5wFwNyXv3hUPaHcWbp5Dy1hVaaRF9MrCoYk6he5Sg%3D%3D";
        let audio_url = "https://rr1---sn-xguxaxjvh-8v1e.googlevideo.com/videoplayback?expire=1656138415&ei=T1a2YrSLDryix_APura1QA&ip=5.3.162.204&id=o-AIF1QbedyHpYsWPdxc5OQ6KzOaDtTx8XGe4lT5HJAx3z&itag=140&source=youtube&requiressl=yes&mh=z8&mm=31%2C29&mn=sn-xguxaxjvh-8v1e%2Csn-n8v7znsy&ms=au%2Crdu&mv=m&mvi=1&pl=22&initcwndbps=1536250&vprv=1&mime=audio%2Fmp4&gir=yes&clen=1464847&dur=90.464&lmt=1579533878395083&mt=1656116466&fvip=2&keepalive=yes&fexp=24001373%2C24007246&c=ANDROID&txp=1311222&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cvprv%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRgIhAIBPs4twuW1b7wQLDUWNl61A5WiNz5QOAgJsDLbc7mtcAiEAhx-bQvRG15Lv1gHe_la_uYNdcTlN_qZBZe6UMhAiSUE%3D&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRgIhAObaFfKag-QO7_55hp-r0n1qz20j-3csyCXSob30wuSOAiEAiV95Ies8k7ykXo0UozH1hv_-NRJH3oMg9D4LoQ-Uz5M%3D";

        let video_resp = client.execute(client.get(video_url).build()?).await?;
        let audio_resp = client.execute(client.get(audio_url).build()?).await?;

        let video_stream = video_resp.bytes_stream().map_err(anyhow::Error::new);

        let audio_stream = audio_resp.bytes_stream().map_err(anyhow::Error::new);

        let out_stream = remuxer::remux(video_stream, audio_stream).await?;

        let out_stream = out_stream
            .map_err(|e| futures::io::Error::new(ErrorKind::Other, e))
            .boxed();

        Ok(out_stream)
    }
}
