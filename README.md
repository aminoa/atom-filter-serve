# Atom/RSS Filter Serve

A Rust application that filters an [Atom](https://en.wikipedia.org/wiki/Atom_(web_standard)) feed to show only entries containing specific keywords and then re-serves that filtered feed as both Atom and RSS feeds.

## Motivation

I wrote this to get an Atom feed for Shonumi's blog using the commits in the repo. However, to only get the new articles, I whipped up a quick Rust app using opencode.

## Quick Start

1. **Configure**: Set `ATOM_FEED_URL` in `.env` file or use `--url` option
2. **Run**: `cargo run`
3. **Access**: 
   - Atom: http://localhost:3000/atom
   - RSS: http://localhost:3000/rss

## Configuration

### Environment Variables (.env file)
```bash
# Required: Any Atom feed URL
ATOM_FEED_URL=https://example.com/feed.atom

# Optional: Atom feed metadata
FEED_TITLE=My Filtered Feed
FEED_DESCRIPTION=Feed entries containing specific keywords
```

## Endpoints

- `/` - Homepage with feed information
- `/atom` or `/feed.xml` - Filtered Atom feed
- `/rss` or `/rss.xml` - Filtered RSS feed

### CLI Options
```bash
cargo run -- --url "https://example.com/feed.atom"           # Any Atom feed
cargo run -- --filter-word "security"                       # Filter for "security" instead of "article"
cargo run -- --port 8080                                    # Custom port
cargo run -- --serve-once                                   # One-time fetch
```

## Examples

```bash
# GitHub commits containing "security" (default filter is "article")
cargo run -- --url "https://github.com/torvalds/linux/commits/master/.atom" --filter-word "security"

# GitLab commits containing "feature"
cargo run -- --url "https://gitlab.com/user/repo/-/commits/main?format=atom" --filter-word "feature"

# Any blog's Atom feed containing "tutorial"
cargo run -- --url "https://blog.example.com/feed.atom" --filter-word "tutorial"

# Default: shonumi articles (configured in .env)
cargo run
```

**Note:** Without `--filter-word`, it filters for "article" by default.
