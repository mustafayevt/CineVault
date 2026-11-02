## ADDED Requirements

### Requirement: Phone and tablet layout
In the browser version, a window narrower than 1024px SHALL use the compact layout: the sidebar panel SHALL be hidden and open as a drawer from a menu button at the left of the title bar, the title bar SHALL hold the menu button, Back and a search field that fills the rest, and the frame SHALL keep clear of the notch and the home indicator. Choosing a page, a tap outside the drawer or Esc SHALL close the drawer. The desktop app SHALL keep its panel at every window size and zoom.

#### Scenario: Open a page from the drawer
- **WHEN** the browser version is open on a 390px-wide phone and the person taps the menu button, then Rename
- **THEN** the drawer slides in with the same destinations, counts and drives as the sidebar, and choosing Rename opens the Rename page and closes the drawer

#### Scenario: Wide window
- **WHEN** the browser version is 1280px wide
- **THEN** the sidebar panel shows at the left as on the desktop and there is no menu button

## MODIFIED Requirements

### Requirement: Legible at common screen sizes
Every page SHALL show its content without clipped labels or horizontal page scrolling at 1366×768, 1440×900, 1920×1080 and 3840×2160, at 100%, 125% and 150% display scaling, and in the browser version also at 390×844 (a phone) and 820×1180 (a tablet). Grids and tables SHALL use the full width of the content column, and reading text SHALL stay narrow enough to read.

#### Scenario: Small laptop screen
- **WHEN** the app runs at 1366×768 with 125% scaling
- **THEN** the Movie page header actions fit and no page scrolls horizontally

#### Scenario: Phone
- **WHEN** the browser version shows the Library, a Movie page or the Rename preview at 390×844
- **THEN** no page scrolls horizontally and every action is reachable
