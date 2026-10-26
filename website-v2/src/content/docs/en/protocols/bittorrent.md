---
title: BitTorrent
description: Magnet links and .torrent files with DHT, UPnP, built-in trackers and file selection.
section: protocols
order: 2
---

FluxDown has a BitTorrent engine built in (powered by librqbit) — you don't need a separate torrent client. It supports both magnet links and `.torrent` files, with DHT and UPnP enabled by default, a curated list of public trackers, and per-file selection before a multi-file torrent starts downloading.

## Adding a torrent

- **Magnet links** — paste a `magnet:` link into the New Download dialog exactly like an HTTP URL.
- **.torrent files** — open one from the New Download dialog's file picker, or associate `.torrent` files with FluxDown in *Settings → General → Associate .torrent Files*. Once associated, double-clicking a `.torrent` file anywhere on your system opens FluxDown and starts the download directly. (Windows only; this writes a per-user registry association and doesn't require administrator rights.)

<!-- TODO(screenshot): Settings → General, "Associate .torrent Files" toggle -->

## Selecting files

Multi-file torrents show a checklist of every file inside, with size, before the download starts — uncheck anything you don't want and FluxDown only fetches the files you selected.

<!-- TODO(screenshot): New Download dialog file-selection list for a multi-file torrent -->

## Existing task files

Open a BitTorrent task's details and select **Files**. The list shows each file's verified downloaded bytes. Change the checkboxes and choose **Apply selection** to add, stop or continue files. Unchecking retains existing data, including partial files; shared pieces may give unselected files some downloaded bytes.

Use **All files** and **Deselect all** to edit the selection; applying still requires at least one checked file. Multi-file torrents keep the torrent root directory even for a partial selection. A completed partial torrent can download additional files; this temporarily needs staging space for copies of its outputs. Legacy flat outputs are retained when a later completion adopts the root directory.

Resolved magnet metadata is saved with the task and reused after a restart or selection change. Cached metadata is checked against the magnet info-hash locally; peers are needed only for the first resolution. Until that first resolution, file controls are unavailable. During initial checking, an outstanding first-choice dialog or final completion, a change may be rejected as busy; finish the dialog or retry after that phase. After a restart, incomplete per-file progress remains unknown until local verification creates a live handle. The Files page is available in Web, GPUI desktop, Android and the full iOS flavor.

### Clean unselected downloaded data

Stop the task (and seeding), apply a selection containing at least one file, then choose **Clean unselected data** and confirm. This **permanently deletes unselected downloaded data, including previously completed outputs**; it cannot be undone. Files still selected, unrelated files and outputs claimed by other tasks are protected. Only paths recorded for this task are considered; directories are never recursively removed.

The host checks the confirmed selection again and rejects cleanup if it changed or any task writes are still active. Shared-piece boundary bytes needed by selected files remain in sparse staging or the `.parts` sidecar; cleanup therefore may retain some bytes and does not promise a particular amount of reclaimed disk space. A failure is reported and may be retried. Selecting a cleaned file later downloads its missing data again; availability depends on the torrent's peers. Cleanup never starts automatically when you uncheck a file.

## DHT, UPnP and listening port

Peer discovery works out of the box: **DHT** (trackerless peer discovery) and **UPnP port mapping** (automatic router port forwarding) are always enabled by the engine — there's nothing to configure. The only connectivity option exposed in *Settings → BitTorrent* is the **Listen Port Range** used for incoming BT connections (default **6881–6891**).

All BitTorrent tasks share one underlying session — one DHT routing table, one set of tracker connections, one listening port — rather than each task opening its own, which keeps resource usage low even with several torrents running at once.

## Trackers

FluxDown ships with **25 built-in public trackers**, curated for good global coverage: CN/Asia trackers first (for better peer locality if you're in mainland China or nearby), then international ones, with a few HTTPS fallbacks for networks that block UDP. These are used automatically for every torrent — including magnet links that don't specify any trackers of their own — unless you override them.

- **Custom trackers** — in *Settings → BitTorrent → Tracker List*, paste your own list (one URL per line) to replace the built-in set.
- **Tracker subscription** — *Settings → BitTorrent → Tracker Subscription* periodically fetches community-maintained tracker lists (`trackerslist.com` and `ngosang/trackerslist` by default, refreshed every 24 hours) and merges them in, deduplicated, alongside whatever tracker list you're using.

<!-- TODO(screenshot): Settings → BitTorrent, Tracker List and Tracker Subscription panels -->

## Resume support

Torrent downloads resume using BitTorrent's own piece-based verification: closing FluxDown, losing power, or a crash never loses already-verified pieces — only the piece that was in flight at the time needs to be re-fetched.

## Related settings

| Setting | Location |
|---|---|
| Listen Port Range | Settings → BitTorrent |
| Tracker List | Settings → BitTorrent |
| Tracker Subscription | Settings → BitTorrent |
| Associate .torrent Files | Settings → General |

## Limitations & FAQ

**A magnet link is stuck on "preparing".** Magnet links carry no file list or size up front — FluxDown has to resolve that metadata from DHT and/or peers first, which shows as "preparing" in the task list. If no peer or DHT node responds within 5 minutes, the task fails with an error telling you to check the trackers or your network (DHT is commonly blocked by restrictive firewalls or some ISPs).

**Can I add more trackers without losing the built-in ones?** Yes for community subscriptions — those merge with whatever list you're using. A hand-edited custom Tracker List, however, replaces the built-in 25 rather than adding to them; include the ones you want to keep.

**Does FluxDown seed after a torrent finishes?** FluxDown is built for downloading; it doesn't provide a dedicated seeding/ratio-management UI.
