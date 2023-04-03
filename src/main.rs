mod filters;

use askama::Template;
use axum::extract::Path;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, get_service, post};
use axum::{Form, Router};
use axum_macros::debug_handler;

use futures::StreamExt;
use serde::Deserialize;
use tokio_stream::wrappers::ReadDirStream;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Template, Clone, Deserialize)]
#[template(path = "article.html", escape = "none")]
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

    async fn load(title: &str) -> Option<Self> {
        let path = format!("content/articles/{}.md", urlencoding::encode(title));
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Some(Article {
                title: title.to_string(),
                content,
            }),
            Err(_) => None,
        }
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

#[derive(Template, Deserialize, Clone, Default)]
#[template(path = "overview.html")]
struct Overview {
    articles: Vec<(String, String)>,
}

impl Overview {
    async fn load() -> Self {
        let mut entries =
            ReadDirStream::new(tokio::fs::read_dir("content/articles").await.unwrap());
        let mut articles = vec![];
        while let Some(Ok(entry)) = entries.next().await {
            let filename = entry.file_name().into_string().unwrap();
            if filename.ends_with(".md") {
                let article = filename.replace(".md", "");
                let title = urlencoding::decode(&article).unwrap().into_owned();
                articles.push((article, title))
            }
        }
        Overview { articles }
    }
}

impl Index {
    async fn write_to_disk(&self) -> tokio::io::Result<()> {
        tokio::fs::write("content/index.md", self.content.as_bytes()).await
    }

    async fn load() -> Self {
        let content = tokio::fs::read_to_string("content/index.md").await.unwrap();
        Index { content }
    }
}

async fn get_article(Path(title): Path<String>) -> impl IntoResponse {
    let title = urlencoding::decode(&title).unwrap().into_owned();
    if let Some(article) = Article::load(&title).await {
        article.into_response()
    } else {
        Redirect::temporary(&format!("/edit/article/{title}")).into_response()
    }
}

async fn edit_article(Path(title): Path<String>) -> impl IntoResponse {
    let title = urlencoding::decode(&title).unwrap().into_owned();

    let content = if let Some(article) = Article::load(&title).await {
        article.content
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

async fn edit_index() -> impl IntoResponse {
    let index = Index::load().await;
    Editor {
        is_index: true,
        title: "Index".to_string(),
        content: index.content,
    }
    .into_response()
}

#[axum_macros::debug_handler]
async fn post_article(Form(article): Form<Article>) -> impl IntoResponse {
    article.write_to_disk().await.unwrap();

    Redirect::to(&format!("/article/{}", article.title))
}

#[debug_handler]
async fn update_index(Form(index): Form<Index>) -> impl IntoResponse {
    index.write_to_disk().await.unwrap();

    Redirect::to("/")
}

#[axum_macros::debug_handler]
async fn get_index() -> impl IntoResponse {
    Index::load().await
}

async fn get_overview() -> impl IntoResponse {
    Overview::load().await
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let router = Router::new()
        .route("/", get(get_index))
        .route("/", post(update_index))
        .route("/overview", get(get_overview))
        .route("/article/:id", get(get_article))
        .route("/edit/article/:id", get(edit_article))
        .route("/edit/index", get(edit_index))
        .route("/article/:id", post(post_article))
        .route_service(
            "/favicon.ico",
            get_service(ServeFile::new("content/media/favicon.ico")),
        )
        .nest_service("/media/", get_service(ServeDir::new("content/media")));

    let addr = "0.0.0.0:8000".parse()?;

    axum::Server::bind(&addr)
        .serve(router.into_make_service())
        .await?;
    Ok(())
}
