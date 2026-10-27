import { Select } from "../../components/Select";
import { useSearch } from "@tanstack/react-router";

import { t } from "../../i18n/catalog";
import { KeyField } from "../providers/KeyField";
import { useProviderKeys } from "../providers/useProviderKeys";
import { FormRow, SettingsSection } from "./FormParts";
import { COUNTRIES, LANGUAGES } from "./schema";
import { useSetting, useSettingsStore } from "./settingsStore";

const set = (id: string, value: string) => {
  useSettingsStore.getState().set(id, value);
};

/** "Get a free key" beside a hint, opening the provider's page. */
function GetKey({ href, label }: { href: string | undefined; label: string }) {
  if (href === undefined || href === "") return null;
  return (
    <>
      {" "}
      <a
        href={href}
        target="_blank"
        rel="noreferrer noopener"
        className="text-text-2 hover:text-text hover:underline"
      >
        {label}
      </a>
    </>
  );
}

/**
 * Settings, Providers (spec `app-shell` "Settings page"): the TMDB key and the optional OMDb,
 * MDBList, Fanart.tv and TheTVDB keys with their state and Test, and the metadata language and
 * ratings country. Changes apply at once.
 */
export function ProvidersSection() {
  const keys = useProviderKeys();
  // A link that asks for one key (search says "TheTVDB needs a key") leaves the cursor in it.
  const { key: wanted } = useSearch({ strict: false }) as { key?: string };
  const language = useSetting<string>("metadata.language");
  const country = useSetting<string>("metadata.country");
  const find = (id: string) => keys.providers.find((provider) => provider.id === id);
  return (
    <SettingsSection
      id="providers"
      title={t("settingsPage.providers.title")}
      hint={t("settingsPage.providers.hint")}
    >
      <FormRow
        label={t("settingsPage.providers.tmdb")}
        hint={
          <>
            {t("settingsPage.providers.tmdb.hint")}
            <GetKey
              href={find("tmdb")?.registration_url}
              label={t("settingsPage.providers.getKey")}
            />
          </>
        }
      >
        <KeyField
          provider={find("tmdb")}
          keys={keys}
          label={t("settingsPage.providers.tmdb")}
          focusOnShow={wanted === "tmdb"}
        />
      </FormRow>
      <FormRow
        label={t("settingsPage.providers.omdb")}
        hint={
          <>
            {t("settingsPage.providers.omdb.hint")}
            <GetKey
              href={find("omdb")?.registration_url}
              label={t("settingsPage.providers.getKey")}
            />
          </>
        }
      >
        <KeyField
          provider={find("omdb")}
          keys={keys}
          label={t("settingsPage.providers.omdb")}
          focusOnShow={wanted === "omdb"}
        />
      </FormRow>
      <FormRow
        label={t("settingsPage.providers.mdblist")}
        hint={
          <>
            {t("settingsPage.providers.mdblist.hint")}
            <GetKey
              href={find("mdblist")?.registration_url}
              label={t("settingsPage.providers.getKey")}
            />
          </>
        }
      >
        <KeyField
          provider={find("mdblist")}
          keys={keys}
          label={t("settingsPage.providers.mdblist")}
          focusOnShow={wanted === "mdblist"}
        />
      </FormRow>
      <FormRow
        label={t("settingsPage.providers.fanart")}
        hint={
          <>
            {t("settingsPage.providers.fanart.hint")}
            <GetKey
              href={find("fanart")?.registration_url}
              label={t("settingsPage.providers.getKey")}
            />
          </>
        }
      >
        <KeyField
          provider={find("fanart")}
          keys={keys}
          label={t("settingsPage.providers.fanart")}
          focusOnShow={wanted === "fanart"}
        />
      </FormRow>
      <FormRow
        label={t("settingsPage.providers.tvdb")}
        hint={
          <>
            {t("settingsPage.providers.tvdb.hint")}
            <GetKey
              href={find("tvdb")?.registration_url}
              label={t("settingsPage.providers.getKey")}
            />
          </>
        }
      >
        <KeyField
          provider={find("tvdb")}
          keys={keys}
          label={t("settingsPage.providers.tvdb")}
          focusOnShow={wanted === "tvdb"}
        />
      </FormRow>
      <FormRow
        label={t("settingsPage.providers.language")}
        hint={t("settingsPage.providers.language.hint")}
      >
        <div className="flex flex-wrap gap-2">
          <Select
            className="w-[260px]"
            aria-label={t("settingsPage.providers.language")}
            options={LANGUAGES}
            value={language}
            onValueChange={(value) => {
              set("metadata.language", value);
            }}
          />
          <Select
            className="w-[200px]"
            aria-label={t("settingsPage.providers.country")}
            options={COUNTRIES.map((option) => ({
              value: option.value,
              label: t("settingsPage.providers.countryOption", { country: option.label }),
            }))}
            value={country}
            onValueChange={(value) => {
              set("metadata.country", value);
            }}
          />
        </div>
      </FormRow>
    </SettingsSection>
  );
}
