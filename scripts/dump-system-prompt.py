#!/usr/bin/env python3
"""Dump the full system prompt and tool descriptions as the LLM sees them.

Usage:
    python3 scripts/dump-system-prompt.py [--provider anthropic|openai|gemini] [--all-tools]

Outputs:
    - system-prompt-dump.txt: Human-readable dump of prompt + tool descriptions
    - system-prompt-dump.json: Machine-parseable JSON with prompt text + tools array

The script reads source files directly — no Node.js runtime needed.
Placeholders like {{OS}}, {{SHELL}} are filled with sensible defaults.
Dynamic sections (skills, custom rules) are noted as [PLACEHOLDER].
"""

import argparse
import json
import os
import re
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOOLS_DIR = os.path.join(REPO_ROOT, "src/core/prompts/system-prompt/tools")
TEMPLATE_PATH = os.path.join(REPO_ROOT, "src/core/prompts/system-prompt/template.ts")
SPEC_PATH = os.path.join(REPO_ROOT, "src/core/prompts/system-prompt/spec.ts")


def extract_template_sections(template_src: str) -> str:
    """Extract the system prompt body from template.ts, resolving placeholders."""
    # Find the return statement content
    match = re.search(r'return\s*`([\s\S]*?)`\s*\}', template_src)
    if not match:
        return "(could not extract template)"

    prompt = match.group(1)

    # Resolve static placeholders
    replacements = {
        "{{OS}}": "linux",
        "{{SHELL}}": "/bin/bash",
        "{{SHELL_TYPE}}": "bash",
        "{{AVAILABLE_CORES}}": "8",
        "{{BROWSER_VIEWPORT_WIDTH}}": "1280",
        "{{BROWSER_VIEWPORT_HEIGHT}}": "720",
        "{{SKILLS_SECTION}}": "[No skills configured]",
    }
    for placeholder, value in replacements.items():
        prompt = prompt.replace(placeholder, value)

    # Handle ${...} template expressions — show as [conditional: description]
    # Replace ternary expressions
    prompt = re.sub(
        r'\$\{\s*(\w+)\s*\?\s*"([^"]*?)"\s*:\s*""\s*\}',
        lambda m: m.group(2) if m.group(1) in ("enableParallelToolCalling",) else f'[if {m.group(1)}: {m.group(2)}]',
        prompt
    )
    # Replace simple ${variable} references
    prompt = re.sub(r'\$\{\s*currentCwd\s*\}', '/path/to/workspace', prompt)
    prompt = re.sub(r'\$\{\s*\w+\s*\}', '[dynamic]', prompt)

    # Clean up JS template expressions that are more complex
    # Replace ternaries with strings
    prompt = re.sub(
        r'\$\{[^}]*\?\s*"([^"]*)"[^}]*\}',
        lambda m: m.group(1),
        prompt
    )
    # Remaining complex expressions
    prompt = re.sub(r'\$\{[\s\S]*?\}', '[dynamic]', prompt)

    # Remove leading/trailing whitespace from the template literal interpolation
    prompt = prompt.strip()
    return prompt


def extract_tool_description(content: str) -> str:
    """Extract description field from a tool spec, handling escaped backticks."""
    match = re.search(r'description:\s*`((?:[^`\\]|\\.)*)`\s*,', content)
    if not match:
        return "(no description)"
    desc = match.group(1)
    # Unescape
    desc = desc.replace(r'\`', '`')
    desc = desc.replace(r'\${', '${')
    return desc.strip()


def extract_tool_name(content: str) -> str:
    """Extract the tool name."""
    match = re.search(r'name:\s*["\'](\w+)["\']', content)
    return match.group(1) if match else "(unknown)"


def extract_context_gate(content: str) -> str:
    """Extract contextRequirements if present."""
    match = re.search(r'contextRequirements:\s*\((\w+)\)\s*=>\s*(.+?)(?:,\s*parameters|\s*\})', content, re.DOTALL)
    if match:
        return match.group(2).strip().rstrip(',')
    return "none"


def extract_parameters(content: str) -> list:
    """Extract parameter definitions from a tool spec."""
    params = []
    param_block = re.search(r'parameters:\s*\[([\s\S]*?)\]', content)
    if not param_block:
        return params

    block = param_block.group(1)
    # Match individual parameter objects
    for pmatch in re.finditer(r'\{\s*name:\s*"([^"]+)"', block):
        name = pmatch.group(1)
        # Get the rest of the param object
        rest_start = pmatch.end()
        rest = block[rest_start:block.find('}', rest_start) + 1]

        required = '"required"' in rest and 'true' in rest[:rest.find('required') + 30]
        type_match = re.search(r'type:\s*"([^"]+)"', rest)
        instruction_match = re.search(r'instruction:\s*"([^"]*)"', rest)
        usage_match = re.search(r'usage:\s*"([^"]*)"', rest)

        params.append({
            "name": name,
            "required": "required: true" in rest,
            "type": type_match.group(1) if type_match else "string",
            "instruction": instruction_match.group(1) if instruction_match else "",
            "usage": usage_match.group(1) if usage_match else "",
        })
    return params


