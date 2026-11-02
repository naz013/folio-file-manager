# Folio landing page

Plain HTML and CSS, deployed to GitHub Pages by `.github/workflows/pages.yml`.
`tokens.css` is copied from `src/lib/styles/tokens.css` at build, so the page shares the app's design tokens.

Preview locally:

```bash
cp src/lib/styles/tokens.css site/tokens.css
npx serve site
```

Before it works for visitors:

- Enable Pages in the repo settings with Source set to "GitHub Actions". On a private repo this needs a paid plan, and the page's GitHub links return 404 to anyone not signed in, so make the repo public first.
- Publish a signed release. The Download button links to `releases/latest`, which only shows published releases, and the installer is unsigned unless the `WINDOWS_CERTIFICATE` secrets are set (see `docs/RELEASING.md`).
