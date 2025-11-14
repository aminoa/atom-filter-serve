use anyhow::{anyhow, Result};
use atom_syndication::{Feed as AtomFeed, FeedBuilder, Text};
use axum::{
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use clap::Parser;
use rss::{ChannelBuilder, Item, ItemBuilder};
use std::collections::HashMap;
use std::env;
use tokio::time::Duration;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[clap(name = "atom-feed-filter")]
#[clap(about = "Filters Atom feed entries by keyword (default: 'article')")]
struct Args {
    #[clap(short, long, default_value = "3000")]
    port: u16,

    #[clap(short, long, default_value = "300")]
    cache_seconds: u64,

    #[clap(long)]
    serve_once: bool,

    #[clap(short, long, help = "Atom feed URL to filter")]
    url: Option<String>,

    #[clap(
        short,
        long,
        default_value = "article",
        help = "Filter keyword (default: 'article')"
    )]
    filter_word: String,
}

#[derive(Clone)]
struct AppConfig {
    atom_feed_url: String,
    filter_word: String,
    feed_title: String,
    feed_description: String,
}

#[derive(Clone)]
struct AppState {
    config: AppConfig,
    cache_duration: Duration,
    cached_feed: std::sync::Arc<tokio::sync::RwLock<Option<(String, std::time::Instant)>>>,
}

impl AppState {
    /// Convenience constructor for `AppState`.
    fn new(config: AppConfig, cache_duration: Duration) -> Self {
        Self {
            config,
            cache_duration,
            cached_feed: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }
}

/// Factory for creating `AppState` instances.
/// Encapsulates creation logic so it can be extended without modifying call sites.
struct AppStateFactory {
    config: AppConfig,
    cache_duration: Duration,
}

impl AppStateFactory {
    /// Create a new factory.
    fn new(config: AppConfig, cache_duration: Duration) -> Self {
        Self {
            config,
            cache_duration,
        }
    }

