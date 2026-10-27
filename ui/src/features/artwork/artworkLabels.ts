import { t } from "../../i18n/catalog";
import { languageName } from "../detail/detailFormat";
import { ANY_LANGUAGE, NO_TEXT } from "./artworkModel";
import type { Popularity } from "./artworkModel";

/** The word for a language filter value or an image's language: "Any language", "No text", "French". */
export function artworkLanguageLabel(code: string, locale: string): string {
  if (code === ANY_LANGUAGE) return t("artwork.language.any");
  if (code === NO_TEXT) return t("artwork.language.none");
  return languageName(code, locale);
}

/** The words for what people think of an image: "50 votes, average 5.6" or "12 likes". */
export function popularityWords(popularity: Popularity, locale: string): string {
  if (popularity.kind === "likes") return t("artwork.pop.likes", { count: popularity.count });
  return t("artwork.pop.votes", {
    count: popularity.count,
    rating: popularity.rating.toLocaleString(locale, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    }),
  });
}
