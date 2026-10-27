# ui-design-system Specification

## Purpose
Defines the visual and interaction language every CineVault screen shares, and the rules contributors follow when they change the UI: the design tokens and the files that hold them, themes, the surface ladder, dividers, radii, the type scale, the gold accent, the floating recipe, motion and reduced motion, status glyphs, the verb glossary, icons, loading and feedback, the shared components shown on the `/gallery` route, picker pages and in-place details, the brand mark and app icons, and a window that never opens white. This specification is the design guide for the UI; the code in `ui/src/styles` and `ui/src/components` implements it.

## Requirements

### Requirement: Tokens in two agreeing files
Every colour, radius, spacing, shadow, size, font, duration and easing SHALL come from the design tokens declared as CSS variables in `ui/src/styles/tokens.css`. Its twin `ui/src/styles/design-tokens.json` SHALL list the same tokens with the same values, and `ui/src/styles/tokens.test.ts` SHALL fail when the two disagree, when a type style of the twin is missing from `typography.css`, or when the two themes declare different colour tokens. A token change SHALL update both files together.

#### Scenario: Token files disagree
- **WHEN** a token's value in `tokens.css` differs from its value in `design-tokens.json`
- **THEN** the token test fails and names the token

#### Scenario: Theme missing a token
- **WHEN** a colour token is declared for the dark theme only
- **THEN** the token test fails

### Requirement: Stylesheets read the tokens
The stylesheets beside the tokens (`base.css`, `typography.css`, `motion.css`, `a11y.css` and `fonts.css` in `ui/src/styles`) SHALL read the tokens and hold no values of their own. In markup, components SHALL use the Tailwind utilities that map the tokens or `var(--token)`.

#### Scenario: A new motion class
- **WHEN** a contributor adds a transition to `motion.css`
- **THEN** its duration and easing are motion tokens, not literal values

### Requirement: No hard-coded colours
Outside `tokens.css`, no UI source file SHALL hold a hex, rgb or hsl colour literal, except the rating-source brand colours and logos, the generated placeholder art (posters, backdrops, stills, faces) and the pictures of the synthetic development backend. A test SHALL fail and name the file and line when another file holds one. Old token, radius, shadow and type-class names SHALL NOT come back, and a test SHALL fail when one does.

#### Scenario: A hard-coded colour
- **WHEN** a component file contains `#2c2c29` instead of a token
- **THEN** the stray-colour test fails and names the file and line

### Requirement: Dark and light themes
Every screen, component and state SHALL be designed for the dark theme (the default) and the light theme, and SHALL use only theme tokens, so switching theme changes every element at once. Colours SHALL switch by `data-theme` on any element, so a panel can show the other theme. Text over artwork SHALL sit on the scrim and use the scrim text colour in both themes.

#### Scenario: Every screen in both themes
- **WHEN** the screenshot check renders each route in dark and in light
- **THEN** no element keeps the other theme's colours

### Requirement: Surface ladder
The UI SHALL use one ladder of warm-neutral surfaces per theme, never blue: window (page, title bar, gutter), card (cards, fields, docked panes), raised (default buttons), active (hover, selected segment, active nav item), pressed, and floating (everything that floats, with its own hover step), plus a well colour for fields inside a floating surface. The floating surface SHALL be the lightest in the light theme, and a card and a docked pane SHALL use the same step.

#### Scenario: Card and docked pane
- **WHEN** a card and the Scrape page's candidate pane are shown together
- **THEN** both use the card surface, not two different greys

#### Scenario: Field inside a dialog
- **WHEN** the permanent-delete dialog shows its title list and its confirmation field
- **THEN** both use the well colour

### Requirement: Dividers in three strengths
Lines SHALL come in three strengths only: a row hairline between table and list rows, a section divider under headers and between docked panes, and the 1px edge of controls and floating surfaces (with a key-cap edge for key caps). Dividers SHALL start at the 24px content inset and never touch the sidebar panel.

#### Scenario: Table rows
- **WHEN** the Library table is shown
- **THEN** rows are separated by the row hairline and the header by the section divider

### Requirement: Radius scale with nesting
Radii SHALL come from the scale 4 (badges, tags, checkboxes, segments, key caps, small thumbnails), 6 (buttons, fields, nav and menu items), 10 (cards, posters, menus, toasts), 14 (sidebar panel, selection bar, dialogs, preview panels) and 999 (chips, pills and round controls). A shape nested in another SHALL have a radius no larger than the outer radius minus the padding between them.

