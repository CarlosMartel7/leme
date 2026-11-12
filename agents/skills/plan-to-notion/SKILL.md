---
name: plan-to-notion
description: Use only in Plan mode to deliver a project plan to a user-named Notion teamspace in a page called Project Summary.
---

# Plan to Notion

Apply this skill only when the active collaboration mode is Plan. Outside Plan mode, ask the user to switch modes before using it.

Receive the destination teamspace name from the user. If it is missing or ambiguous, ask before publishing.

Deliver the complete plan through the Notion integration as a page titled exactly `Project Summary`, directly at the root of that teamspace. Do not place it in Private or inside `Git`. Reuse the existing `Project Summary` for this project when present; otherwise create it.

Preserve the plan's language and content. Verify the saved page's title, teamspace, and contents, then return its link.

Follow the active mode's tool permissions. If they prohibit publishing to Notion, explain that the plan has not been saved and provide the draft for publication when permitted.
