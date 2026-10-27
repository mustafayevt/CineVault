# CineVault brand assets

The logo, app icons, installer images and glyphs of CineVault. Every file was drawn as SVG first; the PNG, BMP, ICO and ICNS files were rendered from the SVGs. All text in the images is outlined Archivo, so nothing depends on installed fonts.

Colours: graphite `#121214`, bone `#ebe9e4`, paper `#f2f1ee`, ink `#1a1a19`, champagne `#cbb68a` (dark) / `#7a6438` (light); icon tile `#1e1e22`. Flat colour only: no gradients, glows or blurred shadows.

## Files and where they go

| File | Size | Used by | Notes |
| --- | --- | --- | --- |
| `logo/symbol-on-dark.svg`, `symbol-on-light.svg` | 48 grid | `ui/src/components/BrandMark.tsx` (paths inline) | Master symbol. Ring = text colour, frame = champagne. |
| `logo/symbol-mono-black.svg`, `symbol-mono-white.svg` | 48 grid | anywhere one colour is required | |
| `logo/hinted/symbol-{16,24,32,48}-on-{dark,light}.svg` | per size | `BrandMark` at 16/18/24 px | Whole-pixel edges at 16 and 32. |
| `logo/wordmark-*.svg` | 474 × 92 | about page | Outlined Archivo SemiExpanded 600. |
| `logo/lockup-horizontal-*.svg`, `lockup-stacked-*.svg` | | README, website, about page | `logo/png/*@2x.png` for places that need bitmaps. |
| `app-icon/windows/icon.ico` | 16, 24, 32, 48, 64, 256 | `src-tauri/icons/icon.ico` | Hand-hinted 16–64 (PNG entries). |
| `app-icon/windows/icon-*.svg`, `png/icon-*.png` | | sources of the ICO | `png/icon-1024.png` is the master for `tauri icon`. |
| `app-icon/macos/icon.icns` | 16–1024 incl. @2x | `src-tauri/icons/icon.icns` | Big Sur grid (824 body in 1024). 16 and 32 are separate drawings. |
| `app-icon/macos/icon-master-1024.svg`, `png/icon_*.png` | | sources of the ICNS | Names match an `.iconset` folder. |
| `app-icon/tauri/*.png` | | `src-tauri/icons/` (same names) | Window icon and Linux PNGs. |
| `tray/*.png` | 18, 36 | not used yet | Menu-bar / taskbar tray icons, ready for a tray feature. |
| `favicon/*` | | website | `ui/index.html` may use `favicon.svg` too. |
| `installer/*.svg`, `installer/png/*` | | `src-tauri/installer/*.bmp` | Sources of the 24-bit NSIS and WiX installer BMPs. |
| `dmg/dmg-background*.svg` | 660 × 400, 1320 × 800 | `src-tauri/installer/dmg-background.png` | Icons at (180, 190) and (480, 190). |
| `social/readme-banner.png`, `@2x.png` | 1280 × 320 | `.github/assets/readme-banner.png` | Top of `README.md`. |
| `social/social-preview.png` | 1280 × 640 | GitHub → Settings → Social preview | Upload by hand. |
| `glyphs/*.svg` | 24 × 24, stroke 1.75 | `ui/src/components/glyphs/` as React components | `currentColor`, lucide conventions. |

## Regenerating the app icons

`tauri icon` downsamples one image, so it cannot produce the hinted small sizes or the macOS icon. Run it from the repository root, then overwrite its outputs with the hand-made files:

```sh
bun run tauri icon assets/brand/app-icon/windows/png/icon-1024.png
cp assets/brand/app-icon/windows/icon.ico  src-tauri/icons/icon.ico
cp assets/brand/app-icon/macos/icon.icns   src-tauri/icons/icon.icns
cp assets/brand/app-icon/tauri/*.png       src-tauri/icons/
```

If `tauri icon` also creates `src-tauri/icons/android` or `ios`, delete them (the app is desktop only). Then run `bun run bundle:check`.