#### Scenario: Progress card in the panel
- **WHEN** the progress card sits 8px inside the 14px sidebar panel
- **THEN** its radius is at most 6px

### Requirement: Type family
All text SHALL be set in Archivo (variable weight and width), bundled with the app and loaded from local files only, with tabular numerals where numbers line up. JetBrains Mono, also bundled, SHALL be used only for literal strings (paths, file names, NFO text, template tokens, release names) and key caps. No serif face SHALL ship.

#### Scenario: A path on the Movie page
- **WHEN** the Movie page shows the file name of the main video
- **THEN** the file name is in JetBrains Mono and the label next to it in Archivo

### Requirement: Type scale
Text SHALL use only the sizes 11, 12, 13, 14, 16, 20 and 28px and the weights 500, 600 and 700, with 700 only for headings and numbers. Every page title SHALL use 20px and the Movie and TV show hero titles SHALL share 28px. Section labels SHALL use one style: 11px, semibold, uppercase, 0.06em tracking, muted.

#### Scenario: Hero titles agree
- **WHEN** the Movie page and the TV show page are compared
- **THEN** their titles have the same size and weight

#### Scenario: Section label
- **WHEN** the Movie page shows its Technical and Cast sections
- **THEN** both labels use the same 11px uppercase muted style

### Requirement: Gold only where it guides
The one accent, champagne gold, SHALL be used only for the brand mark's frame and tittle, the active page icon, progress bars, primary buttons, the selection (selected rows, checks, the current row's left mark) and a focused field's border. Links, the New marker, drive bars, chips and segments SHALL NOT be gold; an active chip and the sidebar count pills SHALL use the gold tint. Each screen SHALL have at most one primary button.

#### Scenario: Scrape page buttons
- **WHEN** the Scrape page shows "Scrape with this" next to a bulk accept button
- **THEN** only "Scrape with this" is gold and the other is a raised default button

### Requirement: Contrast without opacity states
Text SHALL meet 4.5:1 contrast, and large text, control edges, the focus ring and gold marks SHALL meet 3:1, against their composited background in both themes, checked by a contrast test. Disabled text SHALL be told apart by its own colour at full opacity, never by opacity, and SHALL keep at least 2.5:1 on the card surface. Windows high-contrast mode SHALL use the system colours.

#### Scenario: Primary button contrast
- **WHEN** a primary gold button shows its label
- **THEN** the pair meets 4.5:1 in both themes

#### Scenario: Disabled button
- **WHEN** a button is disabled
- **THEN** its label uses the disabled text colour at full opacity

### Requirement: Keyboard focus ring and field border
The keyboard focus ring SHALL be drawn in the text colour, 2px wide with a 2px offset, in both themes, and SHALL show only for keyboard focus. A focused text field SHALL instead show its gold border, with no halo.

#### Scenario: Keyboard focus on a chip
- **WHEN** the user tabs to a filter chip
- **THEN** a 2px text-coloured ring with a 2px offset surrounds it

#### Scenario: Clicked field
- **WHEN** the user clicks into the library search
- **THEN** its border turns gold and no ring is drawn

### Requirement: One floating recipe
Everything that floats (sidebar panel, menus, popovers, selection bar, tooltips, toasts, the dialog, artwork previews) SHALL use the floating surface, a 1px edge and the one floating shadow from the tokens. Page content and docked panes SHALL stay flat, with no shadow and no radius against the window. Only menus, tooltips, popovers, toasts, the selection bar and the permanent-delete dialog SHALL float over a page.

#### Scenario: Menu and selection bar
- **WHEN** the Move to… menu opens above the selection bar
- **THEN** both have the same surface, edge and shadow

### Requirement: Flat surfaces
The app icon, the brand mark, generated posters and backdrops, skeletons, toasts and every surface SHALL be flat colour, with depth from tonal steps and crisp edges only. The only gradients SHALL be readability scrims over real artwork, each marked as a scrim in the code, and a test SHALL fail on any other gradient.

#### Scenario: Placeholder backdrop
- **WHEN** a title without a backdrop opens on its title page
- **THEN** the hero shows a flat backdrop made from the title's poster palette and motif, with no gradient

### Requirement: Purposeful motion
Motion SHALL come only from the motion tokens: an ease-out curve for entering or changing, an ease-in curve for leaving, no bounce or overshoot, and nothing longer than 250 ms. Only transform and opacity, and the row track of expanding sections, SHALL animate; width, height, top and left SHALL NOT. There SHALL be no page-load choreography and no slide across the window.

