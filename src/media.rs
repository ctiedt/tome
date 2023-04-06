use askama::Template;
use askama_axum::IntoResponse;
use axum::{
    extract::{Multipart, State},
    response::Redirect,
};
use tokio_stream::{wrappers::ReadDirStream, StreamExt};

use crate::TomeConfig;

#[derive(Template)]
#[template(path = "media.html")]
pub struct MediaOverview {
    allowed_uploads: String,
    media: Vec<String>,
}

pub async fn get_media_overview(State(config): State<TomeConfig>) -> impl IntoResponse {
    let mut entries = ReadDirStream::new(tokio::fs::read_dir("content/media").await.unwrap());
    let mut media = vec![];
    let allowed_uploads = config.allowed_uploads.join(", ");
    while let Some(Ok(entry)) = entries.next().await {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        media.push(file_name);
    }

    MediaOverview {
        allowed_uploads,
        media,
    }
}

pub async fn post_media(
    State(config): State<TomeConfig>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_string();
        let file_name = field.file_name().unwrap().to_string();
        if name == "image"
            && config
                .allowed_uploads
                .iter()
                .any(|ending| file_name.ends_with(ending))
        {
            let data = field.bytes().await.unwrap();

            tokio::fs::write(format!("content/media/{}", file_name), data)
                .await
                .unwrap();
        }
    }

    Redirect::to("/media")
}
