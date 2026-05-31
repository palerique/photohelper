#!/usr/bin/env python3
import os
import sys
import glob
import json
import subprocess

# Terminal Color Codes
BOLD = "\033[1m"
GREEN = "\033[32m"
BLUE = "\033[34m"
YELLOW = "\033[33m"
CYAN = "\033[36m"
RED = "\033[31m"
MAGENTA = "\033[35m"
RESET = "\033[0m"

def run_cmd(cmd):
    try:
        res = subprocess.run(cmd, shell=True, capture_output=True, text=True, check=True)
        return res.stdout.strip()
    except Exception:
        return None

def get_git_info():
    branch = run_cmd("git branch --show-current")
    if not branch:
        return None

    # Check tracking branch diffs
    ahead = run_cmd("git rev-list --count @{u}..HEAD 2>/dev/null")
    behind = run_cmd("git rev-list --count HEAD..@{u} 2>/dev/null")

    # Local changes count
    unstaged_diff = run_cmd("git diff --name-only 2>/dev/null")
    staged_diff = run_cmd("git diff --cached --name-only 2>/dev/null")

    unstaged_files = len(unstaged_diff.splitlines()) if unstaged_diff else 0
    staged_files = len(staged_diff.splitlines()) if staged_diff else 0

    return {
        "branch": branch,
        "ahead": int(ahead) if ahead else 0,
        "behind": int(behind) if behind else 0,
        "unstaged": unstaged_files,
        "staged": staged_files
    }

def estimate_codebase_tokens():
    # Sum characters of all tracked files
    tracked_files = run_cmd("git ls-files")
    if not tracked_files:
        return 0

    # Text-like files filter: ignore binaries, images, raw assets, archives, AI models, and fonts
    binary_extensions = [
        ".cr3", ".jpg", ".jpeg", ".png", ".gif", ".tiff", ".tif", ".hdr",
        ".tar.gz", ".tar", ".gz", ".zip", ".db", ".sqlite", ".sqlite3",
        ".pdf", ".mov", ".mp4", ".wav", ".mp3", ".dylib", ".so", ".dll", ".a", ".o",
        ".onnx", ".ttf", ".woff", ".woff2", ".eot", ".bin", ".pb", ".pt", ".h5"
    ]

    total_bytes = 0
    for f in tracked_files.splitlines():
        if os.path.exists(f) and os.path.isfile(f):
            # Ignore path patterns
            if any(p in f for p in ["target/", "vendor/", "Cargo.lock", ".git/"]):
                continue
            # Ignore binary extensions
            if any(f.lower().endswith(ext) for ext in binary_extensions):
                continue
            try:
                total_bytes += os.path.getsize(f)
            except Exception:
                pass
    return int(total_bytes / 4)

