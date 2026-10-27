import { ChevronDown } from "lucide-react";

import { Button } from "../../components/Button";
import { Menu, MenuRadioGroup, MenuRadioItem } from "../../components/Menu";
import { t } from "../../i18n/catalog";
import type { TitleHit } from "./torrentBackend";
import { titleName } from "./torrentModel";

interface TitleLineProps {
  /** "Dune: Part Two (2024)". */
  name: string;
  kind: "movie" | "show";
  /** The other titles the search found, best first; empty when there is nothing to pick. */
  choices: readonly TitleHit[];
  currentId: string;
  onPick: (hit: TitleHit) => void;
}

const MAX_CHOICES = 8;

/**
 * "Releases for Dune: Part Two (2024) · Movie" above the results, with a small menu to pick another
 * match when the search found more than one title (spec `torrent-search` "One search field with
 * results as you type"). The line is left out while no title is known.
 */
export function TitleLine({ name, kind, choices, currentId, onPick }: TitleLineProps) {
  const kindText = t(kind === "show" ? "torrents.title.show" : "torrents.title.movie");
  // No title yet: no line at all, so the field and the chips sit 10px apart.
  if (name === "") return null;
  return (
    <div className="flex h-6 items-center gap-1.5 text-(length:--fs-12) text-text-muted">
      <>
        <span>{t("torrentsPage.match.for")}</span>
        {choices.length > 1 ? (
          <Menu
            align="start"
            aria-label={t("torrentsPage.match.pick")}
            trigger={
              <Button variant="ghost" size="sm" trailingIcon={ChevronDown} className="text-text">
                {name}
              </Button>
            }
          >
            <MenuRadioGroup
              value={currentId}
              onValueChange={(id) => {
                const hit = choices.find((choice) => choice.id === id);
                if (hit !== undefined) onPick(hit);
              }}
            >
              {choices.slice(0, MAX_CHOICES).map((choice) => (
                <MenuRadioItem key={choice.id} value={choice.id}>
                  {titleName(choice)} ·{" "}
                  {t(choice.kind === "show" ? "torrents.title.show" : "torrents.title.movie")}
                </MenuRadioItem>
              ))}
            </MenuRadioGroup>
          </Menu>
        ) : (
          <span className="font-(weight:--fw-600) text-text">{name}</span>
        )}
        <span>· {kindText}</span>
      </>
    </div>
  );
}
