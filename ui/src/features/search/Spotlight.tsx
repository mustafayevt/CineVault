import { useNavigate } from "@tanstack/react-router";
import { Clock, User } from "lucide-react";
import { useState } from "react";
import type { ReactNode } from "react";

import { SearchField } from "../../components/SearchField";
import { StageBadge } from "../../components/StageBadge";
import { t } from "../../i18n/catalog";
import { usePreferOriginalTitle } from "../library/displayTitle";
import type { LibraryCard } from "../library/libraryBackend";
import { cardTitle, problemNote, stageBadgeOf, yearText } from "../library/titleFacts";
import { isSimilarOnly, matchScore } from "./highlight";
import {
  clearOpenedTitles,
  clearRecentSearches,
  pushOpenedTitle,
  pushRecentSearch,
  readOpenedTitles,
  readRecentSearches,
} from "./recents";
import { SuggestPanel } from "./SuggestPanel";
import { useCombobox } from "./useCombobox";
import type { SuggestGroup, SuggestItem } from "./SuggestPanel";
import { useSpotlightResults } from "./useSpotlightResults";

/** Rows of one group. */
const PER_GROUP = 5;
/** A title must fit at least this well (it contains the text) to be the top hit. */
const TOP_HIT_SCORE = 40;
const SEE_ALL = "see-all";

interface SpotlightProps {
  /** The text of the field; the title bar keeps it so the address can change it. */
  text: string;
  setText: (text: string) => void;
  /** The shortcut hint at the field's end ("Ctrl F"). */
  shortcut: string;
  /** The attribute that makes the field Ctrl+F's target. */
  fieldAttributes?: Record<string, string>;
  /** Opens the Library with this search; `type` also switches its library type. */
  openLibrary: (query: string, type?: "collections") => void;
  /** The clear button: empties the field and, when the address held a search, the address too. */
  onClear: () => void;
}

/** The stage of a title, as a glyph and a word: "Scraped", "Not matched". */
function StageTag({ card }: { card: LibraryCard }) {
  const stage = stageBadgeOf(card);
  const note = problemNote(card);
  const word =
    note ??
    (stage === "new"
      ? t("stage.new")
      : stage === "renamed"
        ? t("stage.renamed")
        : t("stage.scraped"));
  return (
    <span className="flex shrink-0 items-center gap-1 text-(length:--fs-12) text-text-muted">
      <StageBadge stage={stage} label={word} />
      <span className={stage === "attention" ? "text-warn" : undefined}>{word}</span>
    </span>
  );
}

/**
 * The title-bar search with its suggestion panel: as the text changes, the best title, the movies,
 * shows, collections and people that match drop down under the field; with the field empty it
 * lists the recent searches and the titles opened from it. Enter on a highlighted row opens it,
 * Enter with none highlighted searches the Library as before.
 */
