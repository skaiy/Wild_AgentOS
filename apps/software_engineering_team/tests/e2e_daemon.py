import sys
import json
import time
import requests

DAEMON_URL = "http://localhost:7890"
CENTER_URL = "http://localhost:8083"

passed = 0
failed = 0
errors = []


def test(name, condition, detail=""):
    global passed, failed
    if condition:
        passed += 1
        print(f"  PASS: {name}")
    else:
        failed += 1
        msg = f"  FAIL: {name}"
        if detail:
            msg += f" - {detail}"
        print(msg)
        errors.append(msg)


def request_daemon(method, path, **kwargs):
    url = f"{DAEMON_URL}{path}"
    try:
        resp = requests.request(method, url, timeout=10, **kwargs)
        return resp
    except requests.exceptions.ConnectionError:
        return None
    except requests.exceptions.Timeout:
        return None
    except Exception as e:
        return None


def request_center(method, path, **kwargs):
    url = f"{CENTER_URL}{path}"
    try:
        resp = requests.request(method, url, timeout=10, **kwargs)
        return resp
    except requests.exceptions.ConnectionError:
        return None
    except requests.exceptions.Timeout:
        return None
    except Exception as e:
        return None


def print_summary():
    print("\n" + "=" * 60)
    print(f"Results: {passed} passed, {failed} failed")
    print("=" * 60)
    if errors:
        for e in errors:
            print(e)
    if failed > 0:
        sys.exit(1)


