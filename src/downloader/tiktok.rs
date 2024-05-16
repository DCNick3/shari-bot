use crate::bot::Notifier;
use crate::downloader::{Downloader, VideoDownloadResult};
use crate::whatever::Whatever;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::cookie::{CookieStore, Jar};
use reqwest::header::{COOKIE, LOCATION, ORIGIN, REFERER, SET_COOKIE, USER_AGENT};
use reqwest::redirect::Policy;
use reqwest::{Client, ClientBuilder};
use snafu::{whatever, FromString, OptionExt, ResultExt};
use std::sync::Arc;
use tracing::debug;
use url::Url;

static URL_PATTERNS: [Lazy<Regex>; 2] = [
    Lazy::new(|| {
        Regex::new(r"^(https?://)?((www|m)\.)?tiktok\.com/[@a-zA-Z0-9-_]+/video/[0-9]+(\?.*)?$")
            .unwrap()
    }),
    Lazy::new(|| Regex::new(r"^https://v[mt]\.tiktok\.com/[a-zA-Z0-9]{9}/.*$").unwrap()),
];

static TOKEN_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<input type="hidden" id="token" name="token" value="([0-9a-f]{64})"/>"#).unwrap()
});

static LINK_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"class="size">No watermark </\w+>\s*<\w+[^>]*><a[^>]*href="([^"]+)"[^>]*>Download video</\w+>"#).unwrap()
});

#[tracing::instrument(skip_all)]
async fn ttdownloader_get_video_link(client: &Client, tt_url: &Url) -> Result<Url, Whatever> {
    debug!("Sending a request to the main page to get the token...");
    let main_page_resp = client
        .get("https://ttdownloader.com")
        .send()
        .await
        .whatever_context("Getting ttdownloader main page")?
        .error_for_status()
        .whatever_context("ttdownloader responded with an error")?;

    let jar = Jar::default();
    jar.set_cookies(
        &mut main_page_resp.headers().get_all(SET_COOKIE).iter(),
        main_page_resp.url(),
    );

    let cookies = jar
        .cookies(main_page_resp.url())
        .ok_or(Whatever::without_source(
            "Could not find cookies in the response".to_owned(),
        ))?;

    let main_page = main_page_resp
        .text()
        .await
        .whatever_context("Getting ttdownloader main page")?;

    let captures = TOKEN_PATTERN.captures_iter(&main_page).next();
    let captures =
        captures.whatever_context("Could not find ttdownloader token on the main page")?;
    let token = captures.get(1).unwrap().as_str();

    debug!("Found token: {}", token);
    debug!("Found cookies: {:?}", cookies);

    let params = [("url", tt_url.as_str()), ("format", ""), ("token", token)];

    debug!("Sending POST request to get video links...");
    let resp = client
        .post("https://ttdownloader.com/search/")
        .form(&params)
        .header(USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/92.0.4515.115 Safari/537.36")
        .header(ORIGIN, "https://ttdownloader.com")
        .header(REFERER, "https://ttdownloader.com")
        .header(COOKIE, cookies)
        .header("X-Requested-With", "XMLHttpRequest")
        .send()
        .await
        .whatever_context("Sending the tiktok request to ttdownloader")?
        .error_for_status()
        .whatever_context("ttdownloader responded with an error")?
        .text()
        .await
        .whatever_context("Getting ttdownloader response")?;

    debug!("Response: {}", resp);

    let link = LINK_PATTERN
        .captures_iter(&resp)
        .next()
        .whatever_context("Could not find link in ttdownloader response")?;

    let link = link.get(1).unwrap().as_str();

    debug!("Found link: {}", link);

    // ttdownloader gives us a scary-looking link that then redirects us to the tt CDN
    // we want to dereference the redirect here

    debug!("Making a final request to dereference the link to actual tt CDN");

    let res = client.get(link).send().await.whatever_context(
        "Sending reqwest to ttdownloader link to deference the redirect failed",
    )?;

    if !res.status().is_redirection() {
        whatever!("Expected to get a redirect, but got this:\n{:?}", res);
    }

    let location = res
        .headers()
        .get(LOCATION)
        .whatever_context("Could not find location header in ttdownloader response")?;

    debug!("Extracted Location: {:?}", location);

    let video_url = Url::parse(
        location
            .to_str()
            .whatever_context("Converting video location to String")?,
    )
    .whatever_context("Location is not a URL??")?;

    debug!("Parsed video_url: {}", video_url);

    Ok(video_url)
}

/// Downloads TikTok videos
///
/// It's implemented by scraping https://ttdownloader.com
///
/// TODO: is there a more stable target to depend on?
#[derive(Debug)]
pub struct TikTokDownloader {
    client: Client,
}

impl TikTokDownloader {
    pub fn new() -> Self {
        Self {
            client: ClientBuilder::new()
                .redirect(Policy::none())
                .build()
                .unwrap(),
        }
    }
}

#[async_trait]
impl Downloader for TikTokDownloader {
    fn probe_url(&self, url: &Url) -> bool {
        URL_PATTERNS
            .iter()
            .any(|pattern| pattern.is_match(url.as_str()))
    }

    fn link_text(&self) -> &'static str {
        "🔗 TikTok"
    }

    #[tracing::instrument(skip_all)]
    async fn download(
        self: Arc<Self>,
        url: Url,
        notifier: Notifier,
    ) -> Result<VideoDownloadResult, Whatever> {
        let video_link = ttdownloader_get_video_link(&self.client, &url).await?;
        let video_stream = super::stream_url(&self.client, video_link, notifier).await?;

        Ok(VideoDownloadResult {
            // TODO: resolve the url, get the video information somehow
            canonical_url: url,
            video_information: None,
            video_stream,
        })
    }
}
