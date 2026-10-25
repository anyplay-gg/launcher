# Bundled Noto fonts

Noto Sans substitutes for the console's system fonts, embedded in the launcher
(`crates/anyps5-core/src/fonts.rs`) and written to the data folder when a game needs them.
The file names are the ones the engine reads from `ANYPS5_SYSTEM_FONTS`. Licence: SIL Open Font
License 1.1, see `OFL.txt`. Japanese, Chinese and Korean (Noto Sans CJK) are not bundled; the
launcher downloads them on request.

Each file is the static TTF from the zip attached to the release named below, taken from the
folder shown. Noto Sans Mono has no `full` folder in its release, so its `hinted` TTFs are used.

| File | Release | Folder in the zip | SHA-256 |
|---|---|---|---|
| `NotoSansArabic-Bold.ttf` | [arabic NotoSansArabic-v2.013](https://github.com/notofonts/arabic/releases/tag/NotoSansArabic-v2.013) | `NotoSansArabic/full/ttf` | `5ccd1a8914f7c7e8aa8050f2c7c37b10fc5e855f06583c2a2248a436aad3fc0f` |
| `NotoSansArabic-Light.ttf` | [arabic NotoSansArabic-v2.013](https://github.com/notofonts/arabic/releases/tag/NotoSansArabic-v2.013) | `NotoSansArabic/full/ttf` | `fdad48f549b7d44e2926d0f19d300a221ae0a480d40e57b394df7359d2f6cf59` |
| `NotoSansArabic-Medium.ttf` | [arabic NotoSansArabic-v2.013](https://github.com/notofonts/arabic/releases/tag/NotoSansArabic-v2.013) | `NotoSansArabic/full/ttf` | `23c6d6520915f455585ef796db6897393e486637510dd3b255047f7095a11478` |
| `NotoSansArabic-Regular.ttf` | [arabic NotoSansArabic-v2.013](https://github.com/notofonts/arabic/releases/tag/NotoSansArabic-v2.013) | `NotoSansArabic/full/ttf` | `7ed3fe069312aceac454f17cf613a30f95271d6ed7ce58005ed4d016bd3823d7` |
| `NotoSans-BoldItalic.ttf` | [latin-greek-cyrillic NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) | `NotoSans/full/ttf` | `2e34b41a4b9c234b1be7dff6d06cba18811ecb694b41350873edf0ec16a0f0fa` |
| `NotoSans-Bold.ttf` | [latin-greek-cyrillic NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) | `NotoSans/full/ttf` | `3a08a47daa00cade516425c15c57615aef2fd418ec9811a7b9f465088f92cc05` |
| `NotoSans-Italic.ttf` | [latin-greek-cyrillic NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) | `NotoSans/full/ttf` | `126522ae1bb9cd92120287fc47dfc74ef981e73931d93e52c565fb7e09b2d74a` |
| `NotoSans-LightItalic.ttf` | [latin-greek-cyrillic NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) | `NotoSans/full/ttf` | `6164841f339c49462cf9b70d6df2f61878844e6ad7f7fccb7e7cdf51fa00904e` |
| `NotoSans-Light.ttf` | [latin-greek-cyrillic NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) | `NotoSans/full/ttf` | `ca1cf4f5ae2949be8f65fbdd1c3915be2037c12e6532b202b932c5d9a4414a9d` |
| `NotoSans-MediumItalic.ttf` | [latin-greek-cyrillic NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) | `NotoSans/full/ttf` | `b49c370b3809895db3d35e235c1f4d521dc85605b2aacfccb3c62a81290886e9` |
| `NotoSans-Medium.ttf` | [latin-greek-cyrillic NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) | `NotoSans/full/ttf` | `1d1570dd66d70cbcd56646c55580c3cc453c7abc505534230c165f93b55ad394` |
| `NotoSansMono-Bold.ttf` | [latin-greek-cyrillic NotoSansMono-v2.014](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSansMono-v2.014) | `NotoSansMono/hinted/ttf` | `a21ea0ba6ea49fda7b34ca39a504b487f1130885d36e1a4f9f4255b3ba6994bc` |
| `NotoSansMono-Light.ttf` | [latin-greek-cyrillic NotoSansMono-v2.014](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSansMono-v2.014) | `NotoSansMono/hinted/ttf` | `9c9dc46df01c3acee14690fc1be60f6c863675b5ac9ee190f304656dbb9ba9aa` |
| `NotoSansMono-Medium.ttf` | [latin-greek-cyrillic NotoSansMono-v2.014](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSansMono-v2.014) | `NotoSansMono/hinted/ttf` | `53e262499b3e6b4e7274dcc4840e9374c8d9c4a4fad48227a38d8cedd2e34c40` |
| `NotoSansMono-Regular.ttf` | [latin-greek-cyrillic NotoSansMono-v2.014](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSansMono-v2.014) | `NotoSansMono/hinted/ttf` | `65b5e2b2c4a1fba9ae8be1f026cb35b03dcb8886d9b2a4147054fde12f7e767d` |
| `NotoSans-Regular.ttf` | [latin-greek-cyrillic NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) | `NotoSans/full/ttf` | `f5f552c8c5edb61fe6efb824baf4d4de47b1a8689ab4925ff43f7bd6a4ebece5` |
| `NotoSansThai-Bold.ttf` | [thai NotoSansThai-v2.002](https://github.com/notofonts/thai/releases/tag/NotoSansThai-v2.002) | `NotoSansThai/full/ttf` | `dd99014fba00806b1d8da54afa978b37149b950b56c6b44b5e0b5c5dcd78f245` |
| `NotoSansThai-Light.ttf` | [thai NotoSansThai-v2.002](https://github.com/notofonts/thai/releases/tag/NotoSansThai-v2.002) | `NotoSansThai/full/ttf` | `a2a40fbdd72b56b1e6fcc5a45992adc5b3b5b071876156a1f67b1e27972ecfa6` |
| `NotoSansThai-Medium.ttf` | [thai NotoSansThai-v2.002](https://github.com/notofonts/thai/releases/tag/NotoSansThai-v2.002) | `NotoSansThai/full/ttf` | `c985a1b03881f1b82777b2e6ead45f9233a32871430140d9c5444dbbfa41e38a` |
| `NotoSansThai-Regular.ttf` | [thai NotoSansThai-v2.002](https://github.com/notofonts/thai/releases/tag/NotoSansThai-v2.002) | `NotoSansThai/full/ttf` | `2101faa471942570d506a394df7da555d15246c7814c7522c63d4fa85b0d8d88` |
