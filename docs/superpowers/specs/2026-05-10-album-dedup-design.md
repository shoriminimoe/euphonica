# Album deduplication across MPD mounts

Status: design approved 2026-05-10. Branch: `feat/album-dedup`.

## Problem

When MPD mounts multiple storages that hold overlapping libraries (a NAS plus a local drive, a backup mount, etc.), the same album appears multiple times in Euphonica's views. Today's wrapper hides this only by accident: `get_albums_by_query` groups by `(album, albumartist)` via MPD's `list ... group` and then synthesizes a single `Album` from whichever song MPD returned first — the second copy is silently dropped, so the user has no way to choose between copies and no way to see that more than one exists.

We need to (1) detect duplicate copies deterministically, (2) show one canonical entry per album, and (3) let the user reach the other copies from the album detail view.

## Match key

A "copy" is identified by its **album-level folder URI** (`AlbumInfo.folder_uri`). Two copies are considered the same album iff:

- both have a non-empty `musicbrainz_albumid` and the values are equal, **OR**
- both lack an MBID and the pair `(album_title_casefold, albumartist_casefold)` is equal.

Mixed MBID-tagging state (one has MBID, the other doesn't) counts as **different** albums. The fallback comparison can't safely confirm a match without MBID and we'd rather show two entries than silently merge unrelated releases.

## Canonical-pick algorithm

Within a match group, pick the canonical copy by this strict ordering:

1. Highest `quality_grade` (existing enum: Hi-Res > Lossless > CD > Lossy > Unknown).
2. Earliest position in the user's mount-priority list. The mount of a `folder_uri` is the longest prefix that matches a known mount name; URIs not under any known mount rank after every listed mount.
3. Lexicographic `folder_uri` as a final stable tiebreaker, so the choice is deterministic across runs.

The non-canonical copies become **alternates** ordered by the same rank.

## Data model

### `common::album`

```rust
pub struct AlbumCopy {
    pub folder_uri: String,
    pub mount_name: Option<String>,   // None when the URI is not under any registered mount
    pub quality_grade: QualityGrade,
}

pub struct AlbumInfo {
    // ... existing fields unchanged ...
    pub mount_name: Option<String>,   // mount of the canonical copy
    pub alternates: Vec<AlbumCopy>,   // empty when no duplicate detected; ordered best-first
}
```

`alternates` is empty for any `AlbumInfo` not produced by the dedup pass (e.g. ones synthesized from a single `SongInfo` on the queue side). Existing call sites are unaffected.

`get_comp_id()` is unchanged. The canonical `AlbumInfo.folder_uri` continues to mean "this album's folder URI", so cover art lookups and song-fetch paths require no plumbing changes.

### New module `client::mounts`

```rust
pub struct Mount {
    pub name: String,
    pub storage: String,        // mirrors MPD listmounts
}

pub struct MountRegistry {
    known: Vec<Mount>,          // last seen via listmounts (+ inferred prefixes)
    priority: Vec<String>,      // ordered mount names from GSettings
}

impl MountRegistry {
    pub fn refresh(&mut self, client: &mut mpd::Client) -> ClientResult<()>;
    pub fn classify(&self, folder_uri: &str) -> Option<&str>;  // longest-prefix → mount name
    pub fn rank(&self, mount_name: Option<&str>) -> usize;     // lower = better; unknown → usize::MAX
}
```

Held inside `MpdWrapper` behind a `RefCell` (single-threaded GTK-side access). Refreshed on connect and on the `mount` idle subsystem.

### GSettings additions (under the `library` child)

| key | type | default | purpose |
|---|---|---|---|
| `dedup-albums` | `b` | `true` | master switch |
| `mount-priority` | `as` | `[]` | ordered list of mount names; mounts not in this list rank after listed ones |

These are additions; nothing existing is renamed and no migration is needed.

## Control flow

Dedup runs in `MpdWrapper::get_albums_by_query` — the single place that materializes `Album`s from `list ... group albumartist`. When `library.dedup-albums` is true:

```
mounts.refresh()                                            # once per call
for each (albumartist, album_title) tuple from `list album group albumartist`:
    fetch ALL songs of that album (find album=… albumartist=…)
    bucket the songs by parent folder_uri                   # candidate copies
    partition each copy by MBID:
        copies_with_mbid grouped by their MBID value
        copies_without_mbid grouped together as one fallback group
    each resulting partition is one match group
    within a match group:
        pick canonical copy by (quality_grade, mount priority, folder_uri)
        record other copies as AlbumCopy alternates on the canonical AlbumInfo
    emit one Album per match group
```

The "fetch all songs" step replaces today's `Window::from((0, 1))`. We cap with the existing `FETCH_LIMIT` and only need URI, MBID, and the format/bits/samplerate fields used to derive `quality_grade`. The richer fields (release_date, artists, etc.) still come from a single representative song per match group, exactly as today.

`get_recent_albums` already delegates to `get_albums_by_query`, so it inherits dedup automatically.

### Idle integration

`MpdWrapper`'s background thread already routes the MPD idle subsystems. Add a handler for `mount` that calls `mounts.refresh()` and then triggers the existing library-reload path (the same path used today when `database` fires).

### Settings reload

A `gio::Settings::changed` handler on `library.dedup-albums` and `library.mount-priority` clears the four `*_initialized` flags in `Library` and re-runs `init_albums` / `init_recent` / artist + genre album re-init paths. This mirrors the existing pattern used for delimiter automaton rebuilds.

### Performance

Today: N+1 round-trips (one `list` + N×`find` of one song). New: N+1 round-trips (one `list` + N×`find` returning all songs of one album). Same number of round-trips; the per-call payload grows in proportion to album size, bounded by `FETCH_LIMIT`. Acceptable.

## UI

### Preferences — new "Library sources" group on the existing `library` page

- **Switch** — *Deduplicate album copies*, bound to `library.dedup-albums`. Subtitle: *"Hide duplicate copies of the same album when more than one MPD mount holds it."*
- **Reorderable list** — *Source priority*, populated from `MountRegistry.known`, persisted to `library.mount-priority`. Each row shows the mount name as primary text and the MPD storage URI as subtitle. Drag handles for reordering.
- **Refresh button** on the group header re-runs `listmounts`.
- The list is greyed when the dedup switch is off.
- Empty-state copy: *"No mounts detected. The root storage is always considered."*

Implementation uses `AdwPreferencesGroup` + an `AdwActionRow` per mount with a drag handle, following the patterns already in `src/preferences/`.

### AlbumContentView — new "Source" header control

When the displayed `Album` has a non-empty `alternates`, the album content header gains a `GtkMenuButton` between the title block and the play/queue buttons:

```
Source: [▾ NAS · FLAC 24/96]
        ────────────────────
        ✓ NAS · FLAC 24/96      ← canonical (current)
          Local · MP3 320
          Old-Backup · FLAC 16/44
```

Label: `mount_name · quality_grade` for the active copy, falling back to the URI's first path segment if `mount_name` is `None`. Selecting an alternate:

1. Re-binds the content view to an `AlbumInfo` with the alternate's `folder_uri` (cover art lookup re-runs).
2. Re-fetches the song list with `find album=… albumartist=… AND base "<folder_uri>"` so only that copy's tracks are shown.
3. Subsequent **Play** / **Queue** / **Add to playlist** actions target songs from the chosen copy.

The selection is **session-local**: closing and reopening the album reverts to the canonical copy. Persisting per-album overrides is out of scope for v1.

### What we are not adding in v1

- **No badges in the album grid.** The grid stays clean; the "this album has multiple copies" affordance lives only in the detail view. We can revisit if it proves useful.
- **No global "show all copies" view mode.** The setting is binary: dedup on or off.

## Error handling and edge cases

- **`listmounts` fails or is unsupported.** Log a warning, leave `MountRegistry.known` empty, fall back to "infer mount from the first path segment of `folder_uri`". Dedup still works; mount-priority degenerates to first-seen ordering until a valid `listmounts` succeeds.
- **`library.mount-priority` references mounts that no longer exist.** Stale entries are ignored at rank-time (treated as unknown). They are pruned opportunistically when the user opens the Preferences pane — not aggressively, so transient mount disappearances don't lose the user's ordering.
- **The wider per-album `find` returns more than `FETCH_LIMIT` rows.** Truncate, log a warning, dedup over the truncated set. Same behavior as elsewhere in the wrapper.
- **`quality_grade` tie within the same mount.** Lexicographic `folder_uri` tiebreaker keeps the result deterministic.
- **Single-mount libraries.** No mount in the registry, every URI classified as `None`, `alternates` always empty, source picker never appears. Cost beyond status quo: one extra `listmounts` per library refresh.
- **Same album, mixed MBID-tagging state.** §"Match key" leaves these as different match groups so both stay visible. Intended safety behavior; user fixes by tagging.
- **Multi-disc albums spread across two folder_uris on the same mount with the same MBID.** Without mitigation we'd flag them as duplicates of each other, which is wrong. **Mitigation:** when bucketing, two folder_uris that are siblings under a common parent and whose songs' `disc` tags don't overlap are merged into a single copy (one folder_uri kept for navigation, the other folded in). This is documented as an approximation; the dedup-off setting is the escape hatch if it misfires.
- **Idle `mount` event arriving mid-listing.** `MpdWrapper` already serializes background tasks; the refresh runs after the in-flight listing finishes, then the standard reload path takes over.
- **Switching to an alternate copy while playback is happening.** The picker only changes what *future* play/queue actions target. The currently-playing track keeps playing from whatever copy queued it. No silent re-pointing.

## Test plan

The repo has no test or lint targets, so verification is hands-on. The implementation plan should treat each scenario below as an explicit checklist item.

1. **Two-mount synthetic setup.** Temporary MPD instance with two `mount`s pointing at copies of the same small album, one MBID-tagged, one not.
   - Dedup on, both MBID-tagged → one entry, picker shows two sources.
   - Dedup on, mixed MBID-tagging → two entries.
   - Dedup off → two entries regardless.
2. **Mount priority reorder.** Drag NAS above Local; confirm the canonical flips and the current view refreshes.
3. **Quality tiebreak.** Same MBID, FLAC on slow mount and MP3 on fast mount → FLAC wins as canonical.
4. **Multi-disc heuristic.** Album with `Disc 1/`, `Disc 2/` siblings → one entry, picker does **not** list the other disc as an alternate.
5. **Live mount add/remove via `mpc mount …`** while Euphonica is open → idle event triggers reload, picker contents update.
6. **Settings toggle at runtime.** Affected views (`AlbumView`, `ArtistContentView`, `GenreContentView`, `RecentView`) re-init.
7. **Single-mount library.** No regressions; source picker never appears.
