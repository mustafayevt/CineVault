import { Link } from "@tanstack/react-router";
import { Compass } from "lucide-react";

import { Icon } from "../components/Icon";
import { t } from "../i18n/catalog";

/** The router's fallback for an address that matches no page. It keeps the app shell around it. */
export function NotFoundPage() {
  return (
    <section
      aria-labelledby="not-found-title"
      className="flex flex-col items-start gap-3 px-(--inset) pt-6"
    >
      <Icon icon={Compass} size={24} className="text-text-muted" />
      <h1 id="not-found-title" className="type-page-title">
        {t("notFound.title")}
      </h1>
      <p className="type-body max-w-prose text-text-2">{t("notFound.body")}</p>
      <Link to="/library" className="type-body rounded-(--r-6) text-gold hover:underline">
        {t("notFound.action")}
      </Link>
    </section>
  );
}
