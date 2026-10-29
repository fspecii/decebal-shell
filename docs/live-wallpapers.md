# Live wallpapers

Open **System Settings → Wallpaper → Live wallpapers**. Paste a local MP4, WebM, MKV, MOV or OGV path and choose **Use this video**, or search DesktopHut or Wikimedia Commons and choose **Download and use**. The existing My backgrounds and Wallhaven tabs still select still images.

The player is optional: install `ffmpeg` to extract the fallback image, `mpv` for an X11 session, or `mpvpaper` for a Wayland session. No video plays until selected. `decwall daemon` restores an enabled selection at login; the supplied XDG autostart entry is limited to rdwm. A disabled selection exits immediately instead of leaving a background daemon. The systemd user unit is an alternative; do not enable it as well as the autostart entry.

## Playback and power

Each output has its own muted, looping player. Default playback is capped at 24 fps, configurable from 1–60 fps. Players request hardware decoding when available, have bounded demuxer buffers, use at most two software decoder threads, and run at low CPU priority. Actual resource use depends on resolution, codec and driver; frame-rate limiting does not eliminate the cost of decoding the source frames.

When a visible, non-minimised fullscreen app covers an output, only that output's player pauses. Its current frame stays visible and it stops advancing/decoding. The other outputs continue. Switching to a workspace without a fullscreen app, or leaving fullscreen, resumes playback. Pause detection runs twice a second through rdwm's local state socket. If rdwm is temporarily unavailable, players pause rather than continue blindly. Paused players retain their decoder/frame buffers; this is not a zero-memory mode.

A video frame is extracted once, scaled to at most 1920 pixels wide and saved as rdwm's ordinary `[wallpaper] image`. This is the startup/failure fallback and the still wallpaper retained when you stop animation. A missing or failed player leaves that image visible. Invalid videos do not replace the current wallpaper. Choosing another picture, changing the wallpaper through a theme, or clearing the wallpaper stops animation. Wallpaper-colour matching in Settings uses the extracted still image.

X11 uses input-transparent, override-redirect DESKTOP windows, below widgets, panels and applications. They do not appear in the tiling/window list. Wayland uses mpvpaper's background layer surfaces; it requires rdwm's output names/geometry in `get_state` (added with this feature). No web engine or Wallpaper Engine scene runtime is embedded.

## Online source

[DesktopHut](https://www.desktophut.com/) is the default curated live-wallpaper source. Search by keyword, or leave the search empty to browse recent uploads, then select a thumbnail. A muted video preview plays inside Settings; **Use wallpaper** separately downloads and applies the original video. Decebal reads the public gallery and each wallpaper’s structured VideoObject metadata to fetch the original MP4 rather than its short preview. No account or API key is required. Source terms remain with the wallpaper; these files are not assumed to have an open licence. Use the source button to check details. The site’s HTML can change; failures leave the existing wallpaper unchanged.

DesktopHut results use a compact thumbnail grid with deduplication across pages. Pages are capped at 48 cards; poster fetching has a bounded time budget. Selecting a card fetches only its short preview clip (up to 16 MB), then runs one muted 480×270, 12 fps decoder with a two-frame queue. Closing the preview, changing source, leaving the tab, applying a wallpaper or closing Settings stops and reaps it. Previewing does not change the desktop. Playback/local-file and colour-matching controls sit below the gallery. Only HTTPS source pages, images and original MP4 files on `www.desktophut.com` are accepted; redirects are rejected. Responses and video downloads are bounded (512 MB for videos). The existing still-frame/fullscreen pause and muted playback behavior applies to both providers.

[Wikimedia Commons](https://commons.wikimedia.org/wiki/Commons:Video) provides videos without an API key. Search uses the official [MediaWiki videoinfo API](https://www.mediawiki.org/wiki/Extension:TimedMediaHandler/API). It is a general media library, not a curated rice/wallpaper catalogue; refine searches such as `cloud timelapse`, `waterfall` or `abstract animation`, and check the source page. This picker does not promise Wallhaven-style content filtering.

The picker shows thumbnails, dimensions, download size, author and licence, with a source-page button. Search pages contain at most 12 items and searches are separated by at least two seconds. Downloads occur only on explicit selection, from Wikimedia media hosts, with bounded responses, timeouts and a 512 MB video limit. Search results above 7680×4320 are excluded. Creator/licence/source metadata is saved in a `.credit.json` sidecar for offline attribution. Cached thumbnails are separate from the video library. Videos are downloaded before playback, not streamed in the background.

## CLI

```sh
decwall set /path/to/loop.webm
decwall status                         # config plus player states and errors
decwall off                            # keep the still image
decwall desktophut search cyberpunk
decwall desktophut download https://www.desktophut.com/PAGE-SLUG
decwall search clouds timelapse
decwall download 81916645              # Commons page id returned by search
# Set the returned local path with decwall set.
# Same commands through the theme CLI:
decthemectl wallpaper live set /path/to/loop.webm
```

Configuration: `$XDG_CONFIG_HOME/decebal/live-wallpaper.json`. Videos and attribution: `$XDG_DATA_HOME/decebal/live-wallpapers/`. Posters/thumbnails: `$XDG_CACHE_HOME/decebal/live-wallpapers/`. Runtime/player status: `$XDG_RUNTIME_DIR/decwall-status.json`. Paths fall back to the normal home folders when XDG variables are unset. Playback errors appear in the Settings tab and in CLI status. Re-select a video to retry a failed player.

## Validation

`cargo test --lib livewall` covers provider-result validation and fullscreen visibility across outputs/workspaces/minimisation. `python3 tests/e2e/live-wallpaper.py` uses an automatically allocated private Xvfb display, a private HOME/runtime/config, real rdwm and real mpv. It checks playback progress, frozen frames, fullscreen/workspace resume, desktop-window management, static selection cleanup and invalid input. Build rdwm, rdwmctl and the xclient example first; override `RDWM_BIN_DIR`/`MS_BIN_DIR` for alternate build paths. It never installs or touches the current desktop.

The X11 path has runtime coverage. The mpvpaper/Wayland path is implemented but still requires a real-session validation with mpvpaper installed; it is not claimed as verified on hardware.

For an explicit online DesktopHut integration check, run the same test with `DESKTOPHUT_NETWORK_SMOKE=1`. It additionally searches the real provider through Settings and checks result cards and cached thumbnails; normal test runs stay offline. CLI source fetching can be checked separately with `decwall desktophut search nature` and `decwall desktophut download PAGE_URL` under private XDG paths.