    /// Build the `AppState`.
    fn build(self) -> AppState {
        AppState::new(self.config, self.cache_duration)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file if present
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Get configuration from environment variables or defaults
    let atom_feed_url = args
        .url
        .or_else(|| env::var("ATOM_FEED_URL").ok())
        .unwrap_or_else(|| {
            eprintln!("Error: No Atom feed URL provided.");
            eprintln!("Please set ATOM_FEED_URL environment variable or use --url option.");
            eprintln!("Example: https://example.com/feed.atom");
            std::process::exit(1);
        });

    let feed_title = env::var("FEED_TITLE").unwrap_or_else(|_| "Filtered Feed".to_string());
    let feed_description = env::var("FEED_DESCRIPTION")
        .unwrap_or_else(|_| format!("Feed entries containing '{}'", args.filter_word));

    let config = AppConfig {
        atom_feed_url: atom_feed_url.clone(),
        filter_word: args.filter_word,
        feed_title,
        feed_description,
    };

    if args.serve_once {
        // Just fetch and print the filtered Atom once
        match fetch_and_filter_feed(&config).await {
            Ok(atom_content) => {
                println!("{}", atom_content);
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    let app_state =
        AppStateFactory::new(config.clone(), Duration::from_secs(args.cache_seconds)).build();

    let app = Router::new()
        .route("/", get(serve_homepage))
        .route("/atom", get(serve_atom_feed))
        .route("/feed.xml", get(serve_atom_feed))
        .route("/rss", get(serve_rss_feed))
        .route("/rss.xml", get(serve_rss_feed))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr = format!("0.0.0.0:{}", args.port);
    info!("Starting server on {}", addr);
    info!(
        "Atom feed available at: http://localhost:{}/atom",
        args.port
    );
    info!("RSS feed available at: http://localhost:{}/rss", args.port);
    info!("Monitoring: {}", config.atom_feed_url);
    info!("Filter word: '{}'", config.filter_word);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn serve_homepage(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Html<String> {
    let html = format!(
        r#"
    <!DOCTYPE html>
    <html>
    <head>
        <title>Atom Feed Filter</title>
        <style>
            body {{ font-family: Arial, sans-serif; max-width: 800px; margin: 50px auto; padding: 20px; }}
            .container {{ text-align: center; }}
            .feed-link {{ background: #f0f0f0; padding: 10px; border-radius: 5px; margin: 20px 0; }}
            code {{ background: #e0e0e0; padding: 2px 5px; border-radius: 3px; }}
            .config {{ background: #f9f9f9; padding: 15px; border-radius: 5px; text-align: left; }}
        </style>
    </head>
    <body>
        <div class="container">
            <h1>Atom Feed Filter</h1>
            <p>This service filters Atom feed entries to show only those containing the word <strong>"{}"</strong>.</p>

            <div class="feed-link">
                <h3>Feed URLs:</h3>
                <p><strong>Atom:</strong> <code>/atom</code> or <code>/feed.xml</code></p>
                <p><strong>RSS:</strong> <code>/rss</code> or <code>/rss.xml</code></p>
            </div>

            <div class="config">
                <h3>Configuration:</h3>
                <p><strong>Source:</strong> {}</p>
                <p><strong>Filter:</strong> "{}" (case-insensitive)</p>
                <p><strong>Feed Title:</strong> {}</p>
            </div>

            <p>Add this feed to your reader (Atom or RSS format) to get notified when new entries match your filter!</p>
        </div>
    </body>
    </html>
    "#,
        state.config.filter_word,
        state.config.atom_feed_url,
        state.config.filter_word,
        state.config.feed_title
    );

    Html(html)
}

async fn serve_atom_feed(
    Query(params): Query<HashMap<String, String>>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let force_refresh = params.get("refresh").is_some();

    // Check cache first
    if !force_refresh {
        let cached = state.cached_feed.read().await;
        if let Some((content, timestamp)) = cached.as_ref() {
            if timestamp.elapsed() < state.cache_duration {
                info!("Serving cached Atom feed");
                return (
                    StatusCode::OK,
                    [("Content-Type", "application/atom+xml; charset=utf-8")],
                    content.clone(),
                )
                    .into_response();
            }
        }
    }

    // Fetch fresh content
    info!("Fetching fresh Atom feed");
    match fetch_and_filter_feed(&state.config).await {
        Ok(atom_content) => {
            // Update cache
            let mut cached = state.cached_feed.write().await;
            *cached = Some((atom_content.clone(), std::time::Instant::now()));
            (
                StatusCode::OK,
                [("Content-Type", "application/atom+xml; charset=utf-8")],
                atom_content,
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to fetch feed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to fetch feed: {}", e),
            )
                .into_response()
        }
    }
}

async fn serve_rss_feed(
    Query(params): Query<HashMap<String, String>>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let _force_refresh = params.get("refresh").is_some();

    // Note: We don't cache RSS separately since it's converted from the same source
    info!("Fetching fresh RSS feed");
    match fetch_and_filter_feed_rss(&state.config).await {
        Ok(rss_content) => (
            StatusCode::OK,
            [("Content-Type", "application/rss+xml; charset=utf-8")],
            rss_content,
        )
            .into_response(),
        Err(e) => {
            error!("Failed to fetch feed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to fetch feed: {}", e),
            )
                .into_response()
        }
    }
}

async fn fetch_and_filter_feed(config: &AppConfig) -> Result<String> {
    info!("Fetching atom feed...");

    let client = reqwest::Client::new();
    let response = client
        .get(&config.atom_feed_url)
        .header("User-Agent", "Atom Feed Filter Bot 1.0")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow!("HTTP error: {}", response.status()));
    }

    let content = response.text().await?;
    let feed = AtomFeed::read_from(content.as_bytes())?;

    info!("Found {} total entries", feed.entries().len());

    // Filter entries containing the filter word (case-insensitive)
    let filter_word_lower = config.filter_word.to_lowercase();
    let filtered_entries: Vec<_> = feed
        .entries()
        .iter()
        .filter(|entry| {
            let title = entry.title().as_str().to_lowercase();
            let summary = entry
                .summary()
                .map(|s| s.as_str().to_lowercase())
                .unwrap_or_default();

            title.contains(&filter_word_lower) || summary.contains(&filter_word_lower)
        })
        .collect();

    info!("Filtered to {} matching entries", filtered_entries.len());

    // Create new Atom feed with filtered entries
    let filtered_feed = FeedBuilder::default()
        .title(Text::plain(&config.feed_title))
        .id(feed.id())
        .updated(chrono::Utc::now())
        .authors(feed.authors().to_vec())
        .links(feed.links().to_vec())
        .subtitle(Some(Text::plain(&config.feed_description)))
        .generator(Some(atom_syndication::Generator {
            value: "Atom Feed Filter".to_string(),
            uri: None,
            version: Some("1.0".to_string()),
        }))
        .entries(filtered_entries.into_iter().cloned().collect::<Vec<_>>())
        .build();

    // Convert to XML string
    let mut atom_output = Vec::new();
    filtered_feed.write_to(&mut atom_output)?;

    Ok(String::from_utf8(atom_output)?)
}

async fn fetch_and_filter_feed_rss(config: &AppConfig) -> Result<String> {
    info!("Fetching atom feed for RSS conversion...");

    let client = reqwest::Client::new();
    let response = client
        .get(&config.atom_feed_url)
        .header("User-Agent", "Atom Feed Filter Bot 1.0")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow!("HTTP error: {}", response.status()));
    }

    let content = response.text().await?;
    let feed = AtomFeed::read_from(content.as_bytes())?;

    info!("Found {} total entries", feed.entries().len());

    // Filter entries containing the filter word (case-insensitive)
    let filter_word_lower = config.filter_word.to_lowercase();
    let filtered_entries: Vec<_> = feed
        .entries()
        .iter()
        .filter(|entry| {
            let title = entry.title().as_str().to_lowercase();
            let summary = entry
                .summary()
                .map(|s| s.as_str().to_lowercase())
                .unwrap_or_default();

            title.contains(&filter_word_lower) || summary.contains(&filter_word_lower)
        })
        .collect();

    info!(
        "Filtered to {} matching entries for RSS",
        filtered_entries.len()
    );

    // Convert Atom entries to RSS items
    let rss_items: Vec<Item> = filtered_entries
        .into_iter()
        .map(|entry| {
            let mut item_builder = ItemBuilder::default();

            // Title
            item_builder.title(Some(entry.title().as_str().to_string()));

            // Link
            if let Some(link) = entry.links().first() {
                item_builder.link(Some(link.href().to_string()));
            }

            // Description (from summary or content)
            let description = entry
                .summary()
                .map(|s| s.as_str().to_string())
                .or_else(|| {
                    entry
                        .content()
                        .and_then(|c| c.value().map(|v| v.to_string()))
                })
                .unwrap_or_default();
            item_builder.description(Some(description));

            // Publication date
            let pub_date = entry.updated().to_rfc2822();
            item_builder.pub_date(Some(pub_date));

            // GUID
            item_builder.guid(Some(rss::Guid {
                value: entry.id().to_string(),
                permalink: false,
            }));

            // Author
            if let Some(author) = entry.authors().first() {
                item_builder.author(Some(author.name().to_string()));
            }

            item_builder.build()
        })
        .collect();

    // Create RSS channel
    let channel = ChannelBuilder::default()
        .title(&config.feed_title)
        .link(feed.links().first().map(|l| l.href()).unwrap_or(feed.id()))
        .description(&config.feed_description)
        .items(rss_items)
        .generator(Some("Atom Feed Filter 1.0".to_string()))
        .build();

    // Convert to XML string
    let rss_output = channel.to_string();

    Ok(rss_output)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_filter_matching() {
        // Test case-insensitive matching
        assert!("New Article about Rust".to_lowercase().contains("article"));
        assert!("ARTICLE: How to code".to_lowercase().contains("article"));
        assert!("Updated article on web dev"
            .to_lowercase()
            .contains("article"));

        // Test non-matches
        assert!(!"Fix bug in parser".to_lowercase().contains("article"));
        assert!(!"Update README".to_lowercase().contains("article"));
    }
}
