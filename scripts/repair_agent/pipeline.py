"""新能源车维修助手 Agent 全流水线创建脚本。

步骤：
  1. 创建向量知识库 + 图知识库
  2. 爬取 dianchema.com 故障码页面并导入 RDF 三元组
  3. 注册专业 Skill
  4. 创建 Agent 并绑定图知识库
"""

import base64
import json
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed

import crawler  # 同目录的爬取模块

# ─── 配置 ────────────────────────────────────────────────────────────────────
API_BASE = "http://127.0.0.1:8080/api/v1"
# X-Identity：base64(JSON) 供开发模式模拟 DA 角色
_IDENT = base64.b64encode(json.dumps({
    "user_id": "pipeline-bot",
    "tenant_id": "default",
    "roles": ["DA", "USER"],
}).encode()).decode()

CRAWL_LIMIT = None   # None = 全量；调试时改小
BATCH_SIZE  = 50     # 每批 import 的页面数
WORKERS     = 10     # 并发爬取线程数
# graph:aps/… 被 expand_iri 展开为 http://aps.local/graph/ev-repair/fault-codes
# 该 http:// IRI 可被 Oxigraph SPARQL 正常解析
GRAPH_NAME  = "graph:aps/ev-repair/fault-codes"


# ─── HTTP 工具 ────────────────────────────────────────────────────────────────
def _req(method, path, body=None, *, da=False):
    url = API_BASE + path
    data = json.dumps(body).encode() if body is not None else None
    hdrs = {"Content-Type": "application/json", "Accept": "application/json"}
    if da:
        hdrs["X-Identity"] = _IDENT
    req = urllib.request.Request(url, data=data, headers=hdrs, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            body = r.read()
            return r.status, json.loads(body) if body.strip() else {}
    except urllib.error.HTTPError as e:
        body = e.read()
        try:
            return e.code, json.loads(body) if body.strip() else {"error": f"HTTP {e.code}"}
        except Exception:
            return e.code, {"error": body.decode("utf-8", "replace")[:300]}
    except Exception as exc:
        return 0, {"error": str(exc)}


def get(path):     return _req("GET",    path)
def post(path, b, *, da=False): return _req("POST", path, b, da=da)


# ─── Step 1：创建知识库 ────────────────────────────────────────────────────────
def create_knowledge_bases():
    print("\n=== Step 1: 创建知识库 ===")
    st, r = post("/kb/bases", {"name": "新能源车维修向量库",
                               "kb_type": "vector",
                               "description": "dianchema.com 故障码向量索引"}, da=True)
    vec_id = r.get("id", "")
    print(f"  向量库: HTTP {st}  id={vec_id}")

    st, r = post("/kb/bases", {"name": "新能源车维修知识图谱",
                               "kb_type": "graph",
                               "description": "dianchema.com 故障码 RDF 三元组"}, da=True)
    graph_id = r.get("id", "")
    graph_iri = r.get("graph", "")
    print(f"  图谱库: HTTP {st}  id={graph_id}  graph={graph_iri}")
    return vec_id, graph_id, graph_iri


# ─── Step 2：爬取 + 导入三元组 ────────────────────────────────────────────────
def rec_to_nodes_edges(rec):
    """将单条故障记录转换为 NodeDef 列表 + EdgeDef 列表。

    node_type 必须使用 aps: 前缀（expand_iri → http://aps.local/ontology/...）
    relation  同样使用 aps: 前缀，保证 Oxigraph 接受合法 IRI。
    """
    fid       = f"fault-{rec['brand_slug']}-{rec['code']}"
    brand_id  = f"brand-{rec['brand_slug']}"
    system_id = f"system-{rec['system_slug']}"

    nodes = [
        {"id": fid, "node_type": "aps:FaultCode", "label": rec["name"],
         "properties": {
             "code":      rec["code"],
             "brand":     rec["brand"],
             "system":    rec["system"],
             "meaning":   rec["meaning"],
             "can_drive": rec["can_drive"],
             "repair":    rec.get("repair", ""),
             "url":       rec["url"],
             "source":    rec["source"],
             "models":    json.dumps(rec["models"], ensure_ascii=False),
             "description": rec.get("description", ""),
         }},
        {"id": brand_id,  "node_type": "aps:Brand",  "label": rec["brand"],
         "properties": {"slug": rec["brand_slug"]}},
        {"id": system_id, "node_type": "aps:System", "label": rec["system"],
         "properties": {"slug": rec["system_slug"]}},
    ]
    edges = [
        {"source": fid, "target": brand_id,  "relation": "aps:belongsToBrand"},
        {"source": fid, "target": system_id, "relation": "aps:belongsToSystem"},
    ]
    for i, qa in enumerate(rec["qa"][:5]):  # 最多存 5 个 Q&A
        qid = f"qa-{rec['brand_slug']}-{rec['code']}-{i}"
        nodes.append({"id": qid, "node_type": "aps:QAPair",
                      "label": qa["q"][:80],
                      "properties": {"question": qa["q"], "answer": qa["a"]}})
        edges.append({"source": fid, "target": qid, "relation": "aps:hasQA"})
    return nodes, edges


def import_records(records, graph_iri, clear_first=True):
    print(f"\n=== Step 2: 导入三元组 (共 {len(records)} 条) ===")
    total_nodes = total_edges = ok_batches = 0
    batches = [records[i:i+BATCH_SIZE] for i in range(0, len(records), BATCH_SIZE)]

    for idx, batch in enumerate(batches):
        nodes, edges = [], []
        for rec in batch:
            ns, es = rec_to_nodes_edges(rec)
            nodes.extend(ns); edges.extend(es)
        payload = {"graph": graph_iri, "clear_before": clear_first and idx == 0,
                   "nodes": nodes, "edges": edges}
        st, r = post("/kg/import", payload, da=True)
        # 响应格式: {"entity_count":N, "relation_count":M, "quad_count":Q, "status":"ok"}
        total_nodes += r.get("entity_count", 0)
        total_edges += r.get("relation_count", 0)
        ok_batches += 1 if st in (200, 201) else 0
        sys.stdout.write(f"\r  批次 {idx+1}/{len(batches)}  HTTP {st}  "
                         f"nodes={total_nodes} edges={total_edges}      ")
        sys.stdout.flush()
    print(f"\n  完成: {ok_batches}/{len(batches)} 批次成功")
    return total_nodes, total_edges


# ─── Step 3：注册 Skill ───────────────────────────────────────────────────────
def _skill(iri, name, description, category, input_props=None):
    """构建合规的 SkillMeta 字典（含 input_schema / output_schema）。"""
    props = input_props or {"query": {"type": "string", "description": "用户查询"}}
    return {
        "skill_iri": iri, "name": name, "description": description,
        "version": "1.0.0", "category": category,
        "security_level": "public", "allowed_roles": ["USER", "DA"],
        "input_schema": {"type": "object", "properties": props, "required": list(props)},
        "output_schema": {"type": "object", "properties": {
            "result": {"type": "string"}}},
        "compiled_template": "", "input_mapping": {},
    }


SKILLS = [
    _skill("skill://ev-repair/fault-code-lookup",   "故障码查询",
           "根据故障码或描述，在知识图谱中检索新能源车故障码的含义、适用车型和处理建议。", "诊断"),
    _skill("skill://ev-repair/repair-suggestion",   "维修建议生成",
           "结合故障码信息和车型，利用 LLM 生成专业维修建议和安全注意事项。", "维修"),
    _skill("skill://ev-repair/can-drive-assessment", "行驶安全评估",
           "评估特定故障码出现后车辆是否可以继续行驶，给出安全等级建议。", "安全"),
    _skill("skill://ev-repair/brand-model-info",    "车型信息查询",
           "查询故障码适用的具体车型列表及品牌维修服务联系方式。", "信息"),
    _skill("skill://ev-repair/multi-brand-compare", "跨品牌故障对比",
           "对同类故障码在不同品牌（BYD/Tesla/NIO 等）之间进行横向对比分析。", "分析"),
]


def register_skills():
    print("\n=== Step 3: 注册专业 Skill ===")
    ok = 0
    for s in SKILLS:
        st, r = post("/skills", s, da=True)
        status = "✓" if st in (200, 201) else f"✗ HTTP {st}"
        print(f"  {status}  {s['name']}  ({s['skill_iri']})")
        if st in (200, 201):
            ok += 1
    print(f"  完成: {ok}/{len(SKILLS)} 个 Skill 注册成功")
    return [s["skill_iri"] for s in SKILLS]


# ─── Step 4：创建 Agent + 绑定知识图谱 ───────────────────────────────────────
def create_agent(skill_iris, graph_iri):
    print("\n=== Step 4: 创建 Agent ===")
    # 直接在创建时传入 knowledge_graph，避免 bind_graph 接口加错误的 tenant: 前缀
    st, r = post("/agents", {
        "name": "新能源车维修助手",
        "description": (
            "基于 dianchema.com 故障码库的专业新能源汽车维修问答助手，"
            "覆盖问界/比亚迪/特斯拉/蔚来/理想/小鹏等主流品牌，"
            "提供故障码解读、行驶安全评估和维修建议。"
        ),
        "business_domain": "新能源汽车维修",
        "skills": skill_iris,
        "knowledge_graph": GRAPH_NAME,   # graph:aps/... 格式
        "enabled": True,
    }, da=True)
    agent_id = r.get("id") or (r.get("agent") or {}).get("id", "")
    print(f"  创建 Agent: HTTP {st}  id={agent_id}")
    return agent_id


# ─── 主流程 ───────────────────────────────────────────────────────────────────
def main():
    t0 = time.time()
    print("=" * 60)
    print("  新能源车维修助手 Agent 全流水线")
    print("=" * 60)

    # Step 1
    _vec_id, _graph_id, graph_iri = create_knowledge_bases()
    if not graph_iri:
        # 图谱库可能已存在，查一下
        _, bases = get("/kb/bases")
        for b in bases.get("bases", []):
            if b.get("kb_type") == "graph" and "维修" in b.get("name", ""):
                graph_iri = b.get("graph", "")
                break
    if not graph_iri:
        graph_iri = GRAPH_NAME  # 降级：直接用 graph:aps/... 短名称
    print(f"  使用图谱 IRI: {graph_iri}")

    # Step 2: 爬取
    print(f"\n=== Step 2a: 爬取 dianchema.com (limit={CRAWL_LIMIT or '全量'}) ===")
    records = crawler.crawl(limit=CRAWL_LIMIT, workers=WORKERS)
    print(f"  成功抓取 {len(records)} 条故障记录")
    # 导入
    import_records(records, graph_iri)

    # Step 3
    skill_iris = register_skills()

    # Step 4
    agent_id = create_agent(skill_iris, graph_iri)

    elapsed = time.time() - t0
    print(f"\n{'='*60}")
    print(f"  全部完成！耗时 {elapsed:.1f}s")
    print(f"  Agent ID: {agent_id}")
    print(f"  故障记录: {len(records)} 条")
    print(f"  Skills  : {len(SKILLS)} 个")
    print(f"  图谱 IRI: {graph_iri}")
    print("=" * 60)


if __name__ == "__main__":
    main()
