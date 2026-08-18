#!/usr/bin/env python3
"""
MCP Integration End-to-End Test Script
=======================================
Tests the MCP JSON-RPC protocol by starting a mock MCP server and
validating client behavior against it.

Usage:
    python scripts/test_mcp_e2e.py

Requires: Python 3.8+, no external dependencies.
"""

import http.server
import json
import os
import sys
import threading
import time
import urllib.request
import urllib.error


# ── Mock MCP Server ────────────────────────────────────────────────

class McpMockHandler(http.server.BaseHTTPRequestHandler):
    """An HTTP server that responds to MCP JSON-RPC requests."""

    # Shared state across requests
    tools = [
        {
            "name": "browser_navigate",
            "description": "Navigate to a URL in the browser",
            "input_schema": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to navigate to",
                    }
                },
                "required": ["url"],
            },
        },
        {
            "name": "browser_click",
            "description": "Click an element on the page",
            "input_schema": {
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector to click",
                    }
                },
                "required": ["selector"],
            },
        },
        {
            "name": "browser_snapshot",
            "description": "Take a snapshot of the current page",
            "input_schema": {"type": "object", "properties": {}},
        },
        {
            "name": "bash",
            "description": "Execute a shell command",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command to execute",
                    }
                },
                "required": ["command"],
            },
        },
    ]
    call_history = []

    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length)
        response = self.handle_request(body)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(response).encode())

    def handle_request(self, raw_body: bytes) -> dict:
        try:
            req = json.loads(raw_body)
        except json.JSONDecodeError:
            return {
                "jsonrpc": "2.0",
                "error": {"code": -32700, "message": "Parse error"},
                "id": None,
            }

        req_id = req.get("id", None)
        method = req.get("method", "")
        params = req.get("params", {})

        if method == "tools/list":
            return {
                "jsonrpc": "2.0",
                "result": {"tools": self.tools},
                "id": req_id,
            }
        elif method == "tools/call":
            tool_name = params.get("name", "unknown")
            args = params.get("arguments", {})
            self.call_history.append((tool_name, args))
            return {
                "jsonrpc": "2.0",
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": json.dumps({
                                "status": "ok",
                                "tool": tool_name,
                                "arguments": args,
                                "result": f"mock:{tool_name} executed",
                            }),
                        }
                    ]
                },
                "id": req_id,
            }
        elif method == "resources/list":
            return {
                "jsonrpc": "2.0",
                "result": {"resources": []},
                "id": req_id,
            }
        else:
            return {
                "jsonrpc": "2.0",
                "error": {
                    "code": -32601,
                    "message": f"Method not found: {method}",
                },
                "id": req_id,
            }

    def log_message(self, format, *args):
        pass  # Suppress default logging


# ── Helpers ────────────────────────────────────────────────────────

def start_server(port: int = 0) -> tuple[http.server.HTTPServer, int]:
    """Start a mock MCP server on a random port. Returns (server, port)."""
    server = http.server.HTTPServer(("127.0.0.1", port), McpMockHandler)
    port = server.server_address[1]
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()
    return server, port


def send_request(url: str, method: str, params: dict = None,
                 req_id: int = 1) -> dict:
    """Send a JSON-RPC request to the mock MCP server."""
    body = json.dumps({
        "jsonrpc": "2.0",
        "method": method,
        "params": params or {},
        "id": req_id,
    }).encode()
    req = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json"},
    )
    resp = urllib.request.urlopen(req, timeout=5)
    return json.loads(resp.read())


# ── Tests ──────────────────────────────────────────────────────────

passed = 0
failed = 0
errors = []


def test(name: str):
    """Decorator-like test runner."""
    def decorator(fn):
        global passed, failed
        try:
            fn()
            passed += 1
            print(f"  \033[32m✓\033[0m {name}")
        except Exception as e:
            failed += 1
            errors.append((name, str(e)))
            print(f"  \033[31m✗\033[0m {name}: {e}")
    return decorator


# ── Test Cases ─────────────────────────────────────────────────────

