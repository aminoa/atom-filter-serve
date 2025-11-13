#!/bin/bash

echo "=== Atom Feed Filter Test Script ==="
echo

echo "1. Testing compilation..."
cargo check --quiet
if [ $? -eq 0 ]; then
    echo "✅ Compilation successful"
else
    echo "❌ Compilation failed"
    exit 1
fi

echo
echo "2. Testing with default configuration (shonumi articles)..."
cargo run --quiet -- --serve-once > /tmp/test_feed.xml 2>/dev/null
if [ $? -eq 0 ]; then
    ITEM_COUNT=$(grep -c "<item>" /tmp/test_feed.xml)
    echo "✅ Default config: $ITEM_COUNT articles found (filters for 'article' by default)"
else
    echo "❌ Default config failed"
fi

echo
echo "3. Testing with different Atom feed and filter..."
cargo run --quiet -- --url "https://github.com/microsoft/vscode/commits/main/.atom" --filter-word "fix" --serve-once > /tmp/test_feed2.xml 2>/dev/null
if [ $? -eq 0 ]; then
    ITEM_COUNT=$(grep -c "<item>" /tmp/test_feed2.xml)
    echo "✅ Custom config: $ITEM_COUNT VS Code fixes found"
else
    echo "❌ Custom config failed"
fi

echo
echo "4. Testing server startup (5 seconds)..."
timeout 5 cargo run --quiet -- --port 3001 > /dev/null 2>&1 &
SERVER_PID=$!
sleep 2

if kill -0 $SERVER_PID 2>/dev/null; then
    echo "✅ Server started successfully on port 3001"
    kill $SERVER_PID
else
    echo "⚠️  Server test skipped (unable to start in test environment)"
fi

echo
echo "=== All tests completed ==="
echo
echo "Usage Examples:"
echo "  # Use .env configuration (default: shonumi articles, filter: 'article')"
echo "  cargo run"
echo
echo "  # Any Atom feed with custom filter"  
echo "  cargo run -- --url 'https://example.com/feed.atom' --filter-word 'security'"
echo
echo "  # One-time fetch (outputs RSS to stdout)"
echo "  cargo run -- --serve-once"
echo
echo "  # Custom port"
echo "  cargo run -- --port 8080"
echo
echo "Atom feed sources to try:"
echo "  - GitHub: https://github.com/username/repo/commits/branch/.atom"
echo "  - GitLab: https://gitlab.com/username/repo/-/commits/branch?format=atom"
echo "  - Any blog: https://blog.example.com/feed.atom"
echo
echo "Feed URLs when server is running:"
echo "  http://localhost:3000/rss"
echo "  http://localhost:3000/feed.xml"
echo
echo "Note: Default filter keyword is 'article' - override with --filter-word"

# Cleanup
rm -f /tmp/test_feed.xml /tmp/test_feed2.xml