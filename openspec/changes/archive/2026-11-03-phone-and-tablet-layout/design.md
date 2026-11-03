## Context

`main` is already a size container (`@container-size`, named `pane`), so pages can respond to the width they get rather than to the window. The control heights are tokens (`--h-btn`, `--h-nav`, `--h-menu-item`, …). Radix's context menu opens on a touch long-press but did not run our `onTarget`. The desktop window has a minimum width of 1100 and its own zoom.

## Decisions

- **One breakpoint for the shell, container queries for pages.** The shell switches at a 1024px window (`useCompactLayout`, `(max-width: 1023.98px)`), only in the browser transport, so the desktop app at any zoom keeps its panel. Pages use the `pane` container width (`@max-[…]/pane:` and friends) so they fit whatever width the shell leaves them, including the desktop panel's widths.
- **Drawer = the same panel.** `NavDrawer` renders `SidebarPanel variant="drawer"` in a Radix dialog at the left over the scrim, sliding with the existing `.cv-slide-up` vocabulary (so reduced motion fades). It closes when the address changes, on Esc and on a scrim tap; focus starts on the current page's link and returns to the menu button. The drawer variant is always full width, has no collapse toggle and no back/forward (the compact title bar has Back; the hardware/keyboard back input moves with it). The unused "companion" state of the shell store is removed.
- **Touch sizes as a token layer.** `useTouchTargets` sets `data-touch` on the root in the browser version when the main pointer is coarse; `:root[data-touch="true"]` raises the height tokens. Components keep reading the same tokens, so nothing else changes; the desktop app on a touch laptop is untouched.
- **Safe areas.** `viewport-fit=cover` is in `ui/index.html` (harmless in the desktop webview); in the compact layout the frame pads by `env(safe-area-inset-*)` and the drawer by the larger of the gutter and the inset.
- **Long-press target.** `ContextMenu` remembers the element a non-mouse pointer went down on and calls `onTarget` with it when the menu opens, so a long-press selects the pressed item alone, as a right-click does.
- **Checking.** `bun run shots:compact` (every route at 390×844 and 820×1180) fails on horizontal overflow through the existing overflow check of the screenshot script.

## Risks / Trade-offs

- Hover-only affordances (poster hover controls) stay hover-only; every one of their actions is also in the long-press menu or on the title page.
- The touch token layer makes dense desktop tables taller on a touch tablet; that is the intended trade for tappable rows.
