import { useId } from "react";
import type { ReactNode } from "react";

import { cx } from "../../lib/cx";
import { ratingSourceName } from "./ratingSources";

/*
 * The marks of the rating sources: each source's recognisable logo, drawn small beside its value.
 *
 * The outlines come from the Simple Icons set (CC0 1.0, simpleicons.org): only the few paths the
 * strip needs are inlined here, so the package is not part of the bundle. The colours are the
 * sources' own brand colours, the one kind of literal colour the design system allows outside the
 * tokens. Sources without a mark get a monogram in the same visual size.
 */

/** Simple Icons `imdb`, the letters only (the frame is drawn as a filled badge). */
const IMDB_LETTERS =
  "M4.7954 8.2603v7.3636H2.8899V8.2603h1.9055zm6.5367 0v7.3636H9.6707v-4.9704l-.6711 4.9704H7.813l-.6986-4.8618-.0066 4.8618h-1.668V8.2603h2.468c.0748.4476.1492.9694.2307 1.5734l.2712 1.8713.4407-3.4447h2.4817zm2.9772 1.3289c.0742.0404.122.108.1417.2034.0279.0953.0345.3118.0345.6442v2.8548c0 .4881-.0345.7867-.0955.8954-.0609.1152-.2304.1695-.5018.1695V9.5211c.204 0 .3457.0205.4211.0681zm-.0211 6.0347c.4543 0 .8006-.0265 1.0245-.0742.2304-.0477.4204-.1357.5694-.2648.1556-.1218.2642-.298.3251-.5219.0611-.2238.1021-.6648.1021-1.3224v-2.5832c0-.6986-.0271-1.1668-.0742-1.4039-.041-.237-.1431-.4543-.3126-.6437-.1695-.1973-.4198-.3324-.7456-.421-.3191-.0808-.8542-.1285-1.7694-.1285h-1.4244v7.3636h2.3051zm5.14-1.7827c0 .3523-.0199.5762-.0544.6708-.033.0947-.1894.1424-.3046.1424-.1086 0-.19-.0477-.2238-.1351-.041-.0887-.0609-.2986-.0609-.6238v-1.9469c0-.3324.0199-.5423.0543-.6237.0338-.0808.1086-.122.2171-.122.1153 0 .2709.0412.3114.1425.041.0947.0609.2986.0609.6032v1.8926zm-2.4747-5.5809v7.3636h1.7157l.1152-.4675c.1556.1894.3251.3324.5152.4271.1828.0881.4608.1357.678.1357.3047 0 .5629-.0748.7802-.237.2165-.1562.3589-.3462.4198-.5628.0543-.2173.0887-.543.0887-.9841v-2.0675c0-.4409-.0139-.7324-.0344-.8681-.0199-.1357-.0742-.2781-.1695-.4204-.1021-.1425-.2437-.251-.4272-.3325-.1834-.0742-.3999-.1152-.6576-.1152-.2172 0-.4952.0477-.6846.1285-.1835.0887-.353.2238-.5086.4007V8.2603h-1.8309z";

/** Simple Icons `trakt`. */
const TRAKT =
  "M12 24C5.385 24 0 18.615 0 12S5.385 0 12 0s12 5.385 12 12-5.385 12-12 12zm0-22.789C6.05 1.211 1.211 6.05 1.211 12S6.05 22.79 12 22.79 22.79 17.95 22.79 12 17.95 1.211 12 1.211zm-7.11 17.32c1.756 1.92 4.294 3.113 7.11 3.113 1.439 0 2.801-.313 4.027-.876l-6.697-6.68-4.44 4.443zm14.288-.067c1.541-1.71 2.484-3.99 2.484-6.466 0-3.885-2.287-7.215-5.568-8.76l-6.089 6.076 9.164 9.15h.009zm-9.877-8.429L4.227 15.09l-.679-.68 5.337-5.336 6.23-6.225c-.978-.328-2.02-.509-3.115-.509C6.663 2.337 2.337 6.663 2.337 12c0 2.172.713 4.178 1.939 5.801l5.056-5.055.359.329 7.245 7.245c.15-.082.285-.164.42-.266L9.33 12.05l-4.854 4.855-.679-.679 5.535-5.535.359.331 8.46 8.437c.135-.1.255-.215.375-.316L9.39 10.027l-.083.015-.006-.007zm3.047 1.028l-.678-.676 4.788-4.79.679.689-4.789 4.785v-.008zm4.542-6.578l-5.52 5.52-.68-.679 5.521-5.52.679.684v-.005z";

