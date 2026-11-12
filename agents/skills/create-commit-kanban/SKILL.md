---
name: create-commit-kanban
description: Create a Notion Kanban database from a completed feature commit plan, with one card per planned commit, branch views, dependency links, and separate human review, commit, and approval stages.
---

# Create Commit Kanban

Turn an existing feature commit plan into a usable Notion board. The plan already supplies the scope, commit sequence, and test choices. Preserve those decisions while making the work easy to find, review, and move through the workflow.

## Input

Receive the Notion URL of the completed **feature commit plans** page. Accept a plain URL, a Markdown link, or the labeled form below.

```text
CommitPlans[[FEATURE_COMMIT_PLANS_URL]]
```

Read the actual link destination when Markdown is pasted inside the brackets. Fetch the supplied page and its nested feature blocks through the available Notion integration. Retrieve omitted content before treating the inventory as complete.

Use this skill after commit planning is ready. If the supplied page contains only a feature tree or a vision, ask for the completed commit-plan page. If required planning decisions are explicitly unresolved, identify them before creating the affected cards. Use the test choices already recorded in the plan.

## Phase 1 — Map the commits

Extract every planned commit with its branch, feature ID when present, local commit number, message, implementation points, purpose, files, behavior change, verification, and recorded progress.

- Use one card per planned commit. A feature is a branch grouping, with its commits as the work items.
- Include a MASTER/bootstrap commit if the plan contains one.
- Preserve branch names, commit messages, numbering, test categories, and source detail.
- Give each card a stable plan key. Prefer a source commit ID when present. Otherwise use the feature ID or branch plus its commit number, such as `F01-C02`.
- Keep the record of why a dependency exists. Within a branch, a commit follows its predecessor. A dependent branch's first commit follows the prerequisite branch's final commit.
- Include explicit cross-branch dependencies as well as the visual parent. A reference to a later feature that will consume an output does not make that later feature a prerequisite.
- Verify that the dependency graph has no cycles. Sort cards in an order consistent with that graph, preserving commit order within each branch.

Keep an expected inventory of plan keys, branch counts, and dependency links for verification. Derive counts from the source instead of assuming a fixed project size.

## Phase 2 — Choose the destination

Name the database `Kanban` and create it directly at the root of the project's teamspace or designated project home (`Leme` for this project), alongside `Project Summary` and `Git`. Resolve that root from the supplied commit-plan page or the user's chosen destination. The source commit-plan page is named `Commit breakdown` and sits inside `Git`, alongside `Feature report`; the existing `CommitPlans` input format still applies. This naming and placement rule takes precedence over the child-page default below: do not nest `Kanban` inside `Commit breakdown` or `Git`, and do not default to Private. An explicit destination supplied by the user takes precedence.

Create a native Notion database named `Kanban`, with the cards and views described below, without an extra wrapper page. Reuse the matching existing database and preserve its ID, cards, relations, views, and recorded progress. If it is still nested under the commit-plan page, move that database to the project root instead of recreating it. Verify both its title and parent after saving.

Create the database in the destination the user supplies. Otherwise, create it as a child of the supplied commit-plan page so it inherits that page's project context and permissions.

Look for an existing board for this source in the intended destination. Reuse a matching board when continuing or repeating the same task. Distinguish it by its source-plan link and records, rather than title alone. If its intended identity is ambiguous, clarify before writing to it.

For an existing board, fetch its schema and current records first. Preserve human-maintained progress, blockers, commit SHAs, and other values during synchronization. If a revised plan key now points to different work, reconcile the match before replacing content. Source edits can introduce new cards or changed descriptions, but they do not erase recorded work.

## Phase 3 — Build the database

Use the connector's current tools and documented database/property syntax. Check tool availability once and read the current Markdown and view-configuration specifications when needed. Fetch the created data source before populating it or configuring its views.

Use these properties, adapting existing names and types when reusing a board.

| Property | Type | Purpose |
| --- | --- | --- |
| Commit | Title | Planned commit message |
| Stage | Status | Position in the review workflow |
| Branch | Select | Feature branch or MASTER |
| Step | Number | Commit number within the branch |
| Type | Select | Implementation, Test, Docs, CI, Setup, or another category actually present |
| Test type | Select | Test category stated in the plan, when applicable |
| Plan key | Text | Stable source identity used for matching and retries |
| Plan order | Number | Dependency-respecting display order |
| Depends on | Relation to this data source | Earlier commits required by this card |
| Blocked | Checkbox | A reported runtime blocker |
| Blocker | Text | Description of that blocker |
| Commit SHA | Text | Actual human-created commit when known |
| Source plan | URL | Original commit-plan page |

