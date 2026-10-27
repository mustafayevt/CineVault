import { SegmentedControl } from "../../components/SegmentedControl";
import { Select } from "../../components/Select";
import { t } from "../../i18n/catalog";
import { detectPlatform } from "../shell/platform";
import { FormRow, SettingsSection } from "./FormParts";
import { useSetting, useSettingsStore } from "./settingsStore";
import { ZOOM_STEPS, snapZoom } from "./zoom";

type ThemeChoice = "light" | "dark" | "system";

/**
 * Settings, Appearance (spec `app-shell` "Themes"): Light, Dark or Match Windows (Match macOS).
 * The change applies at once and the sidebar's Theme button shows the same choice. Size is the app
 * zoom (80% to 150%); the zoom keys change the same setting.
 */
export function AppearanceSection() {
  const theme = useSetting<ThemeChoice>("appearance.theme");
  const zoom = snapZoom(useSetting<number>("appearance.zoom"));
  const mac = detectPlatform() === "macos";
  return (
    <SettingsSection id="appearance" title={t("settingsPage.appearance.title")}>
      <FormRow label={t("settingsPage.appearance.theme")}>
        <SegmentedControl<ThemeChoice>
          aria-label={t("settingsPage.appearance.theme")}
          value={theme}
          options={[
            { value: "light", label: t("settingsPage.appearance.light") },
            { value: "dark", label: t("settingsPage.appearance.dark") },
            {
              value: "system",
              label: t(mac ? "settingsPage.appearance.matchMac" : "settingsPage.appearance.match"),
            },
          ]}
          onChange={(value) => {
            useSettingsStore.getState().set("appearance.theme", value);
          }}
        />
      </FormRow>
      <FormRow
        label={t("settingsPage.appearance.size")}
        hint={t(mac ? "settingsPage.appearance.size.hintMac" : "settingsPage.appearance.size.hint")}
      >
        <Select
          className="w-[120px]"
          aria-label={t("settingsPage.appearance.size")}
          options={ZOOM_STEPS.map((step) => ({
            value: String(step),
            label: t("settingsPage.appearance.sizeOption", { percent: step }),
          }))}
          value={String(zoom)}
          onValueChange={(value) => {
            useSettingsStore.getState().set("appearance.zoom", Number(value));
          }}
        />
      </FormRow>
    </SettingsSection>
  );
}
