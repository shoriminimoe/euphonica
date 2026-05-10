# Album dedup implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect duplicate album copies across MPD storage mounts, show a single canonical entry per album in library views, and let the user reach the other copies from the album detail view.

**Architecture:** Dedup runs in `MpdWrapper::get_albums_by_query` after MPD's `list ... group albumartist`. For each `(albumartist, title)` tuple, the wrapper now fetches all songs of that album, buckets them by parent folder URI, partitions each bucket by MBID, picks a canonical copy by `(quality_grade, mount priority, folder_uri)`, and records the rest as alternates on the canonical `AlbumInfo`. A new `client::mounts::MountRegistry` owns mount classification and priority; mount changes are seen via the `Subsystem::Mount` idle event. UI: a Preferences switch + reorderable mount list, and a `GtkMenuButton` "Source" picker on `AlbumContentView` that re-queries with a `base "<folder_uri>"` filter when an alternate is selected.

**Tech Stack:** Rust 2024, GTK4 / libadwaita, GSettings, blocking `mpd` crate (forked, pinned in `Cargo.toml`), Meson + Cargo build, GResource templates.

**Spec:** `docs/superpowers/specs/2026-05-10-album-dedup-design.md`.

**Branch:** `feat/album-dedup` (already created from `origin/main`; spec already committed).

**Build & test cadence:** This repo has no automated test or lint targets (per `CLAUDE.md`). Each task ends with a `meson compile -C build` verification and, where applicable, a manual smoke step against a running MPD. The final task walks the manual test matrix from the spec.

---

## File Structure

| Path | Status | Responsibility |
|---|---|---|
| `data/io.github.htkhiem.Euphonica.gschema.xml` | modify | add `library.dedup-albums` (`b`) and `library.mount-priority` (`as`) |
| `src/common/album.rs` | modify | add `AlbumCopy` struct, `mount_name` + `alternates` on `AlbumInfo`, getters on `Album` |
| `src/client/mounts.rs` | create | `Mount`, `MountRegistry`, `classify`, `rank`, conversion from `mpd::Mount` |
| `src/client/mod.rs` | modify | re-export `mounts` |
| `src/client/connection.rs` | modify | add `Task::ListMounts` and its dispatch |
| `src/client/wrapper.rs` | modify | hold `MountRegistry`, refresh on connect/Mount idle, dedup pass in `get_albums_by_query`, `mounts()` accessor |
| `src/library/controller.rs` | modify | constrain `get_album_songs` and `queue_album` to a folder URI when one is given; settings-change handler that clears `*_initialized` flags |
| `src/library/album_content_view.rs` | modify | "Source" `GtkMenuButton`, alternate-copy refetch path, override folder URI on play/queue |
| `src/gtk/library/album-content-view.ui` | modify | add `source_button` template child |
| `src/preferences/library.rs` | modify | bind dedup switch, populate + drag-reorder mount list, refresh button |
| `src/gtk/preferences/library.ui` | modify | add "Library sources" `AdwPreferencesGroup` |

No new GResource files, so `src/euphonica.gresource.xml` is untouched.

---

## Task 1: Add GSettings keys for dedup toggle and mount priority

**Files:**
- Modify: `data/io.github.htkhiem.Euphonica.gschema.xml`

- [ ] **Step 1: Add the two keys to the `library` schema**

In `data/io.github.htkhiem.Euphonica.gschema.xml`, locate the schema with id `io.github.htkhiem.Euphonica.library` (around line 108) and add these two keys immediately before its closing `</schema>` (around line 142):

```xml
		<key name="dedup-albums" type="b">
			<default>true</default>
			<summary>Deduplicate album copies across MPD mounts</summary>
			<description>
			When more than one MPD mount holds the same album, hide all but
			one canonical copy. The canonical copy is picked by quality
			grade (HiRes &gt; CD &gt; Lossy &gt; Unknown), then by the
			user-defined mount priority list, then by lexicographic folder
			URI as a final tiebreaker. The non-canonical copies remain
			reachable through a source picker in the album detail view.
			</description>
		</key>
		<key name="mount-priority" type="as">
			<default>[]</default>
			<summary>Ordered list of MPD mount names used when picking canonical album copies</summary>
			<description>
			Mount names earlier in the list are preferred when picking
			between duplicate copies of the same album. Mounts not present
			in this list rank after every listed mount. The list is
			populated and reordered through the Library Preferences page.
			</description>
		</key>
```

- [ ] **Step 2: Compile gschemas to verify the schema is valid**

Run: `glib-compile-schemas --strict --dry-run data/`
Expected: no output, exit 0. (A typo in XML or duplicate key would print errors here before we touch the build system.)

- [ ] **Step 3: Run a full build to make sure resource generation still works**

Run: `meson compile -C build`
Expected: build succeeds; the gschema file is recompiled into `data/gschemas.compiled` as part of the resource step.

- [ ] **Step 4: Commit**

```bash
git add data/io.github.htkhiem.Euphonica.gschema.xml
git commit -m "feat(settings): add dedup-albums and mount-priority library keys"
```

---

## Task 2: Add `AlbumCopy` struct and dedup fields on `AlbumInfo` and `Album`

**Files:**
- Modify: `src/common/album.rs`

- [ ] **Step 1: Pull `QualityGrade` into the imports and add `AlbumCopy`**

In `src/common/album.rs`, ensure the `super::` import line at the top includes `QualityGrade` (it already does). Immediately after the `AlbumInfo` struct (before its `impl` block, i.e. before line 34 `impl AlbumInfo`), insert:

```rust
/// One copy of an album, identified by its album-level folder URI.
/// Populated by the dedup pass in `MpdWrapper::get_albums_by_query`.
#[derive(Debug, Clone, PartialEq)]
pub struct AlbumCopy {
    pub folder_uri: String,
    /// `None` when the URI is not under any registered mount.
    pub mount_name: Option<String>,
    pub quality_grade: QualityGrade,
}
```

- [ ] **Step 2: Add `mount_name` and `alternates` to `AlbumInfo`**

In the same file, modify the `AlbumInfo` struct (line 16 onward) to add two fields at the end:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct AlbumInfo {
    pub title: String,
    pub albumsort: Option<String>,
    pub example_uri: String,
    pub folder_uri: String,
    pub artists: Vec<ArtistInfo>,
    pub albumartist: Option<String>,
    pub albumartistsort: Option<String>,
    pub cover: Option<Texture>,
    pub release_date: Option<Date>,
    pub quality_grade: QualityGrade,
    pub mbid: Option<String>,
    /// Mount of the canonical copy. `None` when not under any registered mount,
    /// or when this AlbumInfo wasn't produced by the dedup pass.
    pub mount_name: Option<String>,
    /// Other copies of this album, ordered best-first. Always empty for
    /// AlbumInfos not produced by the dedup pass.
    pub alternates: Vec<AlbumCopy>,
}
```

- [ ] **Step 3: Update `AlbumInfo::new` and `Default` to set the new fields**

Replace `AlbumInfo::new` so it initializes the two new fields to their empty defaults:

```rust
impl AlbumInfo {
    pub fn new(
        example_uri: &str,
        title: &str,
        albumsort: Option<&str>,
        artist_tag: Option<&str>,
        albumartistsort: Option<&str>,
        artists: Vec<ArtistInfo>,
        quality_grade: QualityGrade,
    ) -> Self {
        Self {
            example_uri: example_uri.to_owned(),
            folder_uri: strip_filename_linux(example_uri).to_owned(),
            albumsort: albumsort.map(|s| s.to_owned()),
            artists,
            albumartist: artist_tag.map(str::to_owned),
            albumartistsort: albumartistsort.map(str::to_owned),
            title: title.to_owned(),
            cover: None,
            release_date: None,
            quality_grade,
            mbid: None,
            mount_name: None,
            alternates: Vec::new(),
        }
    }
    // ... (get_comp_id, add_artists_from_string, get_artist_str, get_artist_tag unchanged)
}
```

And `impl Default for AlbumInfo`:

```rust
impl Default for AlbumInfo {
    fn default() -> Self {
        AlbumInfo {
            title: "".to_owned(),
            albumsort: None,
            example_uri: "".to_owned(),
            folder_uri: "".to_owned(),
            artists: Vec::with_capacity(0),
            albumartist: None,
            albumartistsort: None,
            cover: None,
            release_date: None,
            quality_grade: QualityGrade::Unknown,
            mbid: None,
            mount_name: None,
            alternates: Vec::new(),
        }
    }
}
```

- [ ] **Step 4: Add getters on the `Album` GObject wrapper**

In the same file, inside `impl Album { ... }` (the block starting around line 209), add three new methods at the end of the impl, immediately before its closing `}`:

```rust
    pub fn get_mount_name(&self) -> Option<&str> {
        self.get_info().mount_name.as_deref()
    }

    pub fn get_alternates(&self) -> &[AlbumCopy] {
        &self.get_info().alternates
    }

    /// True when this album has at least one detected alternate copy.
    pub fn has_alternates(&self) -> bool {
        !self.get_info().alternates.is_empty()
    }
