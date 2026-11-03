import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { Check, Pencil, Plus, RefreshCw, Search, Send, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { QualityBadge } from "../../components/QualityBadge";
import { SectionLabel } from "../../components/SectionLabel";
import { Skeleton } from "../../components/Skeleton";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { CachedImage } from "../images/CachedImage";
import { tierOf } from "./releaseSort";
import { checkedText } from "./sourceHealth";
import { useTitleDetails } from "./titleQueries";
import { openRelease } from "./torrentActions";
import { currentTorrentBackend } from "./torrentBackend";
import type { WishCriteria, WishEntry, WishMatch, WishScope } from "./torrentBackend";
import { TorrentNotice } from "./TorrentNotice";
import { titleName } from "./torrentModel";
import { parseTitleRef, titleParam, torrentsLink } from "./torrentsSearch";
import type { TitleRef, WishlistSearch } from "./torrentsSearch";
import { TorrentsViewSwitch } from "./TorrentsViewSwitch";
import { useTorrentNotice } from "./useTorrentNotice";
import { WishEditor } from "./WishEditor";
import { WishTitlePicker } from "./WishTitlePicker";
import {
  criteriaText,
  defaultCriteria,
  defaultScope,
  lastChecked,
  noteText,
  orderEntries,
  scopeText,
} from "./wishlistModel";
import { WISHLIST_KEY, announce, refreshWishlist, useWishlist } from "./wishlistQueries";

/** How many matches an entry lists before "Show N more". */
const FIRST_MATCHES = 4;

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function failed(title: string, error: unknown) {
  notify({ severity: "error", title, detail: messageOf(error) });
}

/** The poster of an entry, 40px wide. */
function Poster({ title, posterKey }: { title: string; posterKey: string | null }) {
  return (
    <div className="relative aspect-[2/3] w-[40px] shrink-0 overflow-hidden rounded-(--r-6) bg-surface-active">
      {posterKey === null ? (
        <PlaceholderPoster title={title} className="size-full" />
      ) : (
        <CachedImage imageKey={posterKey} size="list" alt="" className="size-full" />
      )}
    </div>
  );
}

/** The title of an entry, linking to its page when the library holds it. */
function EntryTitle({ entry }: { entry: WishEntry }) {
  const name = titleName(entry);
  const link = entry.inLibrary;
  const className = "max-w-full truncate text-(length:--fs-14) font-(weight:--fw-600) text-text";
  if (link === null) return <span className={className}>{name}</span>;
  return entry.kind === "show" ? (
    <Link to="/tv/$showId" params={{ showId: link.itemId }} className={`${className} cv-link`}>
      {name}
    </Link>
  ) : (
    <Link
      to="/movies/$movieId"
      params={{ movieId: link.itemId }}
      className={`${className} cv-link`}
    >
      {name}
    </Link>
  );
}

interface MatchRowProps {
  match: WishMatch;
  fresh: boolean;
  onSend: () => void;
  onDismiss: () => void;
}

/** One release a check kept: quality, name, size, seeders, New or Sent, Send and Dismiss. */
function MatchRow({ match, fresh, onSend, onDismiss }: MatchRowProps) {
  const format = useFormat();
  const sent = match.state === "sent";
  const quality = [match.source, match.codec, match.audio].filter(Boolean).join(" · ");
  return (
    <li
      data-testid="wish-match"
      data-state={sent ? "sent" : fresh ? "new" : "seen"}
      className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 rounded-(--r-6) px-2 py-1 hover:bg-surface-hover"
    >
      <span className="flex w-[92px] shrink-0">
        <QualityBadge quality={tierOf(match)} hdr={match.hdr ?? false} />
      </span>
      <div
        className={cx(
          "flex min-w-0 flex-1 flex-col @max-[640px]/pane:basis-[calc(100%-104px)]",
          sent && "opacity-60",
        )}
      >
        <span className="truncate font-mono text-(length:--fs-12) text-text" title={match.name}>
          {match.name}
        </span>
        {quality === "" ? null : (
          <span className="truncate text-(length:--fs-12) text-text-muted">{quality}</span>
        )}
      </div>
      <span className="w-[72px] shrink-0 text-right text-(length:--fs-12) text-text-2 tabular-nums @max-[640px]/pane:w-auto">
        {match.size === null ? "" : format.size(match.size)}
      </span>
      <span className="w-[88px] shrink-0 text-right text-(length:--fs-12) text-text-2 tabular-nums @max-[640px]/pane:w-auto">
        {match.seeders === null ? "" : t("wishlist.match.seeders", { count: match.seeders })}
      </span>
      <span className="flex w-[48px] shrink-0 justify-end @max-[640px]/pane:w-auto">
        {fresh && !sent ? <Tag variant="ok">{t("wishlist.match.new")}</Tag> : null}
      </span>
      {sent ? <Tag>{t("wishlist.match.sent")}</Tag> : null}
      <div className="flex shrink-0 items-center gap-1 @max-[640px]/pane:ml-auto">
        <Button
          size="sm"
          icon={Send}
          variant={sent ? "ghost" : "default"}
          disabled={match.magnet === null && match.torrentUrl === null}
          onClick={onSend}
        >
          {t("wishlist.match.send")}
        </Button>
        <IconButton
          aria-label={t("wishlist.match.dismiss")}
          title={t("wishlist.match.dismiss")}
          icon={X}
          size="sm"
          onClick={onDismiss}
        />
      </div>
    </li>
  );
}

interface EntryCardProps {
  entry: WishEntry;
  now: number;
  /** Keys (`id:key`) of the matches that were new when the view opened or a check ran. */
  fresh: ReadonlySet<string>;
  editing: boolean;
  onEdit: (editing: boolean) => void;
  /** Just added or updated: scrolled into view and outlined for a moment. */
  highlight?: boolean;
}

/** One entry: poster, title, what it looks for and its criteria, its state, and its matches. */
function EntryCard({ entry, now, fresh, editing, onEdit, highlight = false }: EntryCardProps) {
  const card = useRef<HTMLElement>(null);
  useEffect(() => {
    if (highlight) card.current?.scrollIntoView?.({ block: "nearest" });
  }, [highlight]);
  const format = useFormat();
  const client = useQueryClient();
  const navigate = useNavigate();
  const [all, setAll] = useState(false);
  const [saving, setSaving] = useState(false);
  const backend = currentTorrentBackend();
  const done = entry.done !== null;
  const matches = done ? [] : entry.matches;
  const shown = all ? matches : matches.slice(0, FIRST_MATCHES);

  let status: string;
  if (done) status = t("wishlist.done.line");
  else if (matches.length > 0) status = t("wishlist.matches", { count: matches.length });
  else {
    const note = noteText(entry, format);
    status =
      note ??
      (entry.checked === null
        ? t("wishlist.notChecked")
        : `${t("wishlist.noMatch")} · ${checkedText(entry.checked, now)}`);
  }

  const setState = async (found: WishMatch, state: "sent" | "dismissed" | "seen") => {
    await backend.wishlistMatchState(entry.id, found.key, state);
    await refreshWishlist(client);
  };
  const send = async (found: WishMatch) => {
    if (!(await openRelease(found))) return;
    try {
      await setState(found, "sent");
    } catch (error) {
      failed(t("wishlist.error.save"), error);
    }
  };
  const dismiss = async (found: WishMatch) => {
    try {
      await setState(found, "dismissed");
      showToast({
        message: t("wishlist.toast.dismissed"),
        glyph: X,
        undo: () => setState(found, found.state === "sent" ? "sent" : "seen"),
      });
    } catch (error) {
      failed(t("wishlist.error.save"), error);
    }
  };
  const remove = async () => {
    try {
      await backend.wishlistRemove(entry.id);
      await refreshWishlist(client);
      showToast({
        message: t("wishlist.toast.removed", { title: titleName(entry) }),
        glyph: Trash2,
        undo: async () => {
          await backend.wishlistAdd({
            kind: entry.kind,
            tmdbId: entry.tmdbId,
            scope: entry.scope,
            criteria: entry.criteria,
          });
          await refreshWishlist(client);
        },
      });
    } catch (error) {
      failed(t("wishlist.error.save"), error);
    }
  };
  const save = async (scope: WishScope, criteria: WishCriteria) => {
    setSaving(true);
    try {
      await backend.wishlistUpdate(entry.id, scope, criteria);
      await refreshWishlist(client);
      onEdit(false);
    } catch (error) {
      failed(t("wishlist.error.save"), error);
    } finally {
      setSaving(false);
    }
  };
  const ref: TitleRef = { kind: entry.kind === "show" ? "show" : "movie", tmdbId: entry.tmdbId };
  const searchNow = () => {
    const scope = entry.scope;
    const season = scope.kind === "next" ? entry.target?.season : (scope.season ?? undefined);
    const episode = scope.kind === "next" ? entry.target?.episode : (scope.episode ?? undefined);
    void navigate({
      href: torrentsLink({
        title: `${ref.kind}:${ref.tmdbId}`,
        ...(season === undefined ? {} : { season }),
        ...(episode === undefined ? {} : { episode }),
      }),
    });
  };

  return (
    <article
      ref={card}
      aria-label={titleName(entry)}
      data-testid="wish-entry"
      data-done={done ? "true" : "false"}
      data-highlight={highlight ? "true" : undefined}
      className={cx(
        "flex min-w-0 flex-col gap-2 rounded-(--r-10) border border-solid bg-surface-card p-3",
        highlight ? "border-gold" : "border-line-edge",
      )}
    >
      <div className="flex min-w-0 items-start gap-3">
        <Poster title={entry.title} posterKey={entry.posterKey} />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
            <EntryTitle entry={entry} />
            <span className="shrink-0 text-(length:--fs-12) text-text-muted">
              {scopeText(entry)}
            </span>
          </div>
          <p className="m-0 truncate text-(length:--fs-13) text-text-2 @max-[640px]/pane:whitespace-normal">
            {criteriaText(entry.criteria, format)}
          </p>
          <p
            data-testid="wish-status"
            className={`m-0 flex items-center gap-1.5 text-(length:--fs-12) ${done ? "text-ok" : "text-text-muted"}`}
          >
            {done ? <Icon icon={Check} size={14} /> : null}
            {status}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {done ? null : (
            <IconButton
              aria-label={t("wishlist.searchNow")}
              title={t("wishlist.searchNow")}
              icon={Search}
              size="sm"
              onClick={searchNow}
            />
          )}
          <IconButton
            aria-label={t("wishlist.edit")}
            title={t("wishlist.edit")}
            icon={Pencil}
            size="sm"
            onClick={() => onEdit(!editing)}
          />
          <IconButton
            aria-label={t("wishlist.remove")}
            title={t("wishlist.remove")}
            icon={Trash2}
            size="sm"
            onClick={() => void remove()}
          />
        </div>
      </div>
      {editing ? (
        <div className="border-t border-solid border-line-section pt-3">
          <WishEditor
            kind={ref.kind}
            tmdbId={entry.tmdbId}
            scope={entry.scope}
            criteria={entry.criteria}
            mode="edit"
            saving={saving}
            onSave={(scope, criteria) => void save(scope, criteria)}
            onCancel={() => onEdit(false)}
          />
        </div>
      ) : null}
      {shown.length === 0 ? null : (
        <ul
          aria-label={t("wishlist.matchesLabel", { title: titleName(entry) })}
          className="m-0 flex list-none flex-col gap-0.5 border-t border-solid border-line-section p-0 pt-2"
        >
          {shown.map((found) => (
            <MatchRow
              key={found.key}
              match={found}
              fresh={found.state === "new" || fresh.has(`${entry.id}:${found.key}`)}
              onSend={() => void send(found)}
              onDismiss={() => void dismiss(found)}
            />
          ))}
        </ul>
      )}
      {matches.length > FIRST_MATCHES ? (
        <div>
          <Button size="sm" variant="ghost" onClick={() => setAll(!all)}>
            {all
              ? t("wishlist.fewer")
              : t("wishlist.more", { count: matches.length - FIRST_MATCHES })}
          </Button>
        </div>
      ) : null}
    </article>
  );
}

/** The editor for a title to add, at the top of the list (`?add=movie:123`). */
interface AddPanelProps {
  add: TitleRef;
  search: WishlistSearch;
  /** The entry that was added or updated, to show it. */
  onAdded: (id: number) => void;
}

function AddPanel({ add, search, onAdded }: AddPanelProps) {
  const details = useTitleDetails(add);
  const client = useQueryClient();
  const navigate = useNavigate();
  const [saving, setSaving] = useState(false);
  const known = details.data?.tmdbId === add.tmdbId ? details.data : null;
  const close = () => void navigate({ to: "/torrents/wishlist", search: {}, replace: true });
  const save = async (scope: WishScope, criteria: WishCriteria) => {
    setSaving(true);
    const backend = currentTorrentBackend();
    try {
      const before = client.getQueryData<WishEntry[]>(WISHLIST_KEY) ?? [];
      const id = await backend.wishlistAdd({
        kind: add.kind,
        tmdbId: add.tmdbId,
        scope,
        criteria,
      });
      await refreshWishlist(client);
      const title = known === null ? "" : titleName(known);
      showToast({
        message: before.some((entry) => entry.id === id)
          ? t("wishlist.toast.updated", { title })
          : t("wishlist.toast.added", { title }),
      });
      onAdded(id);
      close();
      // The new entry is due at once: look for it now, as the app would on its next check.
      const report = await backend.wishlistCheck(true);
      await refreshWishlist(client);
      announce(report.newMatches, () => void navigate({ to: "/torrents/wishlist" }));
    } catch (error) {
      failed(t("wishlist.error.add"), error);
    } finally {
      setSaving(false);
    }
  };

  return (
    <section
      aria-label={t("wishlist.edit.addRegion")}
      data-testid="wish-add"
      className="flex min-w-0 flex-col gap-3 rounded-(--r-10) border border-solid border-line-edge bg-surface-card p-3"
    >
      {known === null ? (
        details.isError ? (
          <p role="alert" className="m-0 text-text">
            {messageOf(details.error)}
          </p>
        ) : (
          <Skeleton shape="row" />
        )
      ) : (
        <>
          <div className="flex min-w-0 items-center gap-3">
            <Poster title={known.title} posterKey={known.posterKey} />
            <div className="flex min-w-0 flex-col">
              <span className="text-(length:--fs-12) font-(weight:--fw-600) text-gold">
                {t("wishlist.add")}
              </span>
              <span className="truncate text-(length:--fs-14) font-(weight:--fw-600) text-text">
                {titleName(known)}
              </span>
            </div>
          </div>
          <WishEditor
            key={`${add.kind}:${add.tmdbId}`}
            kind={add.kind}
            tmdbId={add.tmdbId}
            scope={defaultScope(add.kind, search.season, search.episode)}
            criteria={defaultCriteria(known.libraryCopies[0] ?? null)}
            mode="add"
            saving={saving}
            onSave={(scope, criteria) => void save(scope, criteria)}
            onCancel={close}
          />
        </>
      )}
    </section>
  );
}

/** The matches that are new now, as `id:key`. */
function newKeys(entries: readonly WishEntry[]): string[] {
  return entries.flatMap((entry) =>
    entry.matches.filter((m) => m.state === "new").map((m) => `${entry.id}:${m.key}`),
  );
}

function WishlistContent() {
  const search = useSearch({ strict: false }) as WishlistSearch;
  const add = useMemo(() => parseTitleRef(search.add), [search.add]);
  const list = useWishlist();
  const client = useQueryClient();
  const navigate = useNavigate();
  const [editing, setEditing] = useState<number | null>(null);
  // "Add" was pressed: the title field shows until a title is picked.
  const [picking, setPicking] = useState(false);
  // The entry just added, outlined for a moment.
  const [added, setAdded] = useState<number | null>(null);
  useEffect(() => {
    if (added === null) return;
    const timer = window.setTimeout(() => setAdded(null), 2500);
    return () => window.clearTimeout(timer);
  }, [added]);
  const [checking, setChecking] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  // The matches that were new when they were first shown keep "New" while the view is open.
  const [fresh, setFresh] = useState<ReadonlySet<string>>(new Set());
  const marking = useRef(false);
  const entries = list.data;

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  // Looking at the Wishlist marks its new matches seen; the count in the sidebar goes.
  useEffect(() => {
    if (entries === undefined || marking.current) return;
    const keys = newKeys(entries);
    if (keys.length === 0) return;
    marking.current = true;
    void currentTorrentBackend()
      .wishlistSeen()
      .catch(() => 0)
      .then(() => {
        setFresh((current) => new Set([...current, ...keys]));
        return client.invalidateQueries({ queryKey: [...WISHLIST_KEY, "new"] });
      })
      .finally(() => {
        marking.current = false;
      });
  }, [entries, client]);

  const checkNow = async () => {
    setChecking(true);
    try {
      const report = await currentTorrentBackend().wishlistCheck(false);
      await refreshWishlist(client);
      setNow(Date.now());
      if (report.busy) showToast({ message: t("wishlist.toast.busy") });
      else if (report.newMatches === 0) showToast({ message: t("wishlist.toast.nothing") });
      else announce(report.newMatches, () => void navigate({ to: "/torrents/wishlist" }));
    } catch (error) {
      failed(t("wishlist.error.check"), error);
    } finally {
      setChecking(false);
    }
  };

  const ordered = entries === undefined ? null : orderEntries(entries);
  const latest = entries === undefined ? null : lastChecked(entries);
  const checkable = ordered !== null && ordered.active.length > 0;

  let body: React.ReactNode;
  if (list.isError) {
    body = (
      <p role="alert" className="m-0 py-10 text-center text-text">
        {messageOf(list.error)}
      </p>
    );
  } else if (ordered === null) {
    body = (
      <div aria-busy="true" className="flex flex-col gap-2">
        {[0, 1, 2].map((n) => (
          <Skeleton key={n} shape="row" />
        ))}
      </div>
    );
  } else if (ordered.active.length === 0 && ordered.done.length === 0) {
    body =
      add === null && !picking ? (
        <p role="status" className="m-0 py-16 text-center text-text-2">
          {t("wishlist.empty")}
        </p>
      ) : null;
  } else {
    const card = (entry: WishEntry) => (
      <EntryCard
        key={entry.id}
        entry={entry}
        now={now}
        fresh={fresh}
        editing={editing === entry.id}
        onEdit={(on) => setEditing(on ? entry.id : null)}
        highlight={added === entry.id}
      />
    );
    body = (
      <>
        {ordered.active.map(card)}
        {ordered.done.length === 0 ? null : (
          <>
            <SectionLabel as="h2" className="mt-3">
              {t("wishlist.done")}
            </SectionLabel>
            {ordered.done.map(card)}
          </>
        )}
      </>
    );
  }

  return (
    <section
      aria-label={t("wishlist.region")}
      data-testid="wishlist-page"
      className="relative flex h-full min-h-0 flex-col"
    >
      <h1 className="sr-only">{t("wishlist.region")}</h1>
      <div className="relative flex shrink-0 flex-wrap items-center gap-3 px-6 pt-4 pb-3 @max-[640px]/pane:px-4">
        <span
          aria-hidden="true"
          className="absolute right-0 bottom-0 left-0 h-px bg-line-section"
        />
        <TorrentsViewSwitch value="wishlist" />
        <span className="grow" />
        {latest === null ? null : (
          <span
            aria-live="polite"
            className="text-(length:--fs-12) whitespace-nowrap text-text-muted"
          >
            {checking ? t("wishlist.checking") : checkedText(latest, now)}
          </span>
        )}
        <Button
          size="sm"
          icon={RefreshCw}
          disabled={checking || !checkable}
          onClick={() => void checkNow()}
        >
          {checking ? t("wishlist.checking") : t("wishlist.checkNow")}
        </Button>
        <Button
          size="sm"
          variant="primary"
          icon={Plus}
          disabled={picking || add !== null}
          onClick={() => setPicking(true)}
        >
          {t("wishlist.addButton")}
        </Button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-6 py-4 @max-[640px]/pane:px-4">
        <div className="mx-auto flex w-full max-w-[1100px] flex-col gap-2">
          {add === null && picking ? (
            <section
              aria-label={t("wishlist.edit.addRegion")}
              data-testid="wish-pick"
              className="flex min-w-0 flex-col gap-2 rounded-(--r-10) border border-solid border-line-edge bg-surface-card p-3"
            >
              <span className="text-(length:--fs-12) font-(weight:--fw-600) text-gold">
                {t("wishlist.add")}
              </span>
              <WishTitlePicker
                onCancel={() => setPicking(false)}
                onPick={(hit) => {
                  setPicking(false);
                  void navigate({
                    to: "/torrents/wishlist",
                    search: {
                      add: titleParam({
                        kind: hit.kind === "show" ? "show" : "movie",
                        tmdbId: hit.tmdbId,
                      }),
                    },
                  });
                }}
              />
            </section>
          ) : null}
          {add === null ? null : <AddPanel add={add} search={search} onAdded={setAdded} />}
          {body}
        </div>
      </div>
      <p className="m-0 shrink-0 border-t border-solid border-line-section px-6 py-1.5 text-(length:--fs-12) text-text-muted @max-[640px]/pane:px-4">
        {t("wishlist.foot")}
      </p>
    </section>
  );
}

/**
 * `/torrents/wishlist` (spec `torrent-wishlist`): the titles the user wants a release of, with
 * what each looks for, its matches (Send to torrent app, Dismiss), Check now, and the editor for a
 * title being added. Behind the Torrent search notice like the search itself.
 */
export function WishlistPage() {
  const accepted = useTorrentNotice();
  if (accepted === null) {
    return <section aria-label={t("wishlist.region")} data-testid="wishlist-page" />;
  }
  if (!accepted) {
    return (
      <section
        aria-label={t("wishlist.region")}
        data-testid="wishlist-page"
        className="flex h-full min-h-0 flex-col gap-4 px-6 pt-4 @max-[640px]/pane:px-4"
      >
        <h1 className="sr-only">{t("wishlist.region")}</h1>
        <TorrentNotice />
      </section>
    );
  }
  return <WishlistContent />;
}
