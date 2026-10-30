#!/usr/bin/env python3
"""Claude Code PreToolUse hook: route GADS R3/R4 shell commands to a human.

Reads the hook event as JSON on stdin. Prints a permission decision for risky
commands and nothing otherwise. Pattern matching is a safety net, not a
security boundary: a determined agent can obfuscate a command.
"""
import json
import re
import shlex
import sys

# `git` followed by any global options that may precede the subcommand,
# for example `git -C path`, `git -c k=v`, `git --git-dir=x`, `git --no-pager`.
ARG = r"(?:\"[^\"]*\"|'[^']*'|\S+)"
GIT = (r"\bgit(?:\s+(?:-[Cc]\s+" + ARG + r"|--(?:git-dir|work-tree|namespace)(?:=|\s+)" + ARG
       + r"|--no-pager|--paginate|-P|--bare|--literal-pathspecs))*\s+")
ASK = [
    (GIT + r"push\b[^;&|]*(\s--force(-with-lease)?\b|\s-[a-z]*f[a-z]*\b|\s\+\S)",
     "R3: force-push rewrites shared history"),
    (GIT + r"push\b[^;&|]*(\s--delete\b|\s-d\b|\s:\S|\s--mirror\b|\s--tags\b|\srefs/tags/)",
     "R3: deletes or publishes remote refs or tags"),
    (GIT + r"(reset\s+--hard|clean\s+-[a-z]*f|branch\s+-D|checkout\s+--\s|restore\s+\.)",
     "R3: discards local work"),
    (GIT + r"filter-(branch|repo)\b|" + GIT + r"rebase\b[^;&|]*\s--root\b",
     "R3: rewrites history"),
    (r"\bgh\s+(release\s+(create|delete|upload|edit)|repo\s+(create|delete|edit|rename)"
     r"|secret\s|variable\s|api\s[^;&|]*-X\s*(DELETE|PATCH|PUT|POST))",
     "R3: changes or publishes GitHub resources"),
    (r"\b(npm|yarn|pnpm)\s+publish\b|\btwine\s+upload\b|\b(cargo|poetry|uv)\s+publish\b"
     r"|\bdotnet\s+nuget\s+push\b|\bgem\s+push\b|\bdocker\s+push\b",
     "R3: publishes a package or image"),
    (r"\bkubectl\s+(apply|delete|replace|patch|scale|rollout)\b|\bhelm\s+(install|upgrade|uninstall|rollback)\b"
     r"|\bterraform\s+(apply|destroy|import)\b",
     "R3: changes deployed infrastructure"),
    (r"\b(curl|wget)\b[^;&]*\|\s*(sudo\s+)?(ba|z)?sh\b",
     "R3: runs an unreviewed remote script"),
    # Secret files read or copied through the shell bypass the Read deny rules in settings.json.
    (r"(?<![\w.-])\.env(?:\.(?!example(?![\w.-]))[\w.-]+)?(?![\w.-])|\.(?:pem|key)(?![\w.-])"
     r"|\bid_(?:rsa|dsa|ecdsa|ed25519)(?![\w.-])",
     "R3: reads or copies a secrets file"),
]
# Targets that make a recursive rm destroy the filesystem root or the home directory.
ROOT_OR_HOME = {"/", "~", "$HOME", "${HOME}"}
SHELL_PUNCTUATION = set("();<>|&")


def root_or_home(target):
    """True when target is /, ~ or $HOME, alone or followed by /, /* or /."""
    t = re.sub(r"(?:/\.?|/\*|\*)+$", "", target)
    return t in ROOT_OR_HOME or (t == "" and target.startswith("/"))


def simple_commands(command):
    """Split a shell command into simple commands, respecting quotes.

    Separators (`;`, `&&`, `||`, `|`), subshells `( )`, `$( )` and backticks all
    end a simple command. Raises ValueError on unbalanced quotes.
    """
    lexer = shlex.shlex(command.replace("`", " ; ").replace("\n", " ; "), posix=True, punctuation_chars=True)
    lexer.whitespace_split = True
    current = []
    for tok in lexer:
        if tok and set(tok) <= SHELL_PUNCTUATION:
            if current:
                yield current
            current = []
        else:
            current.append(tok)
    if current:
        yield current


def recursive_rm_of_root_or_home(command, depth=0):
    """Return a reason when any simple command is a recursive rm of / or the home directory."""
    if not re.search(r"\brm\b", command):
        return None
    try:
        commands = list(simple_commands(command))
    except ValueError:
        return "ask", "R4: could not parse an rm command"
    for tokens in commands:
        # A quoted argument can itself be a command, as in `bash -c "rm -rf /"`.
        for tok in tokens:
            if depth < 3 and re.search(r"\brm\b", tok) and re.search(r"\s", tok):
                nested = recursive_rm_of_root_or_home(tok, depth + 1)
                if nested:
                    return nested
        starts = [i for i, tok in enumerate(tokens) if tok == "rm" or tok.endswith("/rm")]
        for start in starts:
            recursive, targets, options = False, [], True
            for tok in tokens[start + 1:]:
                if options and tok == "--":
                    options = False
                elif options and tok.startswith("--"):
                    recursive |= tok == "--recursive"
                elif options and tok.startswith("-") and len(tok) > 1:
                    recursive |= "r" in tok[1:] or "R" in tok[1:]
                else:
                    targets.append(tok)
            if recursive and any(root_or_home(t) for t in targets):
                return "deny", "R4: recursive deletion of the filesystem root or home directory"
    return None


def decide(command):
    if not isinstance(command, str):
        return "ask", "GADS guard received a non-text command"
    rm = recursive_rm_of_root_or_home(command)
    if rm:
        return rm
    for pattern, reason in ASK:
        if re.search(pattern, command):
            return "ask", reason
    return None


def main():
    try:
        event = json.load(sys.stdin)
        command = event.get("tool_input", {}).get("command", "")
        result = decide(command)
    except Exception:  # noqa: BLE001 - fail toward the human on any error, never silently allow
        result = ("ask", "GADS guard could not evaluate the hook input")
    if result:
        decision, reason = result
        print(json.dumps({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": f"GADS {reason}. Explicit human approval is required.",
        }}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
