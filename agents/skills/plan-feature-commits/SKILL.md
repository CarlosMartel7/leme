---
name: plan-feature-commits
description: Use when planning a feature (or any change bigger than a small fix) to break the work into an ordered sequence of reviewable commits. The human reviews and commits.
---

# Plan Feature Commits

Structure the feature plan as an ordered list of commits instead of a flat task list. Each planned commit is a self-contained step with a clear purpose, its files, and how to verify it, so the implementation has an exact sequence to follow and the reviewer gets one focused diff per step instead of one large change.


---

## Notion inputs

Receive the project pages in this format:

```text
Branches[[BRANCHES_PAGE_URL]] | Vision[[VISION_PAGE_URL]]
```

The URLs may also be pasted as Markdown links inside the brackets. Read the actual link destination associated with each label.

- **Branches** contains the feature tree. Use its branch names, descriptions, nesting, and dependencies to produce the commit plans.
- **Vision** provides project context. Read it before asking about tests, so the options fit the application's architecture and behavior.

Fetch both pages through the Notion integration. Read nested feature blocks as needed. If the user names a feature, plan that feature. If only the two links are provided, plan the full feature tree, with a separate commit sequence for each branch.

---

## Phase 1 — Review the project test policy

### Project test policy

The types of tests are decided once for the whole repository, not per feature.

1. Check whether a test policy is already saved in `.agent/test-policy.md`. If it is, follow it and don't ask again.
2. If not, read Vision and look at what the project already has (test framework, test folders, existing tests, CI config), when the repository is available. Then ask the user which applicable test types below they want in this repo. Mention what you found so they can answer quickly.
3. Save their answer to `.agent/test-policy.md` (which types, and any notes such as tools or folders to use). If this is a Notion-only planning session, save the policy once in the new plan page.

When planning a feature, add test commits only for the test types in the policy that apply to the change. If the policy has none that apply, the plan has no test commits and the `Verify` steps rely on build, lint, and existing tests.

Use Vision to filter the question before presenting options. Offer only test types that apply to this project and adapt the examples to it. For example, do not offer API / endpoint tests for a program that exposes no HTTP or RPC endpoints. A client that calls an external API may still need integration or contract tests. Terminal flows can have E2E tests without a browser. Keep the table below as a reference rather than reproducing every option in the question. Wait for the user's answer before finalizing test commits.

| Test type | What it checks | Example |
|---|---|---|
| Unit | A single function, class, or module in isolation, with dependencies mocked | A price calculator returns the right total with discounts |
| Integration | Several parts working together with real dependencies (database, cache, queue) | A repository saves and reads a record from a test database |
| API / endpoint | HTTP or RPC endpoints: status codes, payloads, auth, validation | `POST /orders` returns 400 when a required field is missing |
| Component (UI) | A UI component renders and reacts to user input in isolation | A form shows an error message when the email is invalid |
| End-to-end (E2E) | Full user flows through the running app, usually in a real browser | A user signs up, logs in, and places an order |
| Contract | The agreed shape of data between services or with external APIs | The payments client still matches the provider's response schema |
| Snapshot | Rendered output or serialized data hasn't changed unexpectedly | A component's markup matches the stored snapshot |
| Performance / load | Response time and behavior under load | The search endpoint stays under 200 ms at 100 requests/s |
| Accessibility | UI meets accessibility rules (labels, contrast, keyboard navigation) | The checkout page has no axe violations |
| None | No new tests; rely on build, lint, and existing tests | — |

---

## Phase 2 — Plan the commits

Read the feature request and the relevant code first. Then produce the plan in the format below and wait for approval if the user is present.

For the Notion input workflow, use Branches as the feature request and Vision as its context. Keep the source feature tree and give each feature its own ordered list of commits. When code is unavailable, mark file paths and verification commands as proposed rather than inspected or executed.

### Splitting criteria (apply in this order)

