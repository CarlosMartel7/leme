---
name: implement-feature-yolo
description: Implement every remaining planned step of a Git branch autonomously, using its Notion Feature report and Kanban cards. Commit each step as it's finished and publish a Notion report for every commit, without asking for approval. Use after the feature's commits have been planned and its Kanban cards created, when the user wants the whole feature done in one run.
---

# Implement Feature (YOLO)

Each Kanban card is one planned commit. This skill works through all of the branch's remaining cards in order without stopping for approval. For each card it implements the step, verifies it, commits it, publishes a review report in Notion, and moves on to the next card. The user reviews the finished feature afterwards, one commit at a time.

This is the autonomous version of `implement-feature-step`. The only difference is that it doesn't wait for the user: it decides, commits, and keeps going. Everything else (the Notion structure, step order, commit boundaries, test policy, report format) stays the same.

## Ground rules

- Don't ask the user for approval or acceptance at any point. Make the reasonable decision, record it in that step's report, and continue.
- Make one commit per card, in `Step` order. Never combine cards into one commit or split a card into several.
- Commit locally only. Never push, force-push, open a PR, merge, rebase, amend, or rewrite history.
- Never skip hooks (`--no-verify`) or disable checks to get a commit through.
- Work with the existing Notion pages, database, and stage names. Don't create replacements or change the schema.
- After every Notion write, read it back to confirm it was saved.
- Keep going by default. Unfinished work already in the worktree is not a reason to stop: finish it and commit it (see "Adopt uncommitted work"). If a situation isn't listed in "When to stop", make the reasonable decision, record it in the report, and continue.
- Stop only for the reasons listed in "When to stop". Stopping is not asking: record why, report it, and end the run.

## 1. Identify the branch and project

### Inputs

The user may give a branch name and the project's teamspace, in plain text or in this format:

```text
Branch[[feature/example]] | Teamspace[[Leme]]
```

They may also give direct links to the `Feature report`, the `Kanban`, or the `Review` parent page.

If the conversation has already made the project clear, reuse it. Otherwise, work it out from the repository and the linked project pages. If you still can't tell which project or branch is meant, stop (see "When to stop"); don't guess.

### Repository state

Read the repository instructions, then check the current branch, worktree location, HEAD, and any staged, unstaged, and untracked changes.

- **No branch given:** use the checked-out branch.
- **Branch given, but a different one is checked out:** use that branch's existing worktree. Don't switch the user's current checkout. If there's no worktree for it, stop.
- **Detached HEAD and no branch given:** stop.
- **The worktree has uncommitted changes:** don't stop. Note them, and sort them once you've read the cards (see "Adopt uncommitted work").

Record the starting HEAD so the final summary can list every commit this run made.

### Notion pages

Find these through the Notion integration:

- **Feature report** (under `Git`): read the branch's full entry — description, implementation points, completion check, and dependencies — including any relevant parent features.
- **Kanban**: the existing database at the project root. Fetch its data source and property schema before querying or updating it.
- **Review**: the parent page for review reports, in the project's teamspace. If the user gave a Review destination, use that one, even if it's in another teamspace. A page and a teamspace can share a name; don't assume they're the same thing.

Use `Project Summary` for general context, and `Git → Commit breakdown` when you need the original plan. If the Review parent is missing or ambiguous, stop before implementing anything, since every commit needs a report.

## 2. Build the queue

### Find the branch's cards

Query the database records directly, paginating as needed, for an exact match on the `Branch` property (it may be a select or a multi-select). Don't match on card titles or partial names, and don't rely on board views, which can hide records.

### Check for work already underway

Look at all of the branch's cards in every stage before starting:

- **Ready or In progress:** another run may have started this card. If the worktree holds its uncommitted changes, adopt them. If it doesn't, treat the card as not started and include it in the queue. If its status says another agent is working on it right now, stop.
- **Review, Awaiting commit, or Awaiting approval:** check whether the step's work is already committed on the branch (look for a commit matching the card in `git log` since the branch diverged). If it is, treat the card as done. If it isn't and the worktree holds its changes, adopt them; this is the normal state of a step left by `implement-feature-step`, which stops for the user to commit. If it isn't and the changes are nowhere to be found, put the card back in the queue and implement it again.
- **Approved:** done.

