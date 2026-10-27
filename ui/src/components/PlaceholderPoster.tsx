import type { ReactNode } from "react";

import { cx } from "../lib/cx";
import { initials, posterArt, posterLines, posterTitle, shade } from "./posterArt";
import type { Motif } from "./posterArt";
import { SYMBOL } from "./brandPaths";

interface PlaceholderPosterProps {
  /** The best-known title: the matched title, else the one parsed from the file name. */
  title: string;
  className?: string;
}

/** The motif of a generated poster in a 200×300 box, drawn in the palette's ink. */
function motifShape(motif: Motif, c1: string, c2: string, ink: string): ReactNode {
  switch (motif) {
    case "sun":
      return (
        <>
          <circle cx="100" cy="118" r="46" fill={c2} />
          <rect x="0" y="150" width="200" height="150" fill={c1} opacity=".55" />
          <path d="M0 170 Q 100 140 200 175 L200 300 L0 300Z" fill={c2} opacity=".18" />
        </>
      );
    case "ring":
      return (
        <>
          <circle cx="100" cy="120" r="62" fill="none" stroke={c2} strokeWidth="14" />
          <circle cx="100" cy="120" r="30" fill={c2} opacity=".35" />
        </>
      );
    case "horizon":
      return (
        <>
          <rect x="0" y="0" width="200" height="130" fill={c2} opacity=".9" />
          <rect x="0" y="130" width="200" height="4" fill={ink} opacity=".25" />
        </>
      );
    case "haze":
      return (
        <>
          <circle cx="100" cy="120" r="84" fill={c2} opacity=".25" />
          <circle cx="100" cy="120" r="52" fill={c2} opacity=".45" />
          <path d="M0 230 L80 170 L120 190 L200 150 L200 300 L0 300Z" fill={c1} opacity=".9" />
        </>
      );
    case "wall":
      return (
        <>
          <rect x="24" y="34" width="152" height="180" fill={c2} opacity=".12" />
          <rect x="24" y="34" width="152" height="180" fill="none" stroke={c2} strokeWidth="3" />
          <line x1="24" y1="124" x2="176" y2="124" stroke={c2} strokeWidth="3" />
          <line x1="100" y1="34" x2="100" y2="214" stroke={c2} strokeWidth="3" />
        </>
      );
    case "fall":
      return (
        <>
          <polygon points="30,30 170,30 100,220" fill={c2} />
          <rect x="0" y="222" width="200" height="6" fill={c2} opacity=".35" />
        </>
      );
    case "orb":
      return (
        <>
          <circle cx="100" cy="112" r="60" fill={c2} />
          <circle cx="118" cy="96" r="16" fill={c1} opacity=".9" />
          <path d="M40 112 a60 60 0 0 0 120 0 Z" fill={c1} opacity=".3" />
        </>
      );
    case "leaves":
      return (
        <g fill={c2} opacity=".9">
          <ellipse cx="60" cy="90" rx="22" ry="46" transform="rotate(-30 60 90)" />
          <ellipse cx="130" cy="120" rx="22" ry="52" transform="rotate(25 130 120)" />
          <ellipse cx="95" cy="190" rx="18" ry="40" transform="rotate(-10 95 190)" />
        </g>
      );
    case "city":
      return (
        <>
          <g fill={c2} opacity=".85">
            <rect x="18" y="130" width="26" height="100" />
            <rect x="52" y="90" width="22" height="140" />
            <rect x="82" y="110" width="34" height="120" />
            <rect x="124" y="70" width="18" height="160" />
            <rect x="150" y="120" width="32" height="110" />
          </g>
          <rect x="0" y="230" width="200" height="70" fill={c2} opacity=".15" />
        </>
      );
    case "neon":
      return (
        <g stroke={c2} strokeWidth="5" fill="none" strokeLinecap="round">
          <path d="M20 70 H180" />
          <path d="M20 110 H140" opacity=".7" />
          <path d="M20 150 H180" />
          <path d="M20 190 H110" opacity=".7" />
        </g>
      );
    case "snow":
      return (
        <>
          <rect width="200" height="300" fill={c2} opacity=".08" />
          <g fill={c2}>
            <circle cx="40" cy="60" r="4" />
            <circle cx="120" cy="40" r="3" />
            <circle cx="160" cy="100" r="5" />
            <circle cx="70" cy="140" r="3" />
            <circle cx="150" cy="180" r="4" />
            <circle cx="30" cy="210" r="5" />
            <circle cx="100" cy="100" r="2.5" />
          </g>
          <path d="M0 240 Q100 200 200 240 L200 300 L0 300Z" fill={c2} opacity=".9" />
        </>
      );
    case "shadow":
      return (
        <>
          <path d="M70 230 L70 80 Q70 40 100 40 Q130 40 130 80 L130 230 Z" fill={c2} opacity=".9" />
          <path d="M60 230 L140 230 L190 300 L10 300 Z" fill={c2} opacity=".2" />
        </>
      );
    case "court":
      return (
        <>
          <rect x="22" y="40" width="156" height="190" fill="none" stroke={c2} strokeWidth="4" />
          <line x1="22" y1="135" x2="178" y2="135" stroke={c2} strokeWidth="4" />
          <circle cx="100" cy="135" r="22" fill="none" stroke={c2} strokeWidth="4" />
        </>
      );
    case "stairs":
      return (
        <g fill={c2} opacity=".9">
          <rect x="20" y="200" width="160" height="22" />
          <rect x="48" y="166" width="132" height="22" />
          <rect x="76" y="132" width="104" height="22" />
          <rect x="104" y="98" width="76" height="22" />
          <rect x="132" y="64" width="48" height="22" />
        </g>
      );
    case "flame":
      return (
        <path
          d="M100 60 C140 110 150 150 120 190 C110 150 100 150 96 130 C70 160 60 190 80 220 C40 190 50 120 100 60Z"
          fill={c2}
        />
      );
    case "pod":
      return (
        <>
          <ellipse cx="100" cy="130" rx="48" ry="96" fill={c2} opacity=".9" />
          <ellipse cx="100" cy="130" rx="36" ry="84" fill={c1} opacity=".5" />
          <rect x="0" y="236" width="200" height="64" fill={c2} opacity=".15" />
        </>
      );
    case "beam":
      return (
        <>
          <polygon points="100,70 0,0 0,140" fill={c2} opacity=".35" />
          <polygon points="100,70 200,0 200,140" fill={c2} opacity=".35" />
          <rect x="90" y="70" width="20" height="150" fill={c2} />
          <circle cx="100" cy="70" r="12" fill={c2} />
        </>
      );
  }
}