export function SpotlightSearch({
  text,
  setText,
  shortcut,
  fieldAttributes,
  openLibrary,
  onClear,
}: SpotlightProps) {
  const navigate = useNavigate();
  const preferOriginal = usePreferOriginalTitle();
  const [anchor, setAnchor] = useState<HTMLFormElement | null>(null);
  const [recent, setRecent] = useState(() => ({
    searches: readRecentSearches(),
    opened: readOpenedTitles(),
  }));
  const { results, busy, structured } = useSpotlightResults(text);
  const typed = text.trim();
  const term = results?.term ?? "";

  const submit = (query: string, type?: "collections") => {
    pushRecentSearch(query);
    openLibrary(query, type);
  };

  // The field gives up focus once something was opened from the list.
  const blurField = () => (document.activeElement as HTMLElement | null)?.blur();

  const model = (() => {
    const actions = new Map<string, () => void>();
    const groups: SuggestGroup[] = [];
    let footer: { id: string; label: string } | undefined;
    let empty: ReactNode;
    const kindWord = (kind: "movie" | "show" | "collection") => t(`shell.search.kind.${kind}`);

    const titleItem = (card: LibraryCard, kind: "movie" | "show"): SuggestItem => {
      const label = cardTitle(card, preferOriginal);
      const id = `${kind}:${card.id}`;
      actions.set(id, () => {
        pushOpenedTitle({ kind, id: card.id, title: label, year: card.year, poster: card.poster });
        setText("");
        blurField();
        if (kind === "movie")
          void navigate({ to: "/movies/$movieId", params: { movieId: card.id } });
        else void navigate({ to: "/tv/$showId", params: { showId: card.id } });
      });
      return {
        id,
        label,
        poster: card.poster,
        meta: [kindWord(kind), card.year > 0 ? yearText(card.year) : ""]
          .filter(Boolean)
          .join(" · "),
        badge: <StageTag card={card} />,
        similar: isSimilarOnly(label, term),
        note: t("shell.search.similar"),
      };
    };

    if (typed === "") {
      if (recent.searches.length > 0) {
        groups.push({
          id: "recent",
          label: t("shell.search.recent"),
          action: {
            label: t("shell.search.clearList"),
            onClick: () => {
              clearRecentSearches();
              setRecent((now) => ({ ...now, searches: [] }));
            },
          },
          items: recent.searches.map((search) => {
            const id = `recent:${search}`;
            actions.set(id, () => {
              setText(search);
              submit(search);
            });
            return { id, label: search, icon: Clock };
          }),
        });
      }
      if (recent.opened.length > 0) {
        groups.push({
          id: "opened",
          label: t("shell.search.opened"),
          action: {
            label: t("shell.search.clearList"),
            onClick: () => {
              clearOpenedTitles();
              setRecent((now) => ({ ...now, opened: [] }));
            },
          },
          items: recent.opened.map((title) => {
            const id = `opened:${title.kind}:${title.id}`;
            actions.set(id, () => {
              pushOpenedTitle(title);
              setText("");
              blurField();
              if (title.kind === "movie") {
                void navigate({ to: "/movies/$movieId", params: { movieId: title.id } });
              } else {
                void navigate({ to: "/tv/$showId", params: { showId: title.id } });
              }
            });
            return {
              id,
              label: title.title,
              poster: title.poster,
              meta: [kindWord(title.kind), title.year > 0 ? yearText(title.year) : ""]
                .filter(Boolean)
                .join(" · "),
            };
          }),
        });
      }
      return { groups, footer, empty, actions };
    }

    if (structured) {
      // A filter in the text is the Library page's business; the panel only offers to go there.
      footer = { id: SEE_ALL, label: t("shell.search.seeAllFor", { query: typed }) };
      actions.set(SEE_ALL, () => submit(typed));
      return { groups, footer, empty, actions };
    }

    if (results === null) return { groups, footer, empty, actions };

    // The best fit among movies and shows leads, and leaves its own group.
    const scored = [
      ...results.movies.map((card) => ({ card, kind: "movie" as const })),
      ...results.shows.map((card) => ({ card, kind: "show" as const })),
    ].map((entry) => ({
      ...entry,
      score: matchScore(cardTitle(entry.card, preferOriginal), term),
    }));
    let top: (typeof scored)[number] | null = null;
    for (const entry of scored) {
      if (entry.score >= TOP_HIT_SCORE && (top === null || entry.score > top.score)) top = entry;
    }
    const rest = (kind: "movie" | "show", cards: readonly LibraryCard[]) =>
      cards.filter((card) => !(top?.kind === kind && top.card.id === card.id)).slice(0, PER_GROUP);

    if (top !== null) {
      groups.push({
        id: "top",
        label: t("shell.search.top"),
        items: [titleItem(top.card, top.kind)],
      });
    }
    groups.push({
      id: "movies",
      label: t("shell.search.movies"),
      items: rest("movie", results.movies).map((card) => titleItem(card, "movie")),
    });
    groups.push({
      id: "shows",
      label: t("shell.search.shows"),
      items: rest("show", results.shows).map((card) => titleItem(card, "show")),
    });
    groups.push({
      id: "collections",
      label: t("shell.search.collections"),
      items: results.collections.slice(0, PER_GROUP).map((card) => {
        const id = `collection:${card.id}`;
        actions.set(id, () => {
          setText("");
          blurField();
          submit(card.title, "collections");
        });
        return { id, label: card.title, poster: card.poster, meta: kindWord("collection") };
      }),
    });
    groups.push({
      id: "people",
      label: t("shell.search.people"),
      items: results.people.slice(0, PER_GROUP).map((person) => {
        const id = `person:${person.role}:${person.name}`;
        actions.set(id, () =>
          submit(`${person.role === "director" ? "director" : "actor"}:"${person.name}"`),
        );
        return {
          id,
          label: person.name,
          icon: User,
          meta: t(
            person.role === "director"
              ? "shell.search.person.director"
              : "shell.search.person.actor",
            { count: person.titles },
          ),
        };
      }),
    });

    if (results.total > 0) {
      footer = { id: SEE_ALL, label: t("shell.search.seeAll", { count: results.total }) };
      actions.set(SEE_ALL, () => submit(typed));
    } else if (results.people.length === 0 && !busy) {
      empty = t("shell.search.none", { query: results.term });
    }
    return { groups, footer, empty, actions };
  })();

  const ids = [
    ...model.groups.flatMap((group) => group.items.map((item) => item.id)),
    ...(model.footer === undefined ? [] : [model.footer.id]),
  ];
  const combobox = useCombobox({
    anchor,
    ids,
    showing: ids.length > 0 || model.empty !== undefined,
    onSelect: (id) => model.actions.get(id)?.(),
  });
  const { fieldProps } = combobox;

  return (
    <form
      ref={setAnchor}
      role="search"
      className="flex h-[38px] items-center"
      onSubmit={(event) => {
        event.preventDefault();
        combobox.setOpen(false);
        submit(text);
      }}
    >
      <SearchField
        {...fieldAttributes}
        {...fieldProps}
        aria-label={t("shell.search.label")}
        placeholder={t("shell.search.placeholder")}
        shortcut={shortcut}
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          combobox.typed();
        }}
        onFocus={() => {
          setRecent({ searches: readRecentSearches(), opened: readOpenedTitles() });
          fieldProps.onFocus();
        }}
        onClear={() => {
          onClear();
          combobox.setOpen(true);
        }}
        clearLabel={t("shell.search.clear")}
        className="w-[440px] min-w-[200px] max-w-[40vw]"
        boxClassName="h-[30px]!"
      />
      <SuggestPanel
        combobox={combobox}
        label={t("shell.search.suggestions")}
        groups={model.groups}
        term={term}
        busy={busy}
        {...(model.footer === undefined ? {} : { footer: model.footer })}
        {...(model.empty === undefined ? {} : { empty: model.empty })}
      />
    </form>
  );
}
