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
