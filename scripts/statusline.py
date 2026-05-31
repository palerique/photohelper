#!/usr/bin/env python3
import sys
import json
import os
import subprocess

def run_cmd(cmd, cwd=None):
    try:
        res = subprocess.run(cmd, shell=True, capture_output=True, text=True, check=True, cwd=cwd)
        return res.stdout.strip()
    except Exception:
        return None

def main():
    try:
        # Read the JSON payload from stdin
        payload_str = sys.stdin.read()
        if not payload_str:
            print("📁 photohelper | Antigravity: Wait...")
            return

        data = json.loads(payload_str)
    except Exception as e:
        print(f"📁 photohelper | Error: {str(e)}")
        return

    # Extract CWD and folder name
    cwd = data.get("cwd", os.getcwd())
    folder_name = os.path.basename(cwd) if cwd else "unknown"

    # Extract VCS (Git) information
    vcs = data.get("vcs") or {}
    branch = vcs.get("branch")

    sync_str = ""
    if branch:
        # Check ahead/behind tracking branch
        ahead = run_cmd("git rev-list --count @{u}..HEAD 2>/dev/null", cwd=cwd)
        behind = run_cmd("git rev-list --count HEAD..@{u} 2>/dev/null", cwd=cwd)

        ahead_count = int(ahead) if ahead and ahead.isdigit() else 0
        behind_count = int(behind) if behind and behind.isdigit() else 0

        if ahead_count > 0 or behind_count > 0:
            sync_str = f" (↑{ahead_count} ↓{behind_count})"
        else:
            sync_str = " (synced)"

        # Add dirty indicator
        if vcs.get("dirty"):
            sync_str += " *"
    else:
        branch = "no git"

    # Extract Context Window information
    context = data.get("context_window") or {}
    used_pct = context.get("used_percentage", 0.0)
    current_usage = context.get("current_usage") or {}
    input_tokens = current_usage.get("input_tokens", 0)
    output_tokens = current_usage.get("output_tokens", 0)
    total_tokens = input_tokens + output_tokens

    # Calculate USD Cost (using official rates: $1.25/1M input, $5.00/1M output)
    input_cost = (input_tokens / 1000000.0) * 1.25
    output_cost = (output_tokens / 1000000.0) * 5.00
    total_cost = input_cost + output_cost

    # Formatted parts
    # Color codes
    RESET = "\033[0m"
    BOLD = "\033[1m"
    GREEN = "\033[32m"
    YELLOW = "\033[33m"
    RED = "\033[31m"
    CYAN = "\033[36m"

    # Context token abbreviation (e.g., 576.7k)
    if total_tokens >= 1000000:
        token_str = f"{total_tokens / 1000000.0:.2f}M"
    elif total_tokens >= 1000:
        token_str = f"{total_tokens / 1000.0:.1f}k"
    else:
        token_str = str(total_tokens)

    # Format the segments
    folder_part = f"📁 {BOLD}{folder_name}{RESET}"
    branch_part = f"🌿 {GREEN}{branch}{RESET}{YELLOW}{sync_str}{RESET}"
    context_part = f"💬 {CYAN}{token_str}{RESET} ({used_pct:.1f}%)"
    cost_part = f"💰 {GREEN}${total_cost:.4f}{RESET}"

    # Join them nicely
    status_line = f" {folder_part}  |  {branch_part}  |  {context_part}  |  {cost_part} "

    # Render to stdout
    print(status_line)

if __name__ == "__main__":
    main()
