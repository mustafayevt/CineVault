import { cx } from "../../lib/cx";
import { spansOf } from "./notesFormat";
import type { NotesBlock } from "./notesFormat";

function Line({ text, links }: { text: string; links: boolean }) {
  return (
    <>
      {spansOf(text, { links }).map((span, index) =>
        span.href !== undefined ? (
          <a
            key={index}
            href={span.href}
            target="_blank"
            rel="noreferrer noopener"
            className="text-text underline decoration-line-edge underline-offset-2 hover:decoration-text-2"
          >
            {span.text}
          </a>
        ) : span.bold ? (
          <strong key={index} className="font-(weight:--fw-600) text-text">
            {span.text}
          </strong>
        ) : (
          <span key={index}>{span.text}</span>
        ),
      )}
    </>
  );
}

interface NotesBodyProps {
  blocks: readonly NotesBlock[];
  /** Allow `[text](https://...)` links. Only for text that ships with the app. */
  links?: boolean;
  /** `reading` is the larger type of the release-notes page. */
  size?: "compact" | "reading";
}

/**
 * Notes as headings, paragraphs and bullet lists. Everything is drawn as text: the notes never
 * become markup, and a link exists only when `links` is on and its address is https.
 */
export function NotesBody({ blocks, links = false, size = "compact" }: NotesBodyProps) {
  const reading = size === "reading";
  return (
    <>
      {blocks.map((block, index) =>
        block.kind === "heading" ? (
          <h4
            key={index}
            className={cx(
              "m-0 font-(weight:--fw-600) text-text",
              reading ? "mt-2 text-(length:--fs-13)" : "text-(length:--fs-12)",
            )}
          >
            <Line text={block.text} links={links} />
          </h4>
        ) : block.kind === "paragraph" ? (
          <p key={index} className="m-0">
            <Line text={block.text} links={links} />
          </p>
        ) : (
          <ul
            key={index}
            className={cx("m-0 flex list-disc flex-col pl-5", reading ? "gap-1.5" : "gap-1")}
          >
            {block.items.map((item, at) => (
              <li key={at} className="pl-0.5">
                <Line text={item} links={links} />
              </li>
            ))}
          </ul>
        ),
      )}
    </>
  );
}
