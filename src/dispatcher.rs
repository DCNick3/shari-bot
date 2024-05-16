use std::sync::Arc;

use url::Url;

use crate::downloader::Downloader;

pub struct DownloadDispatcher {
    downloaders: Vec<Arc<dyn Downloader>>,
}

impl DownloadDispatcher {
    pub fn new(downloaders: Vec<Arc<dyn Downloader>>) -> Self {
        Self { downloaders }
    }

    pub fn find_downloader(&self, url: &Url) -> Option<Arc<dyn Downloader>> {
        self.downloaders.iter().find(|d| d.probe_url(url)).cloned()
    }
}
