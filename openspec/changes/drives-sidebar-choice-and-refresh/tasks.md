## 1. Chosen drive in the address

- [x] 1.1 Add `validateDrivesSearch` (`?drive=<id>`, anything else dropped) to the `/drives` route; the Drives page reads the chosen drive from it (first drive when missing or gone) and choosing a card navigates to `?drive=<id>`. Verify: `drivesPage.test.tsx` covers opening with `?drive=`, a missing or unknown id, and choosing a card.
- [x] 1.2 Sidebar drives link to `/drives?drive=<id>` and, on the Drives page, mark the chosen drive selected with `aria-current="page"`. Verify: `sidebar.test.tsx` covers the link target, the mark on `/drives` and no mark elsewhere.

## 2. Refresh

- [x] 2.1 Add the Refresh icon button next to "Updated …" in the Drives header: refetches the overview, busy with a spinning icon while reading (a second press waits for it), then "Updated just now"; a failed refresh keeps the numbers and says it failed. Verify: `drivesPage.test.tsx` covers a refresh that reads again, the busy state and a failure.

## 3. Integration

- [x] 3.1 Run `bun run lint`, `typecheck`, `test` and `format:check` in `ui/`, `openspec validate drives-sidebar-choice-and-refresh --strict`, and `bun run shots -- --routes /drives` in dark and light with no console errors; add the changelog line and run `bun run site:changelog`. Verify: every command passes.
