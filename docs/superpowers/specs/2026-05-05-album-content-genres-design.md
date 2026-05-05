# Genre buttons on AlbumContentView — design

**Date:** 2026-05-05
**Status:** approved (pending implementation)
**Depends on:** `docs/superpowers/specs/2026-04-30-genres-view-design.md` (the Genres view feature; this feature consumes its `goto_genre`-equivalent flow and the per-song `genres` field).

## Goal

Display the genres of an album as clickable pill buttons on the `AlbumContentView`, below the existing metadata row (Originally released / Tracks / Runtime). Clicking a pill navigates to that genre's album list — analogous to how clicking the AlbumArtist pill navigates to the artist's view.

## User-visible behaviour

- A new horizontal wrap of pill buttons sits between the metadata block and the album-wiki infobox in `AlbumContentView`.
- Each pill is the atomic name of a genre that appears on at least one track of the album (post-split).
- Clicking a pill switches the sidebar's active section to Genres and pushes the per-genre album list, same way clicking the AlbumArtist pill switches to Artists.
- If the album has no genres, the entire row is hidden (no empty space).

### Genre source rule

For each track in the album, take the post-split `SongInfo.genres` list (already populated by the existing splitter logic from the Genres view feature). The album's displayed genres are the **union** across all tracks, deduplicated. This matches the membership rule used by `Library::get_albums_by_genre`, so the album → genre → album-list round-trip is consistent: the album is guaranteed to be in the resulting genre's album list.

### Visual

Pill button. Same overall shape as `ArtistTag` (a `gtk::Button` subclass with rounded styling), minus the avatar. Sits in an `AdwWrapBox` so multi-genre albums wrap cleanly across lines.

## Architecture

A new `GenreTag` widget at `src/library/genre_tag.rs` (with `src/gtk/library/genre-tag.ui`) is the per-pill button. It mirrors `ArtistTag`:

- Subclasses `gtk::Button`.
- Holds a `Genre` GObject (constructed fresh via `Genre::new(name)`).
- On `connect_clicked`, calls `window.goto_genre(genre)`.
- Has a single `gtk::Label` child showing the genre name. No avatar, no cache subscription.

A new method `EuphonicaWindow::goto_genre(&self, genre: &Genre)` mirrors `goto_artist`:

```rust
pub fn goto_genre(&self, genre: &Genre) {
    self.imp().genre_view.on_genre_clicked(genre);
    self.imp().sidebar.set_view("genres");
    if self.imp().split_view.shows_sidebar() {
        self.imp().split_view.set_show_sidebar(!self.imp().split_view.is_collapsed());
    }
}
```

The Genres view feature deliberately omitted the `"genres"` arm from `Sidebar::set_view` because nothing cross-navigated to Genres at that time. This feature requires the arm to be added: `"genres" => self.imp().genres_btn.set_active(true)`.

### Population timing

Albums don't carry genre data — `AlbumInfo` has no `genres` field. The data lives on `SongInfo.genres`, populated when each song's tags are parsed. So the genre row can only be populated after the album's track list has been fetched.

The existing `AlbumContentView::bind()` already runs an async closure post-fetch that calculates the runtime by iterating the loaded `song_list`. Genre population hooks into the same closure: after song fetch returns, iterate the song list, dedupe genres into an `FxHashSet`, instantiate one `GenreTag` per unique genre, append to the wrap box, and set the box visible iff the set is non-empty.

If the song fetch fails (existing error path: `dbg!(e)`), the genre row stays hidden alongside the empty track list — same failure mode as the existing runtime label.

## Files

### New files

| Path | Purpose |
|---|---|
| `src/library/genre_tag.rs` | `GenreTag` GObject. Constructed with `(name, window)`. Click calls `window.goto_genre(...)`. |
| `src/gtk/library/genre-tag.ui` | Template — `gtk::Button` parent with a `gtk::Label` child styled as a pill (matching artist tag's chip styling minus the avatar slot). |

### Modified files

| Path | Change |
|---|---|
| `src/gtk/library/album-content-view.ui` | Insert a new `AdwWrapBox` named `genres_box` between the existing `metadata_box` and the `infobox_spinner`. Initial `visible="false"`. |
| `src/library/album_content_view.rs` | (a) Add `genres_box` template child + `genres_tags: gio::ListStore<GenreTag>` field. (b) Inside the post-fetch async closure, populate genre tags from the song list's union, set `genres_box.set_visible(!empty)`. (c) Inside `unbind()`, clear `genres_box` mirroring how `artists_box` is cleared. |
| `src/library/mod.rs` | Add `mod genre_tag;` and `pub use genre_tag::GenreTag;` (or `use ...::GenreTag;` if only used internally — `pub use` is the safer default to mirror the artist_tag pattern). |
| `src/window.rs` | Add `pub fn goto_genre(&self, genre: &Genre)` mirroring `goto_artist`. Requires `Genre` already in the existing `crate::common::{...}` import block. |
| `src/sidebar/sidebar.rs` | Add `"genres" => self.imp().genres_btn.set_active(true),` arm to the `set_view` match. |
| `src/euphonica.gresource.xml` | Register `gtk/library/genre-tag.ui` alongside the other `gtk/library/*.ui` entries. |

## Edge cases

- **Album with no genre tags on any track** → `genres_box.set_visible(false)`. No empty row.
- **Songs not yet loaded (spinner state)** → `genres_box` is initially hidden; only populated inside the post-fetch closure. If fetch fails, stays hidden.
- **`unbind()` cleanup** → must remove all `GenreTag` widgets from `genres_box` (mirroring the existing `artists_box` cleanup at the end of `unbind`). `genres_box` should also be reset to `visible=false`.
- **Same genre on many tracks** → deduplicated by `FxHashSet`. One pill per atomic genre.
- **Genre that doesn't appear in `library.genres()` ListStore** → can't happen if the album's tracks are in MPD's database (the Genres view's `init_genres` runs `list genre` over the same database). If somehow it does happen (e.g. race during library refresh), `genre_view.on_genre_clicked(...)` will still push the content page; `get_albums_by_genre` will be called, and at least the source album will appear there.
- **Long genre names that wrap awkwardly** → the AdwWrapBox handles wrapping. No manual ellipsizing in v1.

## Manual test plan

The codebase has no automated test harness. Verification is manual via Flatpak.

1. **Album with single-value comma genre** (e.g. `Rock, Pop`) → both pills visible, dedup, click each → correct genre album list.
2. **Album with multi-value genre tags** (separate Genre lines per track) → each appears as a pill.
3. **Album where genres differ across tracks** → union: every distinct genre across the track list shows up.
4. **Album with no genres** → row hidden, no empty space.
5. **Click a genre pill** → sidebar switches to Genres, the per-genre album list opens, source album is in it.
6. **Bind / unbind / rebind cycle** → no leftover pills from a previous album appear when binding a new one.

## Out of scope

- Sorting genre pills (alphabetical, by frequency, etc.). v1 just appends in iteration order.
- Limiting pill count for albums with absurd numbers of genres. v1 shows all.
- Avatar/icon on the pill. Genres have no canonical imagery; if added later, it'd need a separate decision.
