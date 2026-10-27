import { SectionLabel } from "../../components/SectionLabel";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";

/**
 * The title's tags (keywords from the provider and the NFO), as quiet chips under the facts.
 * Nothing renders when the title has none.
 */
export function MovieTags({ tags }: { tags: readonly string[] }) {
  const shown = tags.filter((tag) => tag.trim() !== "");
  if (shown.length === 0) return null;
  return (
    <section aria-label={t("movie.tags.title")} className="flex flex-col gap-2">
      <SectionLabel>{t("movie.tags.title")}</SectionLabel>
      <ul className="m-0 flex list-none flex-wrap gap-1.5 p-0" data-testid="movie-tags">
        {shown.map((tag) => (
          <li key={tag}>
            <Tag className="cv-selectable">{tag}</Tag>
          </li>
        ))}
      </ul>
    </section>
  );
}
