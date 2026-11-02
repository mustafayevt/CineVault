## 1. Shell

- [x] 1.1 `useCompactLayout` (browser only, below 1024px), `NavDrawer` with `SidebarPanel variant="drawer"`, the compact `TitleBar` (menu, Back, full-width search), safe-area padding and `viewport-fit=cover`; remove the unused companion state. Verify: `compactLayout.test.tsx`.
- [x] 1.2 Touch token layer (`useTouchTargets`, `:root[data-touch="true"]`). Verify: `compactLayout.test.tsx` touch case; `tokens.test.ts` still passes.
- [x] 1.3 Long-press opens the context menu after targeting the pressed item. Verify: `floating.test.tsx` long-press case.
- [x] 1.4 `bun run shots:compact` script. Verify: `bun run check:scripts`.

## 2. Library

- [x] 2.1 Library toolbar, filter chips, poster grid, table view, collections and the selection bar fit 390 and 820 widths. Verify: `bun run shots:compact` passes for the Library routes; Library tests pass.

## 3. Title pages

- [ ] 3.1 Movie, Show, Season, Episode and Edit pages stack their header, facts and Technical card on narrow widths, with the header actions wrapping. Verify: `bun run shots:compact` passes for these routes; `bun run check:facts` still passes; title page tests pass.

## 4. Pickers and other pages

- [ ] 4.1 Scrape queue and match picker, Artwork, Rename preview, Drives, Torrents, History, Settings, Release notes and the gallery fit 390 and 820 widths. Verify: `bun run shots:compact` passes for these routes; their tests pass.

## 5. Integration

- [ ] 5.1 Run the full UI checks, `bun run shots` at the desktop sizes (no change there), `bun run shots:compact`, `bun run check:facts`, and `openspec validate phone-and-tablet-layout --strict`. Verify: every command passes.
- [ ] 5.2 Needs a human with a phone and a tablet: one-handed use of Library, a Movie page, a Show page, Scrape, Rename preview, Delete and Play on a 390px phone, and the drawer and long-press menus on iOS and Android.
