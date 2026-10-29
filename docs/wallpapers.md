# Wallpapers: My backgrounds and Wallhaven

System Settings > Wallpaper has two tabs, switched by **Pictures from**:

* **My backgrounds**: the pictures in `~/.config/omarchy/backgrounds/**` (or `$DECEBAL_CONFIG/backgrounds`), scaling,
  background colour and a path field. Picking one writes `[wallpaper] image` in rdwm's `config.toml`; rdwm applies it
  and "Colours from wallpaper" / "Match wallpaper" follow it.
* **Wallhaven**: browse and search [wallhaven.cc](https://wallhaven.cc) and set a wallpaper from it without leaving
  the settings window. The standalone `dechaven` browser still exists; this tab is the same idea inside System
  Settings, with filters, an optional API key, favourites and a random button.

## Using the Wallhaven tab

1. Open the tab. The top list of the last month loads (SFW, at least your screen's size).
2. Type words and press Enter to search; **Sort by** Top / Latest / Views / Faves / Random / Relevant (Top has a
   range 1d ... 1y). **Filters** opens categories, content, resolution, shape and a colour.
3. Scroll: the next page loads when you get near the end (24 per page). Thumbnails are loaded only for what is on screen.
4. Click a wallpaper. The block at the top shows it large with its colour palette, size, views and favourites, and,
   a moment later, its tags.
   * **Set as wallpaper** downloads it into the backgrounds folder as `wallhaven-<id>.<ext>` and sets it through the
     same code as picking a picture on My backgrounds (saved in `config.toml`, applied by rdwm, colours follow).
   * **Download only** saves it without applying.
   * **Add to favourites** keeps a local list (**Show: Favourites**); it works offline.
   * **Open in browser** opens the wallhaven.cc page. **Close** hides the block.
5. **Set a random one** picks a random wallpaper from the current search and sets it.

### From a script

```
decthemectl wallpaper wallhaven random                 # a random one from your current filters
decthemectl wallpaper wallhaven search mountain lake   # the best match
decthemectl wallpaper wallhaven search forest --list   # only print id, size and page of the matches
decthemectl wallpaper wallhaven random --screen 3440x1440 --no-set
```

It reads the same filters and API key as the tab, downloads the same way and sets the wallpaper through the same
code, so it persists and the dynamic colours follow. Without `--screen` the size comes from rdwm's first monitor.

## Content: safe by default

Only safe-for-work wallpapers are requested. Sketchy and NSFW appear only when **both** hold: you saved an API key
and switched on **Allow sketchy and NSFW results** (Filters > Account); then the Sketchy / NSFW chips choose which.
The check is made twice: when the query is built, and again on every result the server sends back (anything not
allowed is dropped), and a download of a not-allowed wallpaper is refused.

## API key

A key is optional (wallhaven needs it only for NSFW and your own account's settings). Paste it in Filters > Account
and press Enter. It is stored in `$XDG_CONFIG_HOME/decebal/wallhaven.key` (default `~/.config/decebal/`), mode
0600 in a 0700 folder, written atomically. It is never shown again (the field shows dots), never logged, never put in
a URL (it travels as the `X-API-Key` header, only to the API host, never to image hosts and not across redirects) and
never written to `decebal.json` or `wallhaven.json`, which are safe to share. **Remove** deletes the file. Only
letters and digits (8-64) are accepted.

## Files

| File | Holds |
|---|---|
| `$XDG_CONFIG_HOME/decebal/wallhaven.json` | search text, sort, filters, the sketchy / NSFW switch |
| `$XDG_CONFIG_HOME/decebal/wallhaven.key` | the API key (0600) |
| `$XDG_STATE_HOME/decebal/wallhaven-favourites.json` | favourites (up to 500) |
| `$XDG_CACHE_HOME/decebal/wallhaven/last-search.json` | the last first page, shown when offline |
| `$XDG_CACHE_HOME/decebal/wallhaven/thumbs/` | small thumbnails, pruned to 400 files / 24 MB |
| `<backgrounds>/wallhaven-<id>.<ext>` | downloaded wallpapers |

## Being a good API citizen

* Identifies itself: `User-Agent: decebal-decsettings/<version> (wallhaven wallpaper picker)`.
* Wallhaven allows 45 API requests a minute. All API calls go through one worker and a sliding window of 40 a minute;
  a call that would have to wait more than 4 s is refused instead. A `429` starts a cool-down (`Retry-After`, default
  60 s, at most 10 min) during which nothing is sent, and nothing is ever retried automatically: the pane says
  so and offers **Try again**. Thumbnails and images come from wallhaven's image hosts, not the API, and are not counted.
* Typing and filter changes are debounced (0.4 s); selecting a wallpaper asks for its tags only after 0.45 s.
* Everything network or disk related runs on worker threads with timeouts (8 s connect, 20 s read); the window never waits.
* Offline: the last first page and the cached thumbnails are shown with a note; paging stops until you press Try again.

## Downloads are checked

Only `https://w.wallhaven.cc/` and `https://th.wallhaven.cc/` are fetched. A wallpaper must be served as
`image/jpeg`, `image/png` or `image/webp`, at most 40 MB (refused from `Content-Length` before the body, and again
while reading), its first bytes must match that type, its end must look complete, and its header must decode to a
sane size (at most 120 megapixels). It is written to a hidden `.part` file and renamed into place, so a failed or
cut-off download never leaves a half file; the name is always `wallhaven-<id>.<ext>` with the extension taken from
the content type and the id validated (`[a-z0-9]{1,16}`).

## Memory

Decoded thumbnails live in an LRU of 120 entries (about 110 KB for a small one, 240 KB for the large preview),
so the tab stays around 30 MB resident in total. Workers start only when the tab is first opened.

## Testing

* Unit tests (`cargo test`): `src/settings/wallhaven/*` parse recorded API responses (`tests/fixtures/wallhaven/`,
  recorded from the real API with the uploader removed), the query-string builder, purity gating, the throttle, the
  caches, file-name / content-type / size validation, and the HTTP client against a throw-away local server.
* `tests/e2e/wallhaven.sh [display]`: private Xvfb + rdwm + a local fake wallhaven
  (`tests/e2e/fakes/wallhaven_server.py`, 127.0.0.1 only; the app is pointed at it with `WALLHAVEN_API_BASE`).
  Walks through browsing, selecting, setting, downloading, favourites, infinite scroll, search, filters, the API key
  (file mode, never in URLs or other files), random, offline, 429 and the CLI. `SHOTS=dir` saves screenshots.
* `WALLHAVEN_API_BASE=http://127.0.0.1:PORT/api/v1` also makes image requests to that host acceptable, for testing only.

## Live wallpapers

The **Live wallpapers** tab adds muted local videos and a Wikimedia Commons video search with source/licence information. Playback pauses on each output covered by a visible fullscreen app, retaining a frozen frame. An extracted image remains the normal rdwm wallpaper as a fallback. See [Live wallpapers](live-wallpapers.md) for dependencies, power behaviour, CLI commands and validation.