### Adopt uncommitted work

Committing a card's finished work is this skill's job, even if an earlier run or the user wrote it. Match each uncommitted file to a card, using the `Files` list in the card, its review report, and the file's contents:

- **Belongs to a card that isn't committed yet:** add that card to the queue at its `Step` position and mark it as adopted. Handle it in `Step` order, so earlier cards are committed first.
- **Unrelated to every remaining card:** leave it out of every commit and list it in the final summary. Don't discard or change it.
- **Can't be separated:** stop only if a file mixes changes for several cards or for unrelated work, or if an unrelated change touches a file a remaining card must edit.

For an adopted card, keep the existing work and skip any part of 3.2 that's already done. Run the card's verification and the required checks, finish or fix what's missing within the card's scope, then continue from 3.3. Update its existing report rather than creating a new one, and record the adoption as a decision under "Decisions and review notes".

### Order the queue

The queue is every remaining `Backlog` card (plus any cards resumed above), sorted by the numeric value of the `Step` property, not `Plan order`, the title, or alphabetical order. If step numbers are missing or duplicated so the order isn't clear, stop.

Check each card's `Depends on` relations and recorded blockers. A dependency counts as done if it's approved, already committed on the branch, or committed earlier in this run. A card whose dependency isn't in one of those states, or that has an unresolved blocker, stops the run when its turn comes.

If the queue is empty, report whether the branch is fully done or waiting on the user, and end the run. Don't recreate cards.

## 3. Work through the queue

Repeat these steps for each card in order. Finish one card completely (committed, reported, card moved) before starting the next.

### 3.1 Read the card

Fetch the card's full page body and its entry in the source plan. Compare its scope with the Feature report and the actual code, including what earlier commits in this run changed.

If the plan and the code disagree, choose the approach that best matches the Feature report and the card's intent, and record the decision and why in the step's report under "Decisions and review notes". Only stop if fixing the disagreement would change what the feature does for its users, or would require work that belongs to another card.

### 3.2 Implement

Move the card to `In progress` and confirm it was saved, including a card adopted from a later stage. Then implement only this card. Respect the project's test policy and the planned commit boundaries: an implementation card doesn't take on work from a later test card, and a test card doesn't turn into an unrelated refactor.

Run the verification that matters for this change, plus the project's required checks (build, type-check, lint, tests). Record each command and its actual result.

If something fails, fix it within the card's scope and run the checks again. If you still can't get the required checks to pass after a reasonable effort, stop.

### 3.3 Commit

1. Review the full diff for this step. Make sure it contains only this card's work: no debug code, stray files, or changes that belong to another card.
2. Stage this step's files by path. Don't use `git add -A` or `git add .`, so nothing unexpected (secrets, local config, build output) gets in. If an untracked file looks like it shouldn't be committed, leave it out and mention it in the report.
3. Write the commit message using the repository's convention (check `git log --oneline -20`; default to Conventional Commits). Base the subject on the card title, and add a short body when the reason for the change isn't obvious from the subject.
4. Commit. If a hook fails, fix the problem and commit again; a failed hook means no commit was made, so this is a new attempt, not an amend.
5. Confirm the commit exists and the worktree holds nothing beyond files you deliberately left out, and record the commit SHA.

### 3.4 Publish the report

Create real child pages in this hierarchy:

```text
<project teamspace>
└── Review
    └── <feature ID> - <exact branch name>
        └── <Step number>
```

- **Branch page:** reuse it if it already exists under the Review parent, even if its title is only the branch name; in that case rename it to the format below. Its title is the feature ID from the Feature report, a spaced hyphen, then the full branch name, including `feature/`, for example `F02 - feature/environment-diagnostics`. The slash is part of the title and doesn't create another level.
- **Step page:** titled with the step number, for example `1`. Before creating one, look for an existing report for this step (same parent and step, and the same card link or plan key). If there is one, update it, keep any review notes the user added, and don't create a duplicate.

Write a summary the reviewer can read quickly and still understand the whole commit. Group changes by behavior or responsibility in short bullets, cover every meaningful change, and say which files are involved and why the change was made or what it does. Don't add a bullet for each trivial edit.

