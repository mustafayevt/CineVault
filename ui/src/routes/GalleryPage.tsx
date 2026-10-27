import { useState } from "react";

import { BrandMark } from "../components/BrandMark";
import { FloatingContainerContext } from "../components/floating";
import {
  PlaceholderBackdrop,
  PlaceholderFace,
  PlaceholderPoster,
  PlaceholderStill,
} from "../components/PlaceholderPoster";
import { TooltipProvider } from "../components/Tooltip";
import { ImageDeliveryProbe } from "../features/images/ImageDeliveryProbe";
import type { Theme } from "../stores/shell";
import { ContainerSections } from "./GalleryPageContainers";
import { ControlSections } from "./GalleryPageControls";
import { FloatingSections, MotionSections } from "./GalleryPageFloating";
import { GalleryThemeContext } from "./galleryTheme";
import { Group, Row } from "./GalleryPageParts";

function Art() {
  return (
    <Group
      title="Brand mark and generated artwork"
      note="A title without artwork gets a poster drawn from its name; the same name always gives the same poster."
    >
      <Row label="Brand mark">
        <BrandMark size={18} />
        <BrandMark size={32} />
        <BrandMark variant="symbol" size={18} />
        <BrandMark variant="symbol" size={48} label="CineVault" />
      </Row>
      <Row label="Generated posters, backdrop, still and face">
        {["Arrival", "Heat", "Blade Runner 2049", "The Assassination of Jesse James", "Drive"].map(
          (title) => (
            <div
              key={title}
              className="relative aspect-[2/3] w-20 overflow-hidden rounded-(--r-10)"
            >
              <PlaceholderPoster title={title} />
            </div>
          ),
        )}
        <div className="relative h-20 w-44 overflow-hidden rounded-(--r-10)">
          <PlaceholderBackdrop title="Arrival" />
        </div>
        <div className="relative aspect-video w-24 overflow-hidden rounded-(--r-6)">
          <PlaceholderStill title="Severance" index={1} />
        </div>
        <div className="relative size-14 overflow-hidden rounded-(--r-10)">
          <PlaceholderFace name="Rebecca Ferguson" />
        </div>
      </Row>
    </Group>
  );
}

/**
 * Every shared component in all its states inside one scoped theme. Floating surfaces are drawn
 * into this column (not the page), so a menu in the light column is light whatever the window is.
 */
function ThemePanel({ theme }: { theme: Theme }) {
  const label = theme === "dark" ? "Dark theme" : "Light theme";
  const [container, setContainer] = useState<HTMLElement | null>(null);
  return (
    <section
      ref={setContainer}
      data-theme={theme}
      aria-label={label}
      className="flex min-w-0 flex-col gap-8 rounded-(--r-14) bg-surface-window p-6 text-(length:--fs-13) text-text"
    >
      <GalleryThemeContext value={theme}>
        <FloatingContainerContext value={container}>
          <h2 className="m-0 text-(length:--fs-20) font-(weight:--fw-600)">{label}</h2>
          <ControlSections />
          <ContainerSections />
          <FloatingSections />
          <MotionSections />
          <Art />
        </FloatingContainerContext>
      </GalleryThemeContext>
    </section>
  );
}

/** `/gallery`: every shared component, in all its states, in the dark and the light theme. */
export function GalleryPage() {
  return (
    <TooltipProvider>
      <div className="flex min-h-full flex-col gap-8 bg-surface-window p-8 text-text">
        <header className="flex flex-col gap-1">
          <h1 className="m-0 text-(length:--fs-28) font-(weight:--fw-600)">Component gallery</h1>
          <p className="m-0 text-(length:--fs-13) text-text-2">
            The shared components in the dark and the light theme: every size and state, the
            floating layer, and the motion each one has.
          </p>
        </header>
        <div className="grid gap-8 xl:grid-cols-2">
          <ThemePanel theme="dark" />
          <ThemePanel theme="light" />
        </div>
        <ImageDeliveryProbe />
      </div>
    </TooltipProvider>
  );
}
