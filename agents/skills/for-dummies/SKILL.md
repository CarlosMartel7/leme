---
name: for-dummies
description: Explain code, bugs, audit findings, or technical decisions in depth for someone unfamiliar with the language, framework, and tooling. Use when the user invokes for-dummies, asks for a beginner walkthrough, or says a technical explanation lacks the context they need.
---

# For Dummies

Assume no prior knowledge of the language, framework, or tools involved. Treat
this as a request for context and depth, not childish language or reduced
technical accuracy. Use the user's language and a respectful, adult tone.

Start with what the software is trying to accomplish and why the subject
matters to its user. Define unfamiliar terms before relying on them. Distinguish
language features, libraries, framework behavior, and application-specific
choices; do not attribute an application logic bug to the language itself.

Build a connected explanation from normal behavior to the actual behavior:

- Introduce the smallest concrete example needed to understand the system.
- Walk through the relevant inputs, processing, stored data, and output.
- Locate the precise point where actual behavior diverges from the intended
  behavior, and follow its consequences through to what the user sees.
- Explain impact, triggering conditions, and limits. Distinguish missing data
  from deleted data, possible effects from reproduced effects, and an edge case
  from a failure that affects ordinary use.
- Explain the proposed correction, why it belongs at that boundary, and what
  a meaningful test must demonstrate. Describe why existing tests missed it.

For real code or an audit, inspect the relevant source and available evidence.
Clearly separate intended behavior, observed behavior, inference, and unresolved
questions. Do not imply an old finding still affects a changed version without
checking. Reference relevant files after explaining their role.

Introduce code only after its purpose is understandable. Prefer small excerpts
or explicitly labelled pseudocode; explain unfamiliar syntax used in an excerpt.
Use concrete paths, values, before/after examples, or a compact table when useful.
An analogy can introduce an idea, but connect it back to the real mechanism and
state important limits. Avoid unexplained acronyms and jargon chains.

Provide enough depth that the user can explain why the issue matters and how
it is verified without learning the whole stack. Keep background relevant to
the question, and do not turn the answer into a generic language tutorial.
Explanation does not authorize modifying code, applying fixes, or publishing.