```

- [ ] **Step 5: Re-export `AlbumCopy` from `common::album`**

Open `src/common/mod.rs` and find the line that re-exports `Album` / `AlbumInfo` from this module. Add `AlbumCopy` to the same `pub use` line so other modules can name the type (without this the wrapper code in Task 6 won't compile):

```rust
pub use album::{Album, AlbumCopy, AlbumInfo};
```

(If the existing line is shaped slightly differently — e.g. multiple `pub use` items — preserve the existing layout but include `AlbumCopy` in the album re-export.)

- [ ] **Step 6: Compile**

Run: `meson compile -C build`
Expected: clean build. No warnings about unused fields (they're public, so the compiler won't flag them).

- [ ] **Step 7: Commit**

```bash
git add src/common/album.rs src/common/mod.rs
git commit -m "feat(common): add AlbumCopy and dedup fields on AlbumInfo"
```

---

## Task 3: Create `client::mounts` module

**Files:**
- Create: `src/client/mounts.rs`
- Modify: `src/client/mod.rs`

- [ ] **Step 1: Write the new module**

Create `src/client/mounts.rs` with this content:

```rust
//! MPD mount registry.
//!
//! Tracks the set of MPD storage mounts (last seen via `listmounts`) plus a
//! user-defined priority order from GSettings. `classify` maps an album folder
//! URI to its mount name (longest-prefix match) and `rank` returns an integer
//! ordering for canonical-copy selection.

use mpd::Mount as MpdMount;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub name: String,
    pub storage: String,
}

impl From<MpdMount> for Mount {
    fn from(m: MpdMount) -> Self {
        Self {
            name: m.name,
            storage: m.storage,
        }
    }
}

#[derive(Debug, Default)]
pub struct MountRegistry {
    /// Last seen via `listmounts`. Excludes the implicit root storage; we
    /// represent that as `mount_name == None`.
    known: Vec<Mount>,
    /// User-defined ordering of mount names. Mounts not in this list rank
    /// after every listed mount.
    priority: Vec<String>,
}

impl MountRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the known-mounts set. Drop the implicit root mount (name "")
    /// since it's the same as "URI not under any mount".
    pub fn set_known(&mut self, mounts: Vec<Mount>) {
        self.known = mounts.into_iter().filter(|m| !m.name.is_empty()).collect();
    }

    pub fn known(&self) -> &[Mount] {
        &self.known
    }

    /// Replace the user-defined priority list. Unknown names are kept (we
    /// don't prune here so users don't lose their ordering when a mount
    /// transiently disappears).
    pub fn set_priority(&mut self, priority: Vec<String>) {
        self.priority = priority;
    }

    pub fn priority(&self) -> &[String] {
        &self.priority
    }

    /// Return the mount name whose storage prefix is the longest match for
    /// `folder_uri`. MPD prefixes mounted-storage URIs with `<mount_name>/`
    /// (the mount name as named by the `mount` command), so we match on the
    /// first path component. Returns `None` when nothing matches (i.e. the
    /// URI lives in the implicit root storage).
    pub fn classify(&self, folder_uri: &str) -> Option<&str> {
        // The first path component of folder_uri.
        let first = folder_uri.split('/').next()?;
        if first.is_empty() {
            return None;
        }
        self.known
            .iter()
            .find(|m| m.name == first)
            .map(|m| m.name.as_str())
    }

    /// Lower number = better. Unknown mounts (including `None`) rank after
    /// every listed mount, all sharing `usize::MAX`.
    pub fn rank(&self, mount_name: Option<&str>) -> usize {
        match mount_name {
            Some(name) => self
                .priority
                .iter()
                .position(|n| n == name)
                .unwrap_or(usize::MAX),
            None => usize::MAX,
        }
    }
}
```

- [ ] **Step 2: Re-export from `client::mod.rs`**

Open `src/client/mod.rs` and add:

```rust
pub mod mounts;
```

next to the other `pub mod` declarations. Don't re-export individual types globally — call sites will use `crate::client::mounts::MountRegistry` etc.

- [ ] **Step 3: Compile**

Run: `meson compile -C build`
Expected: clean build. The module is unused so far; the compiler won't warn because `MountRegistry` and `Mount` are public.

- [ ] **Step 4: Commit**

```bash
git add src/client/mounts.rs src/client/mod.rs
git commit -m "feat(client): add MountRegistry for classifying URIs by MPD mount"
```

---

## Task 4: Add `Task::ListMounts` to the connection layer

**Files:**
- Modify: `src/client/connection.rs`

- [ ] **Step 1: Add the variant to `Task`**

In `src/client/connection.rs`, find the `pub enum Task { ... }` block (starts around line 168) and add this variant. Place it next to `Task::List` / `Task::Find` for consistency:

```rust
    /// List MPD storage mounts. Returns the mount list as seen by `listmounts`.
    ListMounts(Responder<Vec<crate::client::mounts::Mount>>),
```

- [ ] **Step 2: Wire the dispatch**

In the same file, find the `match task` block where every other `Task::*` variant is handled (around line 855). Add a new arm next to `Task::List(...)` (around line 1020):

```rust
                    Task::ListMounts(resp) => self.respond_with_client(
                        |c| {
                            c.mounts().map(|mounts| {
                                mounts.into_iter().map(crate::client::mounts::Mount::from).collect()
                            })
                        },
                        resp,
                    ),
```

- [ ] **Step 3: Compile**

Run: `meson compile -C build`
Expected: clean build. The new variant is unused for now; that's fine.

- [ ] **Step 4: Commit**

```bash
git add src/client/connection.rs
git commit -m "feat(client): add ListMounts task plumbing"
```

---

## Task 5: Hold `MountRegistry` in `MpdWrapper`, refresh on connect and Mount idle

**Files:**
- Modify: `src/client/wrapper.rs`

- [ ] **Step 1: Add the field, the priority loader, and the accessor**

In `src/client/wrapper.rs`, at the top of the file find the existing imports and add (next to the `crate::client::*` imports if any, otherwise alongside `super::`):

```rust
use super::mounts::MountRegistry;
```

In the `MpdWrapper` struct (around line 67), add a new field at the end:

```rust
    mount_registry: RefCell<MountRegistry>,
```

In `MpdWrapper::new` (around line 80) initialize it in the `Self { ... }` literal:

```rust
            mount_registry: RefCell::new(MountRegistry::new()),
