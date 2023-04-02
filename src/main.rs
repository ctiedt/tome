mod filters;

use std::collections::HashMap;
use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use axum_macros::debug_handler;
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReadDirStream;

#[derive(Template, Clone, Deserialize)]
#[template(path = "article.html")]
struct Article {
    title: String,
    content: String,
}

impl Article {
    fn path(&self) -> String {
        urlencoding::encode(&self.title).into_owned()
    }

    async fn write_to_disk(&self) -> tokio::io::Result<()> {
        tokio::fs::write(
            format!("content/articles/{}.md", self.path()),
            self.content.as_bytes(),
        )
        .await
    }
}

#[derive(Template, Clone)]
#[template(path = "editor.html")]
struct Editor {
    is_index: bool,
    title: String,
    content: String,
}

#[derive(Template, Deserialize, Clone, Default)]
#[template(path = "index.html", escape = "none")]
struct Index {
    content: String,
}

impl Index {
    async fn write_to_disk(&self) -> tokio::io::Result<()> {
        tokio::fs::write("content/index.md", self.content.as_bytes()).await
    }
}

#[derive(Default)]
struct TomeState {
    articles: HashMap<String, Article>,
    index: Index,
}

impl TomeState {
    async fn load() -> Self {
        let mut articles = HashMap::new();
        let mut entries =
            ReadDirStream::new(tokio::fs::read_dir("content/articles").await.unwrap());
        while let Some(article) = entries.next().await {
            let article = article.unwrap();
            let metadata = article.metadata().await.unwrap();
            if metadata.is_file() && article.file_name().to_string_lossy().ends_with(".md") {
                let title = article.file_name().to_string_lossy().replace(".md", "");
                articles.insert(
                    title.clone(),
                    Article {
                        title,
                        content: tokio::fs::read_to_string(article.path()).await.unwrap(),
                    },
                );
            }
        }
        let index = Index {
            content: tokio::fs::read_to_string("content/index.md").await.unwrap(),
        };
        Self { articles, index }
    }
}

async fn get_article(
    Path(title): Path<String>,
    State(state): State<Arc<Mutex<TomeState>>>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let title = urlencoding::decode(&title).unwrap().into_owned();
    if let Some(article) = state.articles.get(&title) {
        article.clone().into_response()
    } else {
        Redirect::temporary(&format!("/edit/article/{title}")).into_response()
    }
}

async fn edit_article(
    Path(title): Path<String>,
    State(state): State<Arc<Mutex<TomeState>>>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let title = urlencoding::decode(&title).unwrap().into_owned();
    dbg!(&title);

    let content = if let Some(article) = state.articles.get(&title) {
        article.content.clone()
    } else {
        String::new()
    };
    Editor {
        is_index: false,
        title,
        content,
    }
    .into_response()
}

async fn edit_index(State(state): State<Arc<Mutex<TomeState>>>) -> impl IntoResponse {
    let state = state.lock().await;
    Editor {
        is_index: true,
        title: String::new(),
        content: state.index.content.clone(),
    }
    .into_response()
}

#[axum_macros::debug_handler]
async fn post_article(
    State(state): State<Arc<Mutex<TomeState>>>,
    Form(article): Form<Article>,
) -> impl IntoResponse {
    let mut state = state.lock().await;

    let title = article.title.clone();
    dbg!(&title);

    let article = if let Some(mut existing) = state.articles.get_mut(&title) {
        existing.content = article.content;
        existing.clone()
    } else {
        state.articles.insert(title, article.clone());
        article
    };

    article.write_to_disk().await.unwrap();

    article
}

#[debug_handler]
async fn update_index(
    State(state): State<Arc<Mutex<TomeState>>>,
    Form(index): Form<Index>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    state.index = index;
    state.index.write_to_disk().await.unwrap();

    state.index.clone()
}

#[axum_macros::debug_handler]
async fn get_index(State(state): State<Arc<Mutex<TomeState>>>) -> impl IntoResponse {
    state.lock().await.index.clone()
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let state = Arc::new(Mutex::new(TomeState::load().await));

    let router = Router::new()
        .route("/", get(get_index))
        .route("/", post(update_index))
        .route("/article/:id", get(get_article))
        .route("/edit/article/:id", get(edit_article))
        .route("/edit/index", get(edit_index))
        .route("/article/:id", post(post_article))
        .with_state(state);

    let addr = "0.0.0.0:8000".parse()?;

    axum::Server::bind(&addr)
        .serve(router.into_make_service())
        .await?;
    Ok(())
}