1. **Every commit must leave the project working.** It builds, type-checks, lints, and existing tests pass. If a step would break the build on its own, merge it with its neighbor or reorder.
2. **One intent per commit.** You must be able to describe it in one sentence without "and". If you need "and", split it.
3. **Prepare first, then change behavior.** Refactors, renames, extractions, and moves that do not change behavior go in their own commits *before* the commits that change behavior ("make the change easy, then make the easy change").
4. **Isolate mechanical changes.** Renames across many files, file moves, formatting, generated code, dependency/lockfile updates, and migrations generated by tools get their own commit, so the reviewer can skim them.
5. **Order by dependency, bottom-up.** Typical order: schema/migrations → types/models → domain/business logic → API/service layer → UI → wiring/feature-flag/config → docs. Nothing may depend on code introduced in a later commit.
6. **Tests get their own commits, and only where the user wants them.** Not every change needs a test. Test commits follow the project's test policy (see "Project test policy" above). Each test commit comes right after the code commit(s) it covers and touches only test files (plus test fixtures/helpers). Never create test files the user didn't agree to.
7. **Docs and config travel with the change they describe.**
8. **Touch each file in only one commit when you can.** Avoid spreading edits to the same file across several commits; plan the split so all of a file's changes land in a single step. This doesn't mean one file per commit (a commit can and usually will change several files), only that a given file shouldn't keep reappearing. When a file genuinely has to change in more than one commit, say exactly which part of it belongs to which commit.
9. **Size target: reviewable in ~10–15 minutes.** Roughly under ~300 changed lines of real logic (mechanical changes can be larger). Do not split below a meaningful unit, though: a commit that only adds an unused function signature is noise.
10. **Unfinished user-facing behavior stays hidden.** If the feature is only usable after the last commit, intermediate commits must not expose half-working behavior (use a flag, or wire it up in the final commit).

### Anti-patterns to avoid

- One commit per file or per layer when the layers don't build independently.
- "WIP", "misc", "cleanup" commits with mixed content.
- A commit that fixes something introduced earlier in the same plan (fix it in the original step instead).
- Drive-by changes unrelated to the feature. If you spot one, list it under "Out of scope" rather than doing it.

### Plan output format

For Notion, name the output page `Commit breakdown` and place it inside the project's `Git` page, alongside `Feature report`. Find `Git` at the root of the project's teamspace or designated project home (`Leme` for this project), using the supplied source pages or the user's chosen destination to resolve the project location. Reuse `Git`, or create it at that root only if it is missing. Create `Commit breakdown` as a child of `Git`, or update the matching existing plan while preserving its page ID and links. Do not create a duplicate. This naming and placement rule takes precedence over the private-page instruction below; use Private only if the user explicitly requests it. An explicit destination supplied by the user takes precedence.

The input labels `Branches` and `Vision` still apply: `Branches` refers to `Feature report`, and `Vision` refers to `Project Summary`. `Project Summary` stays at the project root. The separate database named `Kanban` also belongs at the project root, outside both `Git` and `Commit breakdown`; creating that database remains the Kanban skill's task.

Create a new private Notion page containing the commit plan and show it to the user. Link to Branches and Vision once. Keep the source feature hierarchy as nested collapsible blocks, with the relevant commit list inside each feature. Use the commit-entry format below. When a repository is available, also write the selected feature's plan to `.agent/commit-plan.md` (create the folder if needed; it should be gitignored or deleted after review).

```markdown
# Commit plan: <feature name>

## Commit 1 — refactor(auth): extract token parsing into TokenParser
- Why: <one sentence on why this step exists / what it enables>
- Files: src/auth/token.ts (new), src/auth/session.ts
- Behavior change: none
- Verify: `npm test -- auth`
- Status: [ ] pending

## Commit 2 — feat(auth): add refresh-token rotation
...

## Commit 3 — test(auth): add unit tests for refresh-token rotation
- Why: covers rotation and expiry edge cases (user asked for unit tests)
- Files: src/auth/__tests__/rotation.test.ts (new)
- Behavior change: none
- Verify: `npm test -- rotation`
- Status: [ ] pending

## Out of scope (noticed, not done)
- ...
```

Use the repository's existing commit-message convention (check `git log --oneline -20`); default to Conventional Commits if there is none.
