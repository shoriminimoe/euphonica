# Album Artists sidebar entry — design

**Date:** 2026-05-06
**Status:** approved (pending implementation)

## Goal

Add a new top-level sidebar entry **Album Artists** that lists artists derived from the MPD `AlbumArtist` tag, parallel to the existing **Artists** entry which uses the `Artist` tag. Clicking a tile pushes the existing `ArtistContentView`, scoped to AlbumArtist queries (Discography lists albums where this is the AlbumArtist; All Songs lists every track on those albums).

## User-visible behaviour

- New sidebar entry **Album Artists** between the existing **Artists** and **Genres** entries. Icon reuses `music-artist-symbolic`; the label disambiguates from "Artists".
- Clicking it opens a grid of artists derived from MPD's `albumartist` tag. The grid renders with the same `ArtistCell` widget used by the Artists view (avatar + name).
- Clicking a tile pushes the same `ArtistContentView` widget the existing Artists view uses, but the underlying queries pull data filtered by `AlbumArtist` instead of `Artist`. Discography shows the albums for which this artist is listed as AlbumArtist; All Songs shows every track on those albums.
- Search and asc/desc sort behave identically to the existing Artists view.

## Architecture

A small `ArtistKind` enum (defined in `src/library/mod.rs` and re-exported from there) drives parameterization of the existing widgets:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArtistKind {
    Artist,        // uses Library.artists, init_artists, get_artist_content
    AlbumArtist,   // uses Library.album_artists, init_album_artists, get_album_artist_content
}
```

The same `ArtistView` and `ArtistContentView` widget classes are instantiated twice in the window — once with `ArtistKind::Artist`, once with `ArtistKind::AlbumArtist`. Each instance stores its kind once in `setup()` and dispatches on it for data fetches. No new top-level widget types are introduced.

### Library controller

Two parallel `ListStore<Artist>` fields and matching init flags:

```rust
// imp::Library
pub artists: gio::ListStore,                  // existing
pub artists_initialized: Cell<bool>,           // existing

