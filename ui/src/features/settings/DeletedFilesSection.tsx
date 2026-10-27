import { FolderOpen } from "lucide-react";
import { useId, useState } from "react";

import { Button } from "../../components/Button";
import { PathText } from "../../components/PathText";
import { Select } from "../../components/Select";
import { Switch } from "../../components/Switch";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { currentPickerBackend } from "../pickers/pickerBackend";
import { EraseConfirmDialog } from "../trash/EraseConfirmDialog";
import { FormRow, SettingsSection } from "./FormParts";
import { useSetting, useSettingsStore } from "./settingsStore";

/** Where deleted and replaced files go (`files.deleted-mode`). */
export type DeletedMode = "bin" | "folder" | "custom" | "erase";

const MODES: readonly DeletedMode[] = ["bin", "folder", "custom", "erase"];

/** The automatic emptying choices (`files.auto-empty-days`). */
const AUTO_EMPTY = ["off", "7", "30", "90"] as const;

/**
 * Settings, Deleted files (spec `title-deletion` "Where deleted files go"): where Delete, a move's
 * originals, rename leftovers and replaced artwork or NFO files go: the Recycle Bin (default),
 * the trash folder on each drive, a custom trash folder, or erased at once. Choosing Delete
 * permanently asks for the typed confirmation of Shift+Delete first. Below it, whether Delete asks
 * first (`delete.confirm`; Delete permanently always asks) and the automatic
 * emptying of CineVault's own trash folders (off by default).
 */
export function DeletedFilesSection() {
  const mode = useSetting<string>("files.deleted-mode") as DeletedMode;
  const folder = useSetting<string>("files.trash-folder");
  const autoEmpty = useSetting<string>("files.auto-empty-days");
  const askFirst = useSetting<boolean>("delete.confirm");
  const [confirming, setConfirming] = useState(false);
  const group = useId();
  const picker = currentPickerBackend();
  const set = (id: string, value: string) => {
    useSettingsStore.getState().set(id, value);
  };

  const choose = (next: DeletedMode) => {
    if (next === mode) return;
    // Erasing at once is chosen only with the typed confirmation of a permanent delete.
    if (next === "erase") {
      setConfirming(true);
      return;
    }
    set("files.deleted-mode", next);
  };

  const browse = () => {
    void picker
      .pick("folder", { title: t("settingsPage.deleted.pickTitle") })
      .then(([path]) => {
        if (path === undefined) return;
        set("files.trash-folder", path);
        set("files.deleted-mode", "custom");
      })
      .catch(() => undefined);
  };

  return (
    <SettingsSection id="deleted-files" title={t("settingsPage.deleted.title")}>
      <FormRow
        align="start"
        label={<span id={`${group}-label`}>{t("settings.files.deleted-mode.title")}</span>}
        hint={t("settings.files.deleted-mode.description")}
      >
        <div
          role="radiogroup"
          aria-labelledby={`${group}-label`}
          className="flex flex-col gap-1"
          data-setting="files.deleted-mode"
        >
          {MODES.map((value) => {
            const id = `${group}-${value}`;
            const checked = mode === value;
            return (
              <div
                key={value}
                className={cx(
                  "cv-hoverable flex cursor-pointer items-start gap-2.5 rounded-(--r-6) px-2 py-1.5",
                  "hover:bg-surface-raised active:bg-surface-active",
                  checked && "bg-surface-raised",
                )}
              >
                <input
                  id={id}
                  type="radio"
                  name={group}
                  value={value}
                  checked={checked}
                  onChange={() => choose(value)}
                  className="mt-[3px] cursor-pointer accent-gold"
                />
                <label htmlFor={id} className="flex min-w-0 flex-1 cursor-pointer flex-col">
                  <span
                    className={cx(
                      "font-(weight:--fw-500)",
                      value === "erase" ? "text-danger" : "text-text",
                    )}
                  >
                    {t(`settings.files.deleted-mode.option.${value}`)}
                  </span>
                  <span
                    className={cx(
                      "text-(length:--fs-12)",
                      value === "erase" ? "text-danger" : "text-text-muted",
                    )}
                  >
                    {t(`settingsPage.deleted.help.${value}`)}
                  </span>
                </label>
              </div>
            );
          })}
        </div>
      </FormRow>
      {mode === "custom" ? (
        <FormRow
          label={t("settings.files.trash-folder.title")}
          hint={t("settings.files.trash-folder.description")}
        >
          <div className="flex min-w-0 items-center gap-2">
            {folder === "" ? (
              <span className="min-w-0 flex-1 text-warn">{t("settingsPage.deleted.noFolder")}</span>
            ) : (
              <PathText
                path={folder}
                className="min-w-0 flex-1 font-mono text-(length:--fs-12) text-text"
              />
            )}
            {picker.available ? (
              <Button icon={FolderOpen} onClick={browse}>
                {t("settingsPage.deleted.choose")}
              </Button>
            ) : null}
          </div>
        </FormRow>
      ) : null}
      <FormRow
        labelId={`${group}-confirm`}
        label={t("settings.delete.confirm.title")}
        hint={t("settings.delete.confirm.description")}
        dim={mode === "erase"}
      >
        <Switch
          aria-labelledby={`${group}-confirm`}
          data-setting="delete.confirm"
          // Delete permanently always asks, whatever this says.
          checked={askFirst || mode === "erase"}
          disabled={mode === "erase"}
          onCheckedChange={(on) => useSettingsStore.getState().set("delete.confirm", on)}
        />
      </FormRow>
      <FormRow
        label={t("settings.files.auto-empty-days.title")}
        hint={t("settings.files.auto-empty-days.description")}
      >
        <Select
          className="w-[160px]"
          aria-label={t("settings.files.auto-empty-days.title")}
          options={AUTO_EMPTY.map((value) => ({
            value,
            label: t(`settings.files.auto-empty-days.option.${value}`),
          }))}
          value={autoEmpty}
          onValueChange={(value) => set("files.auto-empty-days", value)}
        />
      </FormRow>
      {confirming ? (
        <EraseConfirmDialog
          title={t("settingsPage.deleted.confirmTitle")}
          description={t("settingsPage.deleted.help.erase")}
          confirmLabel={t("settingsPage.deleted.confirm")}
          onConfirm={() => {
            set("files.deleted-mode", "erase");
            setConfirming(false);
          }}
          onClose={() => setConfirming(false)}
        />
      ) : null}
    </SettingsSection>
  );
}
