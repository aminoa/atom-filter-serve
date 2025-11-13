use anyhow::{anyhow, Result};
use atom_syndication::Feed as AtomFeed;
use axum::{
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use chrono;
use clap::Parser;
use rss::{Channel, Item};
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
    
    #[clap(short, long, default_value = "article", help = "Filter keyword (default: 'article')")]
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

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file if present
    let _ = dotenvy::dotenv();
    
    tracing_subscriber::fmt::init();
    
    let args = Args::parse();
    
    // Get configuration from environment variables or defaults
    let atom_feed_url = args.url
        .or_else(|| env::var("ATOM_FEED_URL").ok())
        .unwrap_or_else(|| {
            eprintln!("Error: No Atom feed URL provided.");
            eprintln!("Please set ATOM_FEED_URL environment variable or use --url option.");
            eprintln!("Example: https://example.com/feed.atom");
            std::process::exit(1);
        });
    
    let feed_title = env::var("FEED_TITLE").unwrap_or_else(|_| "Filtered Feed".to_string());
    let feed_description = env::var("FEED_DESCRIPTION").unwrap_or_else(|_| 
        format!("Feed entries containing '{}'", args.filter_word)
    );
    
    let config = AppConfig {
        atom_feed_url: atom_feed_url.clone(),
        filter_word: args.filter_word,
        feed_title,
        feed_description,
    };
    
    if args.serve_once {
        // Just fetch and print the filtered RSS once
        match fetch_and_filter_feed(&config).await {
            Ok(rss_content) => {
                println!("{}", rss_content);
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    let app_state = AppState {
        config: config.clone(),
        cache_duration: Duration::from_secs(args.cache_seconds),
        cached_feed: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
    };

    let app = Router::new()
        .route("/", get(serve_homepage))
        .route("/rss", get(serve_rss_feed))
        .route("/feed.xml", get(serve_rss_feed))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr = format!("0.0.0.0:{}", args.port);
    info!("Starting server on {}", addr);
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
    let html = format!(r#"
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
                <h3>RSS Feed URL:</h3>
                <code>/rss</code> or <code>/feed.xml</code>
            </div>
            
            <div class="config">
                <h3>Configuration:</h3>
                <p><strong>Source:</strong> {}</p>
                <p><strong>Filter:</strong> "{}" (case-insensitive)</p>
                <p><strong>Feed Title:</strong> {}</p>
            </div>
            
            <p>Add this RSS feed to your reader to get notified when new entries match your filter!</p>
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

async fn serve_rss_feed(
    Query(params): Query<HashMap<String, String>>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let force_refresh = params.get("refresh").is_some();
    
    // Check cache first
    if !force_refresh {
        let cached = state.cached_feed.read().await;
        if let Some((content, timestamp)) = cached.as_ref() {
            if timestamp.elapsed() < state.cache_duration {
                info!("Serving cached RSS feed");
                return (
                    StatusCode::OK,
                    [("Content-Type", "application/rss+xml; charset=utf-8")],
                    content.clone(),
                ).into_response();
            }
        }
    }

    // Fetch fresh content
    info!("Fetching fresh RSS feed");
    match fetch_and_filter_feed(&state.config).await {
        Ok(rss_content) => {
            // Update cache
            let mut cached = state.cached_feed.write().await;
            *cached = Some((rss_content.clone(), std::time::Instant::now()));
            
            (
                StatusCode::OK,
                [("Content-Type", "application/rss+xml; charset=utf-8")],
                rss_content,
            ).into_response()
        }
        Err(e) => {
            error!("Failed to fetch feed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to fetch feed: {}", e),
            ).into_response()
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
            let summary = entry.summary().map(|s| s.as_str().to_lowercase()).unwrap_or_default();
            
            title.contains(&filter_word_lower) || summary.contains(&filter_word_lower)
        })
        .collect();

    info!("Filtered to {} matching entries", filtered_entries.len());

    // Convert to RSS
    let mut channel = Channel::default();
    channel.set_title(&config.feed_title);
    
    // Extract feed link from URL (remove .atom extension if present)
    let feed_link = config.atom_feed_url
        .strip_suffix(".atom")
        .or_else(|| config.atom_feed_url.strip_suffix("/.atom"))
        .unwrap_or(&config.atom_feed_url)
        .to_string();
    
    channel.set_link(&feed_link);
    channel.set_description(&config.feed_description);
    channel.set_language(Some("en-us".to_string()));
    channel.set_generator(Some("Atom Feed Filter".to_string()));
    channel.set_last_build_date(Some(chrono::Utc::now().to_rfc2822()));

    let mut items = Vec::new();
    for entry in filtered_entries {
        let mut item = Item::default();
        
        item.set_title(Some(entry.title().as_str().to_string()));
        
        if let Some(link) = entry.links().first() {
            item.set_link(Some(link.href().to_string()));
            item.set_guid(Some(rss::Guid {
                value: link.href().to_string(),
                permalink: true,
            }));
        }
        
        if let Some(summary) = entry.summary() {
            item.set_description(Some(summary.as_str().to_string()));
        }
        
        // Convert atom date to RSS date format
        let date = entry.updated().format("%a, %d %b %Y %H:%M:%S %z").to_string();
        item.set_pub_date(Some(date));
        
        if let Some(author) = entry.authors().first() {
            item.set_author(Some(author.name().to_string()));
        }
        
        items.push(item);
    }
    
    channel.set_items(items);
    
    // Convert to XML string
    let mut rss_output = Vec::new();
    channel.write_to(&mut rss_output)?;
    
    Ok(String::from_utf8(rss_output)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_matching() {
        // Test case-insensitive matching
        assert!("New Article about Rust".to_lowercase().contains("article"));
        assert!("ARTICLE: How to code".to_lowercase().contains("article"));
        assert!("Updated article on web dev".to_lowercase().contains("article"));
        
        // Test non-matches
        assert!(!"Fix bug in parser".to_lowercase().contains("article"));
        assert!(!"Update README".to_lowercase().contains("article"));
    }
}
