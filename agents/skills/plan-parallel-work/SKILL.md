---
name: plan-parallel-work
description: Read a project's Notion Kanban and linked plans to identify work that can proceed in parallel, including dependencies, approval gates, and shared-file conflicts. Publish or refresh a Parallel Work Plan report under the project's Git page. Use when asked to plan parallel features, branches, or Kanban steps; this skill plans work and does not start agents or implementation.
---

# Plan Parallel Work

Read the current Kanban and publish an evidence-based Notion child page named
`Parallel Work Plan` under the project's `Git` page. Resolve the correct project
and teamspace before creating anything. For Leme, use its Leme teamspace, not
private pages or Project Summary. Treat "file inside the Git page" as a Notion
report page unless the user requests a downloadable file.

## Gather the current snapshot

- Locate and fetch the Git page, Kanban database/data-source schema, and linked
  feature/commit plans. Reuse known links, but fetch current contents.
- Retrieve every relevant card, following pagination. A filtered view or search
  sample is not the full board. Check coverage against a source count when
  available and state any missing, inaccessible, or truncated information.
- Record card IDs/URLs, plan keys, feature/branch, step order, dependency
  relations, stage, blockers, commit evidence, and scope. Fetch candidate cards
  when their detailed scope or blocker affects the recommendation.
- Consult repository instructions or read-only Git/PR evidence when available
  to resolve branch strategy, delivered prerequisites, and shared files. Treat
  a created branch, commit, passing check, merged PR, and human approval as
  different events. Report stale board information without silently changing it.

## Analyze dependencies and conflicts

Build a directed graph at card level. Resolve dependency IDs, detect missing
references and cycles, and trace indirect dependencies as well as direct ones.
Preserve the project's per-step ordering and human review/commit/approval gates.
An unchecked Blocked field is not proof of readiness; inspect prerequisites.

Separate:

- Work eligible now under the board's recorded gates; eligibility does not
  itself authorize implementation or change a Backlog card to Ready.
- Work possible after named approvals, merges, or completed prerequisites.
- Preparation that can proceed independently from implementation blocked on
  another feature; label preparation explicitly, without bypassing a dependency.

Check planned files, shared APIs/data shapes, migrations, fixtures, manifests,
plugin configuration, and live services. Classify concurrency as independent,
possible with explicit coordination, blocked by dependencies, or unverified.
Different branches/worktrees do not guarantee independent work. For overlapping
files, name the overlap and propose ownership or an integration order. Do not
invent interface contracts to make a blocked feature appear ready.

Produce dependency-respecting groups or lanes. Future groups are conditional,
not estimates of duration or mandatory global barriers. If reporting all
feature pairs, include only pairs with no dependency path in either direction;
a multi-feature group must be pairwise compatible too. State whether feature
aggregation conservatively hides partial-card opportunities. Validate the graph
and groups rather than relying on feature numbers or visual board placement.

## Publish the report

Include snapshot time and source links, coverage counts, a plain-language answer
about what can overlap now, the next eligible cards, future parallel groups,
full scoped feature/dependency coverage, and the coordination needed for shared
files or resources. Include blocked chains, approval/status discrepancies,
uncertainty, and a concrete recommended starting set. Cite relevant cards and
plans near recommendations; distinguish source facts from proposed scheduling.

Read the Notion Markdown specification before writing. Search the Git page's
children for an existing report and update that report on reruns, preserving
unrelated content and child pages. Create a child page only when absent. Do not
fall back to a private page when the named destination is unavailable; report
the access problem and retain a copy-ready draft instead. After an uncertain
write, check for the report before retrying. Return its link and open it with
the Notion page-panel tool when available.

This skill authorizes reading sources and writing the requested report. Do not
change cards, dependency relations, branches, commits, assignments, or approvals;
do not launch agents, start implementations, or merge work unless separately
requested. Preserve existing skills and unrelated project files.
