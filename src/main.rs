mod filters;
mod media;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::SystemTime;

use askama::Template;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, get_service, post};
use axum::{Form, Router};
use axum_macros::debug_handler;

use clap::Parser;
use figment::providers::{Format, Serialized, Toml};
use figment::Figment;
use futures::StreamExt;
use media::{get_media_overview, post_media};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio_stream::wrappers::ReadDirStream;
use tower_http::services::{ServeDir, ServeFile};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[derive(Serialize, Deserialize, Parser, Clone)]
pub struct TomeConfig {
    host: Option<IpAddr>,
    port: Option<u16>,
    allowed_uploads: Vec<String>,
}

#[derive(Template, Clone, Deserialize)]
#[template(path = "not_found.html", escape = "none")]
struct NotFound {}

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
        let _ = tokio::fs::create_dir(format!("content/articles/{}", self.path())).await;

        tokio::fs::write(
            format!(
                "content/articles/{}/{}.md",
                self.path(),
                uuid::Uuid::new_v4().hyphenated()
            ),
            self.content.as_bytes(),
        )
        .await?;

        tokio::fs::write(
            format!("content/articles/{}/current.md", self.path()),
            self.content.as_bytes(),
        )
        .await
    }

    async fn load(title: &str) -> Option<Self> {
        let path = format!("content/articles/{}/current.md", urlencoding::encode(title));
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Some(Article {
                title: title.to_string(),
                content,
            }),
            Err(_) => None,
        }
    }

    async fn load_version(title: &str, version: &str) -> Option<Self> {
        let path = format!(
            "content/articles/{}/{version}.md",
            urlencoding::encode(title)
        );
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Some(Article {
                title: title.to_string(),
                content,
            }),
            Err(_) => None,
        }
    }

    async fn get_versions(title: &str) -> Vec<(String, SystemTime)> {
        let mut entries = ReadDirStream::new(
            tokio::fs::read_dir(format!("content/articles/{}", urlencoding::encode(title)))
                .await
                .unwrap(),
        );

        let mut versions = vec![];

        while let Some(Ok(entry)) = entries.next().await {
            let edited = entry
                .metadata()
                .await
                .unwrap()
                .modified()
                .expect("File Access Time should be available");
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if file_name.ends_with(".md") {
                versions.push((file_name.replace(".md", ""), edited));
            }
        }

        versions
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

#[derive(Template, Deserialize, Clone, Default)]
#[template(path = "history.html")]
struct History {
    article: String,
    versions: Vec<(String, String)>,
}

impl Overview {
    async fn load() -> Self {
        let mut entries =
            ReadDirStream::new(tokio::fs::read_dir("content/articles").await.unwrap());
        let mut articles = vec![];
        while let Some(Ok(entry)) = entries.next().await {
            let article = entry.file_name().into_string().unwrap();
            if entry.file_type().await.unwrap().is_dir() {
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

async fn article_history(Path(title): Path<String>) -> impl IntoResponse {
    let mut versions: Vec<(String, SystemTime)> = Article::get_versions(&title).await;
    versions.sort_by_key(|(_, edited)| *edited);
    versions.reverse();

    History {
        article: title.clone(),
        versions: versions
            .into_iter()
            .map(|(article, edited)| {
                (
                    article,
                    OffsetDateTime::from(edited)
                        .format(&time::format_description::well_known::Rfc2822)
                        .unwrap(),
                )
            })
            .collect(),
    }
}

async fn article_version(Path((title, version)): Path<(String, String)>) -> impl IntoResponse {
    if let Some(article) = Article::load_version(&title, &version).await {
        article.into_response()
    } else {
        (StatusCode::NOT_FOUND, NotFound {}).into_response()
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
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    let config: TomeConfig = Figment::new()
        .merge(Toml::file("tome.toml"))
        .join(Serialized::defaults(TomeConfig::parse()))
        .extract()?;

    dbg!(&config.allowed_uploads);

    let router = Router::new()
        .route("/", get(get_index))
        .route("/", post(update_index))
        .route("/overview", get(get_overview))
        .route("/article/:id", get(get_article))
        .route("/edit/article/:id", get(edit_article))
        .route("/edit/index", get(edit_index))
        .route("/article/:id", post(post_article))
        .route("/article/:id/history/:version", get(article_version))
        .route("/article/:id/history", get(article_history))
        .route("/media", get(get_media_overview))
        .route("/media", post(post_media))
        .route_service(
            "/favicon.ico",
            get_service(ServeFile::new("content/media/favicon.ico")),
        )
        .nest_service("/media/", get_service(ServeDir::new("content/media")))
        .fallback(|| async { NotFound {} })
        .with_state(config.clone());

    let addr = (
        config.host.unwrap_or(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))),
        config.port.unwrap_or(5422),
    );

    axum::Server::bind(&SocketAddr::from(addr))
        .serve(router.into_make_service())
        .await?;
    Ok(())
}
