---
name: implement-feature-step
description: Implement the next planned step of a Git branch using its Notion Feature report and Kanban cards. Get the user's acceptance before coding, then publish a short Review page and stop for the user to commit. Use after the feature's commits have been planned and its Kanban cards created.
---

# Implement Feature Step

Each Kanban card is one planned commit. This skill takes a single card through `Backlog → Ready → In progress → Review`: propose how to implement it, wait for the user to accept, implement it, publish a review summary, and stop. The user reviews the changes and makes the commit.

## Ground rules

- Handle one card per run. Never move on to the next card by yourself.
- Never edit code until the user explicitly accepts the proposal for this card.
- Never commit, push, open a PR, or merge. Leave the user's index and any unrelated changes as they are.
- Work with the existing Notion pages, database, and stage names. Don't create replacements or change the schema.
- After every Notion write, read it back to confirm it was saved.

## 1. Identify the branch and project

### Inputs

The user may give a branch name and the project's teamspace, in plain text or in this format:

```text
Branch[[feature/example]] | Teamspace[[Leme]]
```

They may also give direct links to the `Feature report`, the `Kanban`, or the `Review` parent page.

If the conversation has already made the project clear, reuse it. Otherwise, work it out from the repository and the linked project pages. Only ask the user for information you can't find and need to pick the right project and branch.

### Repository state

Read the repository instructions, then check the current branch, worktree location, HEAD, and any staged, unstaged, and untracked changes.

- **No branch given:** use the checked-out branch.
- **Branch given, but a different one is checked out:** find that branch's existing worktree. Naming a branch doesn't authorize editing a different checkout. If there's no suitable checkout, sort it out with the user before editing anything.
- **Detached HEAD and no branch given:** ask which branch to use.

Record this starting state (HEAD and any existing changes) so the review later covers only this step's changes, and leave unrelated work untouched.

### Notion pages

Find these through the Notion integration:

- **Feature report** (under `Git`): read the branch's full entry — description, implementation points, completion check, and dependencies — including any relevant parent features.
- **Kanban**: the existing database at the project root. Fetch its data source and property schema before querying or updating it.
- **Review**: the parent page for review reports, in the project's teamspace. If the user gave a Review destination, use that one, even if it's in another teamspace. A page and a teamspace can share a name; don't assume they're the same thing.

Use `Project Summary` for general context, and `Git → Commit breakdown` when you need the original plan. If the Review parent is missing or ambiguous, settle where it is before publishing.

## 2. Pick the card

### Find the branch's cards

Query the database records directly, paginating as needed, for an exact match on the `Branch` property (it may be a select or a multi-select). Don't match on card titles or partial names, and don't rely on board views, which can hide records.

### Check for work already underway

Before choosing anything new, look at all of the branch's cards in every stage:

- **Ready:** a proposal waiting for acceptance. Being in Ready is not acceptance. Resume this card instead of taking a new one.
- **In progress:** changes may already exist. Reconcile them with the recorded acceptance before continuing. Don't duplicate another agent's work, and don't assume the card is yours just because of its status.
- **Review, Awaiting commit, or Awaiting approval:** waiting on the user. Share the card link and wait before starting any step that depends on it.

### Select the next Backlog card

Otherwise, take the `Backlog` card with the lowest `Step` number. Use the numeric value of the `Step` property, not `Plan order`, the title, or alphabetical order. If step numbers are missing or duplicated and the choice isn't clear, resolve that before changing any card.

Before preparing the card, check the earlier steps, its `Depends on` relations, and any recorded blockers. A prerequisite counts as done only when its approval is recorded. Never skip an unfinished or blocked earlier step; instead, tell the user which dependency or blocker is in the way.

If no Backlog cards are left, tell the user whether steps are still waiting for review or approval, or whether the whole branch is approved. Don't recreate cards, and don't mark the branch complete just because Backlog is empty.

### Read the card's plan

Fetch the card's full page body and its entry in the source plan. Compare its scope with the Feature report and the actual code. If they contradict each other in a way that matters, raise it before implementing; don't quietly rewrite the plan.

## 3. Propose and wait for acceptance

Read the code you need to explain how this step will work, then present:

- the card link, branch, and step number
- the intended behavior
- concrete implementation points
- the files likely to change
- how you'll verify it

Keep the proposal to this step only.

Move the card from `Backlog` to `Ready` and confirm the change was saved. If the board uses a different label for this stage (such as `Ready to start` or a numbered label), use it; don't rename stages.

Then ask the user to accept the proposal, and wait. Only an explicit acceptance of this card's proposal counts. Running the skill, the card sitting in Ready, or approval of an earlier step does not. While waiting, you may read and inspect code, but don't edit anything or run commands that change the repository.

If the user asks for a different approach, revise the proposal and keep the card in Ready until they accept it. If the current collaboration mode doesn't allow implementation, this skill doesn't override it.

## 4. Implement

After acceptance, check again that the checkout, the card, its dependencies, and the agreed scope haven't changed. If someone else changed them, reconcile that first. Then move the card to `In progress`, confirm it was saved, and start editing.

