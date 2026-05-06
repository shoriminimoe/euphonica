# Shuffle queue (with by-album mode) — design

**Date:** 2026-05-07
**Status:** approved (pending implementation)

## Goal

Add a one-shot **Shuffle queue** action with two modes — uniform track shuffle and **shuffle by album** (group by album, shuffle group order, preserve in-album track order). Triggered from a new `AdwSplitButton` in the queue view's toolbar. The selected mode persists across app restarts via GSettings.

This is distinct from the existing playback-controls "shuffle" toggle (`random_btn`), which is in fact a toggle for MPD's `random` playback mode and not a one-shot queue rewrite. The two stay independent.

## User-visible behaviour

- New `AdwSplitButton` named `shuffle_btn` in the queue view's toolbar (next to the existing edit menu). Icon: `media-playlist-shuffle-symbolic`. Tooltip reflects the current mode: `"Shuffle queue · tracks"` in tracks mode, `"Shuffle queue · by album"` in album mode.
- **Main button** click applies the currently-selected shuffle mode to the queue.
- **Dropdown menu** with two radio-checkable entries:
  - **Shuffle tracks** — uniform random shuffle (default).
  - **Shuffle by album** — group by album, shuffle the group order, keep in-album track order.
- Selecting an item in the dropdown updates the persisted mode and the tooltip; the next main-button click applies that mode.

### Currently-playing track

Both modes preserve the **current cluster** at the head of the queue:

- If a track is playing, identify its album cluster — the contiguous run of queue items sharing the same `Album::get_comp_id()` that includes the playing position. Extend forward from the playing position while the next item's comp_id matches; let `cluster_end` be the last such position (inclusive).
- Define `boundary = cluster_end + 1`. The portion `queue[0..boundary]` is **untouched**; only `queue[boundary..]` (the "tail") gets shuffled.
- If nothing is playing, `boundary = 0` and the entire queue is the tail.

The currently-playing album therefore finishes in its natural order; only what plays AFTER the current album is randomized.

### The two algorithms

**Shuffle tracks** — uniform shuffle of the tail. MPD's native `shuffle START:END` command does this server-side in one call (where `START` is the boundary position and `END` is the queue length). Use it directly via a new `MpdWrapper::shuffle_range` wrapper.

**Shuffle by album** — done client-side because MPD has no native "shuffle by album":

