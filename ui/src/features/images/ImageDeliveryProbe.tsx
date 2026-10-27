import { ImageIcon } from "lucide-react";
import { useState } from "react";
import type { FormEvent } from "react";

import { Button } from "../../components/Button";
import { Field } from "../../components/Field";
import { CachedImage } from "./CachedImage";
import { registerImageSource } from "./imageSource";

type ProbeState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; key: string }
  | { status: "error"; message: string };

/**
 * A small test surface for the image protocol: give it the path of an image file and it shows the
 * grid and list thumbnails, loaded through `cv://img`. Inspect the network log of the web view to
 * see the requests, their cache headers and the absence of image data in IPC.
 */
export function ImageDeliveryProbe() {
  const [path, setPath] = useState("");
  const [state, setState] = useState<ProbeState>({ status: "idle" });

  async function load(event: FormEvent) {
    event.preventDefault();
    if (path.trim() === "") {
      return;
    }
    setState({ status: "loading" });
    try {
      setState({ status: "ready", key: await registerImageSource(path.trim()) });
    } catch (error) {
      setState({
        status: "error",
        message: error instanceof Error ? error.message : "That image could not be loaded.",
      });
    }
  }

  return (
    <section aria-labelledby="image-delivery-title" className="flex flex-col gap-3">
      <h2 id="image-delivery-title" className="type-page-title">
        Image delivery
      </h2>
      <p className="type-body text-text-2">
        Thumbnails load through the <code className="font-mono">cv://img</code> protocol with
        long-lived cache headers, generated on first request. No image data crosses IPC.
      </p>
      <form onSubmit={(event) => void load(event)} className="flex max-w-2xl items-end gap-2">
        <Field
          label="Image file path"
          icon={ImageIcon}
          mono
          className="flex-1"
          value={path}
          onChange={(event) => setPath(event.target.value)}
          error={state.status === "error" ? state.message : undefined}
        />
        <Button type="submit" variant="default" disabled={state.status === "loading"}>
          Load image
        </Button>
      </form>
      {state.status === "ready" && (
        <div className="flex items-end gap-4">
          <figure className="flex flex-col gap-1">
            <CachedImage
              imageKey={state.key}
              size="grid"
              alt="Grid thumbnail"
              className="aspect-[2/3] w-[150px] rounded-(--r-6)"
            />
            <figcaption className="type-small text-text-muted">grid</figcaption>
          </figure>
          <figure className="flex flex-col gap-1">
            <CachedImage
              imageKey={state.key}
              size="list"
              alt="List thumbnail"
              className="aspect-[2/3] w-12 rounded-(--r-6)"
            />
            <figcaption className="type-small text-text-muted">list</figcaption>
          </figure>
        </div>
      )}
    </section>
  );
}