/** Simple Icons `rottentomatoes`. */
const TOMATO =
  "M5.866 0L4.335 1.262l2.082 1.8c-2.629-.989-4.842 1.4-5.012 2.338 1.384-.323 2.24-.422 3.344-.335-7.042 4.634-4.978 13.148-1.434 16.094 5.784 4.612 13.77 3.202 17.91-1.316C27.26 13.363 22.993.65 10.86 2.766c.107-1.17.633-1.503 1.243-1.602-.89-1.493-3.67-.734-4.556 1.374C7.52 2.602 5.866 0 5.866 0zM4.422 7.217H6.9c2.673 0 2.898.012 3.55.202 1.06.307 1.868.973 2.313 1.904.05.106.092.206.13.305l7.623.008.027 2.912-2.745-.024v7.549l-2.982-.016v-7.522l-2.127.016a2.92 2.92 0 0 1-1.056 1.134c-.287.176-.3.19-.254.264.127.2 2.125 3.642 2.125 3.659l-3.39.019-2.013-3.376c-.034-.047-.122-.068-.344-.084l-.297-.02.037 3.48-3.075-.038zm3.016 2.288l.024.338c.014.186.024.729.024 1.206v.867l.582-.025c.32-.013.695-.049.833-.078.694-.146 1.048-.478 1.087-1.018.027-.378-.063-.636-.303-.87-.318-.309-.761-.416-1.733-.418Z";

/** Simple Icons `metacritic`. */
const METACRITIC =
  "M11.99 0A12 12 0 1 0 24 12v-.014A12 12 0 0 0 11.99 0Zm-.055 2.564a9.399 9.399 0 0 1 9.407 9.389v.01a9.399 9.399 0 1 1-9.408-9.399Zm-1.61 17.198 2.046-2.046-3.94-3.94c-.165-.166-.345-.373-.442-.608-.221-.47-.318-1.203.221-1.742.664-.664 1.548-.387 2.406.47l3.788 3.788 2.046-2.046-3.954-3.954a2.48 2.48 0 0 1-.456-.622c-.263-.539-.25-1.216.235-1.7.677-.678 1.562-.429 2.544.553l3.677 3.677 2.046-2.046-3.982-3.982c-2.018-2.018-3.912-1.949-5.212-.65-.498.499-.802 1.024-.954 1.618a4.026 4.026 0 0 0-.055 1.686l-.027.028c-.996-.414-2.13-.166-3 .705-1.162 1.161-1.12 2.392-.982 3.11l-.042.043-1.009-.816-1.77 1.77a64.1 64.1 0 0 1 2.213 2.1z";

/** Simple Icons `myanimelist`. */
const MYANIMELIST =
  "M8.273 7.247v8.423l-2.103-.003v-5.216l-2.03 2.404-1.989-2.458-.02 5.285H.001L0 7.247h2.203l1.865 2.545 2.015-2.546 2.19.001zm8.628 2.069l.025 6.335h-2.365l-.008-2.871h-2.8c.07.499.21 1.266.417 1.779.155.381.298.751.583 1.128l-1.705 1.125c-.349-.636-.622-1.337-.878-2.082a9.296 9.296 0 0 1-.507-2.179c-.085-.75-.097-1.471.107-2.212a3.908 3.908 0 0 1 1.161-1.866c.313-.293.749-.5 1.1-.687.351-.187.743-.264 1.107-.359a7.405 7.405 0 0 1 1.191-.183c.398-.034 1.107-.066 2.39-.028l.545 1.749H14.51c-.593.008-.878.001-1.341.209a2.236 2.236 0 0 0-1.278 1.92l2.663.033.038-1.81h2.309zm3.992-2.099v6.627l3.107.032-.43 1.775h-4.807V7.187l2.13.03z";

