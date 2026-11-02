import type { Ref } from "react";

import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { NetworkShareForm } from "../shares/NetworkShareForm";
import type { ShareDone } from "../shares/NetworkShareForm";
import { useShareSupport } from "../shares/shareQueries";
import { StepHeading } from "./SetupSteps";

interface ShareStepProps {
  headingRef: Ref<HTMLHeadingElement>;
  /** The share is connected and its folder is in the library. */
  onDone: (done: ShareDone) => void;
  /** Back to the folders. */
  onCancel: () => void;
  /** Linux without share mounting: the way on is the folder picker. */
  onAddFolder: () => void;
}

/**
 * The Network share page inside the first-run setup, in place of the Folders step while it is
 * open: the same form as in Settings, where the folder takes the type it looks like.
 */
export function ShareStep({ headingRef, onDone, onCancel, onAddFolder }: ShareStepProps) {
  const support = useShareSupport();
  return (
    <>
      <StepHeading title={t("networkShare.title")} headingRef={headingRef}>
        {t("networkShare.sub.any")}
      </StepHeading>
      <div className="px-6 pb-6 @max-[640px]/pane:px-3">
        {support.data === undefined ? (
          <div aria-busy="true">
            <Skeleton shape="tile" aspect="4 / 1" />
          </div>
        ) : (
          <NetworkShareForm
            mode={{ kind: "connect", library: null }}
            support={support.data}
            onDone={onDone}
            onCancel={onCancel}
            onAddFolder={onAddFolder}
          />
        )}
      </div>
    </>
  );
}
