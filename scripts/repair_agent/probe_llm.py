"""探测 LLM 网关正确的 chat/completions 路径（不打印密钥）。"""
import json
import urllib.request
import urllib.error
from pathlib import Path

CFG = Path(__file__).resolve().parents[2] / "data" / "config_override.json"


def load():
    g = json.loads(CFG.read_text())["gateway"]
    return g["api_key"], g["base_url"].rstrip("/"), g["default_model"]


def probe(url, key, model):
    body = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 8,
    }).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    req.add_header("Authorization", f"Bearer {key}")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            txt = r.read().decode()[:160]
            return r.status, txt
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()[:160]
    except Exception as e:  # noqa
        return -1, str(e)[:160]


def main():
    key, base, model = load()
    # base 可能含 /v1，去掉得到根
    root = base[:-3].rstrip("/") if base.endswith("/v1") else base
    candidates = {
        f"{base}/v1/chat/completions": "as-configured + /v1 (current code path)",
        f"{root}/v1/chat/completions": "root + /v1 (single /v1)",
        f"{base}/chat/completions": "base + /chat/completions",
    }
    print(f"model={model}  base={base}")
    for url, desc in candidates.items():
        st, snip = probe(url, key, model)
        ok = "OK" if st == 200 else "  "
        print(f"[{ok}] {st}  {desc}\n       {url}\n       {snip}\n")


if __name__ == "__main__":
    main()
