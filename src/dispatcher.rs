use crate::downloader::Downloader;

struct Dispatcher {
    downloaders: Vec<Box<dyn Downloader>>,
}

impl Dispatcher {
    pub fn new(downloaders: Vec<Box<dyn Downloader>>) -> Self {
        Self { downloaders }
    }

    pub async fn dispatch(&self, url: &str) {}
}
