import type { LucideIcon } from "lucide-react";

/** Design system icon stroke: outline, 1.75px, rounded caps and joins. */
const ICON_STROKE = 1.75;

interface IconProps {
  icon: LucideIcon;
  /** Drawn at 14-16px in the UI, 18 in the sidebar; 12 is for glyphs inside mono pips and tags, 24 for empty states. */
  size?: 12 | 14 | 16 | 18 | 20 | 24;
  className?: string;
}

/** Decorative lucide icon. Controls that contain only an icon must carry an `aria-label`. */
export function Icon({ icon: Glyph, size = 16, className }: IconProps) {
  return (
    <Glyph
      aria-hidden="true"
      focusable="false"
      strokeWidth={ICON_STROKE}
      size={size}
      className={className}
    />
  );
}
