import { Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Award, GitCompareArrows, Trash2 } from "lucide-react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { Tooltip } from "../../components/Tooltip";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { requestDelete, useDeleteStore } from "../delete/deleteActions";
import { copyName, keepThisOne } from "./copiesActions";
import type { Comparison } from "./copiesBackend";
import { tabLabel, tabTexts } from "./copyGroup";

/**
 * One tab per copy on the title line of the Technical card, in place of its title (spec
 * `movie-copies` "Copy tabs on the Movie page"), so the first screen keeps its rows. A tab is a
 * link to that copy's Movie page that replaces the history entry, so Back still leaves the
 * movie. The tooltip and the accessible name give the whole description, source included.
 */
export function CopyTabs({ group, movieId }: { group: Comparison; movieId: string }) {
  const format = useFormat();
  const texts = tabTexts(group.copies, format.size);
  return (
    <nav
      aria-label={t("copies.tabs.aria", { count: group.copies.length })}
      data-testid="copy-tabs"
      className="-my-0.5 mb-2.5 flex min-w-0 flex-1 flex-wrap gap-1"
    >
      {group.copies.map((copy, index) => {
        const current = copy.id === movieId;
        const label = tabLabel(copy, format.size);
        return (
          <Tooltip key={copy.id} content={label}>
            <Link
              to="/movies/$movieId"
              params={{ movieId: copy.id }}
              replace
              aria-current={current ? "page" : undefined}
              aria-label={copy.best ? t("copies.tabs.best", { label }) : label}
              data-best={copy.best ? "true" : undefined}
              className={cx(
                "cv-hoverable inline-flex h-6 max-w-full min-w-0 items-center gap-1 rounded-(--r-4) border border-solid px-2",
                "text-(length:--fs-12) whitespace-nowrap tabular-nums no-underline",
                current
                  ? "border-line-section bg-surface-active font-(weight:--fw-600) text-text"
                  : "border-transparent font-(weight:--fw-500) text-text-2 hover:bg-surface-raised hover:text-text",
              )}
            >
              {copy.best && (
                <Award
                  size={12}
                  strokeWidth={1.75}
                  aria-hidden="true"
                  className="shrink-0 text-gold"
                />
              )}
              <span className="truncate">{texts[index]}</span>
            </Link>
          </Tooltip>
        );
      })}
    </nav>
  );
}

/**
 * The Copies row at the end of the Technical card: Keep only this one, Delete this copy and
 * Compare (spec `movie-copies` "Copies row on the Movie page"). Both deletes go through the usual
 * Delete, so they follow Settings › Deleted files with its confirmation, toast and Undo.
 */
export function CopyActions({ group, movieId }: { group: Comparison; movieId: string }) {
  const navigate = useNavigate();
  const shown = group.copies.find((copy) => copy.id === movieId);
  // One delete at a time, as on the Compare page: from the click until the delete has run.
  const [checking, setChecking] = useState(false);
  const deleting = useDeleteStore(
    (state) =>
      state.confirm !== null ||
      state.request !== null ||
      group.copies.some((copy) => state.hidden.has(copy.id)),
  );
  const busy = checking || deleting;
  if (shown === undefined) return null;

  const keepOnly = async () => {
    if (busy) return;
    setChecking(true);
    await keepThisOne(group, movieId, {
      // The shown copy is the one kept: its page stays.
      done: () => undefined,
      rename: () => void navigate({ to: "/rename", search: { ids: movieId } as never }),
    }).finally(() => setChecking(false));
  };

  const deleteThis = () => {
    if (busy) return;
    const next = group.copies.find((copy) => copy.id !== movieId);
    requestDelete([{ id: movieId, title: copyName(shown) }], {
      onDone: () => {
        if (next === undefined) void navigate({ to: "/library", search: { type: "movies" } });
        else void navigate({ to: "/movies/$movieId", params: { movieId: next.id }, replace: true });
      },
    });
  };

  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5">
      {group.same_file ? null : (
        <Button size="sm" disabled={busy} onClick={() => void keepOnly()}>
          {t("copies.tabs.keepOnly")}
        </Button>
      )}
      <Button size="sm" icon={Trash2} disabled={busy} onClick={deleteThis}>
        {t("copies.tabs.delete")}
      </Button>
      <Link
        to="/movies/$movieId/copies"
        params={{ movieId }}
        className="cv-link ml-auto inline-flex items-center gap-1 text-(length:--fs-12) whitespace-nowrap"
      >
        <Icon icon={GitCompareArrows} size={12} />
        {t("copies.tabs.compare")}
      </Link>
    </div>
  );
}
