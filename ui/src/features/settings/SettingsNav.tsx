import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";

export interface IndexEntry {
  id: string;
  label: string;
}

interface SettingsNavProps {
  entries: readonly IndexEntry[];
  active: string;
  onSelect: (id: string) => void;
}

/**
 * The flat index at the left of the Settings page: one entry per section, the active one on the
 * active fill with a 2px gold indicator at its left edge. It follows the scroll of the page.
 */
export function SettingsNav({ entries, active, onSelect }: SettingsNavProps) {
  return (
    <nav
      aria-label={t("settingsPage.index")}
      className="flex w-[180px] shrink-0 flex-col gap-0.5 border-r border-solid border-line-section px-2.5 py-5"
    >
      {entries.map((entry) => {
        const current = entry.id === active;
        return (
          <button
            key={entry.id}
            type="button"
            aria-current={current ? "true" : undefined}
            onClick={() => {
              onSelect(entry.id);
            }}
            className={cx(
              "cv-hoverable flex h-(--h-nav) items-center rounded-(--r-6) border-0 px-2.5 text-left text-(length:--fs-13)",
              current
                ? "bg-surface-active font-(weight:--fw-600) text-text shadow-[inset_2px_0_0_var(--gold)]"
                : "bg-transparent font-(weight:--fw-500) text-text-2 hover:bg-surface-raised hover:text-text active:bg-surface-pressed",
            )}
          >
            {entry.label}
          </button>
        );
      })}
    </nav>
  );
}
