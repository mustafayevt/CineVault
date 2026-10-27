/**
 * Placeholder pictures for the synthetic backend (a plain browser, `bun run shots`): flat-colour
 * bands and dunes, drawn here, so the detail pages can be judged with a real backdrop, poster
 * and clear logo. The desktop app never reaches this: its images come from `cv://img`.
 */

const key = (n: number): string => n.toString(16).padStart(32, "0");

function dataUri(svg: string): string {
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

/** A 16:9 backdrop: a dusk sky, a low sun and three layers of dunes. */
const BACKDROP =
  dataUri(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 900" preserveAspectRatio="xMidYMid slice">
<rect width="1600" height="900" fill="#8a4a3a"/>
<rect width="1600" height="300" fill="#1b2540"/>
<rect y="300" width="1600" height="220" fill="#5d3a40"/>
<rect y="760" width="1600" height="140" fill="#e9a15c"/>
<circle cx="1120" cy="470" r="320" fill="#ffc37a" opacity=".3"/>
<circle cx="1120" cy="470" r="190" fill="#ffc37a" opacity=".6"/>
<circle cx="1120" cy="470" r="64" fill="#fff1d2"/>
<path d="M0 560 C240 500 420 540 640 580 S1080 600 1300 540 S1520 520 1600 540 V900 H0Z" fill="#5a2f35"/>
<path d="M0 650 C200 600 420 650 640 690 S1060 690 1260 640 S1500 610 1600 640 V900 H0Z" fill="#3a1f2b"/>
<path d="M0 760 C260 700 480 770 760 790 S1200 760 1600 740 V900 H0Z" fill="#1d1220"/>
</svg>`);

/** A 2:3 poster: the same palette, tall, with a lone figure on a dune. */
const POSTER =
  dataUri(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 900" preserveAspectRatio="xMidYMid slice">
<rect width="600" height="900" fill="#9b5340"/>
<rect width="600" height="320" fill="#232b4a"/>
<rect y="640" width="600" height="260" fill="#f0b56d"/>
<circle cx="300" cy="360" r="150" fill="#ffd9a0" opacity=".85"/>
<path d="M0 640 C150 580 300 620 600 560 V900 H0Z" fill="#4a2630"/>
<path d="M0 760 C200 700 380 760 600 700 V900 H0Z" fill="#1d1220"/>
<path d="M286 640 l14 -70 l14 70 z" fill="#0f0a14"/><circle cx="300" cy="556" r="9" fill="#0f0a14"/>
<rect x="120" y="96" width="360" height="14" rx="7" fill="#fff" opacity=".85"/>
<rect x="190" y="128" width="220" height="10" rx="5" fill="#fff" opacity=".5"/>
</svg>`);

/** A clear logo: a wide, transparent wordmark in light letters. */
const LOGO = dataUri(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 760 150">
<text x="2" y="112" text-anchor="start" font-family="Georgia, 'Times New Roman', serif" font-size="104" letter-spacing="18" fill="#9a8a74">NOCTURNE</text>
</svg>`);

const PICTURES: Readonly<Record<string, string>> = {
  [key(1)]: BACKDROP,
  [key(4)]: BACKDROP,
  [key(2)]: POSTER,
  [key(5)]: POSTER,
  [key(3)]: LOGO,
};

/** The placeholder picture for a fixture image key, or null for any other key. */
export function syntheticImageUrl(imageKey: string): string | null {
  return PICTURES[imageKey] ?? null;
}