Implement only this card, and respect the project's test policy and the planned commit boundaries: an implementation card doesn't take on work from a later test card, and a test card doesn't turn into an unrelated refactor.

Run the verification that matters for this change, plus the project's required checks. Record each command and its actual result, including failures and any checks you couldn't run.

Fix problems within the agreed scope. If you discover something that changes the scope or approach, explain it and get the revision accepted before expanding the work. If you're blocked, record it in the `Blocked` and `Blocker` properties when the board has them. Don't move unfinished work, or work with unresolved required failures, to Review.

Leave everything uncommitted.

## 5. Publish the review summary

Compare the final changes with the starting state you recorded, including new files and any relevant staged changes, so the summary covers only this step and not the whole branch. Check the result against the card and the feature requirements.

### Where to publish

Create real child pages in this hierarchy:

```text
<project teamspace>
└── Review
    └── <exact branch name>
        └── <Step number>
```

- **Branch page:** reuse it if it already exists under the Review parent. Its title is the full branch name, including `feature/`. The slash is part of the title and doesn't create another level.
- **Step page:** titled with the step number, for example `1`. Before creating one, look for an existing report for this step (same parent and step, and the same card link or plan key). If there is one, update it, keep any review notes the user added, and don't create a duplicate when retrying or continuing.

### What to write

Write a summary the reviewer can read quickly and still understand the whole step without a full PR audit. Group changes by behavior or responsibility in short bullets, cover every meaningful change, and say which files are involved and why the change was made or what it does. Don't add a bullet for each trivial edit.

Open the report with a `## TLDR` section, then a `---` line, then the technical report. Write the TLDR for a reader who doesn't know the language, libraries, or tools involved, following the `for-dummies` approach: a respectful, adult tone and full technical accuracy. Keep it short, around 300 words for a feature step and less for a test step. Give each idea one sentence, define each tool or term in a few words where it first appears, and don't repeat a caveat. Avoid unexplained acronyms. The TLDR must not contradict the sections below it or claim more than they show. Use these bold-labelled parts:

- A bold opening sentence: what the step adds, framed as the question it answers.
- **Why this exists.** One or two sentences on the problem it solves for the user.
- **What you would see.** One concrete scenario and its result. For a test step, what the tests try and what they prove. Define any new status or term in a sentence.
- **What was added:** a short numbered list in plain language, with the main file for each item in parentheses.
- **Not included.** What the step doesn't do, and which later step or feature covers it.
- **Known limits.** The main gaps or unverified assumptions in one or two sentences, or a link to where they're detailed. Leave this part out if there are none.

```markdown
## TLDR
<bold opening sentence, then the bold-labelled parts above>

---

Planned commit: <card title>
Card: <Notion card link> | Feature: <Feature report link>
Base HEAD: <SHA before this step> | Changes: uncommitted

## Changes
- `<file or component>`: <what changed and what it enables or fixes>.
- <Next distinct change, including relevant tests, docs, configuration, or dependencies>.

## Verification
- `<command or manual check>` — <actual result and what behavior it covers>.
- <Required check that wasn't run or failed, and why, if any>.

## Decisions and review notes
- **Decision, <topic>:** <what was chosen, and why>.
- **Note, <topic>:** <impact or limitation, how it was established, and the follow-up needed>.
```

Leave out bullets that don't apply, and drop the Decisions and review notes section if there's nothing worth flagging. In the combined block, label each item **Decision** (a choice made during the step, and why: plan/code mismatches, ambiguous requirements, alternatives rejected) or **Note** (an impact on correctness, regressions, compatibility, security, or data handling, or a specific limitation with its follow-up), followed by a short topic. Keep one line per item, list decisions before notes, and leave out anything the TLDR or the code already makes obvious. Make it clear what you confirmed by running something, what you concluded from reading the code, and what is an unverified assumption. Don't mention a commit SHA (there isn't one yet) or suggest that this step is approved for merge. If later changes affect what the report says, update it.

The format is inspired by [pr-audit](https://github.com/akitaonrails/my-skills/blob/master/pr-audit/SKILL.md): evidence-backed verification, concrete findings, and relevant compatibility or risk notes. This skill doesn't run that audit or its full PR checklist; the user runs it separately when the feature PR is ready to merge.

## 6. Hand off

1. Read the published report back and confirm its content, title, and parent pages.
2. Add a link to the report in the card body, keeping the card's implementation plan.
3. Move the card to `Review` and confirm it was saved. Publishing first ensures the card already links to its summary when it reaches Review.

If a Notion write fails or times out, read the page or card back before retrying, and reuse anything that was already created. If you can't confirm the report or the final status change, keep the code changes, tell the user exactly what isn't synced, and don't say the handoff is complete.

Finally, give the user the card and report links, a short summary of the result, and any important gaps in verification. Then stop: the user reviews and commits. Don't mark the card Approved or start the next step. When asked to continue later, re-read the branch and the board instead of assuming the previous step was committed or approved.