def format_anthropic_tool(name: str, description: str, parameters: list) -> dict:
    """Format as Anthropic tool definition."""
    props = {}
    required = []
    for p in parameters:
        props[p["name"]] = {
            "type": p["type"],
            "description": p["instruction"] or p["usage"] or p["name"],
        }
        if p["required"]:
            required.append(p["name"])

    return {
        "name": name,
        "description": description,
        "input_schema": {
            "type": "object",
            "properties": props,
            "required": required,
        }
    }


def format_openai_tool(name: str, description: str, parameters: list) -> dict:
    """Format as OpenAI tool definition."""
    props = {}
    required = []
    for p in parameters:
        props[p["name"]] = {
            "type": p["type"],
            "description": p["instruction"] or p["usage"] or p["name"],
        }
        if p["required"]:
            required.append(p["name"])

    return {
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": props,
                "required": required,
            }
        }
    }


def should_include_tool(gate: str, include_all: bool) -> bool:
    """Check if a tool should be included based on its context gate."""
    if include_all or gate == "none":
        return True
    return False


def main():
    parser = argparse.ArgumentParser(description="Dump system prompt + tool descriptions as the LLM sees them")
    parser.add_argument("--provider", choices=["anthropic", "openai", "gemini"], default="anthropic",
                        help="API format for tool definitions (default: anthropic)")
    parser.add_argument("--all-tools", action="store_true",
                        help="Include all tools, even feature-gated ones")
    parser.add_argument("--json", action="store_true",
                        help="Output only JSON to stdout")
    args = parser.parse_args()

    # Read template
    with open(TEMPLATE_PATH, 'r') as f:
        template_src = f.read()
    prompt_text = extract_template_sections(template_src)

    # Collect tools
    tool_files = sorted(f for f in os.listdir(TOOLS_DIR) if f.endswith('.ts') and f not in ('index.ts', 'init.ts'))

    tools = []
    skipped = []
    for fname in tool_files:
        filepath = os.path.join(TOOLS_DIR, fname)
        with open(filepath, 'r') as f:
            content = f.read()

        name = extract_tool_name(content)
        desc = extract_tool_description(content)
        gate = extract_context_gate(content)
        params = extract_parameters(content)

        if should_include_tool(gate, args.all_tools):
            if args.provider == "openai":
                tool_def = format_openai_tool(name, desc, params)
            else:
                tool_def = format_anthropic_tool(name, desc, params)
            tools.append(tool_def)
        else:
            skipped.append({"name": name, "gate": gate})

    result = {
        "system_prompt": prompt_text,
        "tools": tools,
        "skipped_tools": skipped,
        "provider_format": args.provider,
        "note": "Generated by scripts/dump-system-prompt.py — reads source files directly, does not execute TypeScript. Dynamic placeholders are filled with defaults."
    }

    if args.json:
        print(json.dumps(result, indent=2))
        return

    # Human-readable output
    out_path = os.path.join(REPO_ROOT, "system-prompt-dump.txt")
    json_path = os.path.join(REPO_ROOT, "system-prompt-dump.json")

    lines = []
    lines.append("=" * 70)
    lines.append("SYSTEM PROMPT — as the LLM sees it")
    lines.append(f"Provider format: {args.provider}")
    lines.append(f"Tools: {len(tools)} enabled, {len(skipped)} gated")
    lines.append("=" * 70)
    lines.append("")
    lines.append("SECTION 1: SYSTEM PROMPT TEXT (passed as `system` parameter)")
    lines.append("-" * 70)
    lines.append(prompt_text)
    lines.append("")
    lines.append("=" * 70)
    lines.append(f"SECTION 2: TOOLS ARRAY ({len(tools)} tools, passed as `tools` parameter)")
    lines.append("=" * 70)

    for tool in tools:
        name = tool.get("name") or tool.get("function", {}).get("name", "?")
        lines.append("")
        lines.append(f"--- {name} ---")
        lines.append(tool.get("description") or tool.get("function", {}).get("description", ""))
        if "input_schema" in tool:
            schema = tool["input_schema"]
            lines.append(f"  Parameters: {json.dumps(schema.get('properties', {}), indent=4)}")
            if schema.get("required"):
                lines.append(f"  Required: {schema['required']}")
        elif "function" in tool:
            params = tool["function"].get("parameters", {})
            lines.append(f"  Parameters: {json.dumps(params.get('properties', {}), indent=4)}")
            if params.get("required"):
                lines.append(f"  Required: {params['required']}")

    if skipped:
        lines.append("")
        lines.append("=" * 70)
        lines.append(f"GATED TOOLS (not included — require feature flags)")
        lines.append("=" * 70)
        for s in skipped:
            lines.append(f"  {s['name']}: gate={s['gate']}")

    text_output = "\n".join(lines)

    with open(out_path, 'w') as f:
        f.write(text_output)
    with open(json_path, 'w') as f:
        json.dump(result, f, indent=2, ensure_ascii=False)

    print(f"Dumped to {out_path} ({len(lines)} lines)")
    print(f"         {json_path} ({len(tools)} tools)")


if __name__ == "__main__":
    main()
