<!--
Decision template. Record a decision when the user chooses between options that
affect the plan, the scope of a step or a convention. Write the user's reason in
their own words. If the user gave no reason, write exactly
"rationale not provided"; never supply one yourself. Times use ISO 8601 with an
offset.
-->

# Decision: <short title>

| Field | Value |
| --- | --- |
| Task ID | `<task ID>` |
| Step ID | `<step ID, or "none" for a feature-level decision>` |
| Decided by | <the user> |
| Recorded by | `<role>` (`<agent CLI>`, session `<session ID or "unknown">`) |
| Decided at | `<time the user gave the answer>` |
| Recorded at | `<time this record was written>` |

## Question

<The question put to the user, as asked.>

## Options

1. <Option> — <consequence>.
2. <Option> — <consequence>.

## Choice

<The option the user chose.>

## Stated reason

<The user's reason, quoted or closely paraphrased, or "rationale not provided".>

## Impact on the plan

- **Plan version:** `<unchanged / old → new>`.
- **Estimated commits (y):** `<unchanged / old → new>`.
- **Affected steps:** <step IDs and how they change, or "none">.

---

## Example

### Decision: allow a missing session ID

| Field | Value |
| --- | --- |
| Task ID | `T-12` |
| Step ID | `T-12-S2` |
| Decided by | the user |
| Recorded by | `feature-agent` (`claude`, session `unknown`) |
| Decided at | `2026-11-09T14:20:00-03:00` |
| Recorded at | `2026-11-09T14:22:00-03:00` |

#### Question

Should the `sessions` table require a session ID when a row is created?

#### Options

1. Require it — every row identifies its conversation, but a launch cannot be
   recorded until the agent CLI reports its ID.
2. Allow it to be empty — the launch is recorded immediately and the ID is
   filled in later.

#### Choice

Option 2: allow it to be empty.

#### Stated reason

"Codex only shows the session ID after the first turn, and I want the launch
recorded even if it fails before that."

#### Impact on the plan

- **Plan version:** `2 → 3`.
- **Estimated commits (y):** `4 → 5`.
- **Affected steps:** new step `T-12-S5` fills in the session ID when the agent
  reports it.