/** The generated poster as an SVG that fills its box (`slice`, so any 2:3-ish box works). */
function PosterArtSvg({ title, className }: { title: string; className?: string }) {
  const art = posterArt(title);
  const [c1, c2] = art.palette;
  const { lines, small } = posterLines(title);
  const size = small ? 17 : 21;
  const top = 300 - 18 - (lines.length - 1) * size * 1.08;
  return (
    <svg
      viewBox="0 0 200 300"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
      className={cx("block size-full", className)}
    >
      <rect width="200" height="300" fill={c1} />
      {motifShape(art.motif, c1, c2, art.ink)}
      <rect
        x="0"
        y="210"
        width="200"
        height="90"
        fill={art.dark ? "#000" : "#fff"}
        opacity={art.dark ? 0.28 : 0.2}
      />
      {lines.map((line, index) => (
        <text
          key={`${index}-${line}`}
          x="16"
          y={(top + index * size * 1.08).toFixed(1)}
          fontFamily="Archivo, 'Segoe UI', sans-serif"
          fontSize={size}
          fontWeight="600"
          letterSpacing="-.3"
          fill={art.ink}
          style={{ fontStretch: "104%" }}
        >
          {line}
        </text>
      ))}
      {/* The maker's mark: the brand symbol at 9 units in the corner, so a generated poster reads
          as CineVault's own and is never mistaken for real artwork. */}
      <g
        transform="translate(179 279) scale(0.5625)"
        fill={art.ink}
        opacity="0.35"
        data-testid="poster-mark"
      >
        <path d={SYMBOL.ring} />
        <path d={SYMBOL.frame} />
      </g>
    </svg>
  );
}

