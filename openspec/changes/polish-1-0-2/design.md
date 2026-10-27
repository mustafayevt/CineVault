## Context

The items are small and mostly independent; they share one change so they ship as 1.0.2. Only the date-added repair writes to the library database, and no item touches files in media folders.

## Decisions

- **Table columns:** reorder and resize are pointer-driven with window listeners and pointer capture; during a drag the moving header and the column's width are applied through transforms and a CSS variable on the table element, so the virtualised rows are not re-rendered on each pointer move. The final order or width is committed once on release and persisted as before (webview storage under the table's key). Title is a fixed column.
- **Date added repair:** a one-time, versioned migration that sets `date_added` from the main file's creation time only for rows whose value equals the library's first-scan timestamp; rows are never deleted, and a failed read of a file's time leaves the row unchanged.
- **Release notes:** shown as plain text from the signed manifest's notes field; never rendered as HTML.
- **Optional provider keys:** stored in the credential store like the others; the artwork sources they enable stay off until a key exists.

## Risks

- A migration that rewrites `date_added` must be idempotent and safe when interrupted: it runs in one transaction and records its version only when it commits. It changes only database rows; undo is not needed because the old value is the first-scan time, which can be recomputed.