pub album_artists: gio::ListStore,             // NEW
pub album_artists_initialized: Cell<bool>,    // NEW
```

API changes on `Library`:

- `init_artists` drops its `use_album_artist: bool` parameter. It now only populates `Library.artists` from MPD's `Artist` tag.
- New `init_album_artists()` populates `Library.album_artists` from MPD's `AlbumArtist` tag (calls the existing `client.get_artists(use_album_artist=true, ...)` under the hood).
- New `album_artists()` getter mirrors the existing `artists()` getter.
- New `get_album_artist_content` mirrors `get_artist_content` shape:
  - Server-side: `Term::Tag(ALBUMARTIST), QueryOperation::Contains, artist.name`.
  - Client-side: filter songs where `s.album.as_ref().map(|a| a.artists.iter().any(|ai| ai.get_comp_id() == comp_id))` is true. (We compare against the album's parsed `artists` Vec, which honours the existing artist-tag-delimiter splitting.)
  - Album dedup by `album.get_comp_id()`, song response unchanged.
- `Library::clear` resets both ListStores and both init flags.

### Widget parameterization

`ArtistView::setup` gains a `kind: ArtistKind` parameter. Stored on `imp::ArtistView` as a `Cell<ArtistKind>` (defaults to `Artist`).

- `LazyInit::populate` reads kind and calls either `library.init_artists()` or `library.init_album_artists()`.
- `setup_gridview` reads kind and binds the grid model to `library.artists()` or `library.album_artists()`.
- `on_artist_clicked` passes kind down to `content_view.bind(artist)`. Since `ArtistContentView` is a template_child of `ArtistView`, each `ArtistView` instance owns its own `ArtistContentView` instance — so storing the kind on `ArtistContentView::imp` once at setup is sufficient (no need to thread it through every `bind` call).

`ArtistContentView::setup` gains the same `kind: ArtistKind` parameter, stored on `imp::ArtistContentView`. The async closure inside `bind()` reads the kind and calls `library.get_artist_content` or `library.get_album_artist_content` accordingly.

### Window integration

- `EuphonicaWindow::imp` gains `album_artist_view: TemplateChild<ArtistView>` — same widget class, distinct instance.
- `window.ui` adds a new `<GtkStackPage>` named `album_artists` between the artists page and the folders page, containing `<EuphonicaArtistView id="album_artist_view"/>`.
- The breakpoint setter block adds a `<setter object="album_artist_view" property="collapsed">true</setter>` line.
- `window.rs::setup` calls `album_artist_view.setup(library, cache, &win, ArtistKind::AlbumArtist)`. The existing `artist_view.setup` call gains an `ArtistKind::Artist` argument.
- `maybe_populate_visible` gets a new `"album_artists" => imp.album_artist_view.populate()` arm. **This was the bug missed in the original genres-view rollout — must remember.**
- The show-sidebar Widget array gets `self.album_artist_view.upcast_ref::<gtk::Widget>()`.
- The existing `artist_view.get_content_view().connect_closure("album-clicked", …)` block gets a parallel block for `album_artist_view.get_content_view()` routing to `goto_album`.

### Sidebar

- `gtk/sidebar.ui` adds a new `<EuphonicaSidebarButton id="album_artists_btn">` between `artists_btn` and `folders_btn`. Label "Album Artists", `icon_name="music-artist-symbolic"`, `group="recent_btn"`.
- `sidebar.rs::imp` gains `album_artists_btn: TemplateChild<SidebarButton>`.
- `sidebar.rs::setup` adds a `connect_toggled` handler routing to stack name `"album_artists"`, plus an entry in the show-sidebar click-loop array.
- `Sidebar::set_view` gets a new `"album_artists" => self.imp().album_artists_btn.set_active(true)` arm, mirroring the pattern used by the genres-view-toggle feature.

### State persistence

The existing `state.artistview` schema (one key: `sort-direction`) is shared between both kinds. Sort preference applies uniformly across the two views. No new gschema entries; the schema is unchanged.

## Edge cases

- **Library with no AlbumArtist tags at all** — `list albumartist` returns nothing, the grid is empty, `ContentStack` shows the existing "No Artists" placeholder. The user has to retag for content to appear.
- **Multi-value AlbumArtist tags** (e.g. `"Simon & Garfunkel"`) — parsed by the existing `parse_mb_artist_tag` infrastructure: each split entry becomes a separate `Artist` GObject, each with its own tile, each linking back to the album via the album's `artists` Vec. If the tag is in `artist-tag-delim-exceptions`, it stays together as one tile. Same rules as the existing Artists view.
- **An artist who is both an Artist and an AlbumArtist** — appears in both views as distinct GObject instances. Clicking in either view enters the correctly-scoped content view.
- **`Library::clear` on reconnect** — both ListStores cleared, both init flags reset, both views re-fetch on next navigation.
- **Avatar fetch / cache subscription** — `ArtistCell` already handles avatar binding via cache signals; the same factory works for both views without changes.
- **Persisting last-visited tab** — out of scope for this feature. The current "Recent" tab is the default landing on app start, and that doesn't change.

## Files

**Modified files only.** No new files — the implementation is parameterization of existing widgets.

| Path | Change |
|---|---|
| `src/library/mod.rs` | Add and re-export `ArtistKind` enum. |
| `src/library/controller.rs` | Add `album_artists` ListStore + init flag. Drop `use_album_artist` flag from `init_artists`. Add `init_album_artists()`. Add `album_artists()` getter. Extend `clear()`. Add `get_album_artist_content`. |
| `src/library/artist_view.rs` | Add `kind: Cell<ArtistKind>` template-imp field. `setup` takes a `kind` parameter and stores it. `populate` / `setup_gridview` / `on_artist_clicked` dispatch on kind. |
| `src/library/artist_content_view.rs` | Add `kind: Cell<ArtistKind>` field. `setup` takes a `kind` parameter and stores it. The bind() async closure dispatches on kind to the right Library method. |
| `src/window.rs` | New `album_artist_view: TemplateChild<ArtistView>`. Setup call, signal handlers, maybe_populate_visible arm, show-sidebar array entry. |
| `src/window.ui` | New stack page; new collapsed setter. |
| `src/sidebar/sidebar.rs` | New `album_artists_btn` template child + handler + set_view arm + show-sidebar array entry. |
| `src/gtk/sidebar.ui` | New `EuphonicaSidebarButton id="album_artists_btn"`. |

## Manual test plan

The codebase has no automated test harness; verification is manual via Flatpak.

1. **Default state** — sidebar shows the new "Album Artists" entry between Artists and Folders. Click it → grid populates with artists derived from MPD's AlbumArtist tag.
2. **Distinct from Artists** — pick a library where some tracks have an Artist that's NOT also an AlbumArtist (e.g. featured artist). They appear in **Artists** but not **Album Artists**. Conversely, pick an album where AlbumArtist is set but the per-track Artist tag differs (rare but possible) → AlbumArtist appears in the new view.
3. **Click an album-artist tile** — pushes ArtistContentView; the Discography tab shows that artist's albums (where they're the AlbumArtist), the All Songs tab shows every song on those albums.
4. **Click an album in the discography** — sidebar switches to Albums, AlbumContentView opens for the picked album (existing `goto_album` flow, unchanged).
5. **Membership rule check** — pick an artist who is the AlbumArtist on album X. Confirm album X appears in their AlbumArtist discography even if their Artist-tag presence is incidental on individual tracks.
6. **Refresh path** — F5 (or disconnect/reconnect MPD). Both Artists and Album Artists views reload.
7. **Sidebar toggle group** — clicking Album Artists deselects Artists (radio-grouped behaviour), and the show-sidebar action collapses the sidebar correctly when activated from this entry.
8. **`maybe_populate_visible` regression check** — start the app fresh; navigate directly to Album Artists by clicking the sidebar entry as the first action. Grid populates (this validates the new arm in the dispatch).

## Out of scope

- A separate state schema for the Album Artists view; sort direction is shared with Artists.
- Cross-view links from an Album Artist tile back to the corresponding Artists-view entry (or vice versa). The two views are independent destinations.
- Filtering AlbumArtists by genre or some other dimension. The existing search bar covers name search.
- Bio/avatar fetch differing by kind. The cache layer is keyed on artist name; an artist's avatar is the same regardless of which view shows it.
