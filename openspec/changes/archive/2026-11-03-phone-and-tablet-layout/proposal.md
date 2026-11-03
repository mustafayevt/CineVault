## Why

The browser version can be installed on a phone (`installable-browser-app`), but its layout is built for 1366px and wider: on a 390px phone the 200px sidebar leaves 164px for the page, and on an 820px tablet the Library, Movie and Show pages scroll sideways. Many actions also live in right-click menus that a phone can't open. Step 2 of #51 makes the browser version usable one-handed on a phone and comfortable on a tablet, without changing the desktop app.

## What Changes

- **Compact layout** in the browser version below 1024px: the sidebar panel becomes a drawer opened from a menu button in the title bar; the title bar holds the menu button, Back and a full-width search; the frame keeps clear of the notch and the home indicator (`viewport-fit=cover` with safe-area padding). The desktop window never switches (its minimum width is 1100 and its zoom does not count).
- **Touch targets:** on a touch screen the control-height tokens rise to touch sizes (nav rows, menu items, fields and large buttons 44px).
- **Long-press opens the right-click menu** on touch screens, after selecting the pressed item like a right-click does.
- **Pages fit their width:** every page shows its content without horizontal scrolling at 390×844 and 820×1180. The Library toolbar wraps, the poster grid picks its columns from the width, tables become lists on a phone, the title pages stack the poster, facts and Technical card, and the pickers and other pages use one column.
- `bun run shots:compact` takes every route at 390×844 and 820×1180 and fails on horizontal overflow.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `app-shell`: new "Phone and tablet layout"; "Legible at common screen sizes" adds the phone and tablet sizes of the browser version.
- `ui-design-system`: new "Touch targets"; "Component sizes are fixed" names the touch sizes; "Pickers are pages and only one dialog" allows the navigation drawer.
- `desktop-interaction`: "Own right-click menus" opens on a touch long-press too.

## Impact

- UI only: the shell (`AppShell`, `TitleBar`, `SidebarPanel`, new `NavDrawer` and `useCompactLayout`), `tokens.css`, `ContextMenu`, `ui/index.html`, and the page layouts of every route. No backend, IPC or file changes.

## Out of scope

- A separate phone design with different navigation (bottom tabs, swipe gestures).
- Playback changes for iOS.
