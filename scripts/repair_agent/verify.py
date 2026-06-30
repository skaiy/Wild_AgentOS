"""验证知识图谱数据导入结果，并演示问答能力。"""
import json
import pipeline

FULL_GRAPH = "tenant:default/kb/cbf58bb1-f09d-4256-a195-351f10172a90"


def sparql(q, named_graph=None):
    body = {"sparql": q}
    if named_graph:
        body["named_graph"] = named_graph
    st, r = pipeline.post("/kg/query", body)
    if st != 200:
        return [], f"HTTP {st}: {r}"
    results = r.get("results", [])
    return results, None


def main():
    print("=" * 60)
    print("  知识图谱验证 & 问答演示")
    print("=" * 60)

    # 1. 三元组总数
    rows, err = sparql(
        f"SELECT (COUNT(*) AS ?cnt) WHERE {{ GRAPH <{FULL_GRAPH}> {{ ?s ?p ?o }} }}"
    )
    cnt = rows[0].get("?cnt", "?") if rows else err
    print(f"\n✅ 三元组总数: {cnt}")

    # 2. 实体类型统计
    rows, err = sparql(f"""
        SELECT ?type (COUNT(?n) AS ?cnt) WHERE {{
            GRAPH <{FULL_GRAPH}> {{ ?n a ?type }}
        }} GROUP BY ?type ORDER BY DESC(?cnt)
    """)
    print("\n📊 实体类型统计:")
    for row in rows:
        print(f"  {row.get('?type','').split('/')[-1]:20s} {row.get('?cnt',''):>6s}")

    # 3. 故障码品牌分布
    rows, err = sparql(f"""
        SELECT ?brand (COUNT(?f) AS ?cnt) WHERE {{
            GRAPH <{FULL_GRAPH}> {{
                ?f a <http://aps.local/ontology/FaultCode> .
                ?f <http://aps.local/ontology/belongsToBrand> ?bn .
                ?bn <http://www.w3.org/2000/01/rdf-schema#label> ?brand .
            }}
        }} GROUP BY ?brand ORDER BY DESC(?cnt)
    """)
    print("\n🚗 故障码品牌分布:")
    for row in rows:
        print(f"  {row.get('?brand','?'):15s} {row.get('?cnt','?'):>5s} 条")

    # 4. 查询特斯拉 APP_w009
    rows, err = sparql(f"""
        SELECT ?label ?meaning ?can_drive ?models WHERE {{
            GRAPH <{FULL_GRAPH}> {{
                ?node a <http://aps.local/ontology/FaultCode> .
                ?node <https://agentos.ontology/meta/code> ?code .
                ?node <http://www.w3.org/2000/01/rdf-schema#label> ?label .
                ?node <https://agentos.ontology/meta/meaning> ?meaning .
                ?node <https://agentos.ontology/meta/can_drive> ?can_drive .
                ?node <https://agentos.ontology/meta/models> ?models .
                FILTER(CONTAINS(LCASE(str(?code)), "app_w009"))
            }}
        }} LIMIT 1
    """)
    print("\n🔍 问题: 特斯拉 APP_w009 故障码是什么意思？")
    if rows:
        row = rows[0]
        print(f"  名称: {row.get('?label','')}")
        print(f"  含义: {row.get('?meaning','')[:120]}")
        print(f"  行驶: {row.get('?can_drive','')[:100]}")
        print(f"  车型: {row.get('?models','')}")
    else:
        print("  (未找到，尝试宽泛搜索)")
        rows2, _ = sparql(f"""
            SELECT ?code ?label WHERE {{
                GRAPH <{FULL_GRAPH}> {{
                    ?n a <http://aps.local/ontology/FaultCode> .
                    ?n <https://agentos.ontology/meta/code> ?code .
                    ?n <http://www.w3.org/2000/01/rdf-schema#label> ?label .
                    FILTER(CONTAINS(LCASE(str(?code)), "app_w"))
                }}
            }} LIMIT 5
        """)
        for r in rows2:
            print(f"  {r.get('?code','?'):20s}  {r.get('?label','')[:60]}")

    # 5. 查询比亚迪 P0A80
    rows, err = sparql(f"""
        SELECT ?label ?meaning ?repair WHERE {{
            GRAPH <{FULL_GRAPH}> {{
                ?node a <http://aps.local/ontology/FaultCode> .
                ?node <https://agentos.ontology/meta/code> ?code .
                ?node <http://www.w3.org/2000/01/rdf-schema#label> ?label .
                ?node <https://agentos.ontology/meta/meaning> ?meaning .
                ?node <https://agentos.ontology/meta/repair> ?repair .
                FILTER(CONTAINS(LCASE(str(?code)), "p0a80"))
            }}
        }} LIMIT 1
    """)
    print("\n🔍 问题: 比亚迪 P0A80 故障码如何处理？")
    if rows:
        row = rows[0]
        print(f"  名称: {row.get('?label','')}")
        print(f"  含义: {row.get('?meaning','')[:100]}")
        print(f"  维修: {row.get('?repair','')[:100]}")
    else:
        print("  (未找到此故障码)")

    # 6. 查询蔚来行驶安全评估
    rows, err = sparql(f"""
        SELECT ?label ?can_drive WHERE {{
            GRAPH <{FULL_GRAPH}> {{
                ?node a <http://aps.local/ontology/FaultCode> .
                ?node <http://aps.local/ontology/belongsToBrand> ?bn .
                ?bn <http://www.w3.org/2000/01/rdf-schema#label> "蔚来" .
                ?node <http://www.w3.org/2000/01/rdf-schema#label> ?label .
                ?node <https://agentos.ontology/meta/can_drive> ?can_drive .
                FILTER(CONTAINS(?can_drive, "停车"))
            }}
        }} LIMIT 3
    """)
    print("\n🔍 问题: 蔚来哪些故障码需要立即停车？")
    for row in rows:
        print(f"  • {row.get('?label','')[:50]}")
        print(f"    → {row.get('?can_drive','')[:80]}")

    # 7. Agent 状态
    st, agents = pipeline.get("/agents")
    target = next((a for a in agents.get("agents", []) if "新能源" in a.get("name", "")), None)
    if target:
        print(f"\n🤖 Agent 已创建:")
        print(f"  名称: {target.get('name')}")
        print(f"  ID  : {target.get('id')}")
        print(f"  知识图谱: {target.get('knowledge_graph')}")
        print(f"  技能数量: {len(target.get('skills', []))}")
    print("\n" + "=" * 60)


if __name__ == "__main__":
    main()