#### Scenario: Progress bar animates by transform
- **WHEN** the progress card's bar advances
- **THEN** it moves with `transform: scaleX`, not by animating its width

### Requirement: Motion vocabulary
Hover and pressed colour changes SHALL take 110 ms. Menus, tooltips and popovers SHALL fade in and grow from 0.98 at their anchor over 140 ms. A page change SHALL be a 120 ms crossfade. The selection bar and toasts SHALL slide up over 180 ms, and expanding rows and sections SHALL open over 180 ms. A poster SHALL lift 2px and brighten on hover, progress and space bars SHALL glide to new values, and check badges and Undo states SHALL crossfade.

#### Scenario: Menu opens from its anchor
- **WHEN** the user opens the Move to… menu
- **THEN** it fades in and grows from 0.98 to full size from the Move button in about 140 ms

#### Scenario: Space bar after a move
- **WHEN** a move frees space on a drive
- **THEN** the drive's space bar glides to its new value instead of jumping

### Requirement: Reduced motion stops movement
When the operating system asks for reduced motion, or the `appearance.motion` setting is Reduced, the root SHALL carry `data-motion="reduced"` and every movement SHALL stop: nothing SHALL scale, slide, lift or grow, menus, tooltips, the selection bar and toasts SHALL appear with an opacity fade only, expanding sections and bars SHALL change at once, and skeletons and indeterminate bars SHALL be still. Every motion class SHALL have a reduced-motion rule, checked by a test.

#### Scenario: Reduced motion opens a menu
- **WHEN** reduced motion is on and the user opens the Move to… menu
- **THEN** the menu appears with at most a short opacity fade and no scaling

#### Scenario: Reduced motion expands an episode
- **WHEN** reduced motion is on and the user opens an episode row
- **THEN** its details appear at once with no height animation

### Requirement: Status glyphs and colours
States SHALL be shown with a glyph and a word or tooltip, never colour alone: done (check, ok colour), new (hollow muted dot), not matched or uncertain (warning triangle, warning colour), missing (cross, warning colour), in progress (ring, neutral) and permanent or failed (danger colour). The danger colour SHALL be used only for permanent deletes and failures.

#### Scenario: Missing episode
- **WHEN** an episode the provider lists is not on disk
- **THEN** its row shows a cross with "Not on disk" in the warning colour, not the danger colour

### Requirement: Poster stage badges
A poster SHALL carry at most one stage badge in its corner: a check for Scraped or Renamed, a warning badge for a title that needs attention, or a hollow dot for New. The badge SHALL be named for screen readers and in a tooltip, and the Library table's stage column SHALL name the stage in words.

#### Scenario: Scraped poster
- **WHEN** a movie is Scraped
- **THEN** its poster shows the check badge, and hovering it says "Scraped"

### Requirement: One verb glossary
User-facing text SHALL use one vocabulary on buttons, menus, toasts and History: Scan, Scrape and Rename, the stages New, Scraped and Renamed, and Artwork…, Move to…, Rewrite NFO, Delete to Recycle Bin (Trash on macOS) and Delete permanently…. Scrape on a scraped title SHALL open the match picker with the current match first. `ui/src/i18n/verbs.test.ts` SHALL fail when a catalogue message contains Identify, Enrich, Rescrape, Scrape again, Change match, Organize or Search & scrape.

#### Scenario: Banned verb in a message
- **WHEN** a catalogue message reads "Rescrape 3 titles"
- **THEN** the verb test fails and names the message key

#### Scenario: Scrape on a scraped title
- **WHEN** the user clicks Scrape on the page of a scraped movie
- **THEN** the match picker opens with the current match pinned first

### Requirement: No other product names
No user-facing text, code comment, test or file name SHALL name another media-manager product. Compatibility SHALL be described as the Kodi, Jellyfin, Emby or MediaPortal formats.

#### Scenario: Describing NFO compatibility
- **WHEN** a message explains which NFO files CineVault writes
- **THEN** it names the Kodi, Jellyfin or Emby format and no other application

### Requirement: Icons
Icons SHALL come from `lucide-react` through the shared `Icon` component, drawn at stroke width 1.75: 12px in chips, 16px in controls and 18px in nav items and the sidebar's icon row. Every icon-only button SHALL have an `aria-label`, which SHALL also be its tooltip.

#### Scenario: Icon-only button
- **WHEN** the sidebar shows the History icon button
- **THEN** it is an 18px lucide icon at stroke 1.75, named "History" for screen readers and in its tooltip

