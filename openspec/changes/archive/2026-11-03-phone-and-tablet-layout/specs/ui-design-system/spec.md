## ADDED Requirements

### Requirement: Touch targets
In the browser version on a touch screen, the control-height tokens SHALL rise to touch sizes: nav items, menu items, fields, the main search and large buttons 44px, buttons 40px, small buttons, segments and chips 36px, table rows 48px with 36px headers. The desktop app SHALL keep its sizes, also on a touch screen.

#### Scenario: Phone menu
- **WHEN** a person long-presses a poster on a phone
- **THEN** each item of the menu that opens is 44px high

## MODIFIED Requirements

### Requirement: Component sizes are fixed
Components SHALL keep fixed sizes from the size tokens: buttons 28px (small 24, large 32 for the dialog and commit steps only), icon buttons 28×28, fields 30px (34 for the main search), segments 24px in a 2px-padded track, chips 24px, tags and badges 18px, nav items 32px, table rows 34px with 30px headers, and menu items 28px. Chips and buttons on one row SHALL share a centre line. On a touch screen in the browser version the touch sizes of "Touch targets" SHALL replace these.

#### Scenario: Toolbar row
- **WHEN** the Library toolbar shows chips and a button on one row
- **THEN** the chips are 24px, the button 28px, and their centres line up

### Requirement: Pickers are pages and only one dialog
Picking and editing SHALL happen on pages inside the shell, not in drawers or dialogs: the Scrape match picker, the Artwork picker, the Rename preview and the Edit page. A picker's header SHALL name what is picked and for which title ("Scrape · Example Movie (2020)", "Artwork · Example Movie (2020)") with Cancel or Done and its Esc hint, and closing it SHALL return to the page and scroll position it was opened from. The only dialog in the app SHALL be the permanent-delete confirmation. The one drawer SHALL be the navigation drawer of the compact layout, which holds the sidebar panel and nothing else.

#### Scenario: Back to the Movie page
- **WHEN** the user opens the artwork picker from the Movie page and presses Esc
- **THEN** the Movie page shows again at the same scroll position
