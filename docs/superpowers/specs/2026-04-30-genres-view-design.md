# Genres view — design

**Date:** 2026-04-30
**Status:** approved (pending implementation)

## Goal

Add a top-level **Genres** entry to the sidebar that browses the library by genre. The Genres list must show atomic genres, splitting compound single-value tags like `"Rock, Pop"` so that `"Rock"` and `"Pop"` appear as separate tiles.

The one explicit exception is when MPD itself returns a multi-value Genre response in which one of the values happens to also be compound (e.g. `["Jazz", "Rock, Pop"]`) — see Splitting rule below. This is rare and accepted.

## User-visible behaviour

- New sidebar entry **Genres**, between Artists and Folders.
- Clicking it shows a grid of genre tiles (plain text labels, no cover art).
- Clicking a genre tile pushes a sub-page showing the albums tagged with that genre.
- Clicking an album in that sub-page pushes the existing `AlbumContentView`, identical to clicking an album anywhere else in the app.
- Search by genre name + asc/desc sort (no other sort modes — genres only have one sortable field).

### Membership rule

An album appears under genre `G` if **any of its songs** has `G` in its (post-split) genre list. Single off-genre tracks promote the album under additional genres; this matches MPD's natural `list album genre==G` semantics and the listener's intuitive model.

### Splitting rule (hybrid)

For each set of `Genre` values MPD returns for a single song:

- `len() >= 2` — trust MPD: each value is its own genre, no further splitting.
- `len() == 1` — pass through the splitter; if a configured delimiter matches, the single value is broken up; if not, it is kept as-is.

This honours the user's stated preference exactly: never re-split a value MPD considered atomic; always split a single compound value.

## Architecture

Modelled directly on `ArtistView`. A new `GenreView` widget owns its own `adw::NavigationView` stack with two pages:

```
genre grid  →  genre content (album grid for one genre)
```

Clicking an album in `GenreContentView` follows the **same cross-view convention `ArtistContentView` already uses**: it emits an `album-clicked` signal which the window catches and routes through `EuphonicaWindow::goto_album`, which switches the sidebar to the Albums stack and pushes onto `AlbumView`'s existing `AlbumContentView`. We do not nest a second `AlbumContentView` instance inside `GenreContentView`.

### New files

| Path | Purpose |
|---|---|
| `src/common/genre.rs` | `Genre` GObject (name-only wrapper, modelled after `Artist` minus MBID/avatar fields). `parse_genre_tag(&str) -> Vec<&str>` consulting the genre automatons. **Important:** unlike `parse_mb_artist_tag`, the genre splitter must still run when the exceptions automaton is `None`, because the genre-exceptions default is `[]`. The artist version's outer `if let (Some(exc), Some(delim)) = ...` short-circuits in that case; the genre version restructures to gate only on the delimiter automaton, treating a missing exceptions automaton as "no exceptions". `parse_genre_values(&[String]) -> Vec<String>` applies the hybrid rule (split only when `values.len() == 1`). |
| `src/library/genre_cell.rs` + `src/gtk/library/genre-cell.ui` | Text-only tile for the genre grid (no `Cache` parameter — no cover loading). |
| `src/library/genre_view.rs` + `src/gtk/library/genre-view.ui` | Top-level grid widget. Implements `LazyInit::populate` → `library.init_genres()`. Owns search, sort, and the nav stack. |
| `src/library/genre_content_view.rs` + `src/gtk/library/genre-content-view.ui` | Per-genre album grid. Holds its own `gio::ListStore<Album>`. On `bind(genre)` calls `library.get_genre_albums(name)` to populate. Emits an `album-clicked` signal when a cell is activated; does **not** host its own `AlbumContentView`. |
| `src/gtk/icons/genre-symbolic.svg` | Sidebar icon for the new entry. |

### Modified files

