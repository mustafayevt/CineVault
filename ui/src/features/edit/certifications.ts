/**
 * The certifications the picker offers, by country. The library stores a certification as a token
 * (`US_R`) and the edit fields carry it as `US: R`; people read it as `R (US)`.
 */
export interface CertificationScheme {
  /** ISO 3166 country code. */
  country: string;
  ratings: readonly string[];
}

export const CERTIFICATION_SCHEMES: readonly CertificationScheme[] = [
  {
    country: "US",
    ratings: ["G", "PG", "PG-13", "R", "NC-17", "TV-Y", "TV-G", "TV-PG", "TV-14", "TV-MA"],
  },
  { country: "GB", ratings: ["U", "PG", "12A", "15", "18"] },
  { country: "DE", ratings: ["FSK 0", "FSK 6", "FSK 12", "FSK 16", "FSK 18"] },
  { country: "FR", ratings: ["U", "10", "12", "16", "18"] },
];

/** A certification split into its country (when it names one) and its rating. */
export interface ParsedCertification {
  country: string | null;
  rating: string;
}

/** The text a field holds for a country and a rating: `US: R`. */
export function certificationValue(country: string | null, rating: string): string {
  return country === null ? rating : `${country}: ${rating}`;
}