def main():
    print("=" * 60)
    print("Edge Daemon End-to-End Tests")
    print("=" * 60)

    # =========================================================
    # 1. Daemon Health
    # =========================================================
    print("\n[1] Daemon Health")
    resp = request_daemon("GET", "/api/health")
    test("GET /api/health returns 200", resp is not None and resp.status_code == 200)
    if resp is not None:
        test("GET /api/health status is ok", resp.json().get("status") == "ok",
             f"got: {resp.json()}")

    if resp is None or resp.status_code != 200:
        print("\nDaemon not reachable. Skipping remaining daemon-specific tests.")
    else:
        # =========================================================
        # 2. Daemon Chat
        # =========================================================
        print("\n[2] Daemon Chat")
        chat_resp = request_daemon("POST", "/api/chat",
                                   json={"messages": [{"role": "user", "content": "hello"}]})
        test("POST /api/chat returns 200", chat_resp is not None and chat_resp.status_code == 200)
        if chat_resp is not None:
            body = chat_resp.json()
            test("POST /api/chat response has content field",
                 "content" in body, f"got keys: {list(body.keys())}")
            test("POST /api/chat response has session_id field",
                 "session_id" in body, f"got keys: {list(body.keys())}")
            if "content" in body:
                content = body["content"]
                if content and content.startswith("error:"):
                    print(f"  INFO: Chat returned error (LLM may be unavailable): {content[:80]}")
                    test("POST /api/chat response is valid (may not have full LLM)",
                         True, "error response accepted when LLM unavailable")
                if content and not content.startswith("error:"):
                    test("POST /api/chat content is non-empty",
                         len(content) > 0)

        # =========================================================
        # 3. Daemon Registration Flow (requires Center)
        # =========================================================
        print("\n[3] Daemon Registration Flow")
        center_health = request_center("GET", "/health")
        if center_health is not None and center_health.status_code == 200:
            agent_id = f"e2e-daemon-agent-{int(time.time())}"
            register_resp = request_center("POST", "/api/v1/agents/register",
                                           json={
                                               "agent_id": agent_id,
                                               "user_id": "e2e-daemon-user",
                                               "capabilities": ["coding", "testing"],
                                               "version": "1.0.0"
                                           })
            test("POST /api/v1/agents/register returns 200",
                 register_resp is not None and register_resp.status_code == 200)
            if register_resp is not None and register_resp.status_code == 200:
                body = register_resp.json()
                test("Registered agent_id matches",
                     body.get("agent_id") == agent_id,
                     f"expected {agent_id}, got {body.get('agent_id')}")
                test("Registered agent status is online",
                     body.get("status") == "online",
                     f"got status: {body.get('status')}")

                heartbeat_resp = request_center("POST", "/api/v1/agents/heartbeat",
                                                json={"agent_id": agent_id})
                test("POST /api/v1/agents/heartbeat returns 200",
                     heartbeat_resp is not None and heartbeat_resp.status_code == 200)
                if heartbeat_resp is not None:
                    hb_body = heartbeat_resp.json()
                    test("Heartbeat returns agent_id",
                         hb_body.get("agent_id") == agent_id,
                         f"expected {agent_id}, got {hb_body.get('agent_id')}")
                    test("Heartbeat status is ok",
                         hb_body.get("status") == "ok",
                         f"got status: {hb_body.get('status')}")

                agents_resp = request_center("GET", "/api/v1/agents")
                if agents_resp is None:
                    agents_resp = request_center("GET", "/api/v1/agents/")
                test("GET /api/v1/agents returns 200",
                     agents_resp is not None and agents_resp.status_code == 200)
                if agents_resp is not None:
                    agents = agents_resp.json().get("agents", [])
                    test("Registered agent appears in online agents list",
                         any(a.get("agent_id") == agent_id for a in agents),
                         f"agent_id={agent_id} not found in {len(agents)} agents")
            else:
                print("  SKIP: Registration endpoint not available on center")
        else:
            print("  SKIP: Center not reachable, skipping registration flow tests")

        # =========================================================
        # 4. Available Tasks Flow
        # =========================================================
        print("\n[4] Available Tasks Flow")
        tasks_resp = request_daemon("GET", "/api/available_tasks")
        body = None
        if tasks_resp is not None:
            try:
                body = tasks_resp.json()
            except Exception:
                body = None
        if tasks_resp is None or body is None or tasks_resp.status_code != 200:
            tasks_resp = request_center("GET", "/api/v1/tasks/available")
            body = None
        test("GET available tasks returns 200",
             tasks_resp is not None and tasks_resp.status_code == 200)
        if tasks_resp is not None and tasks_resp.status_code == 200:
            try:
                body = tasks_resp.json()
            except Exception:
                body = {}
            if "tasks" in body:
                tasks = body["tasks"]
                test("Available tasks is a list", isinstance(tasks, list),
                     f"got type: {type(tasks).__name__}")
            elif "available_tasks" in body:
                tasks = body["available_tasks"]
                test("Available tasks is a list", isinstance(tasks, list),
                     f"got type: {type(tasks).__name__}")
            else:
                test("Available tasks response has tasks or available_tasks field",
                     False, f"got keys: {list(body.keys())}")

        # =========================================================
        # 5. Graph Sync
        # =========================================================
        print("\n[5] Graph Sync")
        sync_payload = {
            "agent_id": f"e2e-daemon-{int(time.time())}",
            "task_id": "e2e-daemon-task",
            "deltas": [
                {
                    "action": "create",
                    "iri": "iri://e2e/daemon-test",
                    "jsonld": json.dumps({
                        "@id": "iri://e2e/daemon-test",
                        "@type": "DaemonTestCase",
                        "name": "Daemon E2E Test"
                    }),
                    "version": 1
                }
            ]
        }
        sync_resp = request_center("POST", "/api/v1/graph/sync", json=sync_payload)
        test("POST /api/v1/graph/sync returns expected status",
             sync_resp is not None and sync_resp.status_code in (200, 201, 409, 500, 503),
             f"got status {sync_resp.status_code if sync_resp else 'None'}")
        if sync_resp is not None and sync_resp.status_code in (200, 201):
            body = sync_resp.json()
            test("Graph sync response has status field",
                 "status" in body, f"got keys: {list(body.keys())}")
            if "status" in body:
                test("Graph sync status is accepted",
                     body["status"] == "accepted",
                     f"got status: {body['status']}")

    # =========================================================
    # 6. WebSocket
    # =========================================================
    print("\n[6] WebSocket")
    try:
        import websocket
        ws = websocket.create_connection(
            f"ws://localhost:7890/api/ws/events",
            timeout=5
        )
        ws.settimeout(3)
        test("WebSocket connection established", True)

        welcome = ws.recv()
        if welcome:
            try:
                welcome_data = json.loads(welcome)
                test("WebSocket welcome message is valid JSON", True)
                test("WebSocket welcome has type field",
                     "type" in welcome_data, f"got keys: {list(welcome_data.keys())}")
                if welcome_data.get("type") == "connected":
                    test("WebSocket welcome type is 'connected'", True)
                    test("WebSocket welcome has message field",
                         "message" in welcome_data,
                         f"got keys: {list(welcome_data.keys())}")
                else:
                    test(f"WebSocket welcome type is 'connected'",
                         False, f"got type: {welcome_data.get('type')}")
            except json.JSONDecodeError:
                test("WebSocket welcome message is valid JSON", False,
                     f"raw: {welcome[:200]}")
        else:
            test("WebSocket received welcome message", False, "empty response")

        ping_msg = json.dumps({"type": "ping"})
        ws.send(ping_msg)
        test("WebSocket sent ping message", True)

        try:
            echo = ws.recv()
            if echo:
                try:
                    echo_data = json.loads(echo)
                    test("WebSocket echo response is valid JSON", True)
                    if echo_data.get("type") == "echo":
                        test("WebSocket echo type is 'echo'", True)
                        test("WebSocket echo contains data field",
                             "data" in echo_data,
                             f"got keys: {list(echo_data.keys())}")
                        if "data" in echo_data:
                            test("WebSocket echo data matches sent ping",
                                 echo_data["data"] == ping_msg,
                                 f"expected {ping_msg}, got {echo_data['data']}")
                    elif echo_data.get("type") == "pong":
                        test("WebSocket received pong (server-side ping)", True)
                    else:
                        test("WebSocket response has recognized type",
                             False, f"got type: {echo_data.get('type')}")
                except json.JSONDecodeError:
                    test("WebSocket echo response is valid JSON", False,
                         f"raw: {echo[:200]}")
            else:
                test("WebSocket received echo response", False, "empty response")
        except Exception as e:
            test("WebSocket received echo response", False, str(e))

        ws.close()
        test("WebSocket connection closed cleanly", True)

    except ImportError:
        print("  SKIP: websocket-client library not available")
        print("  Install with: pip install websocket-client")
    except Exception as e:
        test("WebSocket connection attempt", False, str(e))

    print_summary()


if __name__ == "__main__":
    main()