### Requirement: Component sizes are fixed
Components SHALL keep fixed sizes from the size tokens: buttons 28px (small 24, large 32 for the dialog and commit steps only), icon buttons 28×28, fields 30px (34 for the main search), segments 24px in a 2px-padded track, chips 24px, tags and badges 18px, nav items 32px, table rows 34px with 30px headers, and menu items 28px. Chips and buttons on one row SHALL share a centre line.

#### Scenario: Toolbar row
- **WHEN** the Library toolbar shows chips and a button on one row
- **THEN** the chips are 24px, the button 28px, and their centres line up

### Requirement: Shared components
Every screen SHALL be built from the shared components in `ui/src/components`: buttons, fields, select, checkbox, switch, segmented control, chip, tags and badges, key cap, card, section label, nav item, progress and space bars, progress card, selection bar, poster tile, table, toast, skeleton, status tag, path text, file size, placeholder poster, brand mark, menu, context menu, popover, tooltip, dialog and the motion helpers. One quality badge and one score badge SHALL be used everywhere.

#### Scenario: One quality badge everywhere
- **WHEN** the Library grid, the Library table, Drives and Torrents show a resolution
- **THEN** all four use the same quality badge with the same size, colours and words

### Requirement: Component gallery
Development builds SHALL serve a `/gallery` route that shows every shared component with all its states (hover, pressed and focus included) in a dark and a light column, and that can replay each motion. Release builds SHALL NOT route to or contain the gallery.

#### Scenario: Gallery in a release build
- **WHEN** a release build is asked for `/gallery`
- **THEN** the not-found page shows

#### Scenario: Gallery in development
- **WHEN** a contributor opens `/gallery` in a development build
- **THEN** each component shows in both themes with its states

### Requirement: Disabled controls explain themselves
A disabled control that has a reason SHALL stay focusable and show, on hover and on keyboard focus, a one-line reason and the action that enables it.

#### Scenario: Continue without a tested key
- **WHEN** the user focuses the disabled Continue button on the first-run TMDB key step
- **THEN** a line says "Test your TMDB key first." with "Paste the key and press Test, or skip this step."

### Requirement: Late skeletons and placeholders
While data loads for longer than 150 ms, the UI SHALL show flat skeletons shaped like the content (2:3 posters, rows of the row height, cards of the card size) that reserve their space from the first frame, so content replaces them without moving the layout. Spinners SHALL appear only inside buttons, no screen SHALL wait for the network before it first renders, and images SHALL load as they come near the viewport and fade in.

#### Scenario: Grid loading
- **WHEN** the Library grid's data takes 400 ms
- **THEN** poster-shaped skeletons appear after 150 ms and the posters replace them in place

### Requirement: Generated placeholder art
A title without poster art SHALL show a generated typographic poster: a palette and motif chosen from the title, the title set in the poster and the brand symbol small in its corner. The same title SHALL always give the same poster, named with the title. A title without a backdrop, and an episode without a still, SHALL get the matching generated picture.

#### Scenario: Same title, same poster
- **WHEN** a not-yet-scraped "Example Movie (2020)" shows in the grid, the table thumbnail and its title page
- **THEN** all three show the same generated poster with its title, never a grey box

### Requirement: Toasts confirm and offer Undo
Reversible actions SHALL confirm with a toast at the bottom right that starts with the action's glyph and offers Undo with its key cap for 8 seconds (other toasts stay 6 seconds); Ctrl+Z SHALL also undo. A thin time bar SHALL pause while the pointer or focus is on the toast. At most three toasts SHALL show, identical toasts SHALL merge with a count, and toasts SHALL sit above the selection bar. An error toast SHALL say what failed.

#### Scenario: Identical toasts merge
- **WHEN** the same "Rewrote NFO files for 1 title" result arrives three times
- **THEN** one toast shows it with "×3"

#### Scenario: Toast and selection bar
- **WHEN** the selection bar is visible and a toast appears
- **THEN** the toast sits above the bar and both stay readable

### Requirement: Pickers are pages and only one dialog
Picking and editing SHALL happen on pages inside the shell, not in drawers or dialogs: the Scrape match picker, the Artwork picker, the Rename preview and the Edit page. A picker's header SHALL name what is picked and for which title ("Scrape · Example Movie (2020)", "Artwork · Example Movie (2020)") with Cancel or Done and its Esc hint, and closing it SHALL return to the page and scroll position it was opened from. The only dialog in the app SHALL be the permanent-delete confirmation.

