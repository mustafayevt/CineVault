## Context

The Drives page keeps the chosen drive in `useState`, first drive by default, and every sidebar drive is a plain `<Link to="/drives">`. The drives overview is one React Query (`["drives","overview"]`) shared by the page and the sidebar; the backend command reads the disks on every call, with no cache in front of it.

## Decisions

- **The chosen drive is a search parameter.** `/drives?drive=<id>` is validated like the other routes' searches (a non-empty string, anything else dropped). The page reads it and falls back to the first drive when it is missing or that drive is gone. Choosing a card navigates to the new search as a history entry, so Back returns to the previous choice. Keeping it in the address rather than in a store lets the sidebar link set it with no shared state.
- **The sidebar marks the chosen drive only on the Drives page.** It reads the current location; on `/drives` the drive matching `?drive=` (or the first drive when none is given, the same fallback as the page) gets the selected look and `aria-current="page"`. Elsewhere no drive is marked, as today.
- **Refresh refetches the shared query.** The button calls `refetch()` on the overview query, so the page and the sidebar update together. While the read runs the button is marked busy with a spinning icon (still under reduced motion), its label says it is reading, and a second press is ignored; it is not disabled, so keyboard focus stays on it. A failed refresh keeps the last numbers and says it failed; it does not blank the page.

## Risks / Trade-offs

- Refresh wakes network drives on demand. That is the person's explicit choice, so the "not more often" limit of "Space numbers stay current" covers only the automatic reads.
