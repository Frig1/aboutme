use std::{
    collections::HashSet,
    env,
    io::{Error, ErrorKind},
    path::Path,
    sync::Arc,
};

use askama::Template;
use axum::{Router, extract::State, response::Html, routing::get};
use serde::Deserialize;
use tower_http::services::ServeDir;

#[derive(Debug, Deserialize)]
struct ProjectsFile {
    project: Vec<Project>,
}

#[derive(Debug, Deserialize)]
struct Project {
    slug: String,
    title: String,
    category: String,
    year: String,
    summary: String,
    image: String,
    alt: String,
    href: Option<String>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    projects: &'a [Project],
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let projects = load_projects()?;
    let html = Arc::new(
        IndexTemplate {
            projects: &projects,
        }
        .render()?,
    );

    let app = Router::new()
        .route("/", get(index))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(html);

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let address = env::var("ADDRESS").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("Quiet Signal is running at http://{address}");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index(State(html): State<Arc<String>>) -> Html<String> {
    Html((*html).clone())
}

fn load_projects() -> Result<Vec<Project>, Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string("content/projects.toml")?;
    let projects = toml::from_str::<ProjectsFile>(&source)?.project;
    let mut slugs = HashSet::new();

    for project in &projects {
        if project.slug.trim().is_empty()
            || project.title.trim().is_empty()
            || project.summary.trim().is_empty()
            || project.alt.trim().is_empty()
        {
            return Err(
                Error::new(ErrorKind::InvalidData, "project fields cannot be empty").into(),
            );
        }
        if !slugs.insert(&project.slug) {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("duplicate project slug: {}", project.slug),
            )
            .into());
        }

        let asset = project.image.trim_start_matches("/static/");
        if !Path::new("static").join(asset).is_file() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("missing project image: {}", project.image),
            )
            .into());
        }
    }

    Ok(projects)
}