Open the report with a `## TLDR` section, then a `---` line, then the technical report. Write the TLDR for a reader who doesn't know the language, libraries, or tools involved, following the `for-dummies` approach: a respectful, adult tone and full technical accuracy. Keep it short, around 300 words for a feature commit and less for a test commit. Give each idea one sentence, define each tool or term in a few words where it first appears, and don't repeat a caveat. Avoid unexplained acronyms. The TLDR must not contradict the sections below it or claim more than they show. Use these bold-labelled parts:

- A bold opening sentence: what the commit adds, framed as the question it answers.
- **Why this exists.** One or two sentences on the problem it solves for the user.
- **What you would see.** One concrete scenario and its result. For a test commit, what the tests try and what they prove. Define any new status or term in a sentence.
- **What was added:** a short numbered list in plain language, with the main file for each item in parentheses.
- **Not included.** What the commit doesn't do, and which later step or feature covers it.
- **Known limits.** The main gaps or unverified assumptions in one or two sentences, or a link to where they're detailed. Leave this part out if there are none.

```markdown
## TLDR
<bold opening sentence, then the bold-labelled parts above>

---

Commit: <short SHA> <commit subject>
Card: <Notion card link> | Feature: <Feature report link>
Parent: <SHA of the previous commit>

## Changes
- `<file or component>`: <what changed and what it enables or fixes>.
- <Next distinct change, including relevant tests, docs, configuration, or dependencies>.

## Verification
- `<command or manual check>` — <actual result and what behavior it covers>.
- <Required check that couldn't be run, and why, if any>.

## Decisions and review notes
- **Decision, <topic>:** <what was chosen without asking the user, and why>.
- **Note, <topic>:** <impact or limitation, how it was established, and the follow-up needed>.
```

Leave out bullets that don't apply, and drop the Decisions and review notes section if there's nothing to put in it. In the combined block, label each item **Decision** (a choice made without asking the user, and why: plan/code mismatches, ambiguous requirements, alternatives rejected) or **Note** (an impact on correctness, regressions, compatibility, security, or data handling, or a specific limitation with its follow-up), followed by a short topic. Keep one line per item, list decisions before notes, and leave out anything the TLDR or the code already makes obvious. Make it clear what you confirmed by running something, what you concluded from reading the code, and what is an unverified assumption. Don't suggest that the commit is approved for merge.

The format is inspired by [pr-audit](https://github.com/akitaonrails/my-skills/blob/master/pr-audit/SKILL.md): evidence-backed verification, concrete findings, and relevant compatibility or risk notes. This skill doesn't run that audit or its full PR checklist; the user runs it separately when the feature PR is ready to merge.

### 3.5 Update the card

1. Read the published report back and confirm its content, title, and parent pages.
2. Add a link to the report and the commit SHA to the card body, keeping the card's implementation plan.
3. Move the card to `Awaiting approval` (or `Review` if the board has no such stage) and confirm it was saved. Never mark it `Approved`; that's the user's decision.

If a Notion write fails or times out, read the page or card back before retrying, and reuse anything that was already created. If you still can't save the report or the card update, don't undo the commit: record what isn't synced and stop.

Then continue with the next card.

## When to stop

Stop the run, without asking for anything, when:

- the project, branch, worktree, or Review parent can't be determined
- uncommitted changes can't be separated from a card's work (see "Adopt uncommitted work")
- the step order is unclear, or a card's dependency or blocker isn't resolved
- required checks still fail after a reasonable effort to fix them within scope
- fixing a plan/code disagreement would change what the feature does or require another card's work
- another agent is actively working on the branch
- Notion can't be updated, so a commit would be left without its report

When you stop:

1. Leave the current step's uncommitted changes in the worktree. Don't commit work that fails its checks, and don't discard it.
2. Record the reason in the card's `Blocked` and `Blocker` properties when the board has them, and leave the card in `In progress`.
3. Keep every commit already made in this run.

## 4. Final summary

When the queue is finished or the run stops, give the user:

- every commit made in this run (short SHA, subject, and its report link), in order
- the branch's state: all steps done, or which card it stopped at and why
- the most important decisions made without asking, and any gaps in verification
- any uncommitted changes left in the worktree

Then end the run. Don't push and don't start anything beyond the branch's cards.
