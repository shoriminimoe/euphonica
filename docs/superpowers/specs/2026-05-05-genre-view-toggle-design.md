# Genre Content View — Albums / Artists toggle — design

**Date:** 2026-05-05
**Status:** approved (pending implementation)
**Depends on:** `docs/superpowers/specs/2026-04-30-genres-view-design.md` (the Genres view feature this extends).

## Goal

Add a linked-toggle pair to `GenreContentView` (the per-genre detail page) that switches between an album grid and an artist grid for the current genre. Albums is the default. Visual treatment matches the Discography / All Songs toggle in `ArtistContentView`.

## User-visible behaviour

- New toggle row sits below the genre-name header. Two pill buttons in a `linked` `GtkBox`: **Albums** (`library-music-symbolic` icon, default-active) and **Artists** (`music-artist-symbolic` icon). A single count label sits next to them and reflects the active subview's item count.
- Clicking **Artists** swaps the album grid out for an artist grid populated with the same `ArtistCell` widget used by the main `ArtistView` (large avatar + name).
- Clicking an artist tile pushes the existing `ArtistContentView` via `goto_artist` — same flow as clicking an artist anywhere else.
- Clicking an album tile (in the Albums subview) pushes the existing `AlbumContentView` via `goto_album` — unchanged from current behaviour.
- The toggle's last-selected state persists in-memory on `GenreContentView` itself, so navigating between genres in one session preserves the choice. App restart resets to Albums (the in-memory state lives only as long as the widget instance).

### Artist membership rule

An artist appears under genre G if any of their songs has G in its post-split `SongInfo.genres`. Same shape as the album rule: server-side `(genre contains G)` filter narrows candidates; client-side `song.genres.contains(&genre)` verification drops substring false positives like Rock/Rock & Roll; dedup by `artist.get_comp_id()`.

## Architecture

### New library method `Library::get_artists_by_genre`

Mirrors `Library::get_albums_by_genre` exactly, but iterates each surviving song's `artists` Vec (one song can credit multiple artists) and dedupes by `artist.get_comp_id().to_owned()`. Lives in `src/library/controller.rs` immediately below `get_albums_by_genre`. Signature:

```rust
pub async fn get_artists_by_genre<FA>(
    &self,
    genre: String,
    mut respond_artist: FA,
) -> ClientResult<()>
where
    FA: FnMut(Artist),
```

### UI template refactor — `genre-content-view.ui`

Current structure:

```
AdwToolbarView
├── top: AdwHeaderBar
│     └── start: GtkBox (genre_name + album_count)
└── content: ContentStack
              └── GtkScrolledWindow → GtkGridView (album_grid)
```

New structure:

```
AdwToolbarView
├── top: AdwHeaderBar
│     └── start: GtkLabel (genre_name)            ← album_count label removed
├── top: GtkBox (centered)                         ← new toolbar row
│     ├── linked toggle: albums_btn (active by default), artists_btn
│     └── GtkLabel (count) — bound to active subview's n-items
└── content: GtkStack (subview_stack)
              ├── page "albums": existing ContentStack with album_grid
              └── page "artists": new ContentStack with artist_grid
```

Toggle wiring (mirrors `all_songs_btn` in artist-content-view):

- `artists_btn` has `group="albums_btn"` so they're radio-grouped.
- `subview_stack.visible-child-name` is bound to a property expression on the toggle group: when `albums_btn.active` → `"albums"`, else `"artists"`. (In practice this is `albums_btn.bind_property("active", &subview_stack, "visible-child-name").transform_to(|_, a: bool| Some(if a { "albums" } else { "artists" }))` — simplest form using the existing pattern from `ArtistContentView`.)
- `count` label binds to `n-items` of whichever ListStore is active, transformed to a localized "N albums" / "N artists" string. Implementation: a property binding on each ListStore, with the label updated when the active subview changes (cheapest is to listen to both `n-items` properties and read whichever subview is active in the closure).

### `GenreContentView` Rust changes

Add to `imp::GenreContentView`:

```rust
#[template_child] pub albums_btn: TemplateChild<gtk::ToggleButton>,
#[template_child] pub artists_btn: TemplateChild<gtk::ToggleButton>,
#[template_child] pub subview_stack: TemplateChild<gtk::Stack>,
#[template_child] pub artists_stack: TemplateChild<ContentStack>,  // ContentStack for the artists page
#[template_child] pub artist_grid: TemplateChild<gtk::GridView>,
#[template_child] pub count: TemplateChild<gtk::Label>,

#[derivative(Default(value = "gio::ListStore::new::<Artist>()"))]
pub artist_list: gio::ListStore,
pub artists_initialized: Cell<bool>,        // resets on unbind
pub last_subview_albums: Cell<bool>,        // PERSISTS across unbind, default true
```

Note: the existing `album_count` label template child is removed and `bind_property("n-items", &album_count, "label")` in `constructed()` is replaced with the new `count`-label logic that handles both subviews.

In `setup`:
- Wire the artist grid's factory exactly like `ArtistView::setup_gridview` does (avatar binding + cache signal subscription via `ArtistCell::new(item, cache, false)`). Duplicate ~30 lines of factory-setup code rather than refactoring `ArtistView` mid-flight.
- Wire `albums_btn` ↔ `subview_stack.visible-child-name` binding.
- Wire `albums_btn.connect_toggled` to update `last_subview_albums` and to lazy-trigger artist fetch (see below).
- On artist grid activation, emit a new `artist-clicked` signal carrying the `Artist` (mirrors `album-clicked`).

