# Coding Principles

**Normative language:** MUST/ MUST NOT indicate requirements; SHOULD/SHOULD NOT indicate strong recommendations; MAY indicates optional practices.

These principles target the mistakes coding agents make most often: acting on silent assumptions, overbuilding, editing code outside the task, and stopping without a way to check the result. They apply to every role that writes or reviews code, and they refine sections 2 and 3 of `gads/standard/OPERATING_STANDARD.md`.

They favor caution over speed. For trivial tasks, use judgment.

## 1. Think before coding

Do not assume. Do not hide confusion. Surface tradeoffs.

Before implementing, an agent:

- MUST state the assumptions the change depends on. When an assumption is uncertain and consequential, ask instead of guessing.
- MUST present the interpretations when a request has more than one, instead of picking one silently.
- SHOULD say so when a simpler approach exists, and push back when the request is likely to cause harm or waste.
- MUST stop and name what is unclear when it cannot proceed with confidence.

## 2. Simplicity first

Write the minimum code that solves the problem. Nothing speculative.

- No features beyond what was asked.
- No abstractions for code with a single use.
- No configurability or flexibility that was not requested.
- No error handling for scenarios that cannot occur.
- If 200 lines could be 50, rewrite them.

The test: would a senior engineer call this overcomplicated? If so, simplify.

## 3. Surgical changes

Touch only what the task needs. Clean up only your own leftovers.

When editing existing code, an agent:

- MUST NOT reformat, reword, or "improve" adjacent code, comments, or formatting.
- MUST NOT refactor code that is not broken, unless the task asks for it.
- SHOULD match the existing style, even where it would choose differently.
- SHOULD mention unrelated dead code it notices, and MUST NOT delete it unless asked.
- MUST remove imports, variables, and functions that its own change left unused.

The test: every changed line traces directly to the task.

## 4. Goal-driven execution

Define success criteria, then work until they are verified.

Turn each task into a goal that can be checked:

- "Add validation" becomes "write tests for invalid inputs, then make them pass".
- "Fix the bug" becomes "write a test that reproduces it, then make it pass".
- "Refactor X" becomes "the tests pass before and after".

For multi-step tasks, state a brief plan with a check for each step:

```text
1. <step> -> verify: <check>
2. <step> -> verify: <check>
```

Clear criteria let an agent work without constant clarification; vague ones ("make it work") do not. Completion states and evidence follow `gads/standard/WORKFLOW_AND_GATES.md`.

## Source

Adapted from [andrej-karpathy-skills](https://github.com/multica-ai/andrej-karpathy-skills) (MIT License), which derives these principles from [Andrej Karpathy's observations](https://x.com/karpathy/status/2015883857489522876) on LLM coding pitfalls.
