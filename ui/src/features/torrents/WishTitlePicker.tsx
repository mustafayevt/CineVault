import { Check, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Field } from "../../components/Field";
import { Icon } from "../../components/Icon";
import { t } from "../../i18n/catalog";
import { SuggestPanel } from "../search/SuggestPanel";
import type { SuggestItem } from "../search/SuggestPanel";
import { useCombobox } from "../search/useCombobox";
import { useEscapeLayer } from "../shell/escStack";
import type { TitleHit } from "./torrentBackend";
import { titleName } from "./torrentModel";
import { useTitleMatch } from "./useTitleMatch";

interface WishTitlePickerProps {
  onPick: (hit: TitleHit) => void;
  onCancel: () => void;
}

/**
 * The first step of "Add" on the Wishlist: one field that suggests movies and shows as the user
 * types (the title search of the Torrents page, the owned ones marked), and Cancel. Picking a
 * title opens the editor for it.
 */
export function WishTitlePicker({ onPick, onCancel }: WishTitlePickerProps) {
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => field.current?.focus(), []);
  const [text, setText] = useState("");
  const match = useTitleMatch(text, true);
  // The suggestions keep their last rows while the next search is out, so the list never blinks.
  const [kept, setKept] = useState<readonly TitleHit[]>([]);
  if (match.short && kept.length > 0) setKept([]);
  else if (match.hits.length > 0 && match.hits !== kept) setKept(match.hits);
  const suggested: readonly TitleHit[] = match.short
    ? []
    : match.hits.length > 0 || match.settled
      ? match.hits
      : kept;
  const [anchor, setAnchor] = useState<HTMLDivElement | null>(null);
  const combobox = useCombobox({
    anchor,
    ids: suggested.map((hit) => hit.id),
    showing: !match.short && (suggested.length > 0 || match.settled),
    onSelect: (id) => {
      const hit = suggested.find((entry) => entry.id === id);
      if (hit !== undefined) onPick(hit);
    },
  });
  useEscapeLayer("picker", true, onCancel);
  const items: SuggestItem[] = suggested.map((hit) => ({
    id: hit.id,
    label: titleName(hit),
    poster: hit.posterKey === null ? null : { key: hit.posterKey, thumbhash: null, ambient: null },
    meta: t(hit.kind === "show" ? "torrents.title.show" : "torrents.title.movie"),
    badge:
      hit.inLibrary === null ? undefined : (
        <span className="flex shrink-0 items-center gap-1 text-(length:--fs-12) text-ok">
          <Icon icon={Check} size={14} />
          {t("torrentsPage.suggest.owned")}
        </span>
      ),
  }));

  return (
    <div className="flex min-w-0 items-center gap-2">
      <div ref={setAnchor} className="min-w-0 flex-1">
        <Field
          type="text"
          autoComplete="off"
          spellCheck={false}
          ref={field}
          icon={Search}
          aria-label={t("wishlist.pick.field")}
          placeholder={t("wishlist.pick.placeholder")}
          value={text}
          {...combobox.fieldProps}
          onChange={(event) => {
            setText(event.target.value);
            combobox.typed();
          }}
          onKeyDown={(event) => {
            combobox.fieldProps.onKeyDown(event);
            if (event.defaultPrevented) return;
            if (event.key === "Enter" && suggested[0] !== undefined) {
              // Enter with nothing highlighted takes the best suggestion.
              event.preventDefault();
              combobox.select(suggested[0].id);
            }
          }}
        />
        <SuggestPanel
          combobox={combobox}
          label={t("torrentsPage.suggest.label")}
          groups={[{ id: "titles", label: "", items }]}
          term={text.trim()}
          maxWidth={720}
          {...(match.settled && suggested.length === 0
            ? {
                empty: match.failed
                  ? t("torrents.picker.failed")
                  : t("torrents.picker.nothing", { query: text.trim() }),
              }
            : {})}
        />
      </div>
      <Button type="button" variant="ghost" onClick={onCancel}>
        {t("wishlist.edit.cancel")}
      </Button>
    </div>
  );
}