```

Then add these public methods on the impl block, near the other accessors:

```rust
    /// Read-only borrow of the mount registry. Use this to classify URIs and
    /// rank mounts when consuming dedup-aware AlbumInfos.
    pub fn mounts(&self) -> std::cell::Ref<'_, MountRegistry> {
        self.mount_registry.borrow()
    }

    /// Re-run `listmounts` and reload the user's priority list from GSettings.
    /// Logs and returns the error on failure (the registry keeps whatever it
    /// last had so dedup keeps working with stale data).
    pub async fn refresh_mounts(&self) -> ClientResult<()> {
        let (s, r) = oneshot::channel();
        let mounts = self.foreground(Task::ListMounts(s), r).await?;
        let priority: Vec<String> = utils::settings_manager()
            .child("library")
            .strv("mount-priority")
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut reg = self.mount_registry.borrow_mut();
        reg.set_known(mounts);
        reg.set_priority(priority);
        Ok(())
    }

    /// Reload only the priority list from GSettings (cheap; no MPD round-trip).
    /// Call this when the user reorders the mount list in Preferences.
    pub fn reload_mount_priority(&self) {
        let priority: Vec<String> = utils::settings_manager()
            .child("library")
            .strv("mount-priority")
            .iter()
            .map(|s| s.to_string())
            .collect();
        self.mount_registry.borrow_mut().set_priority(priority);
    }
```

- [ ] **Step 2: Refresh on connect**

In `MpdWrapper::connect` (around line 280), at the very end of the function — after the line `self.state.set_connection_state(ConnectionState::Connected);` and before the final `Ok(())` — add:

```rust
        if let Err(e) = self.refresh_mounts().await {
            eprintln!("[mounts] listmounts failed on connect: {e:?}");
        }
```

This is best-effort: the spec says "log a warning, leave registry empty, fall back to inferring from URI prefix". `MountRegistry::classify` already infers from the first path component, so a failed refresh just leaves dedup with no priority info — still functional.

- [ ] **Step 3: Add the Mount idle handler**

Find `handle_idle_changes` (around line 169) and replace it with:

```rust
    async fn handle_idle_changes(&self, subsystem: Subsystem) {
        // Refresh subsystem-relevant local state BEFORE notifying observers,
        // so views that re-init on idle see the fresh data.
        if matches!(subsystem, Subsystem::Mount) {
            if let Err(e) = self.refresh_mounts().await {
                eprintln!("[mounts] refresh failed after Mount idle: {e:?}");
            }
        }
        self.state.emit_boxed_result("idle", subsystem);
        match subsystem {
            Subsystem::Database | Subsystem::Mount => {
                // Database changed (or the set of mounts changed, which can
                // make tracks appear/disappear). Reconnect to trigger a full
                // library refresh on the consumer side.
                let (s, r) = oneshot::channel();
                let _ = self.background(Task::Connect(s), r).await;
            }
            _ => {}
        }
    }
```

- [ ] **Step 4: Compile**

Run: `meson compile -C build`
Expected: clean build.

- [ ] **Step 5: Smoke test**

Start MPD locally (or against the user's existing instance) and run the app:

```bash
meson compile -C build && meson devenv -C build src/euphonica
```

Connect to an MPD server. Confirm there's no crash and no error printed about mounts on connect. (For an MPD without any mounts configured, the registry simply ends up empty — that's the single-mount-library case from the spec.)

- [ ] **Step 6: Commit**

```bash
git add src/client/wrapper.rs
git commit -m "feat(client): refresh MountRegistry on connect and Mount idle events"
```

---

## Task 6: Dedup pass in `MpdWrapper::get_albums_by_query`

**Files:**
- Modify: `src/client/wrapper.rs`

This is the largest task in the plan. It replaces the per-tuple `find Window::from((0, 1))` step with a wider find + bucket + partition + canonical-pick pass when the `library.dedup-albums` setting is on. When the setting is off, behavior matches today exactly.

- [ ] **Step 1: Add a private dedup helper**

Near the top of `wrapper.rs` (after imports, before the `MpdWrapper` struct), add a private free function. It is pure (no I/O) so it's easy to reason about:

```rust
use crate::common::{AlbumCopy, AlbumInfo};
use crate::common::song::QualityGrade;
use std::collections::BTreeMap;

/// Bucket a slice of songs (all from the same `(album, albumartist)` MPD tuple)
/// into match groups of duplicate copies, then pick a canonical AlbumInfo for
/// each group with alternates filled in. The returned Vec is one canonical
/// AlbumInfo per match group.
///
/// `rank_for_uri` returns `(quality_grade, mount_rank, folder_uri)` so the
/// caller can encapsulate the MountRegistry lookup. Lower mount_rank is better.
fn build_dedup_album_infos<F>(
    songs: Vec<crate::common::SongInfo>,
    rank_for_uri: F,
) -> Vec<AlbumInfo>
where
    F: Fn(&str) -> (QualityGrade, usize, Option<String>),
{
    use std::collections::HashMap;

    // Step 1: bucket songs by their parent folder URI (one bucket = one copy).
    let mut by_folder: HashMap<String, Vec<crate::common::SongInfo>> = HashMap::new();
    for s in songs {
        let folder = crate::utils::strip_filename_linux(&s.uri).to_string();
        by_folder.entry(folder).or_default().push(s);
    }

    // Step 2: merge multi-disc siblings under a common parent.
    // Two folder buckets are merged into one copy when:
    //   - their parent paths are equal (i.e. they're siblings), and
    //   - the disc-tag sets of their songs are disjoint and both non-empty.
    // We pick the lexicographically-first folder URI as the surviving key.
    {
        let keys: Vec<String> = by_folder.keys().cloned().collect();
        // Sort so the surviving key is deterministic.
        let mut keys_sorted = keys.clone();
        keys_sorted.sort();
        let mut absorbed: std::collections::HashSet<String> = Default::default();
        for i in 0..keys_sorted.len() {
            let a = &keys_sorted[i];
            if absorbed.contains(a) {
                continue;
            }
            for j in (i + 1)..keys_sorted.len() {
                let b = &keys_sorted[j];
                if absorbed.contains(b) {
                    continue;
                }
                if !is_sibling(a, b) {
                    continue;
                }
                let discs_a: std::collections::BTreeSet<i64> = by_folder[a]
                    .iter()
                    .filter_map(|s| s.disc)
                    .collect();
                let discs_b: std::collections::BTreeSet<i64> = by_folder[b]
                    .iter()
                    .filter_map(|s| s.disc)
                    .collect();
                if discs_a.is_empty() || discs_b.is_empty() {
                    continue;
                }
                if discs_a.is_disjoint(&discs_b) {
                    // Merge b into a.
                    let mut b_songs = by_folder.remove(b).unwrap_or_default();
                    by_folder.get_mut(a).unwrap().append(&mut b_songs);
                    absorbed.insert(b.clone());
                }
            }
        }
    }

    // Step 3: partition each copy by MBID. Within a (folder) bucket all songs
    // SHOULD share an MBID, but tag inconsistency happens. Choose the
    // most-common MBID for the bucket (or None if no song in the bucket has
    // one). Then group buckets by their representative MBID; buckets without
    // an MBID are pooled into one fallback group.
    #[derive(Debug)]
    struct CopyCandidate {
        folder_uri: String,
        rep_song: crate::common::SongInfo,
        mbid: Option<String>,
    }

    let copies: Vec<CopyCandidate> = by_folder
        .into_iter()
        .filter_map(|(folder_uri, mut songs)| {
            if songs.is_empty() {
                return None;
            }
            // Pick representative MBID = most common non-None MBID in bucket.
            let mut counts: HashMap<String, usize> = HashMap::new();
            for s in &songs {
                if let Some(album) = s.album.as_ref() {
                    if let Some(m) = album.mbid.as_ref() {
                        *counts.entry(m.clone()).or_default() += 1;
                    }
                }
            }
            let mbid = counts
                .into_iter()
                .max_by_key(|(_, n)| *n)
                .map(|(m, _)| m);
            // Pick representative song: first by track tag asc, falling back to URI.
            songs.sort_by_key(|s| (s.track.unwrap_or(i64::MAX), s.uri.clone()));
            let rep_song = songs.into_iter().next().unwrap();
            Some(CopyCandidate {
                folder_uri,
                rep_song,
                mbid,
            })
        })
        .collect();

    // Group key: Some(mbid) groups by exact MBID; None groups all
    // mbid-less copies together (fallback group).
    let mut groups: BTreeMap<Option<String>, Vec<CopyCandidate>> = BTreeMap::new();
    for c in copies {
        groups.entry(c.mbid.clone()).or_default().push(c);
    }

    // Step 4: pick canonical for each group.
    let mut out: Vec<AlbumInfo> = Vec::new();
    for (_mbid, mut group) in groups {
        if group.is_empty() {
            continue;
        }
        // Sort by (quality_grade desc, mount_rank asc, folder_uri asc).
        group.sort_by(|a, b| {
            let (qa, ra, _) = rank_for_uri(&a.folder_uri);
            let (qb, rb, _) = rank_for_uri(&b.folder_uri);
            qb.cmp(&qa) // desc on quality
                .then(ra.cmp(&rb)) // asc on mount rank (lower = better)
                .then(a.folder_uri.cmp(&b.folder_uri))
        });
        let canonical = group.remove(0);
        let mut info: AlbumInfo = canonical.rep_song.into_album_info().expect("rep song lacks album info");
        info.mount_name = rank_for_uri(&canonical.folder_uri).2;
        info.alternates = group
            .into_iter()
            .map(|c| {
                let (qg, _, mn) = rank_for_uri(&c.folder_uri);
                AlbumCopy {
                    folder_uri: c.folder_uri,
                    mount_name: mn,
                    quality_grade: qg,
                }
            })
            .collect();
        out.push(info);
    }

    // Stable order: by canonical folder_uri so views see consistent ordering.
    out.sort_by(|a, b| a.folder_uri.cmp(&b.folder_uri));
    out
}

