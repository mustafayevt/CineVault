import { encode } from "uqr";
import { useMemo } from "react";

interface QrCodeProps {
  /** The text to encode, such as `http://192.168.1.20:8787`. */
  value: string;
  /** The side in CSS pixels. */
  size?: number;
  /** What a screen reader says instead of the picture. */
  label: string;
}

/**
 * A QR code drawn as one SVG path: dark modules on a light ground with a quiet zone, in both
 * themes, because many phone cameras do not read an inverted code.
 */
export function QrCode({ value, size = 132, label }: QrCodeProps) {
  const { path, cells } = useMemo(() => {
    const code = encode(value, { ecc: "M", border: 2 });
    let d = "";
    code.data.forEach((row, y) => {
      row.forEach((dark, x) => {
        if (dark) d += `M${x} ${y}h1v1h-1z`;
      });
    });
    return { path: d, cells: code.size };
  }, [value]);
  return (
    <svg
      role="img"
      aria-label={label}
      width={size}
      height={size}
      viewBox={`0 0 ${cells} ${cells}`}
      shapeRendering="crispEdges"
      className="block rounded-(--r-6)"
      data-qr={value}
    >
      <rect width={cells} height={cells} fill="var(--qr-paper)" />
      <path d={path} fill="var(--qr-ink)" />
    </svg>
  );
}