def get_brain_metrics():
    # Find brain folders
    brain_dir = "/Users/ph/.gemini/antigravity-cli/brain"
    if not os.path.exists(brain_dir):
        return None

    # Locate most recently modified transcript_full.jsonl or transcript.jsonl
    log_files = glob.glob(os.path.join(brain_dir, "*/.system_generated/logs/transcript_full.jsonl"))
    if not log_files:
        log_files = glob.glob(os.path.join(brain_dir, "*/.system_generated/logs/transcript.jsonl"))

    if not log_files:
        return None

    # Sort by modification time to find the active conversation
    log_files.sort(key=os.path.getmtime, reverse=True)
    active_log = log_files[0]
    conv_id = active_log.split("/")[-4]

    # Parse transcript to estimate tokens and costs
    input_tokens = 0
    output_tokens = 0
    steps = 0

    try:
        with open(active_log, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    data = json.loads(line)
                    steps += 1
                    source = data.get("source", "")
                    content_len = len(data.get("content") or "")
                    thinking_len = len(data.get("thinking") or "")

                    # Tool calls payload
                    tools_len = 0
                    tools = data.get("tool_calls") or []
                    if tools:
                        tools_len = len(json.dumps(tools))

                    tot_chars = content_len + thinking_len + tools_len
                    tot_toks = int(tot_chars / 4) + 1

                    if source == "MODEL":
                        output_tokens += tot_toks
                    else:
                        input_tokens += tot_toks
                except Exception:
                    pass
    except Exception:
        return None

    # Pricing estimates:
    # Blended rates matching Gemini 1.5 / 2.0 Pro context limits ($1.25 / 1M in, $5.00 / 1M out)
    input_cost = (input_tokens / 1000000.0) * 1.25
    output_cost = (output_tokens / 1000000.0) * 5.00
    total_cost = input_cost + output_cost

    return {
        "conv_id": conv_id,
        "steps": steps,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": input_tokens + output_tokens,
        "cost": total_cost
    }

def print_status():
    cwd = os.getcwd()
    git_info = get_git_info()
    brain_info = get_brain_metrics()
    codebase_toks = estimate_codebase_tokens()

    print(f"\n{BOLD}{CYAN}======================================================================{RESET}")
    print(f"{BOLD}{MAGENTA}                       ANTIGRAVITY SESSION STATUS                      {RESET}")
    print(f"{BOLD}{CYAN}======================================================================{RESET}\n")

    # 1. Environment Info
    print(f"{BOLD}{BLUE}[ENVIRONMENT]{RESET}")
    print(f"  {BOLD}Folder:{RESET} {cwd}")
    if git_info:
        print(f"  {BOLD}Git Branch:{RESET} {GREEN}{git_info['branch']}{RESET}")

        # Diff with origin
        ahead = git_info["ahead"]
        behind = git_info["behind"]
        if ahead == 0 and behind == 0:
            diff_str = f"{GREEN}Synchronized with origin{RESET}"
        else:
            diff_str = ""
            if ahead > 0:
                diff_str += f"{YELLOW}{ahead} commits ahead{RESET}"
            if behind > 0:
                if diff_str: diff_str += ", "
                diff_str += f"{RED}{behind} commits behind{RESET}"
        print(f"  {BOLD}Git Sync:{RESET} {diff_str}")

        # Local changes status
        staged = git_info["staged"]
        unstaged = git_info["unstaged"]
        print(f"  {BOLD}Staged Changes:{RESET} {GREEN if staged else RESET}{staged} files{RESET}")
        print(f"  {BOLD}Unstaged Changes:{RESET} {YELLOW if unstaged else RESET}{unstaged} files{RESET}")
    else:
        print(f"  {BOLD}Git Info:{RESET} {RED}Not a git repository{RESET}")
    print()

    # 2. Conversation & Context Info
    if brain_info:
        print(f"{BOLD}{BLUE}[ACTIVE CONVERSATION]{RESET}")
        print(f"  {BOLD}Conversation ID:{RESET} {brain_info['conv_id']}")
        print(f"  {BOLD}Session Turns:{RESET}   {brain_info['steps']} execution steps")

        # Estimate history tokens
        hist_tokens = brain_info["total_tokens"]
        tot_active_context = hist_tokens + codebase_toks

        # Gemini context budget (1,000,000 baseline)
        limit = 1000000
        percent = (tot_active_context / limit) * 100.0

        # Cost styling
        cost_color = GREEN if brain_info['cost'] < 0.50 else (YELLOW if brain_info['cost'] < 2.00 else RED)

        print(f"  {BOLD}History Tokens:{RESET}  {hist_tokens:,} tokens (estimated)")
        print(f"  {BOLD}Codebase Tokens:{RESET} {codebase_toks:,} tokens (estimated)")
        print(f"  {BOLD}Total Context:{RESET}   {BOLD}{tot_active_context:,} / {limit:,} tokens ({percent:.2f}% usage){RESET}")

        # Context visual bar
        bar_length = 30
        filled = int(percent * bar_length / 100.0)
        filled = min(filled, bar_length)
        bar = "█" * filled + "░" * (bar_length - filled)
        bar_color = GREEN if percent < 20 else (YELLOW if percent < 50 else RED)
        print(f"  {BOLD}Context Bar:{RESET}     [{bar_color}{bar}{RESET}]")

        print(f"  {BOLD}Estimated Cost:{RESET}  {cost_color}${brain_info['cost']:.4f} USD{RESET} (pricing rate: $1.25/1M in, $5/1M out)")
    else:
        print(f"{BOLD}{BLUE}[ACTIVE CONVERSATION]{RESET}")
        print(f"  {RED}No active conversation transcript found in app data directory.{RESET}")
    print(f"\n{BOLD}{CYAN}======================================================================{RESET}\n")

if __name__ == "__main__":
    print_status()
