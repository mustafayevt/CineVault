import { ArrowLeft } from "lucide-react";

import { Button } from "../../components/Button";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { useEscapeLayer } from "../shell/escStack";
import { usePickerClose } from "../scrape/usePickerClose";
import { NotesBody } from "../updates/NotesBody";
import { parseNotes } from "../updates/notesFormat";
import { useAppVersion } from "../updates/useAppVersion";
import { BUNDLED_CHANGELOG } from "./bundledChangelog";
import type { ChangelogSection } from "./changelogModel";

function SectionBlock({
  section,
  installed,
  first,
}: {
  section: ChangelogSection;
  installed: boolean;
  first: boolean;
}) {
  const format = useFormat();
  const id = `changelog-${section.version ?? "more"}`;
  return (
    <section
      aria-labelledby={id}
      data-testid="changelog-section"
      className={
        first
          ? "flex flex-col gap-3"
          : "flex flex-col gap-3 border-t border-solid border-line-section pt-7"
      }
    >
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h2 id={id} className="m-0 text-(length:--fs-20) leading-[1.2] font-(weight:--fw-700)">
          {section.version === null
            ? section.title
            : t("changelog.page.version", { version: section.version })}
        </h2>
        {section.unreleased ? <Tag variant="warn">{t("changelog.page.unreleased")}</Tag> : null}
        {installed ? <Tag variant="ok">{t("changelog.page.current")}</Tag> : null}
        {section.date === null ? null : (
          <time dateTime={section.date} className="text-(length:--fs-12) text-text-muted">
            {format.date(section.date, { timeZone: "UTC" })}
          </time>
        )}
      </div>
      <div className="flex flex-col gap-2.5 text-text-2">
        <NotesBody blocks={parseNotes(section.body)} links size="reading" />
      </div>
    </section>
  );
}

/**
 * The release notes page (`/release-notes`): every version of the bundled CHANGELOG.md, newest
 * first, in comfortable reading type. It opens from Settings › Updates and from the "What's new"
 * card, closes like a picker page (Esc or Back) and needs no network.
 */
export function ReleaseNotesPage() {
  const close = usePickerClose();
  useEscapeLayer("picker", true, close);
  const installed = useAppVersion();
  return (
    <section aria-label={t("changelog.page.title")} className="flex h-full min-h-0 flex-col">
      <div className="relative flex h-12 shrink-0 items-center gap-2.5 px-6">
        <span
          aria-hidden="true"
          className="absolute right-6 bottom-0 left-6 h-px bg-line-section"
        />
        <Button variant="ghost" icon={ArrowLeft} shortcut="Esc" onClick={close}>
          {t("changelog.page.back")}
        </Button>
        <h1 className="m-0 text-(length:--fs-16) font-(weight:--fw-700)">
          {t("changelog.page.title")}
        </h1>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex max-w-[720px] flex-col gap-7 px-6 pt-6 pb-12 text-(length:--fs-14) leading-[1.55]">
          <p className="m-0 text-text-muted">{t("changelog.page.intro")}</p>
          {BUNDLED_CHANGELOG.map((section, index) => (
            <SectionBlock
              key={section.title}
              section={section}
              installed={section.version !== null && section.version === installed}
              first={index === 0}
            />
          ))}
        </div>
      </div>
    </section>
  );
}
