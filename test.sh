#!/bin/bash
# Test script for Dataful MCP
# Run this script to manually test the MCP server

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}Dataful MCP Test Script${NC}"
echo "========================"
echo ""

# Check if the binary exists
if [ ! -f "target/release/dataful-mcp" ] && [ ! -f "target/debug/dataful-mcp" ]; then
    echo -e "${YELLOW}Building MCP server...${NC}"
    cargo build --release
    if [ $? -ne 0 ]; then
        echo -e "${RED}Build failed!${NC}"
        exit 1
    fi
fi

BINARY="target/release/dataful-mcp"
if [ ! -f "$BINARY" ]; then
    BINARY="target/debug/dataful-mcp"
fi

echo -e "${GREEN}Using binary: $BINARY${NC}"
echo ""

# Function to send JSON-RPC request
send_request() {
    local request="$1"
    echo -e "${YELLOW}Request:${NC} $request"
    echo "$request" | $BINARY 2>/dev/null | head -1
    echo ""
}

# Test 1: Initialize
echo -e "${GREEN}Test 1: Initialize${NC}"
send_request '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'

# Test 2: List tools
echo -e "${GREEN}Test 2: List Tools${NC}"
send_request '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'

# Test 3: Ping (not connected)
echo -e "${GREEN}Test 3: Ping (before connect)${NC}"
send_request '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ping","arguments":{}}}'

# Test 4: Connect (will fail if Dataful not running)
echo -e "${GREEN}Test 4: Connect${NC}"
send_request '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"connect","arguments":{"port":9222}}}'

echo -e "${YELLOW}Note: Connect will fail if Dataful is not running with:${NC}"
echo "  WEBKIT_INSPECTOR_SERVER=127.0.0.1:9222 cargo tauri dev"
echo ""

echo -e "${GREEN}Tests complete!${NC}"