/** One drawn mark: its view box and the shapes in it. */
interface MarkArt {
  viewBox: string;
  /** Width over height of the view box. */
  aspect: number;
  draw: (clipId: string) => ReactNode;
}

const MARKS: Readonly<Record<string, MarkArt>> = {
  imdb: {
    viewBox: "1.4 6.9 21.2 10.2",
    aspect: 21.2 / 10.2,
    draw: () => (
      <>
        <rect x="1.4" y="6.9" width="21.2" height="10.2" rx="1.6" fill="#F5C518" />
        <path d={IMDB_LETTERS} fill="#000000" />
      </>
    ),
  },
  // TMDB's stacked logo is unreadable this small: its short wordmark in its own blue instead.
  tmdb: {
    viewBox: "0 0 30 13",
    aspect: 30 / 13,
    draw: () => (
      <>
        <rect width="30" height="13" rx="2.6" fill="#0D253F" />
        <text
          x="15"
          y="9.9"
          textAnchor="middle"
          fontSize="9.6"
          fontWeight="800"
          letterSpacing="0.2"
          fill="#01B4E4"
        >
          TMDB
        </text>
      </>
    ),
  },
  trakt: {
    viewBox: "0 0 24 24",
    aspect: 1,
    draw: () => (
      <>
        <circle cx="12" cy="12" r="12" fill="#ED1C24" />
        <path d={TRAKT} fill="#FFFFFF" />
      </>
    ),
  },
  letterboxd: {
    viewBox: "0 7.4 24 9.2",
    aspect: 24 / 9.2,
    draw: (clipId) => (
      <>
        <clipPath id={clipId}>
          <circle cx="12" cy="12" r="4.444" />
        </clipPath>
        <circle cx="4.45" cy="12" r="4.444" fill="#FF8000" />
        <circle cx="19.55" cy="12" r="4.444" fill="#40BCF4" />
        <circle cx="12" cy="12" r="4.444" fill="#00E054" />
        {/* Where the green dot overlaps its neighbours the logo turns lighter. */}
        <g clipPath={`url(#${clipId})`}>
          <circle cx="4.45" cy="12" r="4.444" fill="#FFFFFF" fillOpacity="0.45" />
          <circle cx="19.55" cy="12" r="4.444" fill="#FFFFFF" fillOpacity="0.45" />
        </g>
      </>
    ),
  },
  tomato: {
    viewBox: "0 0 24 24",
    aspect: 1,
    draw: () => <path d={TOMATO} fill="#FA320A" fillRule="evenodd" />,
  },
  metacritic: {
    viewBox: "0 0 24 24",
    aspect: 1,
    draw: () => (
      <>
        <circle cx="12" cy="12" r="11.5" fill="#FFCC34" />
        <path d={METACRITIC} fill="#000000" />
      </>
    ),
  },
  myanimelist: {
    viewBox: "-1.2 5.4 26.4 13.2",
    aspect: 2,
    draw: () => (
      <>
        <rect x="-1.2" y="5.4" width="26.4" height="13.2" rx="2.4" fill="#2E51A2" />
        <path d={MYANIMELIST} fill="#FFFFFF" />
      </>
    ),
  },
};

