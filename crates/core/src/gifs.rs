use chatsvc::Gif;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::remote::Remote;

impl<R: Remote> SyncEngine<R> {
    /// An empty query returns the trending GIFs.
    pub async fn search_gifs(&self, query: &str) -> Result<Vec<Gif>> {
        self.remote.search_gifs(query).await
    }
}