/// True when `a` and `b` are siblings under a common parent folder URI.
/// e.g. "music/Album/Disc 1" and "music/Album/Disc 2" → true;
///       "music/Album"        and "music/Album/Disc 2" → false.
fn is_sibling(a: &str, b: &str) -> bool {
    let parent_a = a.rsplit_once('/').map(|(p, _)| p);
    let parent_b = b.rsplit_once('/').map(|(p, _)| p);
    match (parent_a, parent_b) {
        (Some(pa), Some(pb)) => pa == pb && a != b,
        _ => false,
    }
}
```

`QualityGrade` derives `Ord` already? Check before relying on it: the enum is declared with `#[derive(Clone, Copy, Debug, glib::Enum, PartialEq, Default)]` only. Add `Eq, PartialOrd, Ord` derives in `src/common/song.rs`:

```rust
#[derive(Clone, Copy, Debug, glib::Enum, PartialEq, Eq, PartialOrd, Ord, Default)]
#[enum_type(name = "EuphonicaQualityGrade")]
pub enum QualityGrade {
    #[default]
    Unknown,   // worst
    Lossy,
    CD,
    HiRes,
    DSD,       // best — keep variants in ascending quality order
}
```

The variant declaration order already happens to be ascending-quality (`Unknown < Lossy < CD < HiRes < DSD`), which is exactly what `derive(PartialOrd, Ord)` produces. The dedup helper sorts `qb.cmp(&qa)` for descending — meaning DSD wins, then HiRes, then CD, then Lossy, then Unknown. Match.

- [ ] **Step 2: Replace the body of `get_albums_by_query`**

Locate `pub async fn get_albums_by_query` (around line 843) and replace it with:

```rust
    pub async fn get_albums_by_query<F>(
        &self,
        query: Query<'static>,
        respond: &mut F,
    ) -> ClientResult<()>
    where
        F: FnMut(Album),
    {
        let dedup_on = utils::settings_manager().child("library").boolean("dedup-albums");

        // Get list of unique album tags, grouped by albumartist.
        let (s, r) = oneshot::channel();
        let grouped_vals = self
            .foreground(
                Task::List(
                    Term::Tag(Cow::Borrowed("album")),
                    query,
                    Some("albumartist"),
                    s,
                ),
                r,
            )
            .await?;

        for (key, tags) in grouped_vals.groups.into_iter() {
            for tag in tags.iter() {
                let mut q = Query::new();
                q.and(Term::Tag(Cow::Borrowed("album")), tag.to_string());
                q.and(Term::Tag(Cow::Borrowed("albumartist")), key.to_string());

                if !dedup_on {
                    // Legacy fast path: one song, one Album per tuple.
                    let (s, r) = oneshot::channel();
                    let mut songs = self
                        .foreground(Task::Find(q, Window::from((0, 1)), s), r)
                        .await?;
                    if !songs.is_empty() {
                        if let Some(info) = std::mem::take(&mut songs[0]).into_album_info() {
                            self.emit_album_with_stickers(info, respond).await;
                        } else {
                            println!("No album info found for {tag}");
                        }
                    }
                    continue;
                }

                // Dedup path: fetch all songs of this album-tuple, then bucket.
                let (s, r) = oneshot::channel();
                let songs = self
                    .foreground(Task::Find(q, Window::from((0, FETCH_LIMIT as u32)), s), r)
                    .await?;
                if songs.len() == FETCH_LIMIT {
                    eprintln!(
                        "[dedup] album {tag:?} hit FETCH_LIMIT ({FETCH_LIMIT}); dedup truncated"
                    );
                }
                if songs.is_empty() {
                    continue;
                }
                let infos = {
                    let mounts = self.mount_registry.borrow();
                    let rank_for_uri = |uri: &str| {
                        let mn = mounts.classify(uri).map(|s| s.to_owned());
                        let rank = mounts.rank(mn.as_deref());
                        // Quality is derived from the song's QualityGrade, which is
                        // already populated when SongInfo is parsed. We approximate
                        // the COPY's quality as the highest grade among its songs;
                        // build_dedup_album_infos receives songs not URIs, so we
                        // resolve here by URI: walk the by_folder bucket. Since we
                        // can't see that bucket from here, we instead compute
                        // copy-quality inline below and pass it via a trick: we
                        // keep a per-folder cache.
                        // (See note below.)
                        let _ = uri;
                        (QualityGrade::Unknown, rank, mn)
                    };
                    // The quality_grade of a copy must reflect the songs in that
                    // copy, not just any song. We compute that here once and pass
                    // a closure that returns it via a small map.
                    let mut quality_by_folder: std::collections::HashMap<String, QualityGrade> =
                        std::collections::HashMap::new();
                    for s in &songs {
                        let folder = crate::utils::strip_filename_linux(&s.uri).to_string();
                        let qg = s.get_quality_grade();
                        quality_by_folder
                            .entry(folder)
                            .and_modify(|cur| {
                                if qg > *cur {
                                    *cur = qg;
                                }
                            })
                            .or_insert(qg);
                    }
                    let rank_for_uri = move |uri: &str| -> (QualityGrade, usize, Option<String>) {
                        let folder = crate::utils::strip_filename_linux(uri).to_string();
                        let qg = *quality_by_folder.get(&folder).unwrap_or(&QualityGrade::Unknown);
                        let mn = mounts.classify(uri).map(|s| s.to_owned());
                        let rank = mounts.rank(mn.as_deref());
                        (qg, rank, mn)
                    };
                    build_dedup_album_infos(songs, rank_for_uri)
                };

                for info in infos {
                    self.emit_album_with_stickers(info, respond).await;
                }
            }
        }
        Ok(())
    }

    /// Wrap an AlbumInfo in an Album GObject, attach known stickers, and emit.
    async fn emit_album_with_stickers<F>(&self, info: AlbumInfo, respond: &mut F)
    where
        F: FnMut(Album),
    {
        let res: Album = info.into();
        let (s, r) = oneshot::channel();
        if let Ok(stickers) = self
            .foreground(
                Task::GetKnownStickers("album", res.get_title().to_owned(), s),
                r,
            )
            .await
        {
            res.set_stickers(stickers);
        }
        respond(res);
    }
```

Note: there's a quirk — we cannot hold `Ref<MountRegistry>` across an `await` (it's a `RefCell` borrow). The closure captures `mounts` by move from a freshly-bound borrow; **the entire dedup expression up through `build_dedup_album_infos` must run between `await` points, with the borrow dropped before the for-loop body's `await`.** That's exactly the shape above: the borrow is taken inside the inner `let infos = { ... };` block and dropped at the `}`, before `emit_album_with_stickers(.., ..).await`.