/** How each source is drawn: a mark, with the small "people" glyph for the audience scores. */
const SOURCE_MARKS: Readonly<Record<string, { mark: string; people?: boolean; avg?: boolean }>> = {
  imdb: { mark: "imdb" },
  tmdb: { mark: "tmdb" },
  trakt: { mark: "trakt" },
  letterboxd: { mark: "letterboxd" },
  tomatometerallcritics: { mark: "tomato" },
  tomatometerallaudience: { mark: "tomato", people: true },
  tomatometeravgcritics: { mark: "tomato", avg: true },
  tomatometeravgaudience: { mark: "tomato", people: true, avg: true },
  metacritic: { mark: "metacritic" },
  metacriticuser: { mark: "metacritic", people: true },
  myanimelist: { mark: "myanimelist" },
};

/** The letters of a source without a mark. */
const MONOGRAMS: Readonly<Record<string, string>> = {
  rogerebert: "RE",
  tvdb: "TVDB",
  tvmaze: "TVm",
  moviemeter: "MM",
  ofdb: "OF",
  anidb: "AD",
  mpdb: "MP",
  fernsehserien: "FS",
};

/** The monogram of a source: its own, or the first two letters of its name. */
function ratingMonogram(source: string): string {
  const own = MONOGRAMS[source.toLowerCase()];
  if (own !== undefined) return own;
  const letters = ratingSourceName(source).replace(/[^\p{L}\p{N}]/gu, "");
  return letters.slice(0, 2).toUpperCase() || "?";
}

/** A small person: the audience or user score of a source that also has a critics score. */
function PeopleGlyph({ size }: { size: number }) {
  const s = Math.round(size * 0.62);
  return (
    <svg
      viewBox="0 0 10 10"
      width={s}
      height={s}
      aria-hidden="true"
      focusable="false"
      className="text-text-2 -ml-0.5 shrink-0 self-end"
      data-glyph="people"
    >
      <circle cx="5" cy="3" r="2.1" fill="currentColor" />
      <path d="M1 10a4 4 0 0 1 8 0Z" fill="currentColor" />
    </svg>
  );
}

interface RatingMarkProps {
  /** The stored source key: `imdb`, `tomatometerallaudience`, ... */
  source: string;
  /** The height of the mark in pixels. */
  size?: number;
  /** Greyed out: the source has no value to show. */
  muted?: boolean;
  className?: string;
}

/**
 * The mark of a rating source at `size` pixels high: its logo in its brand colours, or a
 * monogram for a source without one. Audience and user scores add a small person, the averages
 * a small "avg", so the two scores of one site stay apart. Decorative: the chip carries the name.
 */
export function RatingMark({ source, size = 14, muted = false, className }: RatingMarkProps) {
  const clipId = useId();
  const key = source.toLowerCase();
  const spec = SOURCE_MARKS[key];
  const art = spec === undefined ? undefined : MARKS[spec.mark];
  const body =
    art === undefined ? (
      <Monogram letters={ratingMonogram(key)} size={size} />
    ) : (
      <svg
        viewBox={art.viewBox}
        width={Math.round(size * art.aspect * 10) / 10}
        height={size}
        aria-hidden="true"
        focusable="false"
        className="shrink-0"
      >
        {art.draw(clipId)}
      </svg>
    );
  return (
    <span
      aria-hidden="true"
      data-mark={spec?.mark ?? "monogram"}
      className={cx(
        "inline-flex shrink-0 items-center gap-0.5 leading-none",
        muted && "opacity-60 grayscale",
        className,
      )}
    >
      {body}
      {spec?.people === true ? <PeopleGlyph size={size} /> : null}
      {spec?.avg === true ? (
        <span className="text-text-2 self-end text-[9px] font-semibold">avg</span>
      ) : null}
    </span>
  );
}

/** The letters of a source without a logo, in a small outlined tile as high as the logos. */
function Monogram({ letters, size }: { letters: string; size: number }) {
  return (
    <span
      className="border-text-2 text-text-2 inline-flex shrink-0 items-center justify-center rounded-[3px] border px-[3px] font-bold tracking-[0.02em]"
      style={{
        height: size,
        minWidth: size,
        fontSize: Math.max(8, Math.round(size * 0.58)),
      }}
    >
      {letters}
    </span>
  );
}