/**
 * The poster of a title that has no artwork: a generated typographic poster in the title's own
 * palette (a motif and the title, chosen deterministically from the title), never a grey box. It
 * fills its positioned parent. The title is the accessible name, so the card or row that holds it
 * does not need to repeat it.
 */
export function PlaceholderPoster({ title, className }: PlaceholderPosterProps) {
  const name = posterTitle(title) || title;
  return (
    <div
      data-testid="placeholder-poster"
      role="img"
      aria-label={name}
      className={cx("absolute inset-0 overflow-hidden rounded-[inherit]", className)}
    >
      <PosterArtSvg title={title} />
    </div>
  );
}

/** A generated backdrop in the title's palette: a flat ground, the poster's motif and a darker foot. */
export function PlaceholderBackdrop({ title, className }: { title: string; className?: string }) {
  const art = posterArt(title);
  const [c1, c2] = art.palette;
  return (
    <svg
      viewBox="0 0 1600 700"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
      data-testid="placeholder-backdrop"
      className={cx("block size-full", className)}
    >
      <rect width="1600" height="700" fill={c1} />
      {/* The poster's own motif, large and flat on the right, so hero and poster belong together. */}
      <g transform="translate(900 -120) scale(2.6)" opacity=".85" data-motif={art.motif}>
        {motifShape(art.motif, c1, c2, art.ink)}
      </g>
      <rect x="0" y="560" width="1600" height="140" fill={shade(c1, -0.35)} />
    </svg>
  );
}

/** A generated 16:9 still for an episode without a thumbnail, in its show's palette. */
export function PlaceholderStill({
  title,
  index,
  className,
}: {
  title: string;
  index: number;
  className?: string;
}) {
  const [c1, c2] = posterArt(title).palette;
  const k = Math.abs(index) % 9;
  return (
    <svg
      viewBox="0 0 160 90"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
      className={cx("block size-full", className)}
    >
      <rect width="160" height="90" fill={c1} />
      {k % 3 === 0 ? (
        <circle cx={40 + k * 9} cy="50" r="18" fill={c2} opacity=".8" />
      ) : k % 3 === 1 ? (
        <rect x={20 + k * 5} y="20" width="60" height="50" fill={c2} opacity=".55" />
      ) : (
        <path d={`M0 70 Q80 ${30 + k * 4} 160 65 L160 90 L0 90Z`} fill={c2} opacity=".6" />
      )}
    </svg>
  );
}

/**
 * A portrait placeholder: a soft silhouette on a tone picked from the name, with the initials.
 * Square; the parent sets the size and radius.
 */
export function PlaceholderFace({ name, className }: { name: string; className?: string }) {
  const base = posterArt(name).palette[0];
  const tone = posterArt(name).dark ? shade(base, 0.22) : shade(base, -0.35);
  return (
    <svg
      viewBox="0 0 100 100"
      aria-hidden="true"
      data-testid="placeholder-face"
      className={cx("block size-full", className)}
    >
      <rect width="100" height="100" fill={tone} />
      <circle cx="50" cy="40" r="17" fill="#000" opacity=".22" />
      <path d="M18 100 C18 72 82 72 82 100Z" fill="#000" opacity=".22" />
      <text
        x="50"
        y="94"
        fontFamily="Archivo, 'Segoe UI', sans-serif"
        fontSize="12"
        fontWeight="600"
        fill="#fff"
        opacity=".75"
        textAnchor="middle"
      >
        {initials(name)}
      </text>
    </svg>
  );
}