Make sure `SongInfo` has a public `get_quality_grade()` accessor — line 379 of `src/common/song.rs` confirms it does.

- [ ] **Step 3: Re-export `SongInfo::get_quality_grade` if necessary**

Confirm `pub fn get_quality_grade(&self) -> QualityGrade` exists on `SongInfo` in `src/common/song.rs:379`. If it's named differently, adjust the call. If it returns by-value (it does, `Copy` enum), the `*cur` pattern above is correct.

- [ ] **Step 4: Compile**

Run: `meson compile -C build`
Expected: clean build. Lifetime errors here would indicate a borrow held across an `await`; if so, restructure so the `mounts` borrow ends before the inner `for info in infos { ... emit_album_with_stickers(...).await }` loop.

- [ ] **Step 5: Manual smoke**

Run the app against an MPD server with at least one populated album.

- With `gsettings set io.github.htkhiem.Euphonica.library dedup-albums true`: the AlbumView should look the same as before (assuming no actual duplicates). No errors in the terminal.
- With `gsettings set io.github.htkhiem.Euphonica.library dedup-albums false`: again, AlbumView populates normally.

- [ ] **Step 6: Commit**

```bash
git add src/client/wrapper.rs src/common/song.rs
git commit -m "feat(client): dedup duplicate album copies in get_albums_by_query"
```

---

## Task 7: Settings change handler in `Library`

**Files:**
- Modify: `src/library/controller.rs`

When the user toggles `library.dedup-albums` or reorders `library.mount-priority`, the affected views need to re-init.

- [ ] **Step 1: Add a helper that clears album-view init flags**

In `src/library/controller.rs`, add this method on `impl Library` (anywhere alongside `clear`):

```rust
    /// Clear initialization flags for all views whose contents depend on
    /// dedup output. Next navigation to those views will re-fetch from MPD
    /// through the (now dedup-aware) wrapper.
    pub fn clear_dedup_dependent_initialized(&self) {
        let imp = self.imp();
        imp.albums_initialized.set(false);
        imp.recent_initialized.set(false);
        imp.albums.remove_all();
        imp.recent_albums.remove_all();
        // ArtistContentView and GenreContentView don't cache through these
        // flags — they re-fetch from the wrapper on every open via
        // get_album_artist_content / get_albums_by_genre, which now run
        // through the dedup pass automatically.
    }
```

- [ ] **Step 2: Wire the GSettings listener in `Library::setup`**

In `Library::setup` (around line 123), after the existing `let _ = self.imp().player.set(player);` line, add:

```rust
        let library_settings = utils::settings_manager().child("library");
        library_settings.connect_changed(
            Some("dedup-albums"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.clear_dedup_dependent_initialized();
                }
            ),
        );
        library_settings.connect_changed(
            Some("mount-priority"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    if let Some(client) = this.imp().client.get() {
                        client.reload_mount_priority();
                    }
                    this.clear_dedup_dependent_initialized();
                }
            ),
        );
```

(`utils::settings_manager` and `clone!` are already imported in this file; verify the imports include `glib::clone` — line 15 of `controller.rs` should already do this.)

- [ ] **Step 3: Compile**

Run: `meson compile -C build`
Expected: clean.

- [ ] **Step 4: Smoke**

Run the app, navigate to AlbumView. From a separate terminal:

```bash
gsettings set io.github.htkhiem.Euphonica.library dedup-albums false
```

Click another sidebar entry then back to Albums — the list should re-fetch (briefly show a loading state). Toggle back to `true`, repeat.

- [ ] **Step 5: Commit**

```bash
git add src/library/controller.rs
git commit -m "feat(library): re-init album views when dedup settings change"
```

---

## Task 8: Constrain `get_album_songs` and `queue_album` to a folder URI when given

**Files:**
- Modify: `src/library/controller.rs`

Today both methods query by `(album, albumartist[, mbid])`, which would pull songs from EVERY copy when duplicates exist. With dedup on, we want them to target a specific folder URI.

- [ ] **Step 1: Update `get_album_songs`**

Replace the body of `get_album_songs` (around line 158) with:

```rust
    pub async fn get_album_songs<F>(&self, album: &Album, respond: &mut F) -> ClientResult<()>
    where
        F: FnMut(Vec<Song>),
    {
        let mut query = Query::new();
        if let Some(mbid) = album.get_mbid() {
            query.and(Term::Tag(tags::ALBUM_MBID.into()), mbid.to_owned());
        } else {
            query.and(Term::Tag(tags::ALBUM.into()), album.get_title().to_owned());
            if let Some(albumartist) = album.get_artist_tag() {
                query.and(Term::Tag(tags::ALBUMARTIST.into()), albumartist.to_owned());
            }
        }
        // Dedup-aware: constrain to this Album's folder URI when dedup is on
        // OR when the Album has alternates. This avoids pulling songs from
        // duplicate copies into the song list.
        let dedup_on = utils::settings_manager()
            .child("library")
            .boolean("dedup-albums");
        if dedup_on || album.has_alternates() {
            let folder = album.get_folder_uri().to_owned();
            if !folder.is_empty() {
                query.and(Term::Base, folder);
            }
        }
        self.client().get_songs_by_query(query, true, respond).await
    }
```

- [ ] **Step 2: Update `queue_album`**

Replace the body of `queue_album` (around line 219):

```rust
    pub async fn queue_album(
        &self,
        album: Album,
        replace: bool,
        play: bool,
        play_from: Option<u32>,
    ) -> ClientResult<()> {
        let client = self.client();
        if replace {
            client.clear_queue().await?;
        }
        let mut query = Query::new();
        query.and(Term::Tag(tags::ALBUM.into()), album.get_title().to_owned());
        if let Some(artist) = album.get_artist_tag() {
            query.and(Term::Tag(tags::ALBUMARTIST.into()), artist.to_owned());
        }
        if let Some(mbid) = album.get_mbid() {
            query.and(Term::Tag(tags::ALBUM_MBID.into()), mbid.to_owned());
        }
        let dedup_on = utils::settings_manager()
            .child("library")
            .boolean("dedup-albums");
        if dedup_on || album.has_alternates() {
            let folder = album.get_folder_uri().to_owned();
            if !folder.is_empty() {
                query.and(Term::Base, folder);
            }
        }
        client.find_add(query).await?;
        if play {
            client.play_at(play_from.unwrap_or(0), false).await?;
        }
        Ok(())
    }
```

(`Term::Base` is already valid in the `mpd` fork — see `src/search.rs:18` and `:116` of the crate. The serialization writes `(base "<value>")` with no operator, exactly what MPD expects.)

- [ ] **Step 3: Compile**

Run: `meson compile -C build`
Expected: clean.

- [ ] **Step 4: Smoke**

With dedup on, click into an album and verify its song list still appears correctly (single-mount libraries should be unaffected). Use **Replace queue** and confirm the queue gets the same tracks.

- [ ] **Step 5: Commit**

```bash
git add src/library/controller.rs
git commit -m "feat(library): scope album song fetch and queue_album to canonical folder"
```

---

## Task 9: "Library sources" preferences group

**Files:**
- Modify: `src/gtk/preferences/library.ui`
- Modify: `src/preferences/library.rs`

- [ ] **Step 1: Add the preferences group to the UI template**

