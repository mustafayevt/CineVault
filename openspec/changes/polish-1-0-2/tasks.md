## 1. Table columns

- [x] 1.1 Smooth drag-reorder and resize in the Library table: pointer-driven reorder with lifted header, sliding neighbours, drop marker, Esc to cancel, Move left/right in the header menu, Title pinned; resize with an 8px edge, live width without row re-renders, auto-fit on double-click, widths and order persisted. Verified by the table and library table tests and `bun run perf:grid` not regressing.

## 2. Spotlight search

- [x] 2.1 Instant grouped results, recent searches, keyboard and "See all results" under the title-bar search. Verified by a component test and by trying it in the app.

## 3. Stage column

- [ ] 3.1 Labelled Stage header, New/Scraped/Renamed tags without a pointer cursor, problem colour and tooltip, app tooltip on the grid tick. Verified by the library table tests and a screenshot.

## 4. Title page header actions

- [ ] 4.1 Move Delete into the last place of the ⋯ menu (red, after a divider) and add "Change artwork" to the header buttons. Verified by the title page tests.

## 5. Date added

- [ ] 5.1 Take date added from the main file's creation time on insert and repair rows that share the first-scan timestamp once, in a transaction. Verified by cv-db and cv-scan tests.

## 6. Poster size

- [ ] 6.1 Persisted poster size control in the grid view. Verified by a library view test and the grid geometry tests.

## 7. Big artwork preview

- [ ] 7.1 Expand button, double-click and Space open a large preview; click or Esc closes it. Verified by an artwork picker test.

## 8. Episode expand animation

- [ ] 8.1 Height and fade animation of about 180ms with a rotating chevron, instant under reduced motion. Verified by a TV show page test and by trying it in the app.

## 9. Optional provider keys

- [ ] 9.1 Fanart.tv and TheTVDB keys in Settings › Providers and onboarding step 3, and the "needs a key" link in search opening them. Verified by cv-providers and settings tests.

## 10. Release notes

- [ ] 10.1 Show the offered version's release notes in the update prompt and Settings › Updates. Verified by an updates test.

## 11. Aspect ratio

- [ ] 11.1 Check and fix aspect ratio detection so ratios wider than the frame are reported. Verified by cv-media tests.