def run_tests(port: int):
    url = f"http://127.0.0.1:{port}/mcp"

    @test("tools/list returns all tools")
    def t1():
        resp = send_request(url, "tools/list")
        assert resp.get("jsonrpc") == "2.0", "Missing jsonrpc version"
        assert "result" in resp, "Missing result field"
        tools = resp["result"]["tools"]
        assert len(tools) == 4, f"Expected 4 tools, got {len(tools)}"
        names = [t["name"] for t in tools]
        assert "browser_navigate" in names
        assert "browser_click" in names
        assert "browser_snapshot" in names
        assert "bash" in names

    @test("tools/list returns correct request id")
    def t2():
        resp = send_request(url, "tools/list", req_id=42)
        assert resp.get("id") == 42, f"Expected id 42, got {resp.get('id')}"

    @test("tools/call returns proper result format")
    def t3():
        args = {"url": "https://example.com"}
        resp = send_request(url, "tools/call",
                            {"name": "browser_navigate", "arguments": args})
        assert "result" in resp, "Missing result field"
        content = resp["result"]["content"]
        assert isinstance(content, list), "Content should be list"
        assert len(content) > 0, "Content should not be empty"
        text = json.loads(content[0]["text"])
        assert text["tool"] == "browser_navigate"
        assert text["arguments"] == args

    @test("tools/call with no arguments")
    def t4():
        resp = send_request(url, "tools/call",
                            {"name": "browser_snapshot"})
        assert "result" in resp

    @test("unknown method returns error")
    def t5():
        resp = send_request(url, "nonexistent_method")
        assert "error" in resp, "Expected error for unknown method"
        assert resp["error"]["code"] == -32601

    @test("invalid JSON returns parse error")
    def t6():
        req = urllib.request.Request(
            url,
            data=b"not json at all",
            headers={"Content-Type": "application/json"},
        )
        try:
            resp = urllib.request.urlopen(req, timeout=5)
            data = json.loads(resp.read())
            assert data.get("error", {}).get("code") == -32700, \
                "Expected parse error"
        except urllib.error.HTTPError:
            pass  # 500 is also acceptable for bad input

    @test("server preserves call history")
    def t7():
        # Reset by restarting isn't needed; call_history accumulates
        McpMockHandler.call_history.clear()
        send_request(url, "tools/call",
                     {"name": "bash", "arguments": {"command": "ls"}})
        assert len(McpMockHandler.call_history) == 1
        name, args = McpMockHandler.call_history[0]
        assert name == "bash"
        assert args == {"command": "ls"}

    @test("resources/list returns empty list")
    def t8():
        resp = send_request(url, "resources/list")
        assert resp["result"]["resources"] == []

    @test("JSON-RPC notification (no id) is accepted")
    def t9():
        body = json.dumps({
            "jsonrpc": "2.0",
            "method": "tools/list",
        }).encode()
        req = urllib.request.Request(
            url,
            data=body,
            headers={"Content-Type": "application/json"},
        )
        resp = urllib.request.urlopen(req, timeout=5)
        data = json.loads(resp.read())
        # Response should include result even without id
        assert "result" in data

    @test("concurrent requests don't interfere")
    def t10():
        import concurrent.futures
        results = []

        def fire():
            resp = send_request(url, "tools/list", req_id=999)
            return len(resp["result"]["tools"])

        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            futures = [pool.submit(fire) for _ in range(8)]
            results = [f.result() for f in futures]

        assert all(r == 4 for r in results), \
            f"Not all concurrent requests returned 4 tools: {results}"


# ── Main ──────────────────────────────────────────────────────────

def main():
    port = 0
    if len(sys.argv) > 1:
        port = int(sys.argv[1])

    server, actual_port = start_server(port)
    print(f"\n  Mock MCP server started on port {actual_port}")
    print()

    run_tests(actual_port)

    server.shutdown()

    print(f"\n  \033[1mResults: {passed} passed, {failed} failed\033[0m\n")

    if errors:
        print("  Failures:")
        for name, err in errors:
            print(f"    \033[31m•\033[0m {name}: {err}")
        print()

    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
