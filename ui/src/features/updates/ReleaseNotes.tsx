import { t } from "../../i18n/catalog";
import { parseNotes, spansOf } from "./notesFormat";
import type { UpdateOffer } from "./updatesBackend";

function Line({ text }: { text: string }) {
  return (
    <>
      {spansOf(text).map((span, index) =>
        span.bold ? (
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

/**
 * "What's new in 1.2.0": the release notes of the offered version as a short readable list, from
 * the signed manifest. Long notes scroll inside the block. Nothing is drawn when there are none.
 */
export function ReleaseNotes({ offer }: { offer: UpdateOffer }) {
  const blocks = parseNotes(offer.notes);
  if (blocks.length === 0) return null;
  return (
    <section
      aria-label={t("settingsPage.updates.whatsNew", { version: offer.version })}
      data-testid="release-notes"
      className="flex flex-col gap-2 border-t border-solid border-line-row px-4 py-3"
    >
      <h3 className="m-0 text-(length:--fs-13) font-(weight:--fw-600)">
        {t("settingsPage.updates.whatsNew", { version: offer.version })}
      </h3>
      {/* A long list scrolls here, and can be scrolled with the keyboard. */}
      <div
        // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
        tabIndex={0}
        className="flex max-h-56 flex-col gap-2 overflow-y-auto pr-2 text-(length:--fs-13) text-text-2 outline-none focus-visible:outline-2 focus-visible:outline-gold"
      >
        {blocks.map((block, index) =>
          block.kind === "heading" ? (
            <h4 key={index} className="m-0 text-(length:--fs-12) font-(weight:--fw-600) text-text">
              <Line text={block.text} />
            </h4>
          ) : block.kind === "paragraph" ? (
            <p key={index} className="m-0">
              <Line text={block.text} />
            </p>
          ) : (
            <ul key={index} className="m-0 flex list-disc flex-col gap-1 pl-5">
              {block.items.map((item, at) => (
                <li key={at}>
                  <Line text={item} />
                </li>
              ))}
            </ul>
          ),
        )}
      </div>
    </section>
  );
}
