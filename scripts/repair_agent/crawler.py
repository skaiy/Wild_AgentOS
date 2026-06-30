"""dianchema.com 递归爬取与 JSON-LD 抽取。

仅使用标准库（urllib / re / json），不引入第三方依赖。
抽取每个故障码页面的：故障名称、含义、能否继续行驶、适用车型、
来源、关键词、原始 Q&A，并归一化为统一的结构化字典。
"""
import json
import re
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed

SITE = "https://dianchema.com"
SITEMAP = f"{SITE}/sitemap-0.xml"
UA = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/126.0 Safari/537.36")

# 系统 slug → 中文名（缺失时回退 slug）
SYSTEM_CN = {
    "adas": "驾驶辅助", "battery": "动力电池", "motor": "驱动电机",
    "charge": "充电系统", "brake": "制动系统", "tyre": "胎压监测",
    "body": "车身电子", "climate": "空调热管理", "light": "灯光系统",
    "power": "高压系统", "engine": "发动机", "transmission": "变速器",
}

_JSONLD_RE = re.compile(
    r'<script type="application/ld\+json">(.*?)</script>', re.S)


def _fetch(url, timeout=25):
    req = urllib.request.Request(url, headers={"User-Agent": UA,
                                               "Accept-Encoding": "identity"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read().decode("utf-8", "replace")


def list_fault_urls(limit=None):
    """从 sitemap 读取所有 /{brand}/{system}/{code}/ 叶子故障页 URL。"""
    xml = _fetch(SITEMAP)
    urls = re.findall(r"<loc>([^<]+)</loc>", xml)
    faults = []
    for u in urls:
        path = u[len(SITE):].strip("/")
        if path.count("/") == 2 and path:  # brand/system/code
            faults.append(u)
    faults.sort()
    return faults[:limit] if limit else faults


def _graph_nodes(data):
    g = data.get("@graph", data if isinstance(data, list) else [data])
    return g if isinstance(g, list) else [g]


def _find_type(nodes, t):
    for n in nodes:
        nt = n.get("@type")
        if nt == t or (isinstance(nt, list) and t in nt):
            return n
    return None


def _breadcrumb(nodes):
    bc = _find_type(nodes, "BreadcrumbList") or {}
    out = []
    for it in bc.get("itemListElement", []) or []:
        item = it.get("item")
        name = item.get("name") if isinstance(item, dict) else it.get("name")
        if name:
            out.append(name)
    return out


_BRAND_STRIP = ("DTC 故障码", "故障码查询", "故障码", "报警灯查询", "报警灯",
                "警告灯查询", "警告灯", "查询")


def _clean_brand(label):
    s = (label or "").strip()
    for suf in _BRAND_STRIP:
        if s.endswith(suf):
            s = s[: -len(suf)].strip()
    return s or label


def extract_page(url):
    """抓取单页并抽取结构化故障知识；失败返回 None。"""
    try:
        html = _fetch(url)
    except Exception as e:  # noqa: BLE001
        return {"url": url, "error": str(e)}
    blocks = _JSONLD_RE.findall(html)
    if not blocks:
        return {"url": url, "error": "no jsonld"}
    try:
        data = json.loads(blocks[0])
    except json.JSONDecodeError:
        return {"url": url, "error": "bad jsonld"}
    nodes = _graph_nodes(data)
    path = url[len(SITE):].strip("/").split("/")
    brand_slug, system_slug, code = path[0], path[1], path[2]

    faq = _find_type(nodes, "FAQPage") or {}
    article = _find_type(nodes, "TechArticle") or {}
    about = article.get("about", {}) if isinstance(article, dict) else {}

    qa = []
    for item in faq.get("mainEntity", []) or []:
        q = item.get("name", "")
        a = (item.get("acceptedAnswer") or {}).get("text", "")
        if q and a:
            qa.append({"q": q, "a": a})
    # 仅保留含真实 Q&A 的故障知识页，跳过分类索引页。
    if not qa:
        return {"url": url, "error": "index page (no FAQ)"}

    meaning = next((x["a"] for x in qa if "意思" in x["q"] or "含义" in x["q"]), "")
    can_drive = next((x["a"] for x in qa if "继续开" in x["q"] or "行驶" in x["q"]), "")
    models_ans = next((x["a"] for x in qa if "车型" in x["q"]), "")
    repair = next((x["a"] for x in qa if "怎么" in x["q"] or "如何" in x["q"]
                   or "处理" in x["q"] or "解决" in x["q"] or "维修" in x["q"]), "")

    crumbs = _breadcrumb(nodes)
    brand = _clean_brand(crumbs[1]) if len(crumbs) > 1 else brand_slug
    system = crumbs[2] if len(crumbs) > 2 else SYSTEM_CN.get(system_slug, system_slug)
    name = article.get("headline") or (crumbs[-1] if crumbs else code)
    desc = about.get("description") or article.get("description") or meaning
    kws = article.get("keywords") or []
    if isinstance(kws, str):
        kws = [kws]
    kws = [k for k in kws if k]

    # 适用车型：合并关键词与车型答案中的型号词。
    pool = " ".join(kws) + " " + models_ans
    models = re.findall(r"Model\s?[0-9A-Za-z]+|[A-Za-z]{1,4}\s?\d{1,3}[A-Za-z+]*", pool)
    models = sorted({m.strip() for m in models if len(m.strip()) >= 2})

    return {
        "url": url, "code": code,
        "brand_slug": brand_slug, "brand": brand,
        "system_slug": system_slug, "system": system,
        "name": name, "description": desc,
        "meaning": meaning, "can_drive": can_drive, "repair": repair,
        "models": models, "models_text": models_ans,
        "source": (about.get("inDefinedTermSet") or
                   (article.get("publisher") or {}).get("name") or "电车码"),
        "keywords": kws, "qa": qa,
    }


def crawl(limit=120, workers=8, delay=0.0):
    """并发爬取前 limit 个故障页，返回成功抽取的记录列表。"""
    urls = list_fault_urls(limit)
    out = []
    with ThreadPoolExecutor(max_workers=workers) as ex:
        futs = {ex.submit(extract_page, u): u for u in urls}
        for i, fut in enumerate(as_completed(futs), 1):
            rec = fut.result()
            if rec and not rec.get("error"):
                out.append(rec)
            if delay:
                time.sleep(delay)
    return out
