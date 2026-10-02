# README artwork

The wordmark extends Closingtime's existing bracket, connector and owned-node identity. The palette and typefaces match the separate website project. These assets are self-contained; the README does not load external fonts or badge services.

- `logo-light.png` and `logo-dark.png`: transparent wordmarks for the reader's theme.
- `ownership-walkthrough.gif`: a three-step illustrative motion graphic. It plays twice and settles on the review state.
- `ownership-static.png`: the final state, linked directly and used for reduced-motion readers.
- `badges/`: static license, Rust, platform and prototype labels. They do not claim CI results or published packages.
- `fonts/`: Manrope and JetBrains Mono, each with its OFL license.

The visual is authored artwork, not a recording of terminal output. Run IDs, projects, process names and ports are illustrative. The unknown listener has no owner edge; kept work stays excluded from the preview. No cleanup is performed by the graphic or its renderer.

## Regenerate

With Pillow installed in your Python environment, run from the repository root:

```sh
python3 scripts/readme-assets.py
```

The script generates both logos, the animation, its static fallback, and the badges. It makes no network calls and does not run Closingtime or inspect local processes.

GitHub supports image media in Markdown, but cannot run an interactive demo or arbitrary animation scripts. The README therefore embeds a raster animation with a static alternative. Theme and reduced-motion selection use standard `<picture>` media queries.