Open `src/gtk/preferences/library.ui`. Find the closing tag of the existing top-level page (after the last `</object>` of the last group, before the page's `</object>`) and insert a new `AdwPreferencesGroup`. Use the existing groups in the same file as a layout reference; below is a self-contained block:

```xml
<object class="AdwPreferencesGroup">
  <property name="title" translatable="yes">Library sources</property>
  <property name="description" translatable="yes">Behavior when MPD reports the same album under more than one mount.</property>
  <child>
    <object class="AdwSwitchRow" id="dedup_albums">
      <property name="title" translatable="yes">Deduplicate album copies</property>
      <property name="subtitle" translatable="yes">Hide duplicate copies of the same album when more than one MPD mount holds it.</property>
    </object>
  </child>
  <child>
    <object class="AdwActionRow" id="mount_priority_header">
      <property name="title" translatable="yes">Source priority</property>
      <property name="subtitle" translatable="yes">Drag to reorder. Mounts higher in the list win when copies tie on quality.</property>
      <child type="suffix">
        <object class="GtkButton" id="mount_priority_refresh">
          <property name="icon-name">view-refresh-symbolic</property>
          <property name="tooltip-text" translatable="yes">Re-query MPD for the current set of mounts</property>
          <property name="valign">center</property>
          <style>
            <class name="flat"/>
          </style>
        </object>
      </child>
    </object>
  </child>
  <child>
    <object class="GtkListBox" id="mount_priority_list">
      <property name="selection-mode">none</property>
      <style>
        <class name="boxed-list"/>
      </style>
    </object>
  </child>
</object>
```

- [ ] **Step 2: Add the matching template children to `LibraryPreferences`**

Open `src/preferences/library.rs`. In the `imp::LibraryPreferences` struct (around line 15) add:

```rust
        #[template_child]
        pub dedup_albums: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub mount_priority_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub mount_priority_refresh: TemplateChild<gtk::Button>,
```

- [ ] **Step 3: Bind the dedup switch and populate the mount list**

In `LibraryPreferences::setup` (around line 103), append:

```rust
        // Dedup switch
        library_settings
            .bind("dedup-albums", &imp.dedup_albums.get(), "active")
            .build();

        // Sensitivity: list and refresh button greyed when dedup is off.
        let dedup_row = imp.dedup_albums.get();
        let list = imp.mount_priority_list.get();
        let refresh_btn = imp.mount_priority_refresh.get();
        let sync_sensitivity = move |row: &adw::SwitchRow, list: &gtk::ListBox, btn: &gtk::Button| {
            let on = row.is_active();
            list.set_sensitive(on);
            btn.set_sensitive(on);
        };
        sync_sensitivity(&dedup_row, &list, &refresh_btn);
        dedup_row.connect_active_notify(clone!(
            #[weak]
            list,
            #[weak]
            refresh_btn,
            move |row| {
                let on = row.is_active();
                list.set_sensitive(on);
                refresh_btn.set_sensitive(on);
            }
        ));

        // Populate the mount list from the wrapper's MountRegistry.
        self.repopulate_mount_list();

        refresh_btn.connect_clicked(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                this.refresh_mount_list_async();
            }
        ));
```

- [ ] **Step 4: Implement `repopulate_mount_list` and `refresh_mount_list_async` on `LibraryPreferences`**

Below `setup` (still in `impl LibraryPreferences { ... }`), add:

```rust
    fn repopulate_mount_list(&self) {
        let imp = self.imp();
        let list = imp.mount_priority_list.get();
        // Clear
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let app = gio::Application::default()
            .and_then(|a| a.downcast::<crate::application::EuphonicaApplication>().ok());
        let Some(app) = app else { return };
        let client = app.get_client();
        let library_settings = utils::settings_manager().child("library");
        let priority: Vec<String> = library_settings
            .strv("mount-priority")
            .iter()
            .map(|s| s.to_string())
            .collect();
        let known: Vec<crate::client::mounts::Mount> = client.mounts().known().to_vec();
        if known.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No mounts detected")
                .subtitle("Only the root storage is in use.")
                .selectable(false)
                .build();
            list.append(&row);
            return;
        }
        // Order: known mounts in priority order first, then any not in priority.
        let mut ordered: Vec<crate::client::mounts::Mount> = Vec::new();
        for name in &priority {
            if let Some(m) = known.iter().find(|m| &m.name == name) {
                ordered.push(m.clone());
            }
        }
        for m in &known {
            if !ordered.iter().any(|o| o.name == m.name) {
                ordered.push(m.clone());
            }
        }
        for m in &ordered {
            let row = adw::ActionRow::builder()
                .title(&m.name)
                .subtitle(&m.storage)
                .build();
            // Drag handle
            let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
            handle.set_tooltip_text(Some("Drag to reorder"));
            row.add_prefix(&handle);
            list.append(&row);
        }
        self.wire_drag_reorder();
    }

    fn wire_drag_reorder(&self) {
        let list = self.imp().mount_priority_list.get();
        // GtkListBox supports drag-and-drop reordering via GtkDropTarget +
        // GtkDragSource on each row. Implement once: walk all rows, attach
        // sources/targets, and on drop write the new order to GSettings.
        let mut child = list.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            if let Some(row) = c.downcast_ref::<adw::ActionRow>() {
                attach_drag_source(row);
                attach_drop_target(row, &list);
            }
            child = next;
        }
    }

    fn refresh_mount_list_async(&self) {
        let app = gio::Application::default()
            .and_then(|a| a.downcast::<crate::application::EuphonicaApplication>().ok());
        let Some(app) = app else { return };
        let client = app.get_client();
        glib::spawn_future_local(clone!(
            #[weak(rename_to = this)]
            self,
            #[strong]
            client,
            async move {
                if let Err(e) = client.refresh_mounts().await {
                    eprintln!("[prefs] refresh_mounts failed: {e:?}");
                }
                this.repopulate_mount_list();
            }
        ));
    }
```

Add the two free helpers (drag plumbing is verbose; keep them at module bottom outside the impl):

```rust
fn attach_drag_source(row: &adw::ActionRow) {
    let drag = gtk::DragSource::new();
    drag.set_actions(gtk::gdk::DragAction::MOVE);
    drag.connect_prepare(clone!(
        #[weak]
        row,
        #[upgrade_or]
        None,
        move |_, _, _| {
            let title = row.title().to_string();
            Some(gtk::gdk::ContentProvider::for_value(&title.to_value()))
        }
    ));
    row.add_controller(drag);
}

fn attach_drop_target(row: &adw::ActionRow, list: &gtk::ListBox) {
    let drop = gtk::DropTarget::new(glib::types::Type::STRING, gtk::gdk::DragAction::MOVE);
    drop.connect_drop(clone!(
        #[weak]
        row,
        #[weak]
        list,
        #[upgrade_or]
        false,
        move |_, value, _, _| {
            let Ok(name) = value.get::<String>() else {
                return false;
            };
            // Find source row by title; remove it; insert before `row`.
            let mut src: Option<adw::ActionRow> = None;
            let mut child = list.first_child();
            while let Some(c) = child {
                let next = c.next_sibling();
                if let Some(r) = c.downcast_ref::<adw::ActionRow>() {
                    if r.title() == name {
                        src = Some(r.clone());
                        break;
                    }
                }
                child = next;
            }
            let Some(src) = src else { return false };
            if src == row {
                return false;
            }
            list.remove(&src);
            let target_idx = row.index();
            list.insert(&src, target_idx);
            // Persist new order.
            let new_priority = collect_mount_order(&list);
            let _ = utils::settings_manager()
                .child("library")
                .set_strv("mount-priority", &new_priority);
            true
        }
    ));
    row.add_controller(drop);
}

fn collect_mount_order(list: &gtk::ListBox) -> Vec<String> {
    let mut out = Vec::new();
    let mut child = list.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if let Some(r) = c.downcast_ref::<adw::ActionRow>() {
            out.push(r.title().to_string());
        }
        child = next;
    }
    out
}
```

(The cast `EuphonicaApplication` and its `get_client()` accessor are already in `src/application.rs`. If the existing code uses a slightly different accessor name — verify in `application.rs` — adjust the call accordingly.)

- [ ] **Step 5: Compile**

Run: `meson compile -C build`
Expected: clean. If GTK reports a missing template child, ensure the `id="…"` attributes in the `.ui` exactly match the field names.

- [ ] **Step 6: Smoke**

Open Preferences → Library. Confirm:

- "Library sources" group appears at the bottom.
- Toggle dedup off → list and refresh button grey out. Toggle on → they re-enable.
- The mount list shows whatever `listmounts` returned. With no mounts, the placeholder row appears.
- Drag a mount up; release; verify `gsettings get io.github.htkhiem.Euphonica.library mount-priority` reflects the new order.

- [ ] **Step 7: Commit**

```bash
git add src/gtk/preferences/library.ui src/preferences/library.rs
git commit -m "feat(prefs): add Library sources group with dedup toggle and mount priority"
```

---

## Task 10: Source picker on `AlbumContentView`

**Files:**
- Modify: `src/gtk/library/album-content-view.ui`
- Modify: `src/library/album_content_view.rs`

- [ ] **Step 1: Add the menu button to the template**

Open `src/gtk/library/album-content-view.ui`. Find the row of action buttons near the album header — the `replace_queue`, `queue_split_button`, and `add_to_playlist` buttons (the template children referenced in `imp::AlbumContentView`). Insert a new `GtkMenuButton` immediately before `replace_queue` so it sits leftmost in the action row:

```xml
<child>
  <object class="GtkMenuButton" id="source_button">
    <property name="visible">false</property>
    <property name="tooltip-text" translatable="yes">Switch between copies of this album</property>
    <child>
      <object class="AdwButtonContent" id="source_button_content">
        <property name="icon-name">drive-harddisk-symbolic</property>
        <property name="label" translatable="no"></property>
      </object>
    </child>
  </object>
</child>
```

- [ ] **Step 2: Add the matching template children**

In `src/library/album_content_view.rs`, in the `imp::AlbumContentView` struct (around line 32), add:

```rust
        #[template_child]
        pub source_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub source_button_content: TemplateChild<adw::ButtonContent>,
        pub active_copy_folder: RefCell<Option<String>>,
```

(`active_copy_folder` is the session-local override: `None` means "use canonical".)

- [ ] **Step 3: Populate the picker in `bind`**

In `pub fn bind(&self, album: &Album)` (around line 823), at the **start** of the function (right after `self.imp().on_selection_changed();`), add:

```rust
        self.imp().active_copy_folder.replace(None);
        self.populate_source_picker(album);
```

Then add the `populate_source_picker` method on `impl AlbumContentView` (anywhere in the public impl block):

```rust
    fn populate_source_picker(&self, album: &Album) {
        let imp = self.imp();
        let btn = imp.source_button.get();
        let content = imp.source_button_content.get();
        if !album.has_alternates() {
            btn.set_visible(false);
            return;
        }
        btn.set_visible(true);

        // Build a label for "current source".
        let label = source_label(
            album.get_mount_name(),
            album.get_quality_grade(),
            album.get_folder_uri(),
        );
        content.set_label(&label);

        // Build the menu model: canonical first, then each alternate.
        // We use a SimpleActionGroup attached to this view so each entry
        // can carry its `folder_uri` as the action target.
        let menu = Menu::new();
        let canonical_label = format!("\u{2713} {label}"); // U+2713 CHECK MARK
        let canonical_item = gio::MenuItem::new(Some(&canonical_label), None);
        canonical_item.set_action_and_target_value(
            Some("album.set-source"),
            Some(&album.get_folder_uri().to_variant()),
        );
        menu.append_item(&canonical_item);
        for alt in album.get_alternates() {
            let alt_label = source_label(
                alt.mount_name.as_deref(),
                alt.quality_grade,
                &alt.folder_uri,
            );
            let item = gio::MenuItem::new(Some(&alt_label), None);
            item.set_action_and_target_value(
                Some("album.set-source"),
                Some(&alt.folder_uri.to_variant()),
            );
            menu.append_item(&item);
        }
        btn.set_menu_model(Some(&menu));

        // Action group (one per AlbumContentView; replaced on each bind).
        let action = ActionEntry::builder("set-source")
            .parameter_type(Some(&String::static_variant_type()))
            .activate(clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _, param| {
                    let Some(folder) = param.and_then(|v| v.get::<String>()) else { return };
                    this.switch_to_copy(folder);
                }
            ))
            .build();
        let group = SimpleActionGroup::new();
        group.add_action_entries([action]);
        self.insert_action_group("album", Some(&group));
    }

    fn switch_to_copy(&self, folder_uri: String) {
        let imp = self.imp();
        let Some(album) = imp.album.borrow().clone() else { return };
        // Same canonical → clear override, refresh.
        let canonical = album.get_folder_uri().to_owned();
        let new_override = if folder_uri == canonical {
            None
        } else {
            Some(folder_uri.clone())
        };
        imp.active_copy_folder.replace(new_override.clone());

        // Update label.
        let (label_mount, label_q) = if new_override.is_none() {
            (album.get_mount_name().map(|s| s.to_owned()), album.get_quality_grade())
        } else if let Some(alt) = album.get_alternates().iter().find(|a| a.folder_uri == folder_uri) {
            (alt.mount_name.clone(), alt.quality_grade)
        } else {
            (None, album.get_quality_grade())
        };
        imp.source_button_content.set_label(&source_label(
            label_mount.as_deref(),
            label_q,
            &folder_uri,
        ));

        // Re-fetch the song list constrained to the chosen folder.
        let library = imp.library.upgrade();
        let Some(library) = library else { return };
        let song_list = imp.song_list.clone();
        song_list.remove_all();
        let stack = imp.content_stack.get();
        stack.show_spinner();
        glib::spawn_future_local(clone!(
            #[weak(rename_to = this)]
            self,
            #[strong]
            album,
            #[strong]
            folder_uri,
            async move {
                let res = this
                    .imp()
                    .library
                    .upgrade()
                    .unwrap()
                    .get_album_songs_at(&album, &folder_uri, &mut |songs| {
                        this.imp().song_list.extend_from_slice(&songs);
                    })
                    .await;
                let stack = this.imp().content_stack.get();
                match res {
                    Ok(()) => {
                        if this.imp().song_list.n_items() > 0 {
                            stack.show_content();
                        } else {
                            stack.show_placeholder();
                        }
                    }
                    Err(e) => {
                        eprintln!("[album] switch_to_copy failed: {e:?}");
                    }
                }
                this.imp().runtime.set_label(&format_secs_as_duration(
                    this.imp().song_list
                        .iter()
                        .map(|item: Result<Song, _>| {
                            if let Ok(song) = item {
                                return song.get_duration();
                            }
                            0
                        })
                        .sum::<u64>() as f64,
                ));
                let _ = library;
            }
        ));
    }
```

Add the `source_label` free helper at the end of the file, after the `glib::wrapper!` block:

```rust
fn source_label(mount: Option<&str>, quality: crate::common::song::QualityGrade, folder_uri: &str) -> String {
    let mount_str = match mount {
        Some(m) => m.to_owned(),
        None => folder_uri
            .split('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("Root")
            .to_owned(),
    };
    let q = match quality {
        crate::common::song::QualityGrade::DSD => "DSD",
        crate::common::song::QualityGrade::HiRes => "Hi-Res",
        crate::common::song::QualityGrade::CD => "CD",
        crate::common::song::QualityGrade::Lossy => "Lossy",
        crate::common::song::QualityGrade::Unknown => "?",
    };
    format!("{mount_str} \u{00B7} {q}")
}
```

- [ ] **Step 4: Add `Library::get_album_songs_at`**

In `src/library/controller.rs`, immediately after `get_album_songs`, add:

```rust
    /// Like `get_album_songs` but constrained to a specific folder URI.
    /// Used by AlbumContentView when the user picks a non-canonical copy.
    pub async fn get_album_songs_at<F>(
        &self,
        album: &Album,
        folder_uri: &str,
        respond: &mut F,
    ) -> ClientResult<()>
    where
        F: FnMut(Vec<Song>),
    {
        let mut query = Query::new();
        if let Some(mbid) = album.get_mbid() {
            query.and(Term::Tag(tags::ALBUM_MBID.into()), mbid.to_owned());
        } else {
            query.and(Term::Tag(tags::ALBUM.into()), album.get_title().to_owned());
            if let Some(albumartist) = album.get_artist_tag() {
                query.and(Term::Tag(tags::ALBUMARTIST.into()), albumartist.to_owned());
            }
        }
        if !folder_uri.is_empty() {
            query.and(Term::Base, folder_uri.to_owned());
        }
        self.client().get_songs_by_query(query, true, respond).await
    }
```

- [ ] **Step 5: Honour the active copy override on play/queue**

In `album_content_view.rs`, find the existing call sites that invoke `library.queue_album(album.clone(), ..)` (lines 611, 650, 776 per the earlier grep). For each one, replace the `album.clone()` argument with a freshly-cloned Album whose `folder_uri` reflects the active copy. The cleanest way: wrap the override in a local helper at the top of `impl AlbumContentView`:

```rust
    /// If the user has selected a non-canonical copy, return a clone of
    /// `album` rebuilt with that folder URI on its AlbumInfo so any
    /// dedup-aware Library method targets the right copy. Otherwise return
    /// `album` unchanged.
    fn album_for_action(&self, album: &Album) -> Album {
        let folder = match self.imp().active_copy_folder.borrow().as_ref() {
            Some(f) => f.clone(),
            None => return album.clone(),
        };
        let mut info = album.get_info().clone();
        info.folder_uri = folder;
        // Clear alternates so further dedup-aware code doesn't re-mux.
        info.alternates = Vec::new();
        Album::from(info)
    }
```

Now at each `library.queue_album(album.clone(), ...)` site, replace `album.clone()` with `this.album_for_action(&album)` (where `this` is the `#[weak(rename_to = this)]` of the AlbumContentView in the surrounding `clone!` block — the existing handlers already have access to `this`).

- [ ] **Step 6: Compile**

Run: `meson compile -C build`
Expected: clean. Watch out for `Album::clone()` not being defined — it is, since `Album` is a `glib::wrapper!` GObject; `.clone()` produces a new Rust handle on the same underlying GObject. `Album::from(info)` wraps a fresh `AlbumInfo` in a new GObject (already used elsewhere in this codebase per `album.rs:299`).

- [ ] **Step 7: Smoke**

With dedup on:

- An album with no alternates: source button stays hidden (visual regression check).
- An album with alternates (set up via the Task-11 test fixture): source button visible, label shows mount + quality, picker lists every copy. Selecting an alternate refreshes the song list. Clicking **Replace queue** queues from the chosen alternate (verify via the queue view — URIs should fall under the chosen folder).
- Source switching does NOT affect the currently-playing track.

- [ ] **Step 8: Commit**

```bash
git add src/gtk/library/album-content-view.ui src/library/album_content_view.rs src/library/controller.rs
git commit -m "feat(library): source picker for albums with multiple copies"
```

---

## Task 11: Run the manual test matrix from the spec

**Files:** none — verification only.

This is the validation gate. Walk every scenario in §"Test plan" of `docs/superpowers/specs/2026-05-10-album-dedup-design.md`. Each item below corresponds to a numbered scenario in the spec.

- [ ] **Step 1: Two-mount synthetic setup**

On a scratch MPD configuration, create two storage mounts pointing at copies of the same small album (one MBID-tagged, one untagged). Start Euphonica.

- Expected with dedup on, both MBID-tagged: ONE entry in AlbumView. Open it; source picker shows two entries.
- Expected with dedup on, mixed MBID-tagging: TWO entries.
- Expected with dedup off: TWO entries regardless.

- [ ] **Step 2: Mount priority reorder**

In Preferences → Library → Library sources, drag the slower mount above the faster one. Re-enter AlbumContentView for the dedup test album. Canonical should flip to match the new order; song list refreshes.

- [ ] **Step 3: Quality tiebreak**

Configure the test album so one copy is FLAC and the other is MP3, with the FLAC copy on the lower-priority mount. Confirm the FLAC copy wins as canonical (quality precedes mount priority).

- [ ] **Step 4: Multi-disc heuristic**

Use a real multi-disc album with `Disc 1/`, `Disc 2/` siblings under a common parent, all tagged with the same MBID. Confirm: ONE entry in AlbumView, song list contains tracks from BOTH disc folders, source picker has NO alternate listed.

- [ ] **Step 5: Live mount add/remove**

```bash
mpc mount fixture-mount file:///tmp/fixture
mpc unmount fixture-mount
```

While Euphonica is open. Confirm: idle event triggers reload; Preferences mount list updates after re-opening the page; AlbumView re-fetches.

- [ ] **Step 6: Settings toggle at runtime**

```bash
gsettings set io.github.htkhiem.Euphonica.library dedup-albums false
gsettings set io.github.htkhiem.Euphonica.library dedup-albums true
```

Re-open AlbumView, ArtistContentView (an artist with a deduped album), GenreContentView (a genre with a deduped album), RecentView. All should re-init with the new setting in effect.

- [ ] **Step 7: Single-mount library**

Connect to an MPD instance with no mounts configured (the most common case). Confirm: AlbumView looks identical to pre-feature behavior. Source picker NEVER appears. No errors on connect.

- [ ] **Step 8: Final commit**

If any of the above scenarios required a fix, commit it with a `fix(dedup):` prefix. Otherwise nothing to commit at this step.

```bash
git status
# (Expect: nothing to commit, working tree clean.)
```

---

## Self-Review

**Spec coverage:**

- §"Match key" (MBID, mixed-tagging) → Task 6 (`build_dedup_album_infos` partition step).
- §"Canonical-pick algorithm" → Task 6 sort key `(qb.cmp(&qa), ra.cmp(&rb), folder_uri.cmp)`.
- §"Data model: AlbumCopy / AlbumInfo additions" → Task 2.
- §"Data model: MountRegistry" → Task 3.
- §"Data model: GSettings additions" → Task 1.
- §"Control flow: dedup runs in MpdWrapper" → Task 6.
- §"Control flow: idle integration" → Task 5.
- §"Control flow: settings reload" → Task 7.
- §"Performance" → Task 6 uses `Window::from((0, FETCH_LIMIT as u32))`, same number of round-trips.
- §"UI: Preferences group" → Task 9.
- §"UI: source picker" → Task 10.
- §"What we are not adding" → no task; covered by absence.
- §"Error handling: listmounts fails" → Task 5 best-effort + classify-by-prefix fallback already present in Task 3.
- §"Error handling: stale priority entries" → Task 3 (`set_priority` keeps unknown names; pruning happens implicitly because `repopulate_mount_list` rebuilds from the intersection of `priority` and `known`).
- §"Error handling: FETCH_LIMIT truncation" → Task 6 prints a warning when the result hits the cap.
- §"Edge case: multi-disc" → Task 6 `is_sibling` + disjoint-discs merge.
- §"Edge case: idle mid-listing" → Task 5 reuses MpdWrapper's existing serialization; no new code required.
- §"Edge case: switching alt while playing" → Task 10's `switch_to_copy` only re-points future actions; nothing touches the current queue position.
- §"Test plan" → Task 11.

No spec gaps.

**Placeholder scan:**

- No "TBD"/"TODO"/"implement later" tokens in any task.
- All steps that change code show the actual code.
- All commands have expected output described.

**Type consistency:**

- `MountRegistry::classify` returns `Option<&str>`; consumers in Task 6 and Task 9 call `.map(|s| s.to_owned())`. ✓
- `Album::has_alternates` (Task 2), `Album::get_alternates` (Task 2), used in Tasks 8 and 10. ✓
- `MpdWrapper::mounts() -> Ref<'_, MountRegistry>` (Task 5), used in Tasks 6, 9. ✓
- `MpdWrapper::refresh_mounts()`, `MpdWrapper::reload_mount_priority()` (Task 5), used in Tasks 7, 9. ✓
- `Library::clear_dedup_dependent_initialized()` (Task 7) and `Library::get_album_songs_at()` (Task 10) are both new and used by callers in the same task or later tasks. ✓
- `QualityGrade` is `Ord` after Task 6 Step 1. Task 10's `source_label` does not need ordering (only matching). ✓

Everything checks out.
