use crate::bot::Notifier;
use crate::downloader::Downloader;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;

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

    async fn download(&self, url: &str, notifier: Notifier) {
        todo!()
    }
}
