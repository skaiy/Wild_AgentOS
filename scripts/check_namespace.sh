#!/usr/bin/env bash
# 命名空间一致性检查。
#
# 背景：仓库历史上并存过 pdca-agent.org / agent-os.org / agent-harness.os /
# wild-agent-os.org 四套本体命名空间，同一概念对应多个 IRI，导致跨图 SPARQL
# join 静默失败。全部统一到 https://wildagentos.org 后，用本脚本防止回潮。
#
# 退出码：0 通过，1 发现已废弃命名空间。

set -uo pipefail
cd "$(dirname "$0")/.."

SCAN=(--include=*.rs --include=*.jsonld --include=*.md)
ROOTS=(src apps tests skills docs)
fail=0

# ── 1. 已废弃命名空间：硬失败 ──────────────────────────────────────────
LEGACY='pdca-agent\.org|agent-harness\.os|wild-agent-os\.org|agent-os\.org'
if hits=$(grep -rnE "$LEGACY" "${SCAN[@]}" "${ROOTS[@]}" 2>/dev/null); then
    echo "✗ 发现已废弃的命名空间，请统一到 https://wildagentos.org"
    echo "$hits"
    fail=1
fi

# ── 2. opaque `<skill:x>` 写入谓词：硬失败 ─────────────────────────────
# 尖括号会让 SPARQL 把 skill: 当作 opaque URI scheme 原样存储，谓词无法与
# 其他命名图 join。正确写法是 PREFIX 声明 + 裸 skill:x。
# 只查 INSERT/CONSTRUCT 等写入语句；断言旧格式已消失的回归测试会用 SELECT
# 查询 opaque 形态，那是有意为之，不应拦截。
if hits=$(grep -rnE '(INSERT|CONSTRUCT|DELETE).*<(skill|task|exec|agent|mem):[a-zA-Z]' \
    --include=*.rs "${ROOTS[@]}" 2>/dev/null); then
    echo "✗ 写入语句中发现尖括号包裹的 opaque IRI，应改用 PREFIX 声明 + 裸前缀写法"
    echo "$hits"
    fail=1
fi

# ── 3. 未收敛的本体 host：仅告警 ───────────────────────────────────────
# 除官方 wildagentos.org 与领域词汇 aps.local 外，其余 host 多为增量开发时
# 现造的命名空间。列出以便人工判断，暂不阻断构建。
unknown=$(grep -rhoE 'https?://[a-zA-Z0-9.-]+/(ontology|prop|type|context|vocab|graph|share|methodology|schema)' \
    "${SCAN[@]}" "${ROOTS[@]}" 2>/dev/null |
    sed -E 's|https?://([^/]+)/.*|\1|' |
    grep -vE '^(wildagentos\.org|aps\.local|www\.w3\.org|schema\.org|test)$' |
    sort | uniq -c | sort -rn)
if [ -n "$unknown" ]; then
    echo "⚠ 以下本体 host 未收敛到 wildagentos.org，请确认是否为有意设计："
    echo "$unknown"
fi

[ "$fail" -eq 0 ] && echo "✓ 命名空间检查通过"
exit "$fail"
