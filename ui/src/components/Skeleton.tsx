import { cx } from "../lib/cx";
import { useLateFlag } from "./motionHooks";
import "./skeleton.css";

export type SkeletonShape = "poster" | "row" | "line" | "tile";

const shapes: Record<SkeletonShape, string> = {
  // A 2:3 poster box, the same as the real poster, so nothing shifts when it arrives.
  poster: "aspect-[2/3] w-full rounded-(--r-10)",
  // One table row at the row height.
  row: "h-(--h-row) w-full rounded-(--r-4)",
  // A line of text: the width says how long the line will be.
  line: "h-4 rounded-(--r-4)",
  // A tile of any aspect ratio, given by `aspect`: a candidate that has not arrived yet.
  tile: "w-full rounded-(--r-10)",
};

/** How long data may take before a skeleton shows (spec desktop-interaction "Reserved space"). */
export const SKELETON_DELAY_MS = 150;

interface SkeletonProps {
  shape: SkeletonShape;
  /** Line only: the width of the line to come, as a CSS length such as "60%" or "12rem". */
  width?: string;
  /** Line only: the height of the block to come, for a figure or a chart that is taller than text. */
  height?: string;
  /** Tile only: the CSS aspect ratio of the image to come, such as "2 / 3". */
  aspect?: string;
  /** Shows at once instead of after 150 ms (the gallery and screenshots; never in the app). */
  immediate?: boolean;
  className?: string;
}

/**
 * A placeholder with the shape of the content it stands in for. It reserves its box from the first
 * frame but stays invisible for the first 150 ms (`useLateFlag`), so data that arrives quickly
 * never flashes a skeleton. It is hidden from assistive technology (the region that loads carries
 * `aria-busy`), shimmers by transform while motion is allowed and is still under reduced motion.
 * Never put a spinner where a skeleton fits.
 */
export function Skeleton({
  shape,
  width,
  height,
  aspect,
  immediate = false,
  className,
}: SkeletonProps) {
  const late = useLateFlag(SKELETON_DELAY_MS, !immediate);
  const visible = immediate || late;
  return (
    <div
      aria-hidden="true"
      data-cv-skeleton=""
      data-shape={shape}
      data-visible={visible ? "true" : "false"}
      className={cx(
        "bg-surface-active",
        shapes[shape],
        visible ? "cv-xfade" : "opacity-0",
        className,
      )}
      style={
        width === undefined && height === undefined && aspect === undefined
          ? undefined
          : { width, height, aspectRatio: aspect }
      }
    />
  );
}