In `bind(&Genre)`:
- Existing album population continues unchanged (always populates on bind — albums is the default and visible page).
- Restore the toggle state: `albums_btn.set_active(self.imp().last_subview_albums.get())` (or `artists_btn.set_active(!...)`). If `last_subview_albums` is false on bind, the toggle handler will fire and lazy-populate artists.
- Lazy artist population: if `artists_btn.is_active() && !artists_initialized.get()`, kick off `library.get_artists_by_genre(...)` via `glib::spawn_future_local`, mirroring the album-fetch shape.

In `unbind()`:
- Clear `album_list` (existing) **and** `artist_list`.
- Reset `artists_initialized` to false (forces re-fetch on next bind).
- Do NOT reset `last_subview_albums` — that's the persistence the user wants.
- Reset both ContentStacks to placeholder.

The toggle-button toggled handler (added in `setup`) does:
1. Updates `last_subview_albums` from the new state.
2. If switching to artists and `!artists_initialized.get()`, spawns the artist-fetch future and sets `artists_initialized = true`. Shows spinner during fetch via `artists_stack.show_spinner()`.
3. The active stack page transition is handled automatically by the property binding.

### Window-level wiring

`src/window.rs`'s existing handler block has:

```rust
win.imp().genre_view.get_content_view().connect_closure(
    "album-clicked",
    false,
    closure_local!(...this.goto_album(&album)...),
);
```

Add a parallel handler for `artist-clicked`:

```rust
win.imp().genre_view.get_content_view().connect_closure(
    "artist-clicked",
    false,
    closure_local!(
        #[watch(rename_to = this)]
        win,
        move |_: GenreContentView, artist: Artist| {
            this.goto_artist(&artist);
        }
    ),
);
```

`Artist` is already in the `crate::common::{...}` import block.

## Files

### Modified files

| Path | Change |
|---|---|
| `src/library/controller.rs` | Add `pub async fn get_artists_by_genre` immediately after `get_albums_by_genre`. |
| `src/gtk/library/genre-content-view.ui` | Header refactor: remove `album_count` from header bar; add toolbar row with linked toggle + count label; wrap existing album grid in a `GtkStack` with two named pages; new `artist_grid` page mirrors the album page's structure. |
| `src/library/genre_content_view.rs` | New template children, artist ListStore, lazy-populate logic, toggle handler, persistence cell, `artist-clicked` signal, factory wiring. Remove the existing `album_count` binding (replaced by subview-aware `count` label). |
| `src/window.rs` | Add `artist-clicked` signal handler on `genre_view.get_content_view()` calling `goto_artist`. |

### New files

None.

## Edge cases

- **Genre with zero albums but some artists, or zero artists but some albums** — each subview's `ContentStack` independently switches between spinner / content / placeholder based on its own ListStore. The count label updates appropriately when toggling.
- **Switch to Artists while still loading albums** — independent operations; both fetches are spawned via `glib::spawn_future_local` and don't block each other.
- **`unbind()` while artist fetch is in flight** — the spawned future captures references via the existing weak-reference pattern; if the widget is unbound (and the closures find a stale state), they no-op. Same approach the album fetch already takes.
- **Re-bind to same genre** — `unbind()` clears both lists and resets `artists_initialized`; `bind()` re-populates albums; artists re-populate on next toggle. This is the same cost-model as the album-only behaviour today.
- **Artist with no MusicBrainz ID** — falls back to comparing by name (handled by `Artist::get_comp_id()`'s existing logic). Same as `get_artist_content`.
- **Long genre name + long count label overflow** — both labels live in a centered toolbar row; if either overflows, GTK wraps. Acceptable for v1.

## Manual test plan

The codebase has no automated test harness; verification is manual via Flatpak.

1. **Default state on entering a genre** — Albums toggle is active; album grid populates as it does today.
2. **Toggle to Artists** — first toggle shows spinner briefly, then artists appear. Each artist has avatar + name. Subsequent toggles between Albums/Artists are instant (no re-fetch).
3. **Click an artist tile** — sidebar switches to Artists, the existing `ArtistContentView` opens scoped to that artist.
4. **Membership rule check** — pick a genre G. The artists list should contain every artist whose tracks have G (including ones whose AlbumArtist isn't G, since the rule is per-track).
5. **Persistence across genres in one session** — set the toggle to Artists in genre G, navigate back, click genre H → Artists is still active for H (and H's artists populate).
6. **App restart** — close and relaunch; click any genre → Albums is active (in-memory state is gone).
7. **Genre with no artists** — placeholder shown when toggled to Artists; toggling back to Albums works normally.
8. **`unbind` cleanup** — navigate from genre G (with artists loaded) to genre H, then back to G → artist fetch re-runs (since `artists_initialized` was cleared on unbind). Result is identical to first visit.

## Out of scope

- Cross-session persistence of the toggle state (would require GSettings; user picked in-memory only).
- Sorting or filtering within either subview (existing behaviour: insertion order from MPD; no per-subview search).
- A combined Albums+Artists view.
- Refactoring the duplicated `ArtistCell` factory code into a shared helper. The genres-view artist factory will duplicate the ~30 lines from `ArtistView::setup_gridview` rather than introduce an indirection mid-feature. If the factory drift becomes a problem later, a dedicated refactor is the right vehicle, not this feature.
