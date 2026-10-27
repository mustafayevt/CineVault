# CineVault UI

React 19 + TypeScript + Vite + Tailwind v4 frontend, hosted by the Tauri 2 shell in `src-tauri/`.

Stack: Radix (`radix-ui`), TanStack Router / Query / Virtual, Zustand, cmdk, Motion, lucide-react.

- `bun install` (here and in the repo root, where `@tauri-apps/cli` lives)
- `bun run dev` serves Vite on `127.0.0.1:1420` (the Tauri dev URL); the desktop window is started from
  the repo root with `bun run tauri dev`, which also launches this dev server.
- `bun run typecheck`, `bun run test`, `bun run build`.

Design tokens are in `src/styles/tokens.css` (colours switch with `[data-theme]`, mapped to Tailwind utilities). Use the CSS variables or token-backed utilities; do not
hard-code colours.

## Design system components

- Fonts (Geist, Geist Mono, Instrument Serif) are bundled from the `@fontsource` packages through
  `src/styles/fonts.css`; nothing loads from the network.
- `src/styles/design-tokens.json` is the machine-readable twin of `tokens.css`. `tokens.test.ts` checks
  that `tokens.css` matches it exactly (names and values), so update both together.
- Type styles are `.type-*` classes (`src/styles/typography.css`).
- Base components live in `src/components/` (Button, IconButton, FilterChip, SegmentedControl, Input,
  Badge, TechBadge, Toast, Drawer, InspectorPanel). Open `/gallery` to see them in both themes.
- Contrast: `src/styles/contrastPairs.ts` lists the token pairs the components use and the AA
  threshold each must meet; `a11y.contrast.test.ts` checks them in both themes. Pairs the published tokens
  do not bring to AA are listed in `KNOWN_TOKEN_SHORTFALLS`; keep components off them.

## Accessibility infrastructure

- `src/styles/a11y.css` holds the focus ring (2px `--focus-ring`, 2px offset), the reduced-motion
  rules and the Windows high-contrast (`forced-colors`) styles. `a11y.css.test.ts` checks them.
- Reduced motion is a 100 ms crossfade: slide-ins use `--motion-shift`, which becomes 0, and transitions
  keep only colour and opacity. It follows `prefers-reduced-motion` unless the Motion setting
  (`appearance.motion`: Match system, Reduced, Full) sets `data-motion` on `<html>`. Animation-library
  motion follows the same setting through `MotionConfig` in `AppShell`. A shimmer element should carry
  `data-cv-shimmer` so reduced motion stops it.
- `src/lib/roving.ts` has `useRovingTabindex` and `nextRovingIndex` for groups that are one Tab stop
  (toolbars, lists, the poster grid): arrows, Home/End, optional wrap, grid columns, disabled items.
- `features/chrome/shell.a11y.test.tsx` runs axe-core over the shell (title bar, sidebar, pages,
  notification centre). jsdom cannot measure rendered colours, so axe's
  `color-contrast` rule is off there and `a11y.contrast.test.ts` covers contrast from the tokens.

## Strings, dates, numbers and sizes

- No inline English in the shell: write `t("key", { param })` (from `src/i18n/catalog.ts`) and add the
  message to `src/i18n/messages/en.ts`. Plurals: `{count, plural, one {# job} other {# jobs}}`.
- Show dates, times, numbers and file sizes with `useFormat()` (or `<FileSize bytes>`), never
  `toLocaleString()` or a shared `Intl` object. They follow Settings > General > Regional format and
  Settings > Advanced > File size units (base 10: GB, base 2: GiB).
