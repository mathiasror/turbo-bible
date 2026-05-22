# website/

Hand-authored static site for turbo-bible. No build step, no SSG —
just HTML, CSS, and a sprinkle of JS.

## Deploy

GitHub Pages, deployed from this directory. No workflow is wired up
yet; when there's real content worth deploying, add
`.github/workflows/pages.yml` that triggers on push to `main`
filtered to `website/**` and uploads the directory as the Pages
artifact.

## Working on it

Open `index.html` in a browser. There's no dev server.
