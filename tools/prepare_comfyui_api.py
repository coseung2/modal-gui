"""Convert a ComfyUI UI workflow into an API prompt without dropping widgets."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


IGNORED_TYPES = {
    "FancyTimerNode",
    "MarkdownNote",
    "Note",
    "BlockSparseAttention",
    "MiniMaxH3MemoryEfficientSageAttentionPatch",
    "MiniMaxChunkFeedForward",
}


def link_sources(workflow: dict[str, Any]) -> dict[int, tuple[str, int]]:
    sources: dict[int, tuple[str, int]] = {}
    for link in workflow.get("links", []):
        if len(link) >= 4:
            link_id, source_node, source_slot = int(link[0]), link[1], int(link[2])
            sources[link_id] = (str(source_node), source_slot)
    return sources


def convert(workflow: dict[str, Any], input_image: str | None = None) -> dict[str, Any]:
    sources = link_sources(workflow)
    prompt: dict[str, Any] = {}
    for node in workflow.get("nodes", []):
        node_type = node.get("type")
        if not node_type or node_type in IGNORED_TYPES or node.get("mode", 0) != 0:
            continue

        inputs: dict[str, Any] = {}
        for socket in node.get("inputs", []):
            link_id = socket.get("link")
            if link_id is None:
                continue
            source = sources.get(int(link_id))
            if source is not None:
                inputs[socket["name"]] = [source[0], source[1]]

        named = node.get("widgets_values_named") or {}
        if isinstance(named, dict):
            for name, value in named.items():
                if name not in inputs:
                    inputs[name] = value

        if input_image:
            if node_type == "LoadImage" and "image" in inputs:
                inputs["image"] = input_image
            if node_type == "DenoMiniMaxH3ReferenceImageLoader":
                inputs["image_paths"] = input_image

        prompt[str(node["id"])] = {"class_type": node_type, "inputs": inputs}
    return prompt


def missing_widget_candidates(prompt: dict[str, Any]) -> list[dict[str, str]]:
    missing: list[dict[str, str]] = []
    for node_id, node in prompt.items():
        if not node["inputs"]:
            missing.append({"node_id": node_id, "class_type": node["class_type"]})
    return missing


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("workflow", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--input-image")
    args = parser.parse_args()

    workflow = json.loads(args.workflow.read_text(encoding="utf-8"))
    prompt = convert(workflow, args.input_image)
    result = {
        "prompt": prompt,
        "workflow_source": str(args.workflow),
        "node_count": len(prompt),
        "missing_input_candidates": missing_widget_candidates(prompt),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "node_count": len(prompt), "missing_input_candidates": result["missing_input_candidates"]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
