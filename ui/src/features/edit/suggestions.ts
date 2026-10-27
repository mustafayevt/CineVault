/** Short lists of known values the editors offer as suggestions. They never limit what can be typed. */

import { CERTIFICATION_SCHEMES, certificationValue } from "./certifications";

/** Certifications as the editors write them (`US: R`); the library stores them as tokens. */
export const CERTIFICATIONS: readonly string[] = CERTIFICATION_SCHEMES.flatMap((scheme) =>
  scheme.ratings.map((rating) => certificationValue(scheme.country, rating)),
);

export const EDITIONS = [
  "Directors Cut",
  "Extended Edition",
  "Theatrical Edition",
  "Unrated",
  "Uncut",
  "IMAX",
  "Remastered",
  "Collectors Edition",
  "Final Cut",
  "Special Edition",
];

export const SOURCES = [
  "UHD Bluray",
  "Bluray",
  "DVD",
  "HDDVD",
  "TV",
  "VHS",
  "WEBRIP",
  "WEB DL",
  "Stream",
];

export const GENRES = [
  "Action",
  "Adventure",
  "Animation",
  "Anime",
  "Biography",
  "Comedy",
  "Crime",
  "Documentary",
  "Drama",
  "Family",
  "Fantasy",
  "Film Noir",
  "History",
  "Horror",
  "Music",
  "Musical",
  "Mystery",
  "Romance",
  "Science Fiction",
  "Short",
  "Sport",
  "Thriller",
  "War",
  "Western",
];
