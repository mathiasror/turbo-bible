# website/

Hand-authored static site for turbo-bible. No build step, no SSG —
just HTML, CSS, and a sprinkle of JS.

## Deploy

GitHub Pages. `.github/workflows/pages.yml` triggers on push to
`main` whenever anything under `website/**` changes (or on manual
`workflow_dispatch`) and uploads this directory as the Pages
artifact. `CNAME` pins the custom domain to `turbo.bible`.

DNS setup (do once, in the registrar): four `A` records for the
apex pointing at GitHub Pages (`185.199.108.153`, `.109.153`,
`.110.153`, `.111.153`) plus a `CNAME` `www → mathiasror.github.io.`.

## Working on it

Open `index.html` in a browser. There's no dev server.

`og-image.png` (the 1200x630 social card referenced by the OG/Twitter
meta tags) is a real VHS capture of the splash screen — regenerate with
`just og-image` (source: `demo/og-image.tape`, needs VHS and the bundled
translations, same toolchain as the demo GIF and the user-guide shots).

`reading.png` (the reading-view screenshot in the page body) is a copy
of `docs/screenshots/03-reading.png`, produced by `just screenshots`.

`apple-touch-icon.png` (the 180x180 home-screen icon, a raster twin of
the inline-SVG favicon) is likewise generated — regenerate with `just
apple-touch-icon` (source: `demo/apple-touch-icon.py`, needs Pillow).