| Path | Change |
|---|---|
| `data/io.github.htkhiem.Euphonica.gschema.xml` | Add `genre-tag-delims` (`as`, default `[",", ";", "/"]`) and `genre-tag-delim-exceptions` (`as`, default `[]`) under the existing `library` schema. Add a new `state.genreview` schema with one `sort-direction` enum key, referenced from the `state` schema's children. |
| `src/common/tags.rs` | Add `pub const GENRE: &str = "genre";`. |
| `src/common/mod.rs` | `pub mod genre;` and re-export `Genre`, `parse_genre_tag`, `parse_genre_values`. |
| `src/common/song.rs` | Add `pub genres: Vec<String>` to `SongInfo`. In the existing tag-iteration loop in `from_mpd`, collect all `("genre", val)` entries into a temp `Vec<String>`, then call `parse_genre_values` once after the loop and store the result. |
| `src/utils.rs` | Add `GENRE_DELIM_AUTOMATON` and `GENRE_DELIM_EXCEPTION_AUTOMATON` (Lazy `RwLock<Option<AhoCorasick>>`) plus `rebuild_genre_delim_automaton` / `rebuild_genre_delim_exception_automaton`, mirroring the existing artist counterparts. |
| `src/client/wrapper.rs` | Add `pub async fn get_genres(&self, respond: &mut F) where F: FnMut(Genre)` running `Task::List(Term::Tag("genre"), Query::new(), None, ...)`, splitting each returned value via `parse_genre_tag`, deduping, emitting `Genre` GObjects. |
| `src/library/controller.rs` | Add `genres: gio::ListStore`, `genres_initialized: Cell<bool>` to `imp::Library`. Add `genres()` getter. Extend `clear()` to reset both. Add `init_genres()`. Add `get_genre_albums(genre, respond)` (see algorithm below). |
| `src/library/mod.rs` | Declare and re-export the three new modules. |
| `src/sidebar/sidebar.rs` + `src/gtk/sidebar.ui` | Add `genres_btn: TemplateChild<SidebarButton>` between Artists and Folders, with the toggle handler routing to stack name `"genres"`. Include it in the show-sidebar click loop. (No change to `set_view` is needed — that match only handles entries that can be cross-navigated to from outside the sidebar; Genres is not such a destination today.) |
| `src/window.rs` + `src/window.ui` | Register `GenreView` as the `"genres"` page in the existing stack. Call `GenreView::setup` mirroring `AlbumView::setup`. Add an `album-clicked` signal handler on `GenreContentView` that calls the existing `goto_album()` (same shape as the `ArtistContentView` handler at `window.rs:1042`). |
| `src/preferences/library.rs` + `src/gtk/preferences/library.ui` | Add a "Genre tag delimiters" section mirroring the artist delimiters UI. On change, call the new `rebuild_genre_*` functions. Reuse the same restart-required wording the artist section uses. |
| `src/euphonica.gresource.xml` | Register the three new `.ui` files under the main prefix and the new SVG under the icons prefix. |

### `get_genre_albums` algorithm

Mirrors `get_artist_content`'s shape:

1. Build `Query::new().and_with_op(Term::Tag(tags::GENRE.into()), QueryOperation::Contains, genre.clone())` — substring filter narrows server-side, avoiding a full-library scan.
2. Stream `SongInfo`s via the existing batched `client.get_song_infos_by_query`.
3. **Verification:** for each song, check `song.genres.contains(&genre)` (post-split exact match). Drops false positives like `"Rock & Roll"` and `"Indie Rock"` when the user clicked `"Rock"`.
4. Bucket surviving songs by `album.get_comp_id()` into a `FxHashSet`. Emit one `Album` per unique album, built from the song's nested `AlbumInfo`.

The candidate `(genre contains G)` filter is the only practical way to avoid pulling every song in the library; the verification pass is mandatory because MPD's `contains` is substring-based.

## Edge cases

- Empty / whitespace-only genre values are skipped before splitting.
- Library with zero genres → existing `ContentStack` placeholder.
- Multi-value tag where one of the values is itself compound (e.g. MPD returns `["Jazz", "Rock, Pop"]`) → per the agreed rule we trust the server and do not re-split. `"Rock, Pop"` will appear as a single tile in this case. Documented as accepted behaviour; the workaround is for the user to retag the file using separate `Genre:` lines.
- Settings change to delimiters / exceptions: rebuild automatons immediately so future song-loads use the new rules. Already-loaded genre tiles will not reflect the new split until refresh / reconnect — same caveat the artist delimiter UI carries today.

## Refresh / disconnect handling

- `Library::clear()` resets the new `genres` ListStore and `genres_initialized` flag so reconnection re-fetches.
- The new view uses the existing `LazyInit` two-layer guard, so raising the window from background mode does not retrigger fetches.

## Manual test plan

The codebase has no automated test harness; verification is manual.

1. **Splitter behaviour** (one-off `dbg!` log on first song scan):
   - `["Rock"]` → `["Rock"]`
   - `["Rock, Pop"]` → `["Rock", "Pop"]`
   - `["Rock", "Pop"]` → `["Rock", "Pop"]`
   - `["Drum & Bass"]` → `["Drum & Bass"]` (no `&` in delims)
   - `["Rock; Pop; Jazz"]` → `["Rock", "Pop", "Jazz"]`
2. **Library scan**: tag test files with each variant; confirm the Genres view shows atomic items only.
3. **False-positive verification**: with both `Rock` and `Rock & Roll` genres in the library, click `Rock` and confirm only Rock-tagged albums appear (verification pass dropped substring matches).
4. **Refresh path**: disconnect / reconnect MPD; confirm genres reload.
5. **Preferences round-trip**: add `/` to delims, confirm `"AC/DC"` gets split; add `AC/DC` as exception, confirm it is preserved on next library scan.
