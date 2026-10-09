use std::sync::Arc;

use teams_core::{Error, Gif};
use tokio::sync::oneshot;

use crate::backend::Engine;
use crate::{demo, runtime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifError {
    Disabled,
    Failed,
}

pub type GifSearch = oneshot::Receiver<Result<Vec<Gif>, GifError>>;

/// An empty query returns the trending GIFs. Demo mode answers from generated local files.
pub fn search(engine: Option<Arc<Engine>>, demo_mode: bool, query: String) -> GifSearch {
    if demo_mode {
        return runtime::spawn(async move { Ok(demo::search_gifs(&query)) });
    }
    let Some(engine) = engine else {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Err(GifError::Failed));
        return receiver;
    };
    runtime::spawn(async move {
        engine
            .search_gifs(&query)
            .await
            .map_err(|error| match error {
                Error::GifsDisabled => GifError::Disabled,
                _ => GifError::Failed,
            })
    })
}