1. Walk `queue[boundary..]` to build a `Vec<Group>` where each `Group` is a contiguous run of queue items sharing the same `Album::get_comp_id()`. Tracks with no album metadata each form a one-track group (their unique fallback id is per-song, so they're naturally distinct).
2. Shuffle the `Vec<Group>` order using `rand::seq::SliceRandom::shuffle`. Track order within each group is unchanged.
3. Flatten back to a `Vec<String>` of URIs representing the new sequence at `queue[boundary..]`.
4. Apply by calling MPD `delete START:END` (for `START..END = boundary..queue_len`) followed by `push_multiple` (which mpd-rs already implements as a command list of `addid` calls). Two server round-trips total. A subtle race window exists between the two commands where MPD's queue holds only the unmodified head — but the currently-playing track is in the head and isn't going anywhere, so this is benign in practice. (`moveid`-based in-place reordering would avoid the gap entirely but requires a fork extension to mpd-rs because `Id` doesn't currently implement `ToQueueRangeOrPlace`; not worth the complexity.)

### Same album appearing twice

Each contiguous run is its own group. If the user added album X, then album Y, then album X again, the two X clusters are distinct groups. Treating them as one would require non-local merging and would surprise users who placed them deliberately.

### Tracks with no album metadata

Per the brainstorm decision: each is its own one-track group. They get scattered through the album shuffle individually. This maps cleanly to the existing `Album::get_comp_id()` fallback (folder URI when no MBID is present); tracks without an album resolve to per-song unique ids and are guaranteed to be distinct from each other.

## Architecture

### gschema additions

In `data/io.github.htkhiem.Euphonica.gschema.xml`:

New top-level enum:

```xml
<enum id="io.github.htkhiem.Euphonica.shufflemode">
    <value nick="tracks" value="0"/>
    <value nick="album" value="1"/>
</enum>
```

New key inside the existing `state.queueview` schema:

```xml
<key name="shuffle-mode" enum="io.github.htkhiem.Euphonica.shufflemode">
    <default>'tracks'</default>
    <summary>Last-selected mode for the queue's Shuffle button</summary>
</key>
```

### `Player` controller

A new public `ShuffleMode` enum (in `player/controller.rs`):

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ShuffleMode {
    Tracks,
    Album,
}
```

A new public async method:

```rust
pub async fn shuffle_queue(&self, mode: ShuffleMode) -> ClientResult<()>
```

Internally:

1. Read the current queue from the player's queue ListStore (already maintained on the player as `imp.queue`).
2. Determine the boundary:
   - If `current_song()` returns `None` or the queue length is 0, boundary = 0.
   - Otherwise, find the position of the currently-playing song in the queue via its song-id; let that be `cur_pos`. Walk forward from `cur_pos` while consecutive items share the same `Album::get_comp_id()`. The last such position is `cluster_end`. Boundary = `cluster_end + 1`.
3. If `boundary >= queue_len`, return early — there's nothing to shuffle.
4. Dispatch:
   - `ShuffleMode::Tracks`: `client.shuffle_range(boundary).await`.
   - `ShuffleMode::Album`: build groups, shuffle them, flatten to `Vec<String>` of URIs, then `client.delete_range(boundary, queue_len).await` followed by `client.add_multi(uris, false, None).await`.

### `MpdWrapper` (client/wrapper.rs)

Two new wrappers, both routing to existing mpd-rs methods:

```rust
/// MPD `shuffle START:` — server-side shuffle of positions [start, queue_len).
pub async fn shuffle_range(&self, start: u32) -> ClientResult<()>;

/// MPD `delete START:END` — server-side delete of a queue range.
pub async fn delete_range(&self, start: u32, end: u32) -> ClientResult<()>;
```

Corresponding `Task` variants in `client/connection.rs`:

```rust
ShuffleRange(u32, Responder<()>),
DeleteRange(u32, u32, Responder<()>),
```

The dispatch arms call mpd-rs's `Client::shuffle(start..)` and `Client::delete(start..end)` respectively. Album mode reuses the existing `add_multi(uris, false, None)` after `delete_range` to repopulate the tail.

### UI changes

`gtk/player/queue-view.ui`:

- Add an `AdwSplitButton id="shuffle_btn"` to the toolbar (specifically into the same `<child type="end">` region or alongside, depending on the existing toolbar layout — `queue-view.ui` line ~45 has the existing `edit_menu`).
- The split button's `menu-model` references a new `<menu id="shuffle_menu_model">` with two `<item>` entries bound to a stateful action `queue-view.shuffle-mode` (string state, values `"tracks"` and `"album"`).
- The `<icon-name>` is `media-playlist-shuffle-symbolic`.

`src/player/queue_view.rs`:

- New `shuffle_btn: TemplateChild<adw::SplitButton>` field.
- Register a stateful `gio::SimpleAction` `shuffle-mode` (string-typed) on the queue-view's existing action group. Initial state from GSettings; activation persists the new state and updates the tooltip.
- Bind GSettings `state.queueview.shuffle-mode` ⇌ action state two-way (via the standard pattern used elsewhere in the codebase for sortable views).
- Wire the split button's main `clicked` signal to `player.shuffle_queue(current_mode).await` (spawned via `glib::spawn_future_local`).
- Bind the split button's `sensitive` property to `player.queue-len > 0` (same pattern used for other queue-mutation actions).

## Edge cases

- **Empty queue** — split button is insensitive (bound to `queue-len > 0`).
- **Queue with one track / single album** — shuffle-by-album is identity. Shuffle-tracks shuffles in place but with one track is also identity. No-ops are silent — no toast.
- **Boundary at end of queue** (currently playing track is in the last album cluster, no tail) — return early. Nothing to shuffle.
- **Currently-playing track with no album metadata** — the cluster has length 1 (just the current track). Boundary = current_pos + 1. The rest gets shuffled normally.
- **`current_song` returns Some but its position can't be located in the queue ListStore** (race during queue updates) — fall back to boundary = 0 (shuffle everything). Conservatively safe.
- **Same album split across non-contiguous queue runs** — treated as separate groups (per brainstorm decision). The shuffle-by-album operation respects what the user added.
- **Random mode** — orthogonal. Whether MPD's `random` is on/off has no bearing on this operation. Both can coexist.
- **Consume mode** — orthogonal. Doesn't change the operation; consumed-after-play applies to whatever ends up next.
- **`saved_to_history`** — currently-playing track stays at its preserved position, so the history-recording logic in `Player::on_status_update` (the 50%/240s threshold) is unaffected.
- **Multi-disc albums** — preserved automatically. In-album track order respects disc/track tags as the queue already represents; we only reorder groups, never tracks within.
- **Large queues (1000+ tracks)** — by-album path uses one MPD command list with up to N `moveid` ops. Latency scales linearly but is a single network round-trip. Tracks-path is server-native and constant-time client-side.

## Manual test plan

1. **Empty queue** — split button insensitive. No-op confirmed visually.
2. **Single song queue** — split button sensitive (queue_len > 0); main click is a no-op (boundary = 1, tail empty).
3. **Multiple albums, nothing playing** — click main in tracks mode → queue fully shuffled. Click in album mode → albums clustered, album order randomized, in-album order preserved.
4. **Playing track 3 of album A; queue is A, B, C, D** — shuffle-by-album: tracks 1-N of A stay; B, C, D appear after A in some random order. Playback uninterrupted.
5. **Playing last track of album A; queue is A, B, C, D** — boundary = end of A. Same as #4 — B, C, D get shuffled.
6. **Single album fills the whole queue** — by-album shuffle = identity. Tracks shuffle does shuffle within.
7. **Tracks with no album** — interspersed with albumed tracks → singles each scatter independently in by-album mode.
8. **Same album twice** — by-album shuffle treats them as separate groups.
9. **Mode persistence** — pick "by album" via the dropdown, quit and relaunch — the dropdown still shows "by album" checked, and main-click runs by-album. Repeat with "tracks".
10. **Tooltip update** — switching modes via the dropdown immediately updates the button's tooltip.
11. **Random mode interaction** — toggle MPD random mode on, then run shuffle-by-album. Queue is reordered as expected; playback continues randomly per MPD's mode (queue order may not be the play order, but the visible queue is correctly reorganized).

## Files

**Modified files only — no new files.**

| Path | Change |
|---|---|
| `data/io.github.htkhiem.Euphonica.gschema.xml` | Add `shufflemode` enum (top-level, alongside the other enums); add `shuffle-mode` key to `state.queueview` schema. |
| `src/client/wrapper.rs` | Add `shuffle_range` and `delete_range` methods. |
| `src/client/connection.rs` | Add `Task::ShuffleRange` and `Task::DeleteRange` variants + dispatch arms in the task loop. |
| `src/player/controller.rs` | Add `ShuffleMode` enum + `pub async fn shuffle_queue(&self, mode: ShuffleMode) -> ClientResult<()>` with the boundary/group/dispatch logic. |
| `src/player/queue_view.rs` | New `shuffle_btn` template child, stateful `shuffle-mode` action, GSettings binding, click handler that calls `player.shuffle_queue(...)`, sensitivity binding, tooltip update. |
| `src/gtk/player/queue-view.ui` | New `AdwSplitButton id="shuffle_btn"` + `shuffle_menu_model` menu definition with two radio-style items. |

## Out of scope

- Shuffling a saved (MPD-side) playlist on disk. The playlist file is unchanged; only the queue gets reordered.
- An "always shuffle by album when loading" persistent option (could be v2).
- Shuffling a multi-select subset of the queue. The operation is whole-queue (with the current-cluster preservation rule).
- Undo / re-shuffle history. Once shuffled, the previous order is unrecoverable without a snapshot the user didn't request.
- A "Stop after current album" or similar related queue-affordance. Out of scope but might be a natural follow-up.