Leave unknown commit SHAs and blocker reasons empty. Add owners and dates only when supplied by the user or the source. Waiting for dependencies and a reported runtime blocker have separate meanings.

### Workflow columns

Use this order from left to right.

1. **Backlog** — planned work awaiting selection.
2. **Ready to start** — selected work whose prerequisites are satisfied.
3. **In progress** — the agent is implementing the step.
4. **Review** — changes and verification results are ready for human inspection.
5. **Awaiting commit** — reviewed changes are ready for the human to commit.
6. **Awaiting approval** — the manual commit exists and awaits explicit approval.
7. **Approved** — the corresponding commit has received human approval.

Map Backlog and Ready to start to the to-do status group, the four active stages to in-progress, and Approved to complete.

For a plan whose steps are all pending, initialize every card in Backlog. Carry over more advanced progress only when the source or existing board establishes it. An implementation marked done does not establish that a manual commit exists or has been approved.

Set the actual board column order explicitly using supported view controls. Check the saved grouping and displayed order. Alphabetical sorting can put Ready to start after In progress and Approved first.

If the connector can only guarantee alphabetical ordering, create numbered labels from the beginning, such as `01 Backlog`, `02 Ready to start`, through `07 Approved`. Use those exact labels consistently in records and filters and disclose the numbered-label choice. Preserve populated status values when changing an existing board's ordering.

### Card contents

Keep the complete implementation points, purpose, files, behavior change, and verification instructions in each card's page body. Add a native Notion reference to the source plan when supported. Use concise board properties to help scanning and open-card content for the detail.

Keep projected files and commands labeled as proposed when the source does so. Planned verification remains a future check until actual results are recorded. Use test-type labels from the existing plan and keep ordinary product code separate from test cards.

## Phase 4 — Configure the views

Create these views over the same data source.

| View | Configuration |
| --- | --- |
| Kanban | Board grouped by each Stage option, with all workflow columns visible and cards sorted by Plan order. Show Branch, Step, Type, and relevant test/blocker indicators. |
| By branch | Table grouped by Branch and sorted by Step. Show Stage, Type, Test type, Depends on, and Commit SHA. |
| All commits | Table sorted by Plan order, with branch, progress, and dependency properties visible. |
| Review queue | Board filtered to Review, Awaiting commit, and Awaiting approval, using the same stage order. |
| Blocked | Table filtered to Blocked being checked, showing the blocker reason and dependency links. |

Add quick filters for Branch, Stage, and Type where useful. Keep card layouts compact and table titles wide enough to read commit messages. Reuse the automatically created table as All commits when possible. Read the connector's view DSL before configuring grouping, filters, sorting, and display properties.

Return a link that opens the Kanban view directly when supported. Confirm that selecting the link opens the board even if another table remains the database's first tab.

## Phase 5 — Populate and verify

1. Create cards in bounded batches and retain each returned page ID with its plan key. Resolve the exact data source and property names from the fetched schema.
2. Create dependency relations using the resulting page IDs or URLs. Forward references require a second pass after the target cards exist.
3. If a batch fails or times out, query the created records by plan key before retrying. Resume only missing work. Apply the same rule to relation updates.
4. Read back enough records, with pagination, to verify every expected card. Compare plan keys, branch/step assignments, messages, test labels, and dependency targets. Check for duplicates and cycles.
5. Inspect representative card bodies, including bootstrap, a test commit, and a later dependent feature. Confirm that the implementation and verification details were preserved.
6. Verify saved view grouping, filters, sort order, and workflow column order. A view name alone does not prove its filter works. Check that review and blocker queues include only matching records.

The database represents the approved plan and observed progress. Moving cards and enforcing approvals are distinct operations. Record any automatic behavior only when it was actually configured and verified.

Return the board link, its destination, card and branch counts, available views, and initial stage placement. Report any incomplete import, unresolved dependencies, ordering workaround, or connector warning that affects use.