#### Scenario: Back to the Movie page
- **WHEN** the user opens the artwork picker from the Movie page and presses Esc
- **THEN** the Movie page shows again at the same scroll position

### Requirement: Preview before applying
A picker SHALL show the chosen candidate in a large preview with what applying it changes before anything is written. A click SHALL only choose; "Scrape with this" or "Use this" (or Enter) SHALL apply, and Cancel SHALL write nothing. Applying SHALL update the page at once, confirm with an Undo toast and record a History entry; if the write fails, the change SHALL roll back with a message that says why.

#### Scenario: Click does not apply
- **WHEN** the user clicks a poster candidate
- **THEN** the preview shows it with its size, language and source, and the title's poster is unchanged until "Use this"

#### Scenario: Write fails
- **WHEN** writing the chosen poster fails because the folder is read-only
- **THEN** the old poster stays and a message says the folder could not be written

### Requirement: Picker results stream per provider
Picker results SHALL appear as each provider answers, never waiting for all of them. A slow or failing provider SHALL NOT hide the others' results, and a failure SHALL show as one line with Retry in place of that provider's results.

#### Scenario: One provider slow
- **WHEN** TMDB answers at once and a second provider takes 6 seconds
- **THEN** TMDB's candidates are listed at once and the second provider's are added when they arrive

### Requirement: Details on demand
Title pages SHALL show a short technical summary by default and a "Show all streams and details" link that expands every container, video, audio and subtitle field and the folder's file list in place, with "Copy as text" and "Hide details". Edit fields SHALL use typed controls: certification with its country, languages and countries by name, dates as dates, and provider IDs as labelled links.

#### Scenario: Expand details
- **WHEN** the user clicks "Show all streams and details" on the Movie page
- **THEN** the stream tables and the file list appear in place, and "Hide details" collapses them

### Requirement: Paths shown in full
Folder and file paths SHALL be shown in the mono face, shortened in the middle when too long so sibling folders stay distinct, with the full path in a tooltip and for assistive technology, and SHALL be selectable for copying.

#### Scenario: Long path
- **WHEN** a path is longer than its cell in the Drives table
- **THEN** the middle is cut with an ellipsis and hovering shows the full path

### Requirement: Brand mark
The app SHALL show the CineVault brand mark (a C-shaped ring with round terminals and a 2:3 frame in its opening, with the wordmark) in the top row of the expanded sidebar panel, on the first-run setup and on generated posters as a small maker's mark. The ring and wordmark SHALL be drawn in the text colour and the frame and the i's tittle in gold, so the mark follows the theme. The mark SHALL be hidden from assistive technology next to visible text and named "CineVault" where it stands alone.

#### Scenario: Light theme
- **WHEN** the app switches to the light theme
- **THEN** the mark's ring and letters turn to the light text colour and its frame to the light gold, with no other change

#### Scenario: Sidebar header
- **WHEN** the app opens with the sidebar expanded on Windows
- **THEN** the top row shows the symbol and "CineVault", followed by Back and Forward

### Requirement: App icons and installer images
The bundle SHALL use the flat CineVault app icon on every platform: on Windows an ICO holding 16, 24, 32, 48, 64 and 256 pixel images, on macOS an ICNS. The Windows NSIS installer SHALL show the CineVault header (150 × 57) and sidebar (164 × 314) images and the MSI the CineVault banner (493 × 58) and dialog (493 × 312) images. The macOS disk image SHALL open on a 660 × 400 background with the app at (180, 190) and the Applications link at (480, 190).

#### Scenario: Small Windows icon
- **WHEN** the app runs on Windows at 100% scaling
- **THEN** the taskbar shows the 24 pixel image of the icon, not a downscaled 256 pixel image

#### Scenario: Disk image layout
- **WHEN** the DMG is opened on macOS
- **THEN** the window is 660 × 400 with the app icon on the left and the Applications link on the right

### Requirement: No white first frame
The main window SHALL paint the window ground of the theme in use from its first frame on Windows and macOS: the native window SHALL start on the dark ground, and the page SHALL apply the theme, motion and zoom of the last session before the interface loads, so neither a white frame nor the other theme flashes at start.

#### Scenario: Cold start in dark theme
- **WHEN** CineVault starts with the dark theme and the interface is still loading
- **THEN** the window shows the dark window ground, never white

#### Scenario: Cold start in light theme
- **WHEN** CineVault starts and the last session used the light theme
- **THEN** the page paints the light window ground before the interface appears
