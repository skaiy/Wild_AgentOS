#!/usr/bin/env python3
"""
parse_csv.py — 解析 CSV 文件并计算数值列的统计信息，输出为 JSON。

用法:
    python parse_csv.py [--input <csv_path>] [--output <json_path>]

默认:
    input  = test.csv
    output = result.json
"""

import csv
import json
import sys
import argparse
from pathlib import Path
from typing import Dict, List, Union


def parse_numeric(value: str) -> Union[int, float, None]:
    """尝试将字符串转为 int 或 float，失败返回 None。"""
    v = value.strip()
    if not v:
        return None
    try:
        if "." in v:
            return float(v)
        return int(v)
    except ValueError:
        return None


def compute_stats(values: List[Union[int, float]]) -> Dict[str, Union[float, int, None]]:
    """计算数值列表的统计信息。"""
    if not values:
        return {"min": None, "max": None, "sum": None, "avg": None, "count": 0}

    n = len(values)
    total = sum(values)
    min_val = min(values)
    max_val = max(values)
    avg = total / n
    return {
        "min": min_val,
        "max": max_val,
        "sum": total,
        "avg": round(avg, 2),
        "count": n,
    }


def parse_csv(input_path: str) -> Dict[str, Dict]:
    """读取 CSV 并计算每列统计。"""
    with open(input_path, mode="r", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        rows = list(reader)

    if not rows:
        return {"columns": {}, "total_rows": 0}

    columns = {}
    col_names = list(rows[0].keys())
    total_rows = len(rows)

    for col in col_names:
        numeric_values = []
        for row in rows:
            val = parse_numeric(row[col])
            if val is not None:
                numeric_values.append(val)

        columns[col] = compute_stats(numeric_values)
        columns[col]["type"] = "numeric" if numeric_values else "non-numeric"

    return {"columns": columns, "total_rows": total_rows}


def main():
    parser = argparse.ArgumentParser(description="CSV 统计解析器")
    parser.add_argument("--input", default="test.csv", help="CSV 文件路径")
    parser.add_argument("--output", default="result.json", help="输出 JSON 路径")
    args = parser.parse_args()

    input_path = Path(args.input)
    output_path = Path(args.output)

    if not input_path.exists():
        print(f"Error: 文件不存在 — {input_path}", file=sys.stderr)
        sys.exit(1)

    result = parse_csv(str(input_path))

    with open(output_path, mode="w", encoding="utf-8") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)

    print(f"✓ 已解析 {input_path}")
    print(f"  行数: {result['total_rows']}")
    print(f"  列数: {len(result['columns'])}")
    for col, stats in result["columns"].items():
        if stats["type"] == "numeric":
            print(f"    {col}: sum={stats['sum']}, avg={stats['avg']}, "
                  f"min={stats['min']}, max={stats['max']}, count={stats['count']}")
        else:
            print(f"    {col}: <非数值列>")
    print(f"✓ 已输出到 {output_path}")


if __name__ == "__main__":
    main()